#!/usr/bin/env python
"""Apply anonymization actions from a config file and write a new table plus a report."""
from __future__ import annotations

import argparse
import json
import random
import re
from typing import Any

import pandas as pd

from common import load_dataframe, load_structured_file, parse_sheet_name, refuse_to_overwrite, write_dataframe, write_json

REDACTIONS = [
    ("[EMAIL]", re.compile(r"[\w.+-]+@[\w-]+\.[\w.-]+")),
    ("[URL]", re.compile(r"https?://\S+", re.I)),
    ("[NUMBER]", re.compile(r"\+?\d[\d\s().-]{5,}\d")),
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_path", help="Path to the raw table. It is never modified.")
    parser.add_argument("--actions", required=True, help="JSON or YAML file describing anonymization actions.")
    parser.add_argument("--output", required=True, help="Path to write the anonymized table.")
    parser.add_argument("--report", required=True, help="Path to write the anonymization report JSON.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    parser.add_argument("--key-file", help="Optional path for the pseudonym key (original to code). Keep it outside any shared folder.")
    return parser.parse_args()


def _columns(action: dict[str, Any], df: pd.DataFrame) -> list[str]:
    columns = action.get("columns") or [action["column"]]
    missing = [c for c in columns if c not in df.columns]
    if missing:
        raise SystemExit(f"Action {action['type']}: columns not found: {missing}")
    return columns


def _changed(before: pd.Series, after: pd.Series) -> int:
    return int((before.astype("string").fillna("") != after.astype("string").fillna("")).sum())


def drop(df: pd.DataFrame, action: dict[str, Any], keys: dict) -> tuple[pd.DataFrame, dict]:
    columns = _columns(action, df)
    return df.drop(columns=columns), {"columns_dropped": columns}


def pseudonymize(df: pd.DataFrame, action: dict[str, Any], keys: dict) -> tuple[pd.DataFrame, dict]:
    """Replace each distinct value with PREFIX_0001... Codes are assigned in random order,
    so they reveal nothing about the original order of the file."""
    column = _columns(action, df)[0]
    prefix = action.get("prefix") or re.sub(r"\W+", "_", column).upper()
    originals = sorted(df[column].dropna().astype(str).unique())
    random.SystemRandom().shuffle(originals)
    width = max(4, len(str(len(originals))))
    mapping = {value: f"{prefix}_{i:0{width}d}" for i, value in enumerate(originals, start=1)}
    keys[column] = mapping
    out = df.copy()
    out[column] = out[column].map(lambda v: mapping[str(v)] if pd.notna(v) else v)
    return out, {"column": column, "distinct_values_replaced": len(mapping)}


def round_coordinates(df: pd.DataFrame, action: dict[str, Any], keys: dict) -> tuple[pd.DataFrame, dict]:
    decimals = int(action.get("decimals", 1))
    out, cells = df.copy(), 0
    for column in _columns(action, df):
        rounded = pd.to_numeric(out[column], errors="coerce").round(decimals)
        cells += _changed(out[column], rounded.astype("string"))
        out[column] = rounded
    return out, {"decimals": decimals, "cells_changed": cells}


def generalize_date(df: pd.DataFrame, action: dict[str, Any], keys: dict) -> tuple[pd.DataFrame, dict]:
    column, to = _columns(action, df)[0], action.get("to", "year")
    if to not in {"year", "month"}:
        raise SystemExit("generalize_date: 'to' must be 'year' or 'month'.")
    dates = pd.to_datetime(df[column], errors="coerce", dayfirst=bool(action.get("dayfirst", False)))
    out = df.copy()
    out[column] = dates.dt.strftime("%Y" if to == "year" else "%Y-%m")
    return out, {"column": column, "to": to, "unparsed_set_to_missing": int((dates.isna() & df[column].notna()).sum())}


def bin_numeric(df: pd.DataFrame, action: dict[str, Any], keys: dict) -> tuple[pd.DataFrame, dict]:
    column, width = _columns(action, df)[0], float(action["width"])
    numbers = pd.to_numeric(df[column], errors="coerce")
    lower = (numbers // width) * width
    label = lambda x: f"{x:g}"  # noqa: E731
    out = df.copy()
    out[column] = [f"{label(lo)}-{label(lo + width - (1 if width >= 1 else 0))}" if pd.notna(lo) else pd.NA for lo in lower]
    return out, {"column": column, "width": width, "bands": int(lower.nunique())}


def top_code(df: pd.DataFrame, action: dict[str, Any], keys: dict) -> tuple[pd.DataFrame, dict]:
    column = _columns(action, df)[0]
    numbers = pd.to_numeric(df[column], errors="coerce")
    upper, lower = action.get("upper"), action.get("lower")
    out = df.copy()
    out[column] = numbers.clip(lower=lower, upper=upper)
    capped = (int((numbers > upper).sum()) if upper is not None else 0) + (int((numbers < lower).sum()) if lower is not None else 0)
    return out, {"column": column, "upper": upper, "lower": lower, "values_capped": capped}


def keep_top_categories(df: pd.DataFrame, action: dict[str, Any], keys: dict) -> tuple[pd.DataFrame, dict]:
    column, other = _columns(action, df)[0], action.get("other", "Other")
    counts = df[column].value_counts()
    keep = set(counts[counts >= int(action.get("min_count", 10))].index)
    if "top_n" in action:
        keep &= set(counts.head(int(action["top_n"])).index)
    out = df.copy()
    merge = out[column].notna() & ~out[column].isin(keep)
    out.loc[merge, column] = other
    return out, {"column": column, "categories_kept": len(keep), "categories_merged": int(len(counts) - len(keep)), "rows_merged": int(merge.sum())}


def redact_text(df: pd.DataFrame, action: dict[str, Any], keys: dict) -> tuple[pd.DataFrame, dict]:
    column = _columns(action, df)[0]

    def redact(value: Any) -> Any:
        if pd.isna(value):
            return value
        text = str(value)
        for label, pattern in REDACTIONS:
            text = pattern.sub(label, text)
        return text

    out = df.copy()
    out[column] = out[column].map(redact)
    return out, {"column": column, "cells_changed": _changed(df[column], out[column]), "note": "Patterns only: names in free text are not removed."}


def suppress_small_groups(df: pd.DataFrame, action: dict[str, Any], keys: dict) -> tuple[pd.DataFrame, dict]:
    columns, k = _columns(action, df), int(action.get("k", 5))
    sizes = df.groupby(columns, dropna=False)[columns[0]].transform("size")
    out = df[sizes >= k].copy()
    return out, {"columns": columns, "k": k, "rows_removed": int(len(df) - len(out)), "rows_removed_pct": round((len(df) - len(out)) / max(len(df), 1), 4)}


ACTIONS = {f.__name__: f for f in (drop, pseudonymize, round_coordinates, generalize_date, bin_numeric, top_code, keep_top_categories, redact_text, suppress_small_groups)}


def main() -> None:
    args = parse_args()
    refuse_to_overwrite(args.input_path, args.output)
    df = load_dataframe(args.input_path, sheet_name=parse_sheet_name(args.sheet_name))
    config = load_structured_file(args.actions)
    actions = config["actions"] if isinstance(config, dict) else config

    keys: dict[str, dict[str, str]] = {}
    out, log = df, []
    for action in actions:
        if action.get("type") not in ACTIONS:
            raise SystemExit(f"Unsupported action type: {action.get('type')}. Supported: {sorted(ACTIONS)}")
        out, detail = ACTIONS[action["type"]](out, action, keys)
        log.append({"type": action["type"], **detail})

    write_dataframe(out, args.output)
    if args.key_file and keys:
        write_json(keys, args.key_file)

    report = {
        "input_path": args.input_path,
        "output_path": args.output,
        "rows_before": len(df),
        "rows_after": len(out),
        "columns_before": len(df.columns),
        "columns_after": len(out.columns),
        "actions": log,
        "pseudonym_key_written": bool(args.key_file and keys),
        "note": "Pseudonymized data is still personal data. A data steward must review this output before it is shared.",
    }
    write_json(report, args.report)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()

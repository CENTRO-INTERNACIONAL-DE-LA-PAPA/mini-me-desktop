#!/usr/bin/env python
from __future__ import annotations

import argparse
import re
from pathlib import Path
from typing import Any

import pandas as pd

from common import (
    DEFAULT_MISSING_MARKERS,
    load_dataframe,
    load_structured_file,
    normalize_missing_markers,
    to_jsonable,
    write_dataframe,
    write_json,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Apply deterministic pandas cleaning actions from a config file.")
    parser.add_argument("input_path", help="Path to the input table.")
    parser.add_argument("--actions", required=True, help="JSON or YAML file describing cleaning actions.")
    parser.add_argument("--output", required=True, help="Path to write the cleaned table.")
    parser.add_argument("--report", required=True, help="Path to write the cleaning report JSON.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    return parser.parse_args()


def _row_selector(df: pd.DataFrame, action: dict[str, Any]) -> pd.Series:
    column = action["column"]
    op = action["op"]
    value = action.get("value")
    series = df[column]

    if op == "eq":
        return series == value
    if op == "ne":
        return series != value
    if op == "lt":
        return series < value
    if op == "le":
        return series <= value
    if op == "gt":
        return series > value
    if op == "ge":
        return series >= value
    if op == "in":
        return series.isin(value)
    if op == "not_in":
        return ~series.isin(value)
    if op == "isna":
        return series.isna()
    if op == "notna":
        return series.notna()

    raise ValueError(f"Unsupported row filter op: {op}")


def _cast_series(series: pd.Series, dtype: str, errors: str) -> pd.Series:
    if dtype.lower() in {"int", "int64", "float", "float64", "int32", "float32", "numeric"}:
        return pd.to_numeric(series, errors=errors)
    if dtype in {"datetime", "datetime64[ns]"}:
        return pd.to_datetime(series, errors=errors)
    return series.astype(dtype)


def apply_action(df: pd.DataFrame, action: dict[str, Any]) -> tuple[pd.DataFrame, dict[str, Any]]:
    action_type = action["type"]
    updated = df.copy()
    summary: dict[str, Any] = {"type": action_type}

    if action_type == "normalize_missing":
        columns = action.get("columns")
        markers = action.get("markers", DEFAULT_MISSING_MARKERS)
        updated, replacements = normalize_missing_markers(updated, columns=columns, markers=markers)
        summary["columns"] = columns or list(df.columns)
        summary["affected_values"] = replacements
        return updated, summary

    if action_type == "strip_whitespace":
        columns = action["columns"]
        changed = 0
        for column in columns:
            before = updated[column].copy()
            updated[column] = updated[column].astype("string").str.strip()
            changed += int((before.fillna("<NA>") != updated[column].fillna("<NA>")).sum())
        summary["columns"] = columns
        summary["affected_values"] = changed
        return updated, summary

    if action_type in {"lowercase", "uppercase", "titlecase"}:
        columns = action["columns"]
        changed = 0
        for column in columns:
            before = updated[column].copy()
            text = updated[column].astype("string")
            if action_type == "lowercase":
                updated[column] = text.str.lower()
            elif action_type == "uppercase":
                updated[column] = text.str.upper()
            else:
                updated[column] = text.str.title()
            changed += int((before.fillna("<NA>") != updated[column].fillna("<NA>")).sum())
        summary["columns"] = columns
        summary["affected_values"] = changed
        return updated, summary

    if action_type == "cast":
        columns = action["columns"]
        dtype = action["dtype"]
        errors = action.get("errors", "raise")
        for column in columns:
            updated[column] = _cast_series(updated[column], dtype=dtype, errors=errors)
        summary["columns"] = columns
        summary["dtype"] = dtype
        return updated, summary

    if action_type == "map_values":
        column = action["column"]
        mapping = action["mapping"]
        affected = int(updated[column].isin(mapping.keys()).sum())
        updated[column] = updated[column].replace(mapping)
        summary["column"] = column
        summary["affected_values"] = affected
        return updated, summary

    if action_type == "replace_regex":
        column = action["column"]
        pattern = action["pattern"]
        replacement = action.get("replacement", "")
        flags = re.IGNORECASE if action.get("ignore_case", False) else 0
        matches = int(
            updated[column]
            .astype("string")
            .str.contains(pattern, regex=True, flags=flags, na=False)
            .sum()
        )
        updated[column] = updated[column].astype("string").str.replace(
            pattern,
            replacement,
            regex=True,
            flags=flags,
        )
        summary["column"] = column
        summary["affected_values"] = matches
        return updated, summary

    if action_type == "drop_duplicates":
        subset = action.get("subset")
        keep = action.get("keep", "first")
        before = len(updated)
        updated = updated.drop_duplicates(subset=subset, keep=keep)
        summary["subset"] = subset
        summary["rows_removed"] = before - len(updated)
        return updated, summary

    if action_type == "drop_rows_where":
        mask = _row_selector(updated, action)
        removed = int(mask.sum())
        updated = updated.loc[~mask].copy()
        summary["rows_removed"] = removed
        summary["column"] = action["column"]
        summary["op"] = action["op"]
        summary["value"] = action.get("value")
        return updated, summary

    if action_type == "clip":
        column = action["column"]
        lower = action.get("lower")
        upper = action.get("upper")
        series = pd.to_numeric(updated[column], errors="coerce")
        affected = int(((series < lower) if lower is not None else False).sum()) if lower is not None else 0
        affected += int(((series > upper) if upper is not None else False).sum()) if upper is not None else 0
        updated[column] = series.clip(lower=lower, upper=upper)
        summary["column"] = column
        summary["affected_values"] = affected
        summary["lower"] = lower
        summary["upper"] = upper
        return updated, summary

    if action_type == "fillna":
        columns = action["columns"]
        value = action.get("value")
        affected = int(updated[columns].isna().sum().sum())
        updated[columns] = updated[columns].fillna(value)
        summary["columns"] = columns
        summary["affected_values"] = affected
        summary["value"] = value
        return updated, summary

    if action_type == "rename_columns":
        mapping = action["mapping"]
        updated = updated.rename(columns=mapping)
        summary["mapping"] = mapping
        return updated, summary

    if action_type == "sort_values":
        by = action["by"]
        ascending = action.get("ascending", True)
        updated = updated.sort_values(by=by, ascending=ascending).reset_index(drop=True)
        summary["by"] = by
        summary["ascending"] = ascending
        return updated, summary

    raise ValueError(f"Unsupported cleaning action type: {action_type}")


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    config = load_structured_file(args.actions)
    actions = config.get("actions", [])

    if not isinstance(actions, list):
        raise ValueError("Actions config must define a top-level 'actions' list.")

    working = df.copy()
    action_summaries: list[dict[str, Any]] = []
    for action in actions:
        working, summary = apply_action(working, action)
        action_summaries.append(summary)

    write_dataframe(working, args.output)
    report = {
        "input_path": args.input_path,
        "output_path": args.output,
        "rows_before": int(len(df)),
        "rows_after": int(len(working)),
        "columns_before": list(df.columns),
        "columns_after": list(working.columns),
        "actions_applied": to_jsonable(action_summaries),
    }
    write_json(report, args.report)
    print(f"Wrote cleaned dataset to {args.output}")
    print(f"Wrote cleaning report to {args.report}")


if __name__ == "__main__":
    main()

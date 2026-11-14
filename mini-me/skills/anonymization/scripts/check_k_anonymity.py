#!/usr/bin/env python
"""Measure k-anonymity over a set of quasi-identifiers, and l-diversity for a sensitive column."""
from __future__ import annotations

import argparse
import json

from common import load_dataframe, parse_sheet_name, write_json


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_path", help="Path to the table to measure.")
    parser.add_argument("--quasi", action="append", required=True, help="Quasi-identifier column. Repeat for each one.")
    parser.add_argument("--sensitive", help="Sensitive column to measure l-diversity on.")
    parser.add_argument("--k", type=int, default=5, help="Threshold a group must reach (default 5).")
    parser.add_argument("--output", required=True, help="Path to write the risk report JSON.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    df = load_dataframe(args.input_path, sheet_name=parse_sheet_name(args.sheet_name))
    missing = [c for c in args.quasi if c not in df.columns]
    if missing:
        raise SystemExit(f"Columns not found: {missing}")

    groups = df.groupby(args.quasi, dropna=False)
    sizes = groups.size()
    below = sizes[sizes < args.k]
    report = {
        "input_path": args.input_path,
        "n_rows": len(df),
        "quasi_identifiers": args.quasi,
        "distinct_values": {column: int(df[column].nunique(dropna=False)) for column in args.quasi},
        "k_threshold": args.k,
        "k": int(sizes.min()) if len(sizes) else 0,
        "groups": int(len(sizes)),
        "unique_records": int((sizes == 1).sum()),
        "groups_below_k": int(len(below)),
        "rows_below_k": int(below.sum()),
        "rows_below_k_pct": round(float(below.sum()) / max(len(df), 1), 4),
        "passes": bool(len(sizes) and sizes.min() >= args.k),
    }
    if args.sensitive:
        diversity = groups[args.sensitive].nunique(dropna=False)
        report["l_diversity"] = {"sensitive_column": args.sensitive, "l": int(diversity.min()), "groups_with_one_value": int((diversity == 1).sum())}

    write_json(report, args.output)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()

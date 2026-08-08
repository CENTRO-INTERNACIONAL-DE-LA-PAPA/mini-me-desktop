#!/usr/bin/env python
from __future__ import annotations

import argparse
import json
from pathlib import Path

import pointblank as pb

from common import load_dataframe, missingness_by_column, to_jsonable, write_json


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Profile a tabular dataset with pointblank DataScan.")
    parser.add_argument("input_path", help="Path to the input table.")
    parser.add_argument("--output", required=True, help="Path to write the profile JSON report.")
    parser.add_argument("--table-name", help="Optional logical table name for reports.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    parser.add_argument(
        "--key",
        action="append",
        default=[],
        help="Key column to assess for duplicate key combinations. Repeat for composite keys.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    table_name = args.table_name or Path(args.input_path).stem
    scan = pb.DataScan(df, tbl_name=table_name)

    duplicate_key_rows = 0
    duplicate_key_pct = 0.0
    if args.key:
        duplicate_key_rows = int(df.duplicated(subset=args.key, keep=False).sum())
        duplicate_key_pct = duplicate_key_rows / max(len(df), 1)

    report = {
        "input_path": args.input_path,
        "table_name": table_name,
        "n_rows": int(len(df)),
        "n_columns": int(df.shape[1]),
        "columns": list(df.columns),
        "dtypes": {column: str(dtype) for column, dtype in df.dtypes.items()},
        "missingness_by_column": missingness_by_column(df),
        "duplicate_rows": {
            "count": int(df.duplicated().sum()),
            "pct": float(df.duplicated().sum() / max(len(df), 1)),
        },
        "duplicate_keys": {
            "columns": args.key,
            "count": duplicate_key_rows,
            "pct": duplicate_key_pct,
        },
        "datascan_summary": to_jsonable(scan.summary_data),
        "datascan_report": json.loads(scan.to_json()),
    }
    write_json(report, args.output)
    print(f"Wrote dataset profile to {args.output}")


if __name__ == "__main__":
    main()

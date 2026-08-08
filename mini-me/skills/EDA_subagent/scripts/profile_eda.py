#!/usr/bin/env python
from __future__ import annotations

import argparse
import json
from pathlib import Path

import pandas as pd
import pointblank as pb

from common import (
    ensure_directory,
    infer_column_groups,
    load_dataframe,
    maybe_parse_datetimes,
    missingness_summary,
    safe_name,
    save_current_figure,
    setup_plotting,
    to_jsonable,
    write_json,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Create an EDA profile for a tabular dataset.")
    parser.add_argument("input_path", help="Path to the input table.")
    parser.add_argument("--output", required=True, help="Path to the output JSON profile.")
    parser.add_argument("--plot-dir", required=True, help="Directory where plots will be written.")
    parser.add_argument("--table-name", help="Optional logical table name.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    parser.add_argument("--skip-missingno", action="store_true", help="Skip optional missingno plots.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    df = maybe_parse_datetimes(df)
    table_name = args.table_name or Path(args.input_path).stem
    plot_dir = ensure_directory(args.plot_dir)

    setup_plotting()

    plot_paths: list[str] = []
    try:
        import missingno as msno

        if not args.skip_missingno and len(df.columns) > 0:
            msno.matrix(df)
            matrix_path = plot_dir / f"{safe_name(table_name)}_missingno_matrix.png"
            save_current_figure(matrix_path)
            plot_paths.append(str(matrix_path))

            msno.bar(df)
            bar_path = plot_dir / f"{safe_name(table_name)}_missingno_bar.png"
            save_current_figure(bar_path)
            plot_paths.append(str(bar_path))
    except ImportError:
        pass

    scan = pb.DataScan(df, tbl_name=table_name)
    groups = infer_column_groups(df)
    profile = {
        "input_path": args.input_path,
        "table_name": table_name,
        "n_rows": int(len(df)),
        "n_columns": int(df.shape[1]),
        "column_groups": groups,
        "dtypes": {column: str(dtype) for column, dtype in df.dtypes.items()},
        "missingness_by_column": missingness_summary(df),
        "duplicate_rows": {
            "count": int(df.duplicated().sum()),
            "pct": float(df.duplicated().sum() / max(len(df), 1)),
        },
        "sample_records": to_jsonable(df.head(10)),
        "datascan_summary": to_jsonable(scan.summary_data),
        "datascan_report": json.loads(scan.to_json()),
        "plots": plot_paths,
    }
    write_json(profile, args.output)
    print(f"Wrote EDA profile to {args.output}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python
from __future__ import annotations

import argparse
from pathlib import Path

import pandas as pd

from common import load_dataframe, to_jsonable, write_dataframe, write_json


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate join keys and join effects between two tables.")
    parser.add_argument("left_path", help="Path to the left table.")
    parser.add_argument("right_path", help="Path to the right table.")
    parser.add_argument("--output", required=True, help="Path to write the join validation JSON report.")
    parser.add_argument("--left-on", nargs="+", required=True, help="Left join key columns.")
    parser.add_argument("--right-on", nargs="+", required=True, help="Right join key columns.")
    parser.add_argument("--how", default="left", choices=["left", "inner", "right", "outer"], help="Join type to simulate.")
    parser.add_argument("--joined-output", help="Optional path to materialize the joined table.")
    return parser.parse_args()


def _null_summary(df: pd.DataFrame, columns: list[str]) -> dict[str, float]:
    denominator = max(len(df), 1)
    return {column: float(df[column].isna().sum() / denominator) for column in columns}


def main() -> None:
    args = parse_args()
    left = load_dataframe(args.left_path)
    right = load_dataframe(args.right_path)

    joined = left.merge(
        right,
        how=args.how,
        left_on=args.left_on,
        right_on=args.right_on,
        indicator=True,
        suffixes=("_left", "_right"),
    )

    left_only = int((joined["_merge"] == "left_only").sum())
    right_only = int((joined["_merge"] == "right_only").sum())
    both = int((joined["_merge"] == "both").sum())

    right_nonkey_columns = [column for column in right.columns if column not in args.right_on]
    nulls_before = _null_summary(right, right_nonkey_columns) if right_nonkey_columns else {}
    nulls_after = _null_summary(joined, right_nonkey_columns) if right_nonkey_columns else {}
    null_inflation = {
        column: float(nulls_after[column] - nulls_before[column])
        for column in right_nonkey_columns
    }

    report = {
        "left_path": args.left_path,
        "right_path": args.right_path,
        "how": args.how,
        "left_on": args.left_on,
        "right_on": args.right_on,
        "left_rows": int(len(left)),
        "right_rows": int(len(right)),
        "joined_rows": int(len(joined)),
        "left_key_null_rows": int(left[args.left_on].isna().any(axis=1).sum()),
        "right_key_null_rows": int(right[args.right_on].isna().any(axis=1).sum()),
        "left_duplicate_key_rows": int(left.duplicated(subset=args.left_on, keep=False).sum()),
        "right_duplicate_key_rows": int(right.duplicated(subset=args.right_on, keep=False).sum()),
        "match_counts": {
            "both": both,
            "left_only": left_only,
            "right_only": right_only,
        },
        "right_side_null_inflation": to_jsonable(null_inflation),
    }
    write_json(report, args.output)

    if args.joined_output:
        output_df = joined.drop(columns=["_merge"])
        write_dataframe(output_df, args.joined_output)

    print(f"Wrote join validation report to {args.output}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import pointblank as pb

from common import load_dataframe, load_structured_file, to_jsonable, write_json


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run pointblank validations against a tabular dataset.")
    parser.add_argument("input_path", help="Path to the input table.")
    parser.add_argument("--output", required=True, help="Path to write the validation JSON report.")
    parser.add_argument("--rules", help="JSON or YAML rules file describing pointblank validation steps.")
    parser.add_argument("--table-name", help="Optional logical table name for reports.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    parser.add_argument(
        "--key",
        action="append",
        default=[],
        help="Key column to enforce as non-null and distinct. Repeat for composite keys.",
    )
    parser.add_argument(
        "--not-null",
        action="append",
        default=[],
        help="Column that must be non-null. Repeat as needed.",
    )
    return parser.parse_args()


def _schema_from_value(value: Any) -> Any:
    if isinstance(value, (dict, list, tuple)):
        return pb.Schema(value)
    return value


def _build_validator(df, table_name: str, rules: dict[str, Any] | None, keys: list[str], not_null: list[str]) -> pb.Validate:
    validate_kwargs = {"tbl_name": table_name}
    if rules and isinstance(rules.get("validate_kwargs"), dict):
        validate_kwargs.update(rules["validate_kwargs"])

    validator = pb.Validate(df, **validate_kwargs)

    if keys:
        validator = validator.col_vals_not_null(columns=keys)
        validator = validator.rows_distinct(columns_subset=keys)

    if not_null:
        validator = validator.col_vals_not_null(columns=not_null)

    if not rules:
        return validator

    checks = rules.get("checks", [])
    if not isinstance(checks, list):
        raise ValueError("Rules file must define a top-level 'checks' list.")

    for check in checks:
        if not isinstance(check, dict):
            raise ValueError("Each validation check must be an object.")

        method_name = check.get("method")
        if not method_name or not hasattr(validator, method_name):
            raise ValueError(f"Unknown pointblank validation method: {method_name}")

        kwargs = dict(check.get("kwargs", {}))
        if method_name == "col_schema_match" and "schema" in kwargs:
            kwargs["schema"] = _schema_from_value(kwargs["schema"])

        validator = getattr(validator, method_name)(**kwargs)

    return validator


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    table_name = args.table_name or Path(args.input_path).stem
    rules = load_structured_file(args.rules) if args.rules else None

    validator = _build_validator(df, table_name, rules, args.key, args.not_null)
    validator = validator.interrogate()

    report = {
        "input_path": args.input_path,
        "table_name": table_name,
        "all_passed": bool(validator.all_passed()),
        "summary": {
            "n": to_jsonable(validator.n),
            "n_passed": to_jsonable(validator.n_passed),
            "n_failed": to_jsonable(validator.n_failed),
            "f_passed": to_jsonable(validator.f_passed),
            "f_failed": to_jsonable(validator.f_failed),
        },
        "checks": json.loads(validator.get_json_report()),
    }
    write_json(report, args.output)
    print(f"Wrote validation report to {args.output}")


if __name__ == "__main__":
    main()

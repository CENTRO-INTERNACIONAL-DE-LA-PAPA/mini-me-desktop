#!/usr/bin/env python
from __future__ import annotations

import argparse

import matplotlib.pyplot as plt
import pandas as pd

from common import (
    coefficient_table,
    ensure_directory,
    fit_regression,
    load_dataframe,
    maybe_parse_datetimes,
    safe_name,
    save_current_figure,
    setup_plotting,
    to_jsonable,
    write_json,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Check how a focal estimate changes after adding controls.")
    parser.add_argument("input_path")
    parser.add_argument("--outcome", required=True)
    parser.add_argument("--focal-predictor", required=True)
    parser.add_argument("--control", action="append", default=[])
    parser.add_argument("--categorical", action="append", default=[])
    parser.add_argument("--stratify-by")
    parser.add_argument("--output", required=True)
    parser.add_argument("--plot-dir", required=True)
    parser.add_argument("--sheet-name")
    return parser.parse_args()


def _extract_term(table: pd.DataFrame, term: str) -> dict[str, object] | None:
    matches = table[table["term"] == term]
    if matches.empty:
        prefix_matches = table[table["term"].str.startswith(f"{term}_")]
        if prefix_matches.empty:
            return None
        return prefix_matches.iloc[0].to_dict()
    return matches.iloc[0].to_dict()


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    df = maybe_parse_datetimes(df)

    base_type, base_result, base_frame, _, _ = fit_regression(
        df,
        args.outcome,
        [args.focal_predictor],
        categorical=args.categorical or None,
    )
    adj_type, adj_result, adj_frame, _, _ = fit_regression(
        df,
        args.outcome,
        [args.focal_predictor, *args.control],
        categorical=args.categorical or None,
    )

    base_table = coefficient_table(base_result)
    adj_table = coefficient_table(adj_result)
    base_term = _extract_term(base_table, args.focal_predictor)
    adj_term = _extract_term(adj_table, args.focal_predictor)

    pct_change = None
    if base_term and adj_term and float(base_term["coef"]) != 0:
        pct_change = float((float(adj_term["coef"]) - float(base_term["coef"])) / float(base_term["coef"]))

    missingness = (
        df[[args.outcome, args.focal_predictor, *args.control]]
        .isna()
        .mean()
        .rename("missing_pct")
        .reset_index()
        .rename(columns={"index": "column"})
    )

    subgroup_summary = None
    if args.stratify_by and args.stratify_by in df.columns:
        subgroup_summary = (
            df[[args.stratify_by, args.outcome, args.focal_predictor]]
            .dropna()
            .groupby(args.stratify_by)
            .agg(
                n=(args.outcome, "size"),
                outcome_mean=(args.outcome, "mean"),
                focal_mean=(args.focal_predictor, "mean"),
            )
            .reset_index()
        )

    setup_plotting()
    plot_dir = ensure_directory(args.plot_dir)
    comparison_df = pd.DataFrame(
        [
            {
                "model": "unadjusted",
                "coef": base_term["coef"] if base_term else None,
                "ci_low": base_term["ci_low"] if base_term else None,
                "ci_high": base_term["ci_high"] if base_term else None,
            },
            {
                "model": "adjusted",
                "coef": adj_term["coef"] if adj_term else None,
                "ci_low": adj_term["ci_low"] if adj_term else None,
                "ci_high": adj_term["ci_high"] if adj_term else None,
            },
        ]
    ).dropna()

    plots: list[str] = []
    if not comparison_df.empty:
        plt.figure(figsize=(6, 4))
        plt.errorbar(
            x=comparison_df["coef"],
            y=comparison_df["model"],
            xerr=[
                comparison_df["coef"] - comparison_df["ci_low"],
                comparison_df["ci_high"] - comparison_df["coef"],
            ],
            fmt="o",
            color="#f58518",
        )
        plt.axvline(0.0, color="black", linewidth=1, linestyle="--")
        plt.title(f"Focal estimate: {args.focal_predictor}")
        coef_plot_path = plot_dir / f"confounding_{safe_name(args.focal_predictor)}.png"
        save_current_figure(coef_plot_path)
        plots.append(str(coef_plot_path))

    report = {
        "input_path": args.input_path,
        "outcome": args.outcome,
        "focal_predictor": args.focal_predictor,
        "controls": args.control,
        "base_model_type": base_type,
        "adjusted_model_type": adj_type,
        "unadjusted_term": base_term,
        "adjusted_term": adj_term,
        "estimate_pct_change": pct_change,
        "missingness": to_jsonable(missingness),
        "n_unadjusted_rows": int(len(base_frame)),
        "n_adjusted_rows": int(len(adj_frame)),
        "subgroup_summary": to_jsonable(subgroup_summary) if subgroup_summary is not None else None,
        "plots": plots,
    }
    write_json(report, args.output)
    print(f"Wrote confounding check report to {args.output}")


if __name__ == "__main__":
    main()

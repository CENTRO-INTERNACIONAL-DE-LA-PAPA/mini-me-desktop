#!/usr/bin/env python
from __future__ import annotations

import argparse
from pathlib import Path

import matplotlib.pyplot as plt
import pandas as pd
import seaborn as sns

from common import (
    ensure_directory,
    infer_column_groups,
    load_dataframe,
    maybe_parse_datetimes,
    safe_name,
    save_current_figure,
    setup_plotting,
    top_correlated_pairs,
    to_jsonable,
    write_json,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run bivariate EDA with plots and grouped summaries.")
    parser.add_argument("input_path", help="Path to the input table.")
    parser.add_argument("--output", required=True, help="Path to the output JSON file.")
    parser.add_argument("--plot-dir", required=True, help="Directory where plots will be written.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    parser.add_argument("--groupby", help="Categorical column to group numeric summaries by.")
    parser.add_argument("--target", action="append", default=[], help="Numeric target column for grouped summaries.")
    parser.add_argument(
        "--categorical-pair",
        nargs=2,
        action="append",
        default=[],
        metavar=("LEFT", "RIGHT"),
        help="Categorical pair for contingency analysis. Repeat as needed.",
    )
    parser.add_argument("--max-scatter-pairs", type=int, default=6, help="Max numeric pairs to plot.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    df = maybe_parse_datetimes(df)
    groups = infer_column_groups(df)
    plot_dir = ensure_directory(args.plot_dir)

    setup_plotting()
    plots: list[str] = []

    correlations = {}
    strongest_pairs = top_correlated_pairs(df, max_pairs=args.max_scatter_pairs)
    numeric_cols = groups["numeric"]
    if len(numeric_cols) >= 2:
        corr = df[numeric_cols].corr(numeric_only=True)
        correlations = to_jsonable(corr)

        plt.figure(figsize=(8, 6))
        sns.heatmap(corr, cmap="coolwarm", center=0, annot=False)
        plt.title("Numeric correlation heatmap")
        plot_path = plot_dir / "bivariate_correlation_heatmap.png"
        save_current_figure(plot_path)
        plots.append(str(plot_path))

        for pair in strongest_pairs:
            plt.figure(figsize=(6, 5))
            sns.scatterplot(data=df, x=pair["left"], y=pair["right"], alpha=0.7)
            plt.title(f"{pair['left']} vs {pair['right']}")
            plot_path = plot_dir / f"bivariate_scatter_{safe_name(pair['left'])}_{safe_name(pair['right'])}.png"
            save_current_figure(plot_path)
            plots.append(str(plot_path))

    grouped_summaries = {}
    if args.groupby and args.groupby in df.columns:
        targets = args.target or groups["numeric"][: min(4, len(groups["numeric"]))]
        for target in targets:
            if target not in df.columns:
                continue
            summary = (
                df.groupby(args.groupby)[target]
                .agg(["count", "mean", "median", "std", "min", "max"])
                .reset_index()
            )
            grouped_summaries[target] = to_jsonable(summary)

            plt.figure(figsize=(10, 4))
            sns.boxplot(data=df, x=args.groupby, y=target)
            plt.xticks(rotation=45, ha="right")
            plt.title(f"{target} by {args.groupby}")
            plot_path = plot_dir / f"bivariate_grouped_box_{safe_name(args.groupby)}_{safe_name(target)}.png"
            save_current_figure(plot_path)
            plots.append(str(plot_path))

    contingency_tables = {}
    for left, right in args.categorical_pair:
        if left not in df.columns or right not in df.columns:
            continue
        table = pd.crosstab(df[left].fillna("<MISSING>"), df[right].fillna("<MISSING>"))
        contingency_tables[f"{left}__{right}"] = to_jsonable(table)

        plt.figure(figsize=(8, 6))
        sns.heatmap(table, cmap="Blues")
        plt.title(f"{left} vs {right}")
        plot_path = plot_dir / f"bivariate_crosstab_{safe_name(left)}_{safe_name(right)}.png"
        save_current_figure(plot_path)
        plots.append(str(plot_path))

    report = {
        "input_path": args.input_path,
        "correlations": correlations,
        "strongest_numeric_pairs": strongest_pairs,
        "grouped_summaries": grouped_summaries,
        "contingency_tables": contingency_tables,
        "plots": plots,
    }
    write_json(report, args.output)
    print(f"Wrote bivariate EDA output to {args.output}")


if __name__ == "__main__":
    main()

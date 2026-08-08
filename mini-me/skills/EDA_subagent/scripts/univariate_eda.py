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
    to_jsonable,
    write_json,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run univariate EDA with summaries and plots.")
    parser.add_argument("input_path", help="Path to the input table.")
    parser.add_argument("--output", required=True, help="Path to the output JSON file.")
    parser.add_argument("--plot-dir", required=True, help="Directory where plots will be written.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    parser.add_argument("--max-numeric", type=int, default=8, help="Max numeric columns to plot.")
    parser.add_argument("--max-categorical", type=int, default=8, help="Max categorical columns to plot.")
    parser.add_argument("--max-datetime", type=int, default=4, help="Max datetime columns to plot.")
    parser.add_argument("--top-k", type=int, default=15, help="Top categories to include in bar plots.")
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

    numeric_summary: dict[str, dict[str, float | int | None]] = {}
    for column in groups["numeric"][: args.max_numeric]:
        summary = df[column].describe(percentiles=[0.05, 0.25, 0.5, 0.75, 0.95]).to_dict()
        numeric_summary[column] = to_jsonable(summary)

        fig, axes = plt.subplots(1, 2, figsize=(12, 4))
        sns.histplot(df[column].dropna(), kde=True, ax=axes[0], color="#4c78a8")
        axes[0].set_title(f"{column} distribution")
        sns.boxplot(x=df[column], ax=axes[1], color="#72b7b2")
        axes[1].set_title(f"{column} boxplot")
        plot_path = plot_dir / f"univariate_numeric_{safe_name(column)}.png"
        save_current_figure(plot_path)
        plots.append(str(plot_path))

    categorical_summary: dict[str, list[dict[str, object]]] = {}
    for column in groups["categorical"][: args.max_categorical]:
        counts = df[column].fillna("<MISSING>").astype("string").value_counts(dropna=False).head(args.top_k)
        categorical_summary[column] = [
            {"value": str(index), "count": int(value), "pct": float(value / max(len(df), 1))}
            for index, value in counts.items()
        ]

        plt.figure(figsize=(10, 4))
        sns.barplot(x=counts.index.astype(str), y=counts.values, color="#f58518")
        plt.xticks(rotation=45, ha="right")
        plt.title(f"{column} top categories")
        plt.xlabel(column)
        plt.ylabel("count")
        plot_path = plot_dir / f"univariate_categorical_{safe_name(column)}.png"
        save_current_figure(plot_path)
        plots.append(str(plot_path))

    datetime_summary: dict[str, dict[str, object]] = {}
    for column in groups["datetime"][: args.max_datetime]:
        non_null = df[column].dropna()
        if non_null.empty:
            continue
        datetime_summary[column] = {
            "min": non_null.min().isoformat(),
            "max": non_null.max().isoformat(),
            "n_non_null": int(non_null.shape[0]),
        }

        counts = non_null.dt.to_period("M").astype(str).value_counts().sort_index()
        plt.figure(figsize=(10, 4))
        sns.barplot(x=counts.index, y=counts.values, color="#54a24b")
        plt.xticks(rotation=45, ha="right")
        plt.title(f"{column} monthly coverage")
        plt.xlabel("month")
        plt.ylabel("count")
        plot_path = plot_dir / f"univariate_datetime_{safe_name(column)}.png"
        save_current_figure(plot_path)
        plots.append(str(plot_path))

    report = {
        "input_path": args.input_path,
        "numeric_summary": numeric_summary,
        "categorical_summary": categorical_summary,
        "datetime_summary": datetime_summary,
        "plots": plots,
    }
    write_json(report, args.output)
    print(f"Wrote univariate EDA output to {args.output}")


if __name__ == "__main__":
    main()

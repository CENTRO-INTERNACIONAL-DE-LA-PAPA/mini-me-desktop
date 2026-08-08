#!/usr/bin/env python
from __future__ import annotations

import argparse

import matplotlib.pyplot as plt
import pandas as pd
import seaborn as sns
from scipy import stats

from common import (
    ensure_directory,
    load_dataframe,
    maybe_parse_datetimes,
    safe_name,
    save_current_figure,
    setup_plotting,
    to_jsonable,
    write_json,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Analyze change over time or around an intervention date.")
    parser.add_argument("input_path")
    parser.add_argument("--outcome", required=True)
    parser.add_argument("--date-column", required=True)
    parser.add_argument("--intervention-date")
    parser.add_argument("--groupby")
    parser.add_argument("--output", required=True)
    parser.add_argument("--plot-dir", required=True)
    parser.add_argument("--sheet-name")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    df = maybe_parse_datetimes(df)
    frame = df[[args.date_column, args.outcome] + ([args.groupby] if args.groupby else [])].dropna().copy()
    frame[args.date_column] = pd.to_datetime(frame[args.date_column], errors="coerce")
    frame[args.outcome] = pd.to_numeric(frame[args.outcome], errors="coerce")
    frame = frame.dropna().sort_values(args.date_column)

    setup_plotting()
    plot_dir = ensure_directory(args.plot_dir)
    plots: list[str] = []

    monthly = (
        frame.set_index(args.date_column)[args.outcome]
        .resample("ME")
        .agg(["count", "mean", "median"])
        .reset_index()
    )
    plt.figure(figsize=(9, 4))
    sns.lineplot(data=monthly, x=args.date_column, y="mean", marker="o")
    plt.title(f"Monthly mean of {args.outcome}")
    monthly_plot = plot_dir / f"time_monthly_{safe_name(args.outcome)}.png"
    save_current_figure(monthly_plot)
    plots.append(str(monthly_plot))

    if args.groupby and args.groupby in frame.columns:
        grouped = (
            frame.groupby([pd.Grouper(key=args.date_column, freq="ME"), args.groupby])[args.outcome]
            .mean()
            .reset_index()
        )
        plt.figure(figsize=(9, 4))
        sns.lineplot(data=grouped, x=args.date_column, y=args.outcome, hue=args.groupby, marker="o")
        plt.title(f"Monthly mean of {args.outcome} by {args.groupby}")
        grouped_plot = plot_dir / f"time_grouped_{safe_name(args.outcome)}_{safe_name(args.groupby)}.png"
        save_current_figure(grouped_plot)
        plots.append(str(grouped_plot))

    pre_post = None
    if args.intervention_date:
        intervention_date = pd.to_datetime(args.intervention_date)
        pre = frame.loc[frame[args.date_column] < intervention_date, args.outcome].dropna().astype(float)
        post = frame.loc[frame[args.date_column] >= intervention_date, args.outcome].dropna().astype(float)
        if len(pre) and len(post):
            pre_post = {
                "intervention_date": intervention_date.isoformat(),
                "pre": {
                    "n": int(len(pre)),
                    "mean": float(pre.mean()),
                    "median": float(pre.median()),
                    "std": float(pre.std(ddof=1)) if len(pre) > 1 else None,
                },
                "post": {
                    "n": int(len(post)),
                    "mean": float(post.mean()),
                    "median": float(post.median()),
                    "std": float(post.std(ddof=1)) if len(post) > 1 else None,
                },
                "welch_t_test": to_jsonable(stats.ttest_ind(pre, post, equal_var=False, nan_policy="omit")),
                "mannwhitney_u": to_jsonable(stats.mannwhitneyu(pre, post, alternative="two-sided")),
            }

    report = {
        "input_path": args.input_path,
        "date_column": args.date_column,
        "outcome": args.outcome,
        "n_complete_rows": int(len(frame)),
        "time_range": {
            "min": frame[args.date_column].min().isoformat() if not frame.empty else None,
            "max": frame[args.date_column].max().isoformat() if not frame.empty else None,
        },
        "monthly_summary": to_jsonable(monthly),
        "pre_post": pre_post,
        "plots": plots,
    }
    write_json(report, args.output)
    print(f"Wrote time-change report to {args.output}")


if __name__ == "__main__":
    main()

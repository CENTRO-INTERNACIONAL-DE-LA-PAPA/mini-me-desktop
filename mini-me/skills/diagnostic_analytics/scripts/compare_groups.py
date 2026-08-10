#!/usr/bin/env python
from __future__ import annotations

import argparse
import json
import warnings
from pathlib import Path
from typing import Any

import dabest
import matplotlib.pyplot as plt
import pandas as pd
from scipy import stats

from common import (
    ensure_directory,
    ensure_parent,
    grouped_numeric_summary,
    load_dataframe,
    maybe_parse_datetimes,
    safe_name,
    setup_plotting,
    to_jsonable,
    write_json,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Compare an outcome across groups with DABEST as the default engine.")
    parser.add_argument("input_path")
    parser.add_argument("--outcome", required=True)
    parser.add_argument("--group", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--plot-dir", required=True)
    parser.add_argument("--sheet-name")
    parser.add_argument("--id-col")
    parser.add_argument("--idx", help="Comparison order as comma-separated values or JSON (supports nested shared-control layouts).")
    parser.add_argument(
        "--effect-size",
        choices=["mean_diff", "median_diff", "cohens_d", "hedges_g", "cliffs_delta", "cohens_h"],
        help="DABEST effect size to report. Defaults to cohens_h for proportional data, otherwise mean_diff.",
    )
    parser.add_argument("--paired", choices=["baseline", "sequential"])
    parser.add_argument("--proportional", action="store_true")
    parser.add_argument("--delta2", action="store_true")
    parser.add_argument("--mini-meta", action="store_true")
    parser.add_argument("--ps-adjust", action="store_true")
    parser.add_argument("--ci", type=int, default=95)
    parser.add_argument("--resamples", type=int, default=5000)
    parser.add_argument("--random-seed", type=int, default=12345)
    parser.add_argument("--experiment")
    parser.add_argument("--experiment-label")
    parser.add_argument("--x1-level")
    parser.add_argument("--horizontal", action="store_true")
    parser.add_argument("--forest-plot", action="store_true")
    parser.add_argument("--title")
    parser.add_argument(
        "--supplemental-test",
        choices=[
            "none",
            "welch_t",
            "students_t",
            "mannwhitney",
            "anova",
            "kruskal",
            "chi_square",
            "fisher_exact",
        ],
        default="none",
        help="Optional classical test to add alongside DABEST output.",
    )
    return parser.parse_args()


def _tupleify_idx(value: Any) -> Any:
    if isinstance(value, list):
        return tuple(_tupleify_idx(item) for item in value)
    return str(value)


def _parse_idx(raw_idx: str | None, observed_levels: list[str]) -> tuple[Any, ...]:
    if raw_idx:
        stripped = raw_idx.strip()
        if stripped.startswith("["):
            parsed = json.loads(stripped)
            return _tupleify_idx(parsed)
        return tuple(part.strip() for part in stripped.split(",") if part.strip())
    return tuple(observed_levels)


def _default_effect_size(*, proportional: bool) -> str:
    return "cohens_h" if proportional else "mean_diff"


def _save_figure(fig: plt.Figure, path: str | Path) -> None:
    file_path = Path(path)
    ensure_parent(file_path)
    fig.savefig(file_path, dpi=150, bbox_inches="tight")
    plt.close(fig)


def _ensure_binary_proportions(frame: pd.DataFrame, outcome: str) -> pd.DataFrame:
    frame[outcome] = pd.to_numeric(frame[outcome], errors="coerce")
    frame = frame.dropna(subset=[outcome]).copy()
    unique_values = set(frame[outcome].astype(float).unique().tolist())
    if not unique_values.issubset({0.0, 1.0}):
        raise ValueError("Proportional comparisons require a binary 0/1 outcome.")
    frame[outcome] = frame[outcome].astype(int)
    return frame


def _group_summary(frame: pd.DataFrame, *, group: str, outcome: str, proportional: bool) -> pd.DataFrame:
    if proportional:
        return (
            frame.groupby(group)[outcome]
            .agg(["count", "sum", "mean"])
            .rename(columns={"sum": "positive_count", "mean": "proportion"})
            .reset_index()
        )
    return grouped_numeric_summary(frame, group, outcome)


def _summarize_effect_results(results: pd.DataFrame) -> pd.DataFrame:
    drop_cols = {
        "bootstraps",
        "permutations",
        "permutations_var",
        "bec_bootstraps",
        "bca_interval_idx",
        "pct_interval_idx",
        "bec_bca_interval_idx",
        "bec_pct_interval_idx",
        "resamples",
        "random_seed",
    }
    keep_cols = [column for column in results.columns if column not in drop_cols]
    return results[keep_cols].copy()


def _run_supplemental_test(
    frame: pd.DataFrame,
    *,
    group: str,
    outcome: str,
    test_name: str,
    proportional: bool,
) -> dict[str, Any] | None:
    if test_name == "none":
        return None

    grouped = [series.astype(float) for _, series in frame.groupby(group, sort=False)[outcome]]
    labels = [str(label) for label in frame[group].drop_duplicates().tolist()]

    if test_name == "welch_t":
        if len(grouped) != 2:
            raise ValueError("welch_t requires exactly two groups.")
        result = stats.ttest_ind(grouped[0], grouped[1], equal_var=False, nan_policy="omit")
        return {"test": test_name, "groups": labels[:2], "result": to_jsonable(result)}

    if test_name == "students_t":
        if len(grouped) != 2:
            raise ValueError("students_t requires exactly two groups.")
        result = stats.ttest_ind(grouped[0], grouped[1], equal_var=True, nan_policy="omit")
        return {"test": test_name, "groups": labels[:2], "result": to_jsonable(result)}

    if test_name == "mannwhitney":
        if len(grouped) != 2:
            raise ValueError("mannwhitney requires exactly two groups.")
        result = stats.mannwhitneyu(grouped[0], grouped[1], alternative="two-sided")
        return {"test": test_name, "groups": labels[:2], "result": to_jsonable(result)}

    if test_name == "anova":
        if len(grouped) < 2:
            raise ValueError("anova requires at least two groups.")
        result = stats.f_oneway(*grouped)
        return {"test": test_name, "groups": labels, "result": to_jsonable(result)}

    if test_name == "kruskal":
        if len(grouped) < 2:
            raise ValueError("kruskal requires at least two groups.")
        result = stats.kruskal(*grouped)
        return {"test": test_name, "groups": labels, "result": to_jsonable(result)}

    contingency = pd.crosstab(frame[group], frame[outcome])
    if test_name == "chi_square":
        statistic, p_value, dof, expected = stats.chi2_contingency(contingency)
        return {
            "test": test_name,
            "groups": labels,
            "contingency_table": to_jsonable(contingency.reset_index()),
            "result": {
                "statistic": float(statistic),
                "p_value": float(p_value),
                "dof": int(dof),
                "expected": to_jsonable(expected.tolist()),
            },
        }

    if test_name == "fisher_exact":
        if not proportional:
            raise ValueError("fisher_exact is only valid for binary/proportional group comparisons.")
        if contingency.shape != (2, 2):
            raise ValueError("fisher_exact requires a 2x2 contingency table.")
        odds_ratio, p_value = stats.fisher_exact(contingency.values)
        return {
            "test": test_name,
            "groups": labels[:2],
            "contingency_table": to_jsonable(contingency.reset_index()),
            "result": {"odds_ratio": float(odds_ratio), "p_value": float(p_value)},
        }

    raise ValueError(f"Unsupported supplemental test: {test_name}")


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    df = maybe_parse_datetimes(df)

    required_columns = [args.group, args.outcome]
    if args.id_col:
        required_columns.append(args.id_col)
    frame = df[required_columns].dropna().copy()
    if frame.empty:
        raise ValueError("No complete rows available for group comparison.")

    frame[args.group] = frame[args.group].astype("string")
    if args.proportional:
        frame = _ensure_binary_proportions(frame, args.outcome)
    else:
        frame[args.outcome] = pd.to_numeric(frame[args.outcome], errors="coerce")
        frame = frame.dropna(subset=[args.outcome]).copy()

    if frame.empty:
        raise ValueError("No valid rows remain after coercing the outcome column.")

    observed_levels = [str(value) for value in frame[args.group].drop_duplicates().tolist()]
    if len(observed_levels) < 2:
        raise ValueError("At least two groups are required for comparison.")

    idx = _parse_idx(args.idx, observed_levels)
    effect_size = args.effect_size or _default_effect_size(proportional=args.proportional)

    setup_plotting()
    plot_dir = ensure_directory(args.plot_dir)

    dabest_obj = dabest.load(
        frame,
        idx=idx,
        x=args.group,
        y=args.outcome,
        paired=args.paired,
        id_col=args.id_col,
        ci=args.ci,
        resamples=args.resamples,
        random_seed=args.random_seed,
        proportional=args.proportional,
        delta2=args.delta2,
        experiment=args.experiment,
        experiment_label=args.experiment_label,
        x1_level=args.x1_level,
        mini_meta=args.mini_meta,
        ps_adjust=args.ps_adjust,
    )

    if not hasattr(dabest_obj, effect_size):
        raise ValueError(f"DABEST object does not support effect size '{effect_size}' for this design.")
    effect = getattr(dabest_obj, effect_size)

    comparison_title = args.title or f"{args.outcome} by {args.group} ({effect_size})"
    estimation_path = plot_dir / f"dabest_{safe_name(args.group)}_{safe_name(args.outcome)}_{safe_name(effect_size)}.png"
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        estimation_fig = effect.plot(horizontal=args.horizontal, title=comparison_title)
    _save_figure(estimation_fig, estimation_path)

    plot_paths = [str(estimation_path)]
    if args.forest_plot:
        forest_path = plot_dir / f"dabest_forest_{safe_name(args.group)}_{safe_name(args.outcome)}_{safe_name(effect_size)}.png"
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            forest_fig = dabest.forest_plot(
                [dabest_obj],
                effect_size=effect_size,
                horizontal=args.horizontal,
                title=f"{comparison_title} forest plot",
            )
        _save_figure(forest_fig, forest_path)
        plot_paths.append(str(forest_path))

    supplemental_test = _run_supplemental_test(
        frame,
        group=args.group,
        outcome=args.outcome,
        test_name=args.supplemental_test,
        proportional=args.proportional,
    )

    report = {
        "input_path": args.input_path,
        "group": args.group,
        "group_levels": observed_levels,
        "outcome": args.outcome,
        "n_complete_rows": int(len(frame)),
        "n_groups": int(frame[args.group].nunique()),
        "comparison_engine": "dabest",
        "effect_size": effect_size,
        "design": {
            "idx": to_jsonable(idx),
            "paired": args.paired,
            "id_col": args.id_col,
            "proportional": args.proportional,
            "delta2": args.delta2,
            "mini_meta": args.mini_meta,
            "ps_adjust": args.ps_adjust,
            "ci": args.ci,
            "resamples": args.resamples,
            "random_seed": args.random_seed,
            "horizontal": args.horizontal,
            "forest_plot": args.forest_plot,
        },
        "group_summary": to_jsonable(_group_summary(frame, group=args.group, outcome=args.outcome, proportional=args.proportional)),
        "effect_results": to_jsonable(_summarize_effect_results(effect.results)),
        "statistical_tests": to_jsonable(effect.statistical_tests),
        "supplemental_test": to_jsonable(supplemental_test),
        "plots": plot_paths,
    }
    write_json(report, args.output)
    print(f"Wrote group comparison report to {args.output}")


if __name__ == "__main__":
    main()

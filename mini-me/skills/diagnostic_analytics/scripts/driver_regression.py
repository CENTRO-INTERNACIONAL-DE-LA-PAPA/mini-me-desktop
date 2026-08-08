#!/usr/bin/env python
from __future__ import annotations

import argparse

import matplotlib.pyplot as plt
import pandas as pd
import seaborn as sns

from common import (
    coefficient_table,
    compute_vif,
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
    parser = argparse.ArgumentParser(description="Fit an interpretable regression for diagnostic analysis.")
    parser.add_argument("input_path")
    parser.add_argument("--outcome", required=True)
    parser.add_argument("--predictor", action="append", required=True, dest="predictors")
    parser.add_argument("--categorical", action="append", default=[])
    parser.add_argument("--output", required=True)
    parser.add_argument("--plot-dir", required=True)
    parser.add_argument("--sheet-name")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    df = maybe_parse_datetimes(df)

    model_type, result, frame, y, X_const = fit_regression(
        df,
        args.outcome,
        args.predictors,
        categorical=args.categorical or None,
    )

    coef_table = coefficient_table(result)
    vif_table = compute_vif(X_const)
    setup_plotting()
    plot_dir = ensure_directory(args.plot_dir)
    plots: list[str] = []

    coef_plot_df = coef_table[coef_table["term"] != "const"].copy()
    if not coef_plot_df.empty:
        plt.figure(figsize=(8, max(4, 0.45 * len(coef_plot_df))))
        plt.errorbar(
            x=coef_plot_df["coef"],
            y=coef_plot_df["term"],
            xerr=[coef_plot_df["coef"] - coef_plot_df["ci_low"], coef_plot_df["ci_high"] - coef_plot_df["coef"]],
            fmt="o",
            color="#4c78a8",
        )
        plt.axvline(0.0, color="black", linewidth=1, linestyle="--")
        plt.title("Coefficient estimates with 95% CI")
        plt.xlabel("estimate")
        coef_plot_path = plot_dir / f"regression_coefficients_{safe_name(args.outcome)}.png"
        save_current_figure(coef_plot_path)
        plots.append(str(coef_plot_path))

    if model_type == "ols":
        fitted = result.fittedvalues
        residuals = result.resid
        plt.figure(figsize=(7, 5))
        sns.scatterplot(x=fitted, y=residuals, alpha=0.8)
        plt.axhline(0.0, color="black", linewidth=1, linestyle="--")
        plt.xlabel("fitted values")
        plt.ylabel("residuals")
        plt.title("Residuals vs fitted")
        residual_plot_path = plot_dir / f"regression_residuals_{safe_name(args.outcome)}.png"
        save_current_figure(residual_plot_path)
        plots.append(str(residual_plot_path))
        fit_summary = {
            "r_squared": float(result.rsquared),
            "adj_r_squared": float(result.rsquared_adj),
            "aic": float(result.aic),
            "bic": float(result.bic),
        }
    else:
        fitted = result.fittedvalues
        outcome_df = pd.DataFrame({"predicted_probability": fitted, "outcome": y.astype(int)})
        plt.figure(figsize=(7, 5))
        sns.histplot(
            data=outcome_df,
            x="predicted_probability",
            hue="outcome",
            bins=20,
            element="step",
            stat="density",
            common_norm=False,
        )
        plt.title("Predicted probability distribution by outcome")
        prob_plot_path = plot_dir / f"logit_probability_{safe_name(args.outcome)}.png"
        save_current_figure(prob_plot_path)
        plots.append(str(prob_plot_path))
        fit_summary = {
            "aic": float(result.aic),
            "bic": None,
            "deviance": float(result.deviance),
            "null_deviance": float(result.null_deviance),
        }

    report = {
        "input_path": args.input_path,
        "outcome": args.outcome,
        "predictors": args.predictors,
        "categorical_predictors": args.categorical,
        "model_type": model_type,
        "n_complete_rows": int(len(frame)),
        "coefficients": to_jsonable(coef_table),
        "vif": to_jsonable(vif_table),
        "fit_summary": fit_summary,
        "plots": plots,
    }
    write_json(report, args.output)
    print(f"Wrote regression diagnostic report to {args.output}")


if __name__ == "__main__":
    main()

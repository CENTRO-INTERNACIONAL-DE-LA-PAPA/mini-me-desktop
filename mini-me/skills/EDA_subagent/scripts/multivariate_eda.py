#!/usr/bin/env python
from __future__ import annotations

import argparse
from pathlib import Path

import matplotlib.pyplot as plt
import pandas as pd
import seaborn as sns
from sklearn.decomposition import PCA
from sklearn.ensemble import IsolationForest
from sklearn.impute import SimpleImputer
from sklearn.preprocessing import StandardScaler

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
    parser = argparse.ArgumentParser(description="Run multivariate EDA with PCA and anomaly detection.")
    parser.add_argument("input_path", help="Path to the input table.")
    parser.add_argument("--output", required=True, help="Path to the output JSON file.")
    parser.add_argument("--plot-dir", required=True, help="Directory where plots will be written.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    parser.add_argument("--groupby", help="Optional column to color plots by.")
    parser.add_argument("--use-umap", action="store_true", help="Attempt UMAP if the package is installed.")
    parser.add_argument("--use-hdbscan", action="store_true", help="Attempt HDBSCAN if the package is installed.")
    parser.add_argument("--contamination", default="auto", help="IsolationForest contamination setting.")
    return parser.parse_args()


def _make_numeric_matrix(df: pd.DataFrame, numeric_cols: list[str]):
    numeric_df = df[numeric_cols].copy()
    imputer = SimpleImputer(strategy="median")
    scaler = StandardScaler()
    matrix = imputer.fit_transform(numeric_df)
    matrix = scaler.fit_transform(matrix)
    return numeric_df, matrix


def main() -> None:
    args = parse_args()
    sheet_name = int(args.sheet_name) if args.sheet_name and args.sheet_name.isdigit() else args.sheet_name
    df = load_dataframe(args.input_path, sheet_name=sheet_name)
    df = maybe_parse_datetimes(df)
    groups = infer_column_groups(df)
    numeric_cols = groups["numeric"]
    plot_dir = ensure_directory(args.plot_dir)

    setup_plotting()
    plots: list[str] = []
    report: dict[str, object] = {
        "input_path": args.input_path,
        "numeric_columns": numeric_cols,
        "plots": plots,
    }

    if len(numeric_cols) < 2 or len(df) < 3:
        report["skipped"] = "Need at least two numeric columns and three rows for multivariate EDA."
        write_json(report, args.output)
        print(f"Wrote multivariate EDA output to {args.output}")
        return

    numeric_df, matrix = _make_numeric_matrix(df, numeric_cols)

    pca = PCA(n_components=min(2, matrix.shape[1]))
    pca_coords = pca.fit_transform(matrix)
    pca_df = pd.DataFrame(pca_coords, columns=["PC1", "PC2"][: pca_coords.shape[1]])
    pca_y = pca_df.columns[1]
    if args.groupby and args.groupby in df.columns:
        pca_df[args.groupby] = df[args.groupby].astype("string")

    plt.figure(figsize=(7, 5))
    if args.groupby and args.groupby in pca_df.columns:
        sns.scatterplot(data=pca_df, x="PC1", y=pca_y, hue=args.groupby, alpha=0.8)
    else:
        sns.scatterplot(data=pca_df, x="PC1", y=pca_y, alpha=0.8)
    plt.title("PCA projection")
    pca_plot = plot_dir / "multivariate_pca.png"
    save_current_figure(pca_plot)
    plots.append(str(pca_plot))

    loadings = pd.DataFrame(
        pca.components_.T,
        index=numeric_cols,
        columns=["PC1", "PC2"][: pca.components_.shape[0]],
    )

    iso = IsolationForest(contamination=args.contamination, random_state=42)
    iso_labels = iso.fit_predict(matrix)
    iso_scores = iso.score_samples(matrix)
    anomaly_count = int((iso_labels == -1).sum())

    anomaly_df = pca_df.copy()
    anomaly_df["is_anomaly"] = iso_labels == -1

    plt.figure(figsize=(7, 5))
    sns.scatterplot(data=anomaly_df, x="PC1", y=pca_y, hue="is_anomaly", alpha=0.8)
    plt.title("IsolationForest anomalies on PCA plane")
    anomaly_plot = plot_dir / "multivariate_isolation_forest.png"
    save_current_figure(anomaly_plot)
    plots.append(str(anomaly_plot))

    report["pca"] = {
        "explained_variance_ratio": to_jsonable(pca.explained_variance_ratio_),
        "components": to_jsonable(loadings.reset_index().rename(columns={"index": "feature"})),
    }
    report["isolation_forest"] = {
        "anomaly_count": anomaly_count,
        "anomaly_fraction": anomaly_count / max(len(df), 1),
        "scores_summary": {
            "min": float(iso_scores.min()),
            "median": float(pd.Series(iso_scores).median()),
            "max": float(iso_scores.max()),
        },
    }

    if args.use_umap:
        try:
            import umap

            reducer = umap.UMAP(random_state=42)
            umap_coords = reducer.fit_transform(matrix)
            umap_df = pd.DataFrame(umap_coords, columns=["UMAP1", "UMAP2"])
            if args.groupby and args.groupby in df.columns:
                umap_df[args.groupby] = df[args.groupby].astype("string")

            plt.figure(figsize=(7, 5))
            if args.groupby and args.groupby in umap_df.columns:
                sns.scatterplot(data=umap_df, x="UMAP1", y="UMAP2", hue=args.groupby, alpha=0.8)
            else:
                sns.scatterplot(data=umap_df, x="UMAP1", y="UMAP2", alpha=0.8)
            plt.title("UMAP projection")
            umap_plot = plot_dir / "multivariate_umap.png"
            save_current_figure(umap_plot)
            plots.append(str(umap_plot))
            report["umap"] = {"status": "ok"}
        except ImportError:
            report["umap"] = {"status": "skipped", "reason": "umap-learn is not installed"}

    if args.use_hdbscan:
        try:
            import hdbscan

            clusterer = hdbscan.HDBSCAN()
            labels = clusterer.fit_predict(matrix)
            cluster_df = pca_df.copy()
            cluster_df["cluster"] = labels.astype(str)

            plt.figure(figsize=(7, 5))
            sns.scatterplot(data=cluster_df, x="PC1", y=pca_y, hue="cluster", alpha=0.8)
            plt.title("HDBSCAN clusters on PCA plane")
            cluster_plot = plot_dir / "multivariate_hdbscan.png"
            save_current_figure(cluster_plot)
            plots.append(str(cluster_plot))
            report["hdbscan"] = {
                "status": "ok",
                "clusters": to_jsonable(pd.Series(labels).value_counts().sort_index()),
            }
        except ImportError:
            report["hdbscan"] = {"status": "skipped", "reason": "hdbscan is not installed"}

    write_json(report, args.output)
    print(f"Wrote multivariate EDA output to {args.output}")


if __name__ == "__main__":
    main()

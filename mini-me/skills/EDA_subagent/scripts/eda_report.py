#!/usr/bin/env python
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Combine EDA script outputs into a markdown report.")
    parser.add_argument("--profile", required=True, help="Path to the profile JSON output.")
    parser.add_argument("--univariate", required=True, help="Path to the univariate JSON output.")
    parser.add_argument("--bivariate", required=True, help="Path to the bivariate JSON output.")
    parser.add_argument("--multivariate", required=True, help="Path to the multivariate JSON output.")
    parser.add_argument("--output", required=True, help="Path to the markdown report.")
    return parser.parse_args()


def load_json(path: str | Path) -> dict[str, Any]:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def main() -> None:
    args = parse_args()
    profile = load_json(args.profile)
    univariate = load_json(args.univariate)
    bivariate = load_json(args.bivariate)
    multivariate = load_json(args.multivariate)

    missingness = sorted(
        profile.get("missingness_by_column", []),
        key=lambda item: item.get("missing_pct", 0),
        reverse=True,
    )[:5]
    strongest_pairs = bivariate.get("strongest_numeric_pairs", [])[:5]

    lines = [
        f"# EDA Report: {profile.get('table_name', 'dataset')}",
        "",
        "## Dataset overview",
        f"- Rows: {profile.get('n_rows')}",
        f"- Columns: {profile.get('n_columns')}",
        f"- Numeric columns: {len(profile.get('column_groups', {}).get('numeric', []))}",
        f"- Categorical columns: {len(profile.get('column_groups', {}).get('categorical', []))}",
        f"- Datetime columns: {len(profile.get('column_groups', {}).get('datetime', []))}",
        "",
        "## Missingness caveats",
    ]

    if missingness:
        for item in missingness:
            lines.append(
                f"- {item['column']}: {item['missing_count']} missing ({item['missing_pct']:.1%})"
            )
    else:
        lines.append("- No major missingness issues recorded.")

    lines.extend(["", "## Strongest bivariate relationships"])
    if strongest_pairs:
        for item in strongest_pairs:
            lines.append(
                f"- {item['left']} vs {item['right']}: correlation {item['correlation']:.3f}"
            )
    else:
        lines.append("- No numeric pair relationships were available.")

    lines.extend(["", "## Multivariate structure"])
    pca = multivariate.get("pca")
    if isinstance(pca, dict):
        ratio = pca.get("explained_variance_ratio", [])
        if ratio:
            rendered = ", ".join(f"{float(value):.3f}" for value in ratio)
            lines.append(f"- PCA explained variance ratio: {rendered}")

    iso = multivariate.get("isolation_forest", {})
    if isinstance(iso, dict) and "anomaly_count" in iso:
        lines.append(
            f"- IsolationForest anomalies: {iso['anomaly_count']} ({iso['anomaly_fraction']:.1%})"
        )

    if multivariate.get("umap"):
        umap_status = multivariate["umap"].get("status")
        lines.append(f"- UMAP status: {umap_status}")
    if multivariate.get("hdbscan"):
        hdbscan_status = multivariate["hdbscan"].get("status")
        lines.append(f"- HDBSCAN status: {hdbscan_status}")

    lines.extend(["", "## Generated artifacts"])
    for section_name, payload in [
        ("profile", profile),
        ("univariate", univariate),
        ("bivariate", bivariate),
        ("multivariate", multivariate),
    ]:
        plots = payload.get("plots", [])
        if plots:
            lines.append(f"- {section_name}:")
            for plot in plots:
                lines.append(f"  - {plot}")

    Path(args.output).write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Wrote EDA report to {args.output}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Combine diagnostic analysis outputs into a markdown report.")
    parser.add_argument("--compare-groups")
    parser.add_argument("--regression")
    parser.add_argument("--confounding")
    parser.add_argument("--time-change")
    parser.add_argument("--output", required=True)
    return parser.parse_args()


def load_json(path: str | None) -> dict[str, Any] | None:
    if not path:
        return None
    file_path = Path(path)
    if not file_path.exists():
        return None
    return json.loads(file_path.read_text(encoding="utf-8"))


def main() -> None:
    args = parse_args()
    compare_groups = load_json(args.compare_groups)
    regression = load_json(args.regression)
    confounding = load_json(args.confounding)
    time_change = load_json(args.time_change)

    lines = ["# Diagnostic Analytics Report", ""]

    if compare_groups:
        lines.extend(
            [
                "## Group comparison",
                f"- Outcome: {compare_groups['outcome']}",
                f"- Group: {compare_groups['group']}",
                f"- Comparison engine: {compare_groups.get('comparison_engine', 'unknown')}",
                f"- Effect size: {compare_groups.get('effect_size', 'unknown')}",
                f"- Complete rows: {compare_groups['n_complete_rows']}",
                "",
            ]
        )

    if regression:
        fit_summary = regression.get("fit_summary", {})
        lines.extend(
            [
                "## Regression diagnostics",
                f"- Model type: {regression['model_type']}",
                f"- Outcome: {regression['outcome']}",
                f"- Predictors: {', '.join(regression['predictors'])}",
                f"- Complete rows: {regression['n_complete_rows']}",
                f"- Fit summary: {fit_summary}",
                "",
            ]
        )

    if confounding:
        lines.extend(
            [
                "## Confounding checks",
                f"- Focal predictor: {confounding['focal_predictor']}",
                f"- Controls: {', '.join(confounding['controls']) if confounding['controls'] else '(none)'}",
                f"- Estimate percent change after controls: {confounding['estimate_pct_change']}",
                "",
            ]
        )

    if time_change:
        lines.extend(
            [
                "## Time-change analysis",
                f"- Outcome: {time_change['outcome']}",
                f"- Date column: {time_change['date_column']}",
                f"- Complete rows: {time_change['n_complete_rows']}",
                "",
            ]
        )

    lines.extend(
        [
            "## Interpretation reminder",
            "- Prefer associative language unless the design supports stronger claims.",
            "- Report caveats around missingness, imbalance, outliers, collinearity, and confounding.",
        ]
    )

    Path(args.output).write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Wrote diagnostic report to {args.output}")


if __name__ == "__main__":
    main()

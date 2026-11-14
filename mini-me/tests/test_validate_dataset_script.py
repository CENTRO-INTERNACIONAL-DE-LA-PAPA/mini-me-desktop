"""Runs `skills/data_cleaning/scripts/validate_dataset.py` for real and reads what it writes.

`test_validation_tools.py` pins how the backend calls the script and parses its report, from a
hand-written report. Nothing there executes the script, so a report whose summary held method
objects instead of counts would pass. This test does execute it.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pandas as pd
import pytest

pytest.importorskip("pointblank")

SCRIPT = Path(__file__).resolve().parents[1] / "skills" / "data_cleaning" / "scripts" / "validate_dataset.py"


@pytest.fixture
def report(tmp_path: Path) -> dict:
    """Six plots: one id repeated, one genotype missing."""
    table = tmp_path / "plots.csv"
    pd.DataFrame(
        {
            "plot_id": ["P1", "P2", "P3", "P4", "P5", "P5"],
            "genotype": ["Yungay", "Canchan", None, "Unica", "Huayro", "Huayro"],
            "yield_t_ha": [12.5, 14.0, 9.8, 15.1, 11.0, 11.0],
        }
    ).to_csv(table, index=False)
    output = tmp_path / "validation.json"
    result = subprocess.run(
        [sys.executable, str(SCRIPT), str(table), "--output", str(output), "--key", "plot_id", "--not-null", "genotype"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr
    return json.loads(output.read_text(encoding="utf-8"))


def test_the_summary_holds_counts_not_method_objects(report: dict) -> None:
    summary = report["summary"]
    assert set(summary) == {"n", "n_passed", "n_failed", "f_passed", "f_failed"}
    for name, per_step in summary.items():
        assert isinstance(per_step, dict) and per_step, name
        assert all(isinstance(value, (int, float)) for value in per_step.values()), (name, per_step)

    steps = sorted(summary["n"], key=int)
    assert all(summary["n"][step] == 6 for step in steps)
    assert all(summary["n_passed"][step] + summary["n_failed"][step] == 6 for step in steps)


def test_the_report_counts_the_defects_and_carries_no_rows(report: dict) -> None:
    assert report["all_passed"] is False
    failed = {(row["rule"], row["column"]): row["failed"] for row in report["checks_summary"]}
    assert failed[("rows_distinct", "plot_id")] == 2  # both rows of the repeated id
    assert failed[("col_vals_not_null", "genotype")] == 1

    # A summary built from an uncalled method would embed a preview of the table.
    assert "bound method" not in json.dumps(report["summary"])
    assert "Canchan" not in json.dumps(report["summary"])

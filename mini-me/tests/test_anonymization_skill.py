"""Tests for the anonymization skill scripts (`skills/anonymization/scripts/`).

They pin the three promises the skill makes: the detection report holds no cell
value, the raw file is never overwritten, and the output reaches the k that the
actions asked for.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pandas as pd
import pytest

SCRIPTS = Path(__file__).resolve().parents[1] / "skills" / "anonymization" / "scripts"


def run(script: str, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(SCRIPTS / script), *args], capture_output=True, text=True)


@pytest.fixture
def survey(tmp_path: Path) -> Path:
    rows = [
        {
            "farmer_name": f"Persona Numero{i}",
            "phone": f"9{i:08d}",
            "dni": f"{40000000 + i}",
            "household_id": f"HH-{i % 12}",
            "latitude": f"{-12.0 - i / 10000:.6f}",
            "district": "Huancayo" if i % 2 else "Jauja",
            "sex": "F" if i % 3 else "M",
            "age": str(20 + i % 40),
            "yield_t_ha": str(10 + i % 7),
        }
        for i in range(60)
    ]
    path = tmp_path / "survey.csv"
    pd.DataFrame(rows).to_csv(path, index=False)
    return path


def test_detection_flags_identifiers_and_leaks_no_value(survey: Path, tmp_path: Path) -> None:
    report_path = tmp_path / "detect.json"
    result = run("detect_pii.py", str(survey), "--output", str(report_path))
    assert result.returncode == 0, result.stderr

    report = json.loads(report_path.read_text())
    assert {"farmer_name", "phone", "dni", "latitude"} <= set(report["summary"]["direct"])
    assert {"district", "sex", "age", "household_id"} <= set(report["summary"]["quasi"])
    assert "yield_t_ha" not in sum(report["summary"].values(), [])

    text = report_path.read_text() + result.stdout
    assert "Persona" not in text and "900000001" not in text and "40000001" not in text


def test_refuses_to_overwrite_the_raw_file(survey: Path, tmp_path: Path) -> None:
    actions = tmp_path / "actions.json"
    actions.write_text(json.dumps({"actions": [{"type": "drop", "columns": ["phone"]}]}))
    before = survey.read_bytes()
    result = run("anonymize_dataset.py", str(survey), "--actions", str(actions), "--output", str(survey), "--report", str(tmp_path / "r.json"))
    assert result.returncode != 0
    assert survey.read_bytes() == before


def test_actions_reach_the_requested_k(survey: Path, tmp_path: Path) -> None:
    actions = tmp_path / "actions.json"
    actions.write_text(json.dumps({"actions": [
        {"type": "drop", "columns": ["farmer_name", "phone", "dni"]},
        {"type": "pseudonymize", "column": "household_id", "prefix": "HH"},
        {"type": "round_coordinates", "columns": ["latitude"], "decimals": 1},
        {"type": "bin_numeric", "column": "age", "width": 20},
        {"type": "suppress_small_groups", "columns": ["district", "sex", "age"], "k": 5},
    ]}))
    out, risk = tmp_path / "anon.csv", tmp_path / "risk.json"
    result = run("anonymize_dataset.py", str(survey), "--actions", str(actions), "--output", str(out), "--report", str(tmp_path / "r.json"))
    assert result.returncode == 0, result.stderr

    anon = pd.read_csv(out, dtype=str)
    assert not {"farmer_name", "phone", "dni"} & set(anon.columns)
    assert anon["household_id"].str.fullmatch(r"HH_\d{4}").all()
    assert anon["household_id"].nunique() == 12  # same household, same code
    assert not list(tmp_path.glob("*key*"))  # no pseudonym key unless asked for

    result = run("check_k_anonymity.py", str(out), "--quasi", "district", "--quasi", "sex", "--quasi", "age", "--output", str(risk))
    assert result.returncode == 0, result.stderr
    assert json.loads(risk.read_text())["passes"] is True

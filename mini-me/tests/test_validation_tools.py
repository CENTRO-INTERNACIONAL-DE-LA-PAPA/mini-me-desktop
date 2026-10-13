"""Guards for the Pointblank validation tools (`backend.validation_tools`).

The command builders are the contract with `skills/data_cleaning/scripts`, so their argv is pinned
here. The parsing tests use the shape `validate_dataset.py` writes, including the `checks_summary`
rows the answer's table is built from.
"""

from __future__ import annotations

import asyncio
import json
import shlex
from pathlib import Path
from types import SimpleNamespace

from backend.validation_tools import (
    MISSING_POINTBLANK,
    _build_profile_command,
    _build_validate_command,
    _parse_report,
    profile_dataset,
    validate_dataset,
)

WORK = "/w/thread-1"
SCRIPTS = Path("/backend/skills/data_cleaning/scripts")

REPORT = {
    "input_path": "/w/thread-1/plots.csv",
    "table_name": "plots",
    "all_passed": False,
    "summary": {"n": [4, 4]},
    "checks": [],
    "checks_summary": [
        {"step": 1, "rule": "col_vals_not_null", "column": "plot_id", "passed": 3, "failed": 1, "ok": False},
        {"step": 2, "rule": "rows_distinct", "column": "plot_id", "passed": 2, "failed": 2, "ok": False},
    ],
}


def _validate(**kwargs):
    return _build_validate_command(WORK, "/plots.csv", scripts_dir=SCRIPTS, python="python", **kwargs)


def test_the_validation_writes_json_and_html_beside_the_conversation_never_over_the_input():
    command = _validate()
    assert "/backend/skills/data_cleaning/scripts/validate_dataset.py" in command
    assert "--output /w/thread-1/validation/plots-validation.json" in command
    assert "--html /w/thread-1/validation/plots-validation.html" in command
    assert command.endswith("cat /w/thread-1/validation/plots-validation.json")
    # The input is only ever read: it never follows --output or --html.
    assert "--output /w/thread-1/plots.csv" not in command
    assert "--html /w/thread-1/plots.csv" not in command


def test_a_leading_slash_means_the_conversation_folder():
    command = _validate()
    assert "input=/w/thread-1/plots.csv" in command
    assert '[ -f /plots.csv ] && input=/plots.csv' in command
    assert '"$input"' in command


def test_keys_and_required_columns_are_repeated_flags():
    command = _validate(key_columns=["site", "plot"], not_null=["yield"])
    assert "--key site --key plot" in command
    assert "--not-null yield" in command
    assert "--rules" not in command


def test_rules_are_passed_only_when_given():
    assert "--rules rules.yaml" in _validate(rules_path="rules.yaml")


def test_names_with_spaces_are_quoted():
    command = _build_validate_command(WORK, "field data.csv", key_columns=["plot id"], python="python")
    assert shlex.quote("/w/thread-1/validation/field data-validation.json") in command
    assert shlex.quote("plot id") in command


def test_the_profile_writes_its_own_json_and_html():
    command = _build_profile_command(WORK, "plots.csv", key_columns=["plot_id"], scripts_dir=SCRIPTS, python="python")
    assert "profile_dataset.py" in command
    assert "--output /w/thread-1/validation/plots-profile.json" in command
    assert "--html /w/thread-1/validation/plots-profile.html" in command
    assert "--key plot_id" in command


def test_the_report_survives_a_stderr_section_after_it():
    output = json.dumps(REPORT) + "\n[stderr]\nUserWarning: something"
    assert _parse_report(output)["checks_summary"][1]["rule"] == "rows_distinct"
    assert _parse_report("Traceback (most recent call last):\n  boom") is None


class _Sandbox:
    def __init__(self, output="", error=None):
        self.output, self.error, self.commands = output, error, []

    async def aget_work_dir(self):
        return WORK

    async def aexecute_untruncated(self, command, timeout=None):
        self.commands.append(command)
        if self.error:
            raise self.error
        return SimpleNamespace(output=self.output, exit_code=0)


def _call(tool, sandbox, **kwargs):
    from backend.runtime import _active_sandbox

    async def go():
        token = _active_sandbox.set(sandbox)
        try:
            return json.loads(await tool.coroutine(**kwargs))
        finally:
            _active_sandbox.reset(token)

    return asyncio.run(go())


def test_every_check_comes_back_with_the_report_paths():
    result = _call(validate_dataset, _Sandbox(json.dumps(REPORT)), path="/plots.csv", key_columns=["plot_id"])
    assert result["error"] is None
    assert result["all_passed"] is False
    assert [check["rule"] for check in result["checks"]] == ["col_vals_not_null", "rows_distinct"]
    assert result["report_path"] == "validation/plots-validation.json"
    assert result["html_report"] == "validation/plots-validation.html"


def test_a_failed_html_page_still_returns_every_check():
    report = {**REPORT, "html_error": "ValueError: cannot draw the table"}
    result = _call(validate_dataset, _Sandbox(json.dumps(report)), path="/plots.csv")
    assert result["error"] is None
    assert len(result["checks"]) == 2
    assert result["html_report"] is None


def test_no_sandbox_is_an_error_not_a_raise():
    from backend.runtime import _active_sandbox

    async def go():
        token = _active_sandbox.set(None)
        try:
            return json.loads(await validate_dataset.coroutine(path="d.csv"))
        finally:
            _active_sandbox.reset(token)

    result = asyncio.run(go())
    assert result["error"] and result["checks"] == []


def test_an_execute_failure_is_an_error_not_a_raise():
    result = _call(validate_dataset, _Sandbox(error=TimeoutError("took too long")), path="d.csv")
    assert "took too long" in result["error"]
    assert result["checks"] == []


def test_a_missing_pointblank_is_named_as_an_installation_problem():
    output = "Traceback (most recent call last):\nModuleNotFoundError: No module named 'pointblank'"
    result = _call(validate_dataset, _Sandbox(output), path="d.csv")
    assert result["error"] == MISSING_POINTBLANK


def test_an_unreadable_file_reports_the_scripts_own_error():
    output = "Traceback (most recent call last):\nValueError: Unsupported file type: .pdf"
    result = _call(validate_dataset, _Sandbox(output), path="paper.pdf")
    assert result["error"] == "ValueError: Unsupported file type: .pdf"


def test_the_profile_returns_the_columns_missing_the_most():
    report = {
        "n_rows": 4,
        "n_columns": 2,
        "missingness_by_column": [
            {"column": "a", "missing_count": 0, "missing_pct": 0.0},
            {"column": "b", "missing_count": 3, "missing_pct": 0.75},
        ],
        "duplicate_rows": {"count": 1, "pct": 0.25},
    }
    result = _call(profile_dataset, _Sandbox(json.dumps(report)), path="plots.csv")
    assert result["error"] is None
    assert result["missing_by_column"][0]["column"] == "b"
    assert result["duplicate_rows"] == 1
    assert result["html_report"] == "validation/plots-profile.html"

"""Validate and profile a dataset with Pointblank, and hand back the results as data.

# Why tools rather than the skill's scripts alone

`skills/data_cleaning` ships `validate_dataset.py` and `profile_dataset.py`, and its guide tells the
data-cleaning agent to use them. Left as a suggestion, the agent writes its own pandas checks
instead, and a researcher cannot tell those from a Pointblank validation. These tools run the same
scripts, so the work is Pointblank's, and return each check as a row the answer can quote rather
than recompute. `middleware/validate_first.py` makes calling `validate_dataset` a condition of
answering.

# What they write

Into `validation/` in the conversation folder, never over the input:

    <stem>-validation.json   every step, plus `checks_summary` (one row per check)
    <stem>-validation.html   Pointblank's own report, self-contained
    <stem>-profile.json      shape, types, missingness, duplicates, DataScan summary
    <stem>-profile.html      Pointblank's DataScan report

The HTML files are listed with the conversation's documents and open in the researcher's browser.

# Shape

The command builders are pure, for the reason `paper_tools._build_search_command` is: the argv is
the contract with the script, and a unit test can pin it.
"""

from __future__ import annotations

import json
import logging
import shlex
import sys
from pathlib import Path, PurePosixPath
from typing import Any

from langchain_core.tools import tool

from backend.runtime import _active_sandbox

logger = logging.getLogger(__name__)

#: Profiling and validating a research-sized table takes seconds; a very large one, minutes.
VALIDATION_TIMEOUT_S = 300

#: The skill's scripts, read from the backend's own checkout. The sandbox is this machine's
#: filesystem, so the scripts run in place with the backend's interpreter, which has Pointblank.
SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "skills" / "data_cleaning" / "scripts"

#: Where the reports go, relative to the conversation folder.
REPORT_DIR = "validation"

MISSING_POINTBLANK = (
    "Pointblank is not installed in the backend — an installation problem, not a data problem. "
    "Reinstall the backend from Setup."
)


def _stem(path: str) -> str:
    return PurePosixPath(path).stem or "dataset"


def _report_paths(work_dir: str, path: str, kind: str) -> tuple[str, str]:
    """`(json, html)` paths for one dataset's `kind` report (`validation` or `profile`)."""
    base = f"{work_dir.rstrip('/')}/{REPORT_DIR}/{_stem(path)}-{kind}"
    return f"{base}.json", f"{base}.html"


def _resolve_input(work_dir: str, path: str) -> str:
    """A shell fragment that sets `$input` to the dataset the agent named.

    The agent's file tools treat a leading `/` as the conversation folder, while a researcher may
    give a real absolute path. The path as given wins when it exists; otherwise it is read relative
    to the conversation folder.
    """
    given = shlex.quote(path)
    in_workspace = shlex.quote(f"{work_dir.rstrip('/')}/{path.lstrip('/')}")
    return f"input={in_workspace}; [ -f {given} ] && input={given}"


def _build_validate_command(
    work_dir: str,
    path: str,
    key_columns: list[str] | None = None,
    not_null: list[str] | None = None,
    rules_path: str | None = None,
    scripts_dir: Path = SCRIPTS_DIR,
    python: str = sys.executable,
) -> str:
    """The shell command that validates one dataset and prints its JSON report."""
    report_json, report_html = _report_paths(work_dir, path, "validation")
    argv = [
        python,
        str(scripts_dir / "validate_dataset.py"),
        "__INPUT__",
        "--output",
        report_json,
        "--html",
        report_html,
    ]
    for column in key_columns or []:
        argv += ["--key", column]
    for column in not_null or []:
        argv += ["--not-null", column]
    if rules_path:
        argv += ["--rules", rules_path]
    run = shlex.join(argv).replace("__INPUT__", '"$input"')
    return f"{_resolve_input(work_dir, path)}; {run} >/dev/null && cat {shlex.quote(report_json)}"


def _build_profile_command(
    work_dir: str,
    path: str,
    key_columns: list[str] | None = None,
    scripts_dir: Path = SCRIPTS_DIR,
    python: str = sys.executable,
) -> str:
    """The shell command that profiles one dataset and prints its JSON report."""
    report_json, report_html = _report_paths(work_dir, path, "profile")
    argv = [
        python,
        str(scripts_dir / "profile_dataset.py"),
        "__INPUT__",
        "--output",
        report_json,
        "--html",
        report_html,
    ]
    for column in key_columns or []:
        argv += ["--key", column]
    run = shlex.join(argv).replace("__INPUT__", '"$input"')
    return f"{_resolve_input(work_dir, path)}; {run} >/dev/null && cat {shlex.quote(report_json)}"


def _parse_report(output: str) -> dict[str, Any] | None:
    """The JSON report out of execute output, which may carry a `[stderr]` section after it."""
    head = (output or "").split("[stderr]", 1)[0]
    start, end = head.find("{"), head.rfind("}")
    if start == -1 or end <= start:
        return None
    try:
        report = json.loads(head[start : end + 1])
    except (json.JSONDecodeError, ValueError):
        return None
    return report if isinstance(report, dict) else None


def _failure(output: str) -> str:
    """One line saying why a run produced no report."""
    if "No module named 'pointblank'" in (output or ""):
        return MISSING_POINTBLANK
    lines = [line.strip() for line in (output or "").splitlines() if line.strip()]
    errors = [line for line in lines if "Error" in line or "error:" in line]
    if errors:
        return errors[-1][:300]
    return lines[-1][:300] if lines else "the validation script produced no report"


def _relative(work_dir: str, path: str) -> str:
    prefix = work_dir.rstrip("/") + "/"
    return path[len(prefix) :] if path.startswith(prefix) else path


async def _run(sandbox: Any, command: str) -> str:
    """The merged output of `command`. Untruncated, because a long report would be cut mid-JSON."""
    runner = getattr(sandbox, "aexecute_untruncated", None) or sandbox.aexecute
    response = await runner(command, timeout=VALIDATION_TIMEOUT_S)
    return getattr(response, "output", "") or ""


async def _work_dir(sandbox: Any) -> str:
    try:
        return await sandbox.aget_work_dir()
    except Exception:  # noqa: BLE001
        return "."


@tool
async def validate_dataset(
    path: str,
    key_columns: list[str] | None = None,
    not_null: list[str] | None = None,
    rules_path: str | None = None,
) -> str:
    """Validate a tabular dataset with Pointblank and get back every check's result.

    Call this on every dataset before reporting on its quality, and again on the cleaned output.
    It never modifies the input file.

    Include **every** returned check in your answer as a table with the columns
    Check · Column · Passed · Failed · Result, then name `html_report`: Pointblank's full report,
    which the researcher can open from the conversation's files.

    If the result has an `error`, say that validation could not run and why. Never present checks
    you wrote yourself as Pointblank results.

    Args:
        path: the dataset (CSV, TSV, Parquet, JSON, JSONL or Excel), as you see it in the files.
        key_columns: columns that identify a row; checked as non-null and distinct together.
        not_null: other columns that must never be empty.
        rules_path: optional JSON or YAML file with a top-level `checks` list of Pointblank
            validation methods and their `kwargs`.

    Returns:
        JSON: {dataset, report_path, html_report, all_passed, checks: [{step, rule, column,
        passed, failed, ok}], error}
    """
    sandbox = _active_sandbox.get()
    if sandbox is None:
        return json.dumps({"dataset": path, "error": "no sandbox available to run the validation", "checks": []})
    work_dir = await _work_dir(sandbox)
    report_json, report_html = _report_paths(work_dir, path, "validation")
    command = _build_validate_command(work_dir, path, key_columns, not_null, rules_path)
    try:
        output = await _run(sandbox, command)
    except Exception as exc:  # noqa: BLE001
        logger.warning("validate_dataset failed for %r: %s", path, exc)
        return json.dumps({"dataset": path, "error": f"the validation failed: {exc}", "checks": []})

    report = _parse_report(output)
    if report is None:
        reason = _failure(output)
        logger.info("validate_dataset(%r) -> no report: %s", path, reason)
        return json.dumps({"dataset": path, "error": reason, "checks": []})

    checks = report.get("checks_summary") or []
    logger.info("validate_dataset(%r) -> %d check(s), all passed: %s", path, len(checks), report.get("all_passed"))
    return json.dumps(
        {
            "dataset": path,
            "report_path": _relative(work_dir, report_json),
            # None when Pointblank could not draw the page; the checks are still real.
            "html_report": None if report.get("html_error") else _relative(work_dir, report_html),
            "all_passed": bool(report.get("all_passed")),
            "checks": checks,
            "error": None,
        },
        indent=2,
    )


@tool
async def profile_dataset(path: str, key_columns: list[str] | None = None) -> str:
    """Profile a tabular dataset with Pointblank's DataScan before validating it.

    Gives the shape, column types, missing values per column and duplicate rows, and saves
    Pointblank's HTML profile alongside. It never modifies the input file.

    Args:
        path: the dataset, as you see it in the files.
        key_columns: columns that identify a row; duplicate key combinations are counted.

    Returns:
        JSON: {dataset, report_path, html_report, n_rows, n_columns, missing_by_column,
        duplicate_rows, error}
    """
    sandbox = _active_sandbox.get()
    if sandbox is None:
        return json.dumps({"dataset": path, "error": "no sandbox available to run the profile"})
    work_dir = await _work_dir(sandbox)
    report_json, report_html = _report_paths(work_dir, path, "profile")
    command = _build_profile_command(work_dir, path, key_columns)
    try:
        output = await _run(sandbox, command)
    except Exception as exc:  # noqa: BLE001
        logger.warning("profile_dataset failed for %r: %s", path, exc)
        return json.dumps({"dataset": path, "error": f"the profile failed: {exc}"})

    report = _parse_report(output)
    if report is None:
        return json.dumps({"dataset": path, "error": _failure(output)})

    return json.dumps(
        {
            "dataset": path,
            "report_path": _relative(work_dir, report_json),
            "html_report": _relative(work_dir, report_html),
            "n_rows": report.get("n_rows"),
            "n_columns": report.get("n_columns"),
            "missing_by_column": _top_missing(report.get("missingness_by_column")),
            "duplicate_rows": (report.get("duplicate_rows") or {}).get("count"),
            "error": None,
        },
        indent=2,
    )


def _top_missing(missingness: Any, limit: int = 10) -> list[dict[str, Any]]:
    """The `limit` columns with the most missing values.

    `missingness` is `common.missingness_by_column`'s list of `{column, missing_count, missing_pct}`.
    """
    rows = [row for row in missingness or [] if isinstance(row, dict)]
    rows.sort(key=lambda row: row.get("missing_count") or 0, reverse=True)
    return rows[:limit]

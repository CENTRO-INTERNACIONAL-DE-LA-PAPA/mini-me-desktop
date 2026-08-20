"""Guards for the Asta DataVoyager tool (`backend.datavoyager_tools`).

Like the theorizer tests, these pin the real `asta analyze-data` CLI contract —
the `submit` / `task` / `artifacts` subcommands and their flags — so flag drift is
caught in CI rather than in a live run, and they exercise the shape-tolerant
parsing of the A2A task record (state, narrative, failure reason) plus the
persist/export step, all without a live service.

DataVoyager is async like the theorizer: `analyze_data` submits and returns
immediately; status comes from the cheap `asta analyze-data task <id>` fetch (not
the blocking `poll`), and terminal states persist + export to the sandbox.
"""

from __future__ import annotations

from types import SimpleNamespace

import asyncio
import json
import subprocess
import sys

from backend.sandbox import EXECUTE_OUTPUT_MAX_BYTES
from backend.datavoyager_tools import (
    _REDUCE_TASK_PY,
    _analysis_text,
    _artifact_names,
    _build_submit_command,
    _build_task_command,
    _export_shell,
    _extract_json,
    _failure_reason,
    _split_paths,
    _state_of,
    _status_message_text,
    _submit_shell,
    _task_shell,
    analysis_markdown,
    is_valid_task_id,
    persist_analysis_outputs,
    poll_analysis_status,
)

_TID = "6580ec74-121a-4757-b5e2-2e1ed9fc210e"


# ---------------------------------------------------------------------------
# CLI contract — the part that would break in production on flag drift
# ---------------------------------------------------------------------------

def test_submit_command_uses_real_subcommand_and_positional_question() -> None:
    args = _build_submit_command("What drives yield?", ["./data.csv"])
    assert args[:3] == ["asta", "analyze-data", "submit"]
    assert "--output" in args
    # The question is positional and precedes the dataset paths.
    q_index = args.index("What drives yield?")
    assert args[q_index + 1] == "./data.csv"
    assert "--context-id" not in args


def test_submit_command_passes_multiple_datasets_after_question() -> None:
    args = _build_submit_command("q", ["./a.csv", "./b.csv"])
    assert args[-2:] == ["./a.csv", "./b.csv"]
    assert args.index("q") < args.index("./a.csv")


def test_submit_command_adds_context_id_for_followups() -> None:
    args = _build_submit_command("follow up", [], context_id="ctx-123")
    assert "--context-id" in args
    assert args[args.index("--context-id") + 1] == "ctx-123"
    # A follow-up with no new files is valid (reuses the workspace data).
    assert args[-1] == "follow up"


def test_task_command_is_the_cheap_nonblocking_fetch() -> None:
    # Status must use `task` (returns immediately), NOT `poll` (blocks until
    # terminal) — the whole point of the async design.
    args = _build_task_command(_TID)
    assert args == ["asta", "analyze-data", "task", _TID]
    assert "poll" not in args


def test_submit_shell_cats_the_response_json() -> None:
    shell = _submit_shell("q", ["./d.csv"], None)
    assert "asta analyze-data submit" in shell
    assert ">/dev/null 2>&1" in shell
    assert shell.strip().split(";")[-1].strip().startswith("cat ")


def test_task_shell_pipes_task_through_reducer() -> None:
    shell = _task_shell(_TID)
    assert "asta analyze-data task" in shell
    assert _TID in shell
    assert "python3 -c" in shell
    assert "2>/dev/null" in shell  # progress stream dropped
    assert "poll" not in shell


def test_export_shell_dumps_task_then_runs_artifacts_export() -> None:
    shell = _export_shell(_TID, "/workspace/analysis")
    # Full record is dumped in-sandbox (never pulled to the server)…
    assert f"asta analyze-data task {_TID}" in shell
    assert "/workspace/analysis/" + _TID + "/task.json" in shell
    # …then rendered to files via the artifacts exporter.
    assert "asta artifacts --input" in shell
    assert "--format md" in shell
    assert "/export" in shell


# ---------------------------------------------------------------------------
# Path splitting + task-id guard
# ---------------------------------------------------------------------------

def test_split_paths_handles_commas_spaces_and_backticks() -> None:
    assert _split_paths("./a.csv, ./b.csv") == ["./a.csv", "./b.csv"]
    assert _split_paths("`./a.csv` `./b.csv`") == ["./a.csv", "./b.csv"]
    assert _split_paths("") == []
    assert _split_paths("./only.csv") == ["./only.csv"]


def test_is_valid_task_id() -> None:
    assert is_valid_task_id(_TID)
    assert not is_valid_task_id("")
    assert not is_valid_task_id("../etc/passwd")
    assert not is_valid_task_id("not-a-uuid")


# ---------------------------------------------------------------------------
# Parsing helpers — shape-tolerant, no fabrication
# ---------------------------------------------------------------------------

def test_state_of_handles_none() -> None:
    assert _state_of(None) is None
    assert _state_of({}) is None
    assert _state_of({"status": {"state": "completed"}}) == "completed"


def test_status_message_text_reads_string_and_parts() -> None:
    assert _status_message_text({"status": {"message": "boom"}}) == "boom"
    task = {"status": {"message": {"parts": [{"text": "PaperFinder timed out"}]}}}
    assert "PaperFinder timed out" in _status_message_text(task)
    data_task = {"status": {"message": {"parts": [{"data": {"short_desc": "gateway 503"}}]}}}
    assert "gateway 503" in _status_message_text(data_task)


def test_analysis_text_concatenates_artifact_text_and_caps() -> None:
    task = {
        "status": {"message": "done"},
        "artifacts": [
            {"name": "EDA", "type": "analysis", "text": "correlation is 0.8"},
            {"name": "Chart", "type": "figure", "text": ""},
        ],
    }
    text = _analysis_text(task)
    assert "correlation is 0.8" in text
    assert "## EDA" in text
    assert "done" in text


def test_artifact_names_prefers_name_then_type() -> None:
    task = {"artifacts": [{"name": "Report"}, {"type": "figure"}, {}]}
    assert _artifact_names(task) == ["Report", "figure"]


def test_failure_reason_prefers_message_then_honest_generic() -> None:
    task = {"status": {"state": "failed", "message": "worker crashed"}}
    assert _failure_reason(task, "failed") == "worker crashed"
    generic = _failure_reason({"status": {"state": "failed"}}, "failed")
    assert "without reporting a reason" in generic
    assert "inside Asta" in generic


def test_failure_reason_reads_top_level_error_fields() -> None:
    task = {"status": {"state": "failed"}, "error_fields": {"error": "internal 500"}}
    assert _failure_reason(task, "failed") == "internal 500"


# ---------------------------------------------------------------------------
# In-sandbox reducer — strips bulk, keeps status + narrative under the cap
# ---------------------------------------------------------------------------

def _run_reducer(task: dict) -> str:
    proc = subprocess.run(
        [sys.executable, "-c", _REDUCE_TASK_PY],
        input=json.dumps(task),
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, proc.stderr
    return proc.stdout


def _giant_completed_task() -> dict:
    bulk = "x" * 500_000  # stand-in for the notebook source + data tables
    return {
        "id": _TID,
        "status": {"state": "completed", "message": {"parts": [{"text": "Analysis complete."}]}},
        "artifacts": [
            {"name": "Notebook", "metadata": {"type": "notebook"},
             "parts": [{"kind": "file", "data": {"blob": bulk}}]},
            {"name": "Data table", "metadata": {"type": "table"},
             "parts": [{"data": {"rows": bulk}}]},
            {"name": "Findings", "metadata": {"type": "analysis"},
             "parts": [{"text": "Yield correlates with rainfall (r=0.72)."}]},
        ],
    }


def test_reducer_shrinks_giant_task_and_keeps_narrative() -> None:
    reduced_out = _run_reducer(_giant_completed_task())
    assert len(reduced_out.encode()) < EXECUTE_OUTPUT_MAX_BYTES
    task = _extract_json(reduced_out)
    assert task is not None
    assert _state_of(task) == "completed"
    assert "xxxxxxxx" not in reduced_out  # bulk blobs gone
    text = _analysis_text(task)
    assert "Yield correlates with rainfall" in text
    assert "Analysis complete." in text
    assert _artifact_names(task) == ["Notebook", "Data table", "Findings"]


def test_reducer_forwards_top_level_error() -> None:
    failed = {"status": {"state": "failed", "message": None}, "error": "dv worker crashed"}
    reduced = _extract_json(_run_reducer(failed))
    assert reduced is not None
    assert _state_of(reduced) == "failed"
    assert _failure_reason(reduced, "failed") == "dv worker crashed"


# ---------------------------------------------------------------------------
# poll_analysis_status — terminal states over a fake (task-fetch) sandbox
# ---------------------------------------------------------------------------

class _FakeSandbox:
    """Sandbox stub whose aexecute returns a canned `task <id>` output."""

    def __init__(self, output: str) -> None:
        self._output = output

    async def aexecute(self, command: str, *, timeout: int | None = None):
        class _Resp:
            output = self._output

        return _Resp()


def test_poll_completed_parses_narrative() -> None:
    task = {
        "status": {"state": "completed", "message": "done"},
        "artifacts": [{"name": "Findings", "text": "r=0.9 between X and Y"}],
    }
    sb = _FakeSandbox(json.dumps(task))
    res = asyncio.run(poll_analysis_status(sb, _TID, "ctx-1"))
    assert res["status"] == "completed"
    assert res["task_id"] == _TID
    assert res["context_id"] == "ctx-1"
    assert "r=0.9" in res["analysis_text"]
    assert res["artifacts"] == ["Findings"]


def test_poll_failed_surfaces_reason() -> None:
    sb = _FakeSandbox(json.dumps({"status": {"state": "failed", "message": "no numeric columns"}}))
    res = asyncio.run(poll_analysis_status(sb, _TID))
    assert res["status"] == "failed"
    assert res["reason"] == "no numeric columns"


def test_poll_input_required_relays_prompt() -> None:
    task = {"status": {"state": "input-required", "message": "Which column is the target?"}}
    sb = _FakeSandbox(json.dumps(task))
    res = asyncio.run(poll_analysis_status(sb, _TID, "ctx-2"))
    assert res["status"] == "input-required"
    assert res["prompt"] == "Which column is the target?"
    assert res["context_id"] == "ctx-2"


def test_poll_nonterminal_reports_running_with_progress() -> None:
    task = {"status": {"state": "working", "message": "fitting model"}}
    sb = _FakeSandbox(json.dumps(task))
    res = asyncio.run(poll_analysis_status(sb, _TID))
    assert res["status"] == "running"
    assert res["task_id"] == _TID
    assert res["progress"] == "fitting model"


class _UntruncatedSandbox:
    """Sandbox stub exposing only `aexecute_untruncated` (the preferred path)."""

    def __init__(self, output: str) -> None:
        self._output = output

    async def aexecute_untruncated(self, command: str, *, timeout: int | None = None):
        class _Resp:
            output = self._output

        return _Resp()


def test_poll_prefers_untruncated_execute() -> None:
    task = {"status": {"state": "completed"}, "artifacts": []}
    sb = _UntruncatedSandbox(json.dumps(task))
    res = asyncio.run(poll_analysis_status(sb, _TID))
    assert res["status"] == "completed"


# ---------------------------------------------------------------------------
# analysis_markdown + persist_analysis_outputs (P2.2 durability + export)
# ---------------------------------------------------------------------------

def test_analysis_markdown_renders_narrative_and_disclosure() -> None:
    md = analysis_markdown(
        "What drives yield?",
        {"status": "completed", "analysis_text": "Rainfall is the top driver.",
         "artifacts": ["Findings", "Correlation heatmap"]},
    )
    assert "# DataVoyager analysis: What drives yield?" in md
    assert "Rainfall is the top driver." in md
    assert "- Correlation heatmap" in md
    assert "AI-generated" in md  # org policy disclosure


class _WritableSandbox:
    """Sandbox stub recording awrite() files and aexecute() commands."""

    def __init__(self) -> None:
        self.files: dict[str, str] = {}
        self.commands: list[str] = []

    async def aget_work_dir(self) -> str:
        return "/workspace"

    async def awrite(self, path: str, content: str):
        self.files[path] = content

        class _Res:
            error = None

        return _Res()

    async def aexecute(self, command: str, *, timeout: int | None = None):
        self.commands.append(command)

        class _Resp:
            output = ""

        return _Resp()


_COMPLETED_RESULT = {
    "status": "completed",
    "task_id": _TID,
    "context_id": "ctx-9",
    "analysis_text": "Rainfall is the top driver (r=0.72).",
    "artifacts": ["Findings", "Correlation heatmap"],
}


def test_persist_completed_writes_files_and_exports() -> None:
    sb = _WritableSandbox()
    written = asyncio.run(
        persist_analysis_outputs(sb, _TID, "What drives yield?", _COMPLETED_RESULT)
    )
    assert f"/workspace/analysis/{_TID}.md" in written
    assert f"/workspace/analysis/{_TID}.json" in written
    # Files land under a NON-hidden dir so FileSyncMiddleware surfaces them.
    assert all(not p.startswith("/workspace/.") for p in written)
    parsed = json.loads(sb.files[f"/workspace/analysis/{_TID}.json"])
    assert parsed["question"] == "What drives yield?"
    # The export step ran in-sandbox (artifacts exporter invoked).
    assert any("asta artifacts --input" in c for c in sb.commands)


def test_persist_failed_writes_error_log_and_skips_export() -> None:
    sb = _WritableSandbox()
    result = {"status": "failed", "task_id": _TID, "reason": "no numeric columns"}
    written = asyncio.run(persist_analysis_outputs(sb, _TID, "Q", result))
    assert written == [f"/workspace/analysis/{_TID}.error.log"]
    assert "no numeric columns" in sb.files[written[0]]
    # No export for a failed run.
    assert not any("asta artifacts" in c for c in sb.commands)


def test_persist_is_noop_without_awrite() -> None:
    written = asyncio.run(persist_analysis_outputs(object(), _TID, "Q", _COMPLETED_RESULT))
    assert written == []


# --- a greeting is not a question (§237) --------------------------------------------------------

def test_a_greeting_is_refused_before_a_run_is_spent_on_it():
    """The run this comes from completed successfully and produced nothing.

    `question="hiiiiiiii"` loaded two tables, summarised them, and answered *"I'm sorry, but I
    can't yet answer that"*. Twenty minutes and a submission, spent because the only check was
    emptiness.
    """
    import asyncio
    import json as _json

    from backend.datavoyager_tools import MIN_QUESTION_CHARS, analyze_data

    for greeting in ["hiiiiiiii", "hi", "analyze", "do it", "?" * (MIN_QUESTION_CHARS - 1)]:
        answer = _json.loads(
            asyncio.run(analyze_data.ainvoke({"question": greeting, "dataset_paths": "a.csv"}))
        )
        assert answer["status"] == "error", greeting
        assert "too short" in answer["message"], greeting
        # The message has to say what a real one looks like, or it is a refusal without a remedy.
        assert "Name the dataset" in answer["message"]


def test_a_real_question_is_not_refused_for_its_length():
    """The threshold must not stand between a researcher and a legitimate run."""
    from backend.datavoyager_tools import MIN_QUESTION_CHARS

    real = (
        "Using SOC_Covariables_TrainValV5.csv, compare candidate models predicting SOC_MgHa "
        "from the covariables and report held-out metrics on SOC_Covariables_TESTV5.csv"
    )
    assert len(real) >= MIN_QUESTION_CHARS
    # And the threshold is low enough that a terse but genuine question survives it.
    assert len("Does yield vary with cultivar in trials.csv?") >= MIN_QUESTION_CHARS


# --- a task id is read, never scavenged (§238) --------------------------------------------------

def test_the_previous_response_is_cleared_before_a_submit():
    """`--output` names one path for every run, so a failed submit left the last one's JSON there.

    A run reported task `01a01fda-41b2-7d01-…` — a UUIDv7 in the shape of this app's own thread ids
    — and Asta answered `{"error": {"code": -32001, "message": "Task not found"}}`. The researcher
    waited on a run that did not exist.
    """
    from backend.datavoyager_tools import _SUBMIT_OUT, _submit_shell

    shell = _submit_shell("a real analytical question about x.csv", ["x.csv"], None)
    assert shell.startswith(f"rm -f {_SUBMIT_OUT}"), shell
    # And the clear must come before the submit, or it deletes the answer it just wrote.
    assert shell.index("rm -f") < shell.index("analyze-data")
    assert shell.rstrip().endswith(f"cat {_SUBMIT_OUT} 2>/dev/null")


def test_no_readable_response_is_a_failure_and_not_a_guess():
    """The old fallback searched the whole output for any UUID, and found one that was not a task."""
    import asyncio

    from backend.datavoyager_tools import _submit

    class Sandbox:
        async def aexecute(self, command, timeout=None):
            # No JSON, and a UUID sitting in the noise — exactly the shape that produced a
            # phantom task id.
            # The production shape: an object with `.output`, as the overlay's ExecuteResponse is.
            return SimpleNamespace(
                exit_code=1,
                output="cat: /tmp/asta-analyze-data-submit.json: No such file\n"
                "[cwd] /w run_id=01a01fda-41b2-7d01-80e6-db886bfbcdbb",
            )

    assert asyncio.run(_submit(Sandbox(), "a real analytical question", ["x.csv"], None)) is None


def test_a_parsed_response_still_yields_its_ids():
    """The working path has to keep working."""
    import asyncio
    import json as _json

    from backend.datavoyager_tools import _submit

    record = {"id": "4ee871fd-64cc-48a7-947b-6baca0e95e4c", "contextId": "a755964b-8cad-4078-9d8c-df8a3d0ea1c2"}

    class Sandbox:
        async def aexecute(self, command, timeout=None):
            return SimpleNamespace(exit_code=0, output=_json.dumps(record))

    got = asyncio.run(_submit(Sandbox(), "a real analytical question", ["x.csv"], None))
    assert got == {"task_id": record["id"], "context_id": record["contextId"]}


def test_a_uuid_outside_the_record_is_not_promoted_to_a_task_id():
    """Narrowed to the parsed record: an id under an unknown key is recoverable, ambient noise is not."""
    import asyncio
    import json as _json

    from backend.datavoyager_tools import _submit

    class Sandbox:
        async def aexecute(self, command, timeout=None):
            # Valid JSON, no usable id inside it, and a UUID in the trailing noise.
            return SimpleNamespace(
                exit_code=0,
                output=_json.dumps({"status": "accepted"})
                + "\n[cwd] /w run_id=01a01fda-41b2-7d01-80e6-db886bfbcdbb",
            )

    assert asyncio.run(_submit(Sandbox(), "a real analytical question", ["x.csv"], None)) is None


def test_a_dict_shaped_response_is_read_too():
    """`_exit_and_output` handles both shapes and `_run` handled one, so a dict became "" —
    indistinguishable from a command that printed nothing. §224, one module over."""
    import asyncio
    import json as _json

    from backend.datavoyager_tools import _submit

    record = {"id": "4ee871fd-64cc-48a7-947b-6baca0e95e4c", "contextId": "ctx-1"}

    class Sandbox:
        async def aexecute(self, command, timeout=None):
            return {"exit_code": 0, "output": _json.dumps(record)}

    got = asyncio.run(_submit(Sandbox(), "a real analytical question", ["x.csv"], None))
    assert got == {"task_id": record["id"], "context_id": "ctx-1"}

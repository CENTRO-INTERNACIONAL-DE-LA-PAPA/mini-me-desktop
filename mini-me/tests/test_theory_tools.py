"""Guards for the Asta Theorizer polling tool (`backend.theory_tools`).

The deployed bug that motivated the `generate_theories` tool was the agent
hand-building the CLI with a non-existent ``--question`` flag (and then the
coordinator fabricating a theory when it failed). These tests pin the real
`asta` v0.101.0 contract — the `literature-theory-generation` subcommand and the
`--theory-query` flag — so flag drift is caught in CI, not in a live run.
"""

from __future__ import annotations

import asyncio
import json
import subprocess
import sys

from backend.sandbox import EXECUTE_OUTPUT_MAX_BYTES
from backend.theory_tools import (
    _REDUCE_TASK_PY,
    _build_submit_command,
    _extract_json,
    _failure_reason,
    _parse_theories,
    _poll_command,
    _run_elapsed_seconds,
    _state_of,
    is_valid_task_id,
    persist_theory_outputs,
    poll_theory_status,
    theories_markdown,
)


# ---------------------------------------------------------------------------
# CLI contract — the part that actually broke in production
# ---------------------------------------------------------------------------

def test_submit_command_uses_real_subcommand_and_query_flag() -> None:
    args = _build_submit_command("why is the sky blue?", 30, do_novelty=False)
    assert args[:3] == ["asta", "generate-theories", "literature-theory-generation"]
    # The query travels under --theory-query, NOT the invalid --question.
    assert "--theory-query" in args
    assert args[args.index("--theory-query") + 1] == "why is the sky blue?"
    assert "--question" not in args, "regression: --question is not a real asta flag"
    assert "--no-wait" in args


def test_submit_command_toggles_novelty_flag() -> None:
    off = _build_submit_command("q", 30, do_novelty=False)
    assert "--no-do-qualified-novelty-evaluation" in off
    on = _build_submit_command("q", 30, do_novelty=True)
    assert "--no-do-qualified-novelty-evaluation" not in on


def test_submit_command_passes_max_papers() -> None:
    args = _build_submit_command("q", 25, do_novelty=False)
    assert "--max-papers-to-retrieve" in args
    assert args[args.index("--max-papers-to-retrieve") + 1] == "25"


# ---------------------------------------------------------------------------
# Artifact parsing — laws + linked papers, no fabrication
# ---------------------------------------------------------------------------

def _theory_artifact(name, laws, papers):
    content = [{"id": "c0", "type": "SECTIONS", "title": "3 Theory Statements",
                "childIds": [f"law{i}" for i in range(len(laws))]}]
    for i, law in enumerate(laws):
        content.append({"id": f"law{i}", "type": "SECTION", "title": law, "childIds": []})
    entities = {f"paper-{i}": p for i, p in enumerate(papers)}
    return {
        "metadata": {"type": "theory"},
        "parts": [{"data": {"name": name, "content": content, "entities": entities}}],
    }


def test_parse_theories_builds_laws_and_linked_papers() -> None:
    task = {
        "artifacts": [
            _theory_artifact(
                "Theory A",
                ["X causes Y", "A increases B"],
                [
                    {"displayLabel": "Doe 2020", "s2Metadata": {"corpusId": 12345}},
                    {"displayLabel": "Roe 2019",
                     "s2Metadata": {"externalIds": {"DOI": "10.1/xyz"}}},
                ],
            )
        ]
    }
    out = _parse_theories(task)
    assert len(out["theories"]) == 1
    t = out["theories"][0]
    assert t["laws"] == ["X causes Y", "A increases B"]
    urls = {p["url"] for p in t["supporting_papers"]}
    # Corpus-id links go through the S2 API redirect (the website /paper/
    # CorpusID: path resolves to the wrong paper).
    assert "https://api.semanticscholar.org/CorpusID:12345" in urls
    assert "https://doi.org/10.1/xyz" in urls
    assert out["papers_reviewed"] == 2
    # We never fabricate a support/conflict split from prose.
    assert t["conflicting_papers"] == []


def test_parse_theories_excludes_non_theory_artifacts() -> None:
    task = {"artifacts": [
        {"metadata": {"type": "novelty"}, "parts": [{"data": {}}]},
        {"metadata": {"type": "extraction-schema"}, "parts": [{"data": {}}]},
    ]}
    out = _parse_theories(task)
    assert out["theories"] == []
    assert out["papers_reviewed"] == 0


def test_parse_theories_falls_back_to_name_when_no_law_sections() -> None:
    art = {"metadata": {"type": "theory"},
           "parts": [{"data": {"name": "Fallback theory", "content": [], "entities": {}}}]}
    out = _parse_theories({"artifacts": [art]})
    assert out["theories"][0]["laws"] == ["Fallback theory"]


# ---------------------------------------------------------------------------
# Robustness helpers
# ---------------------------------------------------------------------------

def test_extract_json_tolerates_stderr_suffix() -> None:
    merged = '{"status": {"state": "working"}}\n[stderr]\nINFO some log line'
    parsed = _extract_json(merged)
    assert parsed is not None
    assert _state_of(parsed) == "working"


def test_extract_json_returns_none_on_garbage() -> None:
    assert _extract_json("not json at all") is None
    assert _extract_json("") is None


def test_state_of_handles_none() -> None:
    assert _state_of(None) is None
    assert _state_of({}) is None


def test_run_elapsed_seconds_from_started_at() -> None:
    assert _run_elapsed_seconds(None) is None
    assert _run_elapsed_seconds({"status": {}}) is None
    task = {"status": {"message": {"parts": [
        {"data": {"started_at": "2020-01-01T00:00:00+00:00"}}]}}}
    val = _run_elapsed_seconds(task)
    assert isinstance(val, int) and val > 0


def test_is_valid_task_id() -> None:
    assert is_valid_task_id("6580ec74-121a-4757-b5e2-2e1ed9fc210e")
    assert not is_valid_task_id("")
    assert not is_valid_task_id("../etc/passwd")
    assert not is_valid_task_id("not-a-uuid")


# ---------------------------------------------------------------------------
# poll_theory_status — the core the status route serves
# ---------------------------------------------------------------------------

class _FakeSandbox:
    """Minimal sandbox stub whose aexecute returns a canned CLI output."""

    def __init__(self, output: str) -> None:
        self._output = output

    async def aexecute(self, command: str, *, timeout: int | None = None):  # noqa: D401
        class _Resp:
            output = self._output

        return _Resp()


def test_poll_theory_status_completed_parses_theories() -> None:
    task = {
        "status": {"state": "completed"},
        "artifacts": [
            _theory_artifact("T", ["X causes Y"],
                             [{"displayLabel": "Doe 2020", "s2Metadata": {"corpusId": 99}}])
        ],
    }
    sb = _FakeSandbox(json.dumps(task))
    res = asyncio.run(poll_theory_status(sb, "6580ec74-121a-4757-b5e2-2e1ed9fc210e"))
    assert res["status"] == "completed"
    assert len(res["theories"]) == 1
    assert res["theories"][0]["supporting_papers"][0]["url"].endswith("/CorpusID:99")


def test_poll_theory_status_running() -> None:
    sb = _FakeSandbox(json.dumps({"status": {"state": "working"}}))
    res = asyncio.run(poll_theory_status(sb, "6580ec74-121a-4757-b5e2-2e1ed9fc210e"))
    assert res["status"] == "running"
    assert "theories" not in res


def test_poll_theory_status_failed() -> None:
    sb = _FakeSandbox(json.dumps({"status": {"state": "failed"}}))
    res = asyncio.run(poll_theory_status(sb, "6580ec74-121a-4757-b5e2-2e1ed9fc210e"))
    assert res["status"] == "failed"


class _UntruncatedSandbox:
    """Sandbox stub that only exposes `aexecute_untruncated` (the poll path)."""

    def __init__(self, output: str) -> None:
        self._output = output

    async def aexecute_untruncated(self, command: str, *, timeout: int | None = None):
        class _Resp:
            output = self._output

        return _Resp()


def test_run_prefers_untruncated_execute() -> None:
    # The poll output is parsed server-side and must never be capped, so `_run`
    # must use the untruncated path when the sandbox offers it.
    task = {"status": {"state": "completed"}, "artifacts": []}
    sb = _UntruncatedSandbox(json.dumps(task))
    res = asyncio.run(poll_theory_status(sb, "6580ec74-121a-4757-b5e2-2e1ed9fc210e"))
    assert res["status"] == "completed"


# ---------------------------------------------------------------------------
# The truncation regression: a real completed task record is ~500 KB (it embeds
# the paper store + per-paper extraction markdown). Piped raw through the
# execute cap it becomes unparseable JSON, and the poll reports "running"
# forever — the bug that left the Theories card generating for 20 h. The
# in-sandbox reducer must strip the bulk BEFORE it crosses back.
# ---------------------------------------------------------------------------

def _giant_completed_task() -> dict:
    """A completed task shaped like the real CLI output, padded past the cap."""
    bulk = "x" * 500_000  # stand-in for paperstore + extraction markdown
    return {
        "id": "6580ec74-121a-4757-b5e2-2e1ed9fc210e",
        "status": {"state": "completed", "message": {"parts": [{"data": {}}]}},
        "artifacts": [
            {"metadata": {"type": "extraction-schema"}, "parts": [{"data": {"blob": bulk}}]},
            {"metadata": {"type": "theory_store"},
             "parts": [{"data": {"paperstore": bulk, "extraction_results": bulk}}]},
            {"metadata": {"type": "extraction"}, "parts": [{"data": {"markdown": bulk}}]},
            _theory_artifact(
                "Nitrogen co-limitation theory",
                ["N response is gated by co-limiting nutrients"],
                [{"displayLabel": "Doe 2021",
                  "s2Metadata": {"corpusId": 286404335, "title": "Blended fertilizers"}}],
            ),
        ],
    }


def _run_reducer(task: dict) -> str:
    """Run the real in-sandbox reducer script over `task` via python3."""
    proc = subprocess.run(
        [sys.executable, "-c", _REDUCE_TASK_PY],
        input=json.dumps(task),
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, proc.stderr
    return proc.stdout


def test_truncated_giant_task_is_unparseable() -> None:
    # Pin the failure mode: the raw record clipped to the execute cap is not
    # valid JSON, so the old code fell through to a permanent "running".
    raw = json.dumps(_giant_completed_task())
    assert len(raw.encode()) > EXECUTE_OUTPUT_MAX_BYTES
    clipped = raw[:EXECUTE_OUTPUT_MAX_BYTES]
    assert _extract_json(clipped) is None
    assert _state_of(_extract_json(clipped)) is None


def test_reducer_shrinks_giant_task_under_cap_and_keeps_theories() -> None:
    reduced_out = _run_reducer(_giant_completed_task())
    # The reduced payload comfortably fits the execute cap...
    assert len(reduced_out.encode()) < EXECUTE_OUTPUT_MAX_BYTES
    # ...and the multi-hundred-KB bulk artifacts are gone.
    assert "theory_store" not in reduced_out
    assert "paperstore" not in reduced_out
    # ...while the state and theories survive intact for the client parser.
    task = _extract_json(reduced_out)
    assert task is not None
    assert _state_of(task) == "completed"
    parsed = _parse_theories(task)
    assert len(parsed["theories"]) == 1
    assert parsed["theories"][0]["laws"] == [
        "N response is gated by co-limiting nutrients"
    ]
    assert parsed["theories"][0]["supporting_papers"][0]["url"].endswith("/CorpusID:286404335")


def test_poll_command_pipes_task_through_reducer() -> None:
    cmd = _poll_command("6580ec74-121a-4757-b5e2-2e1ed9fc210e")
    assert "asta generate-theories task" in cmd
    assert "6580ec74-121a-4757-b5e2-2e1ed9fc210e" in cmd
    assert "python3 -c" in cmd
    # stderr (progress stream) is dropped so only the task JSON reaches python.
    assert "2>/dev/null" in cmd


# ---------------------------------------------------------------------------
# Failure reason — the card showed a useless "Theorizer task failed." because
# we only read status.message.short_desc; probe the other spots a2a uses.
# ---------------------------------------------------------------------------

def test_failure_reason_reads_text_part() -> None:
    task = {"status": {"message": {"parts": [{"kind": "text", "text": "PaperFinder timed out"}]}}}
    assert _failure_reason(task, "failed") == "PaperFinder timed out"


def test_failure_reason_reads_data_error_key() -> None:
    task = {"status": {"message": {"parts": [{"data": {"error": "gateway 503"}}]}}}
    assert _failure_reason(task, "failed") == "gateway 503"


def test_failure_reason_falls_back_to_honest_generic() -> None:
    # Asta commonly fails with a bare status and no reason; say so (and that it's
    # Asta-side, not Mini-Me) instead of the old useless "Theorizer task failed."
    failed = _failure_reason(None, "failed")
    assert "without reporting a reason" in failed
    assert "inside Asta" in failed
    assert "canceled" in _failure_reason({"status": {}}, "canceled")


def test_poll_theory_status_failed_surfaces_real_reason() -> None:
    task = {"status": {"state": "failed",
                       "message": {"parts": [{"data": {"short_desc": "no papers found"}}]}}}
    sb = _FakeSandbox(json.dumps(task))
    res = asyncio.run(poll_theory_status(sb, "6580ec74-121a-4757-b5e2-2e1ed9fc210e"))
    assert res["status"] == "failed"
    assert res["reason"] == "no papers found"


def test_failure_reason_reads_plain_string_message() -> None:
    # Some a2a builds set status.message to a bare string, not a parts list.
    task = {"status": {"message": "quota exceeded for this org"}}
    assert _failure_reason(task, "failed") == "quota exceeded for this org"


def test_failure_reason_reads_top_level_error_fields() -> None:
    # The reducer carries a top-level error through as `error_fields` when the
    # detail lives outside status.message — the shape that produced the useless
    # generic "Theorizer task failed." in the field.
    task = {"status": {"state": "failed"},
            "error_fields": {"error": "internal theorizer error: 500"}}
    assert _failure_reason(task, "failed") == "internal theorizer error: 500"


def test_failure_reason_reports_progress_for_bare_status() -> None:
    # The real failed run: bare status, no reason, but the reducer forwarded the
    # artifact types it produced before dying. Report that mid-run progress.
    task = {"status": {"state": "failed"},
            "artifact_types": ["extraction-schema", "theory_store"]}
    reason = _failure_reason(task, "failed")
    assert "extraction-schema" in reason and "theory_store" in reason
    assert "no theories" in reason
    assert "inside Asta" in reason


def test_reducer_forwards_top_level_error() -> None:
    # A failed record whose reason is a top-level `error` (some a2a builds): the
    # reducer must carry it so _failure_reason surfaces it verbatim.
    failed_task = {
        "status": {"state": "failed", "message": None},
        "error": "theorizer worker crashed",
        "artifacts": [],
    }
    reduced = _extract_json(_run_reducer(failed_task))
    assert reduced is not None
    assert _state_of(reduced) == "failed"
    assert _failure_reason(reduced, "failed") == "theorizer worker crashed"


def test_reducer_forwards_artifact_types_from_real_failed_record() -> None:
    # Mirrors the real record: state=failed, status.message=null, no error, but
    # intermediate artifacts present. The reducer forwards their types and drops
    # their bulk (theory_store payloads are large).
    failed_task = {
        "status": {"state": "failed", "timestamp": "2026-07-14T16:43:38+00:00"},
        "history": [{"role": "agent", "parts": [{"kind": "text", "text": "Task accepted"}]}],
        "artifacts": [
            {"name": "Extraction Schema", "metadata": {"type": "extraction-schema"},
             "parts": [{"kind": "data", "data": {"blob": "x" * 5000}}]},
            {"name": "Theory Store", "metadata": {"type": "theory_store"},
             "parts": [{"kind": "data", "data": {"blob": "y" * 400000}}]},
        ],
    }
    reduced = _extract_json(_run_reducer(failed_task))
    assert reduced is not None
    assert _state_of(reduced) == "failed"
    assert reduced["artifact_types"] == ["extraction-schema", "theory_store"]
    # Bulk payloads are gone (only theory-type artifacts are kept, and there were
    # none), so the reduced record is tiny despite the ~400 KB theory_store.
    assert "theory_store" not in json.dumps(reduced.get("artifacts"))
    assert len(json.dumps(reduced).encode()) < EXECUTE_OUTPUT_MAX_BYTES
    reason = _failure_reason(reduced, "failed")
    assert "extraction-schema" in reason and "no theories" in reason


def test_reducer_does_not_bloat_completed_task() -> None:
    # The failure passthroughs are gated on non-completed state, so a normal
    # completed record grows no new keys.
    reduced = _extract_json(_run_reducer(_giant_completed_task()))
    assert reduced is not None
    assert "error_fields" not in reduced
    assert "artifact_types" not in reduced


# ---------------------------------------------------------------------------
# Persisting theories to the sandbox so the agent can read them later.
# ---------------------------------------------------------------------------

class _WritableSandbox:
    """Sandbox stub that records awrite() calls into an in-memory dict."""

    def __init__(self) -> None:
        self.files: dict[str, str] = {}

    async def aget_work_dir(self) -> str:
        return "/workspace"

    async def awrite(self, path: str, content: str):
        self.files[path] = content

        class _Res:
            error = None

        return _Res()


_COMPLETED_RESULT = {
    "status": "completed",
    "task_id": "6580ec74-121a-4757-b5e2-2e1ed9fc210e",
    "theories": [
        {"laws": ["N gates yield"],
         "supporting_papers": [{"citation": "Doe 2021",
                                "url": "https://www.semanticscholar.org/paper/CorpusID:99"}],
         "conflicting_papers": []}
    ],
    "knowledge_gaps": [],
    "papers_reviewed": 3,
}


def test_theories_markdown_renders_laws_and_links() -> None:
    md = theories_markdown("How does N affect yield?", _COMPLETED_RESULT)
    assert "# Theories: How does N affect yield?" in md
    assert "- N gates yield" in md
    assert "[Doe 2021](https://www.semanticscholar.org/paper/CorpusID:99)" in md
    assert "AI-generated" in md  # org policy disclosure


def test_persist_completed_writes_md_and_json() -> None:
    sb = _WritableSandbox()
    written = asyncio.run(
        persist_theory_outputs(sb, "6580ec74-121a-4757-b5e2-2e1ed9fc210e",
                               "How does N affect yield?", _COMPLETED_RESULT)
    )
    assert "/workspace/theories/6580ec74-121a-4757-b5e2-2e1ed9fc210e.md" in written
    assert "/workspace/theories/6580ec74-121a-4757-b5e2-2e1ed9fc210e.json" in written
    # Files land under a NON-hidden dir so FileSyncMiddleware surfaces them.
    assert all(not p.startswith("/workspace/.") for p in written)
    parsed = json.loads(sb.files["/workspace/theories/6580ec74-121a-4757-b5e2-2e1ed9fc210e.json"])
    assert parsed["question"] == "How does N affect yield?"
    assert len(parsed["theories"]) == 1


def test_persist_failed_writes_error_log_with_reason() -> None:
    sb = _WritableSandbox()
    result = {"status": "failed",
              "task_id": "6580ec74-121a-4757-b5e2-2e1ed9fc210e",
              "reason": "PaperFinder returned no papers."}
    written = asyncio.run(
        persist_theory_outputs(sb, "6580ec74-121a-4757-b5e2-2e1ed9fc210e", "Q", result)
    )
    assert written == ["/workspace/theories/6580ec74-121a-4757-b5e2-2e1ed9fc210e.error.log"]
    assert "PaperFinder returned no papers." in sb.files[written[0]]


def test_persist_is_noop_without_awrite() -> None:
    # A sandbox stub lacking awrite (e.g. a test double) must not raise.
    written = asyncio.run(
        persist_theory_outputs(object(), "6580ec74-121a-4757-b5e2-2e1ed9fc210e",
                               "Q", _COMPLETED_RESULT)
    )
    assert written == []

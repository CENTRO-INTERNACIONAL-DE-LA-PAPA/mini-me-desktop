"""Asta DataVoyager submission tool + status polling for the data_voyager subagent.

DataVoyager (`asta analyze-data`) runs a multi-agent data-science pipeline: it
writes and executes code against a local tabular dataset in a sandboxed notebook
and answers a research question — generating and *testing* hypotheses against the
data. It is a long, async A2A job (a few minutes for a simple EDA, 20–40 min for
multi-step modelling), so — exactly like the theorizer — Mini-Me does NOT block a
chat turn on it.

`analyze_data` just *submits* the run and returns immediately with a `task_id` and
`context_id` and `status="running"`. The subagent emits a `running`
DataAnalysisResults (which populates the Analysis card as a live progress state),
and the frontend polls the `/analyze-data/{thread}/{task}` route — which calls
`poll_analysis_status` here — until the run reaches a terminal state. Status is a
*cheap* fetch (`asta analyze-data task <id>`, not the blocking `poll`), so the
route stays fast and survives sandbox restarts (the run itself lives on Asta's
hosted service).

On completion the run's outputs are made durable in the sandbox
(`persist_analysis_outputs`): a readable `analysis/<task_id>.md` + `.json`, plus a
full `asta artifacts` export of the charts / notebook / tables under
`analysis/<task_id>/` so ``FileSyncMiddleware`` surfaces them in the UI and the
agent can `read_file` the results on a later turn.

The CLI contract (subcommands + flags) lives in pure, unit-tested builders so flag
drift is caught in CI rather than in a live run — the same discipline that
``backend.theory_tools`` applies to the theorizer.
"""

import json
import logging
import re
import shlex
from typing import Any

from langchain_core.tools import tool

from backend.runtime import _active_sandbox

logger = logging.getLogger(__name__)

_SUBMIT_TIMEOUT_S = 180
_STATUS_TIMEOUT_S = 90
_EXPORT_TIMEOUT_S = 120
_UUID_RE = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")

# Sandbox path the submit response JSON (task id + context id) is written to.
# Fixed name — successive submits overwrite it; transient CLI scratch.
_SUBMIT_OUT = "/tmp/asta-analyze-data-submit.json"

# Cap on how much analysis narrative we forward from a status poll. The full task
# record embeds the notebook + tables and is large; the durable export + the
# persisted file carry the detail, so a bounded blob is enough for the card.
_TEXT_CAP = 12_000


# ---------------------------------------------------------------------------
# CLI contract — pure argv builders (unit-tested; kept out of the model's hands)
# ---------------------------------------------------------------------------

def _build_submit_command(
    question: str,
    dataset_paths: list[str],
    *,
    context_id: str | None = None,
    output_path: str = _SUBMIT_OUT,
) -> list[str]:
    """Build the argv for submitting a DataVoyager run (asta CLI v0.101.x).

    Kept pure (no I/O) so the CLI contract is testable. The subcommand is
    ``analyze-data submit``; the response JSON (task ``id`` + session
    ``contextId``) is written to ``--output``. A follow-up against the same
    workspace passes ``--context-id``; new datasets attach to that context, and
    with no files the agent reuses what is already there.
    """
    args = ["asta", "analyze-data", "submit", "--output", output_path]
    if context_id:
        args += ["--context-id", context_id]
    # Question is positional and must precede the dataset paths.
    args.append(question)
    args.extend(dataset_paths)
    return args


def _build_task_command(task_id: str) -> list[str]:
    """Build the argv for a cheap, non-blocking status fetch of a task.

    ``asta analyze-data task <id>`` prints the task's current state and artifacts
    and returns immediately (unlike ``poll``, which blocks until terminal). This
    is the analogue of the theorizer's ``generate-theories task <id>``.
    """
    return ["asta", "analyze-data", "task", task_id]


# In-sandbox reducer for `asta analyze-data task <id>`. A record with the executed
# notebook + data tables is large and would blow past the execute cap. We keep
# only what the status parser needs: the task ``status`` (state + message) and a
# compact per-artifact ``{name, type, text}`` where ``text`` is gathered from
# human-readable parts and truncated. File bytes / giant data blobs / notebook
# source are dropped BEFORE the payload crosses back. Shape-tolerant: it only
# reaches into the documented A2A fields and drops anything it does not recognize,
# so a schema drift degrades to empty text rather than crashing. Apostrophe-free
# so it survives shell single-quoting.
_REDUCE_TASK_PY = """
import sys, json
CAP = 4000
try:
    t = json.load(sys.stdin)
except Exception:
    sys.exit(0)
def part_text(p):
    if not isinstance(p, dict):
        return ""
    txt = p.get("text")
    if isinstance(txt, str) and txt.strip():
        return txt
    d = p.get("data")
    if isinstance(d, dict):
        outs = []
        for k in ("summary", "text", "markdown", "description", "message", "short_desc"):
            v = d.get(k)
            if isinstance(v, str) and v.strip():
                outs.append(v)
        return "\\n".join(outs)
    if isinstance(d, str):
        return d
    return ""
arts = []
for a in t.get("artifacts", []) or []:
    if not isinstance(a, dict):
        continue
    texts = [part_text(p) for p in (a.get("parts") or [])]
    text = "\\n".join(s for s in texts if s)[:CAP]
    arts.append({
        "name": a.get("name"),
        "type": (a.get("metadata") or {}).get("type"),
        "text": text,
    })
out = {"status": t.get("status", {}) or {}, "artifacts": arts}
ef = {}
for k in ("error", "detail", "reason", "message"):
    v = t.get(k)
    if isinstance(v, str) and v.strip():
        ef[k] = v
if ef:
    out["error_fields"] = ef
print(json.dumps(out))
"""


def _submit_shell(
    question: str, dataset_paths: list[str], context_id: str | None
) -> str:
    """Shell: submit, discard the progress stream, cat the response JSON back."""
    argv = _build_submit_command(question, dataset_paths, context_id=context_id)
    cmd = " ".join(shlex.quote(a) for a in argv)
    return f"{cmd} >/dev/null 2>&1; cat {shlex.quote(_SUBMIT_OUT)}"


def _task_shell(task_id: str) -> str:
    """Shell: fetch the task and reduce it in-sandbox to a small record.

    asta streams progress to stderr and the task JSON to stdout, so drop stderr
    and pipe stdout through the reducer. ``python3`` ships with the sandbox image.
    """
    fetch = " ".join(shlex.quote(a) for a in _build_task_command(task_id))
    return f"{fetch} 2>/dev/null | python3 -c {shlex.quote(_REDUCE_TASK_PY)}"


def _export_shell(task_id: str, base_dir: str) -> str:
    """Shell: fetch the FULL task record in-sandbox and export its artifacts.

    Keeps the large record entirely inside the sandbox (never pulled back to the
    server): dump `asta analyze-data task` to ``<base>/<id>/task.json``, then
    ``asta artifacts`` renders the charts/notebook/tables to
    ``<base>/<id>/export`` as markdown. Those files (non-hidden) are picked up by
    ``FileSyncMiddleware``.
    """
    tid = shlex.quote(task_id)
    run_dir = shlex.quote(f"{base_dir}/{task_id}")
    export_dir = shlex.quote(f"{base_dir}/{task_id}/export")
    return (
        f"mkdir -p {run_dir} && "
        f"asta analyze-data task {tid} > {run_dir}/task.json 2>/dev/null && "
        f"asta artifacts --input {run_dir} --output {export_dir} --format md 2>/dev/null"
    )


# ---------------------------------------------------------------------------
# Parsing helpers (pure; fed synthetic A2A records in tests)
# ---------------------------------------------------------------------------

def _extract_json(output: str) -> dict[str, Any] | None:
    """Pull the JSON task record out of merged stdout/stderr output."""
    if not output:
        return None
    head = output.split("[stderr]", 1)[0].strip()
    for candidate in (head, output):
        try:
            return json.loads(candidate)
        except Exception:
            pass
        start, end = candidate.find("{"), candidate.rfind("}")
        if start != -1 and end > start:
            try:
                return json.loads(candidate[start : end + 1])
            except Exception:
                continue
    return None


def _state_of(task: dict[str, Any] | None) -> str | None:
    return ((task or {}).get("status") or {}).get("state")


def is_valid_task_id(task_id: str) -> bool:
    """True if `task_id` is a well-formed A2A task UUID (guards the poll route)."""
    return bool(task_id) and bool(_UUID_RE.fullmatch(task_id))


def _status_message_text(task: dict[str, Any] | None) -> str:
    """Human-readable text from ``status.message`` (string, or a2a parts)."""
    message = ((task or {}).get("status") or {}).get("message")
    if isinstance(message, str):
        return message.strip()
    parts = message.get("parts") if isinstance(message, dict) else None
    out: list[str] = []
    for part in parts or []:
        if not isinstance(part, dict):
            continue
        text = part.get("text")
        if isinstance(text, str) and text.strip():
            out.append(text.strip())
        data = part.get("data")
        if isinstance(data, dict):
            for key in ("summary", "text", "short_desc", "message"):
                val = data.get(key)
                if isinstance(val, str) and val.strip():
                    out.append(val.strip())
    return "\n".join(out)


def _progress_of(task: dict[str, Any] | None) -> str:
    """Short in-progress description from the task's status message, if any."""
    return _status_message_text(task)[:280]


def _analysis_text(task: dict[str, Any] | None) -> str:
    """Concatenate the narrative the run produced, bounded, for the card/subagent."""
    chunks: list[str] = []
    for art in (task or {}).get("artifacts", []) or []:
        if not isinstance(art, dict):
            continue
        text = art.get("text")
        if isinstance(text, str) and text.strip():
            label = art.get("name") or art.get("type") or "artifact"
            chunks.append(f"## {label}\n{text.strip()}")
    msg = _status_message_text(task)
    if msg:
        chunks.append(msg)
    return "\n\n".join(chunks)[:_TEXT_CAP]


def _artifact_names(task: dict[str, Any] | None) -> list[str]:
    """Names (or types) of the artifacts the run produced."""
    names: list[str] = []
    for art in (task or {}).get("artifacts", []) or []:
        if not isinstance(art, dict):
            continue
        label = art.get("name") or art.get("type")
        if isinstance(label, str) and label.strip():
            names.append(label.strip())
    return names


def _failure_reason(task: dict[str, Any] | None, state: str) -> str:
    """Best-effort human-readable reason a DataVoyager run ended non-successfully."""
    msg = _status_message_text(task)
    if msg:
        return msg
    ef = (task or {}).get("error_fields") if isinstance(task, dict) else None
    if isinstance(ef, dict):
        for key in ("reason", "error", "detail", "message"):
            val = ef.get(key)
            if isinstance(val, str) and val.strip():
                return val.strip()
    return (
        f"Asta ended the DataVoyager run as {state} without reporting a reason. "
        "The run failed inside Asta's own service, not Mini-Me. Retry once; if it "
        "keeps failing, DataVoyager is unavailable for your account right now."
    )


# ---------------------------------------------------------------------------
# Sandbox IO
# ---------------------------------------------------------------------------

async def _run(sandbox: Any, command: str, timeout: int) -> str:
    # Prefer the untruncated path: the reduced task record is parsed server-side
    # (never fed to the model) and a truncated record is unparseable JSON. Fall
    # back to `aexecute` for stubs/sandboxes lacking the untruncated method.
    runner = getattr(sandbox, "aexecute_untruncated", None) or sandbox.aexecute
    resp = await runner(command, timeout=timeout)
    return getattr(resp, "output", "") or ""


async def _submit(
    sandbox: Any, question: str, dataset_paths: list[str], context_id: str | None
) -> dict[str, str] | None:
    """Submit a run; return the task id + context id, or None on failure."""
    out = await _run(sandbox, _submit_shell(question, dataset_paths, context_id), _SUBMIT_TIMEOUT_S)
    record = _extract_json(out)
    task_id = ""
    ctx = context_id or ""
    if isinstance(record, dict):
        task_id = str(record.get("id") or "")
        ctx = str(record.get("contextId") or ctx)
    if not is_valid_task_id(task_id):
        match = _UUID_RE.search(out)
        task_id = match.group(0) if match else ""
    if not is_valid_task_id(task_id):
        return None
    return {"task_id": task_id, "context_id": ctx}


async def poll_analysis_status(
    sandbox: Any, task_id: str, context_id: str = ""
) -> dict[str, Any]:
    """One cheap status fetch of a DataVoyager task; parse it when terminal.

    Returns a dict shaped for the frontend / subagent:
      completed      -> {status, task_id, context_id, analysis_text, artifacts}
      failed         -> {status:"failed", task_id, reason}
      input-required -> {status:"input-required", task_id, context_id, prompt}
      running        -> {status:"running", task_id, context_id, progress}
    """
    out = await _run(sandbox, _task_shell(task_id), _STATUS_TIMEOUT_S)
    task = _extract_json(out)
    state = _state_of(task)
    if state == "completed" and task is not None:
        return {
            "status": "completed",
            "task_id": task_id,
            "context_id": context_id,
            "analysis_text": _analysis_text(task),
            "artifacts": _artifact_names(task),
        }
    if state in ("failed", "canceled", "rejected"):
        reason = _failure_reason(task, state)
        logger.warning("analyze-data task %s ended %s: %s", task_id, state, reason)
        return {
            "status": "failed" if state == "rejected" else state,
            "task_id": task_id,
            "reason": reason,
        }
    if state == "input-required":
        return {
            "status": "input-required",
            "task_id": task_id,
            "context_id": context_id,
            "prompt": _status_message_text(task)
            or "DataVoyager needs more input to continue.",
        }
    return {
        "status": "running",
        "task_id": task_id,
        "context_id": context_id,
        "progress": _progress_of(task),
    }


def analysis_markdown(question: str, result: dict[str, Any]) -> str:
    """Render a completed poll result as a readable, self-contained analysis doc.

    Written to the sandbox so the agent (which has filesystem tools) can read and
    reason over the analysis on a later turn, and so the run is a durable record
    alongside the exported charts/notebook.
    """
    lines = [
        f"# DataVoyager analysis: {question}".rstrip(),
        "",
        "_Generated by the Asta DataVoyager agent. AI-generated; review with a "
        "subject-matter expert before use._",
        "",
    ]
    text = (result.get("analysis_text") or "").strip()
    if text:
        lines.append(text)
        lines.append("")
    artifacts = result.get("artifacts") or []
    if artifacts:
        lines.append("## Produced artifacts")
        lines.extend(f"- {name}" for name in artifacts)
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


async def persist_analysis_outputs(
    sandbox: Any, task_id: str, question: str, result: dict[str, Any]
) -> list[str]:
    """Write a terminal poll result to the sandbox as durable, agent-readable files.

    Completed runs get ``analysis/<task_id>.md`` (readable) + ``.json`` (structured)
    and a full ``asta artifacts`` export of the charts/notebook/tables under
    ``analysis/<task_id>/export`` (surfaced by ``FileSyncMiddleware``); failures get
    ``analysis/<task_id>.error.log`` carrying the real reason. Files land under a
    NON-hidden dir so they appear as artifacts and the agent can read them.
    Best-effort: a write/export failure is logged, never raised into the response.
    """
    awrite = getattr(sandbox, "awrite", None)
    if awrite is None:
        return []
    try:
        work_dir = await sandbox.aget_work_dir()
    except Exception:
        work_dir = "/workspace"
    base = f"{work_dir}/analysis"
    status = result.get("status")
    written: list[str] = []
    try:
        if status == "completed":
            targets = {
                f"{base}/{task_id}.md": analysis_markdown(question, result),
                f"{base}/{task_id}.json": json.dumps(
                    {"question": question, **result}, indent=2, ensure_ascii=False
                ),
            }
        else:
            reason = result.get("reason") or f"DataVoyager task {status}."
            targets = {
                f"{base}/{task_id}.error.log": (
                    f"task_id: {task_id}\n"
                    f"question: {question}\n"
                    f"status: {status}\n"
                    f"reason: {reason}\n"
                )
            }
        for path, content in targets.items():
            res = await awrite(path, content)
            err = getattr(res, "error", None)
            if err:
                logger.warning("failed to persist %s: %s", path, err)
            else:
                written.append(path)
    except Exception as exc:  # noqa: BLE001
        logger.warning("persist_analysis_outputs failed for %s: %s", task_id, exc)

    # Export the run's charts/notebook/tables to disk (completed runs only). Done
    # entirely in-sandbox so the large record never crosses back; best-effort.
    if status == "completed":
        try:
            await sandbox.aexecute(_export_shell(task_id, base), timeout=_EXPORT_TIMEOUT_S)
            written.append(f"{base}/{task_id}/export")
        except Exception as exc:  # noqa: BLE001
            logger.warning("analyze-data export failed for %s: %s", task_id, exc)
    return written


def _split_paths(dataset_paths: str) -> list[str]:
    """Split a comma/newline/space-separated path string into a clean list."""
    raw = re.split(r"[,\n]", dataset_paths)
    out: list[str] = []
    for chunk in raw:
        for token in chunk.split():
            token = token.strip().strip("`\"'")
            if token:
                out.append(token)
    return out


@tool
async def analyze_data(
    question: str = "",
    dataset_paths: str = "",
    context_id: str = "",
    resume_task_id: str = "",
) -> str:
    """Run Asta DataVoyager to generate and test hypotheses against a local dataset.

    DataVoyager writes and executes code against your tabular data in a sandboxed
    notebook and answers a specific analytical question — the loop from a theory to
    *testing it against the data*. The run is long (minutes to tens of minutes), so
    this tool only SUBMITS it and returns immediately — it does NOT block until the
    analysis is done. The Analysis panel then fills in on its own as the run
    completes; the user does not need to ask.

    When you get `status: "running"`, return a DataAnalysisResults with
    `status="running"`, that `task_id` and `context_id`, the user's `question` and
    `dataset_paths`, and empty findings, and tell the user their analysis is running
    and will appear in the Analysis panel automatically. Do NOT call this tool again
    in a loop — the frontend watches the task.

    Args:
        question: the tightened analytical question DataVoyager should answer (name
            the dataset and the decision/insight, phrased as a question code can
            answer). Required to start a run.
        dataset_paths: the local dataset file path(s) to analyze — the exact
            relative paths from the user's "Attached files" blockquote. Separate
            multiple with commas. The tool uploads them; never ask the user to
            pre-upload.
        context_id: an existing DataVoyager session id to run a FOLLOW-UP question
            against the same workspace (reuses the uploaded data; attach new files
            via `dataset_paths`). Omit to start a fresh session.
        resume_task_id: an existing task id to CHECK once (used only if the user
            explicitly asks about a specific run). Does a single status fetch and
            returns the parsed result if the run has finished.

    Returns:
        JSON string. Starting a run -> {status:"running", task_id, context_id, ...}.
        Explicit check -> completed/running/failed/input-required per
        `poll_analysis_status`. Errors -> {status:"error", message}.
    """
    try:
        sandbox = _active_sandbox.get()
    except LookupError:
        sandbox = None
    if sandbox is None:
        return json.dumps(
            {"status": "error", "message": "No active sandbox; cannot run DataVoyager."}
        )

    resume_task_id = resume_task_id.strip()
    if resume_task_id:
        if not is_valid_task_id(resume_task_id):
            return json.dumps({"status": "error", "message": "Invalid task id."})
        try:
            return json.dumps(
                await poll_analysis_status(sandbox, resume_task_id, context_id.strip())
            )
        except Exception as exc:  # noqa: BLE001
            return json.dumps({"status": "error", "message": f"poll failed: {exc}"})

    if not question.strip():
        return json.dumps(
            {"status": "error", "message": "An analytical question is required to start a run."}
        )
    paths = _split_paths(dataset_paths)
    ctx = context_id.strip() or None
    if not paths and not ctx:
        return json.dumps(
            {
                "status": "error",
                "message": (
                    "At least one dataset path is required to start a new analysis "
                    "(or a context_id to reuse an existing DataVoyager session)."
                ),
            }
        )
    try:
        submitted = await _submit(sandbox, question.strip(), paths, ctx)
    except Exception as exc:  # noqa: BLE001
        return json.dumps({"status": "error", "message": f"submit failed: {exc}"})
    if not submitted:
        return json.dumps(
            {
                "status": "error",
                "message": (
                    "DataVoyager returned no task id. This usually means your Asta "
                    "access token is missing or expired — refresh it in Settings → "
                    "Asta connection (run `asta auth login` then `asta auth "
                    "print-token`), then try again."
                ),
            }
        )

    return json.dumps(
        {
            "status": "running",
            "task_id": submitted["task_id"],
            "context_id": submitted["context_id"],
            "question": question.strip(),
            "note": "Analysis started; it runs in the background (minutes to tens of minutes).",
            "instruction": (
                "Return a DataAnalysisResults now with status='running', "
                f"task_id='{submitted['task_id']}', "
                f"context_id='{submitted['context_id']}', the user's question and "
                "dataset_paths, and empty findings. Tell the user their analysis is "
                "running and will appear in the Analysis panel automatically. Do NOT "
                "call this tool again."
            ),
        }
    )

"""Artifact file serving: download, upload, and sandbox deletion."""

from __future__ import annotations

from typing import Any

import mimetypes
from pathlib import PurePosixPath

from starlette.requests import Request
from starlette.responses import JSONResponse, Response

from backend.sandbox import LazyLangsmithSandbox
from backend.schemas import _is_supported_artifact_file
from backend.autodiscovery_tools import (
    MetadataNotSaved,
    already_submitted,
    cost_of,
    fetch_experiment_figures,
    is_valid_experiment_id,
    is_valid_run_id,
    persist_discovery_outputs,
    poll_discovery_status,
    read_credits,
    read_metadata,
    submit_run,
    update_metadata,
)
from backend.datavoyager_tools import (
    persist_analysis_outputs,
    poll_analysis_status,
)
from backend.theory_tools import (
    is_valid_task_id,
    persist_theory_outputs,
    poll_theory_status,
)
from backend.routes.common import (
    _existing_sandbox_for_thread,
    _request_user_id,
    _require_auth,
    _resolve_within,
)
from backend.runtime import asta_token_scope


MAX_UPLOAD_BYTES = 50 * 1024 * 1024  # 50 MB


async def get_artifact_file(request: Request) -> Response:
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    rel_path = request.query_params.get("path")
    download = request.query_params.get("download") == "1"

    if not rel_path:
        return JSONResponse({"error": "missing 'path' query param"}, status_code=400)
    if not _is_supported_artifact_file(rel_path):
        return JSONResponse({"error": "unsupported file type"}, status_code=415)

    adapter = await _existing_sandbox_for_thread(thread_id)
    if adapter is None:
        return JSONResponse({"error": "no sandbox for thread"}, status_code=404)

    work_dir = PurePosixPath(await adapter.aget_work_dir())
    abs_path = _resolve_within(work_dir, rel_path)
    if abs_path is None:
        return JSONResponse({"error": "invalid path"}, status_code=400)

    results = await adapter.adownload_files([str(abs_path)])
    if not results or results[0].error or results[0].content is None:
        return JSONResponse({"error": "file not found"}, status_code=404)

    content = results[0].content
    media_type, _ = mimetypes.guess_type(str(abs_path))
    if not media_type:
        media_type = "application/octet-stream"

    filename = PurePosixPath(abs_path).name
    disposition = "attachment" if download else "inline"
    headers = {"Content-Disposition": f'{disposition}; filename="{filename}"'}

    return Response(content=content, media_type=media_type, headers=headers)


def _safe_upload_name(raw_name: str) -> str | None:
    """Return a safe basename or None if the filename is unusable."""
    candidate = PurePosixPath(raw_name).name  # strips any path components
    if not candidate or candidate.startswith("."):
        return None
    if any(ch in candidate for ch in ("\x00", "/", "\\")):
        return None
    return candidate


async def upload_artifact_file(request: Request) -> Response:
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    if not thread_id:
        return JSONResponse({"error": "missing thread_id"}, status_code=400)

    form = await request.form()
    upload = form.get("file")
    if upload is None or not hasattr(upload, "read"):
        return JSONResponse(
            {"error": "missing 'file' field in multipart form"}, status_code=400
        )

    safe_name = _safe_upload_name(getattr(upload, "filename", "") or "")
    if safe_name is None:
        return JSONResponse({"error": "invalid filename"}, status_code=400)
    if not _is_supported_artifact_file(safe_name):
        return JSONResponse({"error": "unsupported file type"}, status_code=415)

    content = await upload.read()
    if not isinstance(content, bytes):
        return JSONResponse({"error": "expected bytes payload"}, status_code=400)
    if len(content) == 0:
        return JSONResponse({"error": "empty file"}, status_code=400)
    if len(content) > MAX_UPLOAD_BYTES:
        return JSONResponse(
            {"error": f"file exceeds {MAX_UPLOAD_BYTES} bytes"}, status_code=413
        )

    adapter = LazyLangsmithSandbox(thread_id)
    await adapter.aresolve()  # create-if-missing is intended for uploads
    work_dir = PurePosixPath(await adapter.aget_work_dir())
    abs_path = work_dir / safe_name

    existing = await adapter.adownload_files([str(abs_path)])
    if existing and existing[0].error is None and existing[0].content is not None:
        return JSONResponse(
            {"error": f"a file named {safe_name!r} already exists"},
            status_code=409,
        )

    results = await adapter.aupload_files([(str(abs_path), content)])
    if not results or results[0].error is not None:
        err = results[0].error if results else "unknown"
        return JSONResponse(
            {"error": f"sandbox upload failed: {err}"}, status_code=502
        )

    media_type, _ = mimetypes.guess_type(safe_name)
    return JSONResponse(
        {
            "name": safe_name,
            "relative_path": safe_name,
            "media_type": media_type,
            "size": len(content),
        },
        status_code=201,
    )


async def start_sandbox(request: Request) -> Response:
    """Resume a thread's idled sandbox so past artifacts become fetchable.

    Never creates a sandbox: if none exists for the thread (expired or the
    thread never ran), respond 404 so the frontend can tell the user the
    generated files are gone and need to be regenerated.
    """
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    if not thread_id:
        return JSONResponse({"error": "missing thread_id"}, status_code=400)

    adapter = LazyLangsmithSandbox(thread_id)
    try:
        resumed = await adapter.aresume()
    except Exception as exc:  # noqa: BLE001
        return JSONResponse(
            {"error": f"sandbox resume failed: {exc}"}, status_code=502
        )

    if not resumed:
        return JSONResponse({"error": "sandbox expired"}, status_code=404)
    return JSONResponse({"state": "ready"})


async def theorizer_status(request: Request) -> Response:
    """Poll an Asta Theorizer task in the thread's sandbox and return its status.

    The frontend calls this on an interval while a hypothesis artifact is
    `running`, so the Theories card fills in on its own when the run completes —
    no chat turn, no coordinator, no "check on it" message. Returns
    ``{"status": "completed", theories, ...}`` when ready, ``{"status":
    "running", ...}`` while in progress, ``failed``/``canceled`` on error, or
    ``unavailable`` if the thread's sandbox is gone.
    """
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    task_id = request.path_params["task_id"]
    if not thread_id or not task_id:
        return JSONResponse({"error": "missing thread_id or task_id"}, status_code=400)
    if not is_valid_task_id(task_id):
        return JSONResponse({"error": "invalid task_id"}, status_code=400)

    adapter = await _existing_sandbox_for_thread(thread_id)
    if adapter is None:
        # Sandbox expired: we can't poll from here. Frontend stops polling.
        return JSONResponse({"status": "unavailable"})

    # This route runs outside agent(), so bind the user's Asta token into the
    # ContextVar the sandbox reads — otherwise the poll authenticates `asta` with
    # the stale process-wide ASTA_TOKEN env var and a completed run polls as
    # "running" forever even after the user refreshes their token.
    async with asta_token_scope(_request_user_id(request)):
        try:
            result = await poll_theory_status(adapter, task_id)
        except Exception as exc:  # noqa: BLE001
            return JSONResponse({"status": "error", "message": str(exc)})

        # On a terminal state, persist the outcome into the sandbox so the agent
        # can read the theories on a later turn (it has filesystem tools) and so
        # the run is a durable artifact — completed theories, or an error log
        # naming the real failure reason. Best-effort; never blocks returning.
        if result.get("status") in ("completed", "failed", "canceled"):
            question = request.query_params.get("q", "")
            try:
                await persist_theory_outputs(adapter, task_id, question, result)
            except Exception:  # noqa: BLE001
                pass  # persistence is best-effort; the card still updates
    return JSONResponse(result)


async def analyze_data_status(request: Request) -> Response:
    """Poll an Asta DataVoyager task in the thread's sandbox and return its status.

    The frontend calls this on an interval while a DataAnalysis artifact is
    `running`, so the Analysis card fills in on its own when the run completes —
    no chat turn, no coordinator. Uses the cheap ``asta analyze-data task <id>``
    fetch, so it stays fast and survives sandbox restarts (the run lives on Asta's
    hosted service). Returns ``completed``/``failed``/``input-required``/``running``,
    or ``unavailable`` if the thread's sandbox is gone.
    """
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    task_id = request.path_params["task_id"]
    if not thread_id or not task_id:
        return JSONResponse({"error": "missing thread_id or task_id"}, status_code=400)
    if not is_valid_task_id(task_id):
        return JSONResponse({"error": "invalid task_id"}, status_code=400)

    adapter = await _existing_sandbox_for_thread(thread_id)
    if adapter is None:
        return JSONResponse({"status": "unavailable"})

    context_id = request.query_params.get("ctx", "")
    # See poll_theory_status: bind the user's Asta token for this out-of-agent poll.
    async with asta_token_scope(_request_user_id(request)):
        try:
            result = await poll_analysis_status(adapter, task_id, context_id)
        except Exception as exc:  # noqa: BLE001
            return JSONResponse({"status": "error", "message": str(exc)})

        # On a terminal state, persist the outcome + export the charts/notebook
        # into the sandbox so the agent can read the analysis on a later turn and
        # the files surface in the UI. Best-effort; never blocks returning.
        if result.get("status") in ("completed", "failed", "canceled"):
            question = request.query_params.get("q", "")
            try:
                await persist_analysis_outputs(adapter, task_id, question, result)
            except Exception:  # noqa: BLE001
                pass  # persistence is best-effort; the card still updates
    return JSONResponse(result)


#: One-shot approval tokens, keyed by token, valued by `(thread_id, run_id, experiments)`.
#:
#: **Why a token at all.** `discovery_submit` used to submit on any POST that reached it — an empty
#: body, `[]`, form data, `{"n_experiment": 1}`, all became `{}`, left no recognised edits, and fell
#: straight through to spending the researcher's credits on whatever budget the service happened to
#: hold. And this backend admits an unauthenticated local request as `local-user`, so
#: `curl -X POST .../submit` was enough. A model-authored HTML page opened in a browser would have
#: been enough. Found in review (§252).
#:
#: So approving is now two steps that only the app can perform in order: open the modal, which
#: issues a token against a specific run *and a specific budget*, then submit that exact token. The
#: token is consumed on use, so a replay cannot re-spend, and it carries the budget so a submit
#: cannot quietly authorise a different one.
#:
#: In memory on purpose. A restart invalidates every outstanding approval, which is the safe
#: direction: the modal is reopened and the researcher presses again.
_APPROVALS: dict[str, tuple[str, str, int]] = {}

#: How many outstanding approvals to keep. Small — one researcher, one modal at a time — and a cap
#: rather than an expiry because the failure mode of forgetting one is a re-press, not a loss.
_MAX_APPROVALS = 8


def _issue_approval(thread_id: str, run_id: str, experiments: int) -> str:
    """Mint a token that authorises submitting exactly this run at exactly this budget."""
    import secrets

    token = secrets.token_urlsafe(24)
    if len(_APPROVALS) >= _MAX_APPROVALS:
        # Oldest first; dicts preserve insertion order.
        _APPROVALS.pop(next(iter(_APPROVALS)), None)
    _APPROVALS[token] = (thread_id, run_id, experiments)
    return token


def _check_approval(token: str, thread_id: str, run_id: str) -> int | None:
    """The budget a token authorises, **without consuming it**, or `None` if it does not apply.

    Bound to the thread *and* the run it was issued for: a token minted for one run must not
    authorise another, or the check would be a formality.
    """
    held = _APPROVALS.get(token or "")
    if held is None:
        return None
    issued_thread, issued_run, experiments = held
    if issued_thread != thread_id or issued_run != run_id:
        return None
    return experiments


def _spend_approval(token: str) -> None:
    """Consume a token, immediately before the call that spends credits.

    **Checked early, consumed late, and the order matters.** The first version consumed on arrival,
    so a submit that failed *after* the check — because the configuration change did not save —
    burned the token, and the researcher's second press came back "this submit carries no valid
    approval". A press that cannot be retried after a recoverable failure is a dead end (§255).

    Consumed before the spend rather than after it, so a replay still cannot pay twice: the window
    where a token is valid and a charge is in flight is one function call wide.
    """
    _APPROVALS.pop(token or "", None)


async def discovery_draft(request: Request) -> Response:
    """What the approval modal needs — the drafted run, and what it costs against the balance.

    Its own route rather than part of the poll, because it is read once when the modal opens and
    costs two extra service calls. A poll that carried it would pay for them every tick.
    """
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    run_id = request.path_params["run_id"]
    if not is_valid_run_id(run_id):
        return JSONResponse({"error": "invalid run_id"}, status_code=400)

    adapter = await _existing_sandbox_for_thread(thread_id)
    if adapter is None:
        return JSONResponse({"status": "unavailable"})

    async with asta_token_scope(_request_user_id(request)):
        metadata = await read_metadata(adapter, run_id)
        credits = await read_credits(adapter)
        # Asked of the service, not of our own record: an approved run's artifact can sit at
        # `awaiting_approval` indefinitely, and re-offering the gate for a run already finished is
        # worse than not offering it (§258).
        started = await already_submitted(adapter, run_id)
        # The run's real state, so a caller adopting an already-approved run shows what it is
        # rather than assuming "running" and flickering to "completed" on the first poll (§260).
        polled = await poll_discovery_status(adapter, run_id) if started else {}
    if not metadata:
        return JSONResponse({"error": "no such drafted run"}, status_code=404)
    # `available` already nets off runs in flight; the others are for context only.
    cost = cost_of(metadata)
    return JSONResponse(
        {
            "run_id": run_id,
            "metadata": metadata,
            "credits": credits,
            "cost": cost,
            # `true` when the service says this run is past `CREATED`. The gate must not be offered
            # for it, and the caller adopts it as a running job instead.
            "submitted": bool(started),
            "status": str(polled.get("status") or "") if started else "",
            # The token the modal must hand back. Issued here because opening the modal is the only
            # thing in this app that legitimately precedes a press. Not issued at all for a run that
            # has already started — there is nothing left to authorise.
            "approval": "" if started else _issue_approval(thread_id, run_id, cost),
        }
    )


async def discovery_submit(request: Request) -> Response:
    """Spend the credits. **The only caller is the researcher pressing approve.**

    This route *is* the credit gate. Nothing the model does reaches it — `draft_discovery_run` stops
    at a configured run, and `submit_run` has no other caller in the codebase. The two edits the
    modal offers — the budget and the intent — are applied here before submitting, so what the
    researcher saw and what runs are the same thing.
    """
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    run_id = request.path_params["run_id"]
    if not is_valid_run_id(run_id):
        return JSONResponse({"error": "invalid run_id"}, status_code=400)

    # **An unreadable body is a refusal, not an approval.** This used to coerce anything —
    # an empty POST, `[]`, form data, a typo'd key — into `{}`, find no recognised edits, and
    # submit whatever budget the service held. Spending money on a request nobody could read is
    # the wrong default in every direction (§252).
    try:
        body = await request.json()
    except Exception:  # noqa: BLE001
        body = None
    if not isinstance(body, dict):
        return JSONResponse(
            {"error": "a submit needs a JSON object naming the approved budget"},
            status_code=400,
        )

    token = str(body.get("approval") or "")
    approved = _check_approval(token, thread_id, run_id)
    if approved is None:
        return JSONResponse(
            {
                "error": (
                    "this submit carries no valid approval. A run is started by approving its "
                    "budget in the app, which is the only thing that issues one."
                )
            },
            status_code=403,
        )

    # The budget must be stated, and must be the one the token was issued against unless the
    # researcher changed it in the modal — in which case that number is what they saw and pressed.
    wanted = body.get("n_experiments", approved)
    if not isinstance(wanted, int) or isinstance(wanted, bool):
        return JSONResponse({"error": "n_experiments must be a whole number"}, status_code=400)
    changes: dict[str, Any] = {"n_experiments": wanted}
    if isinstance(body.get("intent"), str):
        changes["intent"] = body["intent"]

    adapter = await _existing_sandbox_for_thread(thread_id)
    if adapter is None:
        return JSONResponse({"status": "unavailable"})

    async with asta_token_scope(_request_user_id(request)):
        try:
            await update_metadata(adapter, run_id, changes)
        except ValueError as exc:
            return JSONResponse({"error": str(exc)}, status_code=400)
        except MetadataNotSaved as exc:
            # Never submit a run whose edits did not land: the researcher approved a number and the
            # service would charge for whatever it still had stored. The token is deliberately
            # *not* spent here, so pressing again after a transient failure works — and the
            # service's own words come back with it, because "your changes did not save" is true
            # and useless (§255).
            return JSONResponse({"error": str(exc)}, status_code=409)

        # From here on a charge is possible, so the token goes now. `approved=wanted` makes
        # `submit_run` re-read the stored budget and refuse if something else moved it.
        _spend_approval(token)
        result = await submit_run(adapter, run_id, approved=wanted)
    if "error" in result:
        return JSONResponse(result, status_code=502)
    return JSONResponse(result)


async def discovery_status(request: Request) -> Response:
    """Poll a run, and persist it once it stops.

    Cheap enough for a timer — the run's own status plus the experiments list, which is where the
    honest progress number comes from. Deliberately does **not** touch the per-experiment endpoint
    — that is the only place figures live and it is ~458KB a node.
    """
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    run_id = request.path_params["run_id"]
    if not is_valid_run_id(run_id):
        return JSONResponse({"error": "invalid run_id"}, status_code=400)

    adapter = await _existing_sandbox_for_thread(thread_id)
    if adapter is None:
        return JSONResponse({"status": "unavailable"})

    async with asta_token_scope(_request_user_id(request)):
        try:
            result = await poll_discovery_status(adapter, run_id)
        except Exception as exc:  # noqa: BLE001
            return JSONResponse({"status": "error", "message": str(exc)})

        # A finished run has seven days before its datasets expire, so this is not optional —
        # it is the only copy that outlives the service (docs §247).
        if result.get("status") in ("completed", "failed", "canceled"):
            try:
                metadata = await read_metadata(adapter, run_id)
                await persist_discovery_outputs(adapter, run_id, metadata, result)
            except Exception:  # noqa: BLE001
                pass  # persistence is best-effort; the caller still gets the status
    return JSONResponse(result)


async def discovery_figures(request: Request) -> Response:
    """Decode one experiment's figures to disk, on demand.

    Separate from everything else because it is the expensive call and it is only worth making when
    a researcher opens that experiment. The base64 never leaves the sandbox; this returns paths.
    """
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    run_id = request.path_params["run_id"]
    experiment_id = request.path_params["experiment_id"]
    if not is_valid_run_id(run_id) or not is_valid_experiment_id(experiment_id):
        return JSONResponse({"error": "invalid run_id or experiment_id"}, status_code=400)

    adapter = await _existing_sandbox_for_thread(thread_id)
    if adapter is None:
        return JSONResponse({"status": "unavailable"})

    async with asta_token_scope(_request_user_id(request)):
        outcome = await fetch_experiment_figures(adapter, run_id, experiment_id)
    # An error is a 502 rather than an empty list, so the caller does not cache a failed fetch as
    # "this experiment drew nothing" (§260).
    if outcome.get("error"):
        return JSONResponse(
            {"run_id": run_id, "experiment_id": experiment_id, **outcome}, status_code=502
        )
    return JSONResponse({"run_id": run_id, "experiment_id": experiment_id, **outcome})


async def collect_outside_files(request: Request) -> Response:
    """Copy files this conversation's commands wrote outside it back into it.

    **Backend-side because only the backend can see both ends.** `/tmp` is inside WSL and the
    desktop app runs on Windows; the conversation folder is on `/mnt/c`. This process is the one
    with both in its namespace (docs §250's three filesystems).

    **Only what a command was watched writing.** The request carries no paths: the record decides,
    and the record's `wrote` list is the subset confirmed by mtime rather than read off a command's
    text. Letting the caller name a path would turn a report into a file-copier pointed at
    anything, which is a different and much worse tool.

    Copies, never moves. A script often writes a file and reads it back later in the same run.
    """
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    if not thread_id:
        return JSONResponse({"error": "missing thread_id"}, status_code=400)

    try:
        from minime_local import ledger
    except ImportError:
        # The overlay is desktop-only. A sandboxed deployment has no local files to collect, and
        # saying so beats a 500 that reads like a bug.
        return JSONResponse(
            {"error": "collecting local files needs the desktop overlay"}, status_code=501
        )

    adapter = LazyLangsmithSandbox(thread_id)
    try:
        work_dir = await adapter.aget_work_dir()
    except Exception as exc:  # noqa: BLE001
        return JSONResponse({"error": f"no workspace: {exc}"}, status_code=502)

    # **Both halves, so an empty answer can say why it is empty.** `brought=0 refused=0` is a
    # sentence with no information in it, and it is exactly what the first version returned when a
    # file had since been swept from `/tmp`. A caller cannot explain what it was not told.
    report = ledger.outside_files(work_dir)
    gone = [{"path": path, "reason": "it is no longer where the command left it"} for path in report["gone"]]

    if not report["present"]:
        note = (
            f"{len(gone)} file(s) were written outside this conversation, and none is still there"
            if gone
            else "no command in this conversation wrote a file outside it"
        )
        return JSONResponse({"brought": [], "refused": gone, "note": note})

    outcome = ledger.collect(work_dir, report["present"])
    outcome["refused"].extend(gone)
    return JSONResponse(outcome)


async def delete_sandbox(request: Request) -> Response:
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    if not thread_id:
        return JSONResponse({"error": "missing thread_id"}, status_code=400)

    adapter = LazyLangsmithSandbox(thread_id)
    try:
        existed = await adapter.adelete()
    except Exception as exc:  # noqa: BLE001
        return JSONResponse(
            {"error": f"sandbox delete failed: {exc}"}, status_code=502
        )

    if not existed:
        return JSONResponse({"deleted": False}, status_code=404)
    return Response(status_code=204)

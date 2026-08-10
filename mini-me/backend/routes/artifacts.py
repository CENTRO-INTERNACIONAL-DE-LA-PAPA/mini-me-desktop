"""Artifact file serving: download, upload, and sandbox deletion."""

from __future__ import annotations

import mimetypes
from pathlib import PurePosixPath

from starlette.requests import Request
from starlette.responses import JSONResponse, Response

from backend.sandbox import LazyLangsmithSandbox
from backend.schemas import _is_supported_artifact_file
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

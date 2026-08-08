"""Project registry + thread assignment HTTP routes (explicit Projects, P5).

These manage the *containers* — creating, listing, renaming, and deleting named
Projects, and recording which Project a conversation belongs to. A Project's
*content* (mission / completed / pending / run-loop plan) is read and hand-edited
through the sibling ``project`` route module.

All handlers read/write the same LangGraph store the graph uses, via
``langgraph_api.store.get_store()``, under the caller's user-scoped namespaces
(``(user_id, "projects")`` for the registry, ``(user_id, "threads")`` for the
thread→project map). The user id comes from the authenticated identity, so the
namespaces match the ones the coordinator middleware uses.
"""

from __future__ import annotations

from typing import Any

from starlette.requests import Request
from starlette.responses import JSONResponse, Response

from backend.projects import (
    create_project,
    delete_project,
    ensure_default_project,
    get_project_meta,
    list_projects,
    rename_project,
    set_thread_project,
)
from backend.routes.common import _request_user_id, _require_auth

_MAX_NAME_CHARS = 120


async def _get_store_or_error() -> Any:
    from langgraph_api.store import get_store  # noqa: PLC0415

    return await get_store()


def _auth_user(request: Request) -> tuple[str | None, Response | None]:
    if (unauth := _require_auth(request)) is not None:
        return None, unauth
    user_id = _request_user_id(request)
    if not user_id:
        return None, JSONResponse({"error": "unauthorized"}, status_code=401)
    return user_id, None


async def list_projects_route(request: Request) -> Response:
    """List the caller's projects (ensuring a default exists), newest first."""
    user_id, err = _auth_user(request)
    if err is not None:
        return err
    store = await _get_store_or_error()
    await ensure_default_project(store, user_id)
    projects = await list_projects(store, user_id)
    return JSONResponse({"projects": projects})


async def create_project_route(request: Request) -> Response:
    """Create a new named project and return its registry record."""
    user_id, err = _auth_user(request)
    if err is not None:
        return err
    try:
        body = await request.json()
    except Exception:  # noqa: BLE001
        return JSONResponse({"error": "invalid JSON body"}, status_code=400)
    name = str((body or {}).get("name") or "").strip()[:_MAX_NAME_CHARS]
    if not name:
        return JSONResponse({"error": "name is required"}, status_code=400)
    store = await _get_store_or_error()
    meta = await create_project(store, user_id, name)
    return JSONResponse(meta, status_code=201)


async def patch_project_meta_route(request: Request) -> Response:
    """Rename a project (registry metadata only)."""
    user_id, err = _auth_user(request)
    if err is not None:
        return err
    project_id = request.path_params.get("project_id", "")
    try:
        body = await request.json()
    except Exception:  # noqa: BLE001
        return JSONResponse({"error": "invalid JSON body"}, status_code=400)
    name = str((body or {}).get("name") or "").strip()[:_MAX_NAME_CHARS]
    if not name:
        return JSONResponse({"error": "name is required"}, status_code=400)
    store = await _get_store_or_error()
    meta = await rename_project(store, user_id, project_id, name)
    if meta is None:
        return JSONResponse({"error": "project not found"}, status_code=404)
    return JSONResponse(meta)


async def delete_project_route(request: Request) -> Response:
    """Delete a project's registry record and its spine."""
    user_id, err = _auth_user(request)
    if err is not None:
        return err
    project_id = request.path_params.get("project_id", "")
    if not project_id:
        return JSONResponse({"error": "project_id is required"}, status_code=400)
    store = await _get_store_or_error()
    meta = await get_project_meta(store, user_id, project_id)
    if meta is None:
        return JSONResponse({"error": "project not found"}, status_code=404)
    await delete_project(store, user_id, project_id)
    return Response(status_code=204)


async def assign_thread_project_route(request: Request) -> Response:
    """Record which Project a conversation (thread) belongs to."""
    user_id, err = _auth_user(request)
    if err is not None:
        return err
    thread_id = request.path_params.get("thread_id", "")
    try:
        body = await request.json()
    except Exception:  # noqa: BLE001
        return JSONResponse({"error": "invalid JSON body"}, status_code=400)
    project_id = str((body or {}).get("project_id") or "").strip()
    if not thread_id or not project_id:
        return JSONResponse({"error": "thread_id and project_id are required"}, status_code=400)
    store = await _get_store_or_error()
    # Only allow assigning to a project that exists (avoid dangling references).
    if await get_project_meta(store, user_id, project_id) is None:
        return JSONResponse({"error": "project not found"}, status_code=404)
    await set_thread_project(store, user_id, thread_id, project_id)
    return JSONResponse({"thread_id": thread_id, "project_id": project_id})

"""Research-project spine HTTP routes (P3.3 hand-edits + P5 plan edits).

A Project's spine (mission + Completed / Pending Work + the run-loop plan) is
normally updated by ``ProjectSpineMiddleware`` during a run. These routes let the
user read and **edit it by hand** — rename the mission, add a backlog item,
complete / dismiss one, or edit the autonomous run-loop plan — without waiting
for a turn.

They read/write the SAME LangGraph store the graph uses, via
``langgraph_api.store.get_store()``, under the project-scoped namespace
``(user_id, "project", project_id)`` (see ``backend.runtime._project_namespace``).
The project id is passed explicitly by the frontend (``?project_id=`` /
``project_id`` in the body); when omitted, the user's default project is used, so
old single-project clients keep working.

Registry CRUD (create / list / rename / delete projects) and thread→project
assignment live in the sibling ``projects`` route module.
"""

from __future__ import annotations

from typing import Any

from starlette.requests import Request
from starlette.responses import JSONResponse, Response

from backend.plan import apply_plan_edit
from backend.project import (
    ProjectEdit,
    ProjectState,
    apply_project_edit,
    build_project_payload,
    load_project,
    save_project,
)
from backend.projects import ensure_default_project
from backend.runtime import DEFAULT_PROJECT_ID, _project_namespace
from backend.routes.common import _request_user_id, _require_auth

# Sane caps so a hand-edit can't write an unbounded blob into the store.
_MAX_MISSION_CHARS = 500
_MAX_ITEM_CHARS = 300

# Plan edit ops the route accepts (mirrors ``backend.plan.apply_plan_edit``).
_PLAN_OPS = {"accept", "complete", "skip", "activate", "edit", "add", "remove", "reorder", "clear"}


async def _get_store_or_error() -> Any:
    """Resolve the platform store lazily (import defers config load to runtime)."""
    from langgraph_api.store import get_store  # noqa: PLC0415

    return await get_store()


def _resolve_project_id(request: Request, body: Any = None) -> str:
    """The project id from the query string or body, defaulting to the user's default."""
    pid = request.query_params.get("project_id")
    if not pid and isinstance(body, dict):
        pid = body.get("project_id")
    pid = str(pid or "").strip()
    return pid or DEFAULT_PROJECT_ID


def _parse_edit(body: Any) -> ProjectEdit | None:
    """Extract a validated ProjectEdit from a JSON body, or None if unusable."""
    if not isinstance(body, dict):
        return None
    edit: ProjectEdit = {}
    if "mission" in body and isinstance(body["mission"], str):
        edit["mission"] = body["mission"][:_MAX_MISSION_CHARS]
    for key in ("pending_add", "pending_remove", "complete"):
        value = body.get(key)
        if isinstance(value, str) and value.strip():
            edit[key] = value[:_MAX_ITEM_CHARS]  # type: ignore[literal-required]
    return edit or None


def _parse_plan_op(body: Any) -> dict[str, Any] | None:
    """Extract a validated plan edit op (``{"op": ..., ...}``) from the body."""
    if not isinstance(body, dict):
        return None
    plan_op = body.get("plan_op")
    if not isinstance(plan_op, dict):
        return None
    op = str(plan_op.get("op") or "").strip()
    if op not in _PLAN_OPS:
        return None
    # Clip free-text fields defensively; ids/order pass through.
    cleaned: dict[str, Any] = {"op": op}
    for key in ("id", "after_id"):
        if isinstance(plan_op.get(key), str):
            cleaned[key] = plan_op[key][:120]
    for key in ("title", "rationale", "action", "prompt"):
        if isinstance(plan_op.get(key), str):
            cleaned[key] = plan_op[key][:_MAX_MISSION_CHARS]
    if isinstance(plan_op.get("order"), list):
        cleaned["order"] = [str(i)[:120] for i in plan_op["order"] if isinstance(i, str)]
    return cleaned


async def get_project(request: Request) -> Response:
    """Return the caller's persisted project spine (mission + completed + pending + plan).

    ``suggestions`` is always empty here: it is derived from a thread's live
    artifacts during a run, which this stateless route does not have. The
    frontend keeps whatever live suggestions it already holds.
    """
    if (unauth := _require_auth(request)) is not None:
        return unauth
    user_id = _request_user_id(request)
    if not user_id:
        return JSONResponse({"error": "unauthorized"}, status_code=401)

    project_id = _resolve_project_id(request)
    store = await _get_store_or_error()
    if project_id == DEFAULT_PROJECT_ID:
        await ensure_default_project(store, user_id)
    state = await load_project(store, _project_namespace(user_id, project_id))
    return JSONResponse(build_project_payload(state, []))


async def patch_project(request: Request) -> Response:
    """Apply one hand-edit (mission / pending / plan op) to a project and persist."""
    if (unauth := _require_auth(request)) is not None:
        return unauth
    user_id = _request_user_id(request)
    if not user_id:
        return JSONResponse({"error": "unauthorized"}, status_code=401)

    try:
        body = await request.json()
    except Exception:  # noqa: BLE001
        return JSONResponse({"error": "invalid JSON body"}, status_code=400)

    edit = _parse_edit(body)
    plan_op = _parse_plan_op(body)
    if edit is None and plan_op is None:
        return JSONResponse(
            {
                "error": (
                    "body must include one of: mission, pending_add, "
                    "pending_remove, complete, or a plan_op"
                )
            },
            status_code=400,
        )

    project_id = _resolve_project_id(request, body)
    namespace = _project_namespace(user_id, project_id)
    store = await _get_store_or_error()
    state = await load_project(store, namespace)
    if edit is not None:
        state = apply_project_edit(state, edit)
    if plan_op is not None:
        state = ProjectState(
            mission=state.get("mission") or "",
            completed=dict(state.get("completed") or {}),
            pending=list(state.get("pending") or []),
            plan=apply_plan_edit(state.get("plan"), plan_op),
        )
    await save_project(store, namespace, state)
    return JSONResponse(build_project_payload(state, []))

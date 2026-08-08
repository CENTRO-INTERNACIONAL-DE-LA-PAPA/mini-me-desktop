"""Explicit Projects: named containers that group conversations (P5).

A *Project* is the ChatGPT/Claude-style unit the user works in: a named
container that holds its own mission + Completed/Pending + autonomous run-loop
plan (the "spine", see :mod:`backend.project`) and groups many threads. This
module owns the two pieces of bookkeeping *around* the spine:

  * the **registry** — one lightweight ``ProjectMeta`` record per project
    (``{id, name, created_at, updated_at}``), under
    ``(user_id, "projects")``; and
  * the **thread→project map** — which Project a conversation belongs to, under
    ``(user_id, "threads")``.

The spine record for a project lives separately under
``(user_id, "project", project_id)`` and is loaded/saved by
:mod:`backend.project`. Keeping the registry tiny (no mission text) means
listing projects never drags the whole spine along.

Everything here is a thin, well-typed wrapper over the LangGraph store so both
the coordinator middleware and the stateless ``/projects`` HTTP routes share one
implementation. Store timestamps use ``datetime`` (ISO-8601 strings) — the store
is the source of truth, so ordering is stable across processes.
"""

from __future__ import annotations

import uuid
from datetime import datetime, timezone
from typing import Any

from langgraph.store.base import BaseStore
from typing_extensions import TypedDict

from backend.runtime import (
    DEFAULT_PROJECT_ID,
    _projects_registry_namespace,
    _thread_index_namespace,
)

# The default project's display name (created lazily the first time a user has
# no projects, or a run lands with no explicit project id).
DEFAULT_PROJECT_NAME = "My research"

_MAX_NAME_CHARS = 120


class ProjectMeta(TypedDict):
    """A registry record: the project's identity, not its spine content."""

    id: str
    name: str
    created_at: str
    updated_at: str


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def new_project_id() -> str:
    """A fresh, collision-free project id (opaque; never shown to the user)."""
    return f"proj-{uuid.uuid4().hex[:12]}"


def _clean_name(name: Any, *, fallback: str) -> str:
    text = " ".join(str(name or "").split()).strip()
    if not text:
        return fallback
    return text[:_MAX_NAME_CHARS]


def _coerce_meta(value: Any) -> ProjectMeta | None:
    if not isinstance(value, dict):
        return None
    pid = str(value.get("id") or "").strip()
    if not pid:
        return None
    return {
        "id": pid,
        "name": str(value.get("name") or "").strip() or pid,
        "created_at": str(value.get("created_at") or ""),
        "updated_at": str(value.get("updated_at") or value.get("created_at") or ""),
    }


# ---------------------------------------------------------------------------
# Registry IO
# ---------------------------------------------------------------------------

async def list_projects(store: BaseStore, user_id: str) -> list[ProjectMeta]:
    """Every project the user has, newest first (by ``created_at``)."""
    namespace = _projects_registry_namespace(user_id)
    items = await store.asearch(namespace, limit=1000)
    metas = [m for item in items if (m := _coerce_meta(item.value)) is not None]
    metas.sort(key=lambda m: (m.get("created_at") or "", m["id"]))
    return metas


async def get_project_meta(
    store: BaseStore, user_id: str, project_id: str
) -> ProjectMeta | None:
    item = await store.aget(_projects_registry_namespace(user_id), project_id)
    if item is None:
        return None
    return _coerce_meta(item.value)


async def create_project(
    store: BaseStore,
    user_id: str,
    name: str,
    *,
    project_id: str | None = None,
) -> ProjectMeta:
    """Create (or, for a fixed id like the default, upsert) a registry record."""
    pid = (project_id or new_project_id()).strip()
    existing = await get_project_meta(store, user_id, pid)
    now = _now_iso()
    meta: ProjectMeta = {
        "id": pid,
        "name": _clean_name(name, fallback=DEFAULT_PROJECT_NAME),
        "created_at": existing["created_at"] if existing else now,
        "updated_at": now,
    }
    await store.aput(_projects_registry_namespace(user_id), pid, dict(meta))
    return meta


async def rename_project(
    store: BaseStore, user_id: str, project_id: str, name: str
) -> ProjectMeta | None:
    """Rename an existing project; ``None`` if it does not exist."""
    existing = await get_project_meta(store, user_id, project_id)
    if existing is None:
        return None
    meta: ProjectMeta = {
        "id": existing["id"],
        "name": _clean_name(name, fallback=existing["name"]),
        "created_at": existing["created_at"],
        "updated_at": _now_iso(),
    }
    await store.aput(_projects_registry_namespace(user_id), project_id, dict(meta))
    return meta


async def delete_project(store: BaseStore, user_id: str, project_id: str) -> None:
    """Remove a project's registry record and its spine.

    Thread→project mappings that pointed here are left as-is (harmless dangling
    keys); a reopened thread simply re-resolves to the default project.
    """
    from backend.runtime import _project_namespace  # noqa: PLC0415
    from backend.project import PROJECT_STORE_KEY  # noqa: PLC0415

    await store.adelete(_projects_registry_namespace(user_id), project_id)
    await store.adelete(_project_namespace(user_id, project_id), PROJECT_STORE_KEY)


async def ensure_default_project(store: BaseStore, user_id: str) -> ProjectMeta:
    """Return the user's default project, creating its registry record if absent."""
    existing = await get_project_meta(store, user_id, DEFAULT_PROJECT_ID)
    if existing is not None:
        return existing
    return await create_project(
        store, user_id, DEFAULT_PROJECT_NAME, project_id=DEFAULT_PROJECT_ID
    )


# ---------------------------------------------------------------------------
# Thread→project map IO
# ---------------------------------------------------------------------------

async def get_thread_project(
    store: BaseStore, user_id: str, thread_id: str
) -> str | None:
    if not thread_id:
        return None
    item = await store.aget(_thread_index_namespace(user_id), thread_id)
    if item is None or not isinstance(item.value, dict):
        return None
    pid = str(item.value.get("project_id") or "").strip()
    return pid or None


async def set_thread_project(
    store: BaseStore, user_id: str, thread_id: str, project_id: str
) -> None:
    if not thread_id or not project_id:
        return
    await store.aput(
        _thread_index_namespace(user_id),
        thread_id,
        {"project_id": project_id, "updated_at": _now_iso()},
    )


# ---------------------------------------------------------------------------
# Active-project resolution for a run
# ---------------------------------------------------------------------------

async def resolve_active_project_id(
    store: BaseStore,
    user_id: str,
    *,
    explicit_project_id: str | None,
    thread_id: str | None,
) -> str:
    """Resolve which Project this run's spine belongs to.

    Precedence: the explicit id the frontend passed on ``configurable`` → the
    thread's stored mapping → the user's default project. Whatever we resolve is
    written back to the thread→project map (and the default registry record is
    created when needed), so a later stateless route or a reopened thread resolves
    to the same place without the client having to re-send it.
    """
    pid = (explicit_project_id or "").strip()
    if not pid:
        pid = (await get_thread_project(store, user_id, thread_id or "")) or ""
    if not pid:
        await ensure_default_project(store, user_id)
        pid = DEFAULT_PROJECT_ID

    # Persist the association so it survives client-state loss / reopen.
    if thread_id:
        current = await get_thread_project(store, user_id, thread_id)
        if current != pid:
            await set_thread_project(store, user_id, thread_id, pid)
    return pid

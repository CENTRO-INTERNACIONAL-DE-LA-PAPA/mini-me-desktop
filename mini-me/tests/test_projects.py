"""Tests for explicit Projects (`backend.projects`) — the containers, P5.

These pin the registry + thread-index contract: projects are created/listed/
renamed/deleted per user, a default project is created lazily, the thread→project
map records assignments, and ``resolve_active_project_id`` follows the documented
precedence (explicit → thread map → default) while persisting the association.
Each project's spine lives under its own namespace, so two projects never share
mission/plan state.
"""

from __future__ import annotations

import asyncio

from langgraph.store.memory import InMemoryStore

from backend.project import PROJECT_STORE_KEY, empty_project, load_project, save_project
from backend.projects import (
    DEFAULT_PROJECT_NAME,
    create_project,
    delete_project,
    ensure_default_project,
    get_project_meta,
    get_thread_project,
    list_projects,
    rename_project,
    resolve_active_project_id,
    set_thread_project,
)
from backend.runtime import (
    DEFAULT_PROJECT_ID,
    _project_namespace,
    _projects_registry_namespace,
)


def _run(coro):
    return asyncio.run(coro)


# ---------------------------------------------------------------------------
# Registry CRUD
# ---------------------------------------------------------------------------

def test_create_list_and_get_project() -> None:
    async def _t() -> None:
        store = InMemoryStore()
        a = await create_project(store, "u1", "Drought study")
        b = await create_project(store, "u1", "Yield model")
        assert a["id"] != b["id"]
        assert a["name"] == "Drought study"

        projects = await list_projects(store, "u1")
        ids = {p["id"] for p in projects}
        assert {a["id"], b["id"]} <= ids

        got = await get_project_meta(store, "u1", a["id"])
        assert got is not None and got["name"] == "Drought study"
        assert await get_project_meta(store, "u1", "missing") is None

    _run(_t())


def test_rename_project() -> None:
    async def _t() -> None:
        store = InMemoryStore()
        a = await create_project(store, "u1", "Old name")
        renamed = await rename_project(store, "u1", a["id"], "New name")
        assert renamed is not None and renamed["name"] == "New name"
        assert renamed["created_at"] == a["created_at"]  # created_at preserved
        assert await rename_project(store, "u1", "missing", "x") is None

    _run(_t())


def test_delete_project_removes_registry_and_spine() -> None:
    async def _t() -> None:
        store = InMemoryStore()
        a = await create_project(store, "u1", "Temp")
        namespace = _project_namespace("u1", a["id"])
        state = {**empty_project(), "mission": "m"}
        await save_project(store, namespace, state)

        await delete_project(store, "u1", a["id"])
        assert await get_project_meta(store, "u1", a["id"]) is None
        # Spine record is gone → load returns a fresh empty project.
        assert (await load_project(store, namespace)) == empty_project()
        assert (await store.aget(namespace, PROJECT_STORE_KEY)) is None

    _run(_t())


def test_projects_are_user_scoped() -> None:
    async def _t() -> None:
        store = InMemoryStore()
        await create_project(store, "u1", "u1 project")
        await create_project(store, "u2", "u2 project")
        u1 = {p["name"] for p in await list_projects(store, "u1")}
        u2 = {p["name"] for p in await list_projects(store, "u2")}
        assert "u1 project" in u1 and "u1 project" not in u2
        assert "u2 project" in u2 and "u2 project" not in u1

    _run(_t())


# ---------------------------------------------------------------------------
# Default project
# ---------------------------------------------------------------------------

def test_ensure_default_project_is_idempotent() -> None:
    async def _t() -> None:
        store = InMemoryStore()
        first = await ensure_default_project(store, "u1")
        assert first["id"] == DEFAULT_PROJECT_ID
        assert first["name"] == DEFAULT_PROJECT_NAME
        second = await ensure_default_project(store, "u1")
        assert second["created_at"] == first["created_at"]
        # Only one registry record for the default id.
        items = await store.asearch(_projects_registry_namespace("u1"), limit=100)
        assert sum(1 for i in items if i.value.get("id") == DEFAULT_PROJECT_ID) == 1

    _run(_t())


# ---------------------------------------------------------------------------
# Thread → project map
# ---------------------------------------------------------------------------

def test_thread_project_map_round_trip() -> None:
    async def _t() -> None:
        store = InMemoryStore()
        assert await get_thread_project(store, "u1", "t1") is None
        await set_thread_project(store, "u1", "t1", "proj-x")
        assert await get_thread_project(store, "u1", "t1") == "proj-x"

    _run(_t())


# ---------------------------------------------------------------------------
# Active-project resolution
# ---------------------------------------------------------------------------

def test_resolve_prefers_explicit_and_persists_mapping() -> None:
    async def _t() -> None:
        store = InMemoryStore()
        pid = await resolve_active_project_id(
            store, "u1", explicit_project_id="proj-explicit", thread_id="t1"
        )
        assert pid == "proj-explicit"
        # The association is written back so a later stateless call resolves it.
        assert await get_thread_project(store, "u1", "t1") == "proj-explicit"

    _run(_t())


def test_resolve_falls_back_to_thread_map_then_default() -> None:
    async def _t() -> None:
        store = InMemoryStore()
        await set_thread_project(store, "u1", "t1", "proj-mapped")
        pid = await resolve_active_project_id(
            store, "u1", explicit_project_id=None, thread_id="t1"
        )
        assert pid == "proj-mapped"

        # No explicit id and no mapping → default (and it gets created).
        pid2 = await resolve_active_project_id(
            store, "u1", explicit_project_id=None, thread_id="t2"
        )
        assert pid2 == DEFAULT_PROJECT_ID
        assert await get_project_meta(store, "u1", DEFAULT_PROJECT_ID) is not None

    _run(_t())


def test_two_projects_keep_separate_spines() -> None:
    async def _t() -> None:
        store = InMemoryStore()
        ns_a = _project_namespace("u1", "a")
        ns_b = _project_namespace("u1", "b")
        await save_project(store, ns_a, {**empty_project(), "mission": "mission A"})
        await save_project(store, ns_b, {**empty_project(), "mission": "mission B"})
        assert (await load_project(store, ns_a))["mission"] == "mission A"
        assert (await load_project(store, ns_b))["mission"] == "mission B"

    _run(_t())

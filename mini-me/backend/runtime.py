"""Shared backend runtime state and process-wide singletons.

This module is imported first (via :mod:`backend`'s ``__init__``) so that:

* ``load_dotenv()`` runs before any other backend module reads ``os.getenv``;
* the active-sandbox ContextVar and the MCP client/tool caches exist as a
  single shared instance for the whole process.

It deliberately imports only third-party libraries (never another ``backend``
module), so it sits at the bottom of the dependency graph and can never take
part in a circular import.
"""

from __future__ import annotations

import asyncio
import os
from contextlib import asynccontextmanager
from contextvars import ContextVar
from pathlib import PurePosixPath
from typing import Any, AsyncIterator

from dotenv import load_dotenv
from langchain_core.runnables import RunnableConfig
from langgraph.runtime import Runtime
from langgraph.store.base import BaseStore

# Load environment variables. Must happen before any module-level ``os.getenv``
# (here or in other backend modules); ``backend/__init__.py`` imports this
# module first to guarantee that ordering.
load_dotenv()


# ---------------------------------------------------------------------------
# Process-wide singletons (shared mutable state)
#
# These MUST be defined exactly once for the whole process. Every other module
# imports these objects from here and mutates them in place — never rebinds
# them — so sandbox resolution and MCP caching see a single shared instance.
# ---------------------------------------------------------------------------

# MCP adapter + failed-discovery caches, keyed by the tuple of server names. The value type
# of ``_mcp_clients`` is ``langchain.mcp.MCPAdapter``; it is typed ``Any`` here to keep this leaf
# module free of the beta MCP import (and its one-time warning). Successful discovery is cached by
# the adapter's FastMCP client according to the server-provided TTL, not in ``_mcp_tools_cache``.
_mcp_clients: dict[tuple[str, ...], Any] = {}
_mcp_tools_cache: dict[tuple[str, ...], list[Any]] = {}
_mcp_tools_locks: dict[tuple[str, ...], asyncio.Lock] = {}
# Whether each hosted MCP completed its tool handshake in this process. Kept beside the
# corresponding caches so the graph factory and the custom status route always see the same
# object, even when LangGraph loads the graph module through a second import path.
_mcp_statuses: dict[str, bool] = {}

# Holds the per-request sandbox so MCP tool wrappers can write large results
# to disk instead of truncating. Set in agent() before the graph runs.
_active_sandbox: ContextVar[Any] = ContextVar("_active_sandbox", default=None)

# Holds the per-request active project id + thread id so coordinator middleware
# can scope the research-project spine to the right Project (explicit Projects,
# P5). Set in agent() from ``configurable`` before the graph runs; ``None`` when
# the frontend did not pass one (the middleware falls back to the user's default
# project). Same shared-ContextVar pattern as ``_active_sandbox``.
_active_project_id: ContextVar[str | None] = ContextVar(
    "_active_project_id", default=None
)
_active_thread_id: ContextVar[str | None] = ContextVar(
    "_active_thread_id", default=None
)

# Holds the per-request Asta access token (per-user, paste-and-store) so the
# sandbox authenticates the `asta` CLI with the caller's own token. Set in
# agent() from Vault (Vault mode) or ``configurable.__asta_token`` (client mode);
# the sandbox falls back to the process-wide ``ASTA_TOKEN`` env var when unset.
_active_asta_token: ContextVar[str | None] = ContextVar(
    "_active_asta_token", default=None
)


async def resolve_asta_token(
    user_id: str | None, client_token: str | None = None
) -> str | None:
    """Resolve the caller's Asta token: client-supplied first, else their Vault.

    Shared by ``agent()`` and by the status-poll routes so token resolution never
    drifts between the two. Best-effort: any Vault error yields ``None`` so the
    sandbox's process-wide ``ASTA_TOKEN`` env fallback still applies.
    """
    if client_token:
        return str(client_token)
    if not user_id:
        return None
    import backend.vault as vault_store  # noqa: PLC0415 — avoid import cycle at load

    try:
        return await vault_store.get_asta_token(user_id)
    except Exception:  # noqa: BLE001 — Vault optional; env fallback covers it
        return None


@asynccontextmanager
async def asta_token_scope(
    user_id: str | None, client_token: str | None = None
) -> AsyncIterator[str | None]:
    """Bind ``_active_asta_token`` to the caller's token for the block's duration.

    The theorizer/DataVoyager status polls run in plain HTTP routes, *outside*
    ``agent()`` — the only place that used to set this ContextVar — so without
    this they authenticate the sandbox `asta` CLI with the stale process-wide
    ``ASTA_TOKEN`` env var instead of the user's freshly refreshed token, making a
    running task poll as "still running" forever. Wrapping the poll in this scope
    fixes that. The token is reset on exit so it never leaks across requests.
    """
    token = await resolve_asta_token(user_id, client_token)
    reset = _active_asta_token.set(str(token) if token else None)
    try:
        yield token
    finally:
        _active_asta_token.reset(reset)

# Process-wide marker: which (assistant_id, thread_id) sandboxes have already
# had their skill files uploaded in this Python process. Skill files are
# static within a process lifetime; re-uploading them on every user message
# adds several seconds of sandbox round-trip before the first model call.
_thread_skills_synced: set[tuple[str, str]] = set()


# ---------------------------------------------------------------------------
# Runtime mode + identity
# ---------------------------------------------------------------------------

def _runtime_mode() -> str:
    mode = (os.getenv("DEEP_ATD_RUNTIME_MODE") or "").strip().lower()
    if not mode:
        return "local"
    if mode in {"local", "dev", "development"}:
        return "local"
    if mode in {"production", "prod"}:
        return "production"
    raise ValueError(
        "DEEP_ATD_RUNTIME_MODE must be one of: local, dev, development, production, prod"
    )


def _is_production_mode() -> bool:
    return _runtime_mode() == "production"


def _user_id_from_config(config: RunnableConfig) -> str:
    """Resolve the authenticated user id for per-user Vault access.

    langgraph_api injects the auth user into ``configurable`` (see
    ``langgraph_api/models/run.py``). Falls back to a local id outside
    production so ``langgraph dev`` works without WorkOS.
    """
    configurable = config.get("configurable") or {}
    user_id = configurable.get("langgraph_auth_user_id")
    if user_id:
        return str(user_id)
    user = configurable.get("langgraph_auth_user")
    identity = getattr(user, "identity", None) if user is not None else None
    if identity:
        return str(identity)
    if _is_production_mode():
        raise ValueError("Authenticated user identity is required for Vault key access")
    return os.getenv("DEEP_ATD_LOCAL_USER_ID", "local-user")


def _require_server_identity(runtime: Runtime[Any]) -> tuple[str, str]:
    """Return assistant and user identifiers injected by LangGraph Server."""
    server_info = runtime.server_info
    assistant_id = getattr(server_info, "assistant_id", None) if server_info is not None else None
    user = getattr(server_info, "user", None) if server_info is not None else None
    user_id = getattr(user, "identity", None) if user is not None else None

    if assistant_id and user_id:
        return str(assistant_id), str(user_id)

    if _is_production_mode():
        raise ValueError("Authenticated server_info with assistant_id and user identity is required")

    return (
        os.getenv("DEEP_ATD_LOCAL_ASSISTANT_ID", "local-assistant"),
        os.getenv("DEEP_ATD_LOCAL_USER_ID", "local-user"),
    )


# ---------------------------------------------------------------------------
# Store namespaces
# ---------------------------------------------------------------------------

def _skills_namespace(assistant_id: str) -> tuple[str, ...]:
    return ("skills", assistant_id)


def _skills_namespace_for_runtime(runtime: Runtime[Any]) -> tuple[str, ...]:
    assistant_id, _ = _require_server_identity(runtime)
    return _skills_namespace(assistant_id)


def _memory_namespace(assistant_id: str, user_id: str) -> tuple[str, ...]:
    return (assistant_id, user_id)


def _memory_namespace_for_runtime(runtime: Runtime[Any]) -> tuple[str, ...]:
    assistant_id, user_id = _require_server_identity(runtime)
    return _memory_namespace(assistant_id, user_id)


# The stable id of the auto-created fallback project. A run whose config carried
# no explicit project_id (or a thread with no mapping yet) lands here, so the
# spine is never lost. The frontend normally always passes an explicit id.
DEFAULT_PROJECT_ID = "default"

#: The project the current HTTP request is about, for a conversation with no Project.
#:
#: A ``ContextVar`` rather than an argument because ``_project_namespace`` is called
#: several frames below the route handler. Per-context, so two concurrent requests
#: cannot read each other's.
_http_project_scope: ContextVar[str] = ContextVar("_http_project_scope", default="")

#: The conversation the current HTTP request is about, when it is in no Project.
#:
#: Separate from the project scope rather than folded into it: a route that received
#: both would have to decide which wins, and the answer differs by caller. The rule is
#: stated once, in :func:`_current_scope`, and both variables are plain inputs to it.
_http_thread_scope: ContextVar[str] = ContextVar("_http_thread_scope", default="")

#: A run's own ``configurable`` key naming the project folder a workspace shares. Read
#: here (not imported from ``backend.local.workspace``, to avoid a circular import) —
#: the two modules agree on the literal value deliberately, the same way the workspace
#: folder and this spine agree about which conversation they belong to.
_WORKSPACE_PROJECT_KEY = "__workspace_project__"

#: The ``configurable`` key LangGraph puts the conversation id under, inside a run.
_THREAD_KEY = "thread_id"


def _sanitise_scope(name: str) -> str:
    """One namespace segment for a project name, matching the folder it is stored beside.

    The **same** rule as ``backend.local.workspace.workspace_project``, deliberately: a
    project whose spine and whose folder disagreed about punctuation would be two
    projects wearing one name.
    """
    name = (name or "").strip()
    if not name:
        return ""
    cleaned = "".join(
        character if (character.isalnum() or character in " -_") else "_" for character in name
    ).strip(" ._")
    return cleaned[:96]


def _solo_scope(thread_id: str) -> str:
    """The spine segment for a conversation that is in no project, or ``""`` for none.

    Prefixed so the segment cannot be mistaken for a project name in the store, and so
    a person reading the keys can see at a glance which records belong to a single
    conversation.
    """
    cleaned = _sanitise_scope(thread_id)
    return f"solo-{cleaned}" if cleaned else ""


def _current_scope() -> str:
    """Whose spine a project-namespace call is about: a project's, one conversation's, or nobody's.

    **A project always wins over a conversation.** Filing a conversation into a project
    is a statement that its work belongs with the rest of that project's.

    HTTP first, then the run config, because only one of the two is ever populated: a
    route handler sets the ContextVars and has no run config; a turn has a run config
    and never touches them. A call that named neither falls through to an empty
    segment — the pre-Projects shared record, left reachable deliberately for a caller
    that names neither.
    """
    from_request = _http_project_scope.get()
    if from_request:
        return _sanitise_scope(from_request)
    from_thread = _http_thread_scope.get()
    if from_thread:
        return _solo_scope(from_thread)
    try:
        from langgraph.config import get_config  # noqa: PLC0415

        configurable = (get_config() or {}).get("configurable") or {}
        project = _sanitise_scope(str(configurable.get(_WORKSPACE_PROJECT_KEY) or ""))
        if project:
            return project
        # The turn's own conversation. Without this an ungrouped run writes its mission
        # and its plan into the record every other ungrouped conversation reads.
        return _solo_scope(str(configurable.get(_THREAD_KEY) or ""))
    except Exception:  # noqa: BLE001 — outside a run and outside a request
        return ""


def _project_namespace(user_id: str, project_id: str) -> tuple[str, ...]:
    """Namespace for one Project's persistent spine (mission/completed/pending/plan).

    Project-scoped as ``(user_id, "project", project_id)`` (explicit Projects, P5)
    — this replaces P3's single per-user ``(user_id, "project")`` record so each
    named Project keeps its own mission and run-loop plan. Deliberately **not**
    keyed by assistant_id (unlike memories): a Project is the user's, spanning
    every assistant/thread, and this lets the stateless ``/project`` /
    ``/projects`` HTTP routes rebuild the exact namespace from
    ``request.user.identity`` + the project id (a custom route cannot see the
    platform's assistant_id). ``user_id`` first also matches the
    ``@auth.on.store`` guard, which forces client-facing store ops under
    ``(user_id, ...)``.

    A fourth segment further scopes an ungrouped conversation to itself rather than to
    the one shared record every ungrouped conversation used to read — see
    :func:`_current_scope`. Every conversation that predates this scoping stays where
    it is, at the three-segment namespace, because a call that names neither an HTTP
    project/thread nor a run config resolves to an empty scope.
    """
    scope = _current_scope()
    base = (user_id, "project", project_id)
    return (*base, scope) if scope else base


def _projects_registry_namespace(user_id: str) -> tuple[str, ...]:
    """Namespace for the user's Project registry (one item per project).

    Each item is keyed by ``project_id`` with value ``{id, name, created_at,
    updated_at}``. The spine for a project lives separately under
    :func:`_project_namespace`.
    """
    return (user_id, "projects")


def _thread_index_namespace(user_id: str) -> tuple[str, ...]:
    """Namespace for the thread→project map (item key = thread_id).

    Lets a run (and stateless routes) resolve which Project a conversation
    belongs to, so reopening a thread restores its project even if the client's
    local state was lost.
    """
    return (user_id, "threads")


def _project_namespace_for_runtime(
    runtime: Runtime[Any], project_id: str
) -> tuple[str, ...]:
    _, user_id = _require_server_identity(runtime)
    return _project_namespace(user_id, project_id)


# ---------------------------------------------------------------------------
# Store + path helpers
# ---------------------------------------------------------------------------

def _safe_relative_path(key: str) -> str:
    """Reject path traversal and glob characters while preserving subdirectories."""
    rel_path = PurePosixPath(str(key).lstrip("/"))
    if not rel_path.parts:
        raise ValueError(f"Invalid key: {key}")

    for part in rel_path.parts:
        if part in ("", ".", "..") or any(char in part for char in ("*", "?")):
            raise ValueError(f"Invalid key: {key}")

    return rel_path.as_posix()


async def _asearch_all(
    store: BaseStore,
    namespace: tuple[str, ...],
    *,
    batch_size: int = 100,
) -> list[Any]:
    """Fetch every item under a store namespace."""
    results: list[Any] = []
    offset = 0
    while True:
        batch = await store.asearch(namespace, limit=batch_size, offset=offset)
        if not batch:
            break
        results.extend(batch)
        if len(batch) < batch_size:
            break
        offset += batch_size
    return results


def _get_thread_id(config: RunnableConfig) -> str:
    configurable = config.get("configurable") or {}
    thread_id = configurable.get("thread_id")
    if thread_id:
        return str(thread_id)

    require_thread_id = bool(configurable.get("__is_for_execution__")) and (
        os.getenv("DEEP_ATD_REQUIRE_THREAD_ID", "").lower() in {
            "1",
            "true",
            "yes",
            "on",
        }
        or _is_production_mode()
    )
    if require_thread_id:
        raise ValueError("RunnableConfig must include configurable.thread_id for sandbox scoping")

    # LangGraph also instantiates graph factories for assistant inspection
    # endpoints (`/graph`, `/schemas`, `/subgraphs`) with no thread_id. Those
    # requests do not execute agent nodes or resolve a real sandbox, so a
    # stable preview ID is sufficient.
    return os.getenv("DEEP_ATD_LOCAL_THREAD_ID", "langgraph-dev-preview")

"""Coordinator middleware for the persistent research-project spine.

Runs at the coordinator level, so its ``aafter_agent`` fires at the very end of
the whole subagent chain (same timing as ``SandboxSyncMiddleware``) — by which
point ``state["artifacts"]`` holds every subagent's merged output. It:

  * loads the per-user project from the LangGraph store (cross-thread),
  * seeds/updates the mission + Completed Work from this turn's artifacts,
  * derives 1–3 advisory "suggested next steps",
  * persists the updated project back to the store, and
  * emits the project as an artifact slice so the frontend can render it.

``abefore_agent`` emits the stored project up front too, so reopening the app —
or opening a brand-new thread — shows the persistent mission immediately,
before the turn produces anything.

Advisory only: this middleware never executes a subagent. See
:mod:`backend.project` for the (pure) derivation logic.
"""

from collections.abc import Awaitable, Callable
from typing import Any

from langgraph.runtime import Runtime
from langchain.agents.middleware import AgentMiddleware, ModelRequest, ModelResponse

from backend.project import (
    advance_project,
    build_project_payload,
    has_content,
    load_project,
    render_mission_context,
    save_project,
)
from backend.projects import resolve_active_project_id
from backend.runtime import (
    _active_project_id,
    _active_thread_id,
    _project_namespace,
    _require_server_identity,
)
from backend.schemas import ArtifactState, ProjectArtifactPayload


async def _active_project_namespace(runtime: Runtime) -> tuple[str, ...]:
    """Resolve the store namespace for the run's active Project (explicit Projects, P5).

    The active project id rides on the run via ``configurable.project_id`` (set
    into a ContextVar by ``agent()``); ``resolve_active_project_id`` falls back to
    the thread's stored mapping, then the user's default project, and persists
    the association so a reopened thread lands in the same place.
    """
    _, user_id = _require_server_identity(runtime)
    project_id = await resolve_active_project_id(
        runtime.store,
        user_id,
        explicit_project_id=_active_project_id.get(),
        thread_id=_active_thread_id.get(),
    )
    return _project_namespace(user_id, project_id)


def _artifact_update(project: ProjectArtifactPayload) -> dict[str, Any]:
    """Wrap a project payload as a partial ``ArtifactBundle`` state update.

    The empty list slices are intentional: the ``_merge_artifacts`` reducer
    unions them with existing artifacts (a no-op), and omitting hypotheses /
    libraries preserves them. Only ``project`` (last-write-wins) is replaced.
    """
    return {
        "artifacts": {
            "datasets": [],
            "sources": [],
            "reports": [],
            "files": [],
            "project": project,
        }
    }


class ProjectSpineMiddleware(AgentMiddleware[ArtifactState, Any, Any]):
    """Persist the research-project spine and surface advisory next steps."""

    state_schema = ArtifactState

    async def abefore_agent(
        self, state: ArtifactState, runtime: Runtime
    ) -> dict[str, Any] | None:
        """Surface the stored project (mission + prior work) at turn start.

        Uses artifacts already in state (from the thread's checkpoint) so any
        suggestions from prior turns show immediately; does not persist.
        """
        store = runtime.store
        if store is None:
            # Advisory-only feature: never break a run over a missing store.
            return None

        namespace = await _active_project_namespace(runtime)
        prev = await load_project(store, namespace)
        artifacts = state.get("artifacts") or {}
        messages = state.get("messages") or []

        new_state, suggestions = advance_project(prev, artifacts, messages)
        payload = build_project_payload(new_state, suggestions)
        if not has_content(payload):
            return None
        return _artifact_update(payload)

    async def aafter_agent(
        self, state: ArtifactState, runtime: Runtime
    ) -> dict[str, Any] | None:
        """Fold this turn's artifacts into the project, persist, and re-emit."""
        store = runtime.store
        if store is None:
            return None

        namespace = await _active_project_namespace(runtime)
        prev = await load_project(store, namespace)
        artifacts = state.get("artifacts") or {}
        messages = state.get("messages") or []

        new_state, suggestions = advance_project(prev, artifacts, messages)
        await save_project(store, namespace, new_state)

        payload = build_project_payload(new_state, suggestions)
        if not has_content(payload):
            return None
        return _artifact_update(payload)

    async def awrap_model_call(
        self,
        request: ModelRequest,
        handler: Callable[[ModelRequest], Awaitable[ModelResponse]],
    ) -> ModelResponse:
        """Inject the active project's mission into the coordinator system prompt.

        This is the half of the spine that actually reaches the *model*: the
        before/after hooks only emit the mission as a frontend artifact, so
        without this the agent never reads the mission the user set — editing it
        changes what is shown, not how the agent behaves. We load the mission
        fresh from the store each call (a single fast KV get) and append it to the
        assembled system prompt, leaving the large static coordinator prompt as a
        stable, cacheable prefix. No mission ⇒ pass through untouched.
        """
        runtime = request.runtime
        store = getattr(runtime, "store", None)
        if store is None:
            return await handler(request)

        namespace = await _active_project_namespace(runtime)
        state = await load_project(store, namespace)
        block = render_mission_context(state)
        if not block:
            return await handler(request)

        base = request.system_prompt or ""
        merged = f"{base}\n\n{block}" if base else block
        return await handler(request.override(system_prompt=merged))

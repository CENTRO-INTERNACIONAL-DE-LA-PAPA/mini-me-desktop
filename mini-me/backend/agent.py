"""LangGraph graph factory and filesystem-backend composition.

``agent(config)`` is the deployed graph factory (wired via langgraph.json). It
acquires a thread-scoped LangSmith Sandbox lazily, resolves per-request model
routing + keys, loads MCP tools, assembles the runtime subagents and guardrail
middleware, and returns a deep agent. ``make_backend`` composes the agent's
filesystem backend (sandbox + store-backed /skills and /memories routes).
"""

from langchain_core.runnables import RunnableConfig

from deepagents import create_deep_agent
from deepagents.backends import CompositeBackend, StoreBackend

from backend.runtime import (
    _active_asta_token,
    _active_project_id,
    _active_sandbox,
    _active_thread_id,
    _get_thread_id,
    _is_production_mode,
    _memory_namespace_for_runtime,
    _skills_namespace_for_runtime,
    _user_id_from_config,
    resolve_asta_token,
)
from backend.models import _build_model_resolver, _require_model_keys
from backend.sandbox import LazyLangsmithSandbox
from backend.mcp_tools import (
    get_academic_research_mcp_tools,
    get_data_cleaning_mcp_tools,
    get_dataverse_search_mcp_tools,
    _tool_names,
)
from backend.middleware import (
    ArtifactCaptureMiddleware,
    FileSyncMiddleware,
    ProjectSpineMiddleware,
    SandboxSyncMiddleware,
    _build_filesystem_permissions,
    _build_guardrail_middleware,
)
from backend.datavoyager_tools import analyze_data
from backend.prompts import COORDINATOR_SYSTEM_PROMPT
from backend.subagents import _build_runtime_subagents, request_diagnostic_context
from backend.paper_tools import find_papers
from backend.theory_tools import generate_theories


def make_backend(sandbox_backend: "LazyLangsmithSandbox"):
    """Compose the agent's filesystem backend.

    Layout:
      - default → sandbox (Python/shell execution surface for the agent)
      - ``/skills/`` → LangGraph store, namespaced per assistant_id
        (skills are config-like, cached per-process via
        ``_thread_skills_synced``; the store survives process restarts)
      - ``/memories/`` → LangGraph store, namespaced per
        (assistant_id, user_id) so each researcher has their own
        scratch memory. In ``langgraph dev`` the store is in-memory and
        loses content on process restart; production deployments should
        configure a durable LangGraph store (Postgres / Redis) so
        memories survive.

    Context Hub was tried for ``/memories/`` (Phases 3 + 3.1) but is
    designed for shared org-level context (agent definitions, skills,
    policies) — not per-user scratch state. Reverted to ``StoreBackend``
    which already gives per-user isolation natively via namespaces.
    """
    return CompositeBackend(
        default=sandbox_backend,
        routes={
            "/memories/": StoreBackend(
                namespace=_memory_namespace_for_runtime,
            ),
            "/skills/": StoreBackend(
                namespace=_skills_namespace_for_runtime,
            ),
        },
    )


async def agent(config: RunnableConfig):
    """LangGraph factory for a thread-scoped LangSmith Sandbox.

    The sandbox is acquired lazily — the factory returns immediately so
    read-only requests (history fetches, thread switches, page reloads)
    do not pay sandbox startup cost. The real sandbox is created on first
    node execution via ``LazyLangsmithSandbox.aresolve()``.
    """
    thread_id = _get_thread_id(config)
    sandbox_backend = LazyLangsmithSandbox(thread_id)
    _active_sandbox.set(sandbox_backend)

    # Expose the run's active Project + thread to coordinator middleware
    # (explicit Projects, P5). The frontend passes ``project_id`` on
    # ``configurable`` each run; ``ProjectSpineMiddleware`` reads these
    # ContextVars to scope the research-project spine to the right Project.
    configurable = config.get("configurable") or {}
    project_id = configurable.get("project_id")
    _active_project_id.set(str(project_id) if project_id else None)
    _active_thread_id.set(thread_id)

    # Resolve the caller's own Asta access token (per-user, paste-and-store) so
    # the sandbox authenticates the `asta` CLI as this user. Prefer a
    # client-supplied token (client mode; ``__``-prefixed so it never lands in a
    # trace), else the token stored in the user's Vault. The sandbox falls back
    # to the process-wide ``ASTA_TOKEN`` env var when neither is set.
    asta_token = await resolve_asta_token(
        _user_id_from_config(config), configurable.get("__asta_token")
    )
    _active_asta_token.set(str(asta_token) if asta_token else None)

    # Resolve the per-request model routing + keys (Vault or client-supplied),
    # then block real runs that have no usable key for a selected provider.
    # Only enforced in production: locally, ``init_chat_model`` still falls back
    # to provider env vars (e.g. OPENAI_API_KEY) so ``langgraph dev`` works
    # without the config panel.
    model_resolver, subagent_overrides, is_execution = await _build_model_resolver(config)
    if is_execution and _is_production_mode():
        _require_model_keys(model_resolver, subagent_overrides)
    coordinator_model = model_resolver.coordinator()

    # `find_papers` first, and alongside the MCP bundle rather than instead of it. It returns
    # each paper with its reference already built from the record (`backend/citations.py`), which
    # is what stops the model composing one from memory; `snippet_search` stays available for the
    # separate job of quoting a passage out of a paper's body.
    academic_research_tools = [
        find_papers,
        *await get_academic_research_mcp_tools(),
    ]
    dataverse_tools = await get_dataverse_search_mcp_tools()
    data_cleaning_tools = await get_data_cleaning_mcp_tools()
    external_tool_names = _tool_names(
        [
            *academic_research_tools,
            *dataverse_tools,
            *data_cleaning_tools,
        ]
    )
    file_sync = FileSyncMiddleware(sandbox_backend)
    runtime_subagents = _build_runtime_subagents(
        academic_research_tools=academic_research_tools,
        dataverse_tools=dataverse_tools,
        data_cleaning_tools=data_cleaning_tools,
        diagnostic_tools=[request_diagnostic_context],
        theory_tools=[generate_theories],
        datavoyager_tools=[analyze_data],
        file_sync=file_sync,
        model_resolver=model_resolver,
        subagent_overrides=subagent_overrides,
    )

    backend = make_backend(sandbox_backend=sandbox_backend)
    permissions = _build_filesystem_permissions()
    middleware = [
        *_build_guardrail_middleware(external_tool_names),
        ArtifactCaptureMiddleware(),
        SandboxSyncMiddleware(sandbox_backend),
        # Advisory research-project spine: persists the per-user mission and
        # surfaces artifact-grounded next steps. Last so it reads the fully
        # merged artifacts; never executes anything.
        ProjectSpineMiddleware(),
    ]

    return create_deep_agent(
        model=coordinator_model,
        system_prompt=COORDINATOR_SYSTEM_PROMPT,
        subagents=runtime_subagents,
        skills=["/skills/"],
        memory=["/memories/instructions.txt"],
        backend=backend,
        middleware=middleware,
        permissions=permissions,
        name="AsktheData-Agent",
    )

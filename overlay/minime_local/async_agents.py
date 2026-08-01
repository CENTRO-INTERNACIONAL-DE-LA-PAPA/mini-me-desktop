"""Async subagents: launch work in the background and keep the conversation live.

**What this gives the researcher.** Today every subagent except the theorizer and
DataVoyager blocks the chat — an EDA or a report can freeze the conversation for minutes.
With this, the coordinator can hand a whole piece of work to a *background Mini-Me*, say
so, and hand control straight back. You keep asking questions while it runs.

**Why it needs no fork.** `deepagents.AsyncSubAgentMiddleware` wants each async subagent to
be a **graph on the Agent Protocol server**, and Mini-Me declares one graph — which looked
like a structural upstream change (docs §14). Three measured facts made it an *extension*
instead:

1. `AsyncSubAgent` is a **reference** — `{name, description, graph_id, url}` — and
   `url=None` selects the in-process ASGI transport, so no network hop, no second server
   and no auth (verified in `deepagents/middleware/async_subagents.py:_ClientCache`).
2. `langgraph dev` accepts `--config PATH`, and the desktop app builds the launch command.
   So the *client* can declare extra graphs without editing the checkout's own config.
3. `backend/agent.py:agent` is an **async factory**. A second graph id can point at the
   very same factory, so the background worker is a real Mini-Me with every subagent and
   tool it normally has — no reassembly of upstream's wiring, and nothing to keep in sync.

**Why a whole coordinator rather than one subagent per graph.** Rebuilding a single
subagent standalone would mean replicating `_build_runtime_subagents`' MCP tool fetches,
model resolution and middleware selection in this file — precisely the duplicated logic
that becomes merge debt the first time upstream changes. Delegating to a background
*coordinator* reuses upstream's own assembly verbatim, and is strictly more capable: the
worker can chain subagents, run its own analysis and write a report.

This works in our deployment specifically because execution is **local** (docs §19): the
background worker shares the researcher's filesystem, so files it writes are simply there.
Under the remote sandbox each thread gets its own, and results would land somewhere the
user's thread cannot see.
"""

from __future__ import annotations

import contextvars
import logging

logger = logging.getLogger(__name__)

# The graph id our generated config registers for background work. Must match the id the
# desktop app writes into `langgraph.json` (`backend.rs: async_graph_config`).
BACKGROUND_GRAPH_ID = "background"

# Set while a *background* coordinator is being constructed.
#
# Without this, the background worker would itself be handed `start_async_task` and could
# spawn another background worker, and so on. One level of delegation is the feature; a
# tree of them is a runaway that bills the user's model key.
_BUILDING_BACKGROUND: contextvars.ContextVar[bool] = contextvars.ContextVar(
    "minime_local_building_background", default=False
)


def building_background() -> bool:
    """Whether the agent currently being built is the background worker."""
    return _BUILDING_BACKGROUND.get()


def async_subagent_specs() -> list[dict]:
    """The async subagents the coordinator is given.

    One entry. `url=None` is the load-bearing part: it selects the in-process transport,
    so this costs no port, no second process and no credentials.
    """
    return [
        {
            "name": "background_worker",
            "graph_id": BACKGROUND_GRAPH_ID,
            "url": None,
            "description": (
                "A full Mini-Me instance that works in the background while you keep "
                "talking to the researcher. Give it a complete, self-contained piece of "
                "work — a literature review, an exploratory analysis of a dataset, a "
                "written report — including every path and detail it needs, because it "
                "cannot ask you questions. Use it for anything that would otherwise "
                "make the researcher wait several minutes. Start it, tell the "
                "researcher it is running, and continue; check it only when asked."
            ),
        }
    ]


async def background_graph():
    """Build the graph the background worker runs.

    Upstream's own coordinator factory, with async-subagent injection suppressed for the
    duration — see `_BUILDING_BACKGROUND`.
    """
    from backend.agent import agent as upstream_agent

    token = _BUILDING_BACKGROUND.set(True)
    try:
        return await upstream_agent()
    finally:
        _BUILDING_BACKGROUND.reset(token)


def install(deepagents_module) -> None:
    """Give every coordinator the background-work tools.

    Wrapped on the ``deepagents`` package, for the same reason the approval gate is:
    LangGraph loads the graph module from a file path, so `backend/agent.py` never passes
    through the import hook — but its ``from deepagents import create_deep_agent`` does
    read the package attribute we set here (docs §18).

    Chains cleanly with the approval wrapper: whichever installs second wraps the first,
    and both effects apply.
    """
    if not enabled():
        return
    original = getattr(deepagents_module, "create_deep_agent", None)
    if original is None:
        logger.warning("minime_local: no create_deep_agent to wrap for async subagents")
        return

    def create_deep_agent_with_background(*args, **kwargs):
        extra = middleware_for(deepagents_module)
        if extra is not None:
            kwargs["middleware"] = [*kwargs.get("middleware", []), extra]
        return original(*args, **kwargs)

    deepagents_module.create_deep_agent = create_deep_agent_with_background
    logger.warning("minime_local: background work is available to the coordinator")


def enabled() -> bool:
    """Whether background work is switched on.

    Off unless the desktop app asks for it, because it only functions when the app has
    also registered the extra graph in a generated config — and a coordinator holding
    tools that point at a graph the server does not serve would fail at the worst moment,
    mid-task, rather than at startup.
    """
    import os

    return os.environ.get("MINIME_ASYNC_SUBAGENTS", "").strip() not in ("", "0", "false")


def middleware_for(deepagents_module):
    """The `AsyncSubAgentMiddleware` to add, or `None` if it isn't available.

    Returns `None` rather than raising: async subagents are an enhancement, and a
    deepagents that lacks them should still give the researcher a working coordinator.
    """
    if building_background():
        return None
    factory = getattr(deepagents_module, "AsyncSubAgentMiddleware", None)
    if factory is None:
        logger.warning(
            "minime_local: this deepagents has no AsyncSubAgentMiddleware; "
            "background work is unavailable"
        )
        return None
    specs = async_subagent_specs()
    try:
        return _forwarding_config(factory(async_subagents=specs), specs)
    except Exception as exc:  # noqa: BLE001
        # A preview API that changed shape must not take the whole agent down with it.
        logger.warning("minime_local: could not build AsyncSubAgentMiddleware: %s", exc)
        return None


# What travels from the conversation's run onto the background run.
#
# An **allowlist**, not a copy. `configurable` also holds `thread_id`, `checkpoint_ns` and
# `run_id`; forwarding those would point the background run at the conversation's own
# thread and corrupt it.
FORWARDED_CONFIG_KEYS = ("model_config", "__llm_keys", "__is_for_execution__")

# What the recursion limit falls back to when the parent run has none.
#
# LangGraph's default is 25 supersteps and a Mini-Me coordinator spends ~22 on middleware
# alone before it delegates anything, so a background worker started at the default fails
# almost immediately. Same value the desktop app and the web frontend send (docs §37).
BACKGROUND_RECURSION_LIMIT = 10_000


def _forwarded_config() -> dict:
    """The run config to start background work with, taken from the live run.

    Upstream's `start_async_task` calls `client.runs.create(thread_id, assistant_id,
    input=…)` and passes **no config at all**. For a hosted deployment that is fine — the
    server holds the keys and the defaults suit it. Here it is fatal twice over:

    * the model and its key travel *in the request* (docs §20), so a config-less run has
      neither and cannot construct a model;
    * `recursion_limit` falls back to 25, and this background worker is a whole
      coordinator — it burns most of that on middleware before doing any work.

    Reading the parent run's own config means the background worker runs on exactly the
    model the researcher picked, with exactly the budget their own turns get.
    """
    try:
        from langgraph.config import get_config

        config = get_config() or {}
    except Exception as exc:  # noqa: BLE001  # no runnable context, or an SDK change
        logger.warning("minime_local: no live run config to forward to background work: %s", exc)
        config = {}

    configurable = config.get("configurable") or {}
    forwarded = {key: configurable[key] for key in FORWARDED_CONFIG_KEYS if configurable.get(key)}
    if "model_config" not in forwarded:
        # Worth saying out loud: the run will still start, and will still fail later on a
        # model it could not build. This line is the difference between a diagnosable
        # failure and "The async subagent encountered an error".
        logger.warning(
            "minime_local: starting background work with no model config — "
            "the worker will fall back to the server default and may not have a key"
        )
    return {
        "recursion_limit": config.get("recursion_limit") or BACKGROUND_RECURSION_LIMIT,
        "configurable": forwarded,
    }


def _forwarding_config(middleware, specs: list[dict]):
    """`middleware`, with `start_async_task` replaced by one that forwards our config.

    Only the *launch* tool is ours. Checking, updating, cancelling and listing are
    upstream's untouched — they address a run by id and need nothing we have.

    Degrades to the unmodified middleware if the tool cannot be found or rebuilt: a
    background worker that starts with upstream's defaults is worse than this one, but it
    is a great deal better than a coordinator with no background tools at all.
    """
    tools = list(getattr(middleware, "tools", None) or [])
    index = next(
        (i for i, tool in enumerate(tools) if getattr(tool, "name", "") == "start_async_task"),
        None,
    )
    if index is None:
        logger.warning(
            "minime_local: no start_async_task tool to wrap; background work will run "
            "on the server's default model and recursion limit"
        )
        return middleware

    original = tools[index]
    by_name = {spec["name"]: spec for spec in specs}

    async def start_async_task(description: str, subagent_type: str, runtime) -> object:
        from datetime import UTC, datetime

        from langchain_core.messages import ToolMessage
        from langgraph.types import Command
        from langgraph_sdk import get_client

        spec = by_name.get(subagent_type)
        if spec is None:
            allowed = ", ".join(f"`{name}`" for name in by_name)
            return f"Unknown async subagent type `{subagent_type}`. Available types: {allowed}"

        try:
            client = get_client(url=spec.get("url"))
            thread = await client.threads.create()
            run = await client.runs.create(
                thread_id=thread["thread_id"],
                assistant_id=spec["graph_id"],
                input={"messages": [{"role": "user", "content": description}]},
                config=_forwarded_config(),
            )
        except Exception as exc:  # noqa: BLE001  # the LangGraph SDK raises untyped errors
            logger.warning("minime_local: failed to launch background work: %s", exc)
            return f"Failed to launch async subagent '{subagent_type}': {exc}"

        # The middleware keys tasks by thread id, and `check_async_task` looks them up
        # that way — so this must stay the thread id, not the run id.
        task_id = thread["thread_id"]
        now = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")
        return Command(
            update={
                "messages": [
                    ToolMessage(
                        f"Launched async subagent. task_id: {task_id}",
                        tool_call_id=runtime.tool_call_id,
                    )
                ],
                "async_tasks": {
                    task_id: {
                        "task_id": task_id,
                        "agent_name": subagent_type,
                        "thread_id": task_id,
                        "run_id": run["run_id"],
                        "status": "running",
                        "created_at": now,
                        "last_checked_at": now,
                        "last_updated_at": now,
                    }
                },
            }
        )

    try:
        from langchain_core.tools import StructuredTool

        # Name, description and schema come from the tool we are replacing, so the model
        # sees the identical interface and upstream's prompt text stays authoritative.
        tools[index] = StructuredTool.from_function(
            name=original.name,
            # Upstream's sync path is left alone: it rejects `url=None` with a clear
            # message, and an async graph never reaches it.
            func=original.func,
            coroutine=start_async_task,
            description=original.description,
            infer_schema=False,
            args_schema=original.args_schema,
        )
    except Exception as exc:  # noqa: BLE001
        logger.warning("minime_local: could not wrap start_async_task: %s", exc)
        return middleware

    middleware.tools = tools
    logger.warning("minime_local: background work will run on the conversation's own model")
    return middleware

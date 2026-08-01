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
    try:
        return factory(async_subagents=async_subagent_specs())
    except Exception as exc:  # noqa: BLE001
        # A preview API that changed shape must not take the whole agent down with it.
        logger.warning("minime_local: could not build AsyncSubAgentMiddleware: %s", exc)
        return None

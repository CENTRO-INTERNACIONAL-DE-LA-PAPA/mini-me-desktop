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


#: Marks a run as *being* a background worker, carried in its own config.
#:
#: The `ContextVar` above is set around the factory call and is the primary guard. This is a
#: second, independent one, and it exists because the first is silent when it works: a worker that
#: was handed `start_async_task` anyway looks exactly like one that was not, right up until it
#: spawns another worker (docs §114). This signal travels *in the run's config*, which is the same
#: place the model and the workspace come from, so it cannot be lost to a context that did not
#: propagate across an `await`.
BACKGROUND_RUN_KEY = "__is_background__"


def building_background() -> bool:
    """Whether the agent being built is a background worker.

    Two sources, either sufficient. The ContextVar covers the build the factory drives; the config
    key covers the run itself, including any build that happens outside that context.
    """
    if _BUILDING_BACKGROUND.get():
        return True
    try:
        from langgraph.config import get_config

        configurable = (get_config() or {}).get("configurable") or {}
        return bool(configurable.get(BACKGROUND_RUN_KEY))
    except Exception:  # noqa: BLE001 — no live run, which is the coordinator's own build
        return False


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


async def background_graph(config):
    """Build the graph the background worker runs.

    Upstream's own coordinator factory, with async-subagent injection suppressed for the
    duration — see `_BUILDING_BACKGROUND`.

    **`config` is not optional and must be passed on.** `backend/agent.py` declares
    `async def agent(config: RunnableConfig)`, and that argument is the whole reason this
    app works: `_build_model_resolver(config)` reads the model and key out of it. Calling
    it with no argument — which this did, for three rounds — raises `TypeError` while the
    graph is being *constructed*, so the run dies before any checkpoint exists. That is
    why the failure had no error to read anywhere: there was no state to record it in, and
    the middleware's "The async subagent encountered an error" was all that survived
    (docs §39).

    The parameter is deliberately unannotated. The dev server classifies a factory by its
    signature and hands a lone un-annotated parameter the `RunnableConfig`
    (`langgraph_api/_factory_utils.py:_classify_factory`); annotating it `ServerRuntime`
    would silently pass the wrong object instead.
    """
    import inspect

    from backend.agent import agent as upstream_agent

    # Adaptive rather than hardcoded, because getting this wrong is not a visible error —
    # it is a run that dies with nothing to read. If upstream ever drops the parameter,
    # this keeps working; if it adds one with a default, it still gets the config.
    wants_config = bool(inspect.signature(upstream_agent).parameters)
    if not wants_config:
        logger.warning(
            "minime_local: backend.agent.agent takes no config — background work cannot "
            "be handed the researcher's model and key"
        )

    token = _BUILDING_BACKGROUND.set(True)
    try:
        return await (upstream_agent(config) if wants_config else upstream_agent())
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
        # **Said out loud.** One level of delegation is the feature; a worker that can spawn
        # workers is a runaway on the researcher's own model key, and the difference between the
        # guard working and the guard being bypassed was previously invisible — both produce a
        # coordinator that starts, and only one of them produces a tree (docs §114).
        logger.warning(
            "minime_local: background worker built WITHOUT start_async_task, as intended"
        )
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
#
# `__workspace_project__` is here for the same reason the thread pin below is: it decides which
# directory the worker writes into. Left out — as it was when projects shipped (docs §105) — a
# background worker pinned to the conversation's thread still wrote to the *root*, so its report
# landed outside the project whose conversation asked for it, and the app looked for it inside
# (docs §111).
FORWARDED_CONFIG_KEYS = (
    "model_config",
    "__llm_keys",
    "__is_for_execution__",
    "__workspace_project__",
)

# What the recursion limit falls back to when the parent run has none.
#
# LangGraph's default is 25 supersteps and a Mini-Me coordinator spends ~22 on middleware
# alone before it delegates anything, so a background worker started at the default fails
# almost immediately. Same value the desktop app and the web frontend send (docs §37).
BACKGROUND_RECURSION_LIMIT = 10_000


def _conversation_thread(config: dict, configurable: dict) -> tuple[str, str]:
    """The thread whose folder a background worker should write into, and where it came from.

    # Why this is a chain and not one key

    It was `configurable.get(WORKSPACE_THREAD_KEY) or configurable.get("thread_id")`, and on a real
    run neither was there — while `model_config` and `__workspace_project__`, read from the *same*
    `configurable` two lines above, arrived intact. So the worker inherited the conversation's
    project folder and **not** its thread, and wrote thirteen files to

        Documents/Mini-Me/test subagents/<the task's own id>/

    while the coordinator reported them under the conversation's id, and the Files panel showed
    neither. Six plots, produced correctly, that the researcher had to be told how to go and find.

    LangGraph does put `thread_id` in `configurable` — `pregel/main.py` reads
    `saved.config[CONF]["thread_id"]` — but evidently not in every context a tool call sees, and
    the version that matters is whichever is installed on a researcher's machine. So: try each
    source and **say which one answered**. A chain that fails silently would be the same bug with
    more code.

    Returns ``("", "nothing")`` when no source has it, which the caller reports rather than
    swallowing.
    """
    # Imported here, as the caller does: `minime_local.workspace` pulls in deepagents, and this
    # module is imported during graph construction where that is not yet guaranteed.
    from minime_local.workspace import WORKSPACE_THREAD_KEY

    pin = str(configurable.get(WORKSPACE_THREAD_KEY) or "").strip()
    if pin:
        # A worker started by a worker keeps the original conversation, not its parent.
        return pin, "an existing pin"
    for source, value in (
        ("configurable.thread_id", configurable.get("thread_id")),
        ("metadata.thread_id", (config.get("metadata") or {}).get("thread_id")),
        ("configurable.__thread_id__", configurable.get("__thread_id__")),
    ):
        text = str(value or "").strip()
        if text:
            return text, source
    return "", "nothing"


def _report_what_the_worker_will_bill(forwarded: dict) -> None:
    """Say which provider a background worker is about to spend money at, and on what.

    **The keys are the half nobody was watching.** The line above catches a missing `model_config`,
    which is the loud failure — no model at all. `__llm_keys` is the quiet one: the worker builds
    exactly the model the researcher chose, has no key or **no `base_url`** for it, and the client
    library falls back to its default host. On OpenRouter that means every background request goes
    to `api.openai.com` and comes back as *"You have no credits remaining"* pointing at an OpenAI
    billing page the researcher has never used — while the same conversation chats happily,
    because the coordinator's own turns carry the config directly (§211).

    That is §187 again, one layer down: a specialist billed to an account nobody chose, and the
    only symptom arriving minutes later inside a worker.

    **Never the key itself.** Provider names, the model spec, and whether a `base_url` came with
    each — which is exactly enough to tell "went to the wrong host" from "had no key at all", and
    nothing a log should not hold.
    """
    keys = forwarded.get("__llm_keys") or {}
    spec = (forwarded.get("model_config") or {}).get("default")
    if not keys:
        logger.warning(
            "minime_local: background work is starting with NO provider keys (model %s) — its "
            "requests will go wherever the client library defaults to, which is not where the "
            "conversation goes (docs §211)",
            spec or "<none>",
        )
        return
    described = ", ".join(
        "{}{}".format(provider, "" if (entry or {}).get("base_url") else " (no base_url)")
        for provider, entry in sorted(keys.items())
    )
    logger.warning(
        "minime_local: background work will bill %s, model %s", described, spec or "<none>"
    )

    # **A provider a spec names and no key covers is a guaranteed failure, minutes early.**
    # `§186` refuses a *turn* whose coordinator has no key; a specialist pointed at a second
    # provider with none still saves, and the first anyone hears of it is a 429 from a billing
    # account they never chose, raised inside a worker. The set is knowable right here, before the
    # run starts, and it is the difference between a diagnosable failure and "the subagent
    # encountered an error" (docs §211).
    model_config = forwarded.get("model_config") or {}
    referenced = {_provider_of(model_config.get("default"))}
    referenced.update(
        _provider_of(value) for value in (model_config.get("subagents") or {}).values()
    )
    missing = sorted(name for name in referenced if name and name not in keys)
    if missing:
        logger.warning(
            "minime_local: background work names %s and carries no key for %s — those requests "
            "will fail inside the worker, on whatever host the client library defaults to, which "
            "is how a 429 arrives from a billing account nobody chose (docs §211)",
            ", ".join(missing),
            "it" if len(missing) == 1 else "them",
        )


def _provider_of(spec) -> str:
    """The provider half of a `provider::model_id` spec, or `""`.

    Tolerant of the single-colon spelling as well, because the two have both been seen on this
    wire and a diagnostic that silently matches neither is worse than none.
    """
    if not isinstance(spec, str) or not spec.strip():
        return ""
    head = spec.split("::", 1)[0] if "::" in spec else spec.split(":", 1)[0]
    return head.strip()


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

    # Share the conversation's workspace. Note this is *not* forwarding `thread_id` — that
    # would point the run itself at the wrong thread and corrupt it. It is a separate key
    # read only when choosing a directory, so the worker's files land where the researcher
    # and the coordinator already look (docs §43). An existing pin wins, so a worker
    # started by a worker still writes to the conversation's folder rather than its
    # parent's.
    from minime_local.workspace import WORKSPACE_THREAD_KEY

    pinned, source = _conversation_thread(config, configurable)
    if pinned:
        forwarded[WORKSPACE_THREAD_KEY] = pinned
    # Said either way, because the two outcomes were indistinguishable and the failing one is
    # silent by construction: an unpinned worker writes to a directory that exists, fills it
    # correctly, and reports paths under the conversation instead. The researcher is told their
    # plots were saved, opens the folder, and finds nothing (docs §150).
    logger.warning(
        "minime_local: background work pinned to %s (from %s)",
        pinned or "<the worker's own thread — its files will not join the conversation>",
        source,
    )
    if "model_config" not in forwarded:
        # Worth saying out loud: the run will still start, and will still fail later on a
        # model it could not build. This line is the difference between a diagnosable
        # failure and "The async subagent encountered an error".
        logger.warning(
            "minime_local: starting background work with no model config — "
            "the worker will fall back to the server default and may not have a key"
        )
    _report_what_the_worker_will_bill(forwarded)
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

        # Read here, at the launch, not when the tool was built — a graph is constructed per
        # request, including read-only ones with no model in them.
        forwarded = _forwarded_config()
        # Mark the run as a background worker, so the graph it builds knows what it is without
        # depending on a ContextVar surviving the trip (docs §114).
        forwarded.setdefault("configurable", {})[BACKGROUND_RUN_KEY] = True
        configurable = forwarded.get("configurable") or {}
        # **Named on the way out.** A background run that starts without a model reports
        # `success` with an empty result, which is indistinguishable from one that ran and found
        # nothing — and that ambiguity is exactly what cost §81 four rounds. Keys only; a value
        # here would be an API key in a log file.
        logger.warning(
            "minime_local: launching %s with config keys %s, recursion_limit=%s",
            subagent_type,
            sorted(configurable) or "NONE — the worker will have no model",
            forwarded.get("recursion_limit"),
        )

        try:
            client = get_client(url=spec.get("url"))
            thread = await client.threads.create()
            run = await client.runs.create(
                thread_id=thread["thread_id"],
                assistant_id=spec["graph_id"],
                input={"messages": [{"role": "user", "content": description}]},
                config=forwarded,
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
    # `info`, and worded as what it is. This runs on **every graph build** — including the
    # read-only ones behind `GET /threads/{id}/state`, which the client polls while watching a
    # task — so at warning level it filled the log with a sentence that reads like an event and
    # was only ever a wiring step. The line that matters is in the tool itself, where a launch
    # actually happens (docs §112).
    logger.info("minime_local: start_async_task will forward the conversation's config")
    return middleware

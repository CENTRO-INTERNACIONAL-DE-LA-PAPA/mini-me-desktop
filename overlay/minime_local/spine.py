"""Give each project its own research spine.

**The problem.** `backend/runtime.py:141-154` namespaces the spine as `(user_id, "project")` —
one per person, deliberately not keyed by assistant or thread, because *"the research project is
the user's, spanning every assistant/thread."* That was right when a researcher had one line of
work. With projects (docs §105) it is wrong twice: it mixes them, and it never forgets — a panel
still listing what a deleted conversation achieved, under a mission from a different study.

**Why it takes a patch rather than a setting.** The namespace is computed in *two* places that
have to agree:

* `_project_namespace_for_runtime` (`runtime.py:157`), inside a turn, which can see the project
  through `get_config()`;
* `get_project` / `patch_project` (`routes/project.py:76,100`), in an HTTP handler, which cannot —
  and `runtime.py`'s own docstring says that symmetry is *why* the namespace is keyed the way it
  is.

Patch only the first and `GET /project` reads a namespace the turns no longer write to: the panel
goes blank rather than becoming correct. So both are patched here, and the route learns the
project from a query parameter the client already knows.

**One spine per conversation, for conversations in no project.** That last sentence used to
read *"with no project the namespace is unchanged"* — one record shared by every conversation the
researcher had never filed. So a brand-new conversation about late blight opened showing a mission
of *"Testting functionalities"*, six visualizations it had not produced, and the plan from the
conversation before it.

Not only on the screen. `render_mission_context` puts that spine into the **coordinator's system
prompt** every turn, under *"Ground every answer, plan, and delegation in this mission"* — so an
unrelated conversation began by being told what it had already achieved. A panel that overstates is
a nuisance; a prompt that overstates changes what the agent does.

An ungrouped conversation now gets `(user_id, "project", "<default>", "solo-<thread>")`, which is
its own and nobody else's. A project still appends its name. Only a call that names neither — no
project, no conversation — reads the old shared record, which is every non-desktop client and
nothing this app does.

**Where this belongs in the end.** Upstream, as a `project` parameter on the route and a namespace
that takes one — see `docs/upstream/mini-me/`. This is the bridge, in the same sense §18 meant it:
the checkout stays byte-for-byte upstream and a `git pull` there can never conflict with us.
"""

from __future__ import annotations

import contextvars
import functools
import logging

logger = logging.getLogger(__name__)

#: The project the current HTTP request is about.
#:
#: A `ContextVar` rather than an argument because the function that needs it —
#: `_project_namespace` — is called several frames below the handler, from code this overlay does
#: not touch. Per-context, so two concurrent requests cannot read each other's.
_http_project: contextvars.ContextVar[str] = contextvars.ContextVar(
    "minime_local_http_project", default=""
)

#: The conversation the current HTTP request is about, when it is in no project.
#:
#: Separate from the project rather than folded into it: a route that received both would have to
#: decide which wins, and the answer differs by caller. Here the rule is stated once, in
#: :func:`current_scope`, and both variables are plain inputs to it.
_http_thread: contextvars.ContextVar[str] = contextvars.ContextVar(
    "minime_local_http_thread", default=""
)

#: The query parameters the desktop app sends on `/project`.
QUERY_PARAM = "project"
THREAD_PARAM = "thread"

#: The `configurable` key LangGraph puts the conversation id under, inside a run.
_THREAD_KEY = "thread_id"


def sanitise(name: str) -> str:
    """One namespace segment for a project name, matching the folder it is stored beside.

    The **same** rule as `workspace.workspace_project`, deliberately: a project whose spine and
    whose folder disagreed about punctuation would be two projects wearing one name.
    """
    name = (name or "").strip()
    if not name:
        return ""
    cleaned = "".join(
        character if (character.isalnum() or character in " -_") else "_" for character in name
    ).strip(" ._")
    return cleaned[:96]


def solo_scope(thread_id: str) -> str:
    """The spine segment for a conversation that is in no project, or `""` for no conversation.

    Prefixed so the segment cannot be mistaken for a project name in the store, and so a person
    reading the keys can see at a glance which records belong to a single conversation.
    """
    cleaned = sanitise(thread_id)
    return f"solo-{cleaned}" if cleaned else ""


def current_scope() -> str:
    """Whose spine this call is about: a project's, one conversation's, or nobody's.

    **A project always wins over a conversation.** Filing a conversation into a project is a
    statement that its work belongs with the rest of that project's, and the panel has said so
    since §109: *"moving between them changes which one the panel shows"*.

    HTTP first, then the run config, because only one of the two is ever populated: a route
    handler sets the ContextVars and has no run config; a turn has a run config and never touches
    them. A route that named neither falls through to an empty segment, which is the old shared
    record — left reachable deliberately, for a client that does not know about either parameter.
    """
    from_request = _http_project.get()
    if from_request:
        return sanitise(from_request)
    from_thread = _http_thread.get()
    if from_thread:
        return solo_scope(from_thread)
    try:
        from langgraph.config import get_config

        from minime_local.workspace import WORKSPACE_PROJECT_KEY

        configurable = (get_config() or {}).get("configurable") or {}
        project = sanitise(str(configurable.get(WORKSPACE_PROJECT_KEY) or ""))
        if project:
            return project
        # The turn's own conversation. Without this an ungrouped run writes its mission and its
        # plan into the record every other ungrouped conversation reads, which is the defect.
        return solo_scope(str(configurable.get(_THREAD_KEY) or ""))
    except Exception:  # noqa: BLE001 — outside a run and outside a request
        return ""


def install_runtime(module) -> None:
    """Make `backend.runtime._project_namespace` project-aware.

    One function, two callers: `routes/project.py` and `_project_namespace_for_runtime` both
    import it, so patching here covers the HTTP side and the turn side together — which is the
    only way they can be kept in step.
    """
    original = getattr(module, "_project_namespace", None)
    if original is None:
        logger.warning("minime_local: no _project_namespace to make project-aware")
        return

    # **`*args, **kwargs`, and never a restated signature.** The first version declared
    # `(user_id: str)`, matching the reference checkout on the developer's machine — and the
    # pinned checkout a researcher actually runs calls it with two, so every request died with
    # *"takes 1 positional argument but 2 were given"* and the backend could not start (docs
    # §113). A wrapper over someone else's function has no business knowing how it is called; it
    # only needs to pass along whatever it was given and adjust what comes back.
    @functools.wraps(original)
    def _project_namespace_scoped(*args, **kwargs):
        base = original(*args, **kwargs)
        scope = current_scope()
        return (*base, scope) if scope else base

    module._project_namespace = _project_namespace_scoped
    logger.warning("minime_local: the research spine is now per project, or per conversation")


def install_routes(module) -> None:
    """Teach `/project` which project it is being asked about.

    The handlers are bound into the route table by `backend/routes/__init__.py` at *its* import,
    reading these attributes — so replacing them here, when the module finishes loading, is what
    puts the wrapper in the table. That file is loaded by path and cannot be hooked itself
    (docs §18); its imports of submodules can.
    """
    wrapped = 0
    for name in ("get_project", "patch_project"):
        handler = getattr(module, name, None)
        if handler is None:
            continue

        # **`async def`, and the `await` inside the block.** These handlers are coroutine
        # functions: a sync wrapper would set the variable, build the coroutine, reset, and
        # return it unawaited — so the value would be gone by the time the handler actually ran,
        # and every request would read the ungrouped spine while looking like it worked.
        @functools.wraps(handler)
        async def scoped(request, *args, _handler=handler, **kwargs):
            project = _http_project.set(request.query_params.get(QUERY_PARAM, "") or "")
            thread = _http_thread.set(request.query_params.get(THREAD_PARAM, "") or "")
            try:
                return await _handler(request, *args, **kwargs)
            finally:
                _http_project.reset(project)
                _http_thread.reset(thread)

        setattr(module, name, scoped)
        wrapped += 1
    logger.warning(
        "minime_local: /project reads its scope from ?%s / ?%s (%d)",
        QUERY_PARAM,
        THREAD_PARAM,
        wrapped,
    )

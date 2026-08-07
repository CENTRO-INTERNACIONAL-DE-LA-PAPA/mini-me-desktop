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

**Backwards compatible by construction.** With no project the namespace is unchanged,
`(user_id, "project")`, so every spine that exists today is what an ungrouped conversation reads.
A project appends one segment: `(user_id, "project", "<name>")`.

**Where this belongs in the end.** Upstream, as a `project` parameter on the route and a namespace
that takes one — see `docs/upstream/mini-me/`. This is the bridge, in the same sense §18 meant it:
the checkout stays byte-for-byte upstream and a `git pull` there can never conflict with us.
"""

from __future__ import annotations

import contextvars
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

#: The query parameter the desktop app sends on `/project`.
QUERY_PARAM = "project"


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


def current_project() -> str:
    """The project this call is about: an HTTP request's, or the running turn's.

    Checked in that order because only one of them is ever set. A route handler puts the value in
    the ContextVar and has no run config; a turn has a run config and never touches the ContextVar.
    """
    from_request = _http_project.get()
    if from_request:
        return sanitise(from_request)
    try:
        from langgraph.config import get_config

        from minime_local.workspace import WORKSPACE_PROJECT_KEY

        configurable = (get_config() or {}).get("configurable") or {}
        return sanitise(str(configurable.get(WORKSPACE_PROJECT_KEY) or ""))
    except Exception:  # noqa: BLE001 — outside a run, which is the ungrouped case
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

    def _project_namespace_scoped(user_id: str):
        base = original(user_id)
        project = current_project()
        return (*base, project) if project else base

    module._project_namespace = _project_namespace_scoped
    logger.warning("minime_local: the research spine is now per project")


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
        async def scoped(request, _handler=handler):
            token = _http_project.set(request.query_params.get(QUERY_PARAM, "") or "")
            try:
                return await _handler(request)
            finally:
                _http_project.reset(token)

        setattr(module, name, scoped)
        wrapped += 1
    logger.warning("minime_local: /project reads its project from ?%s (%d)", QUERY_PARAM, wrapped)

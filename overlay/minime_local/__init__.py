"""Desktop-only overlay for the Mini-Me backend.

Injected on ``PYTHONPATH``; **nothing in the Mini-Me checkout is modified**. See
``overlay/README.md`` and the desktop plan §18.
"""

from __future__ import annotations

import importlib.abc
import logging
import os
import sys
from pathlib import Path

LOCAL_EXECUTION_ENV = "MINIME_EXECUTION_BACKEND"

#: The module whose sandbox class we replace, and the name we replace.
_SANDBOX_MODULE = "backend.sandbox"
_TARGET_NAME = "LazyLangsmithSandbox"

#: Where the agent factory comes *from*. We wrap it here rather than on
#: `backend.agent`, because LangGraph loads the graph module by file path
#: (`langgraph.json` → `./backend/agent.py:agent`) via `spec_from_file_location`, which
#: bypasses `sys.meta_path` entirely — so a hook on `backend.agent` never fires in the
#: real server. Measured: the sandbox patch landed and the approval patch silently did
#: not. `backend/agent.py` does `from deepagents import create_deep_agent` at its own
#: import time, and *that* goes through normal machinery, so patching the package
#: attribute first is what actually takes effect.
_AGENT_MODULE = "deepagents"

#: Where the research spine's namespace is computed, and where it is asked for over HTTP.
_RUNTIME_MODULE = "backend.runtime"
_PROJECT_ROUTE_MODULE = "backend.routes.project"
#: Where an Asta paper search hands its results to the model, and where a subagent's
#: structured output becomes the `sources` list the desktop app reads. Both are
#: ordinary imports, so the hook reaches them; `http.app` is the one that cannot be
#: (docs §18). See `minime_local/sources.py`.
_SUBAGENTS_MODULE = "backend.subagents"
_MCP_MODULE = "backend.mcp_tools"
_ARTIFACTS_MODULE = "backend.middleware.artifacts"
#: The other tool that finds papers. Watched for the same reason as `_MCP_MODULE` and separately
#: from it, because `find_papers` is not part of the MCP bundle and never passes through the
#: function that wraps it.
_PAPER_MODULE = "backend.paper_tools"

#: Patched only when host execution is on — they *are* host execution.
_LOCAL_TARGETS = (_SANDBOX_MODULE, _AGENT_MODULE)

#: Patched always. Scoping the research spine to a project has nothing to do with where the
#: agent's code runs, and tying it to that switch is exactly the mistake §78 made with the
#: subagent registry: a feature that silently did nothing because it inherited an unrelated
#: setting's default.
_ALWAYS_TARGETS = (
    _RUNTIME_MODULE,
    _PROJECT_ROUTE_MODULE,
    _MCP_MODULE,
    _PAPER_MODULE,
    _ARTIFACTS_MODULE,
    _SUBAGENTS_MODULE,
)

#: Every module we patch, and what patching it means.
_TARGETS = _LOCAL_TARGETS + _ALWAYS_TARGETS

log = logging.getLogger("minime_local")


def local_execution_requested() -> bool:
    """True when the app asked for host execution.

    Opt-in, and the sandbox stays the default: the overlay must be inert unless the
    desktop app deliberately switches it on, so a plain ``langgraph dev`` in the
    checkout behaves exactly as it always has.
    """
    return os.getenv(LOCAL_EXECUTION_ENV, "").strip().lower() == "local"


def _patch(module) -> None:
    """Apply whichever patch this module needs."""
    if module.__name__ == _RUNTIME_MODULE:
        from minime_local import spine

        spine.install_runtime(module)
        return
    if module.__name__ == _PROJECT_ROUTE_MODULE:
        from minime_local import spine

        spine.install_routes(module)
        return
    if module.__name__ == _SUBAGENTS_MODULE:
        from minime_local import sources

        sources.install_prompt(module)
        return
    if module.__name__ == _MCP_MODULE:
        from minime_local import sources

        sources.install_mcp(module)
        return
    if module.__name__ == _PAPER_MODULE:
        from minime_local import sources

        sources.install_papers(module)
        return
    if module.__name__ == _ARTIFACTS_MODULE:
        from minime_local import sources

        sources.install_artifacts(module)
        return
    if not local_execution_requested():
        return
    if module.__name__ == _AGENT_MODULE:
        from minime_local import approval, async_agents, execute_rule, registry

        # First, and reached through the package rather than watched for its own import:
        # `deepagents/__init__.py` has already pulled the middleware in by the time this hook
        # fires, so a second meta-path target would never trigger. What matters is only that it
        # runs before any `FilesystemMiddleware` is constructed — the description is read when
        # the tool is built (`filesystem.py:1481`), and `backend/agent.py` builds its agent
        # after this import completes.
        _rewrite_execute_description(execute_rule)
        approval.install(module)
        # After approval, so the background worker inherits the same gate: its wrapper
        # calls whatever `create_deep_agent` is current, which is the gated one.
        async_agents.install(module)
        # Last, so it is outermost and sees the arguments as `backend/agent.py` passed them.
        # Unconditional: it only *reads* the subagent list, and folding it into the wrapper
        # above meant it inherited that one's off-by-default switch (docs §78).
        registry.install(module)
        return
    _patch_sandbox(module)


def _rewrite_execute_description(execute_rule) -> None:
    """Tell `execute` to keep its output in the workspace, and never take the agent down for it.

    Only under host execution, because the escape it prevents is one: with the remote sandbox an
    absolute path is inside a container nobody else can see, and `/tmp` there is nobody's problem.

    Wrapped in a `try` because an import failure here would cost the whole agent, and the failure
    mode without this patch is *files in the wrong place* — bad, recoverable, and already
    documented — while the failure mode with a raising hook is no backend at all (§18's rule about
    what an overlay may risk).
    """
    try:
        from deepagents.middleware import filesystem

        execute_rule.install(filesystem)
    except Exception as exc:  # noqa: BLE001
        log.warning(
            "minime_local: could not rewrite the execute description (%s) — commands may write "
            "outside the conversation's folder (docs §160)",
            exc,
        )


def _patch_sandbox(module) -> None:
    """Rebind ``backend.sandbox.LazyLangsmithSandbox`` to the local backend.

    Both construction sites — ``backend/agent.py`` and ``backend/routes/common.py`` —
    do ``from backend.sandbox import LazyLangsmithSandbox`` at *their* module import
    time, so rebinding the name on ``backend.sandbox`` the moment it finishes loading
    covers both with one change and no edits to either file.
    """
    # Fail loudly rather than silently keeping the sandbox: if upstream renames this
    # class, the honest outcome is a crash naming the overlay, not a desktop app that
    # quietly starts billing a LangSmith account again.
    if not hasattr(module, _TARGET_NAME):
        raise RuntimeError(
            f"minime_local: {_SANDBOX_MODULE} has no {_TARGET_NAME} to replace — the "
            "pinned Mini-Me commit has moved and overlay/minime_local needs updating "
            "(desktop plan §18)."
        )

    from minime_local.workspace import LocalWorkspaceBackend, workspace_root

    setattr(module, _TARGET_NAME, LocalWorkspaceBackend)
    log.warning(
        "minime_local: host execution is ON — the agent runs commands on this "
        "machine, workspaces under %s",
        workspace_root(),
    )


class _PatchingLoader(importlib.abc.Loader):
    """Delegates to the real loader, then patches the module it just executed."""

    def __init__(self, inner: importlib.abc.Loader):
        self._inner = inner

    def create_module(self, spec):
        return self._inner.create_module(spec)

    def exec_module(self, module) -> None:
        self._inner.exec_module(module)
        _patch(module)


class _PatchOnImport(importlib.abc.MetaPathFinder):
    """Waits for a target module to be imported, whenever that happens.

    A startup-time ``import backend.sandbox`` would be simpler but unreliable: for a
    console script ``sys.path[0]`` is ``.venv/bin``, not the project, and it is
    LangGraph that puts the checkout on the path later while resolving the graph from
    ``langgraph.json``. Hooking the import removes that ordering guesswork entirely.
    """

    def find_spec(self, fullname, path=None, target=None):
        if fullname not in _TARGETS:
            return None
        # Ask every *other* finder for the real spec, then wrap its loader. Returning
        # None for anything else keeps this finder invisible to normal imports.
        for finder in [f for f in sys.meta_path if f is not self]:
            find = getattr(finder, "find_spec", None)
            if find is None:
                continue
            spec = find(fullname, path, target)
            if spec is not None and spec.loader is not None:
                spec.loader = _PatchingLoader(spec.loader)
                return spec
        return None


def _checkout_version(start: "os.PathLike[str] | str | None" = None) -> str:
    """The commit this backend is running, read from the checkout's own git files.

    **Because every diagnosis this week was made without knowing what code was running.** The app
    syncs the checkout to a pin before spawning, and when that fails — a private remote WSL has no
    credentials for, most recently — it says so in the *app's* log, while the backend log the
    researcher actually reads carries no version at all. So a fix that was merged, pulled and never
    delivered produces a log identical to one that was delivered and did not work, and the second
    reading is the one that costs a night.

    No subprocess: this runs during interpreter start-up on a path where a stalled `git` would
    delay the window, and the two files involved are plain text.
    """
    root = Path(start or Path.cwd())
    for base in (root, *root.parents):
        marker = base / ".git"
        if marker.is_file():  # a worktree or submodule: `gitdir: <path>`
            pointer = marker.read_text(errors="replace").partition("gitdir:")[2].strip()
            marker = Path(pointer) if pointer else marker
        if not marker.is_dir():
            continue
        try:
            head = (marker / "HEAD").read_text(errors="replace").strip()
        except OSError:
            return "unknown"
        if not head.startswith("ref:"):
            return f"{head[:7]} (detached)"
        ref = head.partition("ref:")[2].strip()
        # A linked worktree keeps its own HEAD but shares every ref with the repository it was
        # made from, named by `commondir`. Without this the branch resolves nowhere and the stamp
        # reads "unresolved" — a diagnostic that cannot read its own repository is worse than
        # none, because it invites exactly the shrug this whole line exists to prevent.
        common = marker / "commondir"
        if common.is_file():
            marker = (marker / common.read_text(errors="replace").strip()).resolve()
        loose = marker / ref
        if loose.is_file():
            return f"{loose.read_text(errors='replace').strip()[:7]} ({ref.split('/')[-1]})"
        # Packed refs — what a fresh clone that has never been updated looks like.
        packed = marker / "packed-refs"
        if packed.is_file():
            for line in packed.read_text(errors="replace").splitlines():
                sha, _, name = line.partition(" ")
                if name.strip() == ref:
                    return f"{sha[:7]} ({ref.split('/')[-1]})"
        return f"unresolved {ref}"
    return "not a git checkout"


def install() -> bool:
    """Arm the overlay. Returns True when host execution is enabled.

    Safe to call more than once.
    """
    local = local_execution_requested()
    # First line of the backend log, on purpose: it is the one fact every other line is read
    # against, and it has never been there.
    log.warning("minime_local: backend checkout %s", _checkout_version())
    # The spine patches are armed either way; `_patch` declines the host-execution ones itself
    # when they are not wanted, so there is one list of targets and one place that decides.
    targets = _TARGETS if local else _ALWAYS_TARGETS

    for name in targets:
        already = sys.modules.get(name)
        if already is not None:
            _patch(already)

    if not any(isinstance(finder, _PatchOnImport) for finder in sys.meta_path):
        sys.meta_path.insert(0, _PatchOnImport())
    return local

"""Desktop-only overlay for the Mini-Me backend.

Injected on ``PYTHONPATH``; **nothing in the Mini-Me checkout is modified**. See
``overlay/README.md`` and the desktop plan §18.
"""

from __future__ import annotations

import importlib.abc
import logging
import os
import sys

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

#: Every module we patch, and what patching it means.
_TARGETS = (_SANDBOX_MODULE, _AGENT_MODULE)

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
    if module.__name__ == _AGENT_MODULE:
        from minime_local import approval

        approval.install(module)
        return
    _patch_sandbox(module)


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


def install() -> bool:
    """Arm the overlay. Returns True when host execution is enabled.

    Safe to call more than once.
    """
    if not local_execution_requested():
        return False

    for name in _TARGETS:
        already = sys.modules.get(name)
        if already is not None:
            _patch(already)

    if not any(isinstance(finder, _PatchOnImport) for finder in sys.meta_path):
        sys.meta_path.insert(0, _PatchOnImport())
    return True

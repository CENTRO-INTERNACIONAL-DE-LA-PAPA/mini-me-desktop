"""Desktop-only local-execution behavior for the Mini-Me backend.

This package used to be injected onto a separate checkout via ``PYTHONPATH`` (see the old
``overlay/`` directory), back when the desktop app consumed Mini-Me as an external GitHub
checkout it did not want to modify. The backend is vendored directly into this repo now, so
that indirection is gone: this is an ordinary subpackage of ``backend``, and everything it does
is called explicitly rather than injected by import-hook magic.

Two kinds of behavior live here:

* Code targeting **this backend** (the research-spine scoping, the citation/source handling) is
  folded directly into the ``backend`` modules it belongs to (``backend/runtime.py``,
  ``backend/routes/project.py``, ...) rather than kept here — see those files.
* Code targeting **third-party packages this backend depends on but does not vendor**
  (``deepagents``, ``langgraph_runtime_inmem``) still has to patch those packages at runtime,
  because there is no source tree here to edit directly. That patch logic lives in this
  package's modules, and :func:`install` is the single, explicit entry point that applies it —
  called once from the top of ``backend/agent.py``, before its own
  ``from deepagents import create_deep_agent`` import, so every wrapper installed here is what
  that import actually binds.
"""

from __future__ import annotations

import logging
from pathlib import Path

logger = logging.getLogger("backend.local")

_installed = False


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


def install() -> None:
    """Apply every local-execution patch to the third-party packages this backend depends on.

    Safe to call more than once — a second call is a no-op. Order matters for the
    ``deepagents.create_deep_agent`` wrappers: each one wraps whatever is current when it
    installs, so the last one to install is outermost and sees the fully assembled arguments.
    """
    global _installed
    if _installed:
        return
    _installed = True

    # First line of the backend log, on purpose: it is the one fact every other line is read
    # against, and it has never been there.
    logger.warning("backend.local: backend checkout %s", _checkout_version())

    _install_index_guard()

    import deepagents

    from backend.local import approval, async_agents, authorship, execute_rule, registry

    # Module-level rewrites first, and reached through the `deepagents` package rather than by
    # importing the submodule directly: `deepagents/__init__.py` has already pulled these
    # middleware modules in by the time this runs, so importing them here just returns the
    # already-loaded module — the same one `SubAgentMiddleware`/`FilesystemMiddleware` will use.
    # What matters is only that this runs before either middleware is constructed, which
    # `backend/agent.py` does after this module's import completes.
    try:
        from deepagents.middleware import filesystem as _filesystem_middleware

        execute_rule.install(_filesystem_middleware)
    except Exception as exc:  # noqa: BLE001
        logger.warning(
            "backend.local: could not rewrite the execute description (%s) — commands may write "
            "outside the conversation's folder",
            exc,
        )

    try:
        from deepagents.middleware import subagents as _subagents_middleware

        authorship.install(_subagents_middleware)
    except Exception as exc:  # noqa: BLE001
        logger.warning(
            "backend.local: could not name the writer (%s) — files a specialist wrote will be "
            "attributed to the coordinator",
            exc,
        )

    # Then the `create_deep_agent` wrappers, each wrapping whatever the previous one left behind.
    approval.install(deepagents)
    # After approval, so the background worker inherits the same gate: its wrapper calls
    # whatever `create_deep_agent` is current, which is the gated one.
    async_agents.install(deepagents)
    # Last, so it is outermost and sees the arguments as `backend/agent.py` passed them.
    registry.install(deepagents)


def _install_index_guard() -> None:
    """Patch `langgraph_runtime_inmem.database.start_pool`, independent of everything above.

    Has nothing to do with where the agent's code runs — tying it to that switch would be the
    same mistake as making the subagent registry conditional on it. Imported directly rather than
    hooked, since `install()` runs it explicitly rather than waiting for an import to happen.
    """
    from backend.local import index_guard

    try:
        import langgraph_runtime_inmem.database as _database
    except Exception as exc:  # noqa: BLE001 — an optional/renamed dependency must not block startup
        logger.warning(
            "backend.local: could not import langgraph_runtime_inmem.database (%s) — an "
            "unreadable conversation index would be deleted with no copy kept",
            exc,
        )
        return
    index_guard.install(_database)

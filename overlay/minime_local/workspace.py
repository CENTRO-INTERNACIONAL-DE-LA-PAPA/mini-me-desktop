"""Host execution for Mini-Me: a local workspace in place of the LangSmith sandbox.

Mini-Me runs the agent's files and shell commands inside a remote LangSmith sandbox.
For a **local-first desktop app** that is infrastructure we neither need nor want: it
costs a per-user API key, a cold start, a 10-minute idle TTL, a one-concurrent-sandbox
free tier, and it ships the user's files to someone else's VM. See the desktop plan,
§10/§11 — the sandbox is the *only* thing standing between us and dropping LangSmith.

The replacement is deliberately thin, because deepagents already does the work:

  ``LocalShellBackend(FilesystemBackend, SandboxBackendProtocol)`` implements the
  whole *sync* backend surface against the host, and every ``a*`` method in
  ``BackendProtocol`` is a concrete default that offloads its sync twin with
  ``asyncio.to_thread``. So a subclass inherits everything Mini-Me's tools await —
  ``aread``, ``awrite``, ``aedit``, ``als``, ``aglob``, ``agrep``, ``aupload_files``,
  ``adownload_files`` — for free.

What is left is the handful of methods Mini-Me added on top of the protocol, which
deepagents has no equivalent for. That is this file.
"""

from __future__ import annotations

import asyncio
import logging
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from deepagents.backends.local_shell import LocalShellBackend
from deepagents.backends.protocol import ExecuteResponse

# Imported from upstream rather than reimplemented, so the local path truncates
# execute output exactly as the sandbox path does — the cap protects the model's
# context window and is not sandbox-specific.
from backend.sandbox import _emit_sandbox_status, _truncate_execute_response

#: Where per-thread workspaces live. The desktop app sets this; the fallback keeps
#: a bare ``langgraph dev`` working.
WORKSPACE_ROOT_ENV = "MINIME_LOCAL_WORKSPACE"

#: High enough to be no cap at all: truncation is applied deliberately in
#: :meth:`LocalWorkspaceBackend.aexecute` using upstream's rule, and the
#: untruncated path exists precisely so a ~500 KB theorizer record survives whole.
_NO_PRACTICAL_CAP = 64 * 1024 * 1024

#: Matches the sandbox's per-command default (``_aexecute_core`` used 300s).
_DEFAULT_TIMEOUT = 300


#: Config key naming the thread whose workspace a run should share.
#:
#: A background worker runs on its **own** LangGraph thread, so by default it would get its
#: own workspace directory — and everything it produced would be invisible: the app looks in
#: the conversation's directory, and the coordinator, asked for the report afterwards, could
#: only hunt for it with `ls` and `glob` (docs §43). This pins the worker to the
#: conversation's workspace so a file it writes is simply *there*, at the same relative
#: path, for everyone who goes looking.
WORKSPACE_THREAD_KEY = "__workspace_thread__"


def workspace_root() -> Path:
    """The directory that holds one subdirectory per thread."""
    configured = os.getenv(WORKSPACE_ROOT_ENV)
    if configured:
        return Path(configured).expanduser()
    return Path.home() / ".mini-me" / "workspaces"


def workspace_thread(default: str) -> str:
    """Which thread's workspace this run should use.

    ``default`` (the run's own thread) unless something pinned it — see
    :data:`WORKSPACE_THREAD_KEY`. Read from the live run config rather than passed in,
    because upstream constructs the backend as ``LazyLangsmithSandbox(thread_id)`` at two
    call sites this overlay deliberately does not touch.
    """
    try:
        from langgraph.config import get_config

        configurable = (get_config() or {}).get("configurable") or {}
    except Exception:  # noqa: BLE001  # no runnable context: a read-only graph load
        return default
    pinned = configurable.get(WORKSPACE_THREAD_KEY)
    pinned = str(pinned).strip() if pinned else ""
    return pinned or default


logger = logging.getLogger(__name__)

#: How long a minted Asta token is reused before asking the CLI again.
#:
#: Access tokens last seven days, so this is not about expiry — it is about not spawning
#: `asta` once per shell command. Ten minutes means a token that lapses mid-session is
#: replaced within ten minutes, without the researcher doing anything.
_TOKEN_TTL_SECONDS = 600

#: (token, minted_at). Module-level so every workspace in the process shares one.
_token_cache: tuple[str | None, float] = (None, 0.0)


def _looks_like_a_jwt(value: str) -> bool:
    """Three non-empty base64url segments, and nothing else.

    Without ``--raw`` the CLI pretty-prints a decoded header and payload; signed out it
    prints prose. Passing either along as a credential produces an authentication failure
    that blames the wrong thing.
    """
    parts = value.split(".")
    return len(parts) == 3 and all(
        part and all(c.isalnum() or c in "-_" for c in part) for part in parts
    )


def current_asta_token() -> str | None:
    """A usable Asta access token, minted from the CLI if need be.

    **Why the backend mints it rather than receiving it.** Access tokens last seven days,
    and an expired one surfaces as "the theorizer returned no task id" — naming neither
    the token nor the fix. Passing one in as an environment variable turned out to have
    three separate holes: the value is captured once when a workspace is built, the app
    only mints while *spawning* (so it never does when it attaches to a backend that is
    already running), and on Windows it has to survive the crossing into WSL.

    Asking the CLI here removes all three. It runs in the same environment as every other
    `asta` command the agent makes, so if those can authenticate, so can this.

    **Never called on the event loop** — see the call site in ``aexecute``, which is
    already inside ``asyncio.to_thread``. ``langgraph dev``'s blocking-call guard rejects
    subprocesses on the loop, and that guard has aborted a run in this project before.
    """
    global _token_cache

    token, minted_at = _token_cache
    if token and (time.monotonic() - minted_at) < _TOKEN_TTL_SECONDS:
        return token

    try:
        result = subprocess.run(
            ["asta", "auth", "print-token", "--raw", "--refresh"],
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        logger.debug("minime_local: could not run the asta CLI: %s", exc)
        return token
    minted = result.stdout.strip()
    if result.returncode != 0 or not _looks_like_a_jwt(minted):
        logger.debug("minime_local: asta did not return a token")
        return token or _supplied_token()

    _token_cache = (minted, time.monotonic())
    logger.info("minime_local: refreshed the Asta access token from the CLI")
    return minted


def _supplied_token() -> str | None:
    """A token from the environment — the **fallback**, not the preference.

    This ordering was the other way round, and it silently broke everything.
    ``ASTA_TOKEN`` reaches the backend from the OS keychain, where a token pasted days
    earlier is still sitting; the `asta` CLI **prefers that variable over its own stored
    credentials**, and when it is stale the CLI exits **0 with empty output** — no error,
    no message. Upstream then reports "no task id was returned, which usually means the
    access token is missing or expired", which is right about the cause and useless about
    the source. A silent exit-0 failure also slips straight past failure logging.

    The CLI is the authority: `asta auth login` leaves a refresh credential and the CLI
    renews itself, so a *minted* token is always at least as good as a stored one. A
    supplied value is worth trying only when nothing can be minted at all.
    """
    supplied = (os.getenv("ASTA_TOKEN") or "").strip()
    if supplied and _looks_like_a_jwt(supplied):
        logger.warning(
            "minime_local: using ASTA_TOKEN from the environment — the asta CLI could "
            "not mint one. If Asta calls fail, this stored token is likely stale; "
            "clear it in Settings and sign in instead."
        )
        return supplied
    return None


def _log_failure(command: str, result: Any) -> None:
    """Put a failed command and its output in the sidecar log.

    Tools discard what a command actually printed and report their own summary instead —
    the theorizer's *"no task id was returned, which usually means the access token is
    missing or expired"* is a **guess**, offered with no way to see the real error. That
    guess has now sent this project down three wrong paths.

    Whatever the command truly said lands here, in the file the Setup pane already points
    at. Only failures, so a working session stays quiet.
    """
    exit_code = None
    output = ""
    if isinstance(result, dict):
        exit_code, output = result.get("exit_code"), result.get("output") or ""
    else:
        exit_code = getattr(result, "exit_code", None)
        output = getattr(result, "output", "") or ""
    if exit_code in (None, 0):
        return
    logger.warning(
        "minime_local: command failed (exit %s): %s\n%s",
        exit_code,
        command[:400],
        output[-2000:],
    )


def _command_env() -> dict[str, str]:
    """Environment for executed commands.

    Two things have to be true for the agent's code to run:

    1. **The venv's interpreter must be what ``python3`` means.** The prompts tell the
       model to use ``python3``, and the numerical stack (pandas, PyMC, sklearn) lives
       in the backend venv. The app launches ``.venv/bin/langgraph`` directly rather
       than activating the venv, so ``PATH`` would otherwise resolve ``python3`` to a
       bare system interpreter. ``sys.executable`` *is* the venv interpreter, so its
       directory is what we prepend — which also retires the sandbox snapshot's
       duplicate dependency manifest.
    2. **``ASTA_TOKEN`` must be present**, or the ``asta`` CLI (theory generation,
       DataVoyager, PDF extraction) cannot authenticate. Read at call time, not import
       time: ``.env`` is loaded after this module is imported.
    """
    env = dict(os.environ)
    interpreter_dir = str(Path(sys.executable).parent)
    # `~/.local/bin` is where `uv tool install` puts the **asta CLI**, and it is normally
    # added by `~/.profile` — which `execute` never reads: commands run through `/bin/sh`
    # with exactly the environment given here, not through a login shell. If the backend's
    # own PATH happens to lack it, every `asta` command dies with `sh: asta: not found`,
    # exit 127 — and the theorizer reports that as "no task id was returned, which usually
    # means the access token is missing or expired". Which is how a PATH problem spent days
    # masquerading as an authentication one.
    local_bin = str(Path.home() / ".local" / "bin")
    env["PATH"] = os.pathsep.join(
        [interpreter_dir, local_bin, env.get("PATH", "")]
    ).rstrip(os.pathsep)
    asta_token = os.getenv("ASTA_TOKEN")
    if asta_token:
        env["ASTA_TOKEN"] = asta_token

    # Keep the overlay out of the child's PYTHONPATH. Otherwise every command the
    # model runs re-imports our `sitecustomize`, whose startup line lands in the
    # command's stderr — and `execute` merges stderr into the output the model reads,
    # so every result would arrive wearing a banner about the overlay.
    # String comparison, not `Path.resolve()`: this runs on the event loop, where
    # `langgraph dev`'s blocking-call guard rejects filesystem syscalls.
    overlay_dir = os.path.normpath(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    remaining = [
        entry
        for entry in env.get("PYTHONPATH", "").split(os.pathsep)
        if entry and os.path.normpath(entry) != overlay_dir
    ]
    if remaining:
        env["PYTHONPATH"] = os.pathsep.join(remaining)
    else:
        env.pop("PYTHONPATH", None)
    return env


class LocalWorkspaceBackend(LocalShellBackend):
    """A per-thread directory on the host, standing in for a sandbox VM.

    Constructed with the same one-argument signature as
    ``backend.sandbox.LazyLangsmithSandbox`` so it can be dropped in at both of
    upstream's construction sites without touching them.

    **``virtual_mode=False`` is load-bearing.** Upstream's tools build absolute paths
    from ``aget_work_dir()`` — ``f"{work_dir}/theories/{task_id}"`` — hand them to the
    file operations, *and* print them in tool output that the model then opens with
    executed Python. File operations and shell commands therefore have to agree on one
    path namespace. Virtual mode would re-root only the former (deepagents is explicit
    that it never constrains ``execute``), so ``/workspace/x`` would resolve one way
    for ``aread`` and another for ``open()``. Real paths keep them identical — and a
    real path is friendlier on a desktop, where the user can open the file themselves.
    """

    def __init__(self, thread_id: str):
        # Not necessarily this run's own thread: a background worker shares the
        # conversation's workspace, or its output would land where nobody looks.
        self._thread_id = workspace_thread(thread_id)
        self._work_dir = workspace_root() / self._thread_id
        self._announced = False
        super().__init__(
            root_dir=self._work_dir,
            virtual_mode=False,
            timeout=_DEFAULT_TIMEOUT,
            max_output_bytes=_NO_PRACTICAL_CAP,
            env=_command_env(),
            # `inherit_env=False` would hand commands an environment with no PATH at
            # all; `_command_env` is already a copy of ours with PATH fixed up.
            inherit_env=False,
        )

    # -- lifecycle ---------------------------------------------------------------
    #
    # Upstream's sandbox is created lazily and can be resumed or deleted. Locally the
    # equivalents are `mkdir -p` and `rmtree`, but the methods must exist: the HTTP
    # routes and the sync middleware call them by name.

    async def aresolve(self):
        """Ensure the workspace exists. Returns self, so callers can chain.

        Every filesystem call here is offloaded with ``asyncio.to_thread``. That is not
        politeness: ``langgraph dev`` runs a blocking-call detector, and a bare
        ``mkdir`` on the event loop aborts the run with ``BlockingError`` — which is
        exactly how the first live turn on this backend failed.
        """
        await asyncio.to_thread(self._work_dir.mkdir, parents=True, exist_ok=True)
        if not self._announced:
            self._announced = True
            # The desktop status line waits on this; without it the UI shows
            # "Creating sandbox…" forever on a cold thread.
            _emit_sandbox_status("ready", f"Local workspace: {self._work_dir}")
        return self

    async def try_resolve(self):
        """Return self only if this thread already has a workspace.

        Deliberately does **not** create one: the artifact and rendering routes use a
        ``None`` here to mean "nothing has run on this thread yet" and 404. Creating a
        directory as a side effect of a read-only request would make them all succeed
        with empty results.
        """
        exists = await asyncio.to_thread(self._work_dir.is_dir)
        return self if exists else None

    async def aresume(self) -> bool:
        """Nothing to resume — a directory does not go to sleep."""
        await self.aresolve()
        return True

    async def adelete(self) -> bool:
        await asyncio.to_thread(shutil.rmtree, self._work_dir, ignore_errors=True)
        return not await asyncio.to_thread(self._work_dir.exists)

    # -- path semantics ----------------------------------------------------------
    #
    # Reads need no help: with `virtual_mode=False` deepagents allows absolute paths
    # as-is and resolves relative ones under `cwd` — which is the workspace, and is
    # exactly what upstream's `_resolve_for_read` arranged. Writes do need help.

    def _reroute_write(self, path: str) -> str:
        """Send a write outside the workspace to ``<workspace>/<basename>``.

        Mirrors upstream's ``_resolve_for_write``. The deepagents virtual filesystem
        hands the model's "project root" writes through as ``/report.md``; in the
        sandbox that was the POSIX root, where the unprivileged user could not write,
        so upstream re-rooted them. Here the same rule does double duty as the one
        guardrail the file tools have: without it ``write("/etc/hosts", …)`` would be a
        real attempt on the host, and with it the file lands harmlessly in the
        workspace. `execute` is a different matter — see §18 on human-gating it.
        """
        if not isinstance(path, str) or not path:
            return path
        candidate = Path(path)
        if not candidate.is_absolute():
            return path
        try:
            candidate.relative_to(self._work_dir)
        except ValueError:
            return str(self._work_dir / candidate.name)
        return path

    def write(self, file_path: str, content: str):
        return super().write(self._reroute_write(file_path), content)

    def upload_files(self, files: list[tuple[str, bytes]]):
        return super().upload_files(
            [(self._reroute_write(path), data) for path, data in files]
        )

    # -- the surface upstream added on top of deepagents -------------------------

    async def aget_work_dir(self) -> str:
        await self.aresolve()
        return str(self._work_dir)

    async def aexecute(self, command: str, *, timeout: int | None = None) -> ExecuteResponse:
        """Run a command, truncating output as upstream does.

        Overridden rather than inherited for two reasons: the protocol's default
        ``aexecute`` is ``asyncio.to_thread(self.execute, command)``, which **silently
        drops the per-call timeout**; and this output can flow into the agent's
        context, so it gets upstream's cap.
        """
        return _truncate_execute_response(
            await self.aexecute_untruncated(command, timeout=timeout)
        )

    async def aexecute_untruncated(
        self, command: str, *, timeout: int | None = None
    ) -> ExecuteResponse:
        """Run a command with no cap, for server-side callers that parse the output.

        A completed Asta Theorizer record is ~500 KB; capped, it would be clipped to
        invalid JSON.
        """
        await self.aresolve()
        return await asyncio.to_thread(self._execute_with_token, command, timeout)

    def _execute_with_token(self, command: str, timeout: int | None) -> ExecuteResponse:
        """Run a command with a currently-valid Asta token in its environment.

        ``self.env`` was built once when this workspace was constructed, which is the bug
        this exists to close: a token that arrived after that — because the user signed in
        from the Setup pane, or because the seven-day one lapsed — never reached a single
        command. Refreshed here, where we are already off the event loop and ``asta`` may
        safely be spawned (see :func:`current_asta_token`).
        """
        token = current_asta_token()
        # `_env` is deepagents' own attribute (``LocalShellBackend.__init__`` builds it and
        # ``execute`` passes it to the subprocess). Reached defensively **because the first
        # version of this guessed `self.env` and every command failed with
        # `'LocalWorkspaceBackend' object has no attribute 'env'`** — turning a missing
        # token into a total outage. If a future deepagents renames it we lose the refresh,
        # which is a degradation; taking `execute` down with us is not.
        env = getattr(self, "_env", None)
        if token and isinstance(env, dict):
            env["ASTA_TOKEN"] = token
        elif token:
            logger.debug(
                "minime_local: no _env on the backend; the Asta token cannot be refreshed"
            )
        result = self.execute(command, timeout=timeout)
        _log_failure(command, result)
        return result

    @property
    def id(self) -> str:
        return f"local-{self._thread_id}"

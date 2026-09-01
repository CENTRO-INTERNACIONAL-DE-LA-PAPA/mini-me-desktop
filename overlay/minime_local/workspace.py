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
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from deepagents.backends.local_shell import LocalShellBackend
from deepagents.backends.protocol import ExecuteResponse

# Imported from upstream rather than reimplemented, so the local path truncates
# execute output exactly as the sandbox path does — the cap protects the model's
# context window and is not sandbox-specific.
from backend.sandbox import _emit_sandbox_status, _truncate_execute_response

from minime_local import authorship, ledger

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

#: Config key naming the project folder this run's workspace sits inside.
#:
#: **Why a project is a real directory and not only a label.** Docs §42 moved outputs out of the
#: distro into ``Documents\Mini-Me`` on one argument: files a researcher cannot find are files
#: that do not exist. A project is the unit a scientist actually works in, so the same argument
#: applies again one level up — a grouping that exists only inside the app is not a grouping they
#: can zip, back up, or drop on a shared drive (docs §105).
#:
#: Empty or absent means the conversation is not in a project, and its directory sits directly
#: under the root exactly as before. Every conversation that predates this stays where it is.
WORKSPACE_PROJECT_KEY = "__workspace_project__"


def workspace_root() -> Path:
    """The directory that holds one subdirectory per thread."""
    configured = os.getenv(WORKSPACE_ROOT_ENV)
    if configured:
        return Path(configured).expanduser()
    return Path.home() / ".mini-me" / "workspaces"


def _configurable() -> dict:
    """The live run's ``configurable``, or an empty dict outside a run.

    Read from the running config rather than passed in, because upstream constructs the backend
    as ``LazyLangsmithSandbox(thread_id)`` at two call sites this overlay deliberately does not
    touch.
    """
    try:
        from langgraph.config import get_config

        return (get_config() or {}).get("configurable") or {}
    except Exception:  # noqa: BLE001  # no runnable context: a read-only graph load
        return {}


# ---------------------------------------------------------------------------------------------
# **Two Rust tests read this file as text and `exec` slices of it**, to prove the project sanitiser
# and the thread pin agree across the language boundary without importing deepagents. One slice
# runs from the sanitiser below to the pin resolver after it; the other from the pin map to the
# logger.
#
# A function added *inside* either slice is exec'd in a namespace holding almost nothing, and dies
# with a bare `NameError` far from here. **Add new module-level code below the logger.**
#
# The markers are named in the tests, not repeated here: this comment first quoted them and the
# tests then matched *it* instead of the code — a warning that became the thing it warned about
# (§280).
# ---------------------------------------------------------------------------------------------
def workspace_project() -> str:
    """The project folder for this run, or ``""`` for none.

    Sanitised here as well as in the client, because this is the value that becomes a path. A
    project named ``Q1/Q2`` must not write outside the workspace root, and a name is a thing a
    person types.
    """
    raw = _configurable().get(WORKSPACE_PROJECT_KEY)
    name = str(raw).strip() if raw else ""
    if not name:
        return ""
    # One path segment, no traversal, and nothing Windows refuses.
    cleaned = "".join(
        character if (character.isalnum() or character in " -_") else "_"
        for character in name
    ).strip(" ._")
    return cleaned[:96]


#: Which conversation a background thread belongs to, remembered the first time we are told.
#:
#: **The config is not visible at every construction site.** A single background run built its
#: sandbox twice — once where `get_config()` carried the pin, and once where it did not — so two
#: directories appeared for one task: the nested one, empty, and a sibling holding every file. From
#: the outside they are indistinguishable from "the nesting did not work" (docs §151).
#:
#: This is the same shape as §123, where a `ContextVar` store did not survive a task boundary. The
#: answer there was the same as here: keep the fact somewhere the process shares, keyed by
#: something that cannot collide. A task id is unique to one background run, so the map is small,
#: correct, and cannot mis-file one conversation's work under another's.
_PINNED_BY_THREAD: dict[str, str] = {}


def workspace_thread(default: str) -> str:
    """Which thread's workspace this run should use.

    ``default`` (the run's own thread) unless something pinned it — see
    :data:`WORKSPACE_THREAD_KEY`. Read from the live run config rather than passed in,
    because upstream constructs the backend as ``LazyLangsmithSandbox(thread_id)`` at two
    call sites this overlay deliberately does not touch.

    Remembered per thread, because that config is visible at some of those sites and not others.
    """
    pinned = _configurable().get(WORKSPACE_THREAD_KEY)
    pinned = str(pinned).strip() if pinned else ""
    if pinned and default:
        # First sighting wins, and later ones must agree: a thread belongs to one conversation for
        # its whole life, so a *changed* pin is a bug worth seeing rather than silently honouring.
        remembered = _PINNED_BY_THREAD.setdefault(default, pinned)
        if remembered != pinned:
            logger.warning(
                "minime_local: thread %s was pinned to %s and is now %s — keeping the first",
                default,
                remembered,
                pinned,
            )
        return remembered
    return _PINNED_BY_THREAD.get(default, "") or default



logger = logging.getLogger(__name__)


#: One directory listing per thread per process, not one per construction.
#:
#: `LocalWorkspaceBackend` is built for every run *and* every request, and this walks the workspace
#: root — a `/mnt/c` mount, where a listing is slow and a constructor is not a place to be slow.
#: The answer cannot change while the process lives without a conversation being re-filed, and the
#: next launch reads it fresh.
_PROJECT_BY_THREAD: dict[str, str] = {}


def existing_project(root: Path, thread_id: str) -> str:
    """The project folder a conversation already lives in, found by looking.

    **Because `workspace_project()` cannot answer outside a run.** It reads the live run's
    `configurable`, and a *route* has none — so every route resolving a workspace for a
    conversation filed in a project computed `root/<thread>` while its runs had been writing to
    `root/<project>/<thread>`. Two folders for one conversation, and each side counting confidently
    from its own.

    Found rather than remembered. `workspace_thread` keeps a module-level map for the same problem,
    which works only while the process that saw the run is still alive — and the realistic moment
    for this is pressing a button after a restart, when that memory is empty. A directory that
    exists is durable and needs nothing to have happened first.

    One level deep, because a project is one path segment (`workspace_project` enforces that), and
    `""` when the conversation is ungrouped or has no folder yet — which is exactly the case where
    `root/<thread>` is right.
    """
    if thread_id in _PROJECT_BY_THREAD:
        return _PROJECT_BY_THREAD[thread_id]
    found = _look_for_project(root, thread_id)
    _PROJECT_BY_THREAD[thread_id] = found
    return found


def _look_for_project(root: Path, thread_id: str) -> str:
    """The listing itself, split out so the cache above reads as one thing."""
    try:
        # **A folder with something in it beats an empty one**, wherever each of them sits.
        #
        # An empty `root/<thread>` is usually debris from this very bug: a route resolved the wrong
        # path, `aresolve` created the directory, and it has sat there since. Answering "ungrouped"
        # because that debris exists is how the first attempt at this fix stayed broken on the
        # machine it was written for.
        #
        # But emptiness is not proof either way — a conversation filed in a project and not yet
        # written to has an empty folder too — so an empty *project* folder still beats nothing.
        # Content first, existence second, and the root only when it is the one with the files.
        if _has_anything_in_it(root / thread_id):
            return ""
        projects = sorted(entry for entry in root.iterdir() if entry.is_dir())
        for candidate in projects:
            if _has_anything_in_it(candidate / thread_id):
                return candidate.name
        for candidate in projects:
            if (candidate / thread_id).is_dir():
                return candidate.name
    except OSError:
        pass
    return ""


def _has_anything_in_it(path: Path) -> bool:
    """Whether `path` is a directory holding at least one entry.

    An empty directory is not evidence a conversation lives there. It is evidence that *something
    created a directory*, which is a much weaker claim and, here, usually a wrong one.
    """
    try:
        return path.is_dir() and any(path.iterdir())
    except OSError:
        return False

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


#: Appended to a failed command's output, so the model can see where it ran.
_CWD_NOTE = "\n[cwd] {} — this command ran here; use paths relative to it."


def _exit_and_output(result: Any) -> tuple[Any, str]:
    """`(exit_code, output)` from either shape the sandbox protocol returns."""
    if isinstance(result, dict):
        return result.get("exit_code"), result.get("output") or ""
    return getattr(result, "exit_code", None), getattr(result, "output", "") or ""


def _record(
    command: str,
    result: Any,
    work_dir: Any,
    seconds: float,
    *,
    conversation_dir: Any | None = None,
) -> None:
    """Add this command to the conversation's own record. **Never raises.**

    Every command, not only the failures `_log_failure` keeps: the ones that matter most are the
    ones that worked and wrote somewhere nobody looked (§160), and under the conversation-wide
    approval grant (§41) nobody sees them go past at all.

    Placed here because this is the one function every `execute` already passes through — the same
    reason `_say_where_it_ran` is here, and the difference between a record and a record with a
    hole in it.
    """
    try:
        exit_code, _ = _exit_and_output(result)
        finished = time.time()
        owner = conversation_dir if conversation_dir is not None else work_dir
        record = ledger.entry(
            command,
            exit_code=exit_code,
            seconds=seconds,
            work_dir=owner,
            cwd=work_dir,
            at=datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z"),
        )
        started = finished - (seconds or 0.0)
        # Two independent observations. The first checks paths named in the command; the second
        # scans both named external directories and the command's real cwd. The directory scan is
        # how `cd /tmp/job && python analysis.py` and an outside worker writing
        # `missingness.png` are found. Only filesystem-observed writes reach `wrote`; `outside`
        # remains the weaker string claim.
        named_writes = ledger.written_during(record["outside"], started, finished)
        tree_writes, tree_truncated = ledger.observed_writes_under(
            record["outside"], started, finished
        )
        # A cwd already beneath the conversation is visible in Outputs, including a correctly
        # pinned worker's nested folder. Scanning it would spend the 512-entry budget on files the
        # app already has. The scan exists for the distinct case: a real command cwd outside the
        # conversation that owns this record.
        if ledger.paths_outside([str(work_dir)], owner):
            cwd_writes, truncated = ledger.observed_writes(work_dir, started, finished)
        else:
            cwd_writes, truncated = [], False
        observed_outside = ledger.paths_outside(tree_writes + cwd_writes, owner)
        record["wrote"] = list(dict.fromkeys(named_writes + observed_outside))
        record["scan_truncated"] = tree_truncated or truncated
        ledger.append(owner, record)
    except Exception:  # noqa: BLE001 — a diagnostic must never be what takes `execute` down
        logger.debug("minime_local: could not record a command", exc_info=True)


def _say_where_it_ran(result: Any, work_dir: Any) -> None:
    """Tell the model which directory a *failed* command ran in.

    # Why

    A background worker asked to plot a dataset it had just written ran this, and failed:

        python -c "... pd.read_csv('/data/potato_late_blight.csv') ...
                   plt.savefig('/plots/histograms.png')"

    then retried with `/home/piero_linux/Mini-Me/...`, and failed again. Neither directory exists.
    The workspace was `/mnt/c/Users/.../Documents/Mini-Me/<thread>`, commands already run **with
    that as their working directory**, and `pd.read_csv('potato_late_blight.csv')` would have
    worked on the first attempt.

    The model was guessing, because nothing had ever told it. `aresolve` announces the path — to
    the *desktop status line*. The one participant who needs it never sees it, and the failure it
    gets back (`No such file or directory`) names the path it invented rather than the one it has.

    So a run reported *"the plots and summary tables have been saved to files"* with one CSV on
    disk. That claim is its own defect, but the cause underneath it is this: two honest attempts,
    both blind.

    # Only on failure

    A working command must stay quiet. This text enters the model's context, and a line appended to
    every `execute` is a line the model learns to skip — which is how the corpus-id diagnostic
    stopped being read (§116/§132).

    Never raises. A response shape we cannot append to is a lost hint; an exception here would take
    `execute` down entirely, which is the trade this file already records making wrongly once.
    """
    try:
        exit_code, output = _exit_and_output(result)
        if exit_code in (None, 0):
            return
        note = _CWD_NOTE.format(work_dir)
        if str(work_dir) in output:
            return  # It already knows; do not repeat.
        if isinstance(result, dict):
            result["output"] = output + note
        else:
            object.__setattr__(result, "output", output + note)
    except Exception:  # noqa: BLE001
        logger.debug("minime_local: could not append the working directory to a failure")


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
        root = workspace_root()
        # The run's own config first — it is authoritative and it is what creates the folder in the
        # first place. Outside a run it is empty, and then the folder that already exists is the
        # only thing that knows (§280).
        project = workspace_project() or existing_project(root, self._thread_id)
        # **A background worker gets a folder *inside* the conversation's, named after itself.**
        #
        # Three earlier attempts moved these files between sibling directories and none of them
        # answered the actual requirement, which the researcher put plainly: *"the idea is to
        # somehow view it in the app, not as a different folder outside the conversation folder."*
        #
        # Nesting answers it without the app changing at all. `workspace::outputs` already descends
        # through named subfolders and shows the relative path (§143), so `019fe.../plot_yield.png`
        # appears in the conversation's Outputs panel by itself — and *which run produced it* stays
        # legible, which writing straight into the conversation's folder would have destroyed by
        # mixing every worker's files together.
        #
        # The coordinator's own runs are unaffected: `workspace_thread` returns their own id, the
        # two are equal, and there is nothing to nest.
        self._conversation_dir = root.joinpath(
            *([project] if project else []), self._thread_id
        )
        parts: list[str] = []
        if thread_id and thread_id != self._thread_id:
            parts.append(thread_id)
        self._work_dir = self._conversation_dir.joinpath(*parts)
        # **The value everything else depends on, printed once.**
        #
        # A background worker that cannot find a file the conversation just wrote has exactly two
        # explanations — it is looking in the wrong directory, or the file is not where the
        # conversation thinks it is — and from the outside they are the same sentence: *"could not
        # find ./potato_yield.csv"*. The directory is computed right here from three inputs and was
        # never reported, so neither could be ruled out (docs §115).
        #
        # This is the fifth time in a week that the thing needed to end an argument was a value
        # the program already had: §99's laid-out width, §91's count of adoptable threads, §110's
        # overlay path, §114's config keys.
        #
        # `warning` for a real run, `debug` for a read-only graph load. `GET /threads/{id}/state`
        # builds a backend too — the client polls it while watching a task — and those have no run
        # config, so they resolve to the run's own thread at the root and touch nothing. At
        # warning level they outnumbered the lines that matter six to one, which is how a log
        # stops being read (docs §116).
        speak = logger.warning if _configurable() else logger.debug
        speak(
            "minime_local: workspace %s (own thread %s, pinned to %s, project %r)",
            self._work_dir,
            thread_id,
            self._thread_id,
            project or "<none>",
        )
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
        path = self._reroute_write(file_path)
        result = super().write(path, content)
        # After the write, so a failed one is not recorded as having happened.
        authorship.record(self._work_dir, [path])
        return result

    def upload_files(self, files: list[tuple[str, bytes]]):
        rerouted = [(self._reroute_write(path), data) for path, data in files]
        result = super().upload_files(rerouted)
        authorship.record(self._work_dir, [path for path, _ in rerouted])
        return result

    def execute(self, command: str, *, timeout: int | None = None) -> ExecuteResponse:
        """Run a command, and record that it ran.

        **Overridden for the record, and here rather than in `aexecute`, which is where the first
        version put it.** deepagents registers *two* execute tools — a synchronous one calling
        `execute` and an async one calling `aexecute` (`middleware/filesystem.py:1538` and `:1627`)
        — and which is used depends on how the graph was built. Recording in `aexecute` covered one
        of them.

        This is the point both reach: the protocol's `aexecute` delegates to `execute`, and so does
        ours. One override, no double entry, and no way for a command to run without appearing.

        That is the sixth time in this project a correct component has been wired to one of two
        paths (§254, §257, §258, §259, §261, §262). The lesson each time is the same and it is
        cheap to apply: find the function *everything* passes through, not the one you happened to
        be editing.
        """
        # **The workspace has to exist first.** `aresolve` creates it, and only `aexecute` awaits
        # that — so a command arriving through the *synchronous* tool ran with a `cwd` that was not
        # there and died with `FileNotFoundError` before it could do anything. Pre-existing, and
        # invisible while only the async tool was in use.
        #
        # `mkdir` rather than `aresolve`: this is a sync method, the directory is the only part of
        # resolution a command needs, and `exist_ok` makes it free on every call after the first.
        try:
            self._work_dir.mkdir(parents=True, exist_ok=True)
        except OSError:
            logger.debug("minime_local: could not make the workspace before a command", exc_info=True)

        started = time.monotonic()
        try:
            result = super().execute(command, timeout=timeout)
        finally:
            elapsed = time.monotonic() - started
        _record(
            command,
            result,
            self._work_dir,
            elapsed,
            conversation_dir=self._conversation_dir,
        )
        return result

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
        # **Both `aexecute` paths run through here**, which is why the bracket is here rather than
        # on the caller: a plot written by a script inside `execute` registers no artifact, and the
        # desktop app's own comment says those are most of the files it shows.
        #
        # Taken *before* the command, from the same clock the filesystem stamps with, and compared
        # afterwards against the tree this workspace owns. One process reading two of its own
        # readings around one command it started — an interval it can prove, not one inferred from
        # a timeline (docs §201). `authorship` names whoever issued it.
        started = time.time()
        author = authorship.current_agent()
        try:
            return await asyncio.to_thread(self._execute_with_token, command, timeout)
        finally:
            # In `finally` because a command that fails part-way still wrote what it wrote, and a
            # file on disk with no author is the defect this exists to close. The walk itself
            # never raises out — see `authorship.record_written_since`.
            await asyncio.to_thread(
                authorship.record_written_since, self._work_dir, started, author
            )

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
        _say_where_it_ran(result, self._work_dir)
        return result

    @property
    def id(self) -> str:
        return f"local-{self._thread_id}"

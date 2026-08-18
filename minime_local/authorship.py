"""Write down which specialist produced which file.

**The client cannot work this out, and it stopped trying.** §199 gave the desktop app the one
attribution it can make without guessing: a background worker runs on its own LangGraph thread
and writes into a folder named after it, so the folder *is* the record. Everything else — the
specialists a conversation consults, `exploratory_data_analysis`, `academic_researcher`, the
coordinator itself — shares one thread and one directory, and nothing on the wire says who wrote
what. Matching file timestamps against the road strip's arrival windows would produce an
attribution for every file and would be a guess, which `provenance.rs` refuses on the grounds that
a provenance record that quietly guesses is worse than none, because it will be believed.

Here it is not a guess. This process *is* the writer. It knows which delegation it is inside,
because the `task` tool was handed the name; and it knows which files a command produced, because
it started the command and can look at the directory afterwards. Asked for plainly: *"we need to
record the write."*

**Two write paths, both covered.**

* `write` / `upload_files` — deepagents' file tools. The path is the argument; nothing to infer.
* `aexecute` — a shell command, usually a Python script that draws plots. The app's own comment
  says these are *most* of the files, and none of them registers an artifact. So the directory is
  read after the command and anything newer than its start belongs to whoever issued it. That is
  a measurement bracketing one command, not an inference from a timeline: the process that took
  the "before" timestamp is the process that ran the command.

**What it deliberately does not do.** It never runs the walk on a workspace it does not own, and
it never descends into a nested thread folder — a background worker writing while the coordinator
runs a command is the one case where two authors are genuinely active in one tree, and the
worker's own folder is already the answer for its files (§199).

Failures are logged and swallowed. Losing a line of provenance is a worse panel; raising here
would be a turn that died while writing a file successfully, which is §18's rule about what an
overlay may risk.
"""

from __future__ import annotations

import contextvars
import functools
import json
import logging
import os
import time
from pathlib import Path

logger = logging.getLogger(__name__)

#: The file the desktop app reads, inside the conversation's workspace.
#:
#: Dot-prefixed on purpose: `workspace::collect_outputs` skips any basename starting with `.`, so
#: the record of what produced the files never turns up as one of them.
MANIFEST = ".authorship.jsonl"

#: What a write outside any delegation is attributed to.
#:
#: Not `""` and not "unknown". The coordinator writing a file itself is a fact, and one worth
#: telling apart from a specialist doing it — which is exactly what the researcher could not see.
COORDINATOR = "coordinator"

#: Ceiling on one post-command scan, matching the client's own bounded walk.
#:
#: An agent can create a virtualenv or unpack a dataset under its workspace. The cap is said out
#: loud when it bites rather than silently truncating, because "no line in the manifest" and
#: "there were too many files to look at" produce the same empty panel.
MAX_ENTRIES = 4096

#: Which delegation the current context is inside. Empty outside one.
#:
#: A `ContextVar` rather than a module global because the coordinator can run two delegations at
#: once: LangGraph schedules concurrent tool calls as asyncio tasks, and a task copies the context
#: at creation, so each delegation's name is visible only to its own subtree. A global would have
#: the second specialist overwrite the first and both files come out wearing one name. Same shape,
#: and the same reason, as `spine.py`'s `_http_project`.
_current: contextvars.ContextVar[str] = contextvars.ContextVar(
    "minime_local_current_agent", default=""
)


def current_agent() -> str:
    """The specialist this call is running inside, or the coordinator."""
    return _current.get().strip() or COORDINATOR


def looks_like_thread_id(name: str) -> bool:
    """A generated LangGraph thread directory — a *different* author's tree.

    The same shape check the client applies (`workspace::looks_like_thread_id`), deliberately: the
    two sides disagreeing about what a thread folder looks like would mean the backend recording
    a worker's files under the conversation while the app files them under the worker.
    """
    if len(name) != 36:
        return False
    for at, character in enumerate(name):
        if at in (8, 13, 18, 23):
            if character != "-":
                return False
        elif character not in "0123456789abcdefABCDEF":
            return False
    return True


def _manifest(work_dir: Path) -> Path:
    return Path(work_dir) / MANIFEST


def record(work_dir, paths, agent: str | None = None) -> None:
    """Append one line per path, naming who wrote it.

    Append-only and one JSON object per line, so a crash mid-write costs the last record rather
    than the file, and so two writers never have to agree on a document structure. The client
    takes the last line for a path, which is what a filesystem does anyway: the most recent writer
    owns it.
    """
    work_dir = Path(work_dir)
    author = (agent or current_agent()).strip() or COORDINATOR
    lines = []
    now = time.time()
    for path in paths:
        relative = _relative(work_dir, path)
        if relative is None:
            continue
        lines.append(
            json.dumps({"path": relative, "agent": author, "at": round(now, 3)}, ensure_ascii=False)
        )
    if not lines:
        return
    try:
        work_dir.mkdir(parents=True, exist_ok=True)
        with _manifest(work_dir).open("a", encoding="utf-8") as manifest:
            manifest.write("\n".join(lines) + "\n")
    except OSError as error:
        logger.warning("minime_local: could not record authorship (%s)", error)


def record_written_since(work_dir, since: float, agent: str | None = None) -> int:
    """Attribute every file in the workspace touched at or after `since`. Returns how many.

    `since` is taken by the caller *before* it starts the command, from the same clock the
    filesystem stamps with — so this is one process comparing two of its own readings, not two
    machines agreeing about time.
    """
    work_dir = Path(work_dir)
    written = []
    for path in _walk(work_dir):
        try:
            if path.stat().st_mtime >= since:
                written.append(path)
        except OSError:
            continue
    if written:
        record(work_dir, written, agent)
    return len(written)


def _walk(work_dir: Path):
    """Every file in the workspace, skipping other authors' trees and our own bookkeeping."""
    seen = 0
    for root, directories, files in os.walk(work_dir):
        # Pruned in place, which `os.walk` documents as the way to stop it descending: a nested
        # thread folder is a background worker's own workspace and its files are already
        # attributed by the folder they are in (§199).
        directories[:] = [
            directory
            for directory in directories
            if not directory.startswith(".")
            and directory != "__pycache__"
            and not looks_like_thread_id(directory)
        ]
        for name in files:
            if name.startswith("."):
                continue
            seen += 1
            if seen > MAX_ENTRIES:
                logger.warning(
                    "minime_local: stopped attributing after %d files in %s — the rest of this "
                    "command's output is unattributed rather than wrongly attributed",
                    MAX_ENTRIES,
                    work_dir,
                )
                return
            yield Path(root) / name


def _relative(work_dir: Path, path) -> str | None:
    """The path as the client will see it: relative to the workspace, forward slashes.

    `None` for anything outside, which is not an error worth a line in the log — `write` reroutes
    those before they reach the filesystem, and a manifest entry pointing outside the workspace is
    one the client could never match against a file it lists.
    """
    try:
        relative = Path(path).resolve().relative_to(Path(work_dir).resolve())
    except (ValueError, OSError):
        return None
    text = relative.as_posix()
    return text or None


def install(module) -> None:
    """Make the `task` tool announce which specialist it is about to run.

    `_build_task_tool` is module-level in `deepagents.middleware.subagents` and is called once,
    from `SubAgentMiddleware.__init__` — which happens when `backend/agent.py` builds the agent,
    after the `deepagents` import this is hooked from has finished. Same ordering argument as
    `_rewrite_execute_description`, and the same reason it is done through the package rather than
    by watching the submodule for an import that has already happened.
    """
    original = getattr(module, "_build_task_tool", None)
    if original is None:
        logger.warning(
            "minime_local: no _build_task_tool to wrap — files will be attributed to the "
            "coordinator even when a specialist wrote them (docs §201)"
        )
        return

    @functools.wraps(original)
    def build(*args, **kwargs):
        tool = original(*args, **kwargs)
        try:
            _announce(tool)
        except Exception as error:  # noqa: BLE001
            logger.warning("minime_local: could not wrap the task tool (%s)", error)
        return tool

    module._build_task_tool = build
    logger.warning("minime_local: file writes are attributed to the specialist that made them")


def _name_of(args, kwargs) -> str:
    """The `subagent_type` the coordinator asked for.

    Positional fallback because the tool's own signature is `(description, subagent_type,
    runtime)` and a caller is free to use it; keyword first because LangChain does.
    """
    named = kwargs.get("subagent_type")
    if isinstance(named, str) and named.strip():
        return named.strip()
    if len(args) > 1 and isinstance(args[1], str):
        return args[1].strip()
    return ""


def _set(tool, attribute: str, value) -> None:
    """Assign through pydantic's validation, or around it."""
    try:
        setattr(tool, attribute, value)
    except Exception:  # noqa: BLE001 — a frozen or strictly-validated model
        object.__setattr__(tool, attribute, value)


def _announce(tool) -> None:
    """Wrap the tool's callables so the name is set for the whole delegation."""
    sync_fn = getattr(tool, "func", None)
    async_fn = getattr(tool, "coroutine", None)

    if sync_fn is not None:

        @functools.wraps(sync_fn)
        def func(*args, **kwargs):
            token = _current.set(_name_of(args, kwargs))
            try:
                return sync_fn(*args, **kwargs)
            finally:
                _current.reset(token)

        _set(tool, "func", func)

    if async_fn is not None:
        # **`async def`, and the `await` inside the block.** A sync wrapper would set the variable,
        # build the coroutine, reset, and hand back something unawaited — so the name would be
        # gone by the time the subagent actually ran, and every file would come out attributed to
        # the coordinator while the wrapper looked installed. `spine.py` paid for this lesson.
        @functools.wraps(async_fn)
        async def coroutine(*args, **kwargs):
            token = _current.set(_name_of(args, kwargs))
            try:
                return await async_fn(*args, **kwargs)
            finally:
                _current.reset(token)

        _set(tool, "coroutine", coroutine)

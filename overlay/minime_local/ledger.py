"""A record of every command a conversation ran, and what each one named outside it.

# Why this exists rather than a guard

`execute_rule.py` states the position and it has not changed: a command is an arbitrary shell
program, and pattern-matching it for writes "would produce a containment claim that is false in
every case nobody thought of". Nothing here blocks anything.

What is missing is not prevention — it is *knowing*. §160's sixteen files went to `/tmp` and the
researcher's Outputs panel was empty, and the way they found out was by looking, later, having been
told. Under the conversation-wide approval grant (§41) nobody sees the commands at all: that grant
exists because "a researcher who must click Approve twelve times stops reading by the third", which
is correct, and it means the gate is stood down for exactly the runs that produce the most files.

So this records. It is the half of §219's argument — *records and does not block* — applied to the
one tool that has never had either.

# What is claimed, exactly

Three things, with deliberately different strengths:

* **The command ran**: its text, its exit code, when, and for how long. Exact.
* **These absolute paths appear in its text and are not under the conversation folder.** A property
  of the string, checkable by reading it. `python -c "...open('/tmp/x.csv','w')..."` names
  `/tmp/x.csv`, and that is all this says about it.
* **These files in the command's real working directory changed while it ran.** A bounded
  filesystem observation, independent of whether the command text named them.

A command can still write beyond both views — after changing to a directory it never names,
through a background process that outlives it, or past the scan's safety limit. The value is not
completeness; it is that the relative-write failure which actually happened is visible on the day
it happens rather than a week later.
"""

from __future__ import annotations

import json
import logging
import re
from pathlib import PurePosixPath
from typing import Any, Iterable

#: This module's own channel. `minime_local` lines reach the backend log the app writes, which is
#: where anybody looking for an absent record will already be.
logger = logging.getLogger(__name__)

#: Where the record lives, inside the conversation's own folder.
#:
#: Inside, deliberately: it moves when the conversation is filed into a project, it goes when the
#: conversation is deleted, and the app can read it without being told where to look. A dot-folder
#: so `workspace::outputs` does not list it as something the research produced.
RECORD_DIR = ".mini-me"
RECORD_NAME = "commands.jsonl"

#: The other record kept in that folder: what a subagent *said* it produced.
#:
#: Named here rather than in `backend/middleware/claims.py` because the folder is this module's,
#: and two files writing into the same directory under two different notions of where it is, is
#: the defect §278 was. The writer lives in the middleware; the address lives here.
CLAIMS_NAME = "claims.jsonl"

#: One command's text, clipped. A generated script can be tens of kilobytes and the record is meant
#: to be read; the full text is in the backend log for anyone who needs it.
TEXT_CAP = 2_000

#: How many commands to keep. A runaway loop must not fill a researcher's disk, and a record nobody
#: can scroll is not a record. The newest are kept, because the question is always "what just
#: happened".
MAX_ENTRIES = 500

#: Locations excluded from the report — **for noise, not for safety**.
#:
#: `/usr/bin/python3`, `/bin/sh` and `/dev/null` appear in a large fraction of commands and none of
#: them is a place a researcher's results land; a report that names them every time is a report that
#: stops being read, which is §116's and §132's failure exactly. Excluding them is a claim about
#: what is worth showing, not a claim that nothing can be written there.
SYSTEM_PREFIXES = ("/dev/", "/proc/", "/sys/", "/usr/", "/bin/", "/sbin/", "/lib/", "/etc/", "/opt/")

# A `/`-rooted token. The lookbehind drops `https://…` and `file://…` — a URL is not a path, and
# reporting the host of every download as an escaped write is how this becomes noise. Quoting and
# shell metacharacters end the token, so `'/tmp/x.csv'` yields `/tmp/x.csv`.
_POSIX_PATH = re.compile(r"(?<![\w:/])(/[A-Za-z0-9._~][^\s'\"<>|;&()\\]*)")


def named_paths(command: str) -> list[str]:
    """Every absolute path the command's text names, in order, without duplicates.

    Order is kept because it is how the command reads, and a researcher scanning the report is
    matching it against something they remember doing.
    """
    seen: list[str] = []
    for match in _POSIX_PATH.finditer(command or ""):
        path = match.group(1).rstrip(".,:")
        if path and path not in seen:
            seen.append(path)
    return seen


def outside(command: str, work_dir: str | PurePosixPath) -> list[str]:
    """The named paths that are not under the conversation folder and not system locations.

    `work_dir` itself is not reported, nor anything beneath it: those are the files the Outputs
    panel already shows, and repeating them here would bury the ones it cannot.
    """
    base = PurePosixPath(str(work_dir)).as_posix().rstrip("/")
    reported = []
    for path in named_paths(command):
        if base and (path == base or path.startswith(base + "/")):
            continue
        if path.startswith(SYSTEM_PREFIXES):
            continue
        reported.append(path)
    return reported


#: How far outside the measured window a file's mtime may sit and still count as this command's.
#:
#: Filesystem timestamps are coarser than a stopwatch, and the write can land a moment after the
#: process returns. A second either side is enough for that and short enough that the *previous*
#: command's output is not claimed by this one — which is the false positive that would matter,
#: since it is what a copy button would act on.
CLOCK_SLACK = 1.0

#: The working-directory scan is on `execute`'s hot path. A command may unpack a dataset or build a
#: virtualenv, so it has both a depth and an entry budget. Hitting either is reported by
#: :func:`observed_writes`; a silent cap would merely turn the live defect into a missing-513th-file
#: defect (§301).
SCAN_MAX_DEPTH = 3
SCAN_MAX_ENTRIES = 512


def written_during(paths: Iterable[str], start: float, end: float) -> list[str]:
    """Which of `paths` exist as files and were modified while the command ran.

    **This is what turns "named" into "wrote", and it is the whole difference.** A command's text
    cannot say whether a path was read or written — `pd.read_csv('/tmp/input.csv')` names a file the
    researcher owns, and treating that as output would be how a well-meant tidy-up steals somebody's
    data. An mtime inside the command's own window is a fact about the file rather than a guess
    about the string.

    Still not everything the command wrote: a path it never names, a file a background process
    writes after it returns, a write that fails to update mtime on an exotic filesystem. What is
    claimed is exactly the title of this function.
    """
    from pathlib import Path

    written: list[str] = []
    for path in paths:
        try:
            candidate = Path(path)
            if not candidate.is_file():
                continue
            mtime = candidate.stat().st_mtime
        except OSError:
            continue
        if start - CLOCK_SLACK <= mtime <= end + CLOCK_SLACK:
            written.append(path)
    return written


def observed_writes(
    work_dir: str | PurePosixPath,
    start: float,
    end: float,
    *,
    max_depth: int = SCAN_MAX_DEPTH,
    max_entries: int = SCAN_MAX_ENTRIES,
) -> tuple[list[str], bool]:
    """Files under the command's real cwd whose mtime falls inside its execution window.

    Breadth-first and deterministic so shallow outputs win when the budget is exhausted. Symlinked
    directories are never followed: the command may create one pointing at a home directory or an
    ancestor, and this is a bounded observation of one cwd rather than a general filesystem crawl.

    Returns ``(files, truncated)``. The boolean belongs in the record and the warning belongs in the
    backend log; either without the other leaves one of the researcher and developer unable to tell
    "nothing was written" from "the scan stopped first".
    """
    from pathlib import Path

    root = Path(str(work_dir)).absolute()
    return _observed_writes_in_roots(
        [root], start, end, max_depth=max_depth, max_entries=max_entries
    )


def observed_writes_under(
    paths: Iterable[str],
    start: float,
    end: float,
    *,
    max_depth: int = SCAN_MAX_DEPTH,
    max_entries: int = SCAN_MAX_ENTRIES,
) -> tuple[list[str], bool]:
    """Files changed beneath directory paths explicitly named by a command.

    A shell's working directory is private state: a process launched in the conversation can run
    ``cd /tmp/job && python analysis.py`` without changing the cwd visible to its parent. The
    absolute directory in that command is still useful evidence, but only as a bounded place to
    observe — never as proof that every existing file there is the command's output.

    The entry budget is shared across all named roots. Nested duplicate roots are collapsed before
    walking, so repeating a directory in a long command cannot multiply either work or results.
    Files named directly are handled by :func:`written_during`; this function deliberately walks
    directories only.
    """
    from pathlib import Path

    roots: list[Path] = []
    for raw in paths:
        candidate = Path(raw).absolute()
        try:
            if candidate.is_symlink() or not candidate.is_dir():
                continue
        except OSError:
            continue

        # Keep only the shallowest distinct roots. This is lexical on purpose: resolving would
        # follow a symlink, which the observation contract explicitly refuses to do.
        if any(_is_within(candidate, root) for root in roots):
            continue
        roots = [root for root in roots if not _is_within(root, candidate)]
        roots.append(candidate)

    roots.sort(key=str)
    return _observed_writes_in_roots(
        roots, start, end, max_depth=max_depth, max_entries=max_entries
    )


def _is_within(path: Any, parent: Any) -> bool:
    try:
        path.relative_to(parent)
        return True
    except ValueError:
        return False


def _observed_writes_in_roots(
    roots: Iterable[Any],
    start: float,
    end: float,
    *,
    max_depth: int,
    max_entries: int,
) -> tuple[list[str], bool]:
    """The shared bounded walk behind cwd and command-named-directory observation."""
    from collections import deque

    roots = list(roots)
    pending = deque((root, 0) for root in roots)
    candidates: list[str] = []
    inspected = 0
    truncated = False

    while pending:
        directory, depth = pending.popleft()
        try:
            entries = sorted(directory.iterdir(), key=lambda entry: entry.name)
        except OSError:
            continue
        for candidate in entries:
            if inspected >= max_entries:
                truncated = True
                pending.clear()
                break
            inspected += 1
            try:
                if candidate.is_symlink():
                    continue
                if candidate.is_file():
                    candidates.append(str(candidate.absolute()))
                elif candidate.is_dir() and candidate.name != RECORD_DIR:
                    if depth < max_depth:
                        pending.append((candidate, depth + 1))
                    else:
                        truncated = True
            except OSError:
                continue

    if truncated:
        where = ", ".join(str(root) for root in roots)
        logger.warning(
            "minime_local: stopped observing writes in %s at the safety limit "
            "(%d entries, depth %d); later files may be absent from recovery",
            where,
            max_entries,
            max_depth,
        )
    return written_during(candidates, start, end), truncated


def paths_outside(
    paths: Iterable[str], work_dir: str | PurePosixPath
) -> list[str]:
    """Absolute ``paths`` not inside the researcher's conversation directory.

    Unlike :func:`outside`, these paths came from the filesystem rather than command text. Keeping
    the two functions separate is the safety property: a named path may be an input, while an
    observed write is the only kind the recovery button may act on automatically.
    """
    from pathlib import Path

    base = Path(str(work_dir)).absolute()
    reported: list[str] = []
    for raw in paths:
        candidate = Path(raw).absolute()
        try:
            candidate.relative_to(base)
        except ValueError:
            path = str(candidate)
            if path not in reported:
                reported.append(path)
    return reported


def entry(
    command: str,
    *,
    exit_code: int | None,
    seconds: float | None,
    work_dir: str | PurePosixPath,
    at: str,
    wrote: list[str] | None = None,
    cwd: str | PurePosixPath | None = None,
    scan_truncated: bool = False,
) -> dict[str, Any]:
    """One line of the record.

    `at` is passed in rather than read from the clock so the shape can be asserted without one, and
    `wrote` likewise: deciding it needs the filesystem, and this function stays a pure description
    of what happened.
    """
    text = (command or "").strip()
    clipped = len(text) > TEXT_CAP
    named = outside(text, work_dir)
    return {
        "at": at,
        "command": text[:TEXT_CAP] + ("…" if clipped else ""),
        "clipped": clipped,
        "exit": exit_code,
        "seconds": None if seconds is None else round(seconds, 2),
        "cwd": str(cwd if cwd is not None else work_dir),
        "outside": named,
        # Independent of `outside`: this list comes from filesystem observation, not string
        # parsing. A relative write may be here without ever appearing in the command text.
        "wrote": list(dict.fromkeys(wrote or [])),
        "scan_truncated": scan_truncated,
    }


def trim(lines: Iterable[str], keep: int = MAX_ENTRIES) -> list[str]:
    """The last `keep` lines. Newest kept, because the question is what just happened."""
    kept = [line for line in lines if line.strip()]
    return kept[-keep:] if keep > 0 else []


def append(
    work_dir: str | PurePosixPath, record: dict[str, Any], name: str = RECORD_NAME
) -> str | None:
    """Add one line to one of the conversation's records. **Never raises.**

    An unwritable record must cost a researcher nothing. `_say_where_it_ran` makes the same trade
    and states the reason: taking `execute` down to protect a diagnostic is the wrong way round,
    and this file already records making that mistake once.

    **Returns where it wrote, or `None` with the reason swallowed** — and that return value is not
    decoration. The first version answered nothing at all, so a caller could not tell a written
    record from a failed one, and when the claims record silently failed to appear there were two
    invisible paths and no way to choose between them (§285). Never raising and never saying are
    different promises; this keeps the first and drops the second.
    """
    try:
        from pathlib import Path

        folder = Path(str(work_dir)) / RECORD_DIR
        folder.mkdir(parents=True, exist_ok=True)
        target = folder / name
        line = json.dumps(record, ensure_ascii=False)

        existing: list[str] = []
        if target.exists():
            existing = target.read_text(encoding="utf-8", errors="replace").splitlines()
        kept = trim(existing + [line])
        # Rewritten rather than appended, because the cap has to hold. A bounded file that only
        # grows is not bounded.
        target.write_text("\n".join(kept) + "\n", encoding="utf-8")
        return str(target)
    except Exception:  # noqa: BLE001 — see the docstring; nothing here may reach the researcher
        # **The traceback, because "it returned None" is not a diagnosis.** The claims record
        # failed to appear on a real machine and this line was `pass`; the caller could report
        # *that* it failed and nothing could report *why*, so the next step was another release
        # (§286). Warning rather than exception-level: a diagnostic that fails is worth one line,
        # not a wall of text on every turn if the disk is full.
        logger.warning(
            "minime_local: could not write %s in %s",
            name,
            work_dir,
            exc_info=True,
        )
        return None


def record_path(work_dir: str | PurePosixPath, name: str = RECORD_NAME) -> str:
    """Where a record for this conversation lives. Named so a caller can *say* where it looked."""
    from pathlib import Path

    return str(Path(str(work_dir)) / RECORD_DIR / name)


def read(work_dir: str | PurePosixPath, name: str = RECORD_NAME) -> list[dict[str, Any]]:
    """One of the conversation's records, oldest first. A malformed line is skipped, never fatal."""
    from pathlib import Path

    target = Path(str(work_dir)) / RECORD_DIR / name
    try:
        lines = target.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return []
    records: list[dict[str, Any]] = []
    for line in lines:
        if not line.strip():
            continue
        try:
            records.append(json.loads(line))
        except ValueError:
            continue
    return records


def outside_files(work_dir: str | PurePosixPath) -> dict[str, list[str]]:
    """Files this conversation's commands wrote outside it, split by whether they are still there.

    **Only `wrote`, never `outside`.** A named path may be the researcher's own input, and the
    difference between the two lists is the difference between offering somebody their results and
    taking their data (see :func:`written_during`).

    **Both halves are returned, and that is the point.** The first version answered only "here is
    what can be copied", so a conversation whose files had since been swept from `/tmp` produced an
    empty list indistinguishable from one that never wrote anything — and the button reported
    `brought=0 refused=0`, which is a sentence with no information in it. A caller cannot explain
    what it was not told.

    Deduplicated and in the order the commands produced them, which is the order a person
    remembers making them in.
    """
    from pathlib import Path

    present: list[str] = []
    gone: list[str] = []
    for record in read(work_dir):
        for path in record.get("wrote") or []:
            if path in present or path in gone:
                continue
            try:
                (present if Path(path).is_file() else gone).append(path)
            except OSError:
                gone.append(path)
    return {"present": present, "gone": gone}


def collectable(work_dir: str | PurePosixPath) -> list[str]:
    """Just the files still there. Kept because it reads better at the call site."""
    return outside_files(work_dir)["present"]


def free_name(folder: "Path", name: str) -> "Path":  # type: ignore[name-defined]
    """A path in `folder` that is not taken, suffixing before the extension.

    The same rule `workspace::adopt` uses on the app side: `results.csv` becomes `results-2.csv`,
    never `results.csv-2`, because the extension is what makes a file openable.
    """
    from pathlib import Path

    candidate = folder / name
    if not candidate.exists():
        return candidate
    stem, suffix = Path(name).stem, Path(name).suffix
    for index in range(2, 100):
        candidate = folder / f"{stem}-{index}{suffix}"
        if not candidate.exists():
            return candidate
    raise FileExistsError(f"{name} already exists a hundred times over in this conversation")


def collect(work_dir: str | PurePosixPath, paths: Iterable[str]) -> dict[str, Any]:
    """**Copy** the given files into the conversation. Never moves, never overwrites.

    Copied rather than moved for a reason that is not squeamishness: a script often writes a file
    and reads it back later in the same run, and a later turn may expect it where it was left.
    Moving it breaks work that is still going; copying costs disk and breaks nothing.

    Returns what arrived and what did not, with a reason for each — a partial result is the normal
    case here and reporting only the count would hide it.
    """
    from pathlib import Path
    import shutil

    folder = Path(str(work_dir))
    brought: list[dict[str, str]] = []
    refused: list[dict[str, str]] = []
    for path in paths:
        source = Path(path)
        try:
            if not source.is_file():
                refused.append({"path": path, "reason": "it is no longer there"})
                continue
            if folder in source.parents:
                refused.append({"path": path, "reason": "it is already in this conversation"})
                continue
            folder.mkdir(parents=True, exist_ok=True)
            target = free_name(folder, source.name)
            shutil.copy2(source, target)
            brought.append({"path": path, "as": target.name})
        except Exception as exc:  # noqa: BLE001 — one bad file must not lose the others
            refused.append({"path": path, "reason": str(exc)})
    return {"brought": brought, "refused": refused}

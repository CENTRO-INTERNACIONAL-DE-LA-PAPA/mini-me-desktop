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

Two things, and neither is "what the command wrote":

* **The command ran**: its text, its exit code, when, and for how long. Exact.
* **These absolute paths appear in its text and are not under the conversation folder.** A property
  of the string, checkable by reading it. `python -c "...open('/tmp/x.csv','w')..."` names
  `/tmp/x.csv`, and that is all this says about it.

A command can write somewhere it never names — a script it invokes, a library's cache, a relative
path after a `cd`. This will not see those, and says so. The value is not completeness; it is that
the failure which actually happened, twice, is the one this makes visible on the day it happens
rather than a week later.
"""

from __future__ import annotations

import json
import re
from pathlib import PurePosixPath
from typing import Any, Iterable

#: Where the record lives, inside the conversation's own folder.
#:
#: Inside, deliberately: it moves when the conversation is filed into a project, it goes when the
#: conversation is deleted, and the app can read it without being told where to look. A dot-folder
#: so `workspace::outputs` does not list it as something the research produced.
RECORD_DIR = ".mini-me"
RECORD_NAME = "commands.jsonl"

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


def entry(
    command: str,
    *,
    exit_code: int | None,
    seconds: float | None,
    work_dir: str | PurePosixPath,
    at: str,
    wrote: list[str] | None = None,
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
        "outside": named,
        # A subset of `outside`, never more: something the command did not name cannot be checked.
        "wrote": [path for path in (wrote or []) if path in named],
    }


def trim(lines: Iterable[str], keep: int = MAX_ENTRIES) -> list[str]:
    """The last `keep` lines. Newest kept, because the question is what just happened."""
    kept = [line for line in lines if line.strip()]
    return kept[-keep:] if keep > 0 else []


def append(work_dir: str | PurePosixPath, record: dict[str, Any]) -> None:
    """Add one line to the conversation's record. **Never raises.**

    An unwritable record must cost a researcher nothing. `_say_where_it_ran` makes the same trade
    and states the reason: taking `execute` down to protect a diagnostic is the wrong way round,
    and this file already records making that mistake once.
    """
    try:
        from pathlib import Path

        folder = Path(str(work_dir)) / RECORD_DIR
        folder.mkdir(parents=True, exist_ok=True)
        target = folder / RECORD_NAME
        line = json.dumps(record, ensure_ascii=False)

        existing: list[str] = []
        if target.exists():
            existing = target.read_text(encoding="utf-8", errors="replace").splitlines()
        kept = trim(existing + [line])
        # Rewritten rather than appended, because the cap has to hold. A bounded file that only
        # grows is not bounded.
        target.write_text("\n".join(kept) + "\n", encoding="utf-8")
    except Exception:  # noqa: BLE001 — see the docstring; nothing here may reach the researcher
        pass


def record_path(work_dir: str | PurePosixPath) -> str:
    """Where the record for this conversation lives. Named so a caller can *say* where it looked."""
    from pathlib import Path

    return str(Path(str(work_dir)) / RECORD_DIR / RECORD_NAME)


def read(work_dir: str | PurePosixPath) -> list[dict[str, Any]]:
    """The conversation's record, oldest first. A malformed line is skipped, never fatal."""
    from pathlib import Path

    target = Path(str(work_dir)) / RECORD_DIR / RECORD_NAME
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

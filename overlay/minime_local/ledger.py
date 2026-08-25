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


def entry(
    command: str,
    *,
    exit_code: int | None,
    seconds: float | None,
    work_dir: str | PurePosixPath,
    at: str,
) -> dict[str, Any]:
    """One line of the record.

    `at` is passed in rather than read from the clock so the shape can be asserted without one.
    """
    text = (command or "").strip()
    clipped = len(text) > TEXT_CAP
    return {
        "at": at,
        "command": text[:TEXT_CAP] + ("…" if clipped else ""),
        "clipped": clipped,
        "exit": exit_code,
        "seconds": None if seconds is None else round(seconds, 2),
        "outside": outside(text, work_dir),
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

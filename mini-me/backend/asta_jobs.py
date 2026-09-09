"""Shared plumbing for Asta's async CLI jobs (theorizer, DataVoyager, AutoDiscovery).

Asta's theorizer, DataVoyager, and AutoDiscovery are all long-running, async jobs that
this backend submits and polls rather than blocks a turn on. Each grew its own copy of
the same handful of primitives — running a sandbox command and reading its output
whichever shape the sandbox answers in, pulling a JSON record out of merged
stdout/stderr, checking an A2A task's ``status.state``, and validating a task id before
it reaches a shell or a URL path. This module holds the pieces that turned out to be
identical (or close enough that a shared implementation is honest to every caller), so a
fix to one of them — `§224`'s dict-shaped-response handling, independently rediscovered
in a second module before it was fixed in a third — lands once instead of needing to be
found again per file.

`_extract_json` below is the union of what `datavoyager_tools.py` and
`autodiscovery_tools.py` each did, not a straight copy of either: `datavoyager_tools.py`
split off a `[stderr]` suffix before parsing and `autodiscovery_tools.py` did not, and
only `autodiscovery_tools.py` matched a JSON array as well as an object. A first pass at
this module took `autodiscovery_tools.py`'s version verbatim on the (wrong) assumption
that the two were byte-identical, and silently lost DataVoyager's `[stderr]`-splitting —
confirmed as a real behavioral regression no existing test caught, since none exercised
a stderr-suffixed record. The version below tries every combination either original
tried, so it cannot parse *less* than both did.

What deliberately stays in each of the three modules rather than here: the Asta CLI
command a job submits (each has different flags), the markdown rendered for its specific
output shape, and how its outputs are persisted to the workspace. AutoDiscovery's own
status vocabulary (``normalise_status`` in `autodiscovery_tools.py`) also stays there —
it polls a REST job status, not an A2A task, and maps a different, uppercase vocabulary
(``RUNNING``/``SUCCEEDED``/...) that `_state_of` below (a plain ``status.state`` read on
an A2A task) has no equivalent for. Forcing the two into one shape would misrepresent
what either service actually said. `theory_tools.py` also keeps its own `_extract_json`:
it tries the whole string as JSON before falling back to brace-matching, a different (and
simpler) algorithm than the one below, because a theorizer task record is reduced
in-sandbox before it ever reaches this parser.
"""

from __future__ import annotations

import json
import re
from typing import Any

#: An A2A task id, as both the theorizer and DataVoyager mint them.
_UUID_RE = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")


async def _run(sandbox: Any, command: str, timeout: int) -> str:
    """Run a command in the sandbox and return its stdout, whichever shape it answers in.

    Prefers the untruncated path: a reduced task record is parsed server-side (never fed
    to the model) and a truncated one is unparseable JSON. Both response shapes are
    handled for the reason §224 taught the hard way — a dict-shaped response read only
    for attributes yields an empty string, indistinguishable from a command that printed
    nothing.
    """
    runner = getattr(sandbox, "aexecute_untruncated", None) or sandbox.aexecute
    resp = await runner(command, timeout=timeout)
    if isinstance(resp, dict):
        return resp.get("output") or ""
    return getattr(resp, "output", "") or ""


def _extract_json(output: str) -> Any:
    """Pull the first JSON value out of command output that may carry log lines around it.

    Tries the text before a `[stderr]` marker first — `aexecute` appends stderr there, and a
    record that parses cleanly on its own must not be lost to unrelated warning noise after it
    — then the full output, each as a whole-string parse before falling back to bracket-matching
    `{...}` or `[...]` (DataVoyager's and AutoDiscovery's reduced records can be a JSON array,
    not just an object).
    """
    text = (output or "").strip()
    if not text:
        return None
    head = text.split("[stderr]", 1)[0].strip()
    for candidate in (head, text):
        if not candidate:
            continue
        try:
            return json.loads(candidate)
        except json.JSONDecodeError:
            pass
        for opener, closer in (("{", "}"), ("[", "]")):
            start = candidate.find(opener)
            end = candidate.rfind(closer)
            if start != -1 and end > start:
                try:
                    return json.loads(candidate[start : end + 1])
                except json.JSONDecodeError:
                    continue
    return None


def _state_of(task: dict[str, Any] | None) -> str | None:
    """An A2A task's `status.state`, or `None` for anything that is not one."""
    return ((task or {}).get("status") or {}).get("state")


def is_valid_task_id(task_id: str) -> bool:
    """True if `task_id` is a well-formed A2A task UUID (guards a poll route)."""
    return bool(task_id) and bool(_UUID_RE.fullmatch(task_id))

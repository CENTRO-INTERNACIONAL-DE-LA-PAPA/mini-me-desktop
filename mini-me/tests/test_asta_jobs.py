"""Guards for the shared Asta job plumbing (`backend.asta_jobs`).

`theory_tools.py`, `datavoyager_tools.py`, and `autodiscovery_tools.py` each import from
here rather than keeping their own copies of these primitives — see the module docstring
for which pieces turned out to be genuinely shared and which stayed put. These tests exist
so a change here is checked once, directly, rather than only indirectly through whichever
of the three callers happens to exercise a given path.
"""

from __future__ import annotations

import asyncio

from backend.asta_jobs import _extract_json, _run, _state_of, is_valid_task_id


def test_run_prefers_untruncated_execute() -> None:
    class Sandbox:
        async def aexecute_untruncated(self, command, *, timeout=None):
            class Resp:
                output = "from the untruncated path"

            return Resp()

        async def aexecute(self, command, *, timeout=None):
            raise AssertionError("aexecute_untruncated exists; aexecute must not be used")

    assert asyncio.run(_run(Sandbox(), "cmd", 10)) == "from the untruncated path"


def test_run_falls_back_to_aexecute_when_untruncated_is_absent() -> None:
    class Sandbox:
        async def aexecute(self, command, *, timeout=None):
            class Resp:
                output = "from the truncated path"

            return Resp()

    assert asyncio.run(_run(Sandbox(), "cmd", 10)) == "from the truncated path"


def test_run_reads_a_dict_shaped_response_too() -> None:
    """§224: a dict-shaped response read only for attributes yields "", indistinguishable
    from a command that printed nothing."""

    class Sandbox:
        async def aexecute_untruncated(self, command, *, timeout=None):
            return {"exit_code": 0, "output": "a dict answered this time"}

    assert asyncio.run(_run(Sandbox(), "cmd", 10)) == "a dict answered this time"


def test_extract_json_parses_a_bare_object() -> None:
    assert _extract_json('{"a": 1}') == {"a": 1}


def test_extract_json_parses_a_bare_array() -> None:
    assert _extract_json('["figure-01.png"]') == ["figure-01.png"]


def test_extract_json_tolerates_leading_log_lines() -> None:
    assert _extract_json('WARNING: stale\n{"a": 1}\n') == {"a": 1}


def test_extract_json_tolerates_stderr_suffix() -> None:
    """The record parses even when the text after `[stderr]` contains something that looks
    like JSON but is not — head is tried, whole and brace-sliced, before the full text is."""
    merged = '{"a": 1}\n[stderr]\nsome warning about a dict {not json}'
    assert _extract_json(merged) == {"a": 1}


def test_extract_json_returns_none_on_garbage() -> None:
    assert _extract_json("no json here") is None
    assert _extract_json("") is None
    assert _extract_json(None) is None  # type: ignore[arg-type]


def test_state_of_handles_none() -> None:
    assert _state_of(None) is None
    assert _state_of({}) is None
    assert _state_of({"status": {"state": "completed"}}) == "completed"


def test_is_valid_task_id() -> None:
    assert is_valid_task_id("6580ec74-121a-4757-b5e2-2e1ed9fc210e")
    assert not is_valid_task_id("")
    assert not is_valid_task_id("../etc/passwd")
    assert not is_valid_task_id("not-a-uuid")

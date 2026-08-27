"""No synchronous filesystem call may sit in an `async def` we own.

`langgraph dev` wraps the interpreter in `blockbuster`, which raises `BlockingError` on a
synchronous `os.mkdir` inside an async context. Two of ours did:

* `claims._write`, reached from `ClaimsRecorder.aafter_agent` — the claims record failed to be
  written on every real turn while every test passed, because the tests call it directly and
  there is no loop to block (§287);
* `checkpointer.checkpointer`, at server startup — where the cost is not a slow saver but **no
  saver**, and therefore conversations that do not load.

Both were live only after `langgraph-api` went from 0.9.0 to 0.12.6, which is what arrived when
the backend finally started updating (§283). A runtime getting stricter is exactly the kind of
change a bundle nobody was shipping had been hiding.

This walks the syntax tree rather than trusting anyone to remember, because the failure is
invisible from a test that calls the function directly — which is every test we would otherwise
write for it.
"""

from __future__ import annotations

import ast
from pathlib import Path

import pytest

#: Names that touch a filesystem or spawn a process synchronously.
#:
#: Deliberately by *name*, not by resolved symbol: `Path.mkdir`, `os.mkdir` and a bare `open`
#: are the same hazard, and a checker that needed to resolve them would miss the one spelled a
#: way nobody predicted. False positives are cheap here — `asyncio.to_thread` silences them and
#: is the right answer anyway.
BLOCKING = frozenset(
    {
        "mkdir", "makedirs", "write_text", "read_text", "write_bytes", "read_bytes",
        "remove", "unlink", "rmtree", "rename", "copy2", "copyfile", "check_output",
    }
)

ROOTS = ("mini-me/backend", "overlay/minime_local")


def _repo() -> Path:
    return Path(__file__).resolve().parent.parent.parent


def _offenders(tree: ast.AST) -> list[tuple[str, int, str]]:
    found: list[tuple[str, int, str]] = []
    for fn in ast.walk(tree):
        if not isinstance(fn, ast.AsyncFunctionDef):
            continue
        # Anything textually inside an `asyncio.to_thread(...)` argument is already off the loop.
        excused: set[int] = set()
        for call in ast.walk(fn):
            if (
                isinstance(call, ast.Call)
                and isinstance(call.func, ast.Attribute)
                and call.func.attr == "to_thread"
            ):
                for argument in call.args:
                    for inner in ast.walk(argument):
                        excused.add(id(inner))
        for call in ast.walk(fn):
            if not isinstance(call, ast.Call) or id(call) in excused:
                continue
            name = (
                call.func.attr
                if isinstance(call.func, ast.Attribute)
                else getattr(call.func, "id", "")
            )
            if name in BLOCKING:
                found.append((fn.name, call.lineno, name))
    return found


@pytest.mark.parametrize("root", ROOTS)
def test_no_async_function_touches_the_filesystem_directly(root):
    repo = _repo()
    complaints: list[str] = []
    for path in sorted((repo / root).rglob("*.py")):
        try:
            tree = ast.parse(path.read_text(encoding="utf-8"))
        except SyntaxError:  # not ours to compile
            continue
        for function, line, name in _offenders(tree):
            complaints.append(
                f"{path.relative_to(repo)}:{line} — async {function}() calls {name}(); "
                f"wrap it in asyncio.to_thread or blockbuster will raise on a real turn"
            )
    assert not complaints, "\n" + "\n".join(complaints)


def test_the_checker_would_notice_the_bug_it_was_written_for():
    """A checker that cannot fail is a comment. This is the shape that got through twice."""
    offenders = _offenders(
        ast.parse(
            "import os\n"
            "async def aafter_agent(self):\n"
            "    os.makedirs('/tmp/x', exist_ok=True)\n"
        )
    )
    assert offenders == [("aafter_agent", 3, "makedirs")]

    # And the fix silences it, so nobody is tempted to delete the check to get green.
    assert not _offenders(
        ast.parse(
            "import asyncio, os\n"
            "async def aafter_agent(self):\n"
            "    await asyncio.to_thread(os.makedirs, '/tmp/x', exist_ok=True)\n"
        )
    )

"""Every registered route's docstring survives the schema generator that reads it.

Starlette builds its OpenAPI schema by running `yaml.safe_load` over each route's docstring
(`starlette/schemas.py:parse_docstring`). Ordinary English is not YAML: a sentence containing a
colon followed by a space reads as a mapping key, and if the rest does not agree, the loader
raises.

Nothing breaks when it does — langgraph catches it and falls back to using the text as a
description. What it costs is the log. Four routes in this repo each printed a full
`ScannerError` traceback **on every single boot**, so `mini-me-desktop-backend.log` opened with
four stack traces about routes that were working perfectly. That log is the diagnostic path for
real failures — a researcher's lost conversations were found in it (§303) — and four fake
tracebacks at the top is a tax on every future diagnosis.

Starlette's own escape hatch is documented in its source: *"We support having regular docstrings
before the schema definition"*, implemented as `docstring.split("---")[-1]`. A docstring whose
prose is followed by a `---` line hands the loader only what comes after it.

**Why this is a test and not four fixed docstrings.** The four were fixed by hand; the fifth is
the one that matters. A route added later with a colon in its first paragraph reintroduces the
noise silently, and nobody reads a log to notice something *absent*.

**Only endpoints that are actually registered.** The first version of this walked every function
in the package and flagged three private helpers — `_check_approval`, `_spend_approval`,
`_as_source` — which starlette never sees, because it reads the docstrings of *endpoints*. A test
that reports work nobody needs to do is one somebody eventually silences, so the set here is read
from the `Route(..., endpoint=...)` table itself.
"""

from __future__ import annotations

import ast
import pathlib

import pytest
import yaml

ROUTES = pathlib.Path(__file__).resolve().parents[1] / "backend" / "routes"


def _registered_endpoint_names() -> set[str]:
    """Every name passed as `endpoint=` to a `Route(...)` in the package.

    Read from the source rather than by importing: this must run without the backend's
    dependencies installed, and an import here would drag in the whole graph.
    """
    names: set[str] = set()
    for path in sorted(ROUTES.glob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call):
                continue
            func = node.func
            if not (isinstance(func, ast.Name) and func.id == "Route"):
                continue
            for keyword in node.keywords:
                if keyword.arg == "endpoint" and isinstance(keyword.value, ast.Name):
                    names.add(keyword.value.id)
    return names


def _documented_endpoints():
    """(module, name, docstring) for every registered endpoint that has one."""
    wanted = _registered_endpoint_names()
    for path in sorted(ROUTES.glob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        for node in ast.walk(tree):
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            if node.name not in wanted:
                continue
            # `clean=False`: starlette reads `__doc__` as written, indentation included, and
            # cleaning it here would test a string the server never sees.
            doc = ast.get_docstring(node, clean=False)
            if doc:
                yield path.name, node.name, doc


def test_the_endpoint_table_was_actually_found():
    """The walk finds the routes.

    A glob or a matcher that quietly finds nothing passes every assertion below it, which is the
    shape of test this repo has shipped three times while asserting nothing (§294, §297).
    """
    registered = _registered_endpoint_names()
    assert len(registered) > 10, f"only found {len(registered)} registered endpoints"
    # The four this test was written for must be in the set, or it is checking the wrong things.
    for name in (
        "collect_outside_files",
        "start_sandbox",
        "theorizer_status",
        "get_project",
    ):
        assert name in registered, f"{name} is no longer a registered endpoint"

    documented = {name for _, name, _ in _documented_endpoints()}
    assert documented, "no registered endpoint has a docstring — the matcher is broken"


@pytest.mark.parametrize(
    ("module", "name", "docstring"),
    [pytest.param(m, n, d, id=f"{m}::{n}") for m, n, d in _documented_endpoints()],
)
def test_docstring_does_not_break_the_schema_generator(module, name, docstring):
    """Starlette's exact rule, on the exact text the server will hand it."""
    tail = docstring.split("---")[-1]
    try:
        yaml.safe_load(tail)
    except yaml.YAMLError as error:
        pytest.fail(
            f"{module}::{name} makes the OpenAPI schema generator raise, so every boot logs a "
            f"traceback for a working route. End the prose with a line containing only `---` "
            f"and everything above it stops being parsed as YAML.\n\n{error}"
        )

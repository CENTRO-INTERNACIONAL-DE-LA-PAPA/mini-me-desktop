"""Guards for the paper search tool (`backend.paper_tools`).

The CLI contract is pinned here for the reason `test_theory_tools` pins the theorizer's: the
deployed bug behind that tool was an agent hand-building an invocation with a flag that did not
exist. `--fields` is the one that matters here — without it the CLI returns titles and identifiers
and none of the bibliographic fields, and the tool silently goes back to producing references with
nothing in them.

The parser fixture is a real `asta papers search` response, trimmed to two records.
"""

from __future__ import annotations

import asyncio
import json

from backend.paper_tools import (
    MAX_LIMIT,
    SEARCH_FIELDS,
    _build_search_command,
    _parse_search,
    find_papers,
    summarise,
)

# A real response, verbatim apart from being cut to two records.
REAL_OUTPUT = json.dumps(
    {
        "total": 2,
        "data": [
            {
                "paperId": "5fdf96788880894711e20d81ebf42371094e89eb",
                "externalIds": {
                    "DOI": "10.47280/REVFACAGRON(LUZ).V38.N3.03",
                    "CorpusId": 237744014,
                },
                "title": (
                    "Late blight resistance of Ecuadorian potato landraces: field evaluation "
                    "and farmer’s perception"
                ),
                "venue": "Revista de la Facultad de Agronomía",
                "year": 2021,
                "authors": [
                    {"name": "Á. Monteros-Altamirano"},
                    {"name": "Ricardo Delgado"},
                ],
                "tldr": {"text": "Five landraces showed the best field resistance."},
                "abstract": "Late blight, caused by Phytophthora infestans, is one of the most…",
            },
            {
                "externalIds": {"CorpusId": 87709200},
                "title": "A paper with no DOI",
                "year": 2010,
                "authors": [{"name": "Z. Suxian"}],
            },
        ],
    }
)


def test_the_cli_contract_is_pinned():
    """Flag drift should fail in CI, not in a researcher's run."""
    argv = _build_search_command("late blight resistance", 5)
    assert argv[:3] == ["asta", "papers", "search"]
    assert "late blight resistance" in argv
    assert argv[argv.index("--limit") + 1] == "5"
    # Without --fields the CLI returns no journal, no year and no DOI, and the whole tool is
    # pointless while still appearing to work.
    assert "--fields" in argv
    fields = argv[argv.index("--fields") + 1]
    for needed in ("title", "authors", "year", "venue", "journal", "externalIds"):
        assert needed in fields, needed
    assert fields == SEARCH_FIELDS


def test_the_limit_is_bounded_rather_than_trusted():
    """A model asking for a thousand papers gets a shortlist, not a database dump."""
    assert _build_search_command("q", 1000)[-3] == str(MAX_LIMIT)
    assert _build_search_command("q", 0)[-3] == "1"
    assert _build_search_command("q", -5)[-3] == "1"


def test_a_warning_on_stderr_does_not_cost_the_result():
    """`aexecute` merges stderr after a marker; the records are on stdout."""
    merged = REAL_OUTPUT + "\n[stderr]\nWARNING: rate limit approaching\n"
    assert len(_parse_search(merged)) == 2
    assert _parse_search("") == []
    assert _parse_search("command not found: asta") == []
    assert _parse_search("{not json") == []


def test_find_papers_uses_run_asta_cli_when_local(monkeypatch) -> None:
    """Local execution runs `asta` as a native subprocess instead of through a sandbox shell —
    mirroring how `paper_tools._is_local` gates it in production."""
    import backend.paper_tools as module
    from backend.runtime import _active_sandbox

    calls = []

    async def fake_run_asta_cli(args, *, cwd, timeout):
        calls.append((args, cwd, timeout))
        return REAL_OUTPUT, "WARNING: rate limit approaching"

    class _LocalSandbox:
        async def aget_work_dir(self) -> str:
            return "/workspace"

    monkeypatch.setattr(module, "_is_local", lambda sandbox: True)
    monkeypatch.setattr(module, "run_asta_cli", fake_run_asta_cli)

    async def go():
        token = _active_sandbox.set(_LocalSandbox())
        try:
            return await find_papers.coroutine(query="late blight resistance", limit=5)
        finally:
            _active_sandbox.reset(token)

    answer = json.loads(asyncio.run(go()))
    # The stderr warning did not cost the result — merged in the same way `aexecute` merges it.
    assert answer["count"] == 2
    argv, cwd, timeout = calls[0]
    assert argv[:2] == ["papers", "search"]
    assert cwd == "/workspace"


def test_every_paper_comes_back_with_a_reference_already_written():
    """The point of the tool: the model receives citations, it does not compose them."""
    found = summarise(_parse_search(REAL_OUTPUT))
    assert len(found) == 2, "every paper the search returned, not a filtered subset"

    first = found[0]
    assert first["citation"] == (
        "Monteros-Altamirano, Á., & Delgado, R. (2021). Late blight resistance of Ecuadorian "
        "potato landraces: field evaluation and farmer’s perception. Revista de la Facultad de "
        "Agronomía. https://doi.org/10.47280/REVFACAGRON(LUZ).V38.N3.03"
    )
    assert first["link"] == (
        "https://api.semanticscholar.org/DOI:10.47280/REVFACAGRON(LUZ).V38.N3.03"
    )
    assert first["summary"] == "Five landraces showed the best field resistance."

    # No DOI in the record, so the link falls back to the corpus id — which is present on
    # essentially everything the search returns.
    assert found[1]["link"] == "https://api.semanticscholar.org/CorpusID:87709200"
    # And the reference simply has no DOI, rather than a plausible one.
    assert "doi.org" not in found[1]["citation"]

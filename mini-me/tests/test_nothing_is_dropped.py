"""Every paper the search returned reaches the reader.

*"We should get all the papers that asta finds and is up to the scietinst to selct and drop the
ones they want."* The subagent is asked for that in its prompt and still returns a shortlist — 9
of 24 on the run that prompted this — so it is enforced where the list leaves the backend.
"""

from __future__ import annotations

import json

from langchain_core.messages import AIMessage, HumanMessage, ToolMessage

from backend.middleware.artifacts import ArtifactCaptureMiddleware
from backend.paper_tools import papers_in, unreported


class _Source:
    def __init__(self, citation, relevance, link):
        self.citation, self.relevance, self.link = citation, relevance, link


class _Structured:
    def __init__(self, sources):
        self.sources = sources


def _search(papers, name="find_papers"):
    return ToolMessage(
        content=json.dumps({"query": "q", "count": len(papers), "papers": papers}),
        tool_call_id="call_1",
        name=name,
    )


PAPERS = [
    {"citation": "Ames, M. (2010). Blight in landraces.", "link": "https://s2/A", "title": "Blight in landraces"},
    {"citation": "Cruz, R. (2014). QTL mapping in Solanum.", "link": "https://s2/B", "title": "QTL mapping in Solanum"},
    {"citation": "Diaz, L. (2019). Field trials in Puno.", "link": "https://s2/C", "title": "Field trials in Puno"},
]


def test_the_papers_the_summary_skipped_are_added_back():
    state = {
        "messages": [HumanMessage(content="find blight work"), _search(PAPERS)],
        "structured_response": _Structured(
            [_Source("Ames, M. (2010). Blight in landraces.", "Directly on point.", "https://s2/A")]
        ),
    }
    produced = ArtifactCaptureMiddleware("academic_researcher").after_agent(state, None)
    sources = produced["artifacts"]["sources"]

    assert len(sources) == 3, "two retrieved papers were dropped by the summary"
    # The subagent's own ranking still leads; the rest follow.
    assert sources[0]["link"] == "https://s2/A"
    assert sources[0]["relevance"] == "Directly on point."
    assert {s["link"] for s in sources[1:]} == {"https://s2/B", "https://s2/C"}
    assert all(s["relevance"].startswith("Returned by the search") for s in sources[1:])
    # Every source carries provenance, including the ones added back — an artifact the research
    # spine cannot account for is worse than one it never saw.
    assert len(produced["artifacts"]["edges"]) == 3


def test_a_paper_kept_with_a_rewritten_citation_is_not_duplicated():
    """Models rewrite references and almost never rewrite identifiers — match on the link."""
    state = {
        "messages": [_search(PAPERS)],
        "structured_response": _Structured(
            [_Source("Ames (2010), 'Blight in landraces', in press.", "Relevant.", "https://s2/A")]
        ),
    }
    sources = ArtifactCaptureMiddleware("academic_researcher").after_agent(state, None)["artifacts"]["sources"]
    assert [s["link"] for s in sources].count("https://s2/A") == 1


def test_a_paper_kept_with_a_rewritten_link_is_not_duplicated():
    """And where the link *was* changed, the title inside the citation still identifies it."""
    state = {
        "messages": [_search(PAPERS)],
        "structured_response": _Structured(
            [_Source("Ames, M. (2010). Blight in landraces. J. Phytopath.", "Relevant.", "https://doi.org/10.x/y")]
        ),
    }
    sources = ArtifactCaptureMiddleware("academic_researcher").after_agent(state, None)["artifacts"]["sources"]
    assert len(sources) == 3, [s["citation"] for s in sources]


def test_repeated_searches_do_not_repeat_papers():
    found = papers_in([_search(PAPERS), _search(PAPERS[:2])])
    assert [p["link"] for p in found] == ["https://s2/A", "https://s2/B", "https://s2/C"]


def test_only_this_conversations_searches_count():
    """Scoped by construction: the subagent's own messages, not a process-wide store."""
    assert papers_in([]) == []
    assert papers_in([AIMessage(content="I recall a paper by Sorensen")]) == []
    assert papers_in([_search(PAPERS, name="snippet_search")]) == []


def test_malformed_tool_output_costs_nothing():
    """A search that returned junk must not take the answer down with it."""
    broken = ToolMessage(content="not json", tool_call_id="c", name="find_papers")
    assert papers_in([broken, _search(PAPERS)]) == PAPERS


def test_an_answer_that_reported_everything_adds_nothing():
    sources = [{"citation": p["citation"], "relevance": "", "link": p["link"]} for p in PAPERS]
    assert unreported(PAPERS, sources) == []

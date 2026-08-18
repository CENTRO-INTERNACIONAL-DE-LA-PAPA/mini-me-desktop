"""The academic researcher must search before it can answer.

These drive the real middleware against a real `ModelRequest`, because the defect being fixed is
precisely a value written where nothing reads it: middleware that sets `tool_choice` while a
structured output tool is bound is discarded by `langchain/agents/factory.py`, silently. A test
that only asserted "the middleware sets tool_choice" would have passed against the broken version.
"""

from __future__ import annotations

import asyncio
import json
from types import SimpleNamespace

import pytest
from langchain_core.messages import AIMessage, HumanMessage, ToolMessage

from backend.middleware.search_first import (
    SEARCH_TOOL,
    KeepSources,
    SearchBeforeCiting,
    _has_searched,
)


def _request(messages, response_format="AcademicResearchResults"):
    """A stand-in for `ModelRequest` with the two fields the gate touches.

    `override` returns a new object, matching LangChain's immutable contract — so a gate that
    mutated in place instead would fail here rather than in production.
    """

    class Request:
        def __init__(self, state, response_format, tool_choice=None):
            self.state = state
            self.response_format = response_format
            self.tool_choice = tool_choice

        def override(self, **changes):
            return Request(
                self.state,
                changes.get("response_format", self.response_format),
                changes.get("tool_choice", self.tool_choice),
            )

    return Request({"messages": messages}, response_format)


def _tool_message(name: str) -> ToolMessage:
    return ToolMessage(content="{}", tool_call_id="call_1", name=name)


def test_the_first_call_is_forced_into_a_search():
    """Nothing has run yet, so the model gets one option and it is the literature."""
    seen = {}
    middleware = SearchBeforeCiting()
    middleware.wrap_model_call(
        _request([HumanMessage(content="find work on late blight")]),
        lambda request: seen.update(
            format=request.response_format, choice=request.tool_choice
        ),
    )
    assert seen["choice"] == SEARCH_TOOL
    # The other half of the fix, and the half that is easy to leave out. While a structured output
    # tool is bound, `tool_choice` never reaches the model — so naming the tool without dropping
    # the response format is a no-op with a convincing-looking test beside it.
    assert seen["format"] is None


def test_once_a_search_has_returned_the_request_is_untouched():
    """The schema must come back, or the subagent can never produce its artifacts."""
    seen = {}
    messages = [
        HumanMessage(content="find work on late blight"),
        AIMessage(content="", tool_calls=[]),
        _tool_message(SEARCH_TOOL),
    ]
    middleware = SearchBeforeCiting()
    middleware.wrap_model_call(
        _request(messages),
        lambda request: seen.update(
            format=request.response_format, choice=request.tool_choice
        ),
    )
    assert seen["choice"] is None
    assert seen["format"] == "AcademicResearchResults"


def test_a_failed_search_still_opens_the_gate():
    """A search that came back empty must cost a citation, never the turn.

    The gate opens on a result *existing*, not on it being useful — otherwise a missing sandbox or
    a timeout would force the same call again forever.
    """
    messages = [HumanMessage(content="q"), _tool_message(SEARCH_TOOL)]
    assert _has_searched(messages)


def test_the_mcp_search_counts_as_searching():
    """A model that reached for Asta's own search has engaged with the literature."""
    assert _has_searched([_tool_message("snippet_search")])
    assert not _has_searched([_tool_message("write_todos")])
    assert not _has_searched([AIMessage(content="I recall a paper by Sorensen")])


def test_the_async_path_gates_too():
    """The server runs the graph asynchronously; a sync-only hook would do nothing there."""
    seen = {}

    async def handler(request):
        seen.update(format=request.response_format, choice=request.tool_choice)

    asyncio.run(
        SearchBeforeCiting().awrap_model_call(
            _request([HumanMessage(content="q")]), handler
        )
    )
    assert seen["choice"] == SEARCH_TOOL
    assert seen["format"] is None


@pytest.mark.parametrize("messages", [None, []])
def test_an_empty_conversation_is_not_a_search(messages):
    assert not _has_searched(messages)


# --- the file the researcher takes away ---------------------------------------------------------

class _Sandbox:
    """The two calls `KeepSources` makes, and a record of what was written."""

    def __init__(self, error=None):
        self.written: dict[str, str] = {}
        self.error = error

    async def aget_work_dir(self):
        return "/home/user/workspace/"

    async def awrite(self, path, content):
        self.written[path] = content
        return SimpleNamespace(error=self.error)


def _structured(*citations):
    return SimpleNamespace(
        sources=[
            SimpleNamespace(citation=c, relevance="discussed", link=f"https://doi.org/{i}")
            for i, c in enumerate(citations)
        ]
    )


def _kept(structured, messages, sandbox=None):
    sandbox = sandbox or _Sandbox()
    result = asyncio.run(
        KeepSources(sandbox).aafter_agent(
            {"structured_response": structured, "messages": messages}, None
        )
    )
    assert result is None, "KeepSources must not touch state"
    return sandbox


def test_the_papers_are_written_where_the_researcher_can_take_them():
    """*"I want the user to have it."* Until this they lived only in a panel and a dict."""
    sandbox = _kept(_structured("Alquraishi, M. (2021). Machine learning…"), [])
    assert list(sandbox.written) == ["/home/user/workspace/papers.json"]
    rows = json.loads(sandbox.written["/home/user/workspace/papers.json"])
    assert rows[0]["citation"].startswith("Alquraishi")


def test_the_file_holds_everything_the_search_returned_not_the_shortlist():
    """The same rule the Sources panel follows — 9 of 24 was the run that prompted it."""
    dropped = ToolMessage(
        content=json.dumps(
            {
                "papers": [
                    {
                        "title": "A paper the model did not discuss",
                        "link": "https://doi.org/dropped",
                    }
                ]
            }
        ),
        tool_call_id="call_1",
        name="find_papers",
    )
    sandbox = _kept(_structured("Only this one was discussed"), [dropped])
    rows = json.loads(sandbox.written["/home/user/workspace/papers.json"])
    citations = [r["citation"] for r in rows]
    assert "Only this one was discussed" in citations
    assert "A paper the model did not discuss" in citations
    # The subagent's own ranking still leads; the recovered ones follow.
    assert citations.index("Only this one was discussed") == 0


def test_the_panel_and_the_file_cannot_disagree():
    """One definition of "every source", or a researcher comparing the two finds a contradiction."""
    from backend import paper_tools
    from backend.middleware import artifacts

    assert artifacts.paper_tools.complete_sources is paper_tools.complete_sources


def test_a_subagent_that_found_nothing_writes_no_file():
    """An empty `papers.json` in Outputs reads as "the search failed", which is a different claim."""
    assert _kept(_structured(), []).written == {}


def test_no_structured_response_is_left_alone():
    sandbox = _Sandbox()
    assert (
        asyncio.run(KeepSources(sandbox).aafter_agent({"structured_response": None}, None))
        is None
    )
    assert sandbox.written == {}


def test_a_failed_write_costs_the_file_and_not_the_turn():
    sandbox = _kept(_structured("A paper"), [], _Sandbox(error="disk full"))
    assert sandbox.written  # attempted, reported, and the turn survived


def test_a_sandbox_that_raises_costs_the_file_and_not_the_turn():
    class Broken:
        async def aget_work_dir(self):
            raise RuntimeError("sandbox is gone")

    assert (
        asyncio.run(
            KeepSources(Broken()).aafter_agent(
                {"structured_response": _structured("A paper"), "messages": []}, None
            )
        )
        is None
    )

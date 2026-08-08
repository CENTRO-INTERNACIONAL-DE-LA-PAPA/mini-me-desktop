"""The academic researcher must search before it can answer.

These drive the real middleware against a real `ModelRequest`, because the defect being fixed is
precisely a value written where nothing reads it: middleware that sets `tool_choice` while a
structured output tool is bound is discarded by `langchain/agents/factory.py`, silently. A test
that only asserted "the middleware sets tool_choice" would have passed against the broken version.
"""

from __future__ import annotations

import asyncio

import pytest
from langchain_core.messages import AIMessage, HumanMessage, ToolMessage

from backend.middleware.search_first import (
    SEARCH_TOOL,
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

"""The dataverse explorer must search, and read what it found, before it recommends anything.

`DataVerseFindings.persistent_id` is a required field described as *"Dataset DOI or persistent
identifier"*. Composed from memory it is indistinguishable from a real one, and a researcher will
paste it into a citation — which is what happened with the DOIs that started this whole thread.

These drive the real middleware against request objects that behave like LangChain's: `override`
returns a **new** object, so a gate that mutated in place would fail here rather than in
production.
"""

from __future__ import annotations

import asyncio

import pytest
from langchain_core.messages import AIMessage, HumanMessage, ToolMessage

from backend.middleware.dataverse_first import (
    FIXED_FILENAME,
    READ_TOOL,
    SEARCH_TOOL,
    FixedSearchFilename,
    SearchBeforeRecommending,
)


class _ModelRequest:
    def __init__(self, messages, response_format="DataVerseSearchResults", tool_choice=None):
        self.state = {"messages": messages}
        self.response_format = response_format
        self.tool_choice = tool_choice

    def override(self, **changes):
        return _ModelRequest(
            self.state["messages"],
            changes.get("response_format", self.response_format),
            changes.get("tool_choice", self.tool_choice),
        )


class _ToolCallRequest:
    def __init__(self, name, args):
        self.tool_call = {"name": name, "args": args, "id": "call_1"}

    def override(self, **changes):
        request = _ToolCallRequest("", {})
        request.tool_call = changes.get("tool_call", self.tool_call)
        return request


def _returned(name: str) -> ToolMessage:
    return ToolMessage(content="{}", tool_call_id="call_1", name=name)


def _ran(middleware, request):
    """The request the handler actually received."""
    seen = {}
    middleware.wrap_model_call(request, lambda r: seen.update(request=r))
    return seen["request"]


# --- the gate ---------------------------------------------------------------------------------


def test_the_first_call_is_forced_into_a_search():
    ran = _ran(SearchBeforeRecommending(), _ModelRequest([HumanMessage(content="potato yield data")]))
    assert ran.tool_choice == SEARCH_TOOL
    # The half that is easy to leave out: while a structured output tool is bound, `tool_choice`
    # never reaches the model, so naming the tool without dropping the response format is a no-op
    # with a convincing-looking test beside it.
    assert ran.response_format is None


def test_searching_alone_does_not_open_the_gate():
    """The whole reason this is two steps.

    `SearchCIPDataverse` writes to a file. A subagent that stopped here has proven it can call a
    tool and has still never seen a `persistent_id`.
    """
    ran = _ran(
        SearchBeforeRecommending(),
        _ModelRequest([HumanMessage(content="q"), _returned(SEARCH_TOOL)]),
    )
    assert ran.tool_choice == READ_TOOL
    assert ran.response_format is None


def test_once_it_has_read_the_results_the_request_is_untouched():
    """The schema must come back, or the subagent can never produce its artifacts."""
    ran = _ran(
        SearchBeforeRecommending(),
        _ModelRequest(
            [
                HumanMessage(content="q"),
                AIMessage(content="", tool_calls=[]),
                _returned(SEARCH_TOOL),
                _returned(READ_TOOL),
            ]
        ),
    )
    assert ran.tool_choice is None
    assert ran.response_format == "DataVerseSearchResults"


def test_a_search_that_found_nothing_still_opens_the_gate():
    """A failed tool must cost a recommendation, never the turn.

    The steps open on a result *existing*. A Dataverse that is down, or a query that matched
    nothing, still leaves both tool messages behind — so the subagent proceeds and says it found
    nothing, instead of being forced to search again until the recursion limit.
    """
    ran = _ran(
        SearchBeforeRecommending(),
        _ModelRequest([_returned(SEARCH_TOOL), _returned(READ_TOOL)]),
    )
    assert ran.tool_choice is None


def test_a_tool_call_the_model_made_but_never_got_back_does_not_count():
    """An `AIMessage` requesting the search is not the search returning."""
    ran = _ran(
        SearchBeforeRecommending(),
        _ModelRequest(
            [AIMessage(content="", tool_calls=[{"name": SEARCH_TOOL, "args": {}, "id": "c"}])]
        ),
    )
    assert ran.tool_choice == SEARCH_TOOL


def test_the_async_path_gates_too():
    """The server runs the graph asynchronously; a sync-only hook would do nothing there."""
    seen = {}

    async def handler(request):
        seen.update(request=request)

    asyncio.run(
        SearchBeforeRecommending().awrap_model_call(
            _ModelRequest([HumanMessage(content="q")]), handler
        )
    )
    assert seen["request"].tool_choice == SEARCH_TOOL
    assert seen["request"].response_format is None


# --- the filename -----------------------------------------------------------------------------


def _fixed(middleware, name, args):
    seen = {}
    middleware.wrap_tool_call(_ToolCallRequest(name, args), lambda r: seen.update(request=r))
    return seen["request"].tool_call["args"]


@pytest.mark.parametrize(
    ("tool", "argument"),
    [(SEARCH_TOOL, "output_filename"), (READ_TOOL, "filename")],
)
def test_the_filename_is_set_whatever_the_model_asked_for(tool, argument):
    """Including when it asked for nothing — the argument is added, not just corrected."""
    middleware = FixedSearchFilename()
    assert _fixed(middleware, tool, {"query": "potato"})[argument] == FIXED_FILENAME
    assert _fixed(middleware, tool, {argument: "results.json"})[argument] == FIXED_FILENAME


def test_the_two_tools_spell_the_argument_differently():
    """Which is the detail a model recalls wrong three steps into an episode."""
    middleware = FixedSearchFilename()
    assert "output_filename" in _fixed(middleware, SEARCH_TOOL, {})
    assert "filename" in _fixed(middleware, READ_TOOL, {})


def test_the_model_s_other_arguments_survive():
    """It sets one argument. Choosing the query is the model's job and stays that way."""
    args = _fixed(FixedSearchFilename(), SEARCH_TOOL, {"query": "sweetpotato Peru", "limit": 20})
    assert args["query"] == "sweetpotato Peru"
    assert args["limit"] == 20


def test_any_other_tool_is_left_alone():
    """`list_dataset_files` takes a persistent id, not a filename. Touching it would be a bug."""
    args = _fixed(FixedSearchFilename(), "list_dataset_files", {"persistent_id": "doi:10.1/x"})
    assert args == {"persistent_id": "doi:10.1/x"}


def test_a_call_that_was_already_right_is_passed_through_unchanged():
    """Identity, not an equal copy: the middleware must not rebuild a request it has no quarrel with."""
    middleware = FixedSearchFilename()
    request = _ToolCallRequest(SEARCH_TOOL, {"output_filename": FIXED_FILENAME})
    assert middleware._fix(request) is request


def test_the_async_path_fixes_the_filename_too():
    seen = {}

    async def handler(request):
        seen.update(request=request)

    asyncio.run(
        FixedSearchFilename().awrap_tool_call(
            _ToolCallRequest(SEARCH_TOOL, {"query": "q"}), handler
        )
    )
    assert seen["request"].tool_call["args"]["output_filename"] == FIXED_FILENAME


# --- against LangChain's real objects ----------------------------------------------------------


def test_the_gate_works_on_a_real_ModelRequest():
    """The stubs above describe `override`; this proves LangChain's own object honours it.

    Worth its own test because the bug this middleware family exists to fix is exactly a value
    written where nothing reads it. A gate verified only against a hand-written double is a gate
    verified against our belief about the framework.
    """
    from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
    from langchain.agents.middleware.types import ModelRequest

    messages = [HumanMessage(content="potato yield data")]
    request = ModelRequest(
        model=GenericFakeChatModel(messages=iter([AIMessage(content="ok")])),
        messages=messages,
        response_format="DataVerseSearchResults",
        state={"messages": messages},
    )
    gated = SearchBeforeRecommending()._gate(request)

    assert gated is not request, "override must not mutate in place"
    assert gated.tool_choice == SEARCH_TOOL
    assert gated.response_format is None
    # Untouched, because a gate that dropped the conversation would be a much louder bug.
    assert gated.messages == messages


def test_the_filename_fix_works_on_a_real_ToolCallRequest():
    from langchain.agents.middleware.types import ToolCallRequest

    request = ToolCallRequest(
        tool_call={"name": SEARCH_TOOL, "args": {"query": "potato"}, "id": "call_1"},
        tool=None,
        state={"messages": []},
        runtime=None,
    )
    fixed = FixedSearchFilename()._fix(request)

    assert fixed.tool_call["args"]["output_filename"] == FIXED_FILENAME
    assert fixed.tool_call["args"]["query"] == "potato"
    assert fixed.tool_call["id"] == "call_1", "the id must survive or the result cannot be matched"


# --- the prompt no longer says it -------------------------------------------------------------


def test_the_prompt_no_longer_asks_for_what_the_middleware_sets():
    """A rule that is enforced *and* requested teaches the next reader that prompts do this work."""
    from backend.subagents import dataverse_subagent

    prompt = dataverse_subagent["system_prompt"]
    assert FIXED_FILENAME not in prompt
    assert "Mandatory fixed filename rule" not in prompt

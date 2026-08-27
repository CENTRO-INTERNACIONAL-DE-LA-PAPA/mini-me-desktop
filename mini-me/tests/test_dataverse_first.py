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
import json
from types import SimpleNamespace

import pytest
from langchain_core.messages import AIMessage, HumanMessage, ToolMessage

from backend.middleware.dataverse_first import (
    FIXED_FILENAME,
    READ_TOOL,
    SEARCH_TOOL,
    SearchResultsFile,
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


def test_the_search_is_told_where_to_write_whatever_the_model_asked_for():
    """Including when it asked for nothing — the argument is added, not just corrected."""
    middleware = SearchResultsFile()
    assert _fixed(middleware, SEARCH_TOOL, {"query": "potato"})["output_filename"] == FIXED_FILENAME
    assert (
        _fixed(middleware, SEARCH_TOOL, {"output_filename": "results.json"})["output_filename"]
        == FIXED_FILENAME
    )


def test_the_read_takes_file_path_and_nothing_else(caplog):
    """The bug this file was rewritten for.

    Probed against the live MCP (docs §220): `read_search_results(filename=...)` answers
    *'file_path' is a required property*, and passing `filename` **beside** a correct `file_path`
    answers *Unexpected keyword argument*. The previous version injected `filename`, so every
    read failed — including the read the gate above forces, which is why the subagent could search
    and never recommend.
    """
    args = _fixed(SearchResultsFile(), READ_TOOL, {})
    assert "filename" not in args
    assert args["file_path"].endswith(FIXED_FILENAME)
    assert args["file_path"].startswith("/")


def test_a_filename_the_model_supplies_is_removed_rather_than_passed_on():
    """It is not a harmless extra: the tool rejects the whole call."""
    args = _fixed(
        SearchResultsFile(),
        READ_TOOL,
        {"file_path": "/tmp/mcp/json_files/dataverse_search.json", "filename": "x.json"},
    )
    assert args == {"file_path": "/tmp/mcp/json_files/dataverse_search.json"}


def test_the_read_looks_where_the_search_said_it_wrote():
    """Taken from the search's own answer, so a server that moves its directory is followed."""
    middleware = SearchResultsFile()
    elsewhere = "/srv/results/dataverse_search.json"
    middleware.wrap_tool_call(
        _ToolCallRequest(SEARCH_TOOL, {"query": "potato"}),
        lambda r: [
            {
                "type": "text",
                "text": json.dumps({"status": "success", "output_file": elsewhere}),
            }
        ],
    )
    assert _fixed(middleware, READ_TOOL, {})["file_path"] == elsewhere


def test_the_model_s_other_arguments_survive():
    """It sets one argument. Choosing the query is the model's job and stays that way."""
    args = _fixed(SearchResultsFile(), SEARCH_TOOL, {"query": "sweetpotato Peru", "limit": 20})
    assert args["query"] == "sweetpotato Peru"
    assert args["limit"] == 20


def test_any_other_tool_is_left_alone():
    """`list_dataset_files` takes a persistent id, not a filename. Touching it would be a bug."""
    args = _fixed(SearchResultsFile(), "list_dataset_files", {"persistent_id": "doi:10.1/x"})
    assert args == {"persistent_id": "doi:10.1/x"}


def test_a_call_that_was_already_right_is_passed_through_unchanged():
    """Identity, not an equal copy: the middleware must not rebuild a request it has no quarrel with."""
    middleware = SearchResultsFile()
    request = _ToolCallRequest(SEARCH_TOOL, {"output_filename": FIXED_FILENAME})
    assert middleware._fix(request) is request


def test_the_async_path_fixes_the_filename_too():
    seen = {}

    async def handler(request):
        seen.update(request=request)

    asyncio.run(
        SearchResultsFile().awrap_tool_call(
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
    fixed = SearchResultsFile()._fix(request)

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


# --- the copy the researcher can open ----------------------------------------------------------

class _Sandbox:
    """The two calls the copy needs, and a record of what was written."""

    def __init__(self, error=None):
        self.written: dict[str, str] = {}
        self.error = error

    async def aget_work_dir(self):
        return "/home/user/workspace"

    async def awrite(self, path, content):
        self.written[path] = content
        return SimpleNamespace(error=self.error)


def _read_returning(payload, sandbox):
    async def handler(_request):
        return [{"type": "text", "text": json.dumps(payload)}]

    return asyncio.run(
        SearchResultsFile(sandbox).awrap_tool_call(_ToolCallRequest(READ_TOOL, {}), handler)
    )


def test_what_the_read_returned_is_kept_in_the_workspace():
    """*"I want the user to have it."*

    The file the MCP wrote is at `/tmp/mcp/json_files/` on a machine at
    `dataverse-cip.fastmcp.app`, which is nobody's workspace. What comes back through the read is
    the only copy that can reach the researcher.
    """
    sandbox = _Sandbox()
    rows = [{"global_id": "doi:10.21223/P3/0F9T62", "name": "Late blight trials"}]
    _read_returning({"file_path": "/tmp/mcp/json_files/x.json", "content": rows}, sandbox)
    assert list(sandbox.written) == ["/home/user/workspace/dataverse_search.json"]
    assert json.loads(sandbox.written["/home/user/workspace/dataverse_search.json"]) == rows


def test_the_kept_copy_is_the_file_the_claims_check_reads():
    """Two middlewares, one filename. If they drift, the check reads nothing."""
    from backend.middleware.claims import DATAVERSE_SEARCH

    assert DATAVERSE_SEARCH == FIXED_FILENAME


def test_a_failed_write_costs_the_copy_and_not_the_search():
    """A convenience must not take down the tool call that produced it."""
    sandbox = _Sandbox(error="disk full")
    result = _read_returning({"content": [{"global_id": "doi:1/x"}]}, sandbox)
    assert result  # the read's own answer still reaches the model


def test_a_read_that_answered_nothing_parseable_writes_nothing():
    """An error page is not a search result, and must not be filed as one."""
    sandbox = _Sandbox()

    async def handler(_request):
        return [{"type": "text", "text": "<html>gateway timeout</html>"}]

    asyncio.run(
        SearchResultsFile(sandbox).awrap_tool_call(_ToolCallRequest(READ_TOOL, {}), handler)
    )
    assert sandbox.written == {}


def test_without_a_sandbox_the_middleware_still_fixes_the_arguments():
    """`_build_runtime_subagents` passes one; a test or a caller that does not must not crash."""
    assert _fixed(SearchResultsFile(), READ_TOOL, {})["file_path"].endswith(FIXED_FILENAME)


def test_the_search_path_is_read_out_of_a_real_ToolMessage():
    """The shape the handler actually returns, which the first version of this could not read.

    `wrap_tool_call`'s handler is typed `ToolMessage | Command`
    (`langchain/agents/middleware/types.py:652`) — so the end-to-end check that called the MCP
    tool directly proved the *tool's* contract and never the *middleware's*. With the wrapper
    unread, `output_file` was never captured and the workspace copy was never written.
    """
    from langchain_core.messages import ToolMessage

    middleware = SearchResultsFile()
    elsewhere = "/srv/results/dataverse_search.json"
    middleware.wrap_tool_call(
        _ToolCallRequest(SEARCH_TOOL, {"query": "potato"}),
        lambda _r: ToolMessage(
            content=json.dumps({"status": "success", "output_file": elsewhere}),
            tool_call_id="call_1",
            name=SEARCH_TOOL,
        ),
    )
    assert middleware._server_path == elsewhere
    assert _fixed(middleware, READ_TOOL, {})["file_path"] == elsewhere


def test_a_ToolMessage_carrying_content_blocks_is_read_too():
    """MCP tools answer with blocks as often as with a bare string."""
    from langchain_core.messages import ToolMessage

    middleware = SearchResultsFile()
    middleware.wrap_tool_call(
        _ToolCallRequest(SEARCH_TOOL, {"query": "potato"}),
        lambda _r: ToolMessage(
            content=[
                {"type": "text", "text": json.dumps({"output_file": "/blocks/x.json"})}
            ],
            tool_call_id="call_1",
            name=SEARCH_TOOL,
        ),
    )
    assert middleware._server_path == "/blocks/x.json"


def test_the_copy_is_written_from_a_real_ToolMessage():
    """The half that was silently doing nothing: `papers.json`'s Dataverse twin."""
    from langchain_core.messages import ToolMessage

    sandbox = _Sandbox()
    rows = [{"global_id": "doi:10.21223/P3/0F9T62", "name": "Late blight trials"}]

    async def handler(_request):
        return ToolMessage(
            content=json.dumps({"file_path": "/tmp/mcp/json_files/x.json", "content": rows}),
            tool_call_id="call_1",
            name=READ_TOOL,
        )

    asyncio.run(
        SearchResultsFile(sandbox).awrap_tool_call(_ToolCallRequest(READ_TOOL, {}), handler)
    )
    assert json.loads(sandbox.written["/home/user/workspace/dataverse_search.json"]) == rows


def test_a_Command_wrapping_the_answer_is_read_as_well():
    """`wrap_tool_call` may hand back a Command instead; its messages carry the same text."""
    from langchain_core.messages import ToolMessage
    from langgraph.types import Command

    middleware = SearchResultsFile()
    middleware.wrap_tool_call(
        _ToolCallRequest(SEARCH_TOOL, {"query": "potato"}),
        lambda _r: Command(
            update={
                "messages": [
                    ToolMessage(
                        content=json.dumps({"output_file": "/cmd/x.json"}),
                        tool_call_id="call_1",
                        name=SEARCH_TOOL,
                    )
                ]
            }
        ),
    )
    assert middleware._server_path == "/cmd/x.json"


# --- the copy is the turn's, not the last search's ---------------------------------------------

def _reads_through(middleware, *payloads):
    """Several reads through **one** middleware, which is what a turn actually does.

    `_read_returning` builds a fresh `SearchResultsFile` each time, so nothing it does could ever
    show accumulation — the helper agreed with the bug.
    """
    for payload in payloads:
        async def handler(_request, _payload=payload):
            return [{"type": "text", "text": json.dumps(_payload)}]

        asyncio.run(middleware.awrap_tool_call(_ToolCallRequest(READ_TOOL, {}), handler))


def test_a_dataset_found_early_survives_a_later_search():
    """**The false accusation, stated as a test.**

    A real turn ran forty-six steps and several searches. The explorer recommended
    `doi:10.21223/J9NLVP` — real, published, with real authors — found early; the workspace copy
    was then overwritten by a narrower search; and `claims.py` reported it *absent from
    dataverse_search.json*. The recommendation was sound and the record cried wolf (§286).
    """
    sandbox = _Sandbox()
    middleware = SearchResultsFile(sandbox)
    early = {"global_id": "doi:10.21223/J9NLVP", "name": "Three new potato varieties"}
    late = {"global_id": "doi:10.21223/OTHER", "name": "Something narrower"}

    _reads_through(middleware, {"content": [early]}, {"content": [late]})

    kept = json.loads(sandbox.written["/home/user/workspace/dataverse_search.json"])
    assert early in kept, "a recommendation made forty steps ago must still be checkable"
    assert late in kept
    assert kept == [early, late], "and in the order the searches produced them"


def test_the_same_record_twice_is_one_record():
    """Searches overlap constantly — `late blight` and `potato late blight` return the same rows."""
    sandbox = _Sandbox()
    middleware = SearchResultsFile(sandbox)
    row = {"global_id": "doi:1/x", "name": "Trials"}

    _reads_through(middleware, {"content": [row]}, {"content": [row, {"global_id": "doi:1/y"}]})

    kept = json.loads(sandbox.written["/home/user/workspace/dataverse_search.json"])
    assert len(kept) == 2, f"deduplicated on the whole record, not appended blindly: {kept}"


def test_a_read_that_answered_one_object_is_kept_too():
    """`content` is not always a list, and a single record is still a record."""
    sandbox = _Sandbox()
    middleware = SearchResultsFile(sandbox)
    _reads_through(middleware, {"content": {"global_id": "doi:1/only"}})
    kept = json.loads(sandbox.written["/home/user/workspace/dataverse_search.json"])
    assert kept == [{"global_id": "doi:1/only"}]


def test_the_file_stops_growing_rather_than_filling_a_disk():
    """A model that loops on searches must not write until the machine stops."""
    from backend.middleware.dataverse_first import MAX_KEPT_RECORDS

    sandbox = _Sandbox()
    middleware = SearchResultsFile(sandbox)
    _reads_through(
        middleware,
        {"content": [{"global_id": f"doi:1/{n}"} for n in range(MAX_KEPT_RECORDS + 50)]},
    )
    kept = json.loads(sandbox.written["/home/user/workspace/dataverse_search.json"])
    assert len(kept) == MAX_KEPT_RECORDS


def test_each_turn_starts_from_an_empty_slate():
    """Per turn, because that is the scope `ClaimsRecorder` compares at.

    Carrying records across turns would check a recommendation against searches nobody made today,
    and grow without bound.
    """
    sandbox = _Sandbox()
    _reads_through(SearchResultsFile(sandbox), {"content": [{"global_id": "doi:1/yesterday"}]})
    _reads_through(SearchResultsFile(sandbox), {"content": [{"global_id": "doi:1/today"}]})
    kept = json.loads(sandbox.written["/home/user/workspace/dataverse_search.json"])
    assert kept == [{"global_id": "doi:1/today"}]

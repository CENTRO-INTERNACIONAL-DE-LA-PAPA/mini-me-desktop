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
import os
from pathlib import Path
from types import SimpleNamespace

from deepagents.backends.protocol import FileData, ReadResult

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
    """The three calls the copy needs, and a record of what was written.

    `aread` answers with the **real** `ReadResult`/`FileData` types rather than a friendlier
    stand-in: `FileData` is a TypedDict, so `file_data` is a plain dict and `.content` on it
    raises. A permissive double is what let the claims check fail on every turn for two days
    (§221/§224).
    """

    def __init__(self, error=None, files=None, read_error=None):
        self.written: dict[str, str] = {}
        self.error = error
        self.files: dict[str, str] = dict(files or {})
        self.read_error = read_error
        self.reads: list[str] = []

    async def aget_work_dir(self):
        return "/home/user/workspace"

    async def awrite(self, path, content):
        self.written[path] = content
        return SimpleNamespace(error=self.error)

    async def aread(self, file_path, offset=0, limit=2000):
        self.reads.append(file_path)
        if self.read_error:
            return ReadResult(error=self.read_error, file_data=None)
        text = self.files.get(file_path)
        if text is None:
            return ReadResult(error="not found", file_data=None)
        return ReadResult(error=None, file_data=FileData(content=text, encoding="utf-8"))


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
    assert sorted(sandbox.written) == [
        "/home/user/workspace/.mini-me/dataverse_search.meta.json",
        "/home/user/workspace/dataverse_search.json",
    ], "the records, and what the searches said they found (§300)"

    # **Normalised, not verbatim** — this file is what the datasets panel renders now, so it wears
    # the app's shape rather than the MCP's (§290). The original rides along under `raw`.
    kept = json.loads(sandbox.written["/home/user/workspace/dataverse_search.json"])
    assert kept[0]["persistent_id"] == "doi:10.21223/P3/0F9T62"
    assert kept[0]["title"] == "Late blight trials"
    assert kept[0]["raw"] == rows[0], "nothing the mapping missed is lost"


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
    assert _kept_ids(sandbox) == ["doi:10.21223/P3/0F9T62"]


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

def _totals(sandbox) -> dict:
    """What the searches reported about themselves, as the panel reads it."""
    return json.loads(
        sandbox.written["/home/user/workspace/.mini-me/dataverse_search.meta.json"]
    )


def _kept_ids(sandbox) -> list[str]:
    """The persistent ids in the file, which is what every one of these tests is really about."""
    kept = json.loads(sandbox.written["/home/user/workspace/dataverse_search.json"])
    return [row["persistent_id"] for row in kept]


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

    assert _kept_ids(sandbox) == ["doi:10.21223/J9NLVP", "doi:10.21223/OTHER"], (
        "a dataset found forty steps ago must still be here, in the order the searches found it"
    )


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
    assert _kept_ids(sandbox) == ["doi:1/only"]


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
    assert _kept_ids(sandbox) == ["doi:1/today"]


# --- the shape the app renders ------------------------------------------------------------------

def test_the_search_apis_own_answer_becomes_a_row():
    """Dataverse's documented search shape, mapped whole."""
    from backend.middleware.dataverse_first import normalise

    row = normalise(
        {
            "name": "Three new healthy and sustainable potato varieties",
            "type": "dataset",
            "url": "https://data.cipotato.org/dataset.xhtml?persistentId=doi:10.21223/P3/HJLUJZ",
            "global_id": "doi:10.21223/P3/HJLUJZ",
            "description": "Late blight assessed under high disease pressure.",
            "authors": ["Perez, Willmer", "Gastelo, Manuel"],
            "fileCount": 3,
            "name_of_dataverse": "CIP Potato Breeding",
        }
    )
    assert row["persistent_id"] == "doi:10.21223/P3/HJLUJZ"
    assert row["title"].startswith("Three new healthy")
    assert row["authors"] == ["Perez, Willmer", "Gastelo, Manuel"]
    assert row["file_count"] == 3
    assert row["repository"] == "CIP Potato Breeding"
    assert row["link"].endswith("HJLUJZ")


def test_a_record_that_splits_its_identifier_is_put_back_together():
    """Dataverse's native form has no joined id anywhere, which is §288's whole difficulty."""
    from backend.middleware.dataverse_first import normalise

    row = normalise({"protocol": "doi", "authority": "10.21223", "identifier": "P3/HKABUV"})
    assert row["persistent_id"] == "doi:10.21223/P3/HKABUV"


def test_a_layout_nobody_has_met_yields_a_row_rather_than_an_exception():
    """**The rule `_ids_in` set, applied here.**

    A reader that insisted on one key would report an empty search the day the MCP renamed a
    field. An empty row is visible and wrong; an exception loses the whole search.
    """
    from backend.middleware.dataverse_first import normalise

    row = normalise({"somethingNew": "x"})
    assert row["persistent_id"] == "" and row["title"] == ""
    assert row["raw"] == {"somethingNew": "x"}, "and the unmapped record survives whole"

    assert normalise("not a record")["title"] == "not a record"
    assert normalise(None)["persistent_id"] == ""


def test_a_file_count_that_is_not_a_count_is_not_one():
    from backend.middleware.dataverse_first import normalise

    assert normalise({"fileCount": "3"})["file_count"] == 3
    assert normalise({"fileCount": "many"})["file_count"] is None
    assert normalise({"fileCount": True})["file_count"] is None, "a flag is not a count"
    assert normalise({})["file_count"] is None


def test_the_claims_check_can_still_find_an_id_under_a_name_we_never_mapped():
    """`raw` is not sentiment. `unsearched` walks the leaves of this file."""
    from backend.middleware.claims import unsearched
    from backend.middleware.dataverse_first import normalise

    kept = json.dumps([normalise({"someFutureKey": "doi:10.21223/P3/ODDITY"})])
    assert unsearched(["doi:10.21223/P3/ODDITY"], kept) == []


#: The rows as the app must read them, written from this module's own code.
DATASET_FIXTURE = (
    Path(__file__).resolve().parent.parent.parent
    / "crates" / "app" / "tests" / "fixtures" / "dataverse-search.json"
)


def _dataset_sample() -> list[dict]:
    """Four rows covering every branch the client can render wrong."""
    from backend.middleware.dataverse_first import normalise

    return [
        normalise(
            {
                "name": "Three new healthy and sustainable potato varieties",
                "url": "https://data.cipotato.org/dataset.xhtml?persistentId=doi:10.21223/P3/HJLUJZ",
                "global_id": "doi:10.21223/P3/HJLUJZ",
                "description": "Late blight assessed under high disease pressure at Oxapampa.",
                "authors": ["Perez, Willmer", "Gastelo, Manuel"],
                "fileCount": 3,
                "name_of_dataverse": "CIP Potato Breeding",
            }
        ),
        # Everything optional missing: the row still has to render.
        normalise({"global_id": "doi:10.21223/P3/3AIN78", "name": "Yield trials, Comas"}),
        # The split form, which carries no joined id of its own.
        normalise({"protocol": "doi", "authority": "10.21223", "identifier": "P3/CKYEB5"}),
        # A layout nobody has met — an empty row rather than a lost search.
        normalise({"somethingNew": "x"}),
    ]


def test_the_committed_dataset_fixture_matches_what_this_module_writes():
    """Regenerate with `MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_dataverse_first.py`."""
    generated = json.dumps(_dataset_sample(), indent=2, ensure_ascii=False, sort_keys=True) + "\n"
    if os.environ.get("MINIME_WRITE_CONTRACT"):
        DATASET_FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        DATASET_FIXTURE.write_text(generated, encoding="utf-8")
        pytest.skip("fixture regenerated; read the diff")

    assert DATASET_FIXTURE.exists(), f"{DATASET_FIXTURE} is missing — MINIME_WRITE_CONTRACT=1"
    assert DATASET_FIXTURE.read_text(encoding="utf-8") == generated, (
        "the dataset row changed shape. Regenerate with "
        "`MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_dataverse_first.py`, then decide "
        "whether the app should read the new field."
    )


# --- an answer too big to hand to the model ------------------------------------------------------

SAVED = "/workspace/mcp_results/read_search_results_20260828_150525.txt"


def _pointer_text(size_kb: int = 306) -> str:
    """What `mcp_tools._save_mcp_to_sandbox` hands the model instead of a large answer."""
    return (
        f"Full result ({size_kb} KB) saved to `{SAVED}`.\n"
        "Use code execution to read specific sections, e.g.:\n"
        f"  with open('{SAVED}') as f: print(f.read())\n\n"
        "Preview (first 2 KB):\n---\n"
        '{"file_path":"/tmp/mcp/json_files/dataverse_search.json","content":[{"name":"Replic'
    )


def _hundred() -> str:
    rows = [
        {"name": f"Dataset {n}", "global_id": f"doi:10.21223/P3/ROW{n:03d}"} for n in range(100)
    ]
    return json.dumps({"file_path": "/tmp/mcp/json_files/dataverse_search.json", "content": rows})


def _answering(middleware, sandbox, content, artifact=None):
    async def handler(_request):
        message = ToolMessage(content=content, tool_call_id="call_1", name=READ_TOOL)
        if artifact is not None:
            message.artifact = artifact
        return message

    return asyncio.run(
        middleware.awrap_tool_call(_ToolCallRequest(READ_TOOL, {}), handler)
    )


def test_an_answer_too_big_for_the_model_is_followed_to_the_file():
    """**The hundred datasets that became one.**

    `mcp_tools` caps a tool answer at 128 KB. A 314 KB search is written to the workspace and the
    model gets a pointer with a 2 KB preview — prose, so `json.loads` failed, `_payload` answered
    None and `_keep` returned in silence. The panel said *1 dataset found* against an answer
    holding a hundred (§291).
    """
    sandbox = _Sandbox(files={SAVED: _hundred()})
    middleware = SearchResultsFile(sandbox)
    _answering(middleware, sandbox, _pointer_text(), artifact={"saved_path": SAVED})

    kept = _kept_ids(sandbox)
    assert len(kept) == 100, f"kept {len(kept)} of the hundred that were searched"
    assert kept[0] == "doi:10.21223/P3/ROW000"
    assert sandbox.reads == [SAVED], "read back through the backend that wrote it"


def test_the_pointer_is_found_in_the_sentence_when_the_artifact_is_gone():
    """A wrapper that drops the artifact must not cost the search."""
    sandbox = _Sandbox(files={SAVED: _hundred()})
    _answering(SearchResultsFile(sandbox), sandbox, _pointer_text(), artifact=None)
    assert len(_kept_ids(sandbox)) == 100


def test_an_answer_capped_inline_says_so_rather_than_nothing(caplog):
    """No pointer, because nothing was saved. The only honest move is to say it."""
    sandbox = _Sandbox()
    capped = '{"content":[{"global_id":"doi:1/a"}' + "\n\n...[output truncated — 183 KB elided]..."
    with caplog.at_level("WARNING", logger="backend.middleware.dataverse_first"):
        _answering(SearchResultsFile(sandbox), sandbox, capped)
    assert not sandbox.written, "nothing recoverable, so nothing filed"
    assert any("capped inline" in record.getMessage() for record in caplog.records)


def test_an_answer_with_neither_json_nor_a_pointer_says_which(caplog):
    sandbox = _Sandbox()
    with caplog.at_level("WARNING", logger="backend.middleware.dataverse_first"):
        _answering(SearchResultsFile(sandbox), sandbox, "<html>gateway timeout</html>")
    assert any("no pointer" in record.getMessage() for record in caplog.records)


def test_a_pointer_to_a_file_that_will_not_read_costs_the_copy_and_not_the_search(caplog):
    sandbox = _Sandbox(read_error="permission denied")
    with caplog.at_level("WARNING", logger="backend.middleware.dataverse_first"):
        result = _answering(
            SearchResultsFile(sandbox), sandbox, _pointer_text(), artifact={"saved_path": SAVED}
        )
    assert result, "the read's own answer still reaches the model"
    assert any("permission denied" in record.getMessage() for record in caplog.records)


def test_a_small_answer_still_takes_the_direct_path():
    """The pointer is a fallback, not a detour: nothing extra is read for an ordinary answer."""
    sandbox = _Sandbox()
    _answering(
        SearchResultsFile(sandbox),
        sandbox,
        json.dumps({"content": [{"global_id": "doi:1/small"}]}),
    )
    assert _kept_ids(sandbox) == ["doi:1/small"]
    assert sandbox.reads == [], "no file to follow, so none was read"


# --- JSON, then a sentence -----------------------------------------------------------------------

def _trimmed_like_upstream(kept: int, dropped: int) -> str:
    """What `mcp_tools._trim_json_array_text` sends when a result crosses the 128 KB cap.

    Valid JSON, then prose. Reproduced here from that function rather than imagined: it returns
    `json.dumps(result_obj, indent=2) + suffix`, and the suffix is what `json.loads` chokes on.
    """
    rows = [{"name": f"Dataset {n}", "global_id": f"doi:10.21223/P3/ROW{n:03d}"} for n in range(kept)]
    return json.dumps({"content": rows}, indent=2) + (
        f"\n\n[{dropped} item(s) omitted — output exceeded 124 KB. "
        "Use a lower limit or a more specific query for 'read_search_results'.]"
    )


def test_a_trimmed_answer_keeps_the_datasets_that_survived_the_trim():
    """**Forty datasets, or zero.**

    A search returning a hundred is trimmed to whatever fits in 128 KB and the rest is announced
    in a sentence appended after the JSON. `json.loads` rejects the whole string, so the file got
    nothing — the same outcome as a search that failed, which is how it read for three releases
    (§292).
    """
    sandbox = _Sandbox()
    _answering(SearchResultsFile(sandbox), sandbox, _trimmed_like_upstream(kept=40, dropped=60))

    kept = _kept_ids(sandbox)
    assert len(kept) == 40, f"kept {len(kept)} of the forty that survived the trim"
    assert kept[0] == "doi:10.21223/P3/ROW000"


def test_a_trimmed_bare_array_is_a_search_result_too():
    """`_trim_json_array_text` rebuilds `{wrap_key: kept}` only when the original had one."""
    sandbox = _Sandbox()
    body = json.dumps([{"global_id": "doi:1/a"}, {"global_id": "doi:1/b"}], indent=2)
    _answering(SearchResultsFile(sandbox), sandbox, body + "\n\n[8 item(s) omitted — …]")
    assert _kept_ids(sandbox) == ["doi:1/a", "doi:1/b"]


def test_prose_before_the_json_is_still_not_json():
    """`raw_decode` parses from the start, so a pointer sentence is not mistaken for a payload.

    That case has its own path — following the address — and conflating the two would file a 2 KB
    preview as if it were the whole search.
    """
    assert SearchResultsFile._leading_json("Full result (306 KB) saved to `/x.txt`. {\"a\": 1}") is None
    assert SearchResultsFile._leading_json("") is None
    assert SearchResultsFile._leading_json("   {\"a\": 1}  trailing") == {"a": 1}


def test_an_unusable_answer_says_what_it_actually_was(caplog):
    """"no JSON in the answer" was true and cost a release, because it does not say *what*."""
    sandbox = _Sandbox()
    with caplog.at_level("WARNING", logger="backend.middleware.dataverse_first"):
        _answering(SearchResultsFile(sandbox), sandbox, "<html>502 Bad Gateway</html>")
    said = "\n".join(record.getMessage() for record in caplog.records)
    assert "the answer began: <html>502 Bad Gateway" in said


# --- a saved file is not one document -------------------------------------------------------------

def test_a_saved_answer_written_as_several_blocks_files_all_of_them():
    """**`_mcp_result_to_text` joins content blocks with `\\n---\\n`.**

    Two blocks is two valid JSON documents with a delimiter between them, so `json.loads` on the
    file fails with *Extra data* and discards both — §292's defect wearing a different separator,
    inside the recovery path §291 added for it.
    """
    first = json.dumps({"content": [{"global_id": "doi:1/a"}, {"global_id": "doi:1/b"}]})
    second = json.dumps({"content": [{"global_id": "doi:1/c"}]})
    sandbox = _Sandbox(files={SAVED: f"{first}\n---\n{second}"})

    _answering(SearchResultsFile(sandbox), sandbox, _pointer_text(), artifact={"saved_path": SAVED})
    assert _kept_ids(sandbox) == ["doi:1/a", "doi:1/b", "doi:1/c"]


def test_one_unreadable_block_costs_that_block_and_not_the_file(caplog):
    """Losing one section of three is a different fact from losing all three, and is said."""
    good = json.dumps({"content": [{"global_id": "doi:1/kept"}]})
    sandbox = _Sandbox(files={SAVED: f"{good}\n---\n<html>502</html>"})

    with caplog.at_level("WARNING", logger="backend.middleware.dataverse_first"):
        _answering(
            SearchResultsFile(sandbox), sandbox, _pointer_text(), artifact={"saved_path": SAVED}
        )
    assert _kept_ids(sandbox) == ["doi:1/kept"]
    assert any("1 of 2 section(s)" in record.getMessage() for record in caplog.records)


def test_a_saved_answer_with_nothing_readable_says_so(caplog):
    sandbox = _Sandbox(files={SAVED: "<html>502 Bad Gateway</html>"})
    with caplog.at_level("WARNING", logger="backend.middleware.dataverse_first"):
        _answering(
            SearchResultsFile(sandbox), sandbox, _pointer_text(), artifact={"saved_path": SAVED}
        )
    assert not sandbox.written
    assert any("held no records" in record.getMessage() for record in caplog.records)


def test_a_saved_block_under_another_key_is_still_records():
    """`_trim_json_array_text` names the list after whatever key the tool used."""
    sandbox = _Sandbox(files={SAVED: json.dumps({"data": [{"global_id": "doi:1/x"}]})})
    _answering(SearchResultsFile(sandbox), sandbox, _pointer_text(), artifact={"saved_path": SAVED})
    assert _kept_ids(sandbox) == ["doi:1/x"]


# --- the whole answer, not the model's share ------------------------------------------------------

@pytest.fixture
def whole_answer():
    """Set what `mcp_tools` kept aside, and clear it after — a ContextVar leaks between tests."""
    from backend import mcp_tools

    tokens = []

    def keep(tool_name, text):
        tokens.append(mcp_tools._full_answer.set((tool_name, text)))

    yield keep
    for token in reversed(tokens):
        mcp_tools._full_answer.reset(token)


def _trimmed_and_whole(kept: int, total: int) -> tuple[str, str]:
    rows = [{"name": f"Dataset {n}", "global_id": f"doi:10.21223/P3/ROW{n:03d}"} for n in range(total)]
    trimmed = json.dumps({"content": rows[:kept]}, indent=2) + (
        f"\n\n[{total - kept} item(s) omitted — output exceeded 124 KB.]"
    )
    return trimmed, json.dumps({"content": rows})


def test_the_file_gets_the_hundred_the_model_was_spared(whole_answer):
    """**The model's budget is not the researcher's.**

    A hundred datasets, trimmed to forty for the context window. The model should see forty and a
    sentence telling it to narrow; the conversation folder has no reason to inherit that limit
    (§294).
    """
    trimmed, whole = _trimmed_and_whole(kept=40, total=100)
    whole_answer(READ_TOOL, whole)
    sandbox = _Sandbox()

    _answering(SearchResultsFile(sandbox), sandbox, trimmed)
    assert len(_kept_ids(sandbox)) == 100, "the file takes the whole answer, not the model's share"


def test_without_a_kept_answer_nothing_changes(whole_answer):
    """Under the cap there is nothing to keep aside, and the result is already whole."""
    sandbox = _Sandbox()
    _answering(
        SearchResultsFile(sandbox), sandbox, json.dumps({"content": [{"global_id": "doi:1/small"}]})
    )
    assert _kept_ids(sandbox) == ["doi:1/small"]


def test_a_big_answer_from_another_tool_is_not_mistaken_for_this_one(whole_answer):
    """The ContextVar holds one answer. Reading it blind would file a paper search as datasets."""
    trimmed, whole = _trimmed_and_whole(kept=2, total=50)
    whole_answer("snippet_search", whole)
    sandbox = _Sandbox()

    _answering(SearchResultsFile(sandbox), sandbox, trimmed)
    assert len(_kept_ids(sandbox)) == 2, "the trimmed dataverse answer, not asta's fifty"


def test_the_kept_answer_answers_for_its_own_tool_and_no_other(whole_answer):
    """`last_full_answer` is the whole contract between the two files, so test it directly.

    A source-inspection test stood here first and could pass while asserting nothing, which is a
    worse thing to own than no test at all.
    """
    from backend import mcp_tools

    assert mcp_tools.last_full_answer(READ_TOOL) is None, "nothing capped yet"

    whole_answer(READ_TOOL, '{"content": []}')
    assert mcp_tools.last_full_answer(READ_TOOL) == '{"content": []}'
    assert mcp_tools.last_full_answer("snippet_search") is None, "one answer, and it is named"


# --- what the skill teaches --------------------------------------------------------------------

def _skill(name: str) -> str:
    return (
        Path(__file__).resolve().parent.parent / "skills" / "dataverse" / "references" / name
    ).read_text(encoding="utf-8")


def test_the_identifier_is_the_one_field_with_a_documented_source():
    """**The one field that must be copied had nowhere documented to copy it from.**

    `metadata_extraction_rules.md` mapped every core field to its Dataverse source — `title` to
    `title`, `authors` to `author -> authorName` — and listed the persistent id only under
    "required summary fields", with no source named. So the model was told to *produce* the one
    string a researcher pastes into a paper, and given a source for everything else (§299).
    """
    rules = _skill("metadata_extraction_rules.md")
    assert "`persistent_id`" in rules, "the id must be a core field with a source"
    assert "global_id" in rules, "and the source must be named"
    assert "copy it, never compose it" in rules.lower()
    # And the honest fallback, which the other subagents' prompts already carry: omit rather than
    # guess. A reconstructed DOI is indistinguishable from a read one.
    assert "omit the dataset" in rules.lower()


def test_the_skill_no_longer_teaches_an_argument_that_does_not_exist():
    """`read_search_results` takes `file_path`. `filename` is a hard error (§220).

    The middleware strips it, so the stale instruction cost nothing but the model's attention —
    which is not nothing, and is exactly what a skill file spends.
    """
    workflow = _skill("discovery_workflow.md")
    assert 'filename="dataverse_search.json"' not in workflow or "does not exist" in workflow, (
        "if the old argument is still named, it must be named as the mistake it was"
    )
    assert "Call it with no arguments" in workflow


def test_the_skill_no_longer_claims_successive_searches_overwrite():
    """`_accumulate` keeps every search this turn (§286). The doc said the opposite."""
    workflow = _skill("discovery_workflow.md")
    assert "simply\n     overwrite this file" not in workflow
    assert "every result from every search this turn is kept" in " ".join(workflow.split())


def test_the_schema_field_names_where_the_identifier_comes_from():
    """`Field(description=...)` is what the model reads when it fills the field in.

    It said *"Dataset DOI or persistent identifier."* — a description of what the value **is**,
    with no word about where to get it, while every sibling field read the same declarative way.
    So the id was one more thing to author (§299).
    """
    from backend.schemas import DataVerseFindings

    described = DataVerseFindings.model_fields["persistent_id"].description or ""
    assert "global_id" in described, "the source field has to be named where it is filled in"
    assert "copied verbatim" in described
    assert "omit the dataset" in described, "and the honest fallback has to be there too"


# --- of how many? -------------------------------------------------------------------------------

def _search_answering(middleware, sandbox, payload):
    """One `SearchCIPDataverse` call through the middleware, answering `payload`."""

    async def handler(_request):
        return ToolMessage(
            content=json.dumps(payload), tool_call_id="call_s", name=SEARCH_TOOL
        )

    return asyncio.run(
        middleware.awrap_tool_call(_ToolCallRequest(SEARCH_TOOL, {}), handler)
    )


def test_the_denominator_survives_to_the_panel():
    """**"Found 4,000, showing 29" and "found 29" were the same answer at every layer.**

    The MCP read `total_count` to decide when to stop paging and never returned it (§299). Now it
    does, and a researcher reading twenty-nine rows can see whether that is the corpus or a
    sliver of it.
    """
    sandbox = _Sandbox()
    middleware = SearchResultsFile(sandbox)
    _search_answering(
        middleware,
        sandbox,
        {"output_file": "/tmp/mcp/json_files/x.json", "total_count": 4000, "item_count": 29,
         "complete": False},
    )
    _reads_through(middleware, {"content": [{"global_id": f"doi:1/{n}"} for n in range(29)]})

    totals = _totals(sandbox)
    assert totals["total_count"] == 4000
    assert totals["kept"] == 29
    assert totals["complete"] is False


def test_a_whole_small_corpus_says_it_is_whole():
    """The reassuring case has to be distinguishable, or the warning above means nothing."""
    sandbox = _Sandbox()
    middleware = SearchResultsFile(sandbox)
    _search_answering(
        middleware, sandbox,
        {"output_file": "/x.json", "total_count": 3, "item_count": 3, "complete": True},
    )
    _reads_through(middleware, {"content": [{"global_id": f"doi:1/{n}"} for n in range(3)]})

    assert _totals(sandbox) == {"total_count": 3, "kept": 3, "complete": True}


def test_one_partial_search_makes_the_turn_partial():
    """A broad search that was cut short is not redeemed by a narrow one that finished.

    The file holds both, so what it holds is incomplete — and the second search's `complete: true`
    must not overwrite that.
    """
    sandbox = _Sandbox()
    middleware = SearchResultsFile(sandbox)
    _search_answering(
        middleware, sandbox,
        {"output_file": "/x.json", "total_count": 900, "item_count": 100, "complete": False},
    )
    _search_answering(
        middleware, sandbox,
        {"output_file": "/x.json", "total_count": 4, "item_count": 4, "complete": True},
    )
    _reads_through(middleware, {"content": [{"global_id": "doi:1/a"}]})

    totals = _totals(sandbox)
    assert totals["total_count"] == 900, "the larger denominator is the one that was established"
    assert totals["complete"] is False


def test_an_mcp_that_cannot_say_how_many_is_not_read_as_zero():
    """A deployment predating §299 answers without `total_count`.

    "We cannot tell you how many matched" is a different fact from "none matched", and the panel
    has to be able to tell them apart rather than showing `29 of 0`.
    """
    sandbox = _Sandbox()
    middleware = SearchResultsFile(sandbox)
    _search_answering(middleware, sandbox, {"output_file": "/x.json", "item_count": 29})
    _reads_through(middleware, {"content": [{"global_id": "doi:1/a"}]})

    totals = _totals(sandbox)
    assert totals["total_count"] == 0
    assert totals["complete"] is False, "unknown is not complete"

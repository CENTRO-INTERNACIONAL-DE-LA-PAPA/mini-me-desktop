"""Hosted MCP outages degrade capabilities instead of taking down the graph."""

from __future__ import annotations

import asyncio
import json
from types import SimpleNamespace
from typing import TypedDict

from fastmcp import Context, FastMCP
from mcp.types import ElicitRequest, ElicitRequestFormParams, InputRequiredResult

import backend.mcp_tools as mcp_tools
from backend.routes.mcp import get_mcp_status
from backend.runtime import (
    _mcp_clients,
    _mcp_statuses,
    _mcp_tools_cache,
    _mcp_tools_locks,
)


def _run(coro):
    return asyncio.run(coro)


def _clear_mcp_state() -> None:
    _mcp_clients.clear()
    _mcp_statuses.clear()
    _mcp_tools_cache.clear()
    _mcp_tools_locks.clear()


def test_a_failed_host_returns_no_tools_and_records_the_outage(monkeypatch) -> None:
    class BrokenAdapter:
        async def list_tools(self, *, cache_mode):
            assert cache_mode == "use"
            raise RuntimeError("401 Unauthorized: secret deployment detail")

    _clear_mcp_state()
    monkeypatch.setattr(
        mcp_tools, "_get_or_create_mcp_client", lambda _names: BrokenAdapter()
    )

    assert _run(mcp_tools.get_mcp_tools(("dataverse",))) == []
    assert mcp_tools.mcp_status_report()["services"][-1] == {
        "id": "dataverse",
        "name": "CIP Dataverse",
        "status": "unavailable",
    }
    # The failed handshake is cached: graph reconstruction does not hammer the deployment.
    assert _run(mcp_tools.get_mcp_tools(("dataverse",))) == []


def test_a_host_with_no_tools_is_not_reported_as_available(monkeypatch) -> None:
    class EmptyAdapter:
        async def list_tools(self, *, cache_mode):
            assert cache_mode == "use"
            return []

    _clear_mcp_state()
    monkeypatch.setattr(
        mcp_tools, "_get_or_create_mcp_client", lambda _names: EmptyAdapter()
    )

    assert _run(mcp_tools.get_mcp_tools(("asta",))) == []
    assert _mcp_statuses["asta"] is False


def test_paired_cleaning_services_fail_independently(monkeypatch) -> None:
    crop_tool = SimpleNamespace(name="crop_lookup", coroutine=None)

    class Adapter:
        def __init__(self, server: str):
            self.server = server

        async def list_tools(self, *, cache_mode):
            assert cache_mode == "use"
            if self.server == "agrovoc":
                raise OSError("host is down")
            return [crop_tool]

    _clear_mcp_state()
    monkeypatch.setattr(
        mcp_tools,
        "_get_or_create_mcp_client",
        lambda names: Adapter(tuple(names)[0]),
    )

    assert _run(mcp_tools.get_data_cleaning_mcp_tools()) == [crop_tool]
    states = {row["id"]: row["status"] for row in mcp_tools.mcp_status_report()["services"]}
    assert states["agrovoc"] == "unavailable"
    assert states["crop_ontology"] == "available"


def test_an_incompatible_dataverse_manifest_is_non_fatal(monkeypatch) -> None:
    _clear_mcp_state()

    async def incomplete(_names):
        _mcp_statuses["dataverse"] = True
        return [SimpleNamespace(name="SearchCIPDataverse")]

    monkeypatch.setattr(mcp_tools, "get_mcp_tools", incomplete)
    assert _run(mcp_tools.get_dataverse_search_mcp_tools()) == []
    assert _mcp_statuses["dataverse"] is False


def test_status_route_exposes_state_without_the_upstream_error() -> None:
    _clear_mcp_state()
    _mcp_statuses["dataverse"] = False
    response = _run(get_mcp_status(None))
    payload = json.loads(response.body)

    assert response.status_code == 200
    assert payload["services"][-1]["status"] == "unavailable"
    assert "secret" not in response.body.decode()


def test_adapter_uses_stateless_negotiation_and_server_ttl_cache(monkeypatch) -> None:
    captured = {}

    class Transport:
        def __init__(self, url, *, headers=None):
            captured["url"] = url
            captured["headers"] = headers

    class Client:
        def __init__(self, transport, **kwargs):
            captured["transport"] = transport
            captured["client"] = kwargs

    class Adapter:
        def __init__(self, client):
            self.client = client

    _clear_mcp_state()
    monkeypatch.setattr(mcp_tools, "StreamableHttpTransport", Transport)
    monkeypatch.setattr(mcp_tools, "Client", Client)
    monkeypatch.setattr(mcp_tools, "MCPAdapter", Adapter)

    adapter = mcp_tools._get_or_create_mcp_client(("agrovoc",))

    assert isinstance(adapter, Adapter)
    assert captured["url"] == "https://agrovoc.fastmcp.app/mcp"
    assert captured["headers"] is None
    assert captured["client"]["mode"] == "auto"
    assert captured["client"]["cache"] is True


def test_successful_catalogs_are_refreshed_through_fastmcp_cache(monkeypatch) -> None:
    async def invoke(**_kwargs):
        return "ok"

    tool = SimpleNamespace(name="lookup", coroutine=invoke, handle_tool_error=None)

    class Adapter:
        calls = 0

        async def list_tools(self, *, cache_mode):
            assert cache_mode == "use"
            self.calls += 1
            return [tool]

    _clear_mcp_state()
    adapter = Adapter()
    monkeypatch.setattr(mcp_tools, "_get_or_create_mcp_client", lambda _names: adapter)

    assert _run(mcp_tools.get_mcp_tools(("agrovoc",))) == [tool]
    capped = tool.coroutine
    assert _run(mcp_tools.get_mcp_tools(("agrovoc",))) == [tool]
    assert adapter.calls == 2
    assert tool.coroutine is capped, "the cached tool must not accumulate wrappers"
    assert ("agrovoc",) not in _mcp_tools_cache


def test_modern_mcp_elicitation_pauses_and_resumes_through_langgraph() -> None:
    from langgraph.checkpoint.memory import InMemorySaver
    from langgraph.graph import END, START, StateGraph
    from langgraph.types import Command

    server = FastMCP("elicitation-test")

    @server.tool
    async def guarded(ctx: Context) -> str | InputRequiredResult:
        if not ctx.input_responses:
            return InputRequiredResult(
                inputRequests={
                    "details": ElicitRequest(
                        params=ElicitRequestFormParams(
                            message="Which date?",
                            requestedSchema={
                                "type": "object",
                                "properties": {"date": {"type": "string"}},
                                "required": ["date"],
                            },
                        )
                    )
                }
            )
        return "date=" + str(ctx.input_responses["details"].content["date"])

    class State(TypedDict, total=False):
        result: object

    async def scenario():
        tools = mcp_tools._make_mcp_tools_resilient(
            await mcp_tools.MCPAdapter(server).list_tools()
        )

        async def call(_state):
            return {"result": await tools[0].ainvoke({})}

        builder = StateGraph(State)
        builder.add_node("call", call)
        builder.add_edge(START, "call")
        builder.add_edge("call", END)
        graph = builder.compile(checkpointer=InMemorySaver())
        config = {"configurable": {"thread_id": "elicitation-test"}}

        paused = await graph.ainvoke({}, config)
        request = paused["__interrupt__"][0].value
        assert request["type"] == "mcp_elicitation"
        assert request["requests"][0]["requested_schema"]["required"] == ["date"]

        resumed = await graph.ainvoke(
            Command(
                resume={
                    "responses": {
                        "details": {
                            "action": "accept",
                            "content": {"date": "2026-09-14"},
                        }
                    }
                }
            ),
            config,
        )
        assert "date=2026-09-14" in json.dumps(resumed["result"])

    _run(scenario())

"""Local Dataverse configuration and real stdio adapter behavior (no network)."""

from __future__ import annotations

import asyncio
import json
import os
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest
from blockbuster import blockbuster_ctx

from backend import dataverse_local, mcp_tools


@pytest.fixture(autouse=True)
def isolated_mcp_state(monkeypatch):
    for name in (
        "_mcp_clients", "_mcp_statuses", "_mcp_tools_cache",
        "_mcp_tools_locks", "_sign_in_marks",
    ):
        monkeypatch.setattr(mcp_tools, name, {})


@pytest.fixture
def local_paths(tmp_path, monkeypatch):
    folder = tmp_path / "MCP with spaces" / "server"
    folder.mkdir(parents=True)
    script = folder / "dataverse_server.py"
    script.write_text("# fixture", encoding="utf-8")
    monkeypatch.setenv("MINIME_DATAVERSE_SERVER", str(script))
    monkeypatch.setenv("MINIME_DATAVERSE_PYTHON", sys.executable)
    return script


def test_default_paths_use_the_bundled_server_and_backend_python(tmp_path, monkeypatch):
    # An empty home stands in for a colleague's machine with no AskPapa checkout.
    monkeypatch.setattr(Path, "home", classmethod(lambda cls: tmp_path))
    monkeypatch.delenv("MINIME_DATAVERSE_SERVER", raising=False)
    monkeypatch.delenv("MINIME_DATAVERSE_PYTHON", raising=False)
    folder = Path(dataverse_local.__file__).parent / "dataverse_mcp"
    config = dataverse_local.server_config()
    assert config["command"] == sys.executable
    assert config["args"] == [str(folder / "server.py")]
    assert config["cwd"] == str(folder)
    assert dataverse_local.setup_status()["ready"] is True


def test_only_server_settings_and_proxies_are_forwarded(local_paths, monkeypatch):
    monkeypatch.setenv("DATAVERSE_API_KEY", "dataverse-only")
    monkeypatch.setenv("MCP_JSON_DIR", str(local_paths.parent / "results"))
    monkeypatch.setenv("HTTPS_PROXY", "http://proxy.example:3128")
    monkeypatch.setenv("OPENAI_API_KEY", "never-forward-this")
    config = dataverse_local.server_config()
    assert config["command"] == sys.executable
    assert config["args"] == [str(local_paths)]
    assert config["env"]["DATAVERSE_API_KEY"] == "dataverse-only"
    assert config["env"]["HTTPS_PROXY"] == "http://proxy.example:3128"
    assert "OPENAI_API_KEY" not in config["env"]
    assert "url" not in config


@pytest.mark.skipif(os.name == "nt", reason="POSIX virtualenv symlink")
def test_python_symlink_is_not_resolved_to_the_base_interpreter(local_paths, monkeypatch):
    python = local_paths.parent / "venv-python"
    python.symlink_to(sys.executable)
    monkeypatch.setenv("MINIME_DATAVERSE_PYTHON", str(python))
    assert dataverse_local.server_config()["command"] == str(python)


@pytest.mark.parametrize("missing", ["MINIME_DATAVERSE_SERVER", "MINIME_DATAVERSE_PYTHON"])
def test_setup_names_the_missing_path(local_paths, monkeypatch, missing):
    monkeypatch.setenv(missing, str(local_paths.parent / "missing"))
    status = dataverse_local.setup_status()
    assert status["ready"] is False
    assert missing in status["detail"]


def test_stdio_adapter_has_no_http_or_oauth_and_is_reused(local_paths, monkeypatch):
    captured = {}

    class Transport:
        def __init__(self, **kwargs):
            captured["transport"] = kwargs

    class Client:
        def __init__(self, transport, **kwargs):
            captured["client"] = kwargs

    class Adapter:
        def __init__(self, client):
            pass

    def forbidden(*args, **kwargs):
        pytest.fail("local Dataverse must not use HTTP or OAuth")

    monkeypatch.setattr(mcp_tools, "StdioTransport", Transport)
    monkeypatch.setattr(mcp_tools, "StreamableHttpTransport", forbidden)
    monkeypatch.setattr(mcp_tools, "Client", Client)
    monkeypatch.setattr(mcp_tools, "MCPAdapter", Adapter)
    adapter = mcp_tools._get_or_create_mcp_client(("dataverse",))
    assert mcp_tools._get_or_create_mcp_client(("dataverse",)) is adapter
    assert captured["transport"]["args"] == [str(local_paths)]
    assert captured["transport"]["keep_alive"] is False
    assert captured["client"]["mode"] == "auto"
    assert captured["client"]["init_timeout"] == 30
    assert captured["client"]["cache"] is True
    assert "auth" not in captured["transport"]


def test_configuration_is_resolved_off_the_event_loop(local_paths, monkeypatch):
    class Adapter:
        def __init__(self, client):
            pass

        async def list_tools(self, **kwargs):
            return [SimpleNamespace(name="lookup", coroutine=None)]

    from backend import dataverse_auth

    async def forbidden_auth():
        pytest.fail("local Dataverse must not read the hosted OAuth token store")

    # Leave the real path resolution and StdioTransport/Client constructors in place.
    monkeypatch.setattr(mcp_tools, "MCPAdapter", Adapter)
    monkeypatch.setattr(dataverse_auth, "auth_for_runtime", forbidden_auth)

    async def scenario():
        with blockbuster_ctx():
            return await mcp_tools.get_mcp_tools(("dataverse",))

    assert asyncio.run(scenario())
    assert mcp_tools._mcp_statuses["dataverse"] is True


def test_missing_server_is_non_fatal_and_memoized(local_paths, monkeypatch):
    local_paths.unlink()
    attempts = []
    original = dataverse_local.server_config

    def resolve():
        attempts.append(1)
        return original()

    monkeypatch.setattr(dataverse_local, "server_config", resolve)
    assert asyncio.run(mcp_tools.get_dataverse_search_mcp_tools()) == []
    assert asyncio.run(mcp_tools.get_dataverse_search_mcp_tools()) == []
    assert attempts == [1]
    assert mcp_tools._mcp_statuses["dataverse"] is False


def test_real_stdio_discovery_filters_writes_and_files_survive_sessions(local_paths, monkeypatch):
    results = local_paths.parent / "results"
    monkeypatch.setenv("MCP_JSON_DIR", str(results))
    local_paths.write_text(
        "import json, os\n"
        "from pathlib import Path\n"
        "from fastmcp import FastMCP\n"
        "mcp = FastMCP(\"fixture-dataverse\")\n"
        "@mcp.tool\n"
        "def SearchCIPDataverse(query: str, output_filename: str) -> dict:\n"
        "    root = Path(os.environ[\"MCP_JSON_DIR\"]); root.mkdir(exist_ok=True)\n"
        "    path = root / output_filename\n"
        "    path.write_text(json.dumps({\"data\": [{\"name\": query}]}))\n"
        "    return {\"file_path\": str(path)}\n"
        "@mcp.tool\n"
        "def read_search_results(file_path: str) -> dict:\n"
        "    return json.loads(Path(file_path).read_text())\n"
        "@mcp.tool\n"
        "def list_dataset_files(persistent_id: str) -> dict:\n"
        "    return {\"files\": [], \"dataset_persistent_id\": persistent_id}\n"
        "@mcp.tool\n"
        "def delete_file_from_dataset(file_id: int) -> str:\n"
        "    raise AssertionError(\"write tool must never be exposed\")\n"
        "mcp.run(transport=\"stdio\")\n",
        encoding="utf-8",
    )

    async def scenario():
        tools = {tool.name: tool for tool in await mcp_tools.get_dataverse_search_mcp_tools()}
        assert set(tools) == {"SearchCIPDataverse", "read_search_results", "list_dataset_files"}
        await tools["SearchCIPDataverse"].ainvoke(
            {"query": "potato", "output_filename": "search.json"}
        )
        transport = mcp_tools._mcp_clients[("dataverse",)].client.transport
        assert transport._connect_task is None, "no child remains after search"
        read = await tools["read_search_results"].ainvoke(
            {"file_path": str(results / "search.json")}
        )
        assert "potato" in json.dumps(read)
        assert transport._connect_task is None, "no child remains after reading"
        await tools["list_dataset_files"].ainvoke({"persistent_id": "doi:10.21223/test"})
        assert mcp_tools._mcp_statuses["dataverse"] is True

    asyncio.run(scenario())


def test_bundled_mcp_works_without_an_external_checkout_or_api_key(tmp_path, monkeypatch):
    monkeypatch.delenv("MINIME_DATAVERSE_SERVER", raising=False)
    monkeypatch.delenv("MINIME_DATAVERSE_PYTHON", raising=False)
    monkeypatch.delenv("DATAVERSE_API_KEY", raising=False)
    monkeypatch.setattr(Path, "home", classmethod(lambda cls: tmp_path))
    results = tmp_path / "results"
    results.mkdir()
    path = results / "bundled-search.json"
    path.write_text(json.dumps([{"name": "potato"}]), encoding="utf-8")
    monkeypatch.setenv("MCP_JSON_DIR", str(results))
    monkeypatch.setenv("MCP_DOWNLOAD_DIR", str(tmp_path / "downloads"))

    async def scenario():
        tools = {tool.name: tool for tool in await mcp_tools.get_dataverse_search_mcp_tools()}
        assert set(tools) == {"SearchCIPDataverse", "read_search_results", "list_dataset_files"}
        read = await tools["read_search_results"].ainvoke({"file_path": str(path)})
        assert "potato" in json.dumps(read)
        client = mcp_tools._mcp_clients[("dataverse",)].client
        async with client:
            templates = await client.call_tool("list_dataset_templates", {})
            assert "finch_minimal" in json.dumps(templates.structured_content)
            example = await client.call_tool("get_dataset_template", {"name": "finch_minimal"})
            assert "datasetVersion" in json.dumps(example.structured_content)
            prompts = await client.get_prompt("dataverse_template_workflow")
            assert prompts.messages
        assert client.transport._connect_task is None

    asyncio.run(scenario())

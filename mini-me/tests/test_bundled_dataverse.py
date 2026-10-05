"""Bundled Dataverse behavior against mocked public API responses."""

import asyncio
import json
from tempfile import gettempdir

import httpx
import pytest

from backend.dataverse_mcp import server


class Context:
    async def report_progress(self, **kwargs):
        pass

    async def error(self, message):
        pytest.fail(message)


def test_public_search_read_and_file_listing_send_no_api_key(tmp_path, monkeypatch):
    monkeypatch.setattr(server, "API_KEY", None)
    monkeypatch.setattr(server, "DEFAULT_JSON_DIR", tmp_path)
    monkeypatch.setattr(server, "DEFAULT_DOWNLOAD_DIR", tmp_path / "downloads")
    requests = []

    def handle(request):
        requests.append(request)
        assert "X-Dataverse-key" not in request.headers
        if request.url.path == "/api/search":
            return httpx.Response(200, json={
                "status": "OK",
                "data": {"total_count": 1, "items": [
                    {"name": "Potato dataset", "global_id": "doi:10.21223/test"}
                ]},
            })
        assert request.url.params["persistentId"] == "doi:10.21223/test"
        return httpx.Response(200, json={
            "status": "OK",
            "data": [{"dataFile": {"id": 1, "filename": "potato.csv"}, "restricted": False}],
        })

    original = httpx.AsyncClient
    monkeypatch.setattr(
        server.httpx, "AsyncClient",
        lambda **kwargs: original(transport=httpx.MockTransport(handle), **kwargs),
    )

    async def scenario():
        search = await server.SearchCIPDataverse(
            Context(), query="potato", output_filename="search.json", max_results=1
        )
        assert search.status == "success"
        assert search.item_count == 1
        read = await server.read_search_results(search.output_file)
        assert read.content[0]["name"] == "Potato dataset"
        listing = await server.list_dataset_files(Context(), persistent_id="doi:10.21223/test")
        assert listing.status == "success"
        assert listing.files[0].filename == "potato.csv"
        assert listing.files[0].restricted is False

    asyncio.run(scenario())
    assert len(requests) == 2


def test_bundled_templates_are_valid_json_and_no_external_files_are_needed():
    folder = server.Path(server.__file__).parent
    assert (folder / "LICENSE").is_file()
    for template in server.DATASET_TEMPLATE_REGISTRY.values():
        path = template["path"]
        assert path.parent == folder
        assert json.loads(path.read_text(encoding="utf-8"))


def test_temporary_defaults_are_cross_platform():
    assert server._DEFAULT_OUTPUT_ROOT == server.Path(gettempdir()) / "mcp"


def test_read_search_results_rejects_files_outside_managed_directories(tmp_path, monkeypatch):
    monkeypatch.setattr(server, "DEFAULT_JSON_DIR", tmp_path / "managed")
    monkeypatch.setattr(server, "DEFAULT_DOWNLOAD_DIR", tmp_path / "downloads")
    outside = tmp_path / "outside.json"
    outside.write_text("[]", encoding="utf-8")
    with pytest.raises(FileNotFoundError):
        asyncio.run(server.read_search_results(str(outside)))

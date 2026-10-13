"""The report writer's answer is saved in the conversation folder as a PDF and as markdown.

The renderer itself (`routes/rendering._render_pdf_sync`) is replaced by a stub here: these tests
are about what `SaveReportFiles` hands it and where the files land, not about pandoc or Typst.
"""

from __future__ import annotations

import asyncio
import json
from types import SimpleNamespace

from backend.middleware import report_files
from backend.middleware.report_files import (
    SaveReportFiles,
    markdown_filename,
    markdown_for_reports_dir,
    pdf_filename,
)

WORK_DIR = "/work/thread-1"


class FakeWorkspace:
    """The three sandbox calls the middleware makes, over an in-memory file table."""

    def __init__(self, files: dict[str, bytes] | None = None):
        self.files = dict(files or {})
        self.uploaded: dict[str, bytes] = {}

    async def aget_work_dir(self) -> str:
        return WORK_DIR

    async def adownload_files(self, paths):
        return [
            SimpleNamespace(path=p, content=self.files.get(p), error=None if p in self.files else "missing")
            for p in paths
        ]

    async def aupload_files(self, files):
        self.uploaded.update(files)
        return [SimpleNamespace(path=p, error=None) for p, _ in files]


def _report(title: str, markdown: str):
    return {"structured_response": SimpleNamespace(title=title, markdown=markdown), "messages": []}


def _run(middleware: SaveReportFiles, state) -> None:
    assert asyncio.run(middleware.aafter_agent(state, None)) is None


def test_the_file_names_are_a_slug_of_the_title():
    assert pdf_filename("Yield gaps in Peru: 2020-2024!") == "yield-gaps-in-peru-2020-2024.pdf"
    assert markdown_filename("Yield gaps in Peru: 2020-2024!") == "yield-gaps-in-peru-2020-2024.md"
    assert pdf_filename("") == "report.pdf"
    assert pdf_filename("  ???  ") == "report.pdf"
    assert len(pdf_filename("x" * 300)) <= len(".pdf") + 80


def test_figures_are_relinked_for_a_file_inside_reports():
    markdown = (
        "![a](./eda.png) ![b](figs/b.png) ![c](https://x.org/c.png) "
        "![d](/abs/d.png) ![e](../e.png) ![f](data:image/png;base64,AA)"
    )
    assert markdown_for_reports_dir(markdown) == (
        "![a](../eda.png) ![b](../figs/b.png) ![c](https://x.org/c.png) "
        "![d](/abs/d.png) ![e](../e.png) ![f](data:image/png;base64,AA)"
    )


def test_the_report_is_saved_as_pdf_and_markdown(monkeypatch):
    calls = []

    def fake_render(**kwargs):
        calls.append(kwargs)
        return b"%PDF-fake"

    monkeypatch.setattr(report_files, "_render_pdf_sync", fake_render)
    workspace = FakeWorkspace(
        {
            f"{WORK_DIR}/eda.png": b"PNG",
            f"{WORK_DIR}/papers.json": json.dumps(
                [{"citation": "Doe (2024). A paper.", "link": "https://example.org"}]
            ).encode(),
        }
    )
    markdown = "# Results\n\n![Distribution](./eda.png)\n\n![Missing](./gone.png)\n"

    _run(SaveReportFiles(workspace), _report("Late blight review", markdown))

    assert workspace.uploaded[f"{WORK_DIR}/reports/late-blight-review.pdf"] == b"%PDF-fake"
    saved_md = workspace.uploaded[f"{WORK_DIR}/reports/late-blight-review.md"].decode()
    assert "](../eda.png)" in saved_md and "](./eda.png)" not in saved_md
    # The markdown copy keeps every figure link; only the PDF stubs the ones it cannot embed.
    assert "gone.png" in saved_md

    (call,) = calls
    assert call["title"] == "Late blight review"
    assert call["images"] == {"eda.png": b"PNG"}
    assert "gone.png" not in call["markdown"]
    assert call["sources"] == [{"citation": "Doe (2024). A paper.", "link": "https://example.org"}]
    assert call["used_asta"] is True


def test_without_papers_json_the_pdf_has_no_sources(monkeypatch):
    calls = []
    monkeypatch.setattr(report_files, "_render_pdf_sync", lambda **kw: calls.append(kw) or b"%PDF")
    workspace = FakeWorkspace()

    _run(SaveReportFiles(workspace), _report("Plain", "Body"))

    assert calls[0]["sources"] == [] and calls[0]["used_asta"] is False
    assert set(workspace.uploaded) == {f"{WORK_DIR}/reports/plain.pdf", f"{WORK_DIR}/reports/plain.md"}


def test_an_empty_report_saves_nothing(monkeypatch):
    monkeypatch.setattr(report_files, "_render_pdf_sync", lambda **kw: (_ for _ in ()).throw(AssertionError))
    workspace = FakeWorkspace()

    _run(SaveReportFiles(workspace), _report("Empty", "   "))
    _run(SaveReportFiles(workspace), {"structured_response": None, "messages": []})

    assert workspace.uploaded == {}


def test_a_failed_render_still_keeps_the_markdown(monkeypatch):
    def broken(**kwargs):
        raise RuntimeError("typst: unknown font")

    monkeypatch.setattr(report_files, "_render_pdf_sync", broken)
    workspace = FakeWorkspace()

    _run(SaveReportFiles(workspace), _report("Broken", "Body"))

    assert set(workspace.uploaded) == {f"{WORK_DIR}/reports/broken.md"}


def test_the_report_writer_saves_its_files_after_the_file_sync():
    """`after_agent` hooks run last-to-first, so saving must come after `file_sync` in the list."""
    from backend.subagents import _build_runtime_subagents

    class Resolver:
        def for_subagent(self, name, overrides):
            return "openai::gpt-4o-mini"

    file_sync = object()
    built = _build_runtime_subagents(
        academic_research_tools=[],
        dataverse_tools=[],
        data_cleaning_tools=[],
        diagnostic_tools=[],
        theory_tools=[],
        discovery_tools=[],
        validation_tools=[],
        file_sync=file_sync,
        sandbox_backend=FakeWorkspace(),
        model_resolver=Resolver(),
        subagent_overrides={},
    )
    holding = {s["name"] for s in built if any(isinstance(m, SaveReportFiles) for m in s["middleware"])}
    assert holding == {"report_writer"}

    (writer,) = [s for s in built if s["name"] == "report_writer"]
    middleware = writer["middleware"]
    saved_at = next(i for i, m in enumerate(middleware) if isinstance(m, SaveReportFiles))
    assert middleware.index(file_sync) < saved_at

"""PDFs are read as text by `read_pdf`, never handed to the model by `read_file`.

`read_file` passes a PDF as a `file` attachment, which OpenAI's Chat Completions API (and every
OpenRouter model) rejects; with that blocked, gpt-4o-mini told the researcher to go and find a PDF
reader. `read_pdf` extracts the text locally, so it works the same for every model.
"""

from __future__ import annotations

from pathlib import Path

from deepagents.backends.utils import validate_path
from pypdf import PdfWriter

from backend.runtime import _active_sandbox


def _backend(tmp_path: Path, monkeypatch, thread: str):
    from backend.local.workspace import LocalWorkspaceBackend

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    backend = LocalWorkspaceBackend(thread)
    (tmp_path / thread).mkdir(parents=True, exist_ok=True)
    return backend


def _blank_pdf(path: Path, pages: int = 2) -> None:
    writer = PdfWriter()
    for _ in range(pages):
        writer.add_blank_page(width=200, height=200)
    with path.open("wb") as handle:
        writer.write(handle)


def test_a_model_without_pdf_attachments_is_pointed_at_read_pdf(tmp_path, monkeypatch):
    from backend.models import build_chat_model

    backend = _backend(tmp_path, monkeypatch, "thread-pdf-a")
    backend.adapt_to_model(
        build_chat_model(
            "custom::openai/gpt-4o-mini",
            {"api_key": "sk-test", "base_url": "https://openrouter.ai/api/v1"},
        )
    )
    _blank_pdf(tmp_path / "thread-pdf-a" / "paper.pdf")

    result = backend.read(validate_path("./paper.pdf"))

    assert result.error and "read_pdf" in result.error
    assert result.file_data is None, "the PDF must not reach the model as an attachment"


def test_a_model_that_reads_pdfs_still_gets_the_attachment(tmp_path, monkeypatch):
    """Claude and Gemini read scans and figures natively; that must not be taken away."""
    backend = _backend(tmp_path, monkeypatch, "thread-pdf-e")
    _blank_pdf(tmp_path / "thread-pdf-e" / "paper.pdf")

    result = backend.read(validate_path("./paper.pdf"))

    assert not result.error, result.error
    assert result.file_data and result.file_data["encoding"] == "base64"


def test_read_pdf_finds_an_attached_file_and_reports_a_scan(tmp_path, monkeypatch):
    from backend.pdf_tools import read_pdf

    backend = _backend(tmp_path, monkeypatch, "thread-pdf-b")
    _blank_pdf(tmp_path / "thread-pdf-b" / "scan.pdf", pages=3)
    token = _active_sandbox.set(backend)
    try:
        answer = read_pdf.invoke({"file_path": "./scan.pdf"})
    finally:
        _active_sandbox.reset(token)

    # Blank pages have no text layer, which is how a scanned PDF looks.
    assert "3 pages" in answer
    assert "PDF Librarian" in answer


def test_read_pdf_says_when_the_file_is_missing(tmp_path, monkeypatch):
    from backend.pdf_tools import read_pdf

    backend = _backend(tmp_path, monkeypatch, "thread-pdf-c")
    token = _active_sandbox.set(backend)
    try:
        answer = read_pdf.invoke({"file_path": "./nowhere.pdf"})
    finally:
        _active_sandbox.reset(token)

    assert answer.startswith("Error:") and "not found" in answer


def test_a_powerpoint_is_reported_as_unsupported(tmp_path, monkeypatch):
    """deepagents lets a `.pptx` attachment through for OpenAI models, which then return a 400."""
    backend = _backend(tmp_path, monkeypatch, "thread-pdf-d")
    backend.strict_attachments = True  # what `adapt_to_model` sets for an OpenAI-class model
    (tmp_path / "thread-pdf-d" / "slides.pptx").write_bytes(b"PK not really a deck")

    result = backend.read(validate_path("./slides.pptx"))

    assert result.error and "not supported" in result.error
    assert result.file_data is None

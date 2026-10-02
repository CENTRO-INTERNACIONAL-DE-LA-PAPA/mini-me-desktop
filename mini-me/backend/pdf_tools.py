"""Read the text of a PDF in the conversation's folder, locally.

**Why a tool of its own.** deepagents' `read_file` hands a PDF to the model as a `file` attachment,
and many models cannot take one: OpenAI's Chat Completions API (and so every OpenRouter model)
refuses the request with `400 Invalid value: 'file'`. For those models `read_file` points here
instead (`LocalWorkspaceBackend.read`). Extracting the text here works the same for every model,
offline, with no credits spent. Scanned PDFs have no text layer; for those the answer says so and
points at the PDF Librarian, whose OCR can read them.
"""

from __future__ import annotations

import logging

from deepagents.backends.utils import validate_path
from langchain_core.tools import tool

from backend.runtime import _active_sandbox

logger = logging.getLogger(__name__)

#: Enough for a long paper; past it the answer says how to ask for the rest by page.
_MAX_CHARS = 60_000


@tool
def read_pdf(file_path: str, first_page: int = 1, last_page: int | None = None) -> str:
    """Extract the text of a PDF file so you can read, summarise or quote it.

    Use this for every `.pdf`. Attached files arrive as `./<name>.pdf`; pass that exact path.
    Pages are numbered from 1; for a long PDF, read it in page ranges with `first_page` and
    `last_page`.
    """
    sandbox = _active_sandbox.get()
    if sandbox is None:
        return "Error: no working directory is active for this conversation."
    try:
        # The same checks as the other file tools: no `..` or `~`, and `./x` resolved the way
        # `read_file` resolves it (`LocalWorkspaceBackend._resolve_path`).
        path = sandbox._resolve_path(validate_path(file_path))
    except (ValueError, OSError, RuntimeError) as error:
        return f"Error: '{file_path}' is not a usable path ({error})."
    if not path.is_file():
        return f"Error: '{file_path}' was not found in the working directory."
    if path.suffix.lower() != ".pdf":
        return f"Error: '{file_path}' is not a PDF. Use read_file for other files."

    try:
        from pypdf import PdfReader

        reader = PdfReader(str(path))
        total = len(reader.pages)
    except Exception as error:  # a damaged or encrypted file is an answer, not a crash
        logger.warning("read_pdf: could not open %s: %s", path, error)
        return f"Error: '{file_path}' could not be opened as a PDF ({error})."

    start = max(first_page, 1)
    end = total if last_page is None else min(last_page, total)
    if start > end:
        asked = f"page {first_page}" if last_page is None else f"pages {first_page}–{last_page}"
        return f"Error: '{file_path}' has {total} pages; {asked} is outside it."

    parts: list[str] = []
    size = 0
    pages_with_text = 0
    unreadable: list[int] = []
    stopped_at: int | None = None
    for number in range(start, end + 1):
        try:
            text = (reader.pages[number - 1].extract_text() or "").strip()
        except Exception as error:
            unreadable.append(number)
            text = f"[page {number} could not be read: {error}]"
        else:
            pages_with_text += bool(text)
        block = f"--- page {number} of {total} ---\n{text}"
        room = _MAX_CHARS - size
        if len(block) > room:
            if parts:
                stopped_at = number
                break
            # A single page bigger than the whole budget: cut it rather than flood the context.
            block = block[:room] + "\n[page cut to stay within size]"
            stopped_at = number + 1 if number < end else None
            parts.append(block)
            break
        parts.append(block)
        size += len(block)

    read_through = (stopped_at - 1) if stopped_at is not None else end
    if pages_with_text == 0 and not unreadable:
        span = (
            "none of them contains"
            if (start, read_through) == (1, total)
            else f"pages {start}–{read_through} contain"
        )
        return (
            f"'{file_path}' has {total} pages and {span} extractable text — probably a "
            "scanned image. Hand it to the PDF Librarian, whose OCR can read scanned documents."
        )
    if pages_with_text == 0:
        return (
            f"Error: no text could be read from '{file_path}' "
            f"({len(unreadable)} page(s) failed). It may be damaged or encrypted.\n\n"
            + "\n\n".join(parts)
        )
    body = "\n\n".join(parts)
    if stopped_at is not None and stopped_at <= end:
        body += (
            f"\n\n[Stopped at page {read_through} of {total} to stay within size. "
            f"Call read_pdf again with first_page={stopped_at} for the rest.]"
        )
    return body

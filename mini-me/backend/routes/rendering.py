"""Markdown -> Typst -> PDF report rendering for the /render-report route."""

from __future__ import annotations

import asyncio
import re
import tempfile
from pathlib import Path, PurePosixPath

from starlette.requests import Request
from starlette.responses import JSONResponse, Response

from backend.routes.common import (
    _existing_sandbox_for_thread,
    _require_auth,
    _resolve_within,
)


IMAGE_EXTENSIONS = {".png", ".jpg", ".jpeg", ".svg", ".webp", ".gif"}
MARKDOWN_IMAGE_RE = re.compile(r"!\[[^\]]*\]\(\s*([^)\s]+)(?:\s+\"[^\"]*\")?\s*\)")


# Pandoc's Typst writer emits references to helpers like `#horizontalrule`
# and `#blockquote(...)` and expects the document to define them. They are
# normally provided by pandoc's bundled `template.typst`; we are emitting
# `--no-standalone` output and have our own wrapper, so we include the
# definitions ourselves. Must be prepended to `body.typ` (not the wrapper)
# because Typst's `#include` evaluates the included file in its own scope.
PANDOC_TYPST_HELPERS = (
    "#let horizontalrule = [\n"
    "  #line(start: (25%,0%), end: (75%,0%))\n"
    "]\n"
    "\n"
    "#let blockquote(body) = [\n"
    "  #set text(size: 0.92em)\n"
    "  #block(inset: (left: 1.5em, top: 0.2em, bottom: 0.2em))[#body]\n"
    "]\n"
    "\n"
)


def _normalize_image_ref(path: str) -> str | None:
    """Normalize an image reference into a sandbox-relative path, or None."""
    path = path.strip()
    if not path or path.startswith(("http://", "https://", "data:")):
        return None
    if PurePosixPath(path).suffix.lower() not in IMAGE_EXTENSIONS:
        return None
    cleaned = path[2:] if path.startswith("./") else path
    if cleaned.startswith("/"):
        return None
    if ".." in PurePosixPath(cleaned).parts:
        return None
    return cleaned


def _extract_image_refs(markdown: str) -> list[str]:
    """Return unique relative image paths referenced in the markdown."""
    seen: dict[str, None] = {}
    for raw in MARKDOWN_IMAGE_RE.findall(markdown):
        normalized = _normalize_image_ref(raw)
        if normalized is not None:
            seen.setdefault(normalized, None)
    return list(seen.keys())


def _stub_missing_images(markdown: str, available: set[str]) -> str:
    """Replace markdown image refs we couldn't resolve with a visible note."""

    def _replace(match: re.Match[str]) -> str:
        raw = match.group(1)
        normalized = _normalize_image_ref(raw)
        if normalized is not None and normalized in available:
            return match.group(0)
        if normalized is None:
            label = raw.strip()
        else:
            label = normalized
        return f"*[image not available: `{label}`]*"

    return MARKDOWN_IMAGE_RE.sub(_replace, markdown)


ASTA_ATTRIBUTION = (
    "Academic literature search performed using Asta tools (Allen Institute "
    "for AI). Please cite the AstaBench paper: arXiv:2510.21652 — "
    "https://arxiv.org/abs/2510.21652."
)


def _typst_str(value: str) -> str:
    """Quote a Python string for safe inclusion inside a Typst string literal."""
    return (
        value.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("\n", " ")
        .strip()
    )


def _typst_content(value: str) -> str:
    """Escape characters that have special meaning inside Typst content."""
    out = []
    for ch in value.replace("\r", "").replace("\n", " "):
        if ch in ("\\", "*", "_", "#", "$", "<", ">", "@", "[", "]"):
            out.append("\\" + ch)
        else:
            out.append(ch)
    return "".join(out).strip()


def _build_typst_wrapper(
    title: str,
    sources: list[dict],
    used_asta: bool,
) -> str:
    from datetime import datetime

    title_str = _typst_str(title)
    title_content = _typst_content(title)
    date_str = datetime.utcnow().strftime("%B %d, %Y")

    sources_block = ""
    cleaned_sources: list[tuple[str, str]] = []
    for source in sources or []:
        citation_raw = (source.get("citation") or "").strip()
        link_raw = (source.get("link") or "").strip()
        if not citation_raw:
            continue
        cleaned_sources.append((citation_raw, link_raw))

    if cleaned_sources:
        lines = ["#pagebreak()", "= Sources", ""]
        for citation_raw, link_raw in cleaned_sources:
            citation = _typst_content(citation_raw)
            if link_raw:
                link_str = _typst_str(link_raw)
                lines.append(
                    f'- {citation} #h(0.3em) #link("{link_str}")[#text(size: 8.5pt, fill: rgb("#b85c38"))[(source)]]'
                )
            else:
                lines.append(f"- {citation}")
        sources_block = "\n".join(lines) + "\n"

    attribution_block = ""
    if used_asta:
        attribution_text = _typst_content(ASTA_ATTRIBUTION)
        attribution_block = (
            "\n#v(2em)\n"
            '#line(length: 100%, stroke: 0.5pt + rgb("#ebe2d2"))\n'
            "#v(0.5em)\n"
            '#text(size: 8pt, fill: rgb("#586071"))[\n'
            f"  {attribution_text}\n"
            "]\n"
        )

    return (
        f'#set document(title: "{title_str}", author: "AskTheData")\n'
        "#set page(\n"
        '  paper: "a4",\n'
        "  margin: (top: 2.5cm, bottom: 2.5cm, left: 2.2cm, right: 2.2cm),\n"
        '  numbering: "1",\n'
        "  number-align: center,\n"
        ")\n"
        "\n"
        "#set text(\n"
        '  font: ("Inter", "DejaVu Sans", "Helvetica"),\n'
        "  size: 10.5pt,\n"
        '  lang: "en",\n'
        ")\n"
        "\n"
        "#set par(justify: true, leading: 0.65em)\n"
        "\n"
        '#set heading(numbering: "1.1")\n'
        "\n"
        "#show heading: it => block(\n"
        "  spacing: 1.2em,\n"
        "  text(\n"
        '    font: ("Newsreader", "Georgia", "Times New Roman"),\n'
        "    weight: 600,\n"
        '    fill: rgb("#1a2238"),\n'
        "    size: if it.level == 1 { 18pt } else if it.level == 2 { 14pt } else { 11.5pt },\n"
        "  )[#it.body],\n"
        ")\n"
        "\n"
        '#show link: set text(fill: rgb("#b85c38"))\n'
        "\n"
        "#show raw.where(block: false): box.with(\n"
        '  fill: rgb("#f3ede0"),\n'
        "  inset: (x: 3pt, y: 0pt),\n"
        "  outset: (y: 3pt),\n"
        "  radius: 2pt,\n"
        ")\n"
        "\n"
        "#show raw.where(block: true): block.with(\n"
        '  fill: rgb("#faf6ee"),\n'
        '  stroke: 0.5pt + rgb("#ebe2d2"),\n'
        "  radius: 5pt,\n"
        "  inset: 9pt,\n"
        "  width: 100%,\n"
        ")\n"
        "\n"
        '#show table: set table(stroke: 0.5pt + rgb("#cdbfa5"))\n'
        "\n"
        "// Prevent character-level hyphenation inside narrow table cells.\n"
        "// The global `set par(justify: true)` propagates into cells, which\n"
        "// produces garbled word-splits in 5–6 column bibliographic tables.\n"
        "#show table.cell: set par(justify: false, linebreaks: \"simple\")\n"
        "#show table: set text(size: 9pt)\n"
        "\n"
        "// Title page\n"
        "#align(center + horizon)[\n"
        "  #block[\n"
        "    #text(\n"
        '      font: ("Newsreader", "Georgia", "Times New Roman"),\n'
        "      size: 28pt,\n"
        "      weight: 600,\n"
        '      fill: rgb("#1a2238"),\n'
        "    )[\n"
        f"      {title_content}\n"
        "    ]\n"
        "    #v(1em)\n"
        '    #text(size: 10pt, fill: rgb("#586071"))[\n'
        f"      {date_str} · AskTheData research workbench\n"
        "    ]\n"
        "  ]\n"
        "]\n"
        "\n"
        "#pagebreak()\n"
        "\n"
        "// Table of contents\n"
        "#outline(title: [Contents], depth: 2, indent: auto)\n"
        "\n"
        "#pagebreak()\n"
        "\n"
        "// Body content (from pandoc)\n"
        '#include "body.typ"\n'
        "\n"
        f"{sources_block}"
        f"{attribution_block}"
    )


def _render_pdf_sync(
    *,
    markdown: str,
    title: str,
    sources: list,
    used_asta: bool,
    images: dict[str, bytes] | None = None,
) -> bytes:
    """Render markdown -> PDF in-process via pypandoc + typst. Blocking.

    ``images`` is a mapping of relative path -> file bytes. The bytes are
    written into the temporary working directory under the same relative
    name so Typst's ``image()`` directive (emitted by pandoc) can resolve
    them at compile time. Image references that aren't supplied here will
    cause Typst to emit a missing-file error.
    """
    import pypandoc
    import typst

    body_typst = pypandoc.convert_text(
        markdown.strip(),
        "typst",
        format="markdown+pipe_tables+task_lists",
        extra_args=["--wrap=none"],
    )
    wrapper_doc = _build_typst_wrapper(
        title=title, sources=sources, used_asta=used_asta
    )

    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        (tmp_path / "body.typ").write_text(
            PANDOC_TYPST_HELPERS + body_typst, encoding="utf-8"
        )
        (tmp_path / "report.typ").write_text(wrapper_doc, encoding="utf-8")
        for rel_path, content in (images or {}).items():
            target = tmp_path / rel_path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
        result = typst.compile(str(tmp_path / "report.typ"))
    if isinstance(result, list):
        return b"".join(result)
    return result


async def _download_referenced_images(
    thread_id: str, markdown: str
) -> dict[str, bytes]:
    """Download images referenced in the markdown from the thread sandbox."""
    refs = _extract_image_refs(markdown)
    if not refs:
        return {}

    adapter = await _existing_sandbox_for_thread(thread_id)
    if adapter is None:
        return {}

    work_dir = PurePosixPath(await adapter.aget_work_dir())

    abs_paths: list[str] = []
    rel_for_abs: dict[str, str] = {}
    for rel in refs:
        abs_path = _resolve_within(work_dir, rel)
        if abs_path is None:
            continue
        abs_str = str(abs_path)
        abs_paths.append(abs_str)
        rel_for_abs[abs_str] = rel

    if not abs_paths:
        return {}

    results = await adapter.adownload_files(abs_paths)
    downloaded: dict[str, bytes] = {}
    for res in results or []:
        if res.error or res.content is None:
            continue
        rel = rel_for_abs.get(res.path)
        if rel is not None:
            downloaded[rel] = res.content
    return downloaded


async def render_report(request: Request) -> Response:
    if (unauth := _require_auth(request)) is not None:
        return unauth
    thread_id = request.path_params["thread_id"]
    if not thread_id:
        return JSONResponse({"error": "missing thread_id"}, status_code=400)

    try:
        payload = await request.json()
    except Exception:  # noqa: BLE001
        return JSONResponse({"error": "invalid JSON body"}, status_code=400)

    markdown = payload.get("markdown")
    title = payload.get("title") or "Research Report"
    sources = payload.get("sources") or []
    used_asta = bool(payload.get("used_asta", len(sources) > 0))

    if not isinstance(markdown, str) or not markdown.strip():
        return JSONResponse(
            {"error": "missing or empty 'markdown' field"}, status_code=400
        )
    if not isinstance(sources, list):
        return JSONResponse(
            {"error": "'sources' must be an array if provided"}, status_code=400
        )

    try:
        images = await _download_referenced_images(thread_id, markdown)
    except Exception:  # noqa: BLE001
        images = {}

    # Replace image references that we could not resolve with a visible
    # placeholder so Typst doesn't fail the entire compile on one missing
    # file.
    rendered_markdown = _stub_missing_images(markdown, set(images.keys()))

    try:
        pdf_bytes = await asyncio.to_thread(
            _render_pdf_sync,
            markdown=rendered_markdown,
            title=title,
            sources=sources,
            used_asta=used_asta,
            images=images,
        )
    except Exception as exc:  # noqa: BLE001
        return JSONResponse(
            {"error": f"PDF render failed: {exc}"}, status_code=502
        )

    return Response(
        content=pdf_bytes,
        media_type="application/pdf",
        headers={
            "Content-Disposition": 'attachment; filename="report.pdf"',
        },
    )

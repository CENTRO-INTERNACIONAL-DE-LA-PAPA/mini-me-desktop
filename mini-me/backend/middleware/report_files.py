"""Save every report the report writer produces in the conversation folder, as PDF and markdown.

The PDF is the deliverable; the markdown beside it is the editable source of the same report.

The renderer is the one behind the Report card's download (`routes/rendering.py`), so a saved PDF
and a downloaded one are the same document: the markdown through pandoc and Typst, images
referenced as `./figure.png` embedded from the workspace, and a Sources section from
`papers.json` when Academic Research ran in this conversation.

The PDF is written by the backend once the report exists, not requested from the model, so it
cannot be forgotten. The markdown is written first, so a render that fails costs the PDF and
nothing else.
"""

from __future__ import annotations

import asyncio
import json
import logging
import re
from pathlib import PurePosixPath
from typing import Any

from langchain.agents.middleware import AgentMiddleware

from backend.routes.common import _resolve_within
from backend.routes.rendering import (
    _extract_image_refs,
    _render_pdf_sync,
    _stub_missing_images,
)
from backend.schemas import ArtifactState

logger = logging.getLogger(__name__)

#: Where reports land, relative to the conversation folder.
REPORTS_DIR = "reports"

#: Written by `middleware/search_first.KeepSources`: everything the literature searches returned.
SOURCES_FILE = "papers.json"


def report_stem(title: str) -> str:
    """A safe file stem for a report title: `Yield gaps in Peru` -> `yield-gaps-in-peru`."""
    slug = re.sub(r"[^a-z0-9]+", "-", (title or "").lower()).strip("-")[:80].strip("-")
    return slug or "report"


def pdf_filename(title: str) -> str:
    return f"{report_stem(title)}.pdf"


def markdown_filename(title: str) -> str:
    return f"{report_stem(title)}.md"


#: A markdown image whose target is relative to the conversation folder: not a URL, not absolute,
#: not already climbing out with `../`.
_WORKSPACE_IMAGE = re.compile(r"(!\[[^\]]*\]\(\s*)(?:\./)?(?![a-zA-Z][a-zA-Z0-9+.-]*:|/|\.\./)")


def markdown_for_reports_dir(markdown: str) -> str:
    """The report as it must read from inside `reports/`: `./eda.png` becomes `../eda.png`.

    The report writer links figures relative to the conversation folder, and the saved `.md` sits
    one level below it.
    """
    return _WORKSPACE_IMAGE.sub(r"\1../", markdown)


class SaveReportFiles(AgentMiddleware):
    """Save the report writer's structured answer as `reports/<title>.pdf` and `reports/<title>.md`.

    Must sit **after** `FileSyncMiddleware` in the subagent's middleware list: LangChain runs
    `after_agent` hooks last-to-first, and the PDF has to exist before the file sync looks for
    new files, or it only reaches the Outputs panel on the next turn.
    """

    state_schema = ArtifactState

    def __init__(self, sandbox_backend: Any):
        super().__init__()
        self.sandbox_backend = sandbox_backend

    async def _read(self, path: str) -> bytes | None:
        results = await self.sandbox_backend.adownload_files([path])
        for result in results or []:
            if not getattr(result, "error", None) and result.content is not None:
                return result.content
        return None

    async def _images(self, work_dir: PurePosixPath, markdown: str) -> dict[str, bytes]:
        images: dict[str, bytes] = {}
        for rel in _extract_image_refs(markdown):
            path = _resolve_within(work_dir, rel)
            if path is None:
                continue
            content = await self._read(str(path))
            if content is not None:
                images[rel] = content
        return images

    async def _sources(self, work_dir: PurePosixPath) -> list[Any]:
        raw = await self._read(str(work_dir / SOURCES_FILE))
        if raw is None:
            return []
        try:
            sources = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            logger.warning("could not read %s for the report's Sources section", SOURCES_FILE)
            return []
        return sources if isinstance(sources, list) else []

    async def _save(self, target: PurePosixPath, content: bytes, kind: str) -> None:
        results = await self.sandbox_backend.aupload_files([(str(target), content)])
        errors = [r.error for r in results or [] if getattr(r, "error", None)]
        if errors:
            logger.warning("could not save the report %s %s: %s", kind, target, errors[0])
        else:
            logger.info("saved the report %s %s (%d bytes)", kind, target, len(content))

    async def aafter_agent(self, state: Any, runtime: Any) -> dict[str, Any] | None:
        structured = state.get("structured_response")
        markdown = getattr(structured, "markdown", None)
        if not isinstance(markdown, str) or not markdown.strip():
            return None
        title = getattr(structured, "title", None) or "Research Report"
        try:
            work_dir = PurePosixPath(str(await self.sandbox_backend.aget_work_dir()))
        except Exception:
            logger.exception("could not find the workspace to save the report %r", title)
            return None
        reports = work_dir / REPORTS_DIR
        try:
            await self._save(
                reports / markdown_filename(title),
                markdown_for_reports_dir(markdown).encode("utf-8"),
                "markdown",
            )
        except Exception:
            logger.exception("could not save the report %r as markdown", title)
        try:
            images = await self._images(work_dir, markdown)
            sources = await self._sources(work_dir)
            pdf = await asyncio.to_thread(
                _render_pdf_sync,
                markdown=_stub_missing_images(markdown, set(images)),
                title=title,
                sources=sources,
                used_asta=bool(sources),
                images=images,
            )
            await self._save(reports / pdf_filename(title), pdf, "PDF")
        except Exception:
            logger.exception("could not render the report %r to PDF", title)
        return None

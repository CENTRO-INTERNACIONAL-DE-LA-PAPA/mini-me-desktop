#!/usr/bin/env python3
"""Download open-access PDFs for scientific papers into the sandbox workspace.

This is the *download* move the PDF librarian was missing: it resolves a paper
reference (an ID like ``DOI:…`` / ``ARXIV:…`` / ``CorpusId:…``, or a free-text
title) to an **open-access** PDF URL via the ``asta papers`` CLI, fetches the
bytes, and writes them under ``./papers/`` in the sandbox working directory so
the existing extract → index flow can read them.

Scope is deliberately **open access only**. Paywalled publishers need an
authenticated browser, which is not available headless in the sandbox — those
resolve to ``no_oa`` and the caller should ask the user to upload the PDF.

Design: everything that parses CLI JSON or builds URLs/paths is a pure function
(unit-tested in ``tests/test_pdf_fetch.py``); only :func:`resolve_paper` and
:func:`fetch_pdf` touch the CLI / network.

Usage:
    python3 fetch_paper.py "ARXIV:2005.14165" "DOI:10.1101/2020.05.20.106971"
    python3 fetch_paper.py "Yuyama coffea heat shock 2023" -o ./papers
The script prints one JSON array (one record per input) to stdout.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

# Semantic Scholar id schemes that ``asta papers get`` accepts directly. Any
# positional arg NOT matching one of these is treated as a free-text title and
# resolved through ``asta papers search``.
_ID_SCHEMES = ("DOI", "ARXIV", "CORPUSID", "PMID", "PMCID", "MAG", "ACL", "URL")
_ID_RE = re.compile(rf"^({'|'.join(_ID_SCHEMES)}):", re.IGNORECASE)

# Fields we ask Semantic Scholar for — enough to find an OA URL and name a file.
_FIELDS = "title,openAccessPdf,externalIds"

# Guardrails on the fetch itself.
_MAX_BYTES = 80 * 1024 * 1024  # 80 MB — generous for a scanned paper, bounded.
_USER_AGENT = "Mini-Me-PDF-Librarian/1.0 (+research workbench; open-access only)"


# ---------------------------------------------------------------------------
# Pure helpers (unit-tested)
# ---------------------------------------------------------------------------

def is_paper_id(ref: str) -> bool:
    """True if ``ref`` is a scheme-qualified id (vs a free-text title)."""
    return bool(_ID_RE.match(ref.strip()))


def arxiv_pdf_url(arxiv_id: str) -> str:
    """Canonical arXiv PDF URL for a bare arXiv id (version suffix preserved)."""
    return f"https://arxiv.org/pdf/{arxiv_id.strip()}"


def oa_url_from_record(record: Any) -> str | None:
    """Best open-access PDF URL for a Semantic Scholar paper record, or ``None``.

    Prefers the curated ``openAccessPdf.url`` when populated; otherwise falls
    back to arXiv, since ``openAccessPdf`` is frequently empty even for papers
    that are plainly on arXiv (observed for e.g. arXiv:2005.14165).
    """
    if not isinstance(record, dict):
        return None
    oa = record.get("openAccessPdf")
    if isinstance(oa, dict):
        url = (oa.get("url") or "").strip()
        if url:
            return url
    external = record.get("externalIds")
    if isinstance(external, dict):
        arxiv = external.get("ArXiv") or external.get("arXiv") or external.get("arxiv")
        if arxiv:
            return arxiv_pdf_url(str(arxiv))
    return None


def _first_record(payload: Any) -> dict[str, Any] | None:
    """Pull the first paper record from a ``papers get``/``search`` JSON payload.

    ``get`` returns a single object; ``search`` returns either a bare list or a
    ``{"data": [...]}`` envelope depending on CLI version. Tolerate all three.
    """
    if isinstance(payload, dict):
        if "paperId" in payload or "openAccessPdf" in payload or "externalIds" in payload:
            return payload
        data = payload.get("data")
        if isinstance(data, list) and data and isinstance(data[0], dict):
            return data[0]
        return None
    if isinstance(payload, list) and payload and isinstance(payload[0], dict):
        return payload[0]
    return None


def slugify(text: str, *, fallback: str = "paper") -> str:
    """Filesystem-safe, lowercase, hyphenated slug (bounded length)."""
    slug = re.sub(r"[^\w\s-]", "", (text or "").strip().lower())
    slug = re.sub(r"[\s_-]+", "-", slug).strip("-")
    return (slug[:80] or fallback)


def dest_path(out_dir: str, record: dict[str, Any], ref: str) -> Path:
    """Where to save this paper's PDF: ``<out_dir>/<title-or-ref>.pdf``."""
    title = (record.get("title") if isinstance(record, dict) else None) or ref
    return Path(out_dir) / f"{slugify(title, fallback=slugify(ref))}.pdf"


def looks_like_pdf(content_type: str | None, head: bytes) -> bool:
    """Heuristic: a real PDF response by content-type or magic bytes."""
    if content_type and "application/pdf" in content_type.lower():
        return True
    return head[:5] == b"%PDF-"


# ---------------------------------------------------------------------------
# Side-effecting steps (CLI + network) — integration-tested only
# ---------------------------------------------------------------------------

def _run_asta(args: list[str]) -> Any:
    """Run an ``asta`` subcommand expecting JSON on stdout; ``None`` on failure."""
    try:
        proc = subprocess.run(
            ["asta", *args], capture_output=True, text=True, timeout=120
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if proc.returncode != 0 or not proc.stdout.strip():
        return None
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError:
        return None


def resolve_paper(ref: str) -> dict[str, Any] | None:
    """Resolve a reference (id or title) to a Semantic Scholar paper record."""
    if is_paper_id(ref):
        payload = _run_asta(["papers", "get", ref, "--fields", _FIELDS, "--format", "json"])
        return _first_record(payload)
    # Free-text title → top search hit.
    payload = _run_asta(
        ["papers", "search", ref, "--fields", _FIELDS, "--limit", "1", "--format", "json"]
    )
    return _first_record(payload)


def fetch_pdf(url: str, dest: Path) -> tuple[bool, str | None]:
    """Download ``url`` to ``dest`` if it is a PDF. Returns ``(ok, error)``."""
    req = urllib.request.Request(url, headers={"User-Agent": _USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:  # noqa: S310 (OA URLs only)
            content_type = resp.headers.get("Content-Type")
            data = resp.read(_MAX_BYTES + 1)
    except (urllib.error.URLError, OSError, ValueError) as exc:
        return False, f"fetch failed: {exc}"
    if len(data) > _MAX_BYTES:
        return False, f"pdf exceeds {_MAX_BYTES // (1024 * 1024)}MB cap"
    if not looks_like_pdf(content_type, data):
        return False, f"response was not a PDF (content-type={content_type!r})"
    try:
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
    except OSError as exc:
        return False, f"write failed: {exc}"
    return True, None


def download_one(ref: str, out_dir: str) -> dict[str, Any]:
    """Resolve + fetch a single reference into ``out_dir``; structured result."""
    result: dict[str, Any] = {"ref": ref, "title": None, "source_url": None,
                              "path": None, "status": "fetch_failed", "error": None}
    record = resolve_paper(ref)
    if record is None:
        result["error"] = "could not resolve the reference via `asta papers`"
        result["status"] = "unresolved"
        return result
    result["title"] = record.get("title")
    url = oa_url_from_record(record)
    if not url:
        result["status"] = "no_oa"
        result["error"] = "no open-access PDF available (likely paywalled — ask the user to upload)"
        return result
    result["source_url"] = url
    dest = dest_path(out_dir, record, ref)
    ok, err = fetch_pdf(url, dest)
    if ok:
        result["status"] = "downloaded"
        result["path"] = str(dest)
    else:
        result["error"] = err
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("refs", nargs="+", help="Paper ids (DOI:/ARXIV:/CorpusId:…) or titles.")
    parser.add_argument("-o", "--out-dir", default="./papers",
                        help="Directory to save PDFs into (default: ./papers).")
    ns = parser.parse_args(argv)

    results = [download_one(ref, ns.out_dir) for ref in ns.refs]
    json.dump(results, sys.stdout, indent=2)
    sys.stdout.write("\n")
    # Exit non-zero only if EVERY reference failed, so a partial batch still
    # surfaces the successes to the caller.
    return 0 if any(r["status"] == "downloaded" for r in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())

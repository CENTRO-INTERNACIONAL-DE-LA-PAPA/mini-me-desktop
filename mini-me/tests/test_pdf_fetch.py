"""Tests for the PDF-librarian download helper (`skills/pdf_library/scripts/fetch_paper.py`).

These pin the *pure* resolver/URL/path logic that decides where an open-access
PDF lives and what to name it — the parts that must be right for the download
move to be reliable. The CLI (`asta papers …`) and the network fetch are
side-effecting and covered only by manual/integration runs, not here.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

# The skill script is not importable as a package (skills/ has no __init__), so
# load it by path — the same file the subagent runs in the sandbox.
_SCRIPT = Path(__file__).resolve().parents[1] / "skills" / "pdf_library" / "scripts" / "fetch_paper.py"
_spec = importlib.util.spec_from_file_location("fetch_paper", _SCRIPT)
assert _spec and _spec.loader
fetch_paper = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(fetch_paper)


# ---------------------------------------------------------------------------
# id vs title detection
# ---------------------------------------------------------------------------

def test_is_paper_id_recognizes_schemes() -> None:
    assert fetch_paper.is_paper_id("DOI:10.1101/2020.05.20")
    assert fetch_paper.is_paper_id("ARXIV:2005.14165")
    assert fetch_paper.is_paper_id("arxiv:2005.14165")  # case-insensitive
    assert fetch_paper.is_paper_id("CorpusId:218971783")
    assert fetch_paper.is_paper_id("PMID:19872477")


def test_is_paper_id_rejects_titles() -> None:
    assert not fetch_paper.is_paper_id("Yuyama coffea heat shock 2023")
    assert not fetch_paper.is_paper_id("Language Models are Few-Shot Learners")
    assert not fetch_paper.is_paper_id("")


# ---------------------------------------------------------------------------
# open-access URL resolution
# ---------------------------------------------------------------------------

def test_oa_url_prefers_open_access_pdf() -> None:
    record = {"openAccessPdf": {"url": "https://example.org/paper.pdf"},
              "externalIds": {"ArXiv": "2005.14165"}}
    assert fetch_paper.oa_url_from_record(record) == "https://example.org/paper.pdf"


def test_oa_url_falls_back_to_arxiv_when_oa_empty() -> None:
    # Mirrors the real observation: openAccessPdf.url is "" for arXiv:2005.14165.
    record = {"openAccessPdf": {"url": "", "status": None},
              "externalIds": {"ArXiv": "2005.14165", "CorpusId": 218971783}}
    assert fetch_paper.oa_url_from_record(record) == "https://arxiv.org/pdf/2005.14165"


def test_oa_url_none_when_no_source() -> None:
    assert fetch_paper.oa_url_from_record({"openAccessPdf": {"url": ""}, "externalIds": {}}) is None
    assert fetch_paper.oa_url_from_record({}) is None
    assert fetch_paper.oa_url_from_record(None) is None  # type: ignore[arg-type]


# ---------------------------------------------------------------------------
# record extraction across CLI output shapes
# ---------------------------------------------------------------------------

def test_first_record_handles_get_object() -> None:
    obj = {"paperId": "x", "title": "T", "externalIds": {}}
    assert fetch_paper._first_record(obj) is obj


def test_first_record_handles_search_list_and_envelope() -> None:
    rec = {"paperId": "x", "title": "T"}
    assert fetch_paper._first_record([rec]) is rec
    assert fetch_paper._first_record({"data": [rec]}) is rec
    assert fetch_paper._first_record({"data": []}) is None
    assert fetch_paper._first_record([]) is None


# ---------------------------------------------------------------------------
# filenames / destination paths
# ---------------------------------------------------------------------------

def test_slugify_sanitizes_and_bounds() -> None:
    assert fetch_paper.slugify("Coffea arabica: Heat-Shock!") == "coffea-arabica-heat-shock"
    assert fetch_paper.slugify("   ") == "paper"
    assert len(fetch_paper.slugify("x" * 200)) <= 80


def test_dest_path_uses_title_then_ref() -> None:
    p = fetch_paper.dest_path("./papers", {"title": "Denoeud 2014 Coffea Genome"}, "DOI:...")
    assert str(p) == "papers/denoeud-2014-coffea-genome.pdf"
    # No title ⇒ fall back to a slug of the reference.
    p2 = fetch_paper.dest_path("./papers", {}, "ARXIV:2005.14165")
    assert p2.name == "arxiv200514165.pdf"


# ---------------------------------------------------------------------------
# PDF sniffing
# ---------------------------------------------------------------------------

def test_looks_like_pdf_by_content_type_or_magic() -> None:
    assert fetch_paper.looks_like_pdf("application/pdf", b"")
    assert fetch_paper.looks_like_pdf("application/pdf; charset=binary", b"anything")
    assert fetch_paper.looks_like_pdf(None, b"%PDF-1.7\n...")
    assert not fetch_paper.looks_like_pdf("text/html", b"<!DOCTYPE html>")
    assert not fetch_paper.looks_like_pdf(None, b"<html>paywall</html>")

"""The bibliography renders whatever shape the client sent it in.

`POST /render-report` accepted `sources` as long as it was a *list*, and then read each entry as a
mapping. The desktop client sends bare citation strings, so the first report downloaded through it
came back `502 {"error": "PDF render failed: 'str' object has no attribute 'get'"}` — the whole
PDF lost to the reference list, with the report body itself perfectly renderable.

These tests pin the two shapes and the failure mode: a bad entry costs that entry, not the report.
"""

from __future__ import annotations

from backend.routes.rendering import _as_source, _build_typst_wrapper


CITATION = "Barrera, V. (2016). Pests and diseases affecting potato landraces."
LINK = "https://doi.org/10.1234/rlp.2016"


def test_the_web_clients_shape_keeps_its_link():
    assert _as_source({"citation": CITATION, "link": LINK}) == (CITATION, LINK)


def test_a_bare_citation_string_is_a_source():
    """What the desktop app sends. It has no link to give, and that is not an error."""
    assert _as_source(CITATION) == (CITATION, "")


def test_an_entry_with_nothing_to_cite_is_not_a_source():
    assert _as_source({"link": LINK}) is None
    assert _as_source({"citation": "   "}) is None
    assert _as_source("") is None


def test_an_entry_of_the_wrong_type_is_not_a_source():
    """Whatever it is, it is not a reference, and it must not take the report down with it."""
    assert _as_source(None) is None
    assert _as_source(42) is None
    assert _as_source(["Ames, M. (2010)."]) is None


def test_a_link_that_is_not_a_string_is_dropped_and_the_citation_kept():
    """A reference without its link is still a reference; `.strip()` on a dict is a 502."""
    assert _as_source({"citation": CITATION, "link": {"href": LINK}}) == (CITATION, "")


def test_string_sources_render_a_bibliography():
    document = _build_typst_wrapper(title="T", sources=[CITATION], used_asta=True)
    assert "= Sources" in document
    assert "Barrera" in document


def test_object_sources_render_the_link_as_well():
    document = _build_typst_wrapper(
        title="T", sources=[{"citation": CITATION, "link": LINK}], used_asta=True
    )
    assert f'#link("{LINK}")' in document


def test_one_unreadable_entry_costs_only_that_entry(caplog):
    document = _build_typst_wrapper(
        title="T", sources=[CITATION, 42, {"citation": "Ames, M. (2010)."}], used_asta=False
    )
    assert "Barrera" in document and "Ames" in document
    # Dropped in the open: a bibliography quietly one entry short is the failure this repository
    # spent `paper_tools.unreported` on, arriving through a different door.
    assert "dropping a source" in caplog.text


def test_no_sources_at_all_renders_no_bibliography():
    """Why this went a year undetected: with an empty list the loop never runs."""
    document = _build_typst_wrapper(title="T", sources=[], used_asta=False)
    assert "= Sources" not in document

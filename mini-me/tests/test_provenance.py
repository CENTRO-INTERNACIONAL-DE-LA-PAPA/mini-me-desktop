"""Contract tests for P4 provenance edges.

These pin the pure node-id convention (``artifact_node_id`` / ``paper_node_id``),
the deterministic edge derivation done by ``ArtifactCaptureMiddleware`` for each
subagent, and the ``edges`` merge behaviour of the ``_merge_artifacts`` reducer.
Everything is pure Python — no sandbox, no live Asta service — so a change to the
node-id keys, an edge relation, or the reducer's dedup keys is caught in CI.
"""

from __future__ import annotations

from types import SimpleNamespace

from backend.schemas import (
    DerivedRef,
    _merge_artifacts,
    artifact_node_id,
    declared_ref_node_id,
    paper_node_id,
)
from backend.middleware.artifacts import ArtifactCaptureMiddleware


# ---------------------------------------------------------------------------
# Node ids — must match the reducer's dedup keys so an edge endpoint always
# names the node the reducer keeps.
# ---------------------------------------------------------------------------

def test_artifact_node_id_per_kind() -> None:
    assert artifact_node_id("dataset", {"persistent_id": "doi:10/x", "title": "T"}) == "dataset:doi:10/x"
    assert artifact_node_id("source", {"link": "https://x", "citation": "C"}) == "source:https://x"
    assert artifact_node_id("report", {"title": "R"}) == "report:R"
    assert artifact_node_id("file", {"path": "./a.csv"}) == "file:./a.csv"
    assert artifact_node_id("hypothesis", {"question": "Q?"}) == "hypothesis:Q?"
    assert artifact_node_id("library", {"index_path": ".asta/documents"}) == "library:.asta/documents"
    assert artifact_node_id("analysis", {"question": "A?"}) == "analysis:A?"


def test_artifact_node_id_falls_back_to_next_field() -> None:
    # persistent_id missing/empty -> fall back to title.
    assert artifact_node_id("dataset", {"persistent_id": "", "title": "Only Title"}) == "dataset:Only Title"
    assert artifact_node_id("source", {"citation": "Just a citation"}) == "source:Just a citation"
    # Unknown kind or no identifying value -> stable but empty-valued id.
    assert artifact_node_id("mystery", {"foo": "bar"}) == "mystery:"


def test_paper_node_id_precedence() -> None:
    # url > doi > corpus_id > citation > title, all in the `source` namespace.
    assert paper_node_id({"url": "https://u", "doi": "10/x", "corpus_id": "9", "citation": "C"}) == "source:https://u"
    assert paper_node_id({"doi": "10/x", "corpus_id": "9", "citation": "C"}) == "source:https://doi.org/10/x"
    assert paper_node_id({"corpus_id": "9", "citation": "C"}) == "source:https://www.semanticscholar.org/paper/9"
    assert paper_node_id({"citation": "C"}) == "source:C"
    assert paper_node_id({"title": "A Doc"}) == "source:A Doc"
    # Bare citation string and object-attr access both work.
    assert paper_node_id("Plain citation") == "source:Plain citation"
    assert paper_node_id(SimpleNamespace(url="https://o", doi=None, corpus_id=None, citation="")) == "source:https://o"


def test_paper_node_coincides_with_source_node() -> None:
    # A theory's paper (by url) and a separately-searched Source (link == url)
    # must resolve to the same node so the graph links them.
    paper = paper_node_id({"citation": "P", "url": "https://doi.org/10/x"})
    source = artifact_node_id("source", {"link": "https://doi.org/10/x", "citation": "P"})
    assert paper == source


# ---------------------------------------------------------------------------
# Edge derivation — one branch per subagent, from that subagent's own output.
# ---------------------------------------------------------------------------

def _edges(source: str, structured: object) -> list[dict]:
    mw = ArtifactCaptureMiddleware(source=source)
    result = mw.after_agent({"structured_response": structured}, None)
    assert result is not None
    return result["artifacts"].get("edges", [])


def _paper(citation: str, **kw: object) -> SimpleNamespace:
    return SimpleNamespace(
        citation=citation,
        url=kw.get("url"),
        doi=kw.get("doi"),
        corpus_id=kw.get("corpus_id"),
    )


def test_hypothesis_edges_cite_and_contradict() -> None:
    theory = SimpleNamespace(
        laws=["X inhibits Y"],
        supporting_papers=[_paper("Smith 2020", url="https://a")],
        conflicting_papers=[_paper("Jones 2019", doi="10/j")],
        novelty_score=0.5,
    )
    structured = SimpleNamespace(question="Why X?", theories=[theory])
    edges = _edges("hypothesis_generator", structured)

    by_rel = {e["relation"]: e for e in edges}
    assert {"cites", "contradicted_by"} <= set(by_rel)  # (plus a produced_by edge)

    cites = by_rel["cites"]
    assert cites["source"] == "hypothesis:Why X?"
    assert cites["target"] == "source:https://a"
    assert cites["source_kind"] == "hypothesis"
    assert cites["target_kind"] == "source"
    assert cites["target_label"] == "Smith 2020"

    assert by_rel["contradicted_by"]["target"] == "source:https://doi.org/10/j"


def test_hypothesis_no_papers_yields_no_citation_edges() -> None:
    theory = SimpleNamespace(laws=["L"], supporting_papers=[], conflicting_papers=[], novelty_score=None)
    structured = SimpleNamespace(question="Q?", theories=[theory])
    edges = _edges("hypothesis_generator", structured)
    # No papers → no cites/contradicted_by edges (only the produced_by stamp).
    assert [e for e in edges if e["relation"] in ("cites", "contradicted_by")] == []


def test_library_edges_index_papers() -> None:
    structured = SimpleNamespace(
        action="index",
        summary="",
        paper_count=2,
        index_path=".asta/documents",
        query_hint="",
        papers=[
            SimpleNamespace(title="Paper A", path="./a.pdf", doi="10/a", summary=None, tags=[], page_count=None),
            SimpleNamespace(title="Paper B", path="./b.pdf", doi=None, summary=None, tags=[], page_count=None),
        ],
    )
    edges = [e for e in _edges("pdf_librarian", structured) if e["relation"] == "indexes"]
    assert len(edges) == 2
    assert all(e["source"] == "library:.asta/documents" for e in edges)
    targets = {e["target"] for e in edges}
    assert "source:https://doi.org/10/a" in targets  # doi wins
    assert "source:Paper B" in targets  # falls back to title


def test_analysis_edges_link_dataset_paths() -> None:
    structured = SimpleNamespace(
        question="What drives yield?",
        dataset_paths=["./data/yield.csv", ""],  # empty path is skipped
        summary="",
        findings=[],
        hypotheses_tested=[],
        charts=[],
        status="completed",
        task_id="t",
        context_id="c",
    )
    edges = [e for e in _edges("data_voyager", structured) if e["relation"] == "analyzes"]
    assert len(edges) == 1
    edge = edges[0]
    assert edge["source"] == "analysis:What drives yield?"
    assert edge["target"] == "file:./data/yield.csv"
    assert edge["relation"] == "analyzes"
    assert edge["target_label"] == "yield.csv"  # basename


# ---------------------------------------------------------------------------
# Reducer — edges accumulate and dedup by (source, target, relation).
# ---------------------------------------------------------------------------

def _edge(source: str, target: str, relation: str, **extra: object) -> dict:
    return {"source": source, "target": target, "relation": relation, **extra}


def test_merge_dedups_edges_by_triple() -> None:
    left = {"edges": [_edge("h:q", "s:p", "cites")]}
    right = {"edges": [_edge("h:q", "s:p", "cites"), _edge("h:q", "s:p2", "cites")]}
    merged = _merge_artifacts(left, right)
    triples = {(e["source"], e["target"], e["relation"]) for e in merged["edges"]}
    assert triples == {("h:q", "s:p", "cites"), ("h:q", "s:p2", "cites")}


def test_merge_edge_reemit_updates_in_place() -> None:
    # A running->completed hypothesis re-emits its edge; the label refreshes but
    # the edge does not duplicate.
    left = {"edges": [_edge("h:q", "s:p", "cites", target_label="old")]}
    right = {"edges": [_edge("h:q", "s:p", "cites", target_label="new")]}
    merged = _merge_artifacts(left, right)
    assert len(merged["edges"]) == 1
    assert merged["edges"][0]["target_label"] == "new"


def test_merge_without_edges_slice_is_safe() -> None:
    # Older bundles omit `edges` entirely; the reducer must not choke.
    merged = _merge_artifacts({"datasets": []}, {"sources": []})
    assert merged["edges"] == []


# ---------------------------------------------------------------------------
# produced_by — the subagent-identity axis (P4 follow-up part a).
# ---------------------------------------------------------------------------

def _produced_by(edges: list[dict]) -> list[dict]:
    return [e for e in edges if e["relation"] == "produced_by"]


def test_analysis_has_produced_by_edge() -> None:
    structured = SimpleNamespace(
        question="Q?", dataset_paths=[], summary="", findings=[], hypotheses_tested=[],
        charts=[], status="completed", task_id="t", context_id="c", derived_from=[],
    )
    [edge] = _produced_by(_edges("data_voyager", structured))
    assert edge["source"] == "analysis:Q?"
    assert edge["target"] == "subagent:data_voyager"
    assert edge["target_kind"] == "subagent"
    assert edge["target_label"] == "DataVoyager"


def test_each_subagent_stamps_its_producer() -> None:
    hyp = SimpleNamespace(question="H?", theories=[])
    assert _produced_by(_edges("hypothesis_generator", hyp))[0]["target"] == "subagent:hypothesis_generator"

    lib = SimpleNamespace(action="index", summary="", paper_count=0, index_path=".asta/documents", query_hint="", papers=[])
    assert _produced_by(_edges("pdf_librarian", lib))[0]["target"] == "subagent:pdf_librarian"

    rep = SimpleNamespace(title="R", markdown="# r", derived_from=[])
    assert _produced_by(_edges("report_writer", rep))[0]["source"] == "report:R"

    src = SimpleNamespace(sources=[SimpleNamespace(citation="C", relevance="x", link="https://a")])
    assert _produced_by(_edges("academic_researcher", src))[0]["target"] == "subagent:academic_researcher"


# ---------------------------------------------------------------------------
# Declared derived_from — the data-dependency axis (P4 follow-up part b).
# ---------------------------------------------------------------------------

def test_declared_ref_node_id_matches_convention() -> None:
    # A declared ref must resolve to the SAME id the upstream subagent produced.
    q = "Why does resistance vary?"
    assert declared_ref_node_id("hypothesis", q) == artifact_node_id("hypothesis", {"question": q})
    assert declared_ref_node_id("dataset", "./d.csv") == "dataset:./d.csv"
    assert declared_ref_node_id("paper", "Smith 2020") == paper_node_id({"citation": "Smith 2020"})
    assert declared_ref_node_id("analysis", "A?") == "analysis:A?"


def test_analysis_declares_tested_theory() -> None:
    structured = SimpleNamespace(
        question="Does R-gene predict resistance?", dataset_paths=[], summary="",
        findings=[], hypotheses_tested=[], charts=[], status="completed",
        task_id="t", context_id="c",
        derived_from=[DerivedRef(kind="hypothesis", ref="Why does resistance vary?", relation="tests")],
    )
    edges = _edges("data_voyager", structured)
    tests = [e for e in edges if e["relation"] == "tests"]
    assert len(tests) == 1
    assert tests[0]["source"] == "analysis:Does R-gene predict resistance?"
    # Resolves to the exact hypothesis node id so the graph joins them.
    assert tests[0]["target"] == "hypothesis:Why does resistance vary?"
    assert tests[0]["target_kind"] == "hypothesis"


def test_report_declares_synthesized_inputs() -> None:
    structured = SimpleNamespace(
        title="Blight synthesis", markdown="# r",
        derived_from=[
            DerivedRef(kind="analysis", ref="Does R-gene predict resistance?", relation="synthesizes"),
            DerivedRef(kind="paper", ref="Smith 2020", relation="synthesizes"),
        ],
    )
    edges = {(e["relation"], e["target"]) for e in _edges("report_writer", structured)}
    assert ("synthesizes", "analysis:Does R-gene predict resistance?") in edges
    assert ("synthesizes", "source:Smith 2020") in edges  # paper -> source namespace


def test_declared_ref_with_empty_fields_is_skipped() -> None:
    structured = SimpleNamespace(
        question="Q?", dataset_paths=[], summary="", findings=[], hypotheses_tested=[],
        charts=[], status="completed", task_id="t", context_id="c",
        derived_from=[DerivedRef(kind="", ref="x"), DerivedRef(kind="hypothesis", ref="")],
    )
    declared = [e for e in _edges("data_voyager", structured) if e["relation"] not in ("analyzes", "produced_by")]
    assert declared == []

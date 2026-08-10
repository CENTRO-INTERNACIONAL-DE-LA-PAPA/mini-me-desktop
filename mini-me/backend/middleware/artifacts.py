"""Middleware that exposes structured subagent output as frontend artifacts."""

import uuid
from pathlib import PurePosixPath
from typing import Any

from langgraph.runtime import Runtime
from langchain.agents.middleware import AgentMiddleware

from backend import paper_tools
from backend.schemas import (
    ArtifactState,
    artifact_node_id,
    declared_ref_node_id,
    paper_node_id,
)


def _paper_ref(paper: Any) -> dict[str, Any]:
    """Serialize a ``PaperRef`` (or a bare citation string) to a frontend dict."""
    if isinstance(paper, str):
        return {"citation": paper, "url": None, "doi": None, "corpus_id": None}
    return {
        "citation": getattr(paper, "citation", ""),
        "url": getattr(paper, "url", None),
        "doi": getattr(paper, "doi", None),
        "corpus_id": getattr(paper, "corpus_id", None),
    }


# ---------------------------------------------------------------------------
# Provenance edges (P4)
#
# Each helper derives edges from a *single* subagent's structured output —
# nothing is inferred across subagents and nothing is emitted when the source
# data is absent, so an edge is never fabricated. Node ids come from
# ``artifact_node_id`` / ``paper_node_id`` so endpoints match the artifacts the
# reducer keeps. ``*_kind``/``*_label`` make the graph self-describing.
# ---------------------------------------------------------------------------

_LABEL_MAX = 120


def _short_label(text: Any) -> str:
    label = str(text or "").strip().replace("\n", " ")
    return label if len(label) <= _LABEL_MAX else label[: _LABEL_MAX - 1] + "…"


def _node_has_value(node_id: str) -> bool:
    """True when a ``kind:value`` id has a non-empty value part."""
    _, _, value = node_id.partition(":")
    return value != ""


def _edge(
    source: str,
    target: str,
    relation: str,
    *,
    source_kind: str,
    target_kind: str,
    source_label: Any,
    target_label: Any,
) -> dict[str, Any] | None:
    """Build an edge, or ``None`` when either endpoint has no identifying value."""
    if not (_node_has_value(source) and _node_has_value(target)):
        return None
    return {
        "source": source,
        "target": target,
        "relation": relation,
        "source_kind": source_kind,
        "target_kind": target_kind,
        "source_label": _short_label(source_label),
        "target_label": _short_label(target_label),
    }


def _edges_for_hypothesis(structured: Any) -> list[dict[str, Any]]:
    """hypothesis --cites/contradicted_by--> paper, for every theory's refs."""
    question = getattr(structured, "question", "")
    src = artifact_node_id("hypothesis", {"question": question})
    src_label = question or "Theories"
    edges: list[dict[str, Any]] = []
    for theory in getattr(structured, "theories", []):
        for relation, attr in (
            ("cites", "supporting_papers"),
            ("contradicted_by", "conflicting_papers"),
        ):
            for paper in getattr(theory, attr, []):
                ref = _paper_ref(paper)
                edge = _edge(
                    src,
                    paper_node_id(ref),
                    relation,
                    source_kind="hypothesis",
                    target_kind="source",
                    source_label=src_label,
                    target_label=ref.get("citation") or "Paper",
                )
                if edge is not None:
                    edges.append(edge)
    return edges


def _edges_for_library(structured: Any) -> list[dict[str, Any]]:
    """library --indexes--> paper, for every indexed document."""
    index_path = getattr(structured, "index_path", ".asta/documents")
    src = artifact_node_id("library", {"index_path": index_path})
    edges: list[dict[str, Any]] = []
    for paper in getattr(structured, "papers", []):
        title = getattr(paper, "title", "")
        ref = {"doi": getattr(paper, "doi", None), "title": title}
        edge = _edge(
            src,
            paper_node_id(ref),
            "indexes",
            source_kind="library",
            target_kind="source",
            source_label="Library",
            target_label=title or "Document",
        )
        if edge is not None:
            edges.append(edge)
    return edges


def _edges_for_analysis(structured: Any) -> list[dict[str, Any]]:
    """analysis --analyzes--> file, for every dataset path it ran on."""
    question = getattr(structured, "question", "")
    src = artifact_node_id("analysis", {"question": question})
    edges: list[dict[str, Any]] = []
    for path in getattr(structured, "dataset_paths", []):
        if not path:
            continue
        name = PurePosixPath(str(path)).name or str(path)
        edge = _edge(
            src,
            artifact_node_id("file", {"path": path}),
            "analyzes",
            source_kind="analysis",
            target_kind="file",
            source_label=question or "Analysis",
            target_label=name,
        )
        if edge is not None:
            edges.append(edge)
    return edges


# Friendly display names for the subagent that produced an artifact.
_SUBAGENT_LABELS = {
    "hypothesis_generator": "Theorizer",
    "data_voyager": "DataVoyager",
    "pdf_librarian": "PDF Librarian",
    "report_writer": "Report writer",
    "academic_researcher": "Academic search",
    "dataverse_explorer": "Dataverse",
}


def _produced_by_edge(
    node_id: str, node_kind: str, node_label: Any, source_name: str
) -> dict[str, Any] | None:
    """artifact --produced_by--> subagent (identity axis, from `self.source`)."""
    return _edge(
        node_id,
        f"subagent:{source_name}",
        "produced_by",
        source_kind=node_kind,
        target_kind="subagent",
        source_label=node_label,
        target_label=_SUBAGENT_LABELS.get(source_name, source_name),
    )


def _declared_edges(
    source_id: str, source_kind: str, source_label: Any, derived_from: Any
) -> list[dict[str, Any]]:
    """Turn a subagent's declared ``derived_from`` refs into provenance edges.

    Resolution is deterministic (``declared_ref_node_id``); the *validation* that
    the target is a real artifact node happens at render time on the frontend, so
    a paraphrased/invented ref simply produces no visible edge.
    """
    edges: list[dict[str, Any]] = []
    for ref in derived_from or []:
        kind = str(getattr(ref, "kind", "") or "").strip()
        ref_val = str(getattr(ref, "ref", "") or "").strip()
        relation = str(getattr(ref, "relation", "") or "derived_from").strip()
        if not kind or not ref_val:
            continue
        target_kind = "source" if kind == "paper" else kind
        edge = _edge(
            source_id,
            declared_ref_node_id(kind, ref_val),
            relation,
            source_kind=source_kind,
            target_kind=target_kind,
            source_label=source_label,
            target_label=ref_val,
        )
        if edge is not None:
            edges.append(edge)
    return edges


class ArtifactCaptureMiddleware(AgentMiddleware[ArtifactState, Any, Any]):
    """Expose structured subagent outputs as frontend-friendly artifact state."""

    state_schema = ArtifactState

    def __init__(self, source: str | None = None):
        super().__init__()
        self.source = source

    def after_agent(self, state: ArtifactState, runtime: Runtime[Any]) -> dict[str, Any] | None:
        structured = state.get("structured_response")
        if structured is None:
            return None

        if self.source == "academic_researcher":
            sources = [
                {
                    "citation": source.citation,
                    "relevance": source.relevance,
                    "link": source.link,
                }
                for source in getattr(structured, "sources", [])
            ]
            # **Everything the search returned reaches the reader.** *"We should get all the
            # papers that asta finds and is up to the scietinst to selct and drop the ones they
            # want."* The subagent is asked for exactly that and still returns a shortlist — 9 of
            # 24 on the run that prompted this — so the papers it left out are added back here,
            # where the list leaves the backend and no model gets a further say.
            #
            # Appended rather than merged in, so the order the subagent chose still leads: its
            # ranking is genuinely useful, it just must not be subtractive.
            sources.extend(
                {
                    "citation": paper.get("citation") or paper.get("title", ""),
                    "relevance": "Returned by the search; not discussed in the summary.",
                    "link": paper.get("link", ""),
                }
                for paper in paper_tools.unreported(
                    paper_tools.papers_in(state.get("messages", [])), sources
                )
            )
            return {
                "artifacts": {
                    "datasets": [],
                    "sources": sources,
                    "reports": [],
                    "files": [],
                    # Over `sources`, not the structured response: a paper added back above is
                    # every bit as much this subagent's output, and an artifact with no
                    # provenance edge is one the research spine cannot account for.
                    "edges": [
                        e
                        for source in sources
                        if (
                            e := _produced_by_edge(
                                artifact_node_id("source", source),
                                "source",
                                source["citation"],
                                "academic_researcher",
                            )
                        )
                        is not None
                    ],
                }
            }

        if self.source == "dataverse_explorer":
            return {
                "artifacts": {
                    "datasets": [
                        {
                            "title": dataset.title,
                            "authors": list(dataset.authors),
                            "persistent_id": dataset.persistent_id,
                            "doi_url": dataset.doi_url,
                            "description": dataset.description,
                            "repository": dataset.repository,
                            "file_count": dataset.file_count,
                            "file_access_summary": dataset.file_access_summary,
                            "recommendation_reason": dataset.recommendation_reason,
                        }
                        for dataset in getattr(structured, "datasets", [])
                    ],
                    "sources": [],
                    "reports": [],
                    "files": [],
                    "edges": [
                        e
                        for dataset in getattr(structured, "datasets", [])
                        if (
                            e := _produced_by_edge(
                                artifact_node_id(
                                    "dataset",
                                    {
                                        "persistent_id": dataset.persistent_id,
                                        "title": dataset.title,
                                    },
                                ),
                                "dataset",
                                dataset.title,
                                "dataverse_explorer",
                            )
                        )
                        is not None
                    ],
                }
            }

        if self.source == "report_writer":
            return {
                "artifacts": {
                    "datasets": [],
                    "sources": [],
                    "reports": [
                        {
                            "title": structured.title,
                            "markdown": structured.markdown,
                        }
                    ],
                    "files": [],
                    "edges": [
                        e
                        for e in [
                            _produced_by_edge(
                                artifact_node_id("report", {"title": structured.title}),
                                "report",
                                structured.title,
                                "report_writer",
                            ),
                            *_declared_edges(
                                artifact_node_id("report", {"title": structured.title}),
                                "report",
                                structured.title,
                                getattr(structured, "derived_from", []),
                            ),
                        ]
                        if e is not None
                    ],
                }
            }

        if self.source == "hypothesis_generator":
            return {
                "artifacts": {
                    "datasets": [],
                    "sources": [],
                    "reports": [],
                    "files": [],
                    "hypotheses": [
                        {
                            "question": getattr(structured, "question", ""),
                            "theories": [
                                {
                                    "laws": list(getattr(theory, "laws", [])),
                                    "supporting_papers": [
                                        _paper_ref(paper)
                                        for paper in getattr(theory, "supporting_papers", [])
                                    ],
                                    "conflicting_papers": [
                                        _paper_ref(paper)
                                        for paper in getattr(theory, "conflicting_papers", [])
                                    ],
                                    "novelty_score": getattr(theory, "novelty_score", None),
                                }
                                for theory in getattr(structured, "theories", [])
                            ],
                            "knowledge_gaps": list(
                                getattr(structured, "knowledge_gaps", [])
                            ),
                            "papers_reviewed": getattr(structured, "papers_reviewed", 0),
                            "status": getattr(structured, "status", "completed"),
                            "task_id": getattr(structured, "task_id", None),
                        }
                    ],
                    "edges": _edges_for_hypothesis(structured)
                    + [
                        e
                        for e in [
                            _produced_by_edge(
                                artifact_node_id(
                                    "hypothesis",
                                    {"question": getattr(structured, "question", "")},
                                ),
                                "hypothesis",
                                getattr(structured, "question", ""),
                                "hypothesis_generator",
                            )
                        ]
                        if e is not None
                    ],
                }
            }

        if self.source == "pdf_librarian":
            return {
                "artifacts": {
                    "datasets": [],
                    "sources": [],
                    "reports": [],
                    "files": [],
                    "libraries": [
                        {
                            "action": getattr(structured, "action", "index"),
                            "summary": getattr(structured, "summary", ""),
                            "paper_count": getattr(structured, "paper_count", 0),
                            "index_path": getattr(
                                structured, "index_path", ".asta/documents"
                            ),
                            "papers": [
                                {
                                    "title": getattr(paper, "title", ""),
                                    "path": getattr(paper, "path", ""),
                                    "doi": getattr(paper, "doi", None),
                                    "summary": getattr(paper, "summary", None),
                                    "tags": list(getattr(paper, "tags", [])),
                                    "page_count": getattr(paper, "page_count", None),
                                }
                                for paper in getattr(structured, "papers", [])
                            ],
                            "query_hint": getattr(structured, "query_hint", ""),
                        }
                    ],
                    "edges": _edges_for_library(structured)
                    + [
                        e
                        for e in [
                            _produced_by_edge(
                                artifact_node_id(
                                    "library",
                                    {
                                        "index_path": getattr(
                                            structured, "index_path", ".asta/documents"
                                        )
                                    },
                                ),
                                "library",
                                "Library",
                                "pdf_librarian",
                            )
                        ]
                        if e is not None
                    ],
                }
            }

        if self.source == "data_voyager":
            return {
                "artifacts": {
                    "datasets": [],
                    "sources": [],
                    "reports": [],
                    "files": [],
                    "analyses": [
                        {
                            "question": getattr(structured, "question", ""),
                            "dataset_paths": list(
                                getattr(structured, "dataset_paths", [])
                            ),
                            "summary": getattr(structured, "summary", ""),
                            "findings": [
                                {
                                    "title": getattr(finding, "title", ""),
                                    "detail": getattr(finding, "detail", ""),
                                    "chart_path": getattr(finding, "chart_path", None),
                                }
                                for finding in getattr(structured, "findings", [])
                            ],
                            "hypotheses_tested": list(
                                getattr(structured, "hypotheses_tested", [])
                            ),
                            "charts": list(getattr(structured, "charts", [])),
                            "status": getattr(structured, "status", "completed"),
                            "task_id": getattr(structured, "task_id", None),
                            "context_id": getattr(structured, "context_id", None),
                        }
                    ],
                    "edges": _edges_for_analysis(structured)
                    + [
                        e
                        for e in [
                            _produced_by_edge(
                                artifact_node_id(
                                    "analysis",
                                    {"question": getattr(structured, "question", "")},
                                ),
                                "analysis",
                                getattr(structured, "question", ""),
                                "data_voyager",
                            )
                        ]
                        if e is not None
                    ]
                    + _declared_edges(
                        artifact_node_id(
                            "analysis", {"question": getattr(structured, "question", "")}
                        ),
                        "analysis",
                        getattr(structured, "question", ""),
                        getattr(structured, "derived_from", []),
                    ),
                }
            }

        if self.source == "research_planner":
            # Carry the freshly authored run-loop plan (P5) so
            # ``ProjectSpineMiddleware`` folds it into the active project's spine.
            # The ``nonce`` marks this one generation: the carrier lingers in
            # checkpoint state on later turns, and the spine middleware uses the
            # nonce to avoid re-folding it over the user's accepted/edited plan.
            steps = [
                {
                    "title": getattr(step, "title", ""),
                    "rationale": getattr(step, "rationale", ""),
                    "action": getattr(step, "action", ""),
                    "prompt": getattr(step, "prompt", ""),
                }
                for step in getattr(structured, "steps", [])
            ]
            if not steps:
                return None
            return {
                "artifacts": {
                    "datasets": [],
                    "sources": [],
                    "reports": [],
                    "files": [],
                    "plan": {
                        "goal": getattr(structured, "goal", ""),
                        "steps": steps,
                        "nonce": uuid.uuid4().hex,
                    },
                }
            }

        return None

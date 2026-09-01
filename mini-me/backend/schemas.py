"""Centralized data schemas for the backend.

This is the backend analogue of the frontend's ``types.ts``: every Pydantic
subagent-response model, the artifact ``TypedDict`` payloads, the artifact
graph-state slice, and the small artifact-path helpers live here so the rest of
the package shares one source of truth.

NOTE: this module deliberately does **not** use ``from __future__ import
annotations``. ``ArtifactState`` relies on ``Annotated[ArtifactBundle,
_merge_artifacts]`` being a real object at class-creation time so LangGraph can
discover the ``_merge_artifacts`` reducer; stringified annotations would break
artifact accumulation.
"""

from collections.abc import Mapping
from pathlib import PurePosixPath
from typing import Annotated, Any, List, Sequence

from langchain.agents.middleware import AgentState
from pydantic import BaseModel, Field, field_validator
from typing_extensions import NotRequired, TypedDict


# ---------------------------------------------------------------------------
# Academic research response models
# ---------------------------------------------------------------------------

class AcademicSourceFinding(BaseModel):
    """A concise academic source summary for frontend artifacts."""

    citation: str = Field(description="APA-style or equivalent citation for the source.")
    relevance: str = Field(description="Short explanation of why the source matters.")
    link: str | None = Field(
        default=None,
        description="DOI, arXiv, Semantic Scholar, or other stable source link.",
    )


class AcademicResearchResults(BaseModel):
    """Represents an academic research synthesis response."""

    summary: str = Field(description="Short summary of the evidence search outcome.")
    sources: List[AcademicSourceFinding] = Field(
        default_factory=list,
        description="Shortlisted sources to expose as structured artifacts.",
    )


# ---------------------------------------------------------------------------
# Hypothesis / theory response models
# ---------------------------------------------------------------------------

class PaperRef(BaseModel):
    """A paper backing or challenging a theory, with a link when resolvable."""

    citation: str = Field(
        description="Readable citation: title + authors + year.",
    )
    url: str | None = Field(
        default=None,
        description=(
            "Canonical link to the paper when one can be resolved: a DOI URL, a "
            "Semantic Scholar paper URL, or an arXiv abstract URL."
        ),
    )
    doi: str | None = Field(
        default=None,
        description="DOI (bare, without the https://doi.org/ prefix) when known.",
    )
    corpus_id: str | None = Field(
        default=None,
        description="Semantic Scholar corpus ID when known.",
    )


class Theory(BaseModel):
    """A single literature-grounded theory from the Asta Theorizer."""

    laws: List[str] = Field(
        default_factory=list,
        description="Short causal or mechanistic statements, e.g. 'X inhibits Y under condition Z'.",
    )
    supporting_papers: List[PaperRef] = Field(
        default_factory=list,
        description=(
            "Papers that support the theory. Each has a readable citation and, "
            "when resolvable, a link (url/doi/corpus_id)."
        ),
    )
    conflicting_papers: List[PaperRef] = Field(
        default_factory=list,
        description="Papers that contradict or complicate the theory, when any exist.",
    )
    novelty_score: float | None = Field(
        default=None,
        description="Novelty of the theory against the retrieved literature, 0-1 when available.",
    )

    @field_validator("supporting_papers", "conflicting_papers", mode="before")
    @classmethod
    def _coerce_paper_refs(cls, value: Any) -> Any:
        """Accept bare citation strings and wrap them as ``PaperRef``.

        The theorizer should emit structured references, but older runs and
        loosely-followed prompts may still return plain strings; coercing here
        keeps the pipeline (and any persisted artifacts) working.
        """
        if not isinstance(value, list):
            return value
        return [{"citation": item} if isinstance(item, str) else item for item in value]


class HypothesisOutput(BaseModel):
    """Represents a literature-grounded hypothesis-generation response."""

    question: str = Field(description="The research question the theories address.")
    theories: List[Theory] = Field(
        default_factory=list,
        description="Candidate theories with their laws and supporting evidence.",
    )
    knowledge_gaps: List[str] = Field(
        default_factory=list,
        description="Open questions or areas the literature does not yet settle.",
    )
    papers_reviewed: int = Field(
        default=0,
        description="Number of papers the pipeline retrieved and extracted from.",
    )
    status: str = Field(
        default="completed",
        description="Run lifecycle: 'running' (still generating), 'completed', or 'failed'.",
    )
    task_id: str | None = Field(
        default=None,
        description="Asta theorizer task id, used to poll a still-running run.",
    )


# ---------------------------------------------------------------------------
# PDF library response models
# ---------------------------------------------------------------------------

class IndexedPaper(BaseModel):
    """A single document indexed into the local Asta library."""

    title: str = Field(description="Human-readable document title.")
    path: str = Field(
        description="Sandbox path or URL where the document / extracted text lives.",
    )
    doi: str | None = Field(
        default=None,
        description="DOI or other stable identifier, when known.",
    )
    summary: str | None = Field(
        default=None,
        description="Short summary of the document contents used for retrieval.",
    )
    tags: List[str] = Field(
        default_factory=list,
        description="Topic tags attached to the document in the index.",
    )
    page_count: int | None = Field(
        default=None,
        description="Number of pages processed, when known.",
    )


class LibraryArtifact(BaseModel):
    """Represents the state of the local searchable PDF library."""

    action: str = Field(
        default="index",
        description="What the librarian did: 'index', 'extract', or 'search'.",
    )
    summary: str = Field(
        description="Short summary of what happened (papers indexed, matches found).",
    )
    paper_count: int = Field(
        default=0,
        description="Total number of documents currently in the library index.",
    )
    index_path: str = Field(
        default=".asta/documents",
        description="Path to the library index root in the sandbox.",
    )
    papers: List[IndexedPaper] = Field(
        default_factory=list,
        description=(
            "Documents relevant to this turn: the ones just indexed, or the "
            "search matches for a query."
        ),
    )
    query_hint: str = Field(
        default="Ask me to search this library for a topic.",
        description="Guidance for the user on how to query the library next.",
    )


# ---------------------------------------------------------------------------
# DataVoyager (analyze-data) response models
# ---------------------------------------------------------------------------

class DataFinding(BaseModel):
    """A single insight DataVoyager produced from analyzing the dataset."""

    title: str = Field(description="Short headline for the finding.")
    detail: str = Field(
        default="",
        description="1–2 sentence explanation of the finding and what supports it.",
    )
    chart_path: str | None = Field(
        default=None,
        description=(
            "Relative sandbox path to the chart/figure backing this finding, when "
            "one was produced (e.g. './analysis_corr.png')."
        ),
    )


class DerivedRef(BaseModel):
    """A declared upstream input a subagent built on (P4 provenance, "declared").

    The subagent names *what* it used by a natural key it already knows (a
    theory's research question, a dataset path/id, a paper citation); the capture
    middleware turns each ref into a provenance edge, and the frontend keeps it
    only if it resolves to a real artifact node — so a paraphrased/invented ref
    silently produces no edge (never a fabricated link).
    """

    kind: str = Field(
        description=(
            "Artifact kind of the input: 'hypothesis' (a theory set, keyed by its "
            "research question), 'dataset', 'source'/'paper', 'analysis', or 'file'."
        ),
    )
    ref: str = Field(
        description=(
            "The input's natural key, quoted EXACTLY so it matches the existing "
            "artifact: for a theory, its research question verbatim; for a dataset, "
            "its path or persistent id; for a paper, its citation; etc."
        ),
    )
    relation: str = Field(
        default="derived_from",
        description="How this output relates to the input: 'tests', 'synthesizes', or 'derived_from'.",
    )


class DiscoveryRunResults(BaseModel):
    """An Asta AutoDiscovery run this turn prepared.

    **The experiments are not in here, on purpose.** AutoDiscovery writes its own hypotheses,
    executes its own code and reports its own belief shifts; a model filling in a `findings` list
    for it would be authoring results it did not produce. So this artifact records the *run* — what
    was asked, of which data, at what budget — and the experiments arrive from the service through
    the poll route.

    It also exists before the run does. A drafted run has a `run_id` and has spent nothing:
    `status` is `awaiting_approval` until the researcher approves the budget in the app, because
    one credit per experiment comes out of a fixed grant.
    """

    name: str = Field(description="Short title for the run, as the researcher would name it.")
    run_id: str | None = Field(
        default=None,
        description="AutoDiscovery run id returned by the draft step, for polling and submitting.",
    )
    domain: str = Field(default="", description="Research field the run was framed in.")
    intent: str = Field(
        default="",
        description=(
            "How the exploration was steered — the one configuration field worth stating back to "
            "the researcher, because it is what they will want to change."
        ),
    )
    dataset_paths: List[str] = Field(
        default_factory=list,
        description="Local dataset path(s) uploaded for the run.",
    )
    n_experiments: int = Field(
        default=15,
        description="Experiments the run is configured for. One credit each.",
    )
    status: str = Field(
        default="awaiting_approval",
        description=(
            "Run lifecycle: 'awaiting_approval' (drafted, nothing spent), 'running', "
            "'completed', 'failed', or 'canceled'."
        ),
    )
    note: str = Field(
        default="",
        description=(
            "When `status` is 'failed', the tool's reason QUOTED VERBATIM — not summarised. "
            "A draft fails for reasons the researcher can act on (a file the run cannot see, a "
            "budget out of range), and a paraphrase loses the part that says what to do."
        ),
    )
    derived_from: List[DerivedRef] = Field(
        default_factory=list,
        description=(
            "Upstream artifacts the run was built on — normally the dataset "
            "(kind='dataset'/'file'). Leave empty when there is no upstream artifact."
        ),
    )


class DataAnalysisResults(BaseModel):
    """A DataVoyager (`asta analyze-data`) run: hypotheses tested against data."""

    question: str = Field(description="The analytical question DataVoyager answered.")
    dataset_paths: List[str] = Field(
        default_factory=list,
        description="Local dataset path(s) that were analyzed.",
    )
    summary: str = Field(
        default="",
        description="Short narrative synthesis of what the analysis found.",
    )
    findings: List[DataFinding] = Field(
        default_factory=list,
        description="Key insights, each optionally linked to a produced chart.",
    )
    hypotheses_tested: List[str] = Field(
        default_factory=list,
        description=(
            "Hypotheses the run generated and/or evaluated against the data, with "
            "the verdict where the evidence settled it."
        ),
    )
    charts: List[str] = Field(
        default_factory=list,
        description="Relative sandbox paths to charts/figures the run produced.",
    )
    status: str = Field(
        default="completed",
        description="Run lifecycle: 'running', 'completed', 'failed', or 'input-required'.",
    )
    task_id: str | None = Field(
        default=None,
        description="Asta DataVoyager task id, for resuming a still-running run.",
    )
    context_id: str | None = Field(
        default=None,
        description="DataVoyager session id, used to ask follow-ups against the same workspace.",
    )
    derived_from: List[DerivedRef] = Field(
        default_factory=list,
        description=(
            "Upstream artifacts this analysis was built on — e.g. the theory/theories "
            "it tested (kind='hypothesis', ref=the research question) and the dataset "
            "(kind='dataset'/'file'). Used to draw provenance edges; leave empty when "
            "there is no upstream artifact."
        ),
    )


# ---------------------------------------------------------------------------
# Artifact payloads + graph-state slice
# ---------------------------------------------------------------------------

class DatasetArtifactPayload(TypedDict):
    title: str
    authors: list[str]
    persistent_id: str
    recommendation_reason: str
    doi_url: NotRequired[str | None]
    description: NotRequired[str | None]
    repository: NotRequired[str | None]
    file_count: NotRequired[int | None]
    file_access_summary: NotRequired[str | None]


class SourceArtifactPayload(TypedDict):
    citation: str
    relevance: str
    link: NotRequired[str | None]


class ReportArtifactPayload(TypedDict):
    title: str
    markdown: str


class FileArtifactPayload(TypedDict):
    name: str
    path: str
    relative_path: NotRequired[str]
    media_type: NotRequired[str | None]
    description: NotRequired[str | None]


class PaperRefPayload(TypedDict):
    citation: str
    url: NotRequired[str | None]
    doi: NotRequired[str | None]
    corpus_id: NotRequired[str | None]


class TheoryPayload(TypedDict):
    laws: list[str]
    supporting_papers: list[PaperRefPayload]
    conflicting_papers: list[PaperRefPayload]
    novelty_score: NotRequired[float | None]


class HypothesisArtifactPayload(TypedDict):
    question: str
    theories: list[TheoryPayload]
    knowledge_gaps: list[str]
    papers_reviewed: int
    status: NotRequired[str]
    task_id: NotRequired[str | None]


class IndexedPaperPayload(TypedDict):
    title: str
    path: str
    doi: NotRequired[str | None]
    summary: NotRequired[str | None]
    tags: NotRequired[list[str]]
    page_count: NotRequired[int | None]


class LibraryArtifactPayload(TypedDict):
    action: str
    summary: str
    paper_count: int
    index_path: str
    papers: list[IndexedPaperPayload]
    query_hint: str


class DataFindingPayload(TypedDict):
    title: str
    detail: NotRequired[str]
    chart_path: NotRequired[str | None]


class DataAnalysisArtifactPayload(TypedDict):
    question: str
    dataset_paths: list[str]
    summary: str
    findings: list[DataFindingPayload]
    hypotheses_tested: list[str]
    charts: list[str]
    status: NotRequired[str]
    task_id: NotRequired[str | None]
    context_id: NotRequired[str | None]


class DiscoveryRunArtifactPayload(TypedDict):
    name: str
    run_id: NotRequired[str | None]
    domain: NotRequired[str]
    intent: NotRequired[str]
    dataset_paths: list[str]
    n_experiments: NotRequired[int]
    status: NotRequired[str]
    note: NotRequired[str]


class ProjectSuggestionPayload(TypedDict):
    title: str
    rationale: str
    action: str
    # Ready-to-send message that promotes the suggestion (P3.2). The frontend
    # drops it into the composer for review; it is never auto-sent.
    prompt: str


class PlanStepPayload(TypedDict):
    # One step of an AI-authored research plan (P5). ``id`` + ``status`` are
    # assigned deterministically by ``backend.plan`` after generation.
    id: str
    title: str
    rationale: str
    action: str
    prompt: str
    status: str  # "pending" | "active" | "done" | "skipped"


class ResearchPlanPayload(TypedDict):
    # The autonomous run-loop plan (P5): an ordered, human-reviewable sequence.
    # ``status`` is the plan lifecycle ("proposed" → awaiting accept/edit,
    # "active" → stepping through, "done" → every step done/skipped). ``nonce``
    # marks one generation so a lingering carrier slice is not re-folded over the
    # user's accepted/edited plan (see ``backend.project.advance_project``).
    goal: str
    status: str
    steps: list[PlanStepPayload]
    nonce: NotRequired[str]


class ProjectArtifactPayload(TypedDict):
    # The persistent research-project spine (work item C, phase 1). Advisory
    # only: ``suggestions`` are surfaced for the user to act on, never executed
    # automatically. ``plan`` (P5) is the opt-in autonomous run-loop plan, also
    # advisory: each step is run by the user, never automatically.
    mission: str
    completed: list[str]
    pending: list[str]
    suggestions: list[ProjectSuggestionPayload]
    plan: NotRequired[ResearchPlanPayload | None]


class ProvenanceEdgePayload(TypedDict):
    # A directed provenance edge (P4): ``source`` was derived from / built on
    # ``target``. Node ids come from ``artifact_node_id`` so an endpoint always
    # matches the artifact the reducer keeps. ``*_kind``/``*_label`` make the
    # graph self-describing — an endpoint that is not in any artifact slice
    # (e.g. a paper never separately searched) still renders as a labeled node.
    source: str
    target: str
    relation: str  # "cites" | "contradicted_by" | "indexes" | "analyzes"
    source_kind: NotRequired[str]
    target_kind: NotRequired[str]
    source_label: NotRequired[str]
    target_label: NotRequired[str]


class ArtifactBundle(TypedDict):
    datasets: list[DatasetArtifactPayload]
    sources: list[SourceArtifactPayload]
    reports: list[ReportArtifactPayload]
    files: list[FileArtifactPayload]
    # NotRequired so existing partial bundles (which omit these) stay valid;
    # the reducer reads every slice via .get(..., []).
    hypotheses: NotRequired[list[HypothesisArtifactPayload]]
    libraries: NotRequired[list[LibraryArtifactPayload]]
    analyses: NotRequired[list[DataAnalysisArtifactPayload]]
    discoveries: NotRequired[list[DiscoveryRunArtifactPayload]]
    # Single object, not a list: the reducer keeps the most recent one
    # (last-write-wins) rather than deduping/appending.
    project: NotRequired[ProjectArtifactPayload]
    # Transient carrier for a freshly generated run-loop plan (P5): the
    # ``research_planner`` subagent emits it here, and ``ProjectSpineMiddleware``
    # folds it into the active project's persisted spine. Single object,
    # last-write-wins; carries a ``nonce`` so a lingering slice is not re-folded.
    plan: NotRequired[ResearchPlanPayload]
    # Provenance graph (P4): directed edges linking each artifact to the inputs
    # it was derived from. Accumulates + dedups by (source, target, relation).
    edges: NotRequired[list[ProvenanceEdgePayload]]


def _dedupe_artifacts(
    items: list[dict[str, Any]],
    *,
    keys: Sequence[str],
) -> list[dict[str, Any]]:
    merged: dict[tuple[Any, ...], dict[str, Any]] = {}
    passthrough: list[dict[str, Any]] = []

    for item in items:
        key = tuple(item.get(field) for field in keys)
        if any(value not in (None, "") for value in key):
            merged[key] = {**merged.get(key, {}), **item}
        else:
            passthrough.append(item)

    return [*merged.values(), *passthrough]


def _merge_libraries(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Merge library artifacts by index, keeping every document rather than the latest slice.

    **Why this one cannot use `_dedupe_artifacts`.** Every library artifact carries the same
    ``index_path`` — ``.asta/documents`` is the default and nothing changes it — so they all collide
    on that key, and the shallow ``{**old, **new}`` merge replaces ``papers`` wholesale. The library
    is a *container*, and the other collections are not: a dataset artifact is one dataset, an
    analysis is one analysis, but a library artifact is a **slice of one growing thing**.
    ``LibraryArtifact.papers`` says so — *"Documents relevant to this turn: the ones just indexed,
    or the search matches for a query"*.

    So indexing a second paper made the first disappear from the client, while
    ``.asta/documents/index.yaml`` held both. Reported as *"after indexing, the first paper
    dissapeared thats weird"*, and it was: two papers on disk, one in the panel (docs §233).

    ``papers`` is unioned by ``path``, first occurrence winning so the order a researcher watched
    them arrive in is the order they keep. Scalars take the newest value — ``summary`` and
    ``action`` describe the latest turn, and ``paper_count`` is a statement about the index *now*,
    which is the one number that should be allowed to go down when a document is removed.
    """
    by_index: dict[str, dict[str, Any]] = {}
    order: list[str] = []
    for item in items:
        index_path = str(item.get("index_path") or "")
        if not index_path:
            continue
        if index_path not in by_index:
            by_index[index_path] = {**item, "papers": []}
            order.append(index_path)
        held = by_index[index_path]
        papers = list(held["papers"])
        seen = {str(paper.get("path") or "") for paper in papers}
        for paper in item.get("papers") or []:
            path = str(paper.get("path") or "")
            if path and path in seen:
                continue
            if path:
                seen.add(path)
            papers.append(paper)
        # Scalars from the newest, the accumulated documents from all of them.
        by_index[index_path] = {**held, **item, "papers": papers}
    return [by_index[index_path] for index_path in order]


def _merge_artifacts(
    left: ArtifactBundle | None,
    right: ArtifactBundle | None,
) -> ArtifactBundle:
    left = left or {"datasets": [], "sources": [], "reports": [], "files": []}
    right = right or {"datasets": [], "sources": [], "reports": [], "files": []}

    merged = {
        "datasets": _dedupe_artifacts(
            [*left.get("datasets", []), *right.get("datasets", [])],
            keys=("persistent_id", "title"),
        ),
        "sources": _dedupe_artifacts(
            [*left.get("sources", []), *right.get("sources", [])],
            keys=("link", "citation"),
        ),
        "reports": _dedupe_artifacts(
            [*left.get("reports", []), *right.get("reports", [])],
            keys=("title", "markdown"),
        ),
        "files": _dedupe_artifacts(
            [*left.get("files", []), *right.get("files", [])],
            keys=("path", "name"),
        ),
        "hypotheses": _dedupe_artifacts(
            [*left.get("hypotheses", []), *right.get("hypotheses", [])],
            keys=("question",),
        ),
        # Not `_dedupe_artifacts`: a library artifact is a slice of one growing thing, and every
        # one of them keys on the same `index_path`. See `_merge_libraries`.
        "libraries": _merge_libraries(
            [*left.get("libraries", []), *right.get("libraries", [])]
        ),
        "analyses": _dedupe_artifacts(
            [*left.get("analyses", []), *right.get("analyses", [])],
            # Keyed by question (like hypotheses) so a running→completed update for
            # the same question refreshes in place, while a follow-up (a distinct
            # question) accumulates as its own entry.
            keys=("question",),
        ),
        "discoveries": _dedupe_artifacts(
            [*left.get("discoveries", []), *right.get("discoveries", [])],
            # Keyed by the run id the service issued, not by the name: two runs over the same
            # data with the same title are two runs and two bills, while an
            # awaiting_approval→running→completed progression is one run updating in place.
            keys=("run_id",),
        ),
        "edges": _dedupe_artifacts(
            [*left.get("edges", []), *right.get("edges", [])],
            # One node-to-node relationship per (source, target, relation): a
            # re-emit of the same edge (e.g. a running→completed hypothesis
            # re-declaring its papers) collapses in place, while distinct edges
            # from any subagent accumulate.
            keys=("source", "target", "relation"),
        ),
    }

    # The project spine is a single object, not an accumulating list: keep the
    # most recent non-empty one so each turn's refreshed mission/suggestions
    # replace the previous snapshot instead of piling up.
    project = right.get("project") or left.get("project")
    if project is not None:
        merged["project"] = project

    # Same single-object rule for the run-loop plan carrier (P5): keep the most
    # recent generation; the ``nonce`` lets the spine middleware tell a fresh
    # plan from a carrier lingering in checkpoint state.
    plan = right.get("plan") or left.get("plan")
    if plan is not None:
        merged["plan"] = plan

    return merged


class ArtifactState(AgentState[Any]):
    """Graph state slice for structured UI artifacts."""

    artifacts: NotRequired[Annotated[ArtifactBundle, _merge_artifacts]]


# ---------------------------------------------------------------------------
# Provenance node ids (P4)
# ---------------------------------------------------------------------------

# Fields that identify each artifact kind, in precedence order. These mirror the
# dedup keys used by ``_merge_artifacts`` so a provenance edge endpoint always
# names the same node the reducer keeps. The frontend ``nodeId`` helper in
# ``lib/artifacts.ts`` mirrors this convention (kept in sync; the backend copy is
# unit-tested).
_NODE_ID_FIELDS: dict[str, tuple[str, ...]] = {
    "dataset": ("persistent_id", "title"),
    "source": ("link", "citation"),
    "report": ("title",),
    "file": ("path",),
    "hypothesis": ("question",),
    "library": ("index_path",),
    "analysis": ("question",),
    # Keyed on the run id the service issued, matching the `discoveries` reducer. Missing it meant
    # `artifact_node_id("discovery", …)` returned a bare `discovery:` and every provenance edge for
    # a run was silently dropped — the same "unknown kind" fallthrough this map's comment warns of.
    "discovery": ("run_id",),
}


def artifact_node_id(kind: str, payload: Mapping[str, Any]) -> str:
    """Return a stable ``kind:value`` node id for an artifact payload.

    ``value`` is the first non-empty identifying field for the kind (see
    ``_NODE_ID_FIELDS``), matching the reducer's dedup keys.
    """
    value = ""
    for field in _NODE_ID_FIELDS.get(kind, ()):  # unknown kind → "kind:"
        candidate = payload.get(field)
        if candidate not in (None, ""):
            value = str(candidate)
            break
    return f"{kind}:{value}"


def paper_node_id(ref: Any) -> str:
    """Return a node id for a paper, in the ``source`` namespace.

    Papers referenced by theories or indexed by the librarian share the source
    namespace so the same paper coincides with a separately-searched Source node.
    Accepts a ``PaperRef``/``IndexedPaper`` object, a serialized dict, or a bare
    citation string; keys by url → doi → corpus id → citation → title.
    """
    if isinstance(ref, str):
        return f"source:{ref}"
    get = ref.get if isinstance(ref, Mapping) else (lambda k: getattr(ref, k, None))
    url = get("url")
    if url:
        return f"source:{url}"
    doi = get("doi")
    if doi:
        return f"source:https://doi.org/{doi}"
    corpus_id = get("corpus_id")
    if corpus_id:
        return f"source:https://www.semanticscholar.org/paper/{corpus_id}"
    citation = get("citation")
    if citation:
        return f"source:{citation}"
    title = get("title")
    return f"source:{title or ''}"


def declared_ref_node_id(kind: str, ref: str) -> str:
    """Resolve a subagent-declared ``DerivedRef`` (kind + natural key) to a node id.

    Uses the same convention as ``artifact_node_id`` / ``paper_node_id`` so a
    declared ref lands on the exact node the upstream subagent produced (when the
    key was quoted correctly).
    """
    if kind in ("source", "paper"):
        return paper_node_id({"citation": ref})
    fields = _NODE_ID_FIELDS.get(kind)
    if fields:
        return artifact_node_id(kind, {fields[0]: ref})
    return f"{kind}:{ref}"


# ---------------------------------------------------------------------------
# Artifact-path helpers
# ---------------------------------------------------------------------------

def _normalize_artifact_path(base_dir: PurePosixPath, raw_path: str) -> str:
    raw = PurePosixPath(str(raw_path))
    if raw.is_absolute():
        return raw.as_posix()
    return (base_dir / raw).as_posix()


def _infer_artifact_description(path: str) -> str | None:
    suffix = PurePosixPath(path).suffix.lower()
    if suffix in {".png", ".jpg", ".jpeg", ".svg", ".pdf"}:
        return "Generated visualization or document."
    if suffix in {".csv", ".tsv", ".parquet", ".xlsx", ".xls", ".json", ".jsonl", ".ndjson"}:
        return "Generated data artifact."
    if suffix in {".md", ".txt", ".html"}:
        return "Generated report or text artifact."
    return None


def _is_supported_artifact_file(path: str) -> bool:
    suffix = PurePosixPath(path).suffix.lower()
    return suffix in {
        ".csv",
        ".tsv",
        ".parquet",
        ".xlsx",
        ".xls",
        ".json",
        ".jsonl",
        ".ndjson",
        ".md",
        ".txt",
        ".html",
        ".png",
        ".jpg",
        ".jpeg",
        ".svg",
        ".pdf",
        ".pkl",
        ".joblib",
    }


# ---------------------------------------------------------------------------
# Dataverse response models
# ---------------------------------------------------------------------------

class DataVerseFindings(BaseModel):
    """Represents the findings from a Dataverse search."""
    title: str = Field(description="Dataset title.")
    persistent_id: str = Field(
        description=(
            "The dataset's DOI or persistent identifier, **copied verbatim from the "
            "`global_id` of the search result you read**. Never compose one from a title, "
            "a URL, or from what you already know about CIP's collections: a reconstructed "
            "identifier looks exactly like a read one and a researcher pastes it into a "
            "paper. If the record carries no identifier, omit the dataset instead."
        )
    )
    doi_url: str | None = Field(default=None, description="Dataset DOI URL when available.")
    description: str | None = Field(default=None, description="Concise dataset description.")
    authors: List[str] = Field(default_factory=list, description="Dataset authors when available.")
    repository: str | None = Field(default=None, description="Repository or dataverse name.")
    collection_identifier: str | None = Field(default=None, description="Collection or dataverse identifier when available.")
    subjects: List[str] = Field(default_factory=list, description="Controlled subject terms.")
    keywords: List[str] = Field(default_factory=list, description="Dataset keywords.")
    related_publications: List[str] = Field(default_factory=list, description="Related publication citations when available.")
    file_count: int | None = Field(default=None, description="Dataset-level file count when available.")
    file_access_summary: str | None = Field(default=None, description="Short summary of file accessibility or restriction status.")
    recommendation_reason: str = Field(description="Why this dataset is relevant to the user's goal.")


class DataVerseSearchResults(BaseModel):
    """Represents a Dataverse discovery-and-selection response."""
    summary: str = Field(description="Short summary of the dataset search and recommendation outcome.")
    datasets: List[DataVerseFindings] = Field(default_factory=list, description="Shortlisted dataset recommendations.")
    doi_links: List[str] = Field(default_factory=list, description="DOI links for the shortlisted datasets.")


# ---------------------------------------------------------------------------
# Report writer response model
# ---------------------------------------------------------------------------

class ReportWriterOutput(BaseModel):
    """Represents a structured markdown report artifact."""

    title: str = Field(description="Short report title.")
    markdown: str = Field(description="Full markdown report body.")
    derived_from: List[DerivedRef] = Field(
        default_factory=list,
        description=(
            "Artifacts this report synthesizes — the theories, analyses, datasets, "
            "and sources it draws on (each with its natural key). Used to draw "
            "provenance edges; leave empty when the report cites no prior artifact."
        ),
    )


# ---------------------------------------------------------------------------
# Research planner response models (P5 — opt-in autonomous run loop)
# ---------------------------------------------------------------------------

class PlanStep(BaseModel):
    """One step in an AI-authored research plan: a single subagent action.

    The planner produces the ordered *content* only (title / rationale / which
    subagent / the routable prompt). Stable ids and lifecycle status are assigned
    deterministically afterwards by :func:`backend.plan.plan_from_output`, so the
    model never has to reason about them.
    """

    title: str = Field(
        description="Short imperative title, e.g. 'Search the literature for X'.",
    )
    rationale: str = Field(
        default="",
        description="One sentence on why this step advances the goal.",
    )
    action: str = Field(
        description=(
            "Which subagent runs this step, as a friendly label: one of "
            "'Academic Research', 'Dataverse Explorer', 'Data Cleaning', "
            "'Exploratory Data Analysis', 'Diagnostic Analytics', 'Predictive "
            "Analytics', 'Hypothesis Generator', 'PDF Librarian', 'DataVoyager', "
            "'AutoDiscovery', or 'Report Writer'."
        ),
    )
    prompt: str = Field(
        description=(
            "The ready-to-send message that runs this step, phrased 'Use the "
            "<subagent> subagent to …' so the coordinator routes it. The user "
            "reviews and sends it — it is NEVER run automatically."
        ),
    )


class ResearchPlan(BaseModel):
    """An ordered, human-reviewable plan for advancing the research mission.

    Advisory by construction: producing a plan runs nothing. The user accepts or
    edits it, then executes it one confirmed step at a time (P5).
    """

    goal: str = Field(
        default="",
        description="The overall goal this plan advances (usually the project mission).",
    )
    steps: List[PlanStep] = Field(
        default_factory=list,
        description=(
            "Ordered steps, each a single subagent action. Keep it tight — 3 to 7 "
            "steps — and sequence them the way a researcher actually would (evidence "
            "and data first, synthesis and reporting last)."
        ),
    )

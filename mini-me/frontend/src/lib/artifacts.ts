import type {
  ArtifactStatePayload,
  DataAnalysisArtifact,
  DatasetArtifact,
  FileArtifact,
  HypothesisArtifact,
  LibraryArtifact,
  PaperRef,
  PaperRefPayload,
  PlanStep,
  PlanStepStatus,
  ProjectArtifact,
  ProvenanceEdge,
  ReportArtifact,
  ResearchPlan,
  ResearchPlanPayload,
  ResearchPlanStatus,
  SourceArtifact,
} from "../types";

// Normalize a run-loop plan payload (P5) into the frontend shape, tolerating
// missing fields. Returns null when there is no usable plan so the run-loop
// panel simply doesn't render. Shared by the artifact stream and the /project
// client so both produce the same shape.
const _STEP_STATUSES: PlanStepStatus[] = ["pending", "active", "done", "skipped"];
const _PLAN_STATUSES: ResearchPlanStatus[] = ["proposed", "active", "done"];

export function normalizePlan(
  payload: ResearchPlanPayload | null | undefined,
): ResearchPlan | null {
  if (!payload || !Array.isArray(payload.steps) || payload.steps.length === 0) {
    return null;
  }
  const steps: PlanStep[] = payload.steps.map((s, i) => ({
    id: s.id || `s${i + 1}`,
    title: s.title ?? "",
    rationale: s.rationale ?? "",
    action: s.action ?? "",
    prompt: s.prompt ?? "",
    status: (_STEP_STATUSES.includes(s.status as PlanStepStatus)
      ? s.status
      : "pending") as PlanStepStatus,
  }));
  return {
    goal: payload.goal ?? "",
    status: (_PLAN_STATUSES.includes(payload.status as ResearchPlanStatus)
      ? payload.status
      : "proposed") as ResearchPlanStatus,
    steps,
  };
}

export interface NormalizedArtifacts {
  datasets: DatasetArtifact[];
  sources: SourceArtifact[];
  files: FileArtifact[];
  report: ReportArtifact | null;
  hypothesis: HypothesisArtifact | null;
  library: LibraryArtifact | null;
  analysis: DataAnalysisArtifact | null;
  project: ProjectArtifact | null;
  edges: ProvenanceEdge[];
}

// Stable provenance node id — mirrors `artifact_node_id` in backend/schemas.py
// (kept in sync; the backend copy is unit-tested). `value` is the first
// non-empty identifying field for the kind, matching the reducer's dedup keys.
const NODE_ID_FIELDS: Record<string, string[]> = {
  dataset: ["persistentId", "title"],
  source: ["link", "citation"],
  report: ["title"],
  file: ["path"],
  hypothesis: ["question"],
  library: ["indexPath"],
  analysis: ["question"],
};

export function nodeId(kind: string, artifact: Record<string, unknown>): string {
  let value = "";
  for (const field of NODE_ID_FIELDS[kind] ?? []) {
    const candidate = artifact[field];
    if (candidate !== undefined && candidate !== null && candidate !== "") {
      value = String(candidate);
      break;
    }
  }
  return `${kind}:${value}`;
}

// A paper node lives in the `source` namespace — mirrors `paper_node_id` in
// backend/schemas.py (url → doi → corpusId → citation → title).
export function paperNodeId(ref: {
  url?: string;
  doi?: string;
  corpusId?: string;
  citation?: string;
  title?: string;
}): string {
  if (ref.url) return `source:${ref.url}`;
  if (ref.doi) return `source:https://doi.org/${ref.doi}`;
  if (ref.corpusId) return `source:https://www.semanticscholar.org/paper/${ref.corpusId}`;
  if (ref.citation) return `source:${ref.citation}`;
  return `source:${ref.title ?? ""}`;
}

// Tolerate both the new structured PaperRef and legacy bare citation strings
// (older persisted hypotheses, or a run where the model returned plain strings).
function toPaperRef(raw: PaperRefPayload | string): PaperRef {
  if (typeof raw === "string") {
    return { citation: raw };
  }
  return {
    citation: raw.citation,
    url: raw.url ?? undefined,
    doi: raw.doi ?? undefined,
    corpusId: raw.corpus_id ?? undefined,
  };
}

export function normalizeArtifacts(
  artifacts: ArtifactStatePayload | undefined,
): NormalizedArtifacts {
  return {
    datasets: (artifacts?.datasets ?? []).map((dataset) => ({
      title: dataset.title,
      authors: dataset.authors,
      persistentId: dataset.persistent_id,
      doiUrl: dataset.doi_url,
      description: dataset.description ?? "No description available.",
      repository: dataset.repository ?? "Unknown repository",
      fileCount: dataset.file_count ?? undefined,
      accessSummary: dataset.file_access_summary ?? undefined,
      recommendationReason: dataset.recommendation_reason,
    })),
    sources: (artifacts?.sources ?? []).map((source) => ({
      citation: source.citation,
      relevance: source.relevance,
      link: source.link ?? undefined,
    })),
    files: (artifacts?.files ?? []).map((file) => ({
      name: file.name,
      path: file.path,
      relativePath: file.relative_path ?? undefined,
      mediaType: file.media_type ?? undefined,
      description: file.description ?? undefined,
    })),
    report: artifacts?.reports?.length
      ? artifacts.reports[artifacts.reports.length - 1]
      : null,
    hypothesis: artifacts?.hypotheses?.length
      ? (() => {
          const raw = artifacts.hypotheses![artifacts.hypotheses!.length - 1];
          return {
            question: raw.question,
            theories: (raw.theories ?? []).map((theory) => ({
              laws: theory.laws ?? [],
              supportingPapers: (theory.supporting_papers ?? []).map(toPaperRef),
              conflictingPapers: (theory.conflicting_papers ?? []).map(toPaperRef),
              noveltyScore: theory.novelty_score ?? undefined,
            })),
            knowledgeGaps: raw.knowledge_gaps ?? [],
            papersReviewed: raw.papers_reviewed ?? 0,
            status: raw.status ?? "completed",
            taskId: raw.task_id ?? undefined,
          };
        })()
      : null,
    library: artifacts?.libraries?.length
      ? (() => {
          const raw = artifacts.libraries![artifacts.libraries!.length - 1];
          return {
            action: raw.action ?? "index",
            summary: raw.summary ?? "",
            paperCount: raw.paper_count ?? 0,
            indexPath: raw.index_path ?? ".asta/documents",
            papers: (raw.papers ?? []).map((paper) => ({
              title: paper.title,
              path: paper.path,
              doi: paper.doi ?? undefined,
              summary: paper.summary ?? undefined,
              tags: paper.tags ?? [],
              pageCount: paper.page_count ?? undefined,
            })),
            queryHint: raw.query_hint ?? "",
          };
        })()
      : null,
    analysis: artifacts?.analyses?.length
      ? (() => {
          const raw = artifacts.analyses![artifacts.analyses!.length - 1];
          return {
            question: raw.question,
            datasetPaths: raw.dataset_paths ?? [],
            summary: raw.summary ?? "",
            findings: (raw.findings ?? []).map((finding) => ({
              title: finding.title,
              detail: finding.detail ?? "",
              chartPath: finding.chart_path ?? undefined,
            })),
            hypothesesTested: raw.hypotheses_tested ?? [],
            charts: raw.charts ?? [],
            status: raw.status ?? "completed",
            taskId: raw.task_id ?? undefined,
            contextId: raw.context_id ?? undefined,
          };
        })()
      : null,
    project: artifacts?.project
      ? {
          mission: artifacts.project.mission ?? "",
          completed: artifacts.project.completed ?? [],
          pending: artifacts.project.pending ?? [],
          suggestions: (artifacts.project.suggestions ?? []).map((s) => ({
            title: s.title,
            rationale: s.rationale,
            action: s.action,
            prompt: s.prompt ?? "",
          })),
          plan: normalizePlan(artifacts.project.plan),
        }
      : null,
    edges: (artifacts?.edges ?? []).map((edge) => ({
      source: edge.source,
      target: edge.target,
      relation: edge.relation,
      sourceKind: edge.source_kind ?? undefined,
      targetKind: edge.target_kind ?? undefined,
      sourceLabel: edge.source_label ?? undefined,
      targetLabel: edge.target_label ?? undefined,
    })),
  };
}

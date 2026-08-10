import type { BaseMessage } from "@langchain/core/messages";

export type TodoStatus = "pending" | "in_progress" | "completed";

export interface TodoItem {
  content?: string;
  title?: string;
  status: TodoStatus;
  description?: string;
}

export interface AgentState {
  [key: string]: unknown;
  messages: BaseMessage[];
  todos?: TodoItem[];
  artifacts?: ArtifactStatePayload;
}

export type SandboxState = "idle" | "preparing" | "ready" | "error";

export interface SandboxStatus {
  state: SandboxState;
  message: string;
}

export interface ThreadSummary {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  lastPrompt?: string;
}

export type SubagentStatus = "queued" | "running" | "completed" | "failed";

export type SubagentToolCallState = "pending" | "completed" | "error";

export interface SubagentToolCall {
  id: string;
  name: string;
  argsPreview?: string;
  state: SubagentToolCallState;
  resultPreview?: string;
}

export interface SubagentRun {
  id: string;
  name: string;
  label: string;
  status: SubagentStatus;
  task: string;
  elapsed: string;
  resultPreview?: string;
  resultIsStructured?: boolean;
  toolCalls?: SubagentToolCall[];
  latestActivity?: string;
  startedAt?: string;
  completedAt?: string;
}

export interface DatasetArtifact {
  title: string;
  authors: string[];
  persistentId: string;
  doiUrl?: string | null;
  description: string;
  repository: string;
  fileCount?: number;
  accessSummary?: string;
  recommendationReason: string;
}

export interface SourceArtifact {
  citation: string;
  relevance: string;
  link?: string;
}

export interface ReportArtifact {
  title: string;
  markdown: string;
}

export interface FileArtifact {
  name: string;
  path: string;
  relativePath?: string;
  mediaType?: string | null;
  description?: string | null;
}

export interface PaperRef {
  citation: string;
  url?: string;
  doi?: string;
  corpusId?: string;
}

export interface TheoryItem {
  laws: string[];
  supportingPapers: PaperRef[];
  conflictingPapers: PaperRef[];
  noveltyScore?: number;
}

export interface HypothesisArtifact {
  question: string;
  theories: TheoryItem[];
  knowledgeGaps: string[];
  papersReviewed: number;
  status?: string;
  taskId?: string;
}

export interface IndexedPaper {
  title: string;
  path: string;
  doi?: string;
  summary?: string;
  tags: string[];
  pageCount?: number;
}

export interface LibraryArtifact {
  action: string;
  summary: string;
  paperCount: number;
  indexPath: string;
  papers: IndexedPaper[];
  queryHint: string;
}

export interface DataFinding {
  title: string;
  detail: string;
  chartPath?: string;
}

export interface DataAnalysisArtifact {
  question: string;
  datasetPaths: string[];
  summary: string;
  findings: DataFinding[];
  hypothesesTested: string[];
  charts: string[];
  status?: string;
  taskId?: string;
  contextId?: string;
}

export interface ProjectSuggestion {
  title: string;
  rationale: string;
  action: string;
  // Ready-to-send message that promotes the suggestion — dropped into the
  // composer for the user to review and send. Never auto-sent (P3.2).
  prompt: string;
}

// One step of the autonomous run-loop plan (P5). `status` is assigned by the
// backend; the user runs each step (never auto-run).
export type PlanStepStatus = "pending" | "active" | "done" | "skipped";

export interface PlanStep {
  id: string;
  title: string;
  rationale: string;
  action: string;
  prompt: string;
  status: PlanStepStatus;
}

// The opt-in autonomous run-loop plan (P5): an ordered, human-reviewable
// sequence. `status` is the plan lifecycle: "proposed" (awaiting accept/edit),
// "active" (stepping through), "done".
export type ResearchPlanStatus = "proposed" | "active" | "done";

export interface ResearchPlan {
  goal: string;
  status: ResearchPlanStatus;
  steps: PlanStep[];
}

// The persistent research-project spine (work item C + P5). Advisory only:
// `suggestions` and each `plan` step are surfaced for the user to run, never
// executed automatically.
export interface ProjectArtifact {
  mission: string;
  completed: string[];
  pending: string[];
  suggestions: ProjectSuggestion[];
  plan: ResearchPlan | null;
}

// A named Project container that groups conversations (explicit Projects, P5).
// This is the registry record — its spine (mission/plan/…) is a ProjectArtifact.
export interface ProjectMeta {
  id: string;
  name: string;
  created_at: string;
  updated_at: string;
}

// A directed provenance edge (P4): `source` was derived from / built on
// `target`. `*Kind`/`*Label` make the graph self-describing so an endpoint that
// is not in any artifact slice still renders as a labeled node.
export interface ProvenanceEdge {
  source: string;
  target: string;
  relation: string;
  sourceKind?: string;
  targetKind?: string;
  sourceLabel?: string;
  targetLabel?: string;
}

export interface DatasetArtifactPayload {
  title: string;
  authors: string[];
  persistent_id: string;
  recommendation_reason: string;
  doi_url?: string | null;
  description?: string | null;
  repository?: string | null;
  file_count?: number | null;
  file_access_summary?: string | null;
}

export interface SourceArtifactPayload {
  citation: string;
  relevance: string;
  link?: string | null;
}

export interface ReportArtifactPayload {
  title: string;
  markdown: string;
}

export interface FileArtifactPayload {
  name: string;
  path: string;
  relative_path?: string | null;
  media_type?: string | null;
  description?: string | null;
}

export interface PaperRefPayload {
  citation: string;
  url?: string | null;
  doi?: string | null;
  corpus_id?: string | null;
}

export interface TheoryPayload {
  laws: string[];
  supporting_papers: (PaperRefPayload | string)[];
  conflicting_papers: (PaperRefPayload | string)[];
  novelty_score?: number | null;
}

export interface HypothesisArtifactPayload {
  question: string;
  theories: TheoryPayload[];
  knowledge_gaps: string[];
  papers_reviewed: number;
  status?: string;
  task_id?: string | null;
}

export interface IndexedPaperPayload {
  title: string;
  path: string;
  doi?: string | null;
  summary?: string | null;
  tags?: string[];
  page_count?: number | null;
}

export interface LibraryArtifactPayload {
  action: string;
  summary: string;
  paper_count: number;
  index_path: string;
  papers: IndexedPaperPayload[];
  query_hint: string;
}

export interface DataFindingPayload {
  title: string;
  detail?: string;
  chart_path?: string | null;
}

export interface DataAnalysisArtifactPayload {
  question: string;
  dataset_paths: string[];
  summary: string;
  findings: DataFindingPayload[];
  hypotheses_tested: string[];
  charts: string[];
  status?: string;
  task_id?: string | null;
  context_id?: string | null;
}

export interface ProjectSuggestionPayload {
  title: string;
  rationale: string;
  action: string;
  prompt: string;
}

export interface PlanStepPayload {
  id: string;
  title: string;
  rationale: string;
  action: string;
  prompt: string;
  status: string;
}

export interface ResearchPlanPayload {
  goal: string;
  status: string;
  steps: PlanStepPayload[];
  nonce?: string;
}

export interface ProjectArtifactPayload {
  mission: string;
  completed: string[];
  pending: string[];
  suggestions: ProjectSuggestionPayload[];
  plan?: ResearchPlanPayload | null;
}

export interface ProvenanceEdgePayload {
  source: string;
  target: string;
  relation: string;
  source_kind?: string;
  target_kind?: string;
  source_label?: string;
  target_label?: string;
}

export interface ArtifactStatePayload {
  datasets?: DatasetArtifactPayload[];
  sources?: SourceArtifactPayload[];
  reports?: ReportArtifactPayload[];
  files?: FileArtifactPayload[];
  hypotheses?: HypothesisArtifactPayload[];
  libraries?: LibraryArtifactPayload[];
  analyses?: DataAnalysisArtifactPayload[];
  project?: ProjectArtifactPayload;
  edges?: ProvenanceEdgePayload[];
}

export interface ThreadSessionSnapshot {
  threadId: string;
  messages: BaseMessage[];
  values?: Partial<AgentState>;
  subagents?: Map<string, unknown>;
  isLoading: boolean;
  error: unknown;
  sandboxStatus: SandboxStatus;
}

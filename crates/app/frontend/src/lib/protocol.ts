export interface AgentRef {
  ns: string;
  name: string;
}

export interface PendingAction {
  interrupt: string;
  tool: string;
  detail: string;
  description: string;
  allowed: string[];
}

export interface ApprovalRequest {
  actions: PendingAction[];
}

export interface Todo {
  content: string;
  status: string;
}

export interface Source {
  citation: string;
  link: string | null;
}

export interface Dataset {
  title: string;
  persistent_id: string;
  link: string | null;
  description: string;
  authors: string[];
  file_count: number | null;
  repository: string | null;
}

export interface Document {
  title: string;
  path: string;
  doi: string | null;
  summary: string;
  tags: string[];
  page_count: number | null;
}

export interface WrittenReport {
  title: string;
  markdown: string;
}

export type JobKind = "Theorizer" | "Analysis" | "Discovery";

export interface Job {
  kind: JobKind;
  task_id: string;
  question: string;
  context_id: string | null;
  status: string;
  size: number | null;
}

export interface Draft {
  run_id: string;
  name: string;
  intent: string;
  experiments: number;
  datasets: string[];
}

export interface AsyncTask {
  task_id: string;
  thread_id: string;
  agent_name: string;
  status: string;
  description: string;
  pending: ApprovalRequest | null;
  error: string | null;
  activity: string | null;
  todos: Todo[];
  owner: string;
}

export interface Suggestion {
  title: string;
  rationale: string;
  prompt: string;
}

export interface Project {
  mission: string;
  completed: string[];
  pending: string[];
  suggestions: Suggestion[];
}

export interface Bucket {
  name: string;
  items: string[];
}

export interface Snapshot {
  buckets: Bucket[];
  project: Project | null;
  jobs: Job[];
  drafts: Draft[];
  tasks: AsyncTask[];
  reports: WrittenReport[];
  todos: Todo[];
  datasets: Dataset[];
  documents: Document[];
  sources: Source[];
}

export type TurnEvent =
  | { type: "Status"; data: string }
  | { type: "Token"; data: string }
  | { type: "Step"; data: { agent: AgentRef | null; label: string } }
  | { type: "SubagentToken"; data: { agent: AgentRef; text: string } }
  | { type: "Approval"; data: ApprovalRequest }
  | { type: "Snapshot"; data: Snapshot }
  | { type: "Started"; data: { run_id: string } }
  | { type: "Done" }
  | { type: "Error"; data: string };

export type Decision = "Approve" | { Reject: { message: string } };

export interface Answer {
  interrupt: string;
  decision: Decision;
}

export interface Conversation {
  thread_id: string;
  project: string | null;
  title: string;
  updated_at: string;
}

export interface Adopted {
  conversations: Conversation[];
  scanned: boolean;
}

export type DeleteFiles =
  | { Conversation: { project: string | null; thread_id: string } }
  | { Project: { name: string } };

export interface DeleteOutcome {
  files_error: string | null;
}

export interface PendingAttachment {
  source: string;
  reference: string;
}

export type Started = "Attached" | "Spawned";

export interface GalleryListing {
  id: string;
  name: string;
  description: string;
  authors: string[];
  download_count: number;
  provides: string[];
  repository: string;
}

export interface Provider {
  id: string;
  label: string;
  needs_base_url: boolean;
  suggested_model: string;
  models: string[];
}

export interface Settings {
  provider: string;
  model_id: string;
  base_url: string;
  local_execution: boolean;
  approve_execute: boolean;
  backend_port: number;
  backend_dir: string;
  async_subagents: boolean;
  theme: string;
  subagents: Record<string, string>;
  adopted_untagged: boolean;
  sidebar_open: boolean;
  panel_open: boolean;
  road_open: boolean;
  backend_dir_owned: boolean;
  run_record: boolean;
}

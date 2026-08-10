import { getAuthToken } from "./fileClient";
import { LANGGRAPH_API_URL } from "./streamConfig";
import { normalizePlan } from "./artifacts";
import type {
  ProjectArtifact,
  ProjectArtifactPayload,
  ProjectMeta,
} from "../types";

// Hand-edits to the research-project spine (P3.3) + the run-loop plan (P5), plus
// the Project registry (create/list/rename/delete) and thread→project
// assignment. These hit the auth-protected /project(s) routes, which read/write
// the same LangGraph store the graph uses. Edits persist without a run; the next
// run picks them up via ProjectSpineMiddleware.

export interface ProjectEdit {
  mission?: string;
  pending_add?: string;
  pending_remove?: string;
  complete?: string;
}

// One edit to the run-loop plan (mirrors backend/plan.py `apply_plan_edit`).
export type PlanEdit =
  | { op: "accept" }
  | { op: "clear" }
  | { op: "complete" | "skip" | "activate" | "remove"; id: string }
  | { op: "edit"; id: string; title?: string; rationale?: string; action?: string; prompt?: string }
  | { op: "add"; after_id?: string; title: string; action?: string; prompt?: string; rationale?: string }
  | { op: "reorder"; order: string[] };

// Everything the Research project panel needs to manage explicit Projects + the
// P5 run loop. Bundled into one prop so it threads cleanly through
// AppShell → ArtifactPanel → ProjectPanel. Optional throughout: when absent, the
// panel falls back to its P3 advisory-only behavior.
export interface ProjectControls {
  projects: ProjectMeta[];
  activeProjectId: string | null;
  autopilot: boolean;
  onSelectProject: (id: string) => void;
  onCreateProject: (name: string) => void;
  onRenameProject: (id: string, name: string) => void;
  onDeleteProject: (id: string) => void;
  onToggleAutopilot: (on: boolean) => void;
  onPlanEdit: (op: PlanEdit) => void;
  onGeneratePlan: () => void;
}

function toProjectArtifact(payload: ProjectArtifactPayload): ProjectArtifact {
  return {
    mission: payload.mission ?? "",
    completed: payload.completed ?? [],
    pending: payload.pending ?? [],
    suggestions: (payload.suggestions ?? []).map((s) => ({
      title: s.title,
      rationale: s.rationale,
      action: s.action,
      prompt: s.prompt ?? "",
    })),
    plan: normalizePlan(payload.plan),
  };
}

// Optimistic mirror of the backend's `apply_project_edit` (backend/project.py)
// so the panel updates instantly; the server response then reconciles it, so
// any drift self-heals. Operates on the flat frontend shape (completed is a
// list here, keyed dict on the backend). The plan is server-authoritative (its
// edit machine is richer), so it is left untouched here.
export function applyProjectEditLocal(
  project: ProjectArtifact,
  edit: ProjectEdit,
): ProjectArtifact {
  let mission = project.mission;
  let pending = [...project.pending];
  let completed = [...project.completed];

  if (edit.mission !== undefined) {
    mission = edit.mission.replace(/\s+/g, " ").trim();
  }
  const add = edit.pending_add?.trim();
  if (add && !pending.includes(add)) pending.push(add);
  if (edit.pending_remove !== undefined) {
    pending = pending.filter((item) => item !== edit.pending_remove);
  }
  const done = edit.complete?.trim();
  if (done) {
    pending = pending.filter((item) => item !== done);
    if (!completed.includes(done)) completed.push(done);
  }

  return { ...project, mission, pending, completed };
}

async function authHeaders(extra?: Record<string, string>): Promise<Record<string, string>> {
  const token = await getAuthToken();
  const headers: Record<string, string> = { ...extra };
  if (token) headers.Authorization = `Bearer ${token}`;
  return headers;
}

function projectQuery(projectId?: string): string {
  return projectId ? `?project_id=${encodeURIComponent(projectId)}` : "";
}

// ---------------------------------------------------------------------------
// Project spine (mission / completed / pending / plan)
// ---------------------------------------------------------------------------

/** Load a project's persisted spine (mission + completed + pending + plan). */
export async function fetchProject(projectId?: string): Promise<ProjectArtifact | null> {
  const res = await fetch(`${LANGGRAPH_API_URL}/project${projectQuery(projectId)}`, {
    headers: await authHeaders(),
  });
  if (!res.ok) return null;
  return toProjectArtifact((await res.json()) as ProjectArtifactPayload);
}

/** Apply one spine hand-edit and return the updated project. Throws on failure. */
export async function patchProject(
  edit: ProjectEdit,
  projectId?: string,
): Promise<ProjectArtifact> {
  const res = await fetch(`${LANGGRAPH_API_URL}/project`, {
    method: "PATCH",
    headers: await authHeaders({ "Content-Type": "application/json" }),
    body: JSON.stringify({ ...edit, ...(projectId ? { project_id: projectId } : {}) }),
  });
  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    throw new Error(`Failed to update project (${res.status}) ${detail}`.trim());
  }
  return toProjectArtifact((await res.json()) as ProjectArtifactPayload);
}

/** Apply one run-loop plan edit (P5) and return the updated project. */
export async function patchPlan(op: PlanEdit, projectId?: string): Promise<ProjectArtifact> {
  const res = await fetch(`${LANGGRAPH_API_URL}/project`, {
    method: "PATCH",
    headers: await authHeaders({ "Content-Type": "application/json" }),
    body: JSON.stringify({ plan_op: op, ...(projectId ? { project_id: projectId } : {}) }),
  });
  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    throw new Error(`Failed to update plan (${res.status}) ${detail}`.trim());
  }
  return toProjectArtifact((await res.json()) as ProjectArtifactPayload);
}

// ---------------------------------------------------------------------------
// Project registry (explicit Projects) + thread assignment
// ---------------------------------------------------------------------------

/** List the caller's projects (the backend ensures a default exists). */
export async function fetchProjects(): Promise<ProjectMeta[]> {
  const res = await fetch(`${LANGGRAPH_API_URL}/projects`, {
    headers: await authHeaders(),
  });
  if (!res.ok) return [];
  const body = (await res.json()) as { projects?: ProjectMeta[] };
  return body.projects ?? [];
}

/** Create a new named project. */
export async function createProject(name: string): Promise<ProjectMeta> {
  const res = await fetch(`${LANGGRAPH_API_URL}/projects`, {
    method: "POST",
    headers: await authHeaders({ "Content-Type": "application/json" }),
    body: JSON.stringify({ name }),
  });
  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    throw new Error(`Failed to create project (${res.status}) ${detail}`.trim());
  }
  return (await res.json()) as ProjectMeta;
}

/** Rename an existing project. */
export async function renameProject(id: string, name: string): Promise<ProjectMeta> {
  const res = await fetch(`${LANGGRAPH_API_URL}/projects/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: await authHeaders({ "Content-Type": "application/json" }),
    body: JSON.stringify({ name }),
  });
  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    throw new Error(`Failed to rename project (${res.status}) ${detail}`.trim());
  }
  return (await res.json()) as ProjectMeta;
}

/** Delete a project (its spine goes with it). */
export async function deleteProject(id: string): Promise<void> {
  const res = await fetch(`${LANGGRAPH_API_URL}/projects/${encodeURIComponent(id)}`, {
    method: "DELETE",
    headers: await authHeaders(),
  });
  if (!res.ok && res.status !== 204) {
    const detail = await res.text().catch(() => "");
    throw new Error(`Failed to delete project (${res.status}) ${detail}`.trim());
  }
}

/** Record which Project a conversation belongs to (best-effort; never throws). */
export async function assignThreadProject(threadId: string, projectId: string): Promise<void> {
  try {
    await fetch(`${LANGGRAPH_API_URL}/threads/${encodeURIComponent(threadId)}/project`, {
      method: "PUT",
      headers: await authHeaders({ "Content-Type": "application/json" }),
      body: JSON.stringify({ project_id: projectId }),
    });
  } catch {
    // Assignment is a durability nicety; the client also passes project_id on
    // every run, so a failed write here is non-fatal.
  }
}

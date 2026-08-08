import type { SubagentRun, ThreadSummary } from "../types";

let _userScope: string = "anonymous";

/** Set the active user-scope used to namespace localStorage keys. */
export function setThreadScope(userId: string | null | undefined) {
  _userScope = userId && userId.length > 0 ? userId : "anonymous";
}

function activeKey() {
  return `deep_atd.activeThreadId.${_userScope}`;
}
function summariesKey() {
  return `deep_atd.threadSummaries.${_userScope}`;
}
function queuedMessagesKey() {
  return `deep_atd.queuedMessages.${_userScope}`;
}
function subagentCacheKey() {
  return `deep_atd.subagentCache.${_userScope}`;
}

function safeReadStorage(key: string) {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function safeWriteStorage(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Local storage can be unavailable in strict browser privacy modes.
  }
}

function safeRemoveStorage(key: string) {
  try {
    window.localStorage.removeItem(key);
  } catch {
    // Local storage can be unavailable in strict browser privacy modes.
  }
}

function isThreadSummary(value: unknown): value is ThreadSummary {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<ThreadSummary>;
  return (
    typeof candidate.id === "string" &&
    typeof candidate.title === "string" &&
    typeof candidate.createdAt === "string" &&
    typeof candidate.updatedAt === "string"
  );
}

function sortThreads(threads: ThreadSummary[]) {
  return [...threads].sort(
    (a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt),
  );
}

const PLACEHOLDER_TITLES = new Set([
  "New conversation",
  "Untitled research thread",
]);

interface ThreadMetadataLike {
  title?: unknown;
  last_prompt?: unknown;
}

interface ServerThreadLike {
  thread_id?: unknown;
  created_at?: unknown;
  updated_at?: unknown;
  metadata?: unknown;
}

export function promptToThreadTitle(prompt: string) {
  const compact = prompt.replace(/\s+/g, " ").trim();
  if (!compact) return "Untitled research thread";
  return compact.length > 70 ? `${compact.slice(0, 67)}...` : compact;
}

export function isPlaceholderThreadTitle(title: string | null | undefined) {
  return !title || PLACEHOLDER_TITLES.has(title);
}

export function normalizeThreadSummary(value: unknown): ThreadSummary | null {
  if (!value || typeof value !== "object") return null;
  const thread = value as ServerThreadLike;
  if (typeof thread.thread_id !== "string") return null;
  const metadata =
    thread.metadata && typeof thread.metadata === "object"
      ? (thread.metadata as ThreadMetadataLike)
      : {};
  const lastPrompt =
    typeof metadata.last_prompt === "string" && metadata.last_prompt.trim()
      ? metadata.last_prompt.trim()
      : undefined;
  const titleFromMetadata =
    typeof metadata.title === "string" && metadata.title.trim()
      ? metadata.title.trim()
      : undefined;
  const createdAt =
    typeof thread.created_at === "string" && thread.created_at
      ? thread.created_at
      : new Date().toISOString();
  const updatedAt =
    typeof thread.updated_at === "string" && thread.updated_at
      ? thread.updated_at
      : createdAt;
  return {
    id: thread.thread_id,
    title: titleFromMetadata ?? (lastPrompt ? promptToThreadTitle(lastPrompt) : "New conversation"),
    createdAt,
    updatedAt,
    lastPrompt,
  };
}

export function normalizeThreadSummaries(values: unknown[]): ThreadSummary[] {
  return sortThreads(values.map(normalizeThreadSummary).filter(Boolean) as ThreadSummary[]);
}

export function loadActiveThreadId() {
  return safeReadStorage(activeKey());
}

export function saveActiveThreadId(threadId: string) {
  safeWriteStorage(activeKey(), threadId);
}

export function clearActiveThreadId() {
  safeRemoveStorage(activeKey());
}

export function loadThreadSummaries() {
  const raw = safeReadStorage(summariesKey());
  if (!raw) return [];

  try {
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed) ? sortThreads(parsed.filter(isThreadSummary)) : [];
  } catch {
    return [];
  }
}

export function saveThreadSummaries(threads: ThreadSummary[]) {
  safeWriteStorage(summariesKey(), JSON.stringify(sortThreads(threads)));
}

export function upsertThreadSummary(
  threads: ThreadSummary[],
  threadId: string,
  prompt?: string,
) {
  const now = new Date().toISOString();
  const existing = threads.find((thread) => thread.id === threadId);
  const existingIsRealTitle =
    existing?.title && !PLACEHOLDER_TITLES.has(existing.title);
  const title = existingIsRealTitle
    ? (existing!.title)
    : prompt
      ? promptToThreadTitle(prompt)
      : (existing?.title ?? "New conversation");
  const updated: ThreadSummary = {
    id: threadId,
    title,
    createdAt: existing?.createdAt ?? now,
    updatedAt: now,
    lastPrompt: prompt ?? existing?.lastPrompt,
  };

  return sortThreads([
    updated,
    ...threads.filter((thread) => thread.id !== threadId),
  ]);
}

export function removeThreadSummary(
  threads: ThreadSummary[],
  threadId: string,
) {
  return threads.filter((thread) => thread.id !== threadId);
}

export function loadQueuedMessages(): Record<string, string> {
  const raw = safeReadStorage(queuedMessagesKey());
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: Record<string, string> = {};
    for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof value === "string" && value.length > 0) out[key] = value;
    }
    return out;
  } catch {
    return {};
  }
}

export function saveQueuedMessages(map: Record<string, string>) {
  safeWriteStorage(queuedMessagesKey(), JSON.stringify(map));
}

export function loadSubagentCache(): Record<string, SubagentRun[]> {
  const raw = safeReadStorage(subagentCacheKey());
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: Record<string, SubagentRun[]> = {};
    for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (Array.isArray(value)) out[key] = value as SubagentRun[];
    }
    return out;
  } catch {
    return {};
  }
}

export function saveSubagentCache(map: Record<string, SubagentRun[]>) {
  safeWriteStorage(subagentCacheKey(), JSON.stringify(map));
}

import { useAuth } from "@workos-inc/authkit-react";
import { Box } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { AppShell } from "./components/AppShell";
import { AuthGate } from "./components/AuthGate";
import { ConfirmModal } from "./components/ConfirmModal";
import {
  ThreadStreamSession,
  type ThreadSessionCommand,
} from "./components/ThreadStreamSession";
import { LANGGRAPH_API_URL } from "./lib/streamConfig";
import { normalizeArtifacts } from "./lib/artifacts";
import {
  applyProjectEditLocal,
  assignThreadProject,
  createProject,
  deleteProject,
  fetchProject,
  fetchProjects,
  patchPlan,
  patchProject,
  renameProject,
  type PlanEdit,
  type ProjectControls,
  type ProjectEdit,
} from "./lib/projectClient";
import { setFileAuthTokenGetter } from "./lib/fileClient";
import {
  getActiveProjectId,
  setActiveProjectId,
  setConfigAuthTokenGetter,
} from "./lib/llmConfig";
import { normalizeSubagents } from "./lib/subagents";
import { useStableCallback } from "./lib/useStableCallback";
import { useStableValue } from "./lib/useStableValue";
import {
  clearActiveThreadId,
  isPlaceholderThreadTitle,
  loadActiveThreadId,
  loadQueuedMessages,
  loadSubagentCache,
  loadThreadSummaries,
  normalizeThreadSummaries,
  promptToThreadTitle,
  removeThreadSummary,
  saveActiveThreadId,
  saveQueuedMessages,
  saveSubagentCache,
  saveThreadSummaries,
  setThreadScope,
  upsertThreadSummary,
} from "./lib/threads";
import type {
  ProjectArtifact,
  ProjectMeta,
  SandboxStatus,
  SubagentRun,
  ThreadSessionSnapshot,
} from "./types";

// Prompt that asks the coordinator to route to the research planner (P5). The
// user explicitly triggers this (Generate / Re-plan), so submitting it directly
// is the confirmation — it still runs nothing but the read-only planner.
const PLANNER_PROMPT =
  "Use the research_planner subagent to draft an ordered research plan that " +
  "advances this project's mission, building on what we've done so far.";

const DEFAULT_SANDBOX_STATUS: SandboxStatus = {
  state: "idle",
  message: "Local preview",
};

const ALLOWED_EMAIL_DOMAINS = (
  (import.meta.env.VITE_AUTH_ALLOWED_EMAIL_DOMAINS as string | undefined) ??
  "cgiar.org"
)
  .split(",")
  .map((domain) => domain.trim().toLowerCase())
  .filter(Boolean);

const ALLOWED_EMAIL_DOMAINS_LABEL = ALLOWED_EMAIL_DOMAINS.map((domain) => `@${domain}`).join(", ");

function isAllowedEmail(email: string | null | undefined): boolean {
  if (!email) return false;
  const normalized = email.trim().toLowerCase();
  return ALLOWED_EMAIL_DOMAINS.some((domain) => normalized.endsWith(`@${domain}`));
}

function makeCommandId() {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

interface ThreadSearchResponse {
  thread_id: string;
  created_at: string;
  updated_at: string;
  metadata?: Record<string, unknown>;
}

export function App() {
  const { user, isLoading: authLoading, signIn, signOut, getAccessToken } = useAuth();

  const userId = user?.id ?? null;
  setThreadScope(userId);

  const [accessToken, setAccessToken] = useState<string | null>(null);
  const [threadId, setThreadId] = useState<string | null>(() => loadActiveThreadId());
  const [threads, setThreads] = useState(() => loadThreadSummaries());
  const [threadsLoading, setThreadsLoading] = useState(false);
  const [threadsError, setThreadsError] = useState<string | null>(null);
  const [threadSessions, setThreadSessions] = useState<Record<string, ThreadSessionSnapshot>>({});
  const [threadCommands, setThreadCommands] = useState<Record<string, ThreadSessionCommand[]>>({});
  const [queuedMessages, setQueuedMessages] = useState<Record<string, string>>(() => loadQueuedMessages());
  const [subagentCache, setSubagentCache] = useState<Record<string, SubagentRun[]>>(() => loadSubagentCache());
  const [hasUserJustSubmitted, setHasUserJustSubmitted] = useState(false);
  // "Turn on sandbox" prompt for past chats whose generated files can't load.
  // `handledSandboxPrompts` remembers threads already prompted this session so
  // switching back and forth doesn't nag; `sandboxOverrides` reflects a resume
  // done over HTTP (no stream run → no sandbox_status event would arrive).
  const [sandboxPrompt, setSandboxPrompt] = useState<{
    threadId: string;
    busy: boolean;
    error: string | null;
    expired: boolean;
  } | null>(null);
  const [handledSandboxPrompts, setHandledSandboxPrompts] = useState<Set<string>>(
    () => new Set(),
  );
  const [sandboxOverrides, setSandboxOverrides] = useState<Record<string, SandboxStatus>>({});
  // Bumped after a successful sandbox resume so artifact images refetch.
  const [artifactEpoch, setArtifactEpoch] = useState(0);
  const [theme, setTheme] = useState<"light" | "dark">(() => {
    return (localStorage.getItem("theme") as "light" | "dark") || "light";
  });

  useEffect(() => {
    let cancelled = false;
    if (!user) {
      setAccessToken(null);
      return;
    }
    const syncToken = async () => {
      try {
        const token = await getAccessToken();
        if (cancelled) return;
        // Only swap state when the token string actually changed: the stream
        // SDK rebuilds its client whenever defaultHeaders gets a new identity,
        // so no-op refreshes shouldn't churn it.
        setAccessToken((prev) => (token !== prev ? (token ?? null) : prev));
      } catch {
        if (!cancelled) setAccessToken(null);
      }
    };
    void syncToken();
    // Keep the token fresh for the LangSmith-hosted backend: WorkOS access
    // tokens are short-lived, and useStream signs submits/reconnects with the
    // token captured in defaultHeaders — without this loop, a page left open
    // past the TTL starts 401ing. getAccessToken() is cheap when the cached
    // token is still valid, so a 60 s cadence costs nothing.
    const timer = setInterval(() => void syncToken(), 60_000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [user, getAccessToken]);

  useEffect(() => {
    setFileAuthTokenGetter(getAccessToken);
    setConfigAuthTokenGetter(getAccessToken);
  }, [getAccessToken]);

  useEffect(() => {
    setThreadId(loadActiveThreadId());
    setThreads(loadThreadSummaries());
    setThreadsLoading(false);
    setThreadsError(null);
    setThreadSessions({});
    setThreadCommands({});
    setQueuedMessages(loadQueuedMessages());
    setSubagentCache(loadSubagentCache());
    setHasUserJustSubmitted(false);
  }, [userId]);

  // Keyed on token *presence*, not the token value — token rotation from the
  // keep-fresh loop above must not refetch the whole conversation list.
  const hasAccessToken = accessToken !== null;
  useEffect(() => {
    if (!userId || !hasAccessToken) return;
    void refreshThreads();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasAccessToken, userId]);

  useEffect(() => {
    if (theme === "dark") {
      document.documentElement.classList.add("dark");
    } else {
      document.documentElement.classList.remove("dark");
    }
    localStorage.setItem("theme", theme);
  }, [theme]);

  const toggleTheme = useStableCallback(() => {
    setTheme((current) => (current === "light" ? "dark" : "light"));
  });

  const handleSignOut = useStableCallback(() => signOut());

  async function fetchWithAuth(url: string, init: RequestInit = {}): Promise<Response> {
    const token = await getAccessToken().catch(() => undefined);
    const headers = new Headers(init.headers ?? {});
    if (token) headers.set("Authorization", `Bearer ${token}`);
    return fetch(url, { ...init, headers });
  }

  function persistThreads(nextThreads: typeof threads) {
    setThreads(nextThreads);
    saveThreadSummaries(nextThreads);
  }

  async function refreshThreads() {
    setThreadsLoading(true);
    setThreadsError(null);
    try {
      const response = await fetchWithAuth(`${LANGGRAPH_API_URL}/threads/search`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          limit: 200,
          offset: 0,
          sort_by: "updated_at",
          sort_order: "desc",
        }),
      });
      if (!response.ok) {
        throw new Error(`Failed to load conversations (${response.status})`);
      }
      const data = (await response.json()) as ThreadSearchResponse[];
      persistThreads(normalizeThreadSummaries(data));
    } catch (error) {
      setThreadsError(
        error instanceof Error ? error.message : "Failed to load conversations.",
      );
    } finally {
      setThreadsLoading(false);
    }
  }

  async function patchThreadMetadata(
    targetThreadId: string,
    metadata: Record<string, string>,
  ) {
    const response = await fetchWithAuth(
      `${LANGGRAPH_API_URL}/threads/${encodeURIComponent(targetThreadId)}`,
      {
        method: "PATCH",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ metadata }),
      },
    );
    if (!response.ok) {
      throw new Error(`Failed to update conversation metadata (${response.status})`);
    }
  }

  function registerThread(newThreadId: string, prompt?: string) {
    saveActiveThreadId(newThreadId);
    setThreadId(newThreadId);
    persistThreads(upsertThreadSummary(threads, newThreadId, prompt));
  }

  function enqueueThreadCommand(targetThreadId: string, command: Omit<ThreadSessionCommand, "id">) {
    const nextCommand: ThreadSessionCommand = {
      id: makeCommandId(),
      ...command,
    };
    setThreadCommands((prev) => ({
      ...prev,
      [targetThreadId]: [...(prev[targetThreadId] ?? []), nextCommand],
    }));
  }

  const handleCommandProcessed = useCallback(
    (targetThreadId: string, commandIds: string[]) => {
      setThreadCommands((prev) => {
        const existing = prev[targetThreadId] ?? [];
        const remaining = existing.filter((command) => !commandIds.includes(command.id));
        if (remaining.length === existing.length) return prev;
        if (remaining.length === 0) {
          const next = { ...prev };
          delete next[targetThreadId];
          return next;
        }
        return {
          ...prev,
          [targetThreadId]: remaining,
        };
      });
    },
    [],
  );

  const handleSnapshotChange = useCallback((snapshot: ThreadSessionSnapshot) => {
    setThreadSessions((prev) => ({
      ...prev,
      [snapshot.threadId]: snapshot,
    }));
    // Persist a JSON-safe snapshot of subagent progress per thread so a
    // mid-run refresh keeps the cards visible until the live stream catches up.
    // Skip empty writes so we don't clobber the cache while useStream is still
    // hydrating.
    const normalizedSubagents = normalizeSubagents(snapshot.subagents);
    if (normalizedSubagents.length > 0) {
      setSubagentCache((prev) => {
        const next = { ...prev, [snapshot.threadId]: normalizedSubagents };
        saveSubagentCache(next);
        return next;
      });
    }
  }, []);

  async function createThread(prompt?: string): Promise<string> {
    // Tag the LangGraph thread itself with the user so backend tooling and
    // LangSmith filters can scope conversations to a specific researcher.
    // PATCH /threads merges metadata, so subsequent title/last_prompt patches
    // won't overwrite these fields.
    const threadMetadata: Record<string, string> = {};
    if (user?.id) threadMetadata.user_id = user.id;
    if (user?.email) threadMetadata.user_email = user.email;
    const response = await fetchWithAuth(`${LANGGRAPH_API_URL}/threads`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(
        Object.keys(threadMetadata).length > 0 ? { metadata: threadMetadata } : {},
      ),
    });
    if (!response.ok) {
      throw new Error(`Failed to create thread (${response.status})`);
    }
    const data = (await response.json()) as { thread_id: string };
    registerThread(data.thread_id, prompt);
    return data.thread_id;
  }

  const defaultHeaders = useMemo(
    () => (accessToken ? { Authorization: `Bearer ${accessToken}` } : undefined),
    [accessToken],
  );

  const activeSession = threadId ? threadSessions[threadId] : undefined;
  const activeThreadTitle =
    threads.find((thread) => thread.id === threadId)?.title ?? null;
  const activeCommandCount = threadId ? (threadCommands[threadId]?.length ?? 0) : 0;
  // The stream snapshot rebuilds `values`/`subagents` with fresh identities on
  // every tick, so the arrays derived from them below are pinned to their
  // previous reference while their content is unchanged. That keeps the
  // memoized sidebar panels (threads, todos, subagents, artifacts) from
  // re-rendering on every streamed token.
  const todosRaw = activeSession?.values?.todos ?? [];
  const todos = useStableValue(todosRaw, JSON.stringify(todosRaw));
  const subagentsRaw = useMemo(() => {
    const live = normalizeSubagents(activeSession?.subagents);
    if (!threadId) return live;
    const cached = subagentCache[threadId] ?? [];
    if (cached.length === 0) return live;
    const liveIds = new Set(live.map((s) => s.id));
    return [...live, ...cached.filter((s) => !liveIds.has(s.id))];
  }, [activeSession?.subagents, threadId, subagentCache]);
  const subagents = useStableValue(subagentsRaw, JSON.stringify(subagentsRaw));
  const artifactsRaw = normalizeArtifacts(activeSession?.values?.artifacts);
  const artifacts = useStableValue(artifactsRaw, JSON.stringify(artifactsRaw));

  // Explicit Projects (P5): a named Project groups conversations and owns its
  // own mission + run-loop plan. `activeProjectId` is the current selection
  // (persisted in localStorage and passed on every run so the spine is scoped
  // to it); the registry drives the switcher.
  const [projects, setProjects] = useState<ProjectMeta[]>([]);
  const [activeProjectId, setActiveProjectIdState] = useState<string | null>(
    () => getActiveProjectId(),
  );
  const [autopilot, setAutopilot] = useState<boolean>(
    () => localStorage.getItem("atd:autopilot") === "1",
  );
  useEffect(() => {
    // Gate on auth readiness (like the threads fetch): running before the token
    // is available 401s and would leave the registry empty, so the switcher
    // would fall back to a phantom "My research" that vanishes once a real
    // project is created.
    if (!userId || !hasAccessToken) return;
    let cancelled = false;
    fetchProjects()
      .then((list) => {
        if (cancelled) return;
        setProjects(list);
        // Pick a valid active project: the remembered one if it still exists,
        // else the default/first. Persist so runs carry it.
        const remembered = getActiveProjectId();
        const valid = remembered && list.some((p) => p.id === remembered);
        const next = valid ? remembered : (list[0]?.id ?? null);
        setActiveProjectId(next);
        setActiveProjectIdState(next);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [hasAccessToken, userId]);

  // Research-project spine (P3.3 + P5). The graph-state project (from the
  // checkpoint / stream) is authoritative after each run — it already reflects
  // any persisted hand-edits/plan, since ProjectSpineMiddleware read the store.
  // Between runs, hand-edits, plan edits, project switches, and a fresh-open
  // load live in `localProject`; the effective `project` prefers it.
  const graphProject = artifacts.project;
  const [localProject, setLocalProject] = useState<ProjectArtifact | null>(null);
  useEffect(() => {
    if (graphProject) setLocalProject(graphProject);
  }, [graphProject]);
  useEffect(() => {
    // Load the active Project's persisted spine (mission/backlog/plan) so it
    // shows without needing a run. Runs into `localProject`; a subsequent run's
    // graphProject (scoped to the same project) then supersedes it. Gated on
    // auth so it runs once the token is ready (activeProjectId may be restored
    // from localStorage before auth).
    if (!activeProjectId || !hasAccessToken) return;
    let cancelled = false;
    fetchProject(activeProjectId)
      .then((loaded) => {
        if (cancelled) return;
        // Empty spine still replaces stale state when switching projects.
        setLocalProject(loaded ?? { mission: "", completed: [], pending: [], suggestions: [], plan: null });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [activeProjectId, hasAccessToken]);
  const project = localProject ?? graphProject ?? null;

  const emptyProjectArtifact: ProjectArtifact = {
    mission: "",
    completed: [],
    pending: [],
    suggestions: [],
    plan: null,
  };

  const handleEditProject = useStableCallback(async (edit: ProjectEdit) => {
    const base = localProject ?? graphProject ?? emptyProjectArtifact;
    // Optimistic: reflect the edit immediately, then reconcile with the server
    // (which owns completed-item keying and returns authoritative state).
    setLocalProject(applyProjectEditLocal(base, edit));
    try {
      const updated = await patchProject(edit, activeProjectId ?? undefined);
      // The route can't derive live suggestions — keep whatever we have.
      setLocalProject((prev) => ({
        ...updated,
        suggestions: prev?.suggestions ?? updated.suggestions,
      }));
    } catch {
      const server = await fetchProject(activeProjectId ?? undefined).catch(() => null);
      if (server) setLocalProject(server);
    }
  });
  const activeIsLoading = activeSession?.isLoading ?? activeCommandCount > 0;
  const activeError = activeSession?.error;
  // An HTTP resume can't be observed through the stream, so its override wins
  // until the next run (handleSubmit clears it) lets stream events take over.
  const sandboxStatus =
    (threadId ? sandboxOverrides[threadId] : undefined) ??
    activeSession?.sandboxStatus ??
    DEFAULT_SANDBOX_STATUS;

  const runningThreadIdsRaw = Object.values(threadSessions)
    .filter((session) => session.isLoading)
    .map((session) => session.threadId);
  const runningThreadIds = useStableValue(
    runningThreadIdsRaw,
    runningThreadIdsRaw.join("␞"),
  );
  const backgroundThreadTitles = threads
    .filter((thread) => thread.id !== threadId && runningThreadIds.includes(thread.id))
    .map((thread) => thread.title);
  const hasBackgroundRun = backgroundThreadTitles.length > 0;
  const backgroundRunThreadTitle =
    backgroundThreadTitles.length === 1
      ? backgroundThreadTitles[0]
      : backgroundThreadTitles.length > 1
        ? `${backgroundThreadTitles.length} other conversations`
        : null;

  const mountedThreadIds = useMemo(() => {
    const ids = new Set<string>();
    if (threadId) ids.add(threadId);
    for (const session of Object.values(threadSessions)) {
      if (session.isLoading) ids.add(session.threadId);
    }
    for (const [id, commands] of Object.entries(threadCommands)) {
      if (commands.length > 0) ids.add(id);
    }
    return Array.from(ids);
  }, [threadCommands, threadId, threadSessions]);

  useEffect(() => {
    if (!threadId) return;
    if (threads.some((thread) => thread.id === threadId)) return;
    persistThreads(upsertThreadSummary(threads, threadId));
  }, [threadId, threads]);

  useEffect(() => {
    if (activeSession?.messages.length) {
      setHasUserJustSubmitted(false);
    }
  }, [activeSession?.messages.length]);

  useEffect(() => {
    if (!activeIsLoading && activeCommandCount === 0) {
      setHasUserJustSubmitted(false);
    }
  }, [activeCommandCount, activeIsLoading]);

  // Auto-flush the queued message for the active thread once the run settles.
  // 250 ms debounce avoids racing the tail of the stream-end transition, and
  // the cleanup cancels cleanly if the user edits/cancels in that window.
  useEffect(() => {
    if (!threadId) return;
    const queued = queuedMessages[threadId];
    if (!queued) return;
    if (activeIsLoading) return;
    if (activeCommandCount > 0) return;
    const timer = setTimeout(() => {
      setQueuedMessages((prev) => {
        if (!(threadId in prev)) return prev;
        const next = { ...prev };
        delete next[threadId];
        saveQueuedMessages(next);
        return next;
      });
      void handleSubmit(queued);
    }, 250);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [threadId, activeIsLoading, activeCommandCount, queuedMessages]);

  // Offer to turn the sandbox back on when a past chat has generated files
  // (images, reports, data files live in the per-thread sandbox) but the
  // sandbox is off, so those resources would 404. Prompt once per thread per
  // session; artifacts arrive async from the history fetch, which is why this
  // watches state instead of firing directly in handleSelectThread.
  useEffect(() => {
    if (!threadId) return;
    if (activeIsLoading || activeCommandCount > 0) return;
    if (sandboxStatus.state !== "idle") return;
    if (artifacts.files.length === 0) return;
    if (handledSandboxPrompts.has(threadId)) return;
    setHandledSandboxPrompts((prev) => new Set(prev).add(threadId));
    setSandboxPrompt({ threadId, busy: false, error: null, expired: false });
  }, [
    threadId,
    activeIsLoading,
    activeCommandCount,
    sandboxStatus.state,
    artifacts.files.length,
    handledSandboxPrompts,
  ]);

  // Handlers below are passed to memoized children (ThreadPanel,
  // ArtifactPanel, …); useStableCallback keeps their identity fixed across
  // renders so React.memo can actually skip work during streaming.
  const handleSubmit = useStableCallback(async (content: string) => {
    let targetThreadId = threadId;
    if (!targetThreadId) {
      targetThreadId = await createThread(content);
    } else {
      registerThread(targetThreadId, content);
    }
    const existingThread = threads.find((thread) => thread.id === targetThreadId);
    const nextTitle = isPlaceholderThreadTitle(existingThread?.title)
      ? promptToThreadTitle(content)
      : (existingThread?.title ?? promptToThreadTitle(content));
    const nextThreads = upsertThreadSummary(threads, targetThreadId, content).map((thread) =>
      thread.id === targetThreadId
        ? {
            ...thread,
            title: nextTitle,
            lastPrompt: content,
          }
        : thread,
    );
    persistThreads(nextThreads);
    setHasUserJustSubmitted(true);
    void patchThreadMetadata(targetThreadId, {
      title: nextTitle,
      last_prompt: content,
    }).catch((error) => {
      console.error("Failed to persist thread metadata", error);
    });
    // A run is starting: real sandbox_status stream events will arrive, so
    // drop any HTTP-resume override for this thread and let them take over.
    setSandboxOverrides((prev) => {
      if (!(targetThreadId in prev)) return prev;
      const next = { ...prev };
      delete next[targetThreadId];
      return next;
    });
    enqueueThreadCommand(targetThreadId, { type: "submit", content });
  });

  const handleResumeSandbox = useStableCallback(async () => {
    const promptThreadId = sandboxPrompt?.threadId;
    if (!promptThreadId || sandboxPrompt?.busy) return;
    setSandboxPrompt({ threadId: promptThreadId, busy: true, error: null, expired: false });
    try {
      const response = await fetchWithAuth(
        `${LANGGRAPH_API_URL}/sandboxes/${encodeURIComponent(promptThreadId)}/start`,
        { method: "POST" },
      );
      if (response.status === 404) {
        setSandboxPrompt({
          threadId: promptThreadId,
          busy: false,
          expired: true,
          error:
            "This conversation's sandbox has expired and its generated files were reclaimed. Re-run the analysis to regenerate them.",
        });
        return;
      }
      if (!response.ok) {
        throw new Error(`Sandbox start failed (${response.status})`);
      }
      setSandboxOverrides((prev) => ({
        ...prev,
        [promptThreadId]: { state: "ready", message: "Sandbox resumed" },
      }));
      setArtifactEpoch((epoch) => epoch + 1);
      setSandboxPrompt(null);
    } catch (error) {
      setSandboxPrompt({
        threadId: promptThreadId,
        busy: false,
        expired: false,
        error:
          error instanceof Error ? error.message : "Couldn't start the sandbox. Try again.",
      });
    }
  });

  const handleDismissSandboxPrompt = useStableCallback(() => {
    setSandboxPrompt(null);
  });

  async function ensureThreadId(): Promise<string> {
    if (threadId) return threadId;
    return createThread();
  }

  const handleRenderReport = useStableCallback(async (report: {
    title: string;
    markdown: string;
  }): Promise<void> => {
    const id = await ensureThreadId();
    const sourcePayload = artifacts.sources.map((source) => ({
      citation: source.citation,
      relevance: source.relevance,
      link: source.link ?? null,
    }));
    const response = await fetchWithAuth(
      `${LANGGRAPH_API_URL}/render-report/${encodeURIComponent(id)}`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          markdown: report.markdown,
          title: report.title,
          sources: sourcePayload,
          used_asta: sourcePayload.length > 0,
        }),
      },
    );
    if (!response.ok) {
      let message = `Render failed (${response.status})`;
      try {
        const data = (await response.json()) as { error?: string };
        if (data.error) message = data.error;
      } catch {
        // ignore body parse errors
      }
      throw new Error(message);
    }
    const blob = await response.blob();
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "report.pdf";
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
  });

  const handleUpload = useStableCallback(async (file: File): Promise<{ name: string; size: number }> => {
    const id = await ensureThreadId();
    const formData = new FormData();
    formData.append("file", file);
    const response = await fetchWithAuth(
      `${LANGGRAPH_API_URL}/upload/${encodeURIComponent(id)}`,
      { method: "POST", body: formData },
    );
    if (!response.ok) {
      let message = `Upload failed (${response.status})`;
      try {
        const data = (await response.json()) as { error?: string };
        if (data.error) message = data.error;
      } catch {
        // ignore body parse errors
      }
      throw new Error(message);
    }
    return (await response.json()) as { name: string; size: number };
  });

  const handleSelectThread = useStableCallback((selectedThreadId: string) => {
    if (selectedThreadId === threadId) return;
    saveActiveThreadId(selectedThreadId);
    setThreadId(selectedThreadId);
    setHasUserJustSubmitted(false);
  });

  const handleNewThread = useStableCallback(() => {
    clearActiveThreadId();
    setThreadId(null);
    setHasUserJustSubmitted(false);
  });

  const handleStop = useStableCallback(() => {
    if (!threadId || !activeSession?.isLoading) return;
    enqueueThreadCommand(threadId, { type: "stop" });
  });

  const handleQueueMessage = useStableCallback((content: string) => {
    if (!threadId) {
      // No active thread to attach the queue to; submit immediately.
      void handleSubmit(content);
      return;
    }
    setQueuedMessages((prev) => {
      const next = { ...prev, [threadId]: content };
      saveQueuedMessages(next);
      return next;
    });
  });

  const handleCancelQueue = useStableCallback(() => {
    if (!threadId) return;
    setQueuedMessages((prev) => {
      if (!(threadId in prev)) return prev;
      const next = { ...prev };
      delete next[threadId];
      saveQueuedMessages(next);
      return next;
    });
  });

  // ---- Explicit Projects + run-loop plan (P5) ----------------------------

  const handleSelectProject = useStableCallback((id: string) => {
    setActiveProjectIdState(id);
    setActiveProjectId(id);
    // Associate the current thread with this project (durable, best-effort).
    if (threadId) void assignThreadProject(threadId, id);
  });

  const handleCreateProject = useStableCallback(async (name: string) => {
    try {
      const meta = await createProject(name);
      setProjects((prev) => [...prev, meta]);
      handleSelectProject(meta.id);
    } catch {
      /* transient; the switcher stays on the current project */
    }
  });

  const handleRenameProject = useStableCallback(async (id: string, name: string) => {
    setProjects((prev) => prev.map((p) => (p.id === id ? { ...p, name } : p)));
    try {
      await renameProject(id, name);
    } catch {
      setProjects(await fetchProjects().catch(() => projects));
    }
  });

  const handleDeleteProject = useStableCallback(async (id: string) => {
    const target = projects.find((p) => p.id === id);
    if (!window.confirm(`Delete project "${target?.name ?? id}" and its mission/plan? This cannot be undone.`)) {
      return;
    }
    const remaining = projects.filter((p) => p.id !== id);
    setProjects(remaining);
    if (activeProjectId === id) handleSelectProject(remaining[0]?.id ?? "default");
    try {
      await deleteProject(id);
    } catch {
      setProjects(await fetchProjects().catch(() => projects));
    }
  });

  const handleToggleAutopilot = useStableCallback((on: boolean) => {
    setAutopilot(on);
    localStorage.setItem("atd:autopilot", on ? "1" : "0");
  });

  const handlePlanEdit = useStableCallback(async (op: PlanEdit) => {
    try {
      const updated = await patchPlan(op, activeProjectId ?? undefined);
      setLocalProject((prev) => ({
        ...updated,
        suggestions: prev?.suggestions ?? updated.suggestions,
      }));
    } catch {
      const server = await fetchProject(activeProjectId ?? undefined).catch(() => null);
      if (server) setLocalProject(server);
    }
  });

  const handleGeneratePlan = useStableCallback(() => {
    // Explicit user action → submit the planner prompt (runs the read-only
    // planner subagent). The resulting plan lands in the project spine.
    void handleSubmit(PLANNER_PROMPT);
  });

  const projectControls: ProjectControls = useMemo(
    () => ({
      projects,
      activeProjectId,
      autopilot,
      onSelectProject: handleSelectProject,
      onCreateProject: handleCreateProject,
      onRenameProject: handleRenameProject,
      onDeleteProject: handleDeleteProject,
      onToggleAutopilot: handleToggleAutopilot,
      onPlanEdit: handlePlanEdit,
      onGeneratePlan: handleGeneratePlan,
    }),
    [
      projects,
      activeProjectId,
      autopilot,
      handleSelectProject,
      handleCreateProject,
      handleRenameProject,
      handleDeleteProject,
      handleToggleAutopilot,
      handlePlanEdit,
      handleGeneratePlan,
    ],
  );

  const handleDeleteThread = useStableCallback(async (threadIdToDelete: string) => {
    const summary = threads.find((thread) => thread.id === threadIdToDelete);
    const label = summary?.title ?? "this conversation";
    if (threadSessions[threadIdToDelete]?.isLoading) {
      window.alert(`"${label}" is still running. Stop it or wait for it to finish before deleting.`);
      return;
    }
    if (!window.confirm(`Delete "${label}"? This cannot be undone.`)) return;

    if (threadIdToDelete === threadId) {
      clearActiveThreadId();
      setThreadId(null);
      setHasUserJustSubmitted(false);
    }

    persistThreads(removeThreadSummary(threads, threadIdToDelete));
    setThreadSessions((prev) => {
      const next = { ...prev };
      delete next[threadIdToDelete];
      return next;
    });
    setThreadCommands((prev) => {
      const next = { ...prev };
      delete next[threadIdToDelete];
      return next;
    });
    setQueuedMessages((prev) => {
      if (!(threadIdToDelete in prev)) return prev;
      const next = { ...prev };
      delete next[threadIdToDelete];
      saveQueuedMessages(next);
      return next;
    });
    setSubagentCache((prev) => {
      if (!(threadIdToDelete in prev)) return prev;
      const next = { ...prev };
      delete next[threadIdToDelete];
      saveSubagentCache(next);
      return next;
    });

    const results = await Promise.allSettled([
      fetchWithAuth(`${LANGGRAPH_API_URL}/threads/${encodeURIComponent(threadIdToDelete)}`, {
        method: "DELETE",
      }),
      fetchWithAuth(`${LANGGRAPH_API_URL}/sandboxes/${encodeURIComponent(threadIdToDelete)}`, {
        method: "DELETE",
      }),
    ]);

    const failures = results
      .map((result, index) => ({ result, label: index === 0 ? "thread" : "sandbox" }))
      .filter(
        ({ result }) =>
          result.status === "rejected" ||
          (result.status === "fulfilled" && !result.value.ok && result.value.status !== 404),
      );

    if (failures.length > 0) {
      const reasons = failures.map((failure) => failure.label).join(", ");
      window.alert(
        `Removed locally, but cleanup failed for: ${reasons}. The sandbox will be reclaimed automatically.`,
      );
    }
    void refreshThreads();
  });

  if (authLoading) {
    return <AuthGate mode="loading" allowedDomainsLabel={ALLOWED_EMAIL_DOMAINS_LABEL} />;
  }
  if (!user) {
    return (
      <AuthGate
        mode="signin"
        allowedDomainsLabel={ALLOWED_EMAIL_DOMAINS_LABEL}
        onSignIn={() => signIn()}
      />
    );
  }
  if (!isAllowedEmail(user.email)) {
    return (
      <AuthGate
        mode="forbidden"
        allowedDomainsLabel={ALLOWED_EMAIL_DOMAINS_LABEL}
        email={user.email ?? null}
        onSignOut={() => signOut()}
      />
    );
  }

  return (
    <>
      {mountedThreadIds.map((mountedThreadId) => (
        <ThreadStreamSession
          key={mountedThreadId}
          threadId={mountedThreadId}
          defaultHeaders={defaultHeaders}
          commands={threadCommands[mountedThreadId] ?? []}
          onSnapshotChange={handleSnapshotChange}
          onCommandProcessed={handleCommandProcessed}
          userId={user.id}
          userEmail={user.email ?? null}
        />
      ))}
      <AppShell
        messages={activeSession?.messages ?? []}
        todos={todos}
        subagents={subagents}
        datasets={artifacts.datasets}
        sources={artifacts.sources}
        files={artifacts.files}
        report={artifacts.report}
        hypothesis={artifacts.hypothesis}
        library={artifacts.library}
        analysis={artifacts.analysis}
        project={project}
        edges={artifacts.edges}
        onEditProject={handleEditProject}
        projectControls={projectControls}
        threads={threads}
        activeThreadId={threadId}
        activeThreadTitle={activeThreadTitle}
        threadsLoading={threadsLoading}
        threadsError={threadsError}
        isLoading={activeIsLoading}
        hasBackgroundRun={hasBackgroundRun}
        backgroundRunThreadTitle={backgroundRunThreadTitle}
        runningThreadIds={runningThreadIds}
        hasUserJustSubmitted={hasUserJustSubmitted}
        error={activeError}
        onSubmit={handleSubmit}
        onStop={handleStop}
        onUpload={handleUpload}
        onRenderReport={handleRenderReport}
        onSelectThread={handleSelectThread}
        onNewThread={handleNewThread}
        onDeleteThread={handleDeleteThread}
        theme={theme}
        toggleTheme={toggleTheme}
        sandboxStatus={sandboxStatus}
        userEmail={user.email ?? null}
        onSignOut={handleSignOut}
        queuedMessage={threadId ? (queuedMessages[threadId] ?? null) : null}
        onQueueMessage={handleQueueMessage}
        onCancelQueue={handleCancelQueue}
        artifactEpoch={artifactEpoch}
      />
      {sandboxPrompt ? (
        <ConfirmModal
          title="Sandbox is off"
          icon={<Box size={18} />}
          body={
            <>
              <p>
                <strong>Generated resources can't load.</strong> Images, data
                files, and reports from this conversation live in its sandbox,
                which is currently off.
              </p>
              <p>Turn the sandbox on to load them.</p>
            </>
          }
          confirmLabel="Turn on sandbox"
          busyLabel="Starting sandbox…"
          cancelLabel={sandboxPrompt.expired ? "Close" : "Not now"}
          busy={sandboxPrompt.busy}
          error={sandboxPrompt.error}
          hideConfirm={sandboxPrompt.expired}
          onConfirm={() => void handleResumeSandbox()}
          onClose={handleDismissSandboxPrompt}
        />
      ) : null}
    </>
  );
}

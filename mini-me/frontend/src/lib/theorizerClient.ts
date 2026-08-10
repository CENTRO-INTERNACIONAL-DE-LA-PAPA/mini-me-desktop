import { useEffect, useState } from "react";
import { normalizeArtifacts } from "./artifacts";
import { getAuthToken } from "./fileClient";
import { LANGGRAPH_API_URL } from "./streamConfig";
import type { HypothesisArtifact } from "../types";

// Poll cadence while a theorizer run is generating. The run itself takes
// minutes, so a slow cadence is plenty and keeps the backend/sandbox quiet.
const POLL_INTERVAL_MS = 30_000;

interface StatusResponse {
  status?: string;
  task_id?: string | null;
  theories?: unknown[];
  knowledge_gaps?: string[];
  papers_reviewed?: number;
  reason?: string;
  elapsed_seconds?: number | null;
}

/**
 * Turn a status-route response into a fully-normalized HypothesisArtifact by
 * reusing the same snake->camel normalizer the streamed artifacts use. The
 * `question` is carried over from the running artifact (the route doesn't echo
 * it back).
 */
function toHypothesis(question: string, res: StatusResponse): HypothesisArtifact | null {
  const normalized = normalizeArtifacts({
    hypotheses: [
      {
        question,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        theories: (res.theories ?? []) as any,
        knowledge_gaps: res.reason ? [res.reason] : res.knowledge_gaps ?? [],
        papers_reviewed: res.papers_reviewed ?? 0,
        status: res.status ?? "running",
        task_id: res.task_id ?? undefined,
      },
    ],
  });
  return normalized.hypothesis;
}

/**
 * While a hypothesis artifact is `running`, poll the theorizer status route and
 * return the resolved artifact once the run completes (or fails). Returns the
 * original artifact unchanged for non-running artifacts. This is what makes the
 * Theories card fill in on its own — no chat message, no button.
 */
export function useTheorizerStatus(
  threadId: string | null,
  hypothesis: HypothesisArtifact | null | undefined,
): { display: HypothesisArtifact | null; polling: boolean; elapsedSeconds: number | null } {
  const question = hypothesis?.question ?? "";
  const taskId = hypothesis?.taskId;
  const status = hypothesis?.status;
  const isRunning = status === "running" && !!taskId && !!threadId;

  const [resolved, setResolved] = useState<HypothesisArtifact | null>(null);
  const [polling, setPolling] = useState(false);
  const [elapsedSeconds, setElapsedSeconds] = useState<number | null>(null);

  useEffect(() => {
    // Reset when the tracked run changes or is no longer running.
    setResolved(null);
    setElapsedSeconds(null);
    if (!isRunning || !threadId || !taskId) {
      setPolling(false);
      return;
    }

    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    setPolling(true);

    const poll = async () => {
      try {
        const token = await getAuthToken();
        const headers: Record<string, string> = {};
        if (token) headers.Authorization = `Bearer ${token}`;
        // Pass the question so the backend can title the theories file it
        // persists into the sandbox on completion (agent-readable artifact).
        const qs = question ? `?q=${encodeURIComponent(question)}` : "";
        const res = await fetch(
          `${LANGGRAPH_API_URL}/theorizer/${encodeURIComponent(threadId)}/${encodeURIComponent(taskId)}${qs}`,
          { headers },
        );
        if (cancelled) return;
        const data = (await res.json()) as StatusResponse;
        if (cancelled) return;

        if (data.status === "completed") {
          setResolved(toHypothesis(question, data));
          setPolling(false);
          return; // terminal — stop polling
        }
        if (data.status === "failed" || data.status === "canceled") {
          setResolved(toHypothesis(question, data));
          setPolling(false);
          return;
        }
        if (data.status === "unavailable") {
          // Sandbox gone; nothing more we can do — stop and leave the running card.
          setPolling(false);
          return;
        }
        // still running
        setElapsedSeconds(data.elapsed_seconds ?? null);
        timer = setTimeout(poll, POLL_INTERVAL_MS);
      } catch {
        if (cancelled) return;
        timer = setTimeout(poll, POLL_INTERVAL_MS);
      }
    };

    void poll();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [threadId, taskId, status, isRunning, question]);

  return {
    display: resolved ?? hypothesis ?? null,
    polling,
    elapsedSeconds,
  };
}

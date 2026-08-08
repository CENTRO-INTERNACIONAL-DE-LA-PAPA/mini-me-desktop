import { useEffect, useState } from "react";
import { getAuthToken } from "./fileClient";
import { LANGGRAPH_API_URL } from "./streamConfig";
import type { DataAnalysisArtifact } from "../types";

// Poll cadence while a DataVoyager run is generating. The run takes minutes, so a
// slow cadence is plenty and keeps the backend/sandbox quiet.
const POLL_INTERVAL_MS = 30_000;

// The status route returns the run's raw narrative, not LLM-structured findings
// (only the subagent can synthesize those). Cap what we show on the card; the
// full text + charts live in the persisted analysis/<task_id> files.
const SUMMARY_CAP = 1500;

interface StatusResponse {
  status?: string;
  task_id?: string | null;
  context_id?: string | null;
  analysis_text?: string;
  artifacts?: string[];
  reason?: string;
  progress?: string;
}

function trim(text: string): string {
  if (text.length <= SUMMARY_CAP) return text;
  return `${text.slice(0, SUMMARY_CAP).trimEnd()}…`;
}

// Fold a status-route response into a DataAnalysisArtifact, carrying over the
// question / dataset paths from the running artifact (the route doesn't echo
// them). Findings stay empty — structuring them needs the subagent; the card
// shows the narrative summary and points at the Images tab + a follow-up ask.
function toAnalysis(
  base: DataAnalysisArtifact,
  res: StatusResponse,
): DataAnalysisArtifact {
  const summary =
    res.status === "failed" || res.status === "canceled"
      ? res.reason ?? "The DataVoyager run did not complete."
      : trim(res.analysis_text ?? "");
  return {
    question: base.question,
    datasetPaths: base.datasetPaths,
    summary,
    findings: [],
    hypothesesTested: [],
    charts: [],
    status: res.status ?? "completed",
    taskId: res.task_id ?? base.taskId,
    contextId: res.context_id ?? base.contextId,
  };
}

/**
 * While a DataAnalysis artifact is `running`, poll the analyze-data status route
 * and return the resolved artifact once the run completes (or fails). Returns the
 * original artifact unchanged for non-running artifacts. This is what makes the
 * Analysis card fill in on its own — no chat message, no button.
 */
export function useDataVoyagerStatus(
  threadId: string | null,
  analysis: DataAnalysisArtifact | null | undefined,
): { display: DataAnalysisArtifact | null; polling: boolean } {
  const taskId = analysis?.taskId;
  const contextId = analysis?.contextId ?? "";
  const question = analysis?.question ?? "";
  const status = analysis?.status;
  const isRunning = status === "running" && !!taskId && !!threadId;

  const [resolved, setResolved] = useState<DataAnalysisArtifact | null>(null);
  const [polling, setPolling] = useState(false);

  useEffect(() => {
    setResolved(null);
    if (!isRunning || !threadId || !taskId || !analysis) {
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
        // Pass the context id (so a terminal poll can export/persist) and the
        // question (so the persisted analysis file is titled).
        const params = new URLSearchParams();
        if (contextId) params.set("ctx", contextId);
        if (question) params.set("q", question);
        const qs = params.toString() ? `?${params.toString()}` : "";
        const res = await fetch(
          `${LANGGRAPH_API_URL}/analyze-data/${encodeURIComponent(threadId)}/${encodeURIComponent(taskId)}${qs}`,
          { headers },
        );
        if (cancelled) return;
        const data = (await res.json()) as StatusResponse;
        if (cancelled) return;

        if (data.status === "completed") {
          setResolved(toAnalysis(analysis, data));
          setPolling(false);
          return; // terminal — stop polling
        }
        if (data.status === "failed" || data.status === "canceled") {
          setResolved(toAnalysis(analysis, data));
          setPolling(false);
          return;
        }
        if (data.status === "input-required") {
          setResolved({ ...analysis, status: "input-required" });
          setPolling(false);
          return;
        }
        if (data.status === "unavailable") {
          setPolling(false);
          return;
        }
        // still running
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
  }, [threadId, taskId, contextId, question, status, isRunning, analysis]);

  return {
    display: resolved ?? analysis ?? null,
    polling,
  };
}

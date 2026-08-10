import type { BaseMessage } from "@langchain/core/messages";
import { useStream } from "@langchain/react";
import { useEffect, useRef, useState } from "react";
import { messageId, messageText } from "../lib/messages";
import {
  LANGGRAPH_API_URL,
  LANGGRAPH_ASSISTANT_ID,
  STREAM_CONFIG,
} from "../lib/streamConfig";
import { buildSubmitConfigurable } from "../lib/llmConfig";
import type { StreamLike } from "../lib/streamTypes";
import type { AgentState, SandboxStatus, ThreadSessionSnapshot } from "../types";

export interface ThreadSessionCommand {
  id: string;
  type: "submit" | "stop";
  content?: string;
}

interface ThreadStreamSessionProps {
  threadId: string;
  defaultHeaders?: Record<string, string>;
  commands: ThreadSessionCommand[];
  onSnapshotChange: (snapshot: ThreadSessionSnapshot) => void;
  onCommandProcessed: (threadId: string, commandIds: string[]) => void;
  userId?: string | null;
  userEmail?: string | null;
}

const DEFAULT_SANDBOX_STATUS: SandboxStatus = {
  state: "idle",
  message: "Local preview",
};

function normalizeSandboxState(value: string): SandboxStatus["state"] {
  if (value === "preparing" || value === "ready" || value === "error" || value === "idle") {
    return value;
  }
  return "idle";
}

export function ThreadStreamSession({
  threadId,
  defaultHeaders,
  commands,
  onSnapshotChange,
  onCommandProcessed,
  userId,
  userEmail,
}: ThreadStreamSessionProps) {
  const [sandboxStatus, setSandboxStatus] = useState<SandboxStatus>(DEFAULT_SANDBOX_STATUS);
  const handledCommandIdsRef = useRef<Set<string>>(new Set());

  const stream = useStream<AgentState>({
    apiUrl: LANGGRAPH_API_URL,
    assistantId: LANGGRAPH_ASSISTANT_ID,
    threadId,
    defaultHeaders,
    reconnectOnMount: true,
    fetchStateHistory: true,
    filterSubagentMessages: true,
    onCustomEvent: (event: unknown) => {
      const payload = (event as { sandbox_status?: { state?: string; message?: string } } | null)
        ?.sandbox_status;
      if (!payload || typeof payload.state !== "string") return;
      setSandboxStatus({
        state: normalizeSandboxState(payload.state),
        message: payload.message ?? "",
      });
    },
  } as unknown as Parameters<typeof useStream<AgentState>>[0] & {
    filterSubagentMessages: boolean;
    onCustomEvent: (event: unknown) => void;
  }) as unknown as StreamLike;

  // `stream.subagents` and `stream.values` are rebuilt as fresh references on
  // every render (the SDK constructs a new subagents Map on each access), so
  // depending on them directly re-fires this effect every render \u2192 setState \u2192
  // re-render loop that React halts, freezing live updates until a refresh.
  // Depend on stable content signatures instead, and read the live objects
  // inside the effect body.
  const messageSignature = stream.messages
    .map((message, index) => `${messageId(message, index)}:${messageText(message)}`)
    .join("\u241e");

  const subagentSignature = stream.subagents
    ? [...stream.subagents.values()]
        .map((raw) => {
          const s = raw as {
            id?: string;
            status?: string;
            result?: unknown;
            toolCalls?: unknown[];
            messages?: BaseMessage[];
          };
          const last = s.messages?.at(-1);
          return [
            s.id,
            s.status,
            s.toolCalls?.length ?? 0,
            s.result ? 1 : 0,
            last ? messageText(last).length : 0,
          ].join(":");
        })
        .join("|")
    : "";

  const todos = stream.values?.todos ?? [];
  const artifacts = stream.values?.artifacts;
  const valuesSignature = [
    todos.map((todo) => todo.status).join(","),
    artifacts?.datasets?.length ?? 0,
    artifacts?.sources?.length ?? 0,
    artifacts?.files?.length ?? 0,
    artifacts?.reports?.length ?? 0,
    artifacts?.hypotheses?.length ?? 0,
    artifacts?.libraries?.length ?? 0,
  ].join("|");

  useEffect(() => {
    onSnapshotChange({
      threadId,
      // Clone the array so parent components don't depend on a mutable
      // collection retaining the same identity during streaming updates.
      messages: [...stream.messages],
      values: stream.values,
      subagents: stream.subagents,
      isLoading: stream.isLoading,
      error: stream.error,
      sandboxStatus,
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    onSnapshotChange,
    sandboxStatus,
    stream.error,
    stream.isLoading,
    messageSignature,
    subagentSignature,
    valuesSignature,
    threadId,
  ]);

  useEffect(() => {
    const nextIds: string[] = [];
    for (const command of commands) {
      if (handledCommandIdsRef.current.has(command.id)) continue;
      handledCommandIdsRef.current.add(command.id);
      nextIds.push(command.id);
      if (command.type === "submit" && command.content) {
        // Tag every run with the signed-in user so LangSmith can filter runs
        // by metadata.user_id / metadata.user_email (and downstream analyses
        // can correlate behaviour back to a researcher).
        const runMetadata: Record<string, string> = {};
        if (userId) runMetadata.user_id = userId;
        if (userEmail) runMetadata.user_email = userEmail;
        // Per-run model routing + (client-mode) keys. Keys travel under the
        // `__llm_keys` key so LangGraph never copies them into trace metadata.
        const configurable = buildSubmitConfigurable();
        void stream.submit(
          { messages: [{ type: "human", content: command.content }] },
          {
            streamSubgraphs: true,
            config: {
              ...STREAM_CONFIG,
              configurable,
              ...(Object.keys(runMetadata).length > 0 ? { metadata: runMetadata } : {}),
            },
          },
        );
      } else if (command.type === "stop") {
        void stream.stop();
      }
    }
    if (nextIds.length > 0) {
      onCommandProcessed(threadId, nextIds);
    }
  }, [commands, onCommandProcessed, stream, threadId, userId, userEmail]);

  return null;
}

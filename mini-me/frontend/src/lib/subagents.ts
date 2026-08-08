import type { BaseMessage } from "@langchain/core/messages";
import type {
  SubagentRun,
  SubagentStatus,
  SubagentToolCall,
  SubagentToolCallState,
} from "../types";
import { messageText } from "./messages";

const MAX_PREVIEW_CHARS = 50_000;
const MAX_TOOL_RESULT_PREVIEW = 480;
const MAX_TOOL_ARGS_PREVIEW = 200;

// Loose structural view of a v3 SubagentStreamInterface entry. The SDK exports
// SubagentStreamInterface but it's deeply generic and pulls in agent type
// inference that doesn't apply here; we read only the fields we render so a
// structural interface keeps types honest without fighting the SDK.
interface SubagentLike {
  id?: string;
  status?: string;
  result?: string | null;
  messages?: BaseMessage[];
  toolCall?: {
    args?: {
      subagent_type?: string;
      description?: string;
    };
  };
  toolCalls?: ToolCallLike[];
  startedAt?: Date | string | null;
  completedAt?: Date | string | null;
}

interface ToolCallLike {
  id?: string;
  state?: string;
  call?: { id?: string; name?: string; args?: unknown };
  result?: { content?: unknown } | null;
}

function normalizeStatus(status: string | undefined): SubagentStatus {
  if (status === "pending") return "queued";
  if (status === "complete") return "completed";
  if (status === "error") return "failed";
  if (status === "running") return "running";
  return "queued";
}

function normalizeToolCallState(state: string | undefined): SubagentToolCallState {
  if (state === "completed") return "completed";
  if (state === "error") return "error";
  return "pending";
}

function labelFromName(name: string) {
  return name
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function truncatePreview(raw: string): string {
  if (raw.length <= MAX_PREVIEW_CHARS) return raw;
  const kept = raw.slice(0, MAX_PREVIEW_CHARS);
  const droppedKb = Math.round((raw.length - MAX_PREVIEW_CHARS) / 1024);
  return `${kept}\n\n…\n\n*Output truncated — ${droppedKb} KB hidden. Full result remains in the sandbox.*`;
}

function tryParseJson(value: string): unknown | undefined {
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  const first = trimmed[0];
  const last = trimmed[trimmed.length - 1];
  const looksJson =
    (first === "{" && last === "}") || (first === "[" && last === "]");
  if (!looksJson) return undefined;
  try {
    return JSON.parse(trimmed);
  } catch {
    return undefined;
  }
}

function formatResultPreview(raw: string | undefined): {
  preview: string | undefined;
  isStructured: boolean;
} {
  if (!raw) return { preview: undefined, isStructured: false };
  const parsed = tryParseJson(raw);
  if (parsed !== undefined) {
    const pretty = JSON.stringify(parsed, null, 2);
    return {
      preview: truncatePreview("```json\n" + pretty + "\n```"),
      isStructured: true,
    };
  }
  return { preview: truncatePreview(raw), isStructured: false };
}

function previewArgs(args: unknown): string | undefined {
  if (args == null) return undefined;
  if (typeof args === "string") {
    return args.length > MAX_TOOL_ARGS_PREVIEW
      ? args.slice(0, MAX_TOOL_ARGS_PREVIEW) + "…"
      : args;
  }
  try {
    const serialised = JSON.stringify(args);
    if (!serialised || serialised === "{}") return undefined;
    return serialised.length > MAX_TOOL_ARGS_PREVIEW
      ? serialised.slice(0, MAX_TOOL_ARGS_PREVIEW) + "…"
      : serialised;
  } catch {
    return undefined;
  }
}

function previewToolResult(content: unknown): string | undefined {
  if (content == null) return undefined;
  let text = "";
  if (typeof content === "string") {
    text = content;
  } else if (Array.isArray(content)) {
    text = content
      .map((part) => {
        if (typeof part === "string") return part;
        if (
          part &&
          typeof part === "object" &&
          "text" in part &&
          typeof (part as { text?: unknown }).text === "string"
        ) {
          return (part as { text: string }).text;
        }
        return "";
      })
      .filter(Boolean)
      .join("\n");
  } else {
    try {
      text = JSON.stringify(content);
    } catch {
      text = String(content);
    }
  }
  const trimmed = text.trim();
  if (!trimmed) return undefined;
  return trimmed.length > MAX_TOOL_RESULT_PREVIEW
    ? trimmed.slice(0, MAX_TOOL_RESULT_PREVIEW) + "…"
    : trimmed;
}

function buildToolCalls(raw: ToolCallLike[] | undefined): SubagentToolCall[] {
  if (!raw || raw.length === 0) return [];
  return raw.map((entry, idx) => ({
    id: entry.call?.id ?? entry.id ?? `tool-${idx}`,
    name: entry.call?.name ?? "tool",
    argsPreview: previewArgs(entry.call?.args),
    state: normalizeToolCallState(entry.state),
    resultPreview: previewToolResult(entry.result?.content),
  }));
}

function inferLatestActivity(
  status: SubagentStatus,
  toolCalls: SubagentToolCall[],
  messages: BaseMessage[],
): string | undefined {
  if (status !== "running") return undefined;
  const pendingTool = [...toolCalls].reverse().find((t) => t.state === "pending");
  if (pendingTool) {
    return `Calling ${pendingTool.name}…`;
  }
  // No pending tool — the model is writing/thinking. Fall back to the
  // tail of the latest AI message so the user sees something live.
  const lastMessage = messages.at(-1);
  if (!lastMessage) return "Thinking…";
  const text = messageText(lastMessage).trim();
  if (!text) return "Thinking…";
  // Keep a compact, single-line tail
  const flattened = text.replace(/\s+/g, " ");
  const tail =
    flattened.length > 80 ? "…" + flattened.slice(-80) : flattened;
  return tail;
}

function isoFromDate(value: Date | string | null | undefined): string | undefined {
  if (!value) return undefined;
  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? undefined : value.toISOString();
  }
  // SDK sometimes serialises Dates as strings during hydration; pass through.
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? undefined : parsed.toISOString();
}

function formatElapsed(
  startedAt: string | undefined,
  completedAt: string | undefined,
  status: SubagentStatus,
): string {
  if (!startedAt) return status === "running" ? "running" : "latest";
  const start = Date.parse(startedAt);
  if (Number.isNaN(start)) return status === "running" ? "running" : "latest";
  const end =
    completedAt && !Number.isNaN(Date.parse(completedAt))
      ? Date.parse(completedAt)
      : Date.now();
  const seconds = Math.max(0, Math.round((end - start) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

export function normalizeSubagents(
  subagents: Map<string, unknown> | undefined,
): SubagentRun[] {
  if (!subagents) return [];

  return [...subagents.values()]
    .map((rawSubagent, index) => {
      const subagent = rawSubagent as SubagentLike;
      const messages = (subagent.messages ?? []) as BaseMessage[];
      const status = normalizeStatus(subagent.status);
      const toolCalls = buildToolCalls(subagent.toolCalls);

      const lastMessage = messages.at(-1);
      const rawPreview =
        subagent.result ?? (lastMessage ? messageText(lastMessage) : undefined);
      const { preview, isStructured } = formatResultPreview(rawPreview ?? undefined);
      const name = subagent.toolCall?.args?.subagent_type ?? "subagent";
      const startedAt = isoFromDate(subagent.startedAt);
      const completedAt = isoFromDate(subagent.completedAt);

      return {
        id: subagent.id ?? `subagent-${index}`,
        name,
        label: labelFromName(name),
        status,
        task: subagent.toolCall?.args?.description ?? "Running delegated research task.",
        elapsed: formatElapsed(startedAt, completedAt, status),
        resultPreview: preview,
        resultIsStructured: isStructured,
        toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
        latestActivity: inferLatestActivity(status, toolCalls, messages),
        startedAt,
        completedAt,
      };
    })
    .reverse();
}

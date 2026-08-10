import type { BaseMessage } from "@langchain/core/messages";

const SUBAGENT_NAMES = new Set([
  "academic_researcher",
  "dataverse_explorer",
  "data_cleaning",
  "exploratory_data_analysis",
  "diagnostic_analytics",
  "predictive_analytics",
  "report_writer",
]);

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" ? (value as Record<string, unknown>) : {};
}

export function messageId(message: BaseMessage, index: number) {
  return typeof message.id === "string" && message.id ? message.id : `message-${index}`;
}

export function messageRole(message: BaseMessage) {
  const rawMessage = asRecord(message);
  const type =
    typeof message.getType === "function"
      ? message.getType()
      : typeof rawMessage.type === "string"
        ? rawMessage.type
        : "";

  if (type === "human" || type === "user") return "user";
  if (type === "ai" || type === "assistant") return "assistant";
  if (type === "system") return "system";
  if (type === "tool") return "tool";
  return "assistant";
}

export function messageText(message: BaseMessage) {
  const content = message.content;
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part) => {
        if (typeof part === "string") return part;
        if (
          part &&
          typeof part === "object" &&
          "text" in part &&
          typeof part.text === "string"
        ) {
          return part.text;
        }
        return "";
      })
      .filter(Boolean)
      .join("\n");
  }
  return "";
}

function messageName(message: BaseMessage) {
  const rawMessage = asRecord(message);
  return typeof rawMessage.name === "string" ? rawMessage.name : "";
}

export function shouldRenderMainMessage(message: BaseMessage) {
  const role = messageRole(message);
  if (role !== "user" && role !== "assistant") return false;
  if (SUBAGENT_NAMES.has(messageName(message))) return false;
  return messageText(message).trim().length > 0;
}

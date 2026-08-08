import type { BaseMessage } from "@langchain/core/messages";
import type { AgentState } from "../types";

export interface StreamSubmitOptions {
  streamSubgraphs?: boolean;
  config?: Record<string, unknown>;
}

export interface StreamLike {
  messages: BaseMessage[];
  values?: Partial<AgentState>;
  subagents?: Map<string, unknown>;
  isLoading: boolean;
  error: unknown;
  submit: (
    values: Record<string, unknown>,
    options?: StreamSubmitOptions,
  ) => Promise<void>;
  stop: () => void;
  switchThread?: (threadId: string | null) => void;
}

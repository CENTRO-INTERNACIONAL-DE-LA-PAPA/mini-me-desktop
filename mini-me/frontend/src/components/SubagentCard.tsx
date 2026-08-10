import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Circle,
  CircleAlert,
  Clock3,
  Wrench,
} from "lucide-react";
import { useState } from "react";
import type { SubagentRun, SubagentStatus, SubagentToolCall } from "../types";
import { MarkdownContent } from "./MarkdownContent";

const STATUS_LABELS: Record<SubagentStatus, string> = {
  queued: "Queued",
  running: "Running",
  completed: "Completed",
  failed: "Failed",
};

function StatusIcon({ status }: { status: SubagentStatus }) {
  if (status === "completed") return <CheckCircle2 size={15} />;
  if (status === "running") return <span className="status-spinner" aria-hidden="true" />;
  if (status === "failed") return <CircleAlert size={15} />;
  return <Circle size={15} />;
}

function ToolCallIcon({ state }: { state: SubagentToolCall["state"] }) {
  if (state === "completed") return <CheckCircle2 size={12} />;
  if (state === "error") return <CircleAlert size={12} />;
  return <span className="status-spinner status-spinner--xs" aria-hidden="true" />;
}

function ToolCallRow({ call }: { call: SubagentToolCall }) {
  const [showResult, setShowResult] = useState(false);
  const hasResult = Boolean(call.resultPreview);
  return (
    <li className={`subagent-tool subagent-tool--${call.state}`}>
      <button
        type="button"
        className="subagent-tool-head"
        onClick={() => hasResult && setShowResult((v) => !v)}
        disabled={!hasResult}
        aria-expanded={hasResult ? showResult : undefined}
      >
        <span className="subagent-tool-head-row">
          <ToolCallIcon state={call.state} />
          <span className="subagent-tool-name">{call.name}</span>
        </span>
        {call.argsPreview ? (
          <code className="subagent-tool-args">{call.argsPreview}</code>
        ) : null}
      </button>
      {hasResult && showResult ? (
        <pre className="subagent-tool-result">{call.resultPreview}</pre>
      ) : null}
    </li>
  );
}

export function SubagentCard({ subagent }: { subagent: SubagentRun }) {
  const [isExpanded, setIsExpanded] = useState(false);
  const hasPreview = Boolean(subagent.resultPreview?.trim());
  const toolCalls = subagent.toolCalls ?? [];
  const showLiveSubtitle =
    subagent.status === "running" && Boolean(subagent.latestActivity);

  return (
    <article className={`subagent-card ${subagent.status}`}>
      <div className="subagent-topline">
        <div className="subagent-title">
          <StatusIcon status={subagent.status} />
          <div className="subagent-title-text">
            <h3 title={subagent.label}>{subagent.label}</h3>
            {showLiveSubtitle ? (
              <span className="subagent-live" aria-live="polite">
                {subagent.latestActivity}
              </span>
            ) : null}
          </div>
        </div>
        <div className="subagent-actions">
          <span className="status-pill">{STATUS_LABELS[subagent.status]}</span>
          <button
            className="subagent-toggle"
            type="button"
            aria-expanded={isExpanded}
            aria-label={isExpanded ? "Collapse subagent details" : "Expand subagent details"}
            onClick={() => setIsExpanded((current) => !current)}
          >
            {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
          </button>
        </div>
      </div>

      {subagent.status === "running" || subagent.status === "queued" ? (
        <div className="subagent-meter">
          <span>
            {subagent.status === "queued"
              ? "queued"
              : toolCalls.length > 0
                ? `${toolCalls.length} tool${toolCalls.length > 1 ? "s" : ""}`
                : "working"}
          </span>
          <span className="subagent-meter-bar" aria-hidden="true">
            <i />
          </span>
        </div>
      ) : null}

      {isExpanded ? (
        <>
          <p className="subagent-task">{subagent.task}</p>

          <div className="subagent-meta">
            <span>
              <Clock3 size={13} aria-hidden="true" />
              {subagent.elapsed}
            </span>
            <span>{subagent.name}</span>
          </div>

          {toolCalls.length > 0 ? (
            <div className="subagent-toolcalls">
              <p className="subagent-section-title">
                <Wrench size={12} aria-hidden="true" />
                Tool calls ({toolCalls.length})
              </p>
              <ul className="subagent-tool-list">
                {toolCalls.map((call) => (
                  <ToolCallRow key={call.id} call={call} />
                ))}
              </ul>
            </div>
          ) : null}

          {hasPreview ? (
            <div className="subagent-details">
              <MarkdownContent>{subagent.resultPreview ?? ""}</MarkdownContent>
            </div>
          ) : null}
        </>
      ) : null}
    </article>
  );
}

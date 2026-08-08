import type { BaseMessage } from "@langchain/core/messages";
import { BarChart3, BookOpen, Bot, FileText, Paperclip, RefreshCw, Square, SlidersHorizontal } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type DragEvent } from "react";
import {
  messageId,
  messageRole,
  messageText,
  shouldRenderMainMessage,
} from "../lib/messages";
import type { SubagentRun } from "../types";
import { Composer } from "./Composer";
import { MarkdownContent } from "./MarkdownContent";

interface ChatPanelProps {
  messages: BaseMessage[];
  threadTitle?: string | null;
  isLoading: boolean;
  hasBackgroundRun: boolean;
  backgroundRunThreadTitle: string | null;
  hasUserJustSubmitted: boolean;
  error: unknown;
  subagents: SubagentRun[];
  onSubmit: (content: string) => Promise<void>;
  onStop: () => void;
  onUpload: (file: File) => Promise<{ name: string; size: number }>;
  onOpenSettings?: () => void;
  queuedMessage?: string | null;
  onQueueMessage?: (content: string) => void;
  onCancelQueue?: () => void;
  prefill?: { text: string; nonce: number } | null;
}

// Detect 401s from the LangSmith deployment's WorkOS auth layer (expired or
// unusable session token). Checked before isModelConfigError, which would
// otherwise swallow these under its generic "authentication|401" match and
// send the user to model settings instead of back through sign-in.
function isAuthSessionError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error ?? "");
  return /token expired|invalid token|missing authorization|authorization header|token verification failed/i.test(
    message,
  );
}

// Detect errors that mean "the user hasn't configured a usable model/key yet"
// so we can show an actionable banner instead of a raw stream error.
function isModelConfigError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error ?? "");
  return /no api key configured|connect a provider|open settings|api[_ ]?key|incorrect api key|invalid api key|authentication|401/i.test(
    message,
  );
}

function describeActivity(
  subagents: SubagentRun[],
  isLoading: boolean,
  hasBackgroundRun: boolean,
  backgroundRunThreadTitle: string | null,
): string {
  if (!isLoading) {
    if (!hasBackgroundRun) return "Connected to the LangGraph stream.";
    return backgroundRunThreadTitle
      ? `Background run still active in “${backgroundRunThreadTitle}”.`
      : "A different conversation is still running in the background.";
  }
  const running = subagents.filter((s) => s.status === "running");
  if (running.length === 0) return "Agent is thinking…";
  if (running.length > 1) return `Coordinating ${running.length} subagents…`;
  const sole = running[0];
  const task = (sole.task ?? "").replace(/\s+/g, " ").trim();
  if (!task || task === "Running delegated research task.") {
    return `${sole.label} is working…`;
  }
  const compact = task.length > 90 ? `${task.slice(0, 87)}…` : task;
  return `${sole.label} · ${compact}`;
}

export function ChatPanel({
  messages,
  threadTitle,
  isLoading,
  hasBackgroundRun,
  backgroundRunThreadTitle,
  hasUserJustSubmitted,
  error,
  subagents,
  onSubmit,
  onStop,
  onUpload,
  onOpenSettings,
  queuedMessage,
  onQueueMessage,
  onCancelQueue,
  prefill,
}: ChatPanelProps) {
  const submitBlocked = isLoading;
  const visibleMessages = useMemo(
    () => messages.filter(shouldRenderMainMessage),
    [messages],
  );
  const endRef = useRef<HTMLDivElement | null>(null);
  const dragDepth = useRef(0);
  const [isDragOver, setIsDragOver] = useState(false);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end", behavior: "smooth" });
  }, [visibleMessages.length, isLoading]);

  const activity = describeActivity(
    subagents,
    isLoading,
    hasBackgroundRun,
    backgroundRunThreadTitle,
  );
  const hasMessages = visibleMessages.length > 0;
  const sessionTitle = threadTitle?.trim() || "New conversation";

  function handleDragEnter(e: DragEvent<HTMLElement>) {
    e.preventDefault();
    if (!e.dataTransfer.types.includes("Files")) return;
    dragDepth.current += 1;
    setIsDragOver(true);
  }

  function handleDragLeave(e: DragEvent<HTMLElement>) {
    e.preventDefault();
    dragDepth.current -= 1;
    if (dragDepth.current === 0) setIsDragOver(false);
  }

  function handleDragOver(e: DragEvent<HTMLElement>) {
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
  }

  async function handleDrop(e: DragEvent<HTMLElement>) {
    e.preventDefault();
    dragDepth.current = 0;
    setIsDragOver(false);
    const file = e.dataTransfer.files[0];
    if (file) await onUpload(file);
  }

  return (
    <section
      className="chat-panel"
      aria-label="Conversation"
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
    >
      <svg className="goo-filter" aria-hidden="true" width="0" height="0" focusable="false">
        <defs>
          <filter id="goo">
            <feGaussianBlur in="SourceGraphic" stdDeviation="3" result="blur" />
            <feColorMatrix
              in="blur"
              mode="matrix"
              values="
                1 0 0 0 0
                0 1 0 0 0
                0 0 1 0 0
                0 0 0 18 -7
              "
              result="goo"
            />
            <feBlend in="SourceGraphic" in2="goo" />
          </filter>
        </defs>
      </svg>
      {isDragOver ? (
        <div className="drop-overlay" aria-hidden="true">
          <Paperclip size={28} />
          <span>Drop file to upload</span>
        </div>
      ) : null}
      <div className="session">
        <div className="session-head">
          <div className="session-head-top">
            <div>
              <div className="session-eyebrow">
                <span><b>Session</b> private research thread</span>
                <span>·</span>
                <span><b>Mode</b> coordinated analysis</span>
                <span>·</span>
                <span><b>Status</b> {isLoading ? "running" : "ready"}</span>
              </div>
              <h2 className="session-title">{sessionTitle}</h2>
            </div>
            {isLoading ? (
              <button
                type="button"
                className="stop-button"
                onClick={onStop}
                aria-label="Stop agent"
                title="Stop agent"
              >
                <Square size={14} />
                Stop
              </button>
            ) : null}
          </div>

          {hasBackgroundRun ? (
            <div className="background-run-notice" role="status" aria-live="polite">
              {backgroundRunThreadTitle
                ? `“${backgroundRunThreadTitle}” is still running in the background. You can browse other threads now.`
                : "Another conversation is still running in the background. You can browse other threads now."}
            </div>
          ) : null}
        </div>

        <div className="message-list thread-feed">
          {hasMessages ? (
          visibleMessages.map((message, index) => {
            const role = messageRole(message);
            const text = messageText(message);
            const roleLabel = role === "user" ? "You" : "Coordinator";
            const avatarLabel = role === "user" ? "U" : "AI";

            return (
              <article key={messageId(message, index)} className={`message msg ${role}`}>
                <div className="message-icon avatar-blob" aria-hidden="true">
                  {avatarLabel}
                </div>
                <div className="message-content body">
                  <div className="message-role role">{roleLabel}</div>
                  <MarkdownContent>{text}</MarkdownContent>
                </div>
              </article>
            );
          })
        ) : isLoading && !hasUserJustSubmitted ? (
          <div className="empty-chat empty-chat--connecting" aria-live="polite">
            <span className="status-spinner" aria-hidden="true" />
            Preparing your conversation…
          </div>
        ) : isLoading ? (
          <div className="empty-chat empty-chat--connecting" aria-live="polite">
            <span className="status-spinner" aria-hidden="true" />
            Initializing agent…
          </div>
        ) : (
          <div className="empty-chat enhanced-empty-state">
            <div className="empty-icon-wrapper">
              <Bot size={42} className="empty-icon" />
            </div>
            <h3>How can I help you accelerate your research?</h3>
            <p>Select a quick start or type your own request below.</p>
            <div className="quick-starts">
              <button
                type="button"
                disabled={submitBlocked}
                onClick={() => onSubmit("Search for recent literature on my topic.")}
                className="quick-start-btn"
              >
                <BookOpen size={16} aria-hidden="true" /> Search literature
              </button>
              <button
                type="button"
                disabled={submitBlocked}
                onClick={() => onSubmit("Run an exploratory data analysis (EDA).")}
                className="quick-start-btn"
              >
                <BarChart3 size={16} aria-hidden="true" /> Run EDA
              </button>
              <button
                type="button"
                disabled={submitBlocked}
                onClick={() => onSubmit("Generate a markdown summary report.")}
                className="quick-start-btn"
              >
                <FileText size={16} aria-hidden="true" /> Generate report
              </button>
            </div>
          </div>
        )}
          {isLoading && hasMessages ? (
            <div className="thinking" aria-live="polite">
              <div className="loader" aria-hidden="true">
                <i />
                <i />
              </div>
              <div className="meta">
                {activity}
                <span className="dots" aria-hidden="true">
                  <span />
                  <span />
                  <span />
                </span>
              </div>
            </div>
          ) : null}
        <div ref={endRef} aria-hidden="true" />
        </div>
      </div>

      {error ? (
        isAuthSessionError(error) ? (
          <div className="stream-error config-error" role="alert">
            <div className="config-error-text">
              <strong>Your session expired.</strong> Reload the page to sign in
              again — your conversations are saved.
            </div>
            <button
              type="button"
              className="config-error-action"
              onClick={() => window.location.reload()}
            >
              <RefreshCw size={14} aria-hidden="true" />
              Reload &amp; sign in
            </button>
          </div>
        ) : isModelConfigError(error) ? (
          <div className="stream-error config-error" role="alert">
            <div className="config-error-text">
              <strong>No usable model configured.</strong> Connect a provider and
              add your API key to start running.
            </div>
            {onOpenSettings ? (
              <button
                type="button"
                className="config-error-action"
                onClick={onOpenSettings}
              >
                <SlidersHorizontal size={14} aria-hidden="true" />
                Open Model &amp; API settings
              </button>
            ) : null}
          </div>
        ) : (
          <div className="stream-error" role="alert">
            {error instanceof Error ? error.message : "The stream returned an error."}
          </div>
        )
      ) : null}

      <Composer
        isLoading={isLoading}
        onSubmit={onSubmit}
        onUpload={onUpload}
        queuedMessage={queuedMessage}
        onQueueMessage={onQueueMessage}
        onCancelQueue={onCancelQueue}
        prefill={prefill}
      />
    </section>
  );
}

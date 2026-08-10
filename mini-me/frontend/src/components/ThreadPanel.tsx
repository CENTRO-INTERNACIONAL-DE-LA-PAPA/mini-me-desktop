import { Plus, Search, Trash2 } from "lucide-react";
import { memo, useMemo, useState } from "react";
import type { ThreadSummary } from "../types";

interface ThreadPanelProps {
  threads: ThreadSummary[];
  activeThreadId: string | null;
  runningThreadIds: string[];
  isLoading?: boolean;
  error?: string | null;
  onSelectThread: (threadId: string) => void;
  onNewThread: () => void;
  onDeleteThread: (threadId: string) => void | Promise<void>;
}

function formatThreadDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";

  const diffMs = Date.now() - date.getTime();
  const minute = 60 * 1000;
  const hour = 60 * minute;
  const day = 24 * hour;

  if (diffMs < hour) return "now";
  if (diffMs < day) return `${Math.floor(diffMs / hour)}h`;
  if (diffMs < 7 * day) return `${Math.floor(diffMs / day)}d`;

  return new Intl.DateTimeFormat(undefined, {
    month: "short",
  }).format(date);
}

// Memoized: props are stabilized in App so streaming ticks in the chat don't
// re-render the whole conversation list.
export const ThreadPanel = memo(function ThreadPanel({
  threads,
  activeThreadId,
  runningThreadIds,
  isLoading = false,
  error = null,
  onSelectThread,
  onNewThread,
  onDeleteThread,
}: ThreadPanelProps) {
  const [query, setQuery] = useState("");
  const normalizedQuery = query.trim().toLowerCase();
  const visibleThreads = useMemo(() => {
    if (!normalizedQuery) return threads;
    return threads.filter((thread) => {
      const haystack = `${thread.title} ${thread.lastPrompt ?? ""}`.toLowerCase();
      return haystack.includes(normalizedQuery);
    });
  }, [normalizedQuery, threads]);
  const threadCountLabel = `${threads.length} active`;

  return (
    <section className="thread-panel" aria-label="Conversations">
      <div className="panel-heading compact thread-heading">
        <div className="thread-heading-title">
          <span className="thread-heading-dot" aria-hidden="true" />
          <p className="eyebrow">Conversations</p>
        </div>
        <span className="thread-count">{threadCountLabel}</span>
      </div>

      <button
        className="new-thread-button"
        type="button"
        onClick={onNewThread}
        title="New conversation"
      >
        <Plus size={16} aria-hidden="true" />
        <span>Start a new investigation</span>
      </button>

      <label className="thread-search" aria-label="Search conversations">
        <Search size={14} aria-hidden="true" />
        <input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search conversations"
        />
      </label>

      {error ? <div className="thread-feedback thread-feedback--error">{error}</div> : null}
      {isLoading && threads.length === 0 ? (
        <div className="thread-feedback">Loading conversations…</div>
      ) : visibleThreads.length ? (
        <div className="thread-list">
          {visibleThreads.map((thread) => {
            const isActive = thread.id === activeThreadId;
            const isRunning = runningThreadIds.includes(thread.id);

            return (
              <div key={thread.id} className="thread-row">
                <button
                  className={`thread-item ${isActive ? "active" : ""}`}
                  type="button"
                  onClick={() => onSelectThread(thread.id)}
                  disabled={isActive}
                >
                  <span
                    className={`thread-led${isRunning ? " thread-led--live" : ""}`}
                    aria-hidden="true"
                  />
                  <span className="thread-copy">
                    <strong>{thread.title}</strong>
                  </span>
                  <span className={`thread-meta${isRunning ? " thread-meta--live" : ""}`}>
                    {isRunning ? "live" : formatThreadDate(thread.updatedAt)}
                  </span>
                </button>
                <button
                  className="thread-delete"
                  type="button"
                  aria-label={`Delete ${thread.title}`}
                  title="Delete conversation"
                  disabled={isRunning}
                  onClick={(event) => {
                    event.stopPropagation();
                    void onDeleteThread(thread.id);
                  }}
                >
                  <Trash2 size={14} aria-hidden="true" />
                </button>
              </div>
            );
          })}
        </div>
      ) : (
        <div className="empty-threads">
          {threads.length && normalizedQuery
            ? "No conversations match your search."
            : "No saved conversations yet."}
        </div>
      )}
    </section>
  );
});

import type { BaseMessage } from "@langchain/core/messages";
import { Info } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import type {
  DataAnalysisArtifact,
  DatasetArtifact,
  FileArtifact,
  HypothesisArtifact,
  LibraryArtifact,
  ProjectArtifact,
  ProvenanceEdge,
  ReportArtifact,
  SourceArtifact,
  SubagentRun,
  ThreadSummary,
  TodoItem,
} from "../types";
import type { ProjectControls, ProjectEdit } from "../lib/projectClient";
import { branding } from "../branding";
import { ArtifactPanel } from "./ArtifactPanel";
import { AstaStatusDot } from "./AstaStatusDot";
import { Background } from "./Background";
import { ChatPanel } from "./ChatPanel";
import { ConfirmModal } from "./ConfirmModal";
import { SubagentActivityPanel } from "./SubagentActivityPanel";
import { ThreadPanel } from "./ThreadPanel";
import { TodoProgress } from "./TodoProgress";
import { Sun, Moon, PanelLeft, PanelRight, LogOut, SlidersHorizontal } from "lucide-react";

// Both modals are behind explicit clicks, so they stay out of the initial
// bundle and load on first open.
const AboutModal = lazy(() =>
  import("./AboutModal").then((m) => ({ default: m.AboutModal })),
);
const ModelConfigPanel = lazy(() =>
  import("./ModelConfigPanel").then((m) => ({ default: m.ModelConfigPanel })),
);

interface AppShellProps {
  messages: BaseMessage[];
  todos: TodoItem[];
  subagents: SubagentRun[];
  datasets: DatasetArtifact[];
  sources: SourceArtifact[];
  files: FileArtifact[];
  report?: ReportArtifact | null;
  hypothesis?: HypothesisArtifact | null;
  library?: LibraryArtifact | null;
  analysis?: DataAnalysisArtifact | null;
  project?: ProjectArtifact | null;
  edges?: ProvenanceEdge[];
  onEditProject?: (edit: ProjectEdit) => void;
  projectControls?: ProjectControls;
  threads: ThreadSummary[];
  activeThreadId: string | null;
  activeThreadTitle?: string | null;
  threadsLoading?: boolean;
  threadsError?: string | null;
  isLoading: boolean;
  hasBackgroundRun: boolean;
  backgroundRunThreadTitle: string | null;
  runningThreadIds: string[];
  hasUserJustSubmitted: boolean;
  error: unknown;
  onSubmit: (content: string) => Promise<void>;
  onStop: () => void;
  onUpload: (file: File) => Promise<{ name: string; size: number }>;
  onRenderReport: (report: ReportArtifact) => Promise<void>;
  onSelectThread: (threadId: string) => void;
  onNewThread: () => void;
  onDeleteThread: (threadId: string) => void | Promise<void>;
  theme: 'light' | 'dark';
  toggleTheme: () => void;
  sandboxStatus?: {
    state: 'idle' | 'preparing' | 'ready' | 'error';
    message: string;
  };
  userEmail?: string | null;
  onSignOut?: () => void;
  queuedMessage?: string | null;
  onQueueMessage?: (content: string) => void;
  onCancelQueue?: () => void;
  /** Bumped when the sandbox is resumed so artifact images refetch. */
  artifactEpoch?: number;
}

export function AppShell({
  messages,
  todos,
  subagents,
  datasets,
  sources,
  files,
  report,
  hypothesis,
  library,
  analysis,
  project,
  edges,
  onEditProject,
  projectControls,
  threads,
  activeThreadId,
  activeThreadTitle,
  threadsLoading,
  threadsError,
  isLoading,
  hasBackgroundRun,
  backgroundRunThreadTitle,
  runningThreadIds,
  hasUserJustSubmitted,
  error,
  onSubmit,
  onStop,
  onUpload,
  onRenderReport,
  onSelectThread,
  onNewThread,
  onDeleteThread,
  theme,
  toggleTheme,
  sandboxStatus,
  userEmail,
  onSignOut,
  queuedMessage,
  onQueueMessage,
  onCancelQueue,
  artifactEpoch = 0,
}: AppShellProps) {
  const [showAbout, setShowAbout] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  // Bumped when Settings closes so the top-bar Asta dot re-reads its status
  // right after the user pastes/updates their token.
  const [astaRefresh, setAstaRefresh] = useState(0);
  const [showSignOutConfirm, setShowSignOutConfirm] = useState(false);
  const [showLeftSidebar, setShowLeftSidebar] = useState(true);
  const [showRightSidebar, setShowRightSidebar] = useState(true);
  const userInitial = (userEmail?.trim().charAt(0) ?? "U").toUpperCase();

  // Promote-to-execute (P3.2): a project suggestion drops its prompt into the
  // composer for the user to review and send. The nonce lets the same prompt
  // be re-sent on a repeat click; it never auto-submits.
  const [composerPrefill, setComposerPrefill] = useState<
    { text: string; nonce: number } | null
  >(null);
  const promoteSuggestion = useCallback((prompt: string) => {
    setComposerPrefill((prev) => ({ text: prompt, nonce: (prev?.nonce ?? 0) + 1 }));
  }, []);

  const LEFT_MIN = 240;
  const LEFT_MAX = 640;
  const RIGHT_MIN = 280;
  const RIGHT_MAX = 760;

  const [leftWidth, setLeftWidth] = useState<number>(() => {
    const saved = Number(localStorage.getItem("atd:leftSidebarWidth"));
    return Number.isFinite(saved) && saved >= LEFT_MIN && saved <= LEFT_MAX ? saved : 340;
  });
  const [rightWidth, setRightWidth] = useState<number>(() => {
    const saved = Number(localStorage.getItem("atd:rightSidebarWidth"));
    return Number.isFinite(saved) && saved >= RIGHT_MIN && saved <= RIGHT_MAX ? saved : 400;
  });

  const draggingRef = useRef<null | "left" | "right">(null);
  const startXRef = useRef(0);
  const startWidthRef = useRef(0);

  // Debounced so a drag persists once at rest instead of on every mousemove.
  useEffect(() => {
    const timer = setTimeout(() => {
      localStorage.setItem("atd:leftSidebarWidth", String(leftWidth));
    }, 250);
    return () => clearTimeout(timer);
  }, [leftWidth]);
  useEffect(() => {
    const timer = setTimeout(() => {
      localStorage.setItem("atd:rightSidebarWidth", String(rightWidth));
    }, 250);
    return () => clearTimeout(timer);
  }, [rightWidth]);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!draggingRef.current) return;
      const delta = e.clientX - startXRef.current;
      if (draggingRef.current === "left") {
        const next = Math.max(LEFT_MIN, Math.min(LEFT_MAX, startWidthRef.current + delta));
        setLeftWidth(next);
      } else {
        const next = Math.max(RIGHT_MIN, Math.min(RIGHT_MAX, startWidthRef.current - delta));
        setRightWidth(next);
      }
    };
    const onUp = () => {
      if (!draggingRef.current) return;
      draggingRef.current = null;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      document.body.classList.remove("resizing");
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  const startDrag = useCallback(
    (side: "left" | "right") => (e: React.MouseEvent) => {
      e.preventDefault();
      draggingRef.current = side;
      startXRef.current = e.clientX;
      startWidthRef.current = side === "left" ? leftWidth : rightWidth;
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
      document.body.classList.add("resizing");
    },
    [leftWidth, rightWidth],
  );

  const onHandleKey = (side: "left" | "right") => (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const step = e.shiftKey ? 32 : 8;
    const dir = e.key === "ArrowRight" ? 1 : -1;
    e.preventDefault();
    if (side === "left") {
      setLeftWidth((w) => Math.max(LEFT_MIN, Math.min(LEFT_MAX, w + dir * step)));
    } else {
      setRightWidth((w) => Math.max(RIGHT_MIN, Math.min(RIGHT_MAX, w - dir * step)));
    }
  };

  return (
    <main className="app-shell">
      <Background />
      <header className="topbar">
        <div className="topbar-left">
          <div className="brand">
            {branding.logo ? (
              <img
                className="brand-mark"
                src={branding.logo.src}
                alt={branding.logo.alt ?? branding.appName}
              />
            ) : (
              <div className="brand-mark" aria-hidden="true" />
            )}
            <div className="brand-name">
              <b>{branding.appName}</b>
              <span>{branding.tagline}</span>
            </div>
          </div>
        </div>
        <div className="topbar-right">
          {hasBackgroundRun ? (
            <div className="run-state background-pill" aria-label="Background activity">
              <span className="run-dot" />
              {backgroundRunThreadTitle ?? "Background activity"}
            </div>
          ) : null}
          <div
            className={`run-state sandbox-state sandbox-state--${sandboxStatus?.state ?? 'idle'}`}
            aria-label="Sandbox status"
            title={sandboxStatus?.message || undefined}
          >
            <span className="run-dot" />
            {sandboxStatus?.state === 'preparing'
              ? 'Sandbox starting…'
              : sandboxStatus?.state === 'ready'
                ? 'Sandbox ready'
                : sandboxStatus?.state === 'error'
                  ? 'Sandbox error'
                  : 'Sandbox off'}
          </div>
          <div className="topbar-divider" aria-hidden="true" />
          <div className="topbar-group">
            <button
              type="button"
              className={`sidebar-toggle-button ${showLeftSidebar ? 'active' : ''}`}
              onClick={() => setShowLeftSidebar(!showLeftSidebar)}
              aria-label="Toggle Activity Sidebar"
            >
              <PanelLeft size={18} />
            </button>
            <button
              type="button"
              className={`sidebar-toggle-button ${showRightSidebar ? 'active' : ''}`}
              onClick={() => setShowRightSidebar(!showRightSidebar)}
              aria-label="Toggle Artifact Sidebar"
            >
              <PanelRight size={18} />
            </button>
          </div>
          <div className="topbar-divider" aria-hidden="true" />
          <div className="topbar-group">
            <button
              type="button"
              className="theme-toggle-button"
              onClick={toggleTheme}
              aria-label="Toggle theme"
              title="Toggle light/dark mode"
            >
              {theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}
            </button>
            <AstaStatusDot
              onOpenSettings={() => setShowSettings(true)}
              refreshSignal={astaRefresh}
            />
            <button
              type="button"
              className="topbar-info-button"
              aria-label="Model & API settings"
              title="Model & API settings"
              onClick={() => setShowSettings(true)}
            >
              <SlidersHorizontal size={16} aria-hidden="true" />
            </button>
            <button
              type="button"
              className="topbar-info-button"
              aria-label={`About ${branding.appName}`}
              title={`About ${branding.appName}`}
              onClick={() => setShowAbout(true)}
            >
              <Info size={16} aria-hidden="true" />
            </button>
          </div>
          {onSignOut ? (
            <button
              type="button"
              className="user-badge"
              onClick={() => setShowSignOutConfirm(true)}
              aria-label="Sign out"
              title={userEmail ? `Sign out (${userEmail})` : "Sign out"}
            >
              <span className="avatar" aria-hidden="true">{userInitial}</span>
              <LogOut size={16} />
            </button>
          ) : null}
        </div>
      </header>

      <section className="workspace" aria-label="Research workflow workspace">
        <aside
          className={`activity-column${showLeftSidebar ? "" : " collapsed"}`}
          aria-label="Run activity"
          aria-hidden={!showLeftSidebar}
          style={showLeftSidebar ? { width: leftWidth } : undefined}
        >
          <ThreadPanel
            threads={threads}
            activeThreadId={activeThreadId}
            runningThreadIds={runningThreadIds}
            isLoading={threadsLoading}
            error={threadsError}
            onSelectThread={onSelectThread}
            onNewThread={onNewThread}
            onDeleteThread={onDeleteThread}
          />
          <TodoProgress todos={todos} />
          <SubagentActivityPanel subagents={subagents} />
        </aside>

        {showLeftSidebar ? (
          <div
            className="resize-handle"
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize left sidebar"
            aria-valuenow={leftWidth}
            aria-valuemin={LEFT_MIN}
            aria-valuemax={LEFT_MAX}
            tabIndex={0}
            onMouseDown={startDrag("left")}
            onKeyDown={onHandleKey("left")}
            onDoubleClick={() => setLeftWidth(340)}
            title="Drag to resize · double-click to reset"
          />
        ) : null}

        <ChatPanel
          messages={messages}
          threadTitle={activeThreadTitle ?? null}
          isLoading={isLoading}
          hasBackgroundRun={hasBackgroundRun}
          backgroundRunThreadTitle={backgroundRunThreadTitle}
          hasUserJustSubmitted={hasUserJustSubmitted}
          error={error}
          subagents={subagents}
          onSubmit={onSubmit}
          onStop={onStop}
          onUpload={onUpload}
          onOpenSettings={() => setShowSettings(true)}
          queuedMessage={queuedMessage}
          onQueueMessage={onQueueMessage}
          onCancelQueue={onCancelQueue}
          prefill={composerPrefill}
        />

        {showRightSidebar ? (
          <div
            className="resize-handle"
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize right sidebar"
            aria-valuenow={rightWidth}
            aria-valuemin={RIGHT_MIN}
            aria-valuemax={RIGHT_MAX}
            tabIndex={0}
            onMouseDown={startDrag("right")}
            onKeyDown={onHandleKey("right")}
            onDoubleClick={() => setRightWidth(400)}
            title="Drag to resize · double-click to reset"
          />
        ) : null}

        <ArtifactPanel
          key={artifactEpoch}
          threadId={activeThreadId}
          datasets={datasets}
          sources={sources}
          files={files}
          report={report}
          hypothesis={hypothesis}
          library={library}
          analysis={analysis}
          project={project}
          edges={edges}
          onEditProject={onEditProject}
          projectControls={projectControls}
          onRenderReport={onRenderReport}
          onSearchLibrary={(query) => onSubmit(`Search my library for: ${query}`)}
          onPromoteSuggestion={promoteSuggestion}
          collapsed={!showRightSidebar}
          width={rightWidth}
        />
      </section>

      {showAbout ? (
        <Suspense fallback={null}>
          <AboutModal onClose={() => setShowAbout(false)} />
        </Suspense>
      ) : null}
      {showSettings ? (
        <Suspense fallback={null}>
          <ModelConfigPanel
            onClose={() => {
              setShowSettings(false);
              setAstaRefresh((n) => n + 1);
            }}
          />
        </Suspense>
      ) : null}
      {showSignOutConfirm && onSignOut ? (
        <ConfirmModal
          title="Sign out"
          icon={<LogOut size={18} />}
          body={
            <p>
              <strong>Are you sure you want to sign out?</strong>
              {userEmail ? (
                <>
                  {" "}
                  You're signed in as <strong>{userEmail}</strong>.
                </>
              ) : null}
            </p>
          }
          confirmLabel="Sign out"
          cancelLabel="Stay signed in"
          danger
          onConfirm={() => {
            setShowSignOutConfirm(false);
            onSignOut();
          }}
          onClose={() => setShowSignOutConfirm(false)}
        />
      ) : null}
    </main>
  );
}

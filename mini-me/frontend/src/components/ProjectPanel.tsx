import {
  ArrowRight,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Circle,
  Compass,
  ListChecks,
  Pencil,
  Plane,
  Plus,
  Sparkles,
  X,
} from "lucide-react";
import { memo, useState, type FormEvent, type KeyboardEvent } from "react";
import type { ProjectArtifact } from "../types";
import type { ProjectControls, ProjectEdit } from "../lib/projectClient";
import { RunLoopPanel } from "./RunLoopPanel";
import { ProjectSwitcher } from "./ProjectSwitcher";

// The opt-in Autopilot switch (P5). Off by default → the panel shows P3 advisory
// suggestions. On → the AI-authored run-loop plan, walked one confirmed step at
// a time. Frontend-only gate; executing steps still needs the sandbox.
function AutopilotToggle({
  on,
  onToggle,
}: {
  on: boolean;
  onToggle: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      className={`autopilot-toggle${on ? " autopilot-toggle--on" : ""}`}
      role="switch"
      aria-checked={on}
      onClick={() => onToggle(!on)}
    >
      <Plane size={15} aria-hidden="true" />
      <span className="autopilot-toggle-text">
        <span className="autopilot-toggle-title">Autopilot</span>
        <span className="autopilot-toggle-sub">Walk a plan step by step</span>
      </span>
      <span className="autopilot-switch" aria-hidden="true">
        <span className="autopilot-knob" />
      </span>
    </button>
  );
}

// A read-only collapsible list block (used for Completed work, and for Pending
// when editing is unavailable). Mirrors the KnowledgeGaps collapsible.
function WorkList({
  label,
  items,
  icon,
  defaultOpen,
}: {
  label: string;
  items: string[];
  icon: React.ReactNode;
  defaultOpen: boolean;
}) {
  const [isOpen, setIsOpen] = useState(defaultOpen);
  if (items.length === 0) return null;

  return (
    <div className="project-worklist">
      <button
        type="button"
        className="project-worklist-toggle"
        aria-expanded={isOpen}
        onClick={() => setIsOpen((open) => !open)}
      >
        {isOpen ? (
          <ChevronDown size={14} aria-hidden="true" />
        ) : (
          <ChevronRight size={14} aria-hidden="true" />
        )}
        <span>{label}</span>
        <span className="ct">{items.length}</span>
      </button>
      {isOpen ? (
        <ul>
          {items.map((item, index) => (
            <li key={index}>
              <span className="project-worklist-icon" aria-hidden="true">
                {icon}
              </span>
              <span>{item}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

// Inline mission editor. Shows the mission with a pencil; editing swaps in a
// textarea with Save/Cancel. Persisting is the parent's job (onEdit).
function MissionEditor({
  mission,
  onEdit,
}: {
  mission: string;
  onEdit?: (edit: ProjectEdit) => void;
}) {
  const hasMission = mission.trim().length > 0;
  const [isEditing, setIsEditing] = useState(false);
  const [draft, setDraft] = useState(mission);

  function open() {
    setDraft(mission);
    setIsEditing(true);
  }
  function save() {
    onEdit?.({ mission: draft });
    setIsEditing(false);
  }

  if (isEditing) {
    return (
      <div className="project-mission-edit">
        <textarea
          aria-label="Edit mission"
          className="project-mission-input"
          value={draft}
          rows={2}
          autoFocus
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              save();
            } else if (event.key === "Escape") {
              setIsEditing(false);
            }
          }}
        />
        <div className="project-mission-edit-actions">
          <button type="button" className="project-btn project-btn--primary" onClick={save}>
            Save
          </button>
          <button type="button" className="project-btn" onClick={() => setIsEditing(false)}>
            Cancel
          </button>
        </div>
      </div>
    );
  }

  if (!hasMission) {
    return onEdit ? (
      <button type="button" className="project-mission-set" onClick={open}>
        <Plus size={13} aria-hidden="true" />
        Set a mission for this research project
      </button>
    ) : (
      <p className="project-mission project-mission-empty">
        Your mission will appear here once you start an investigation.
      </p>
    );
  }

  return (
    <div className="project-mission-row">
      <p className="project-mission">{mission}</p>
      {onEdit ? (
        <button
          type="button"
          className="project-icon-btn"
          aria-label="Edit mission"
          title="Edit mission"
          onClick={open}
        >
          <Pencil size={13} aria-hidden="true" />
        </button>
      ) : null}
    </div>
  );
}

// Editable Pending backlog: complete/dismiss each item and add new ones.
function PendingBacklog({
  items,
  defaultOpen,
  onEdit,
}: {
  items: string[];
  defaultOpen: boolean;
  onEdit: (edit: ProjectEdit) => void;
}) {
  const [isOpen, setIsOpen] = useState(defaultOpen);
  const [draft, setDraft] = useState("");

  function addItem(event: FormEvent) {
    event.preventDefault();
    const text = draft.trim();
    if (!text) return;
    onEdit({ pending_add: text });
    setDraft("");
  }

  return (
    <div className="project-worklist">
      <button
        type="button"
        className="project-worklist-toggle"
        aria-expanded={isOpen}
        onClick={() => setIsOpen((open) => !open)}
      >
        {isOpen ? (
          <ChevronDown size={14} aria-hidden="true" />
        ) : (
          <ChevronRight size={14} aria-hidden="true" />
        )}
        <span>Pending work</span>
        <span className="ct">{items.length}</span>
      </button>
      {isOpen ? (
        <>
          <ul>
            {items.map((item, index) => (
              <li key={index} className="project-pending-item">
                <span className="project-worklist-icon" aria-hidden="true">
                  <Circle size={11} />
                </span>
                <span className="project-pending-text">{item}</span>
                <span className="project-pending-actions">
                  <button
                    type="button"
                    className="project-icon-btn"
                    aria-label={`Mark "${item}" complete`}
                    title="Mark complete"
                    onClick={() => onEdit({ complete: item })}
                  >
                    <Check size={13} aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    className="project-icon-btn"
                    aria-label={`Dismiss "${item}"`}
                    title="Dismiss"
                    onClick={() => onEdit({ pending_remove: item })}
                  >
                    <X size={13} aria-hidden="true" />
                  </button>
                </span>
              </li>
            ))}
          </ul>
          <form className="project-add-row" onSubmit={addItem}>
            <input
              type="text"
              className="project-add-input"
              placeholder="Add a work item…"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event: KeyboardEvent<HTMLInputElement>) => {
                if (event.key === "Escape") setDraft("");
              }}
            />
            <button
              type="submit"
              className="project-icon-btn"
              aria-label="Add work item"
              title="Add work item"
              disabled={!draft.trim()}
            >
              <Plus size={14} aria-hidden="true" />
            </button>
          </form>
        </>
      ) : null}
    </div>
  );
}

// The persistent research-project spine: mission + advisory next steps that
// survive across threads. Suggestions are advisory: clicking one drops a
// ready-to-send prompt into the composer for the user to review and send —
// it never runs automatically (P3.2). The mission and Pending backlog are
// hand-editable (P3.3); edits persist via `onEdit`.
export const ProjectPanel = memo(function ProjectPanel({
  project,
  onPromote,
  onEdit,
  controls,
}: {
  project: ProjectArtifact;
  onPromote?: (prompt: string) => void;
  onEdit?: (edit: ProjectEdit) => void;
  controls?: ProjectControls;
}) {
  const autopilot = Boolean(controls?.autopilot);
  const hasSuggestions = project.suggestions.length > 0;

  return (
    <section className="project-panel" aria-label="Research project">
      <div className="project-heading">
        <Compass size={15} aria-hidden="true" />
        <p className="eyebrow">Research project</p>
      </div>

      {controls ? (
        <ProjectSwitcher
          projects={controls.projects}
          activeProjectId={controls.activeProjectId}
          onSelect={controls.onSelectProject}
          onCreate={controls.onCreateProject}
          onRename={controls.onRenameProject}
          onDelete={controls.onDeleteProject}
        />
      ) : null}

      <MissionEditor mission={project.mission} onEdit={onEdit} />

      {controls ? (
        <AutopilotToggle on={autopilot} onToggle={controls.onToggleAutopilot} />
      ) : null}

      {controls && autopilot ? (
        project.plan ? (
          <RunLoopPanel
            plan={project.plan}
            onPlanEdit={controls.onPlanEdit}
            onRun={(prompt) => onPromote?.(prompt)}
            onGeneratePlan={controls.onGeneratePlan}
          />
        ) : (
          <div className="runloop-empty">
            <ListChecks size={18} aria-hidden="true" />
            <p>Let the planner draft an ordered plan for this project. You review and edit it, then run each step yourself.</p>
            <button type="button" className="runloop-run runloop-run--wide" onClick={controls.onGeneratePlan}>
              <Sparkles size={14} aria-hidden="true" /> Generate a research plan
            </button>
          </div>
        )
      ) : null}

      {!autopilot && hasSuggestions ? (
        <div className="project-suggestions">
          <div className="project-suggestions-head">
            <Sparkles size={14} aria-hidden="true" />
            <h4>Suggested next steps</h4>
          </div>
          <ul>
            {project.suggestions.map((suggestion, index) => {
              const canPromote = Boolean(onPromote && suggestion.prompt);
              return (
                <li key={index} className="project-suggestion">
                  <p className="project-suggestion-title">{suggestion.title}</p>
                  <p className="project-suggestion-rationale">{suggestion.rationale}</p>
                  {canPromote ? (
                    <button
                      type="button"
                      className="project-suggestion-action project-suggestion-action--button"
                      onClick={() => onPromote?.(suggestion.prompt)}
                      title={`Draft this in the composer: "${suggestion.prompt}"`}
                    >
                      <ArrowRight size={12} aria-hidden="true" />
                      Run {suggestion.action}
                    </button>
                  ) : (
                    <span className="project-suggestion-action">
                      <ArrowRight size={12} aria-hidden="true" />
                      Run {suggestion.action}
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
          <p className="project-advisory-note">
            Click a step to draft it in the composer — review and send when
            you're ready. Nothing runs automatically.
          </p>
        </div>
      ) : null}

      {onEdit ? (
        <PendingBacklog
          items={project.pending}
          defaultOpen={!hasSuggestions && project.pending.length > 0}
          onEdit={onEdit}
        />
      ) : (
        <WorkList
          label="Pending work"
          items={project.pending}
          icon={<Circle size={11} />}
          defaultOpen={!hasSuggestions}
        />
      )}
      <WorkList
        label="Completed work"
        items={project.completed}
        icon={<CheckCircle2 size={12} />}
        defaultOpen={false}
      />
    </section>
  );
});

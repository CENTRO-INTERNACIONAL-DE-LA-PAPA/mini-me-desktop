import {
  ArrowRight,
  Check,
  CircleCheckBig,
  CircleDot,
  Pencil,
  Plus,
  RefreshCw,
  SkipForward,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { useState, type FormEvent } from "react";
import type { PlanStep, ResearchPlan } from "../types";
import type { PlanEdit } from "../lib/projectClient";

// The opt-in autonomous run loop (P5). Renders an AI-authored plan that the user
// accepts/edits, then executes one CONFIRMED step at a time: the active step's
// Run button drops its prompt into the composer (onRun) — it never auto-sends.
// After each step the user can mark it done to advance, or change direction
// (edit / skip / re-activate / add / re-plan). Nothing here runs automatically.

function StatusIcon({ status }: { status: PlanStep["status"] }) {
  if (status === "done") return <CircleCheckBig size={16} className="runloop-ic runloop-ic--done" aria-hidden="true" />;
  if (status === "skipped") return <SkipForward size={15} className="runloop-ic runloop-ic--skip" aria-hidden="true" />;
  if (status === "active") return <CircleDot size={16} className="runloop-ic runloop-ic--active" aria-hidden="true" />;
  return <span className="runloop-ic runloop-ic--pending" aria-hidden="true" />;
}

function StepEditor({
  step,
  onSave,
  onCancel,
}: {
  step: PlanStep;
  onSave: (fields: { title: string; prompt: string }) => void;
  onCancel: () => void;
}) {
  const [title, setTitle] = useState(step.title);
  const [prompt, setPrompt] = useState(step.prompt);
  return (
    <div className="runloop-step-edit">
      <input
        className="runloop-input"
        aria-label="Step title"
        value={title}
        onChange={(e) => setTitle(e.target.value)}
      />
      <textarea
        className="runloop-input"
        aria-label="Step prompt"
        rows={3}
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
      />
      <div className="runloop-edit-actions">
        <button
          type="button"
          className="project-btn project-btn--primary"
          onClick={() => onSave({ title: title.trim(), prompt: prompt.trim() })}
        >
          Save
        </button>
        <button type="button" className="project-btn" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

function AddStepForm({ onAdd }: { onAdd: (fields: { title: string; prompt: string }) => void }) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [prompt, setPrompt] = useState("");

  function submit(e: FormEvent) {
    e.preventDefault();
    const t = title.trim();
    const p = prompt.trim();
    if (!t && !p) return;
    onAdd({ title: t, prompt: p });
    setTitle("");
    setPrompt("");
    setOpen(false);
  }

  if (!open) {
    return (
      <button type="button" className="runloop-linkbtn" onClick={() => setOpen(true)}>
        <Plus size={13} aria-hidden="true" /> Add step
      </button>
    );
  }
  return (
    <form className="runloop-step-edit" onSubmit={submit}>
      <input
        className="runloop-input"
        aria-label="New step title"
        placeholder="Step title"
        value={title}
        autoFocus
        onChange={(e) => setTitle(e.target.value)}
      />
      <textarea
        className="runloop-input"
        aria-label="New step prompt"
        placeholder="Use the … subagent to …"
        rows={3}
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
      />
      <div className="runloop-edit-actions">
        <button type="submit" className="project-btn project-btn--primary">Add</button>
        <button type="button" className="project-btn" onClick={() => setOpen(false)}>Cancel</button>
      </div>
    </form>
  );
}

export function RunLoopPanel({
  plan,
  onPlanEdit,
  onRun,
  onGeneratePlan,
}: {
  plan: ResearchPlan;
  onPlanEdit: (op: PlanEdit) => void;
  onRun: (prompt: string) => void;
  onGeneratePlan: () => void;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const isProposed = plan.status === "proposed";
  const isDone = plan.status === "done";
  const doneCount = plan.steps.filter((s) => s.status === "done" || s.status === "skipped").length;

  return (
    <div className="runloop" aria-label="Autonomous run loop">
      <div className="runloop-head">
        <Sparkles size={14} aria-hidden="true" />
        <h4>Research plan</h4>
        <span className="runloop-status">
          {isProposed
            ? "Proposed · review to start"
            : isDone
              ? "Complete"
              : `Step ${Math.min(doneCount + 1, plan.steps.length)} of ${plan.steps.length}`}
        </span>
      </div>

      <ol className="runloop-steps">
        {plan.steps.map((step, index) => {
          const isActive = step.status === "active";
          return (
            <li
              key={step.id}
              className={`runloop-step runloop-step--${step.status}${isActive ? " runloop-step--current" : ""}`}
            >
              <div className="runloop-step-row">
                <StatusIcon status={step.status} />
                <div className="runloop-step-main">
                  <p className="runloop-step-title">{step.title || step.prompt}</p>
                  {step.action ? <p className="runloop-step-action">{step.action}</p> : null}
                  {isActive && step.prompt ? (
                    <p className="runloop-step-prompt">“{step.prompt}”</p>
                  ) : null}
                </div>
                <div className="runloop-step-tools">
                  <button
                    type="button"
                    className="project-icon-btn"
                    aria-label={`Edit "${step.title}"`}
                    title="Edit step"
                    onClick={() => setEditingId(editingId === step.id ? null : step.id)}
                  >
                    <Pencil size={12} aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    className="project-icon-btn"
                    aria-label={`Remove "${step.title}"`}
                    title="Remove step"
                    onClick={() => onPlanEdit({ op: "remove", id: step.id })}
                  >
                    <Trash2 size={12} aria-hidden="true" />
                  </button>
                </div>
              </div>

              {editingId === step.id ? (
                <StepEditor
                  step={step}
                  onSave={(fields) => {
                    onPlanEdit({ op: "edit", id: step.id, ...fields });
                    setEditingId(null);
                  }}
                  onCancel={() => setEditingId(null)}
                />
              ) : null}

              {!isProposed && isActive ? (
                <div className="runloop-step-actions">
                  <button
                    type="button"
                    className="runloop-run"
                    onClick={() => onRun(step.prompt)}
                    disabled={!step.prompt}
                    title={step.prompt ? `Draft in composer: "${step.prompt}"` : undefined}
                  >
                    <ArrowRight size={13} aria-hidden="true" /> Run this step
                  </button>
                  <button
                    type="button"
                    className="project-btn"
                    onClick={() => onPlanEdit({ op: "complete", id: step.id })}
                  >
                    <Check size={13} aria-hidden="true" /> Done
                  </button>
                  <button
                    type="button"
                    className="project-btn"
                    onClick={() => onPlanEdit({ op: "skip", id: step.id })}
                  >
                    <SkipForward size={13} aria-hidden="true" /> Skip
                  </button>
                </div>
              ) : null}

              {!isProposed && !isActive && step.status === "pending" ? (
                <button
                  type="button"
                  className="runloop-linkbtn runloop-linkbtn--indent"
                  onClick={() => onPlanEdit({ op: "activate", id: step.id })}
                >
                  <ArrowRight size={12} aria-hidden="true" /> Jump to this step
                </button>
              ) : null}

              {index === 0 && isActive ? (
                <p className="runloop-review-note">
                  Review the results in Outputs, then mark done to continue.
                </p>
              ) : null}
            </li>
          );
        })}
      </ol>

      <AddStepForm onAdd={(fields) => onPlanEdit({ op: "add", ...fields })} />

      <div className="runloop-footer">
        {isProposed ? (
          <button
            type="button"
            className="runloop-run runloop-run--wide"
            onClick={() => onPlanEdit({ op: "accept" })}
          >
            <Check size={14} aria-hidden="true" /> Accept plan
          </button>
        ) : null}
        <button type="button" className="project-btn" onClick={onGeneratePlan} title="Ask the planner for a fresh plan">
          <RefreshCw size={13} aria-hidden="true" /> Re-plan
        </button>
        <button type="button" className="project-btn" onClick={() => onPlanEdit({ op: "clear" })}>
          <X size={13} aria-hidden="true" /> Clear
        </button>
      </div>

      <p className="project-advisory-note">
        Nothing runs automatically — each step drops into the composer for you to
        review and send.
      </p>
    </div>
  );
}

import { Check, ChevronDown, FolderOpen, Pencil, Plus, Trash2 } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";
import type { ProjectMeta } from "../types";

// Switch between explicit Projects (P5). A Project groups conversations and owns
// its own mission + run-loop plan, so switching here changes what the whole
// Research project panel shows. Create / rename / delete live in the dropdown.
export function ProjectSwitcher({
  projects,
  activeProjectId,
  onSelect,
  onCreate,
  onRename,
  onDelete,
}: {
  projects: ProjectMeta[];
  activeProjectId: string | null;
  onSelect: (id: string) => void;
  onCreate: (name: string) => void;
  onRename: (id: string, name: string) => void;
  onDelete: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  const active = projects.find((p) => p.id === activeProjectId) ?? projects[0] ?? null;

  useEffect(() => {
    if (!open) return;
    function onDocClick(event: MouseEvent) {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
        setCreating(false);
        setRenamingId(null);
      }
    }
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, [open]);

  function submitCreate(event: FormEvent) {
    event.preventDefault();
    const name = draft.trim();
    if (!name) return;
    onCreate(name);
    setDraft("");
    setCreating(false);
    setOpen(false);
  }

  function submitRename(event: FormEvent, id: string) {
    event.preventDefault();
    const name = draft.trim();
    if (!name) return;
    onRename(id, name);
    setDraft("");
    setRenamingId(null);
  }

  return (
    <div className="project-switcher" ref={rootRef}>
      <button
        type="button"
        className="project-switcher-trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        title="Switch project"
      >
        <FolderOpen size={14} aria-hidden="true" />
        <span className="project-switcher-name">{active?.name ?? "My research"}</span>
        <ChevronDown size={14} aria-hidden="true" />
      </button>

      {open ? (
        <div className="project-switcher-menu" role="menu">
          <ul>
            {projects.map((project) => (
              <li key={project.id}>
                {renamingId === project.id ? (
                  <form className="project-switcher-rename" onSubmit={(e) => submitRename(e, project.id)}>
                    <input
                      className="project-add-input"
                      aria-label="Rename project"
                      value={draft}
                      autoFocus
                      onChange={(e) => setDraft(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Escape") setRenamingId(null);
                      }}
                    />
                    <button type="submit" className="project-icon-btn" aria-label="Save name">
                      <Check size={13} aria-hidden="true" />
                    </button>
                  </form>
                ) : (
                  <div className={`project-switcher-item${project.id === active?.id ? " is-active" : ""}`}>
                    <button
                      type="button"
                      className="project-switcher-select"
                      role="menuitem"
                      onClick={() => {
                        onSelect(project.id);
                        setOpen(false);
                      }}
                    >
                      {project.id === active?.id ? (
                        <Check size={13} aria-hidden="true" />
                      ) : (
                        <span className="project-switcher-dot" aria-hidden="true" />
                      )}
                      <span>{project.name}</span>
                    </button>
                    <span className="project-switcher-actions">
                      <button
                        type="button"
                        className="project-icon-btn"
                        aria-label={`Rename ${project.name}`}
                        title="Rename"
                        onClick={() => {
                          setRenamingId(project.id);
                          setDraft(project.name);
                        }}
                      >
                        <Pencil size={12} aria-hidden="true" />
                      </button>
                      {projects.length > 1 ? (
                        <button
                          type="button"
                          className="project-icon-btn"
                          aria-label={`Delete ${project.name}`}
                          title="Delete project"
                          onClick={() => onDelete(project.id)}
                        >
                          <Trash2 size={12} aria-hidden="true" />
                        </button>
                      ) : null}
                    </span>
                  </div>
                )}
              </li>
            ))}
          </ul>

          {creating ? (
            <form className="project-switcher-create" onSubmit={submitCreate}>
              <input
                className="project-add-input"
                aria-label="New project name"
                placeholder="New project name"
                value={draft}
                autoFocus
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") setCreating(false);
                }}
              />
              <button type="submit" className="project-icon-btn" aria-label="Create project">
                <Check size={13} aria-hidden="true" />
              </button>
            </form>
          ) : (
            <button
              type="button"
              className="project-switcher-new"
              onClick={() => {
                setCreating(true);
                setDraft("");
              }}
            >
              <Plus size={13} aria-hidden="true" /> New project
            </button>
          )}
        </div>
      ) : null}
    </div>
  );
}

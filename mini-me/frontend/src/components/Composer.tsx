import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import {
  BookOpen,
  Brain,
  Database,
  LineChart,
  Paperclip,
  PencilLine,
  ScrollText,
  SendHorizontal,
  Slash,
  TrendingUp,
  Wrench,
  X,
  type LucideIcon,
} from "lucide-react";

interface ComposerProps {
  isLoading: boolean;
  onSubmit: (content: string) => Promise<void>;
  onUpload: (file: File) => Promise<{ name: string; size: number }>;
  queuedMessage?: string | null;
  onQueueMessage?: (content: string) => void;
  onCancelQueue?: () => void;
  // Prompt to drop into the box for the user to review and send (never
  // auto-submitted). The nonce re-triggers the fill on a repeat click of the
  // same suggestion. (P3.2 promote-to-execute.)
  prefill?: { text: string; nonce: number } | null;
}

interface UploadedFile {
  name: string;
  size: number;
}

interface SlashCommand {
  /** Subagent name as registered in backend/subagents.py. */
  name: string;
  label: string;
  icon: LucideIcon;
  /** Prompt prefix inserted into the composer; the coordinator handles the
   * actual delegation via its `task` tool, so these are steering directives,
   * not a direct RPC into the subagent. */
  template: string;
}

const SLASH_COMMANDS: SlashCommand[] = [
  {
    name: "academic_researcher",
    label: "Search literature",
    icon: BookOpen,
    template: "Use the academic_researcher subagent to search the literature on ",
  },
  {
    name: "dataverse_explorer",
    label: "Find datasets",
    icon: Database,
    template: "Use the dataverse_explorer subagent to find datasets about ",
  },
  {
    name: "data_cleaning",
    label: "Clean data",
    icon: Wrench,
    template: "Use the data_cleaning subagent to validate and harmonize ",
  },
  {
    name: "exploratory_data_analysis",
    label: "Run EDA",
    icon: LineChart,
    template:
      "Use the exploratory_data_analysis subagent to run an exploratory data analysis on ",
  },
  {
    name: "diagnostic_analytics",
    label: "Diagnose drivers",
    icon: Brain,
    template: "Use the diagnostic_analytics subagent to explain what drives ",
  },
  {
    name: "predictive_analytics",
    label: "Predict outcomes",
    icon: TrendingUp,
    template: "Use the predictive_analytics subagent to build a predictive model for ",
  },
  {
    name: "report_writer",
    label: "Generate report",
    icon: ScrollText,
    template:
      "Use the report_writer subagent to produce a markdown report of the findings so far.",
  },
];

export function Composer({
  isLoading,
  onSubmit,
  onUpload,
  queuedMessage,
  onQueueMessage,
  onCancelQueue,
  prefill,
}: ComposerProps) {
  const [content, setContent] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isUploading, setIsUploading] = useState(false);
  const [uploadedFiles, setUploadedFiles] = useState<UploadedFile[]>([]);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashIndex, setSlashIndex] = useState(0);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const formRef = useRef<HTMLFormElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  // While the content is a "/query", filter the command list by it; when the
  // menu was opened from the "/" button over arbitrary text there's no query.
  const slashQuery = content.startsWith("/") ? content.slice(1).toLowerCase() : null;
  const filteredCommands =
    slashQuery === null
      ? SLASH_COMMANDS
      : SLASH_COMMANDS.filter(
          (command) =>
            command.label.toLowerCase().includes(slashQuery) ||
            command.name.includes(slashQuery),
        );

  // The highlighted row, clamped as typing shrinks the filtered list.
  const activeSlashIndex =
    filteredCommands.length > 0 ? Math.min(slashIndex, filteredCommands.length - 1) : 0;

  function pickSlashCommand(command: SlashCommand) {
    // Replace the "/query" trigger; keep any other text the user already had.
    const rest = content.startsWith("/") ? "" : content;
    setContent(command.template + rest);
    setSlashOpen(false);
    textareaRef.current?.focus();
  }

  function handleContentChange(event: ChangeEvent<HTMLTextAreaElement>) {
    const value = event.target.value;
    setContent(value);
    // Typing "/" as the first character opens the menu; typing anything else
    // closes it so Enter always sends plain text. An empty box keeps a
    // button-opened menu visible.
    if (value.startsWith("/")) {
      setSlashOpen(true);
      setSlashIndex(0);
    } else if (value.length > 0) {
      setSlashOpen(false);
    }
  }

  // Close the menu when clicking anywhere outside the composer.
  useEffect(() => {
    if (!slashOpen) return;
    function onPointerDown(event: PointerEvent) {
      const target = event.target as Node | null;
      if (target && formRef.current && !formRef.current.contains(target)) {
        setSlashOpen(false);
      }
    }
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [slashOpen]);

  // Keep the keyboard-highlighted row visible if the list ever scrolls.
  useEffect(() => {
    if (!slashOpen) return;
    menuRef.current
      ?.querySelector(".slash-menu-item.selected")
      ?.scrollIntoView({ block: "nearest" });
  }, [slashOpen, activeSlashIndex]);

  // Promote-to-execute (P3.2): a project suggestion drops its prompt in here
  // for the user to review and send. We fill + focus but NEVER submit — the
  // user stays the gate. Keyed on the nonce so re-clicking the same suggestion
  // re-fills; guarded so it never clobbers typing on mount or re-render.
  const prefillNonce = prefill?.nonce ?? null;
  useEffect(() => {
    if (prefillNonce === null || !prefill) return;
    setContent(prefill.text);
    setSlashOpen(false);
    const textarea = textareaRef.current;
    if (textarea) {
      textarea.focus();
      // Put the cursor at the end so the user can immediately edit/append.
      const end = prefill.text.length;
      textarea.setSelectionRange(end, end);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prefillNonce]);

  const canQueue = Boolean(onQueueMessage);
  // Send stays enabled during a run when queuing is wired up — submits are
  // re-routed to the queue instead of being blocked.
  const sendDisabled =
    (isLoading && !canQueue) || isSubmitting || isUploading || !content.trim();
  const uploadDisabled = isUploading;

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (slashOpen) {
      // Submitting while the menu is open picks the highlighted command
      // rather than sending a raw "/query" to the agent.
      if (filteredCommands.length > 0) {
        pickSlashCommand(filteredCommands[activeSlashIndex]);
      }
      return;
    }
    const trimmed = content.trim();
    if (!trimmed || sendDisabled) return;

    const attachedPaths = uploadedFiles.map((f) => `\`./${f.name}\``);
    const body =
      attachedPaths.length > 0
        ? `> 📎 Attached files (already saved in the sandbox working directory): ${attachedPaths.join(", ")}\n\n${trimmed}`
        : trimmed;

    if (isLoading && onQueueMessage) {
      onQueueMessage(body);
      setContent("");
      setUploadedFiles([]);
      return;
    }

    setIsSubmitting(true);
    setContent("");
    try {
      await onSubmit(body);
      setUploadedFiles([]);
    } catch (error) {
      setContent(trimmed);
      throw error;
    } finally {
      setIsSubmitting(false);
    }
  }

  function handleEditQueue() {
    if (!queuedMessage) return;
    setContent(queuedMessage);
    onCancelQueue?.();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (slashOpen) {
      if (event.key === "Escape") {
        event.preventDefault();
        setSlashOpen(false);
        return;
      }
      // Arrow keys move the highlight (wrapping at both ends).
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        if (filteredCommands.length === 0) return;
        const delta = event.key === "ArrowDown" ? 1 : -1;
        setSlashIndex(
          (activeSlashIndex + delta + filteredCommands.length) % filteredCommands.length,
        );
        return;
      }
      // Enter picks the highlighted match instead of sending "/eda" raw.
      if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault();
        if (filteredCommands.length > 0) {
          pickSlashCommand(filteredCommands[activeSlashIndex]);
        }
        return;
      }
    }
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    event.currentTarget.form?.requestSubmit();
  }

  async function handleFileChange(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;

    setUploadError(null);
    setIsUploading(true);
    try {
      const result = await onUpload(file);
      setUploadedFiles((prev) => [...prev, { name: result.name, size: result.size }]);
    } catch (error) {
      setUploadError(error instanceof Error ? error.message : "Upload failed");
    } finally {
      setIsUploading(false);
    }
  }

  function dismissChip(name: string) {
    setUploadedFiles((prev) => prev.filter((f) => f.name !== name));
  }

  return (
    <form ref={formRef} className="composer" aria-label="Message composer" onSubmit={handleSubmit}>
      {queuedMessage ? (
        <div className="composer-queue" aria-label="Queued message" role="status">
          <span className="composer-queue-label">Queued ·</span>
          <span className="composer-queue-text">{queuedMessage}</span>
          <div className="composer-queue-actions">
            <button
              type="button"
              className="composer-queue-btn"
              onClick={handleEditQueue}
              aria-label="Edit queued message"
              title="Move queued message back to the composer"
            >
              <PencilLine size={12} aria-hidden="true" />
              Edit
            </button>
            <button
              type="button"
              className="composer-queue-btn composer-queue-btn--cancel"
              onClick={() => onCancelQueue?.()}
              aria-label="Cancel queued message"
              title="Drop queued message"
            >
              <X size={12} aria-hidden="true" />
              Cancel
            </button>
          </div>
        </div>
      ) : null}
      {uploadedFiles.length > 0 ? (
        <div className="composer-uploads" aria-label="Uploaded files">
          {uploadedFiles.map((file) => (
            <span key={file.name} className="composer-chip">
              <Paperclip size={12} aria-hidden="true" />
              {file.name}
              <button
                type="button"
                aria-label={`Dismiss ${file.name}`}
                onClick={() => dismissChip(file.name)}
              >
                <X size={12} />
              </button>
            </span>
          ))}
        </div>
      ) : null}
      {uploadError ? (
        <div className="composer-error" role="alert">
          {uploadError}
        </div>
      ) : null}
      {slashOpen ? (
        <div ref={menuRef} className="slash-menu" role="listbox" aria-label="Subagent commands">
          <p className="slash-menu-title">Direct a subagent · ↑↓ navigate · Enter select · Esc close</p>
          {filteredCommands.length > 0 ? (
            filteredCommands.map((command, index) => {
              const Icon = command.icon;
              const isActive = index === activeSlashIndex;
              return (
                <button
                  key={command.name}
                  type="button"
                  role="option"
                  aria-selected={isActive}
                  className={`slash-menu-item${isActive ? " selected" : ""}`}
                  onClick={() => pickSlashCommand(command)}
                  onMouseEnter={() => setSlashIndex(index)}
                >
                  <Icon size={15} aria-hidden="true" />
                  <span>{command.label}</span>
                  <span className="cmd">/{command.name}</span>
                </button>
              );
            })
          ) : (
            <p className="slash-menu-empty">No matching subagent command.</p>
          )}
        </div>
      ) : null}
      <textarea
        ref={textareaRef}
        aria-label="Message"
        placeholder="Refine the question, attach a CSV, or type / to direct a subagent..."
        value={content}
        onChange={handleContentChange}
        onKeyDown={handleKeyDown}
        rows={2}
      />
      <input
        type="file"
        ref={fileInputRef}
        onChange={handleFileChange}
        style={{ display: "none" }}
        aria-hidden="true"
        tabIndex={-1}
      />
      <div className="composer-row">
        <div className="left">
          <button
            type="button"
            className="composer-attach attach-btn"
            aria-label="Attach file"
            title={isUploading ? "Uploading..." : "Attach file"}
            disabled={uploadDisabled}
            onClick={() => fileInputRef.current?.click()}
          >
            <Paperclip size={16} />
          </button>
          <button
            type="button"
            className="composer-attach attach-btn"
            aria-label="Direct a subagent"
            title="Direct a subagent"
            aria-expanded={slashOpen}
            onClick={() => {
              setSlashOpen((open) => !open);
              setSlashIndex(0);
              textareaRef.current?.focus();
            }}
          >
            <Slash size={16} />
          </button>
        </div>
        <div className="right">
          <span className="hint">
            <kbd>Shift</kbd> + <kbd>Enter</kbd> newline · <kbd>Enter</kbd> send
          </span>
          <div className="composer-actions">
            <button
              type="submit"
              className="send-btn"
              aria-label="Send message"
              title="Send message"
              disabled={sendDisabled}
            >
              <SendHorizontal size={18} />
            </button>
          </div>
        </div>
      </div>
    </form>
  );
}

import { X } from "lucide-react";
import { useEffect, type ReactNode } from "react";

interface ConfirmModalProps {
  title: string;
  /** Optional icon rendered beside the body text. */
  icon?: ReactNode;
  body: ReactNode;
  confirmLabel: string;
  /** Label swapped in while `busy` is true (e.g. "Starting…"). */
  busyLabel?: string;
  cancelLabel?: string;
  busy?: boolean;
  /** Render the confirm button in the danger palette (e.g. sign out). */
  danger?: boolean;
  /** Inline error shown above the actions; keeps the modal open. */
  error?: string | null;
  /** Hide the confirm button (e.g. after an unrecoverable error). */
  hideConfirm?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}

/**
 * Small confirmation dialog matching the app's modal styling. Used for the
 * sign-out guard and the "turn on sandbox" prompt; Escape and overlay clicks
 * dismiss it (unless a confirm action is in flight).
 */
export function ConfirmModal({
  title,
  icon,
  body,
  confirmLabel,
  busyLabel,
  cancelLabel = "Cancel",
  busy = false,
  danger = false,
  error = null,
  hideConfirm = false,
  onConfirm,
  onClose,
}: ConfirmModalProps) {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape" && !busy) onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  return (
    <div
      className="image-lightbox confirm-modal"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <div
        className="image-lightbox-inner confirm-modal-inner"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="image-lightbox-header">
          <span>{title}</span>
          <div className="image-lightbox-actions">
            <button
              type="button"
              className="image-lightbox-close"
              aria-label="Close"
              disabled={busy}
              onClick={onClose}
            >
              <X size={16} />
            </button>
          </div>
        </header>

        <div className="confirm-modal-body">
          {icon ? (
            <div className="confirm-modal-icon" aria-hidden="true">
              {icon}
            </div>
          ) : null}
          <div className="confirm-modal-text">{body}</div>
        </div>

        {error ? (
          <p className="confirm-modal-error" role="alert">
            {error}
          </p>
        ) : null}

        <div className="confirm-modal-actions">
          <button
            type="button"
            className="confirm-modal-btn ghost"
            disabled={busy}
            onClick={onClose}
          >
            {cancelLabel}
          </button>
          {!hideConfirm ? (
            <button
              type="button"
              className={`confirm-modal-btn primary${danger ? " danger" : ""}`}
              disabled={busy}
              onClick={onConfirm}
            >
              {busy && busyLabel ? busyLabel : confirmLabel}
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}

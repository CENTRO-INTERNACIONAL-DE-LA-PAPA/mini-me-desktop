import { useEffect } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import type { ReportArtifact } from "../types";
import { MarkdownContent } from "./MarkdownContent";

interface ReportLightboxProps {
  report: ReportArtifact;
  onClose: () => void;
}

export function ReportLightbox({ report, onClose }: ReportLightboxProps) {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return createPortal(
    <div
      className="image-lightbox report-lightbox"
      role="dialog"
      aria-modal="true"
      aria-label={`Report: ${report.title}`}
      onClick={onClose}
    >
      <div
        className="image-lightbox-inner report-lightbox-inner"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="image-lightbox-header">
          <span>{report.title}</span>
          <div className="image-lightbox-actions">
            <button
              type="button"
              className="image-lightbox-close"
              aria-label="Close report"
              onClick={onClose}
            >
              <X size={16} />
            </button>
          </div>
        </header>
        <div className="report-lightbox-body">
          <MarkdownContent>{report.markdown}</MarkdownContent>
        </div>
      </div>
    </div>,
    document.body,
  );
}

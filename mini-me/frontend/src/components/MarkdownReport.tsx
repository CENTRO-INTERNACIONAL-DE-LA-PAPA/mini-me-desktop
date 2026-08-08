import { useState } from "react";
import { Eye, FileDown, Loader2 } from "lucide-react";
import type { ReportArtifact } from "../types";
import { ReportLightbox } from "./ReportLightbox";

interface MarkdownReportProps {
  report: ReportArtifact;
  onRender?: (report: ReportArtifact) => Promise<void>;
}

export function MarkdownReport({ report, onRender }: MarkdownReportProps) {
  const [showLightbox, setShowLightbox] = useState(false);
  const [isRendering, setIsRendering] = useState(false);
  const [renderError, setRenderError] = useState<string | null>(null);

  async function handleRender() {
    if (!onRender || isRendering) return;
    setRenderError(null);
    setIsRendering(true);
    try {
      await onRender(report);
    } catch (error) {
      setRenderError(error instanceof Error ? error.message : "Render failed");
    } finally {
      setIsRendering(false);
    }
  }

  return (
    <article className="report-card">
      <div className="report-actions">
        <h4>{report.title}</h4>
        <div className="report-action-buttons">
          <button
            type="button"
            className="report-action-button"
            aria-label="View report in fullscreen"
            title="View report in fullscreen"
            onClick={() => setShowLightbox(true)}
          >
            <Eye size={14} aria-hidden="true" />
          </button>
          {onRender ? (
            <button
              type="button"
              className="report-action-button"
              aria-label="Render report as PDF"
              title="Render Quarto PDF"
              disabled={isRendering}
              onClick={handleRender}
            >
              {isRendering ? (
                <Loader2 size={14} aria-hidden="true" className="report-action-spin" />
              ) : (
                <FileDown size={14} aria-hidden="true" />
              )}
            </button>
          ) : null}
        </div>
      </div>

      {renderError ? (
        <p className="report-error" role="alert">
          {renderError}
        </p>
      ) : null}

      {showLightbox ? (
        <ReportLightbox report={report} onClose={() => setShowLightbox(false)} />
      ) : null}
    </article>
  );
}

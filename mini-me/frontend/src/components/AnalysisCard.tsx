import { BarChart3, ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import type { DataFinding } from "../types";

// One DataVoyager finding: a headline with optional detail (collapsible) and a
// badge naming the chart that backs it. The chart image itself renders in the
// Images tab (surfaced by the file-sync middleware); here we only name it so the
// finding stays linked to its figure without a fragile inline image reference.
export function AnalysisFindingCard({ finding }: { finding: DataFinding }) {
  const [isExpanded, setIsExpanded] = useState(false);
  const hasDetail = Boolean(finding.detail);

  return (
    <article className="analysis-card">
      <div className="analysis-topline">
        <h4>{finding.title || "Finding"}</h4>
        {hasDetail ? (
          <button
            className="subagent-toggle"
            type="button"
            aria-expanded={isExpanded}
            aria-label={isExpanded ? "Collapse finding detail" : "Expand finding detail"}
            onClick={() => setIsExpanded((current) => !current)}
          >
            {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
          </button>
        ) : null}
      </div>

      {finding.chartPath ? (
        <div className="analysis-chart-ref">
          <BarChart3 size={12} aria-hidden="true" />
          <span>{finding.chartPath}</span>
        </div>
      ) : null}

      {isExpanded && hasDetail ? (
        <div className="analysis-detail">
          <p>{finding.detail}</p>
        </div>
      ) : null}
    </article>
  );
}

import { ChevronDown, ChevronRight, ExternalLink } from "lucide-react";
import { useState } from "react";
import type { SourceArtifact } from "../types";

export function SourceCard({ source }: { source: SourceArtifact }) {
  const [isExpanded, setIsExpanded] = useState(false);

  return (
    <article className="source-card">
      <div className="source-topline">
        <h4>{source.citation}</h4>
        <button
          className="subagent-toggle"
          type="button"
          aria-expanded={isExpanded}
          aria-label={isExpanded ? "Collapse source details" : "Expand source details"}
          onClick={() => setIsExpanded((current) => !current)}
        >
          {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </button>
      </div>

      {isExpanded ? (
        <>
          <p>{source.relevance}</p>
          {source.link ? (
            <a href={source.link} aria-label="Open source link" target="_blank" rel="noreferrer">
              Source
              <ExternalLink size={13} aria-hidden="true" />
            </a>
          ) : null}
        </>
      ) : null}
    </article>
  );
}

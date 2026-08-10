import { ChevronDown, ChevronRight, ExternalLink, Search } from "lucide-react";
import { useState } from "react";
import type { PaperRef, TheoryItem } from "../types";

function noveltyTier(score: number): "high" | "mid" | "low" {
  if (score >= 0.66) return "high";
  if (score >= 0.33) return "mid";
  return "low";
}

// Resolve the best link for a paper: a direct link when the pipeline supplied
// an identifier, otherwise a Semantic Scholar search on the citation text so
// the reference is always clickable.
function paperLink(paper: PaperRef): { href: string; direct: boolean } {
  if (paper.url) return { href: paper.url, direct: true };
  if (paper.doi) return { href: `https://doi.org/${paper.doi}`, direct: true };
  return {
    href: `https://www.semanticscholar.org/search?q=${encodeURIComponent(paper.citation)}&sort=relevance`,
    direct: false,
  };
}

function PaperLink({ paper }: { paper: PaperRef }) {
  const { href, direct } = paperLink(paper);
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      title={direct ? "Open paper" : "Search for this paper"}
    >
      {paper.citation}
      {direct ? (
        <ExternalLink size={12} aria-hidden="true" />
      ) : (
        <Search size={12} aria-hidden="true" />
      )}
    </a>
  );
}

export function TheoryCard({ theory, index }: { theory: TheoryItem; index: number }) {
  const [isExpanded, setIsExpanded] = useState(false);
  const hasPapers = theory.supportingPapers.length > 0 || theory.conflictingPapers.length > 0;

  return (
    <article className="theory-card">
      <div className="theory-topline">
        <h4>Theory {index + 1}</h4>
        <div className="theory-topline-meta">
          {typeof theory.noveltyScore === "number" ? (
            <span
              className={`novelty-badge ${noveltyTier(theory.noveltyScore)}`}
              title="Novelty against the retrieved literature (0–1)"
            >
              Novelty {theory.noveltyScore.toFixed(2)}
            </span>
          ) : null}
          {hasPapers ? (
            <button
              className="subagent-toggle"
              type="button"
              aria-expanded={isExpanded}
              aria-label={isExpanded ? "Collapse supporting evidence" : "Expand supporting evidence"}
              onClick={() => setIsExpanded((current) => !current)}
            >
              {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
            </button>
          ) : null}
        </div>
      </div>

      {theory.laws.length > 0 ? (
        <ul className="theory-laws">
          {theory.laws.map((law, lawIndex) => (
            <li key={lawIndex}>{law}</li>
          ))}
        </ul>
      ) : null}

      {isExpanded && hasPapers ? (
        <div className="theory-evidence">
          {theory.supportingPapers.length > 0 ? (
            <div className="theory-papers">
              <p className="theory-papers-label supporting">Supporting</p>
              <ul>
                {theory.supportingPapers.map((paper, paperIndex) => (
                  <li key={paperIndex}><PaperLink paper={paper} /></li>
                ))}
              </ul>
            </div>
          ) : null}
          {theory.conflictingPapers.length > 0 ? (
            <div className="theory-papers">
              <p className="theory-papers-label conflicting">Conflicting</p>
              <ul>
                {theory.conflictingPapers.map((paper, paperIndex) => (
                  <li key={paperIndex}><PaperLink paper={paper} /></li>
                ))}
              </ul>
            </div>
          ) : null}
        </div>
      ) : null}
    </article>
  );
}

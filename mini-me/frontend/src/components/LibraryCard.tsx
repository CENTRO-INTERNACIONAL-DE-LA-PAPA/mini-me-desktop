import { ChevronDown, ChevronRight, FileText, Search } from "lucide-react";
import { useState } from "react";
import type { IndexedPaper } from "../types";

export function LibrarySearchBox({
  onSearch,
}: {
  onSearch: (query: string) => void;
}) {
  const [query, setQuery] = useState("");

  const submit = () => {
    const trimmed = query.trim();
    if (!trimmed) return;
    onSearch(trimmed);
    setQuery("");
  };

  return (
    <div className="library-search">
      <input
        type="text"
        value={query}
        placeholder="Search this library…"
        aria-label="Search the document library"
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            submit();
          }
        }}
      />
      <button
        type="button"
        aria-label="Search library"
        disabled={!query.trim()}
        onClick={submit}
      >
        <Search size={15} aria-hidden="true" />
      </button>
    </div>
  );
}

export function LibraryCard({ paper }: { paper: IndexedPaper }) {
  const [isExpanded, setIsExpanded] = useState(false);
  const hasDetail = Boolean(paper.summary) || paper.tags.length > 0 || Boolean(paper.doi);

  return (
    <article className="library-card">
      <div className="library-topline">
        <h4>
          <FileText size={14} aria-hidden="true" />
          {paper.title || "Untitled document"}
        </h4>
        {hasDetail ? (
          <button
            className="subagent-toggle"
            type="button"
            aria-expanded={isExpanded}
            aria-label={isExpanded ? "Collapse document details" : "Expand document details"}
            onClick={() => setIsExpanded((current) => !current)}
          >
            {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
          </button>
        ) : null}
      </div>

      {paper.tags.length > 0 ? (
        <div className="library-tags">
          {paper.tags.map((tag, tagIndex) => (
            <span key={tagIndex} className="library-tag">
              {tag}
            </span>
          ))}
        </div>
      ) : null}

      {isExpanded ? (
        <div className="library-detail">
          {paper.summary ? <p>{paper.summary}</p> : null}
          {typeof paper.pageCount === "number" ? (
            <p className="library-meta">{paper.pageCount} pages</p>
          ) : null}
          {paper.doi ? <p className="library-meta">DOI: {paper.doi}</p> : null}
          {paper.path ? <p className="library-meta library-path">{paper.path}</p> : null}
        </div>
      ) : null}
    </article>
  );
}

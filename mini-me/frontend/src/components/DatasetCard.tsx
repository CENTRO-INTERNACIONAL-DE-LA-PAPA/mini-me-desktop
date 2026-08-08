import { ExternalLink } from "lucide-react";
import type { DatasetArtifact } from "../types";

export function DatasetCard({ dataset }: { dataset: DatasetArtifact }) {
  return (
    <article className="dataset-card">
      <div className="dataset-topline">
        <h4>{dataset.title}</h4>
        <span>{dataset.repository}</span>
      </div>
      <p>{dataset.description}</p>
      <dl>
        <div>
          <dt>Authors</dt>
          <dd>{dataset.authors.join(", ")}</dd>
        </div>
        <div>
          <dt>Files</dt>
          <dd>{dataset.fileCount ?? "Unknown"}</dd>
        </div>
        <div>
          <dt>Access</dt>
          <dd>{dataset.accessSummary ?? "Not inspected"}</dd>
        </div>
      </dl>
      <p className="recommendation">{dataset.recommendationReason}</p>
      {dataset.doiUrl ? (
        <a href={dataset.doiUrl} aria-label={`Open ${dataset.persistentId}`} target="_blank" rel="noreferrer">
          {dataset.persistentId}
          <ExternalLink size={13} aria-hidden="true" />
        </a>
      ) : (
        <span className="artifact-inline-id">{dataset.persistentId}</span>
      )}
    </article>
  );
}

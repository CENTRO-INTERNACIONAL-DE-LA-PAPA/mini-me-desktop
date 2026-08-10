import { useState } from "react";
import { Download, Loader2 } from "lucide-react";
import { downloadFile } from "../lib/fileClient";
import type { FileArtifact } from "../types";

interface FileCardProps {
  file: FileArtifact;
  threadId: string | null;
}

export function FileCard({ file, threadId }: FileCardProps) {
  const canDownload = Boolean(threadId && file.relativePath);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleDownload() {
    if (!threadId || !file.relativePath) return;
    setBusy(true);
    setError(null);
    try {
      await downloadFile(threadId, file.relativePath, file.name);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Download failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <article className="file-card">
      <div className="file-topline">
        <h4>{file.name}</h4>
        <span>{file.mediaType ?? "Generated file"}</span>
      </div>
      {file.description ? <p>{file.description}</p> : null}
      <code>{file.path}</code>
      {canDownload ? (
        <button
          type="button"
          className="file-download"
          onClick={handleDownload}
          disabled={busy}
        >
          {busy ? (
            <Loader2 size={14} className="status-spinner--xs" aria-hidden="true" />
          ) : (
            <Download size={14} aria-hidden="true" />
          )}
          {busy ? "Downloading…" : "Download"}
        </button>
      ) : null}
      {error ? <p className="file-error" role="alert">{error}</p> : null}
    </article>
  );
}

import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronDown, ChevronUp, Download, Loader2, X } from "lucide-react";
import { downloadFile, useAuthedImage } from "../lib/fileClient";
import type { FileArtifact } from "../types";

interface ImageGridProps {
  images: FileArtifact[];
  threadId: string | null;
  cap?: number;
}

function ThumbImage({
  threadId,
  image,
  onClick,
}: {
  threadId: string | null;
  image: FileArtifact;
  onClick: () => void;
}) {
  const { src, loading, error } = useAuthedImage(threadId, image.relativePath ?? null);
  return (
    <button
      type="button"
      className="image-thumb"
      title={image.name}
      aria-label={`Open ${image.name}`}
      onClick={onClick}
    >
      {src ? (
        <img src={src} alt={image.name} loading="lazy" />
      ) : (
        <span className="image-thumb-placeholder" aria-hidden="true">
          {error ? <X size={16} /> : <Loader2 size={16} className="status-spinner--xs" />}
        </span>
      )}
      <span className="image-thumb-name">{image.name}</span>
    </button>
  );
}

function LightboxImage({
  threadId,
  image,
}: {
  threadId: string | null;
  image: FileArtifact;
}) {
  const { src, loading, error } = useAuthedImage(threadId, image.relativePath ?? null);
  if (error) return <p className="image-lightbox-error">Couldn't load this image.</p>;
  if (!src || loading) {
    return (
      <div className="image-lightbox-loading">
        <Loader2 size={24} className="status-spinner--xs" aria-hidden="true" />
      </div>
    );
  }
  return <img src={src} alt={image.name} />;
}

export function ImageGrid({ images, threadId, cap = 9 }: ImageGridProps) {
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [downloading, setDownloading] = useState(false);

  const overflow = Math.max(0, images.length - cap);
  const visibleImages = expanded || overflow === 0 ? images : images.slice(0, cap);

  useEffect(() => {
    if (activeIndex === null) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setActiveIndex(null);
      if (e.key === "ArrowRight")
        setActiveIndex((i) => (i === null ? null : Math.min(images.length - 1, i + 1)));
      if (e.key === "ArrowLeft")
        setActiveIndex((i) => (i === null ? null : Math.max(0, i - 1)));
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeIndex, images.length]);

  if (!images.length) return null;

  const active = activeIndex !== null ? images[activeIndex] : null;

  async function handleDownload() {
    if (!active || !threadId || !active.relativePath) return;
    setDownloading(true);
    try {
      await downloadFile(threadId, active.relativePath, active.name);
    } catch {
      // surfaced via the disabled state resetting; keep the lightbox open
    } finally {
      setDownloading(false);
    }
  }

  return (
    <>
      <div className="image-grid">
        {visibleImages.map((image, index) => {
          if (!threadId || !image.relativePath) return null;
          return (
            <ThumbImage
              key={image.path}
              threadId={threadId}
              image={image}
              onClick={() => setActiveIndex(index)}
            />
          );
        })}
      </div>

      {overflow > 0 ? (
        <button
          className="cap-expand-toggle"
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((current) => !current)}
        >
          {expanded ? (
            <>
              <ChevronUp size={14} aria-hidden="true" />
              Show fewer
            </>
          ) : (
            <>
              <ChevronDown size={14} aria-hidden="true" />
              Show all ({images.length} images)
            </>
          )}
        </button>
      ) : null}

      {active
        ? createPortal(
            <div
              className="image-lightbox"
              role="dialog"
              aria-modal="true"
              aria-label={`Preview of ${active.name}`}
              onClick={() => setActiveIndex(null)}
            >
              <div className="image-lightbox-inner" onClick={(e) => e.stopPropagation()}>
                <header className="image-lightbox-header">
                  <span>{active.name}</span>
                  <div className="image-lightbox-actions">
                    {threadId && active.relativePath ? (
                      <button
                        type="button"
                        className="image-lightbox-download"
                        onClick={handleDownload}
                        disabled={downloading}
                      >
                        {downloading ? (
                          <Loader2 size={14} className="status-spinner--xs" aria-hidden="true" />
                        ) : (
                          <Download size={14} aria-hidden="true" />
                        )}
                        {downloading ? "Downloading…" : "Download"}
                      </button>
                    ) : null}
                    <button
                      type="button"
                      className="image-lightbox-close"
                      aria-label="Close preview"
                      onClick={() => setActiveIndex(null)}
                    >
                      <X size={16} />
                    </button>
                  </div>
                </header>
                <LightboxImage threadId={threadId} image={active} />
              </div>
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

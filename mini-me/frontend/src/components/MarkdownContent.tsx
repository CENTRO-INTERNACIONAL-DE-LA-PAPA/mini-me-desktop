import { lazy, memo, Suspense, useDeferredValue } from "react";
import { useThrottledValue } from "../lib/useThrottledValue";

const STREAM_THROTTLE_MS = 100; // cap re-parses to ~10 fps

// The markdown parser is the heaviest dependency in the initial bundle and is
// never needed before the first message renders, so it loads on demand.
const MarkdownRenderer = lazy(() => import("./MarkdownRenderer"));

function MarkdownContentImpl({ children }: { children: string }) {
  // Three layers of relief during streaming:
  //  1. Throttle: cap parser invocations to ~10 fps so a long message
  //     re-parses ~10 times instead of hundreds.
  //  2. Defer: render markdown at lower priority so the page stays
  //     interactive (scroll, clicks, typing) even when the parser is busy.
  //  3. Memo (below): static messages skip parsing entirely.
  const throttled = useThrottledValue(children, STREAM_THROTTLE_MS);
  const deferred = useDeferredValue(throttled);
  return (
    <div className="markdown-body">
      <Suspense fallback={<p className="md-plain-fallback">{deferred}</p>}>
        <MarkdownRenderer>{deferred}</MarkdownRenderer>
      </Suspense>
    </div>
  );
}

export const MarkdownContent = memo(MarkdownContentImpl);

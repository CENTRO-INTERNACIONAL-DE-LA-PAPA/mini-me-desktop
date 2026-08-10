import { useEffect, useRef, useState } from "react";

/**
 * Returns a throttled view of `value` that updates at most once every
 * `delay` milliseconds. The trailing value always lands so the final state
 * (e.g., the last token of a streamed message) is never dropped.
 *
 * Used to cap how often expensive renders (markdown parsing) run during
 * fast-moving inputs like LLM token streaming.
 */
export function useThrottledValue<T>(value: T, delay = 100): T {
  const [throttled, setThrottled] = useState(value);
  const lastUpdate = useRef<number>(
    typeof performance !== "undefined" ? performance.now() : Date.now(),
  );
  const timer = useRef<number | null>(null);

  useEffect(() => {
    const now = typeof performance !== "undefined" ? performance.now() : Date.now();
    const elapsed = now - lastUpdate.current;

    function commit() {
      lastUpdate.current =
        typeof performance !== "undefined" ? performance.now() : Date.now();
      setThrottled(value);
      timer.current = null;
    }

    if (elapsed >= delay) {
      commit();
      return;
    }

    if (timer.current !== null) {
      window.clearTimeout(timer.current);
    }
    timer.current = window.setTimeout(commit, delay - elapsed);

    return () => {
      if (timer.current !== null) {
        window.clearTimeout(timer.current);
        timer.current = null;
      }
    };
  }, [value, delay]);

  return throttled;
}

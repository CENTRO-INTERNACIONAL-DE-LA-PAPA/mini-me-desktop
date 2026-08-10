import { useCallback, useLayoutEffect, useRef } from "react";

/**
 * Returns a callback with a permanently stable identity that always invokes
 * the latest `fn`. Lets memoized children (React.memo) skip re-renders that
 * would otherwise be forced by handler props being recreated on every parent
 * render, without the stale-closure hazards of hand-managed dependency arrays.
 *
 * Only safe for callbacks invoked from events/effects (never during render) —
 * the ref is synced in a layout effect, so it is current by the time any
 * event handler or effect can fire.
 */
export function useStableCallback<Args extends unknown[], R>(
  fn: (...args: Args) => R,
): (...args: Args) => R {
  const ref = useRef(fn);
  useLayoutEffect(() => {
    ref.current = fn;
  });
  return useCallback((...args: Args) => ref.current(...args), []);
}

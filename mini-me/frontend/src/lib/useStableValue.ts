/* eslint-disable react-hooks/refs -- intentional render-time cache keyed by signature */
import { useRef } from "react";

/**
 * Returns a referentially stable value for as long as `signature` is
 * unchanged. The LangGraph stream rebuilds `values`/`subagents` as fresh
 * objects on every tick, so arrays derived from them get new identities even
 * when their content is identical — which defeats React.memo on the panels
 * they feed. This collapses "same content, new reference" back to the
 * previous reference.
 */
export function useStableValue<T>(value: T, signature: string): T {
  const ref = useRef({ signature, value });
  if (ref.current.signature !== signature) {
    ref.current = { signature, value };
  }
  return ref.current.value;
}

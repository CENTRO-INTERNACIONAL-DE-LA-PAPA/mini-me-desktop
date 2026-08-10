import { useEffect, useState } from "react";
import { loadModelConfig } from "../lib/llmConfig";
import {
  fetchAstaStatus,
  formatExpiry,
  getAstaTokenLocal,
  localTokenStatus,
  type AstaStatus,
} from "../lib/astaClient";

const EMPTY: AstaStatus = { connected: false, expires_at: null, expired: false, seconds_left: null };

// Soon-to-expire threshold — mirrors the warn cutoff in AstaConnectionCard.
const WARN_SECONDS = 2 * 86400;

type Health = "connected" | "warn" | "off";

function health(status: AstaStatus): Health {
  if (!status.connected || status.expired) return "off";
  if ((status.seconds_left ?? Infinity) < WARN_SECONDS) return "warn";
  return "connected";
}

/**
 * A compact Asta auth indicator for the top bar: a green / amber / red dot that
 * reflects whether the `asta` CLI in the sandbox can authenticate. Clicking it
 * opens Settings (where the AstaConnectionCard lets the user paste a fresh
 * token). This surfaces the "you need to log in to Asta" state the user
 * otherwise only discovered when the theorizer/DataVoyager silently stalled.
 *
 * `refreshSignal` is bumped by the parent (e.g. when the Settings panel closes)
 * so the dot re-reads status right after the user updates their token.
 */
export function AstaStatusDot({
  onOpenSettings,
  refreshSignal = 0,
}: {
  onOpenSettings: () => void;
  refreshSignal?: number;
}) {
  const [status, setStatus] = useState<AstaStatus>(EMPTY);

  useEffect(() => {
    let cancelled = false;
    const mode = loadModelConfig().storage_mode;
    if (mode === "client") {
      setStatus(localTokenStatus(getAstaTokenLocal()));
      return;
    }
    void fetchAstaStatus()
      .then((s) => {
        if (!cancelled) setStatus(s);
      })
      .catch(() => {
        if (!cancelled) setStatus(EMPTY);
      });
    return () => {
      cancelled = true;
    };
  }, [refreshSignal]);

  const state = health(status);
  const label = `Asta: ${formatExpiry(status)}`;

  return (
    <button
      type="button"
      className={`asta-dot-button asta-dot--${state}`}
      aria-label={label}
      title={label}
      onClick={onOpenSettings}
    >
      <span className="asta-dot" aria-hidden="true" />
      <span className="asta-dot-text">Asta</span>
    </button>
  );
}

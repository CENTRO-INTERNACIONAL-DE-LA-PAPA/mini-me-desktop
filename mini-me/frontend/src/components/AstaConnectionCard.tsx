import { useEffect, useState } from "react";
import { AlertTriangle, Check, Eye, EyeOff, Loader2, Sparkles, Trash2 } from "lucide-react";
import type { StorageMode } from "../lib/llmConfig";
import {
  deleteAstaTokenRemote,
  fetchAstaStatus,
  formatExpiry,
  getAstaTokenLocal,
  localTokenStatus,
  saveAstaTokenRemote,
  setAstaTokenLocal,
  type AstaStatus,
} from "../lib/astaClient";

const EMPTY: AstaStatus = { connected: false, expires_at: null, expired: false, seconds_left: null };

// Self-service Asta token refresh. The token authenticates the `asta` CLI in the
// sandbox (theorizer, DataVoyager, PDF tools) and expires ~weekly; pasting a
// fresh one here updates it in seconds, no redeploy. Mirrors the two storage
// modes of the API-key manager: "vault" persists server-side, "client" keeps it
// in this browser and passes it on each run.
export function AstaConnectionCard({ storageMode }: { storageMode: StorageMode }) {
  const [status, setStatus] = useState<AstaStatus>(EMPTY);
  const [draft, setDraft] = useState("");
  const [show, setShow] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    if (storageMode === "client") {
      setStatus(localTokenStatus(getAstaTokenLocal()));
      return;
    }
    setBusy(true);
    void fetchAstaStatus()
      .then((s) => {
        if (!cancelled) setStatus(s);
      })
      .catch(() => {
        if (!cancelled) setError("Couldn't load Asta status.");
      })
      .finally(() => {
        if (!cancelled) setBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [storageMode]);

  async function save() {
    const token = draft.trim();
    if (token.length < 16) {
      setError("Paste the full token from `asta auth print-token`.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (storageMode === "client") {
        const s = localTokenStatus(token);
        if (s.expired) {
          setError("That token is already expired — run `asta auth login` again.");
          return;
        }
        setAstaTokenLocal(token);
        setStatus(s);
      } else {
        setStatus(await saveAstaTokenRemote(token));
      }
      setDraft("");
      setShow(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Save failed.");
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    setBusy(true);
    setError(null);
    try {
      if (storageMode === "client") setAstaTokenLocal(null);
      else await deleteAstaTokenRemote();
      setStatus(EMPTY);
    } catch {
      setError("Remove failed.");
    } finally {
      setBusy(false);
    }
  }

  const expiryLabel = formatExpiry(status);
  const warn = status.connected && (status.expired || (status.seconds_left ?? Infinity) < 2 * 86400);

  return (
    <section className="asta-conn">
      <div className="asta-conn-head">
        <Sparkles size={15} aria-hidden="true" />
        <h4>Asta connection</h4>
        {status.connected ? (
          <span className={`asta-conn-pill${warn ? " asta-conn-pill--warn" : ""}`}>
            {warn ? <AlertTriangle size={12} aria-hidden="true" /> : <Check size={12} aria-hidden="true" />}
            {expiryLabel}
          </span>
        ) : (
          <span className="asta-conn-pill asta-conn-pill--off">Not connected</span>
        )}
      </div>

      <p className="asta-conn-help">
        Authenticates the theorizer, DataVoyager, and PDF tools. Tokens expire — when
        they do, get a fresh one in a terminal with{" "}
        <code>asta auth login</code> then <code>asta auth print-token</code>, and paste it below.
      </p>

      <div className="asta-conn-row">
        <div className="asta-conn-input">
          <input
            type={show ? "text" : "password"}
            aria-label="Asta token"
            placeholder={status.connected ? "Paste a new token to update…" : "Paste your Asta token…"}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void save();
            }}
          />
          <button
            type="button"
            className="asta-conn-eye"
            aria-label={show ? "Hide token" : "Show token"}
            onClick={() => setShow((v) => !v)}
          >
            {show ? <EyeOff size={14} aria-hidden="true" /> : <Eye size={14} aria-hidden="true" />}
          </button>
        </div>
        <button type="button" className="asta-conn-save" disabled={busy || !draft.trim()} onClick={() => void save()}>
          {busy ? <Loader2 size={14} className="asta-spin" aria-hidden="true" /> : <Check size={14} aria-hidden="true" />}
          {status.connected ? "Update" : "Save"}
        </button>
        {status.connected ? (
          <button type="button" className="asta-conn-remove" aria-label="Remove Asta token" disabled={busy} onClick={() => void remove()}>
            <Trash2 size={14} aria-hidden="true" />
          </button>
        ) : null}
      </div>

      {error ? <p className="asta-conn-error">{error}</p> : null}
      <p className="asta-conn-mode">
        {storageMode === "vault"
          ? "Stored encrypted in your account (WorkOS Vault)."
          : "Stored in this browser only; sent with each run."}
      </p>
    </section>
  );
}

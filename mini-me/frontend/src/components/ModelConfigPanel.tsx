import { useEffect, useMemo, useState } from "react";
import { X, Check, Eye, EyeOff, KeyRound, ShieldCheck, Plus, Trash2, Loader2 } from "lucide-react";
import { branding } from "../branding";
import { AstaConnectionCard } from "./AstaConnectionCard";
import {
  PROVIDERS,
  SUBAGENTS,
  loadModelConfig,
  saveModelConfig,
  loadClientKeys,
  saveClientKeys,
  fetchRemoteConfig,
  saveRemoteConfig,
  saveRemoteKey,
  deleteRemoteKey,
  testKey,
  parseSpec,
  type ModelConfig,
  type StorageMode,
  type ProviderInfo,
} from "../lib/llmConfig";

interface ModelConfigPanelProps {
  onClose: () => void;
}

type ConnState = "idle" | "testing" | "ok" | "err";

interface ProviderUI {
  connected: boolean;
  editing: boolean;
  draftKey: string;
  draftBase: string;
  showKey: boolean;
  state: ConnState;
  message: string;
}

function emptyProviderUI(): ProviderUI {
  return {
    connected: false,
    editing: false,
    draftKey: "",
    draftBase: "",
    showKey: false,
    state: "idle",
    message: "",
  };
}

function firstModelSpec(provider: ProviderInfo): string {
  return `${provider.id}::${provider.models[0].id}`;
}

export function ModelConfigPanel({ onClose }: ModelConfigPanelProps) {
  const [config, setConfig] = useState<ModelConfig>(() => loadModelConfig());
  const [ui, setUi] = useState<Record<string, ProviderUI>>(() => {
    const init: Record<string, ProviderUI> = {};
    for (const p of PROVIDERS) init[p.id] = emptyProviderUI();
    return init;
  });
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [savedNote, setSavedNote] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const storageMode = config.storage_mode;

  const setProvider = (id: string, patch: Partial<ProviderUI>) =>
    setUi((prev) => ({ ...prev, [id]: { ...prev[id], ...patch } }));

  // Load connected state on open + whenever the storage mode flips.
  useEffect(() => {
    let cancelled = false;
    setLoadError(null);
    if (storageMode === "client") {
      const keys = loadClientKeys();
      setUi((prev) => {
        const next = { ...prev };
        for (const p of PROVIDERS) {
          next[p.id] = { ...next[p.id], connected: Boolean(keys[p.id]?.api_key) };
        }
        return next;
      });
      return;
    }
    // vault mode → ask the backend which providers are connected
    setLoading(true);
    void fetchRemoteConfig()
      .then((remote) => {
        if (cancelled) return;
        setUi((prev) => {
          const next = { ...prev };
          for (const p of PROVIDERS) {
            next[p.id] = {
              ...next[p.id],
              connected: remote.providers_connected.includes(p.id),
            };
          }
          return next;
        });
        if (remote.model_config) {
          setConfig((c) => ({
            ...remote.model_config!,
            storage_mode: "vault",
          }));
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoadError(e instanceof Error ? e.message : "Failed to load");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [storageMode]);

  const connectedIds = useMemo(
    () => PROVIDERS.filter((p) => ui[p.id]?.connected).map((p) => p.id),
    [ui],
  );

  // Keep the default spec valid as providers connect/disconnect.
  useEffect(() => {
    setConfig((c) => {
      if (parseSpec(c.default) && connectedIds.includes(c.default.split("::")[0])) return c;
      if (connectedIds.length === 0) return c;
      const firstProvider = PROVIDERS.find((p) => p.id === connectedIds[0])!;
      return { ...c, default: firstModelSpec(firstProvider) };
    });
  }, [connectedIds]);

  async function handleTestConnect(provider: ProviderInfo) {
    const u = ui[provider.id];
    const key = u.draftKey.trim();
    const base = u.draftBase.trim();
    if (!provider.custom && key.length < 6) {
      setProvider(provider.id, { state: "err", message: "Enter a valid API key first." });
      return;
    }
    if (provider.custom && !base) {
      setProvider(provider.id, { state: "err", message: "Enter an endpoint base URL." });
      return;
    }
    setProvider(provider.id, { state: "testing", message: `Reaching ${provider.name}…` });
    const result = await testKey({
      provider: provider.id,
      model_id: provider.models[0].id,
      api_key: key || undefined,
      base_url: base || null,
    });
    if (!result.ok) {
      setProvider(provider.id, { state: "err", message: result.error ?? "Connection failed." });
      return;
    }
    // Persist per mode.
    try {
      if (storageMode === "vault") {
        await saveRemoteKey(provider.id, key, base || null);
      } else {
        const keys = loadClientKeys();
        keys[provider.id] = { api_key: key, base_url: base || null };
        saveClientKeys(keys);
      }
    } catch (e) {
      setProvider(provider.id, {
        state: "err",
        message: e instanceof Error ? e.message : "Save failed.",
      });
      return;
    }
    setProvider(provider.id, {
      connected: true,
      editing: false,
      state: "ok",
      message: `${provider.name} connected.`,
      draftKey: "",
    });
  }

  async function handleDisconnect(provider: ProviderInfo) {
    try {
      if (storageMode === "vault") {
        await deleteRemoteKey(provider.id);
      } else {
        const keys = loadClientKeys();
        delete keys[provider.id];
        saveClientKeys(keys);
      }
    } catch {
      /* best-effort */
    }
    setProvider(provider.id, { ...emptyProviderUI() });
  }

  function handleToggleMode(mode: StorageMode) {
    setConfig((c) => ({ ...c, storage_mode: mode }));
  }

  function setSubagentModel(name: string, spec: string) {
    setConfig((c) => {
      const subagents = { ...c.subagents };
      if (spec === "__default__") delete subagents[name];
      else subagents[name] = spec;
      return { ...c, subagents };
    });
  }

  const canSave = connectedIds.length > 0 && Boolean(parseSpec(config.default));

  async function handleSave() {
    if (!canSave) return;
    setSaving(true);
    try {
      saveModelConfig(config); // always: localStorage is the live source for runs
      if (storageMode === "vault") {
        await saveRemoteConfig(config); // mirror for cross-device
      }
      const overrides = Object.keys(config.subagents).length;
      setSavedNote(
        `Saved · ${connectedIds.length} provider${connectedIds.length > 1 ? "s" : ""}` +
          (overrides ? ` + ${overrides} override${overrides > 1 ? "s" : ""}` : ""),
      );
      setTimeout(() => setSavedNote(null), 3000);
    } catch (e) {
      setSavedNote(e instanceof Error ? e.message : "Save failed");
    } finally {
      setSaving(false);
    }
  }

  const defaultParsed = parseSpec(config.default);

  return (
    <div className="mcfg-overlay" role="dialog" aria-modal="true" aria-label="Model & API configuration">
      <div className="mcfg-modal">
        <header className="mcfg-head">
          <div>
            <div className="mcfg-ey">Model &amp; API</div>
            <h2>Connect models &amp; route subagents</h2>
            <p>{branding.appName} ships no model — bring your own. Connect each provider you want, pick a default, and optionally give each subagent its own model.</p>
          </div>
          <button type="button" className="mcfg-close" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </header>

        <div className="mcfg-body">
          {/* Storage mode toggle */}
          <section className="mcfg-group">
            <div className="mcfg-fg-head">
              <span className="mcfg-n">0</span>
              <h3>Where your keys live</h3>
            </div>
            <div className="mcfg-toggle-row">
              <button
                type="button"
                className={`mcfg-toggle ${storageMode === "vault" ? "active" : ""}`}
                onClick={() => handleToggleMode("vault")}
              >
                <ShieldCheck size={15} /> Save to my workspace (WorkOS Vault)
              </button>
              <button
                type="button"
                className={`mcfg-toggle ${storageMode === "client" ? "active" : ""}`}
                onClick={() => handleToggleMode("client")}
              >
                <KeyRound size={15} /> Keep on this device only
              </button>
            </div>
            <p className="mcfg-sec-note">
              {storageMode === "vault" ? (
                <>
                  <b>Set once, everywhere.</b> Keys are encrypted at rest in WorkOS Vault, scoped to
                  your account, and read server-side at run time — they never return to the browser.
                </>
              ) : (
                <>
                  <b>Stays on this device.</b> Keys live only in this browser and travel with each run
                  under a private field that is never written to logs or traces. They don&apos;t follow
                  you to another device.
                </>
              )}
            </p>
            {loadError ? <p className="mcfg-err-text">{loadError}</p> : null}
          </section>

          {/* Providers */}
          <section className="mcfg-group">
            <div className="mcfg-fg-head">
              <span className="mcfg-n">1</span>
              <h3>Providers &amp; API keys</h3>
              <span className="mcfg-opt">
                {loading ? "loading…" : `${connectedIds.length} connected`}
              </span>
            </div>
            <div className="mcfg-provider-list">
              {PROVIDERS.map((p) => {
                const u = ui[p.id];
                return (
                  <div key={p.id} className={`mcfg-provider ${u.connected ? "connected" : ""}`}>
                    <div className="mcfg-pv-row">
                      <span
                        className="mcfg-pv-mark"
                        style={{ background: `linear-gradient(135deg, ${p.hue[0]}, ${p.hue[1]})` }}
                      >
                        {p.abbr}
                      </span>
                      <div className="mcfg-pv-info">
                        <div className="mcfg-pv-name">
                          {p.name}
                          {u.connected ? <span className="mcfg-pv-badge"><Check size={11} /> Connected</span> : null}
                        </div>
                        <div className="mcfg-pv-sub">
                          {p.docs} · {p.models.length} model{p.models.length > 1 ? "s" : ""}
                        </div>
                      </div>
                      {u.connected ? (
                        <button type="button" className="mcfg-btn-sm line" onClick={() => handleDisconnect(p)}>
                          <Trash2 size={13} /> Disconnect
                        </button>
                      ) : (
                        <button
                          type="button"
                          className="mcfg-btn-sm line"
                          onClick={() => setProvider(p.id, { editing: !u.editing })}
                        >
                          <Plus size={13} /> Add key
                        </button>
                      )}
                    </div>

                    {u.editing && !u.connected ? (
                      <div className="mcfg-pv-editor">
                        <div className="mcfg-control">
                          <KeyRound size={15} className="mcfg-lead" />
                          <input
                            className="mcfg-mono"
                            type={u.showKey ? "text" : "password"}
                            autoComplete="off"
                            spellCheck={false}
                            placeholder={p.custom ? "endpoint key (optional)" : p.prefix ? `${p.prefix}...` : "paste API key"}
                            value={u.draftKey}
                            onChange={(e) => setProvider(p.id, { draftKey: e.target.value })}
                          />
                          <button type="button" className="mcfg-ghost" onClick={() => setProvider(p.id, { showKey: !u.showKey })}>
                            {u.showKey ? <EyeOff size={13} /> : <Eye size={13} />}
                          </button>
                        </div>
                        {p.custom ? (
                          <div className="mcfg-control">
                            <input
                              className="mcfg-mono"
                              type="text"
                              autoComplete="off"
                              spellCheck={false}
                              placeholder="https://api.your-host.ai/v1"
                              value={u.draftBase}
                              onChange={(e) => setProvider(p.id, { draftBase: e.target.value })}
                            />
                          </div>
                        ) : null}
                        <div className={`mcfg-status ${u.state}`}>
                          {u.state === "testing" ? <Loader2 size={13} className="mcfg-spin" /> : <span className="mcfg-led" />}
                          <span>{u.message || "Paste a key, then test the connection."}</span>
                        </div>
                        <div className="mcfg-editor-actions">
                          <button
                            type="button"
                            className="mcfg-btn-sm solid"
                            disabled={u.state === "testing"}
                            onClick={() => void handleTestConnect(p)}
                          >
                            Test &amp; connect
                          </button>
                          <button type="button" className="mcfg-btn-sm line" onClick={() => setProvider(p.id, { editing: false, state: "idle", message: "" })}>
                            Cancel
                          </button>
                        </div>
                      </div>
                    ) : null}
                  </div>
                );
              })}
            </div>
          </section>

          {/* Default model */}
          <section className="mcfg-group">
            <div className="mcfg-fg-head">
              <span className="mcfg-n">2</span>
              <h3>Default model</h3>
              <span className="mcfg-opt">required</span>
            </div>
            <p className="mcfg-desc">Used for the coordinator and any subagent left on “Use default”.</p>
            <ModelSelect
              value={config.default}
              connectedIds={connectedIds}
              includeDefault={false}
              onChange={(v) => setConfig((c) => ({ ...c, default: v }))}
            />
            {defaultParsed ? (
              <div className="mcfg-meta">
                <span className="mcfg-chip"><b>{defaultParsed.provider.name}</b></span>
                <span className="mcfg-chip"><b>{defaultParsed.model.label}</b></span>
                <span className="mcfg-chip">context <b>{defaultParsed.model.ctx}</b></span>
                <span className="mcfg-chip">{defaultParsed.model.note}</span>
              </div>
            ) : (
              <p className="mcfg-empty">Connect a provider above to choose a model.</p>
            )}
          </section>

          {/* Subagent routing */}
          <section className="mcfg-group">
            <div className="mcfg-fg-head">
              <span className="mcfg-n">3</span>
              <h3>Subagent models</h3>
              <span className="mcfg-opt">cross-provider</span>
            </div>
            <p className="mcfg-desc">Assign each subagent its own model, or leave on “Use default”.</p>
            <div className="mcfg-sub-list">
              {SUBAGENTS.map((s) => {
                const override = config.subagents[s.id];
                const resolved = parseSpec(override ?? config.default);
                return (
                  <div key={s.id} className="mcfg-sub-row">
                    <div className="mcfg-sub-info">
                      <div className="mcfg-sub-nm">{s.id}</div>
                      <div className="mcfg-sub-task">{s.task}</div>
                    </div>
                    <div className="mcfg-sub-pick">
                      <ModelSelect
                        value={override ?? "__default__"}
                        connectedIds={connectedIds}
                        includeDefault
                        onChange={(v) => setSubagentModel(s.id, v)}
                      />
                      {resolved ? (
                        <div className="mcfg-resolved">
                          {override ? "" : "inherits default · "}
                          {resolved.provider.name} · {resolved.model.label}
                        </div>
                      ) : null}
                    </div>
                  </div>
                );
              })}
            </div>
          </section>

          <AstaConnectionCard storageMode={storageMode} />
        </div>

        <footer className="mcfg-actions">
          <span className="mcfg-save-note">
            {savedNote ??
              (connectedIds.length === 0
                ? "Connect a provider to begin"
                : !canSave
                  ? "Pick a default model"
                  : `${connectedIds.length} provider${connectedIds.length > 1 ? "s" : ""} ready`)}
          </span>
          <span className="mcfg-spacer" />
          <button type="button" className="mcfg-btn ghost" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="mcfg-btn primary" disabled={!canSave || saving} onClick={() => void handleSave()}>
            {saving ? <Loader2 size={15} className="mcfg-spin" /> : <Check size={15} />}
            Save configuration
          </button>
        </footer>
      </div>
    </div>
  );
}

function ModelSelect({
  value,
  connectedIds,
  includeDefault,
  onChange,
}: {
  value: string;
  connectedIds: string[];
  includeDefault: boolean;
  onChange: (v: string) => void;
}) {
  const isDefault = value === "__default__";
  return (
    <div className={`mcfg-select ${isDefault ? "is-default" : ""}`}>
      <select value={value} onChange={(e) => onChange(e.target.value)} disabled={connectedIds.length === 0 && !includeDefault}>
        {includeDefault ? <option value="__default__">Use default model</option> : null}
        {connectedIds.length === 0 ? (
          <option value="" disabled>
            — connect a provider above —
          </option>
        ) : (
          connectedIds.map((pid) => {
            const provider = PROVIDERS.find((p) => p.id === pid)!;
            return (
              <optgroup key={pid} label={provider.name}>
                {provider.models.map((m) => (
                  <option key={m.id} value={`${pid}::${m.id}`}>
                    {m.label} · {m.ctx}
                  </option>
                ))}
              </optgroup>
            );
          })
        )}
      </select>
    </div>
  );
}

// Bring-your-own model & API-key configuration.
//
// Two storage modes, chosen by a single global toggle:
//   - "vault":  keys are stored server-side in WorkOS Vault (set once,
//               cross-device). The browser never holds key material; the
//               backend reads them from Vault at run time by user id.
//   - "client": keys live only in this browser's localStorage and ride along
//               each run in `configurable.__llm_keys` (a dict + `__` prefix, so
//               LangGraph never copies them into trace metadata).
//
// Non-secret routing (default model + per-subagent overrides) is always kept
// in localStorage as the live source for every run, and additionally mirrored
// to Vault in vault mode for cross-device restore.

import { LANGGRAPH_API_URL } from "./streamConfig";
import { getAstaTokenLocal } from "./astaClient";

export type StorageMode = "vault" | "client";

export interface ProviderModel {
  id: string;
  label: string;
  ctx: string;
  note: string;
}

export interface ProviderInfo {
  id: string;
  name: string;
  abbr: string;
  hue: [string, string];
  docs: string;
  prefix: string;
  custom?: boolean;
  models: ProviderModel[];
}

// provider id -> info. Mirrors the backend PROVIDER_SPECS keys.
export const PROVIDERS: ProviderInfo[] = [
  {
    id: "openai",
    name: "OpenAI",
    abbr: "AI",
    hue: ["#10A37F", "#0E8C6D"],
    docs: "platform.openai.com",
    prefix: "sk-",
    models: [
      { id: "gpt-5.4", label: "GPT-5.4", ctx: "256K", note: "Default" },
      { id: "gpt-4o", label: "GPT-4o", ctx: "128K", note: "Flagship multimodal" },
      { id: "gpt-4o-mini", label: "GPT-4o mini", ctx: "128K", note: "Fast & low-cost" },
      { id: "gpt-4.1", label: "GPT-4.1", ctx: "1M", note: "Long context" },
      { id: "o3", label: "o3", ctx: "200K", note: "Deep reasoning" },
    ],
  },
  {
    id: "anthropic",
    name: "Anthropic",
    abbr: "An",
    hue: ["#D97757", "#C4623F"],
    docs: "console.anthropic.com",
    prefix: "sk-ant-",
    models: [
      { id: "claude-opus-4", label: "Claude Opus 4", ctx: "200K", note: "Most capable" },
      { id: "claude-sonnet-4", label: "Claude Sonnet 4", ctx: "200K", note: "Balanced" },
      { id: "claude-3-5-haiku", label: "Claude 3.5 Haiku", ctx: "200K", note: "Fastest" },
    ],
  },
  {
    id: "google",
    name: "Google",
    abbr: "Gg",
    hue: ["#4285F4", "#2C6BD4"],
    docs: "aistudio.google.com",
    prefix: "AIza",
    models: [
      { id: "gemini-2.5-pro", label: "Gemini 2.5 Pro", ctx: "1M", note: "Top reasoning" },
      { id: "gemini-2.5-flash", label: "Gemini 2.5 Flash", ctx: "1M", note: "Fast & cheap" },
    ],
  },
  {
    id: "mistral",
    name: "Mistral",
    abbr: "Ms",
    hue: ["#EE7203", "#EA5A0B"],
    docs: "console.mistral.ai",
    prefix: "",
    models: [
      { id: "mistral-large-latest", label: "Mistral Large", ctx: "128K", note: "Frontier-class" },
      { id: "mistral-small-latest", label: "Mistral Small", ctx: "128K", note: "Efficient" },
    ],
  },
  {
    id: "custom",
    name: "Custom",
    abbr: "{}",
    hue: ["#56217A", "#3E1759"],
    docs: "OpenAI-compatible endpoint",
    prefix: "",
    custom: true,
    models: [{ id: "custom-model", label: "Custom model id", ctx: "—", note: "Set on your endpoint" }],
  },
];

export interface SubagentInfo {
  id: string;
  task: string;
}

// The real coordinator subagents (names must match ask_the_data.py).
export const SUBAGENTS: SubagentInfo[] = [
  { id: "academic_researcher", task: "Finds & synthesizes scientific evidence with citations (Asta)." },
  { id: "dataverse_explorer", task: "Searches & recommends CIP Dataverse datasets." },
  { id: "data_cleaning", task: "Validates and cleans data into versioned outputs." },
  { id: "exploratory_data_analysis", task: "Profiles, summarizes and visualizes — 'what happened?'." },
  { id: "diagnostic_analytics", task: "Interpretable comparisons & inference — 'why it happened?'." },
  { id: "predictive_analytics", task: "Prediction & forecasting — 'what will happen?'." },
  { id: "report_writer", task: "Assembles the polished markdown report." },
];

export interface ModelConfig {
  storage_mode: StorageMode;
  // "provider::model_id"
  default: string;
  // subagent name -> "provider::model_id" (overrides only; absent = default)
  subagents: Record<string, string>;
}

export interface KeyRecord {
  api_key: string;
  base_url?: string | null;
}

const CONFIG_KEY = "atd:llmConfig";
const KEYS_KEY = "atd:llmKeys";

export const DEFAULT_MODEL_CONFIG: ModelConfig = {
  storage_mode: "client",
  default: "openai::gpt-5.4",
  subagents: {},
};

export function loadModelConfig(): ModelConfig {
  try {
    const raw = localStorage.getItem(CONFIG_KEY);
    if (!raw) return { ...DEFAULT_MODEL_CONFIG };
    const parsed = JSON.parse(raw) as Partial<ModelConfig>;
    return {
      storage_mode: parsed.storage_mode === "vault" ? "vault" : "client",
      default: parsed.default || DEFAULT_MODEL_CONFIG.default,
      subagents: parsed.subagents && typeof parsed.subagents === "object" ? parsed.subagents : {},
    };
  } catch {
    return { ...DEFAULT_MODEL_CONFIG };
  }
}

export function saveModelConfig(config: ModelConfig): void {
  localStorage.setItem(CONFIG_KEY, JSON.stringify(config));
}

// Client-mode keys (never persisted server-side).
export function loadClientKeys(): Record<string, KeyRecord> {
  try {
    const raw = localStorage.getItem(KEYS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, KeyRecord>;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

export function saveClientKeys(keys: Record<string, KeyRecord>): void {
  localStorage.setItem(KEYS_KEY, JSON.stringify(keys));
}

// ---------------------------------------------------------------------------
// Submit-time wiring: what gets attached to `configurable` on every run.
// ---------------------------------------------------------------------------

// localStorage key holding the active Project id (explicit Projects, P5). The
// app writes it whenever the active project changes; it rides on every run's
// `configurable` so ``ProjectSpineMiddleware`` scopes the spine to that project.
export const ACTIVE_PROJECT_KEY = "atd:activeProjectId";

export function getActiveProjectId(): string | null {
  return localStorage.getItem(ACTIVE_PROJECT_KEY);
}

export function setActiveProjectId(projectId: string | null): void {
  if (projectId) localStorage.setItem(ACTIVE_PROJECT_KEY, projectId);
  else localStorage.removeItem(ACTIVE_PROJECT_KEY);
}

export function buildSubmitConfigurable(): Record<string, unknown> {
  const config = loadModelConfig();
  const out: Record<string, unknown> = { model_config: config };
  if (config.storage_mode === "client") {
    // `__`-prefixed → excluded from LangGraph checkpoint/trace metadata.
    out.__llm_keys = loadClientKeys();
    // Client-mode Asta token rides on the run the same way; the sandbox uses it
    // to authenticate the `asta` CLI. Vault mode reads it server-side instead.
    const astaToken = getAstaTokenLocal();
    if (astaToken) out.__asta_token = astaToken;
  }
  const projectId = getActiveProjectId();
  if (projectId) out.project_id = projectId;
  return out;
}

// ---------------------------------------------------------------------------
// Authenticated backend calls (vault mode + key testing).
// ---------------------------------------------------------------------------

type TokenGetter = () => Promise<string | undefined | null>;
let _getToken: TokenGetter = async () => undefined;

/** Register the WorkOS access-token getter (called once from App). */
export function setConfigAuthTokenGetter(fn: TokenGetter): void {
  _getToken = fn;
}

async function authFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const token = await _getToken().catch(() => undefined);
  const headers = new Headers(init.headers ?? {});
  if (token) headers.set("Authorization", `Bearer ${token}`);
  return fetch(`${LANGGRAPH_API_URL}${path}`, { ...init, headers });
}

export interface RemoteConfig {
  model_config: ModelConfig | null;
  providers_connected: string[];
}

export async function fetchRemoteConfig(): Promise<RemoteConfig> {
  const res = await authFetch("/config", { method: "GET" });
  if (!res.ok) throw new Error(`Failed to load config (${res.status})`);
  return (await res.json()) as RemoteConfig;
}

export async function saveRemoteConfig(config: ModelConfig): Promise<void> {
  const res = await authFetch("/config", {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ model_config: config }),
  });
  if (!res.ok) throw new Error(`Failed to save config (${res.status})`);
}

export async function saveRemoteKey(
  provider: string,
  apiKey: string,
  baseUrl?: string | null,
): Promise<void> {
  const res = await authFetch("/config/keys", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ provider, api_key: apiKey, base_url: baseUrl ?? null }),
  });
  if (!res.ok) {
    const msg = await res.json().catch(() => ({}));
    throw new Error((msg as { error?: string }).error ?? `Save failed (${res.status})`);
  }
}

export async function deleteRemoteKey(provider: string): Promise<void> {
  const res = await authFetch(`/config/keys/${encodeURIComponent(provider)}`, {
    method: "DELETE",
  });
  if (!res.ok && res.status !== 404) throw new Error(`Delete failed (${res.status})`);
}

export interface TestResult {
  ok: boolean;
  error?: string;
}

export async function testKey(params: {
  provider: string;
  model_id: string;
  api_key?: string;
  base_url?: string | null;
}): Promise<TestResult> {
  const res = await authFetch("/config/test", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(params),
  });
  if (!res.ok) {
    const msg = await res.json().catch(() => ({}));
    return { ok: false, error: (msg as { error?: string }).error ?? `Test failed (${res.status})` };
  }
  return (await res.json()) as TestResult;
}

// Helpers shared with the panel UI.
export function providerById(id: string): ProviderInfo | undefined {
  return PROVIDERS.find((p) => p.id === id);
}

export function parseSpec(spec: string): { provider: ProviderInfo; model: ProviderModel } | null {
  const [pid, mid] = spec.split("::");
  const provider = providerById(pid);
  if (!provider) return null;
  const model = provider.models.find((m) => m.id === mid);
  if (!model) return null;
  return { provider, model };
}

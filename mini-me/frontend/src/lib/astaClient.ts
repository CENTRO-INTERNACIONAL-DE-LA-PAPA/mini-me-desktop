import { getAuthToken } from "./fileClient";
import { LANGGRAPH_API_URL } from "./streamConfig";

// Per-user Asta access-token management (self-service token refresh). The token
// authenticates the `asta` CLI in the sandbox (theorizer, DataVoyager, PDF
// extraction) and expires ~weekly. In "vault" mode it is stored server-side via
// /config/asta; in "client" mode it stays in the browser and rides on every run
// as `__asta_token` (see buildSubmitConfigurable). We never round-trip the token
// through the panel after saving — only its status.

export interface AstaStatus {
  connected: boolean;
  expires_at: number | null; // unix seconds
  expired: boolean;
  seconds_left: number | null;
}

const CLIENT_TOKEN_KEY = "atd:astaToken";

// ---- client-mode local storage ----
export function getAstaTokenLocal(): string | null {
  return localStorage.getItem(CLIENT_TOKEN_KEY);
}

export function setAstaTokenLocal(token: string | null): void {
  if (token) localStorage.setItem(CLIENT_TOKEN_KEY, token);
  else localStorage.removeItem(CLIENT_TOKEN_KEY);
}

// ---- JWT expiry decode (client mode status; mirrors backend/asta_auth) ----
function decodeExp(token: string): number | null {
  const parts = token.split(".");
  if (parts.length !== 3) return null;
  try {
    const pad = "=".repeat((4 - (parts[1].length % 4)) % 4);
    const json = JSON.parse(atob(parts[1].replace(/-/g, "+").replace(/_/g, "/") + pad));
    return typeof json.exp === "number" ? json.exp : null;
  } catch {
    return null;
  }
}

export function localTokenStatus(token: string | null): AstaStatus {
  const t = (token ?? "").trim();
  if (!t) return { connected: false, expires_at: null, expired: false, seconds_left: null };
  const exp = decodeExp(t);
  if (exp === null) return { connected: true, expires_at: null, expired: false, seconds_left: null };
  const secondsLeft = exp - Math.floor(Date.now() / 1000);
  return { connected: true, expires_at: exp, expired: secondsLeft <= 0, seconds_left: secondsLeft };
}

/** Human-readable "expires in …" from a status. */
export function formatExpiry(status: AstaStatus): string {
  if (!status.connected) return "Not connected";
  if (status.expires_at === null) return "Connected";
  if (status.expired) return "Expired — refresh it";
  const s = status.seconds_left ?? 0;
  const days = Math.floor(s / 86400);
  const hours = Math.floor((s % 86400) / 3600);
  if (days > 0) return `Expires in ${days}d ${hours}h`;
  if (hours > 0) return `Expires in ${hours}h`;
  return `Expires in ${Math.max(1, Math.floor(s / 60))}m`;
}

async function authHeaders(extra?: Record<string, string>): Promise<Record<string, string>> {
  const token = await getAuthToken();
  const headers: Record<string, string> = { ...extra };
  if (token) headers.Authorization = `Bearer ${token}`;
  return headers;
}

// ---- vault-mode HTTP ----
export async function fetchAstaStatus(): Promise<AstaStatus> {
  const res = await fetch(`${LANGGRAPH_API_URL}/config/asta`, { headers: await authHeaders() });
  if (!res.ok) return { connected: false, expires_at: null, expired: false, seconds_left: null };
  return (await res.json()) as AstaStatus;
}

export async function saveAstaTokenRemote(token: string): Promise<AstaStatus> {
  const res = await fetch(`${LANGGRAPH_API_URL}/config/asta`, {
    method: "POST",
    headers: await authHeaders({ "Content-Type": "application/json" }),
    body: JSON.stringify({ token }),
  });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error((body as { error?: string }).error ?? `Save failed (${res.status})`);
  return body as AstaStatus;
}

export async function deleteAstaTokenRemote(): Promise<void> {
  await fetch(`${LANGGRAPH_API_URL}/config/asta`, {
    method: "DELETE",
    headers: await authHeaders(),
  });
}

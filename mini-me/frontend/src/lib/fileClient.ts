import { useEffect, useState } from "react";
import { LANGGRAPH_API_URL } from "./streamConfig";

// The /files/ route is auth-protected (enable_custom_route_auth), so plain
// <a href>/<img src> can't reach it — browser-native loads don't carry the
// Authorization header. Everything here fetches with a Bearer token and
// hands back blob URLs instead.

type TokenGetter = () => Promise<string | undefined | null>;

let _getToken: TokenGetter = async () => undefined;

/** Register the WorkOS access-token getter (called once from App). */
export function setFileAuthTokenGetter(fn: TokenGetter): void {
  _getToken = fn;
}

/** Shared WorkOS access-token getter for other authed backend-route fetches. */
export async function getAuthToken(): Promise<string | undefined | null> {
  return _getToken().catch(() => undefined);
}

function fileUrl(threadId: string, relativePath: string, download = false): string {
  const base = `${LANGGRAPH_API_URL}/files/${encodeURIComponent(threadId)}?path=${encodeURIComponent(relativePath)}`;
  return download ? `${base}&download=1` : base;
}

async function fetchFile(
  threadId: string,
  relativePath: string,
  download: boolean,
): Promise<Response> {
  const token = await _getToken().catch(() => undefined);
  const headers: Record<string, string> = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  return fetch(fileUrl(threadId, relativePath, download), { headers });
}

/** Fetch a file as an object URL (caller is responsible for revoking it). */
export async function fetchFileBlobUrl(
  threadId: string,
  relativePath: string,
): Promise<string> {
  const res = await fetchFile(threadId, relativePath, false);
  if (!res.ok) throw new Error(`Failed to load file (${res.status})`);
  return URL.createObjectURL(await res.blob());
}

/** Fetch a file with the auth header and trigger a browser download. */
export async function downloadFile(
  threadId: string,
  relativePath: string,
  filename: string,
): Promise<void> {
  const res = await fetchFile(threadId, relativePath, true);
  if (!res.ok) throw new Error(`Download failed (${res.status})`);
  const url = URL.createObjectURL(await res.blob());
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

/**
 * Load an auth-protected image as an object URL for use in <img src>.
 * Returns { src, loading, error }; revokes the blob URL on unmount.
 */
export function useAuthedImage(
  threadId: string | null,
  relativePath: string | null,
): { src: string | null; loading: boolean; error: boolean } {
  const [src, setSrc] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (!threadId || !relativePath) {
      setSrc(null);
      setLoading(false);
      setError(false);
      return;
    }
    let cancelled = false;
    let objectUrl: string | null = null;
    setLoading(true);
    setError(false);
    fetchFileBlobUrl(threadId, relativePath)
      .then((url) => {
        if (cancelled) {
          URL.revokeObjectURL(url);
          return;
        }
        objectUrl = url;
        setSrc(url);
        setLoading(false);
      })
      .catch(() => {
        if (cancelled) return;
        setError(true);
        setLoading(false);
      });
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [threadId, relativePath]);

  return { src, loading, error };
}

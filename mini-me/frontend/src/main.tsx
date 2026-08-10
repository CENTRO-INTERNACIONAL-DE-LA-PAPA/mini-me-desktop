import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AuthKitProvider } from "@workos-inc/authkit-react";
import { App } from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { branding } from "./branding";
// Self-hosted fonts (replaces the render-blocking Google Fonts stylesheet).
// Latin subset only — it covers Western European diacritics (U+0000–00FF),
// and skipping the other subsets keeps ~30 KB of @font-face rules out of the
// stylesheet. Add e.g. "@fontsource/inter/latin-ext-400.css" if content in
// other scripts ever shows up.
import "@fontsource/outfit/latin-400.css";
import "@fontsource/outfit/latin-500.css";
import "@fontsource/outfit/latin-600.css";
import "@fontsource/outfit/latin-700.css";
import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-500.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/jetbrains-mono/latin-400.css";
import "@fontsource/jetbrains-mono/latin-500.css";
import "./styles.css";
import { LANGGRAPH_API_URL } from "./lib/streamConfig";

// Brand the browser tab from the single branding source (overrides the static
// fallback <title> in index.html once the app mounts).
document.title = branding.appName;

// Warm the connection to the LangSmith-hosted backend before the first API
// call. The API origin comes from VITE_LANGGRAPH_API_URL at build time, so
// this can't live as a static tag in index.html.
try {
  const apiOrigin = new URL(LANGGRAPH_API_URL, window.location.href).origin;
  if (apiOrigin !== window.location.origin) {
    const link = document.createElement("link");
    link.rel = "preconnect";
    link.href = apiOrigin;
    // Authorization-header fetches run in CORS mode without credentials, so
    // the preconnect must be flagged anonymous to hit the same socket pool.
    link.crossOrigin = "anonymous";
    document.head.append(link);
  }
} catch {
  // Malformed URL — nothing to warm.
}

const WORKOS_CLIENT_ID = import.meta.env.VITE_WORKOS_CLIENT_ID as string | undefined;
const WORKOS_REDIRECT_URI = (import.meta.env.VITE_WORKOS_REDIRECT_URI ??
  window.location.origin) as string;

if (!WORKOS_CLIENT_ID) {
  console.warn(
    "[auth] VITE_WORKOS_CLIENT_ID is not set — sign-in is disabled.",
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <AuthKitProvider
        clientId={WORKOS_CLIENT_ID ?? ""}
        redirectUri={WORKOS_REDIRECT_URI}
        // Persist the refresh token in localStorage so reloads keep the user
        // signed in. Without this, AuthKit falls back to memory storage in
        // production and relies on third-party cookies for silent refresh,
        // which modern browsers block — and you get bounced to login on every
        // reload. Acceptable for an internal CGIAR SSO tool; revisit if the
        // app is ever exposed to untrusted XSS surfaces.
        devMode={true}
      >
        <App />
      </AuthKitProvider>
    </ErrorBoundary>
  </StrictMode>,
);

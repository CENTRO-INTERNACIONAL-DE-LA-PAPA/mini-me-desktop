import { LogIn, LogOut, ShieldAlert } from "lucide-react";
import { branding } from "../branding";

interface AuthGateProps {
  mode: "loading" | "signin" | "forbidden";
  allowedDomainsLabel: string;
  onSignIn?: () => void;
  onSignOut?: () => void;
  email?: string | null;
}

export function AuthGate({
  mode,
  allowedDomainsLabel,
  onSignIn,
  onSignOut,
  email,
}: AuthGateProps) {
  return (
    <main className="auth-gate" role="main">
      <div className="auth-gate-card">
        <p className="eyebrow">{branding.appName}</p>
        <h1>{branding.tagline}</h1>

        {mode === "loading" ? (
          <p className="auth-gate-status">Restoring your session…</p>
        ) : mode === "forbidden" ? (
          <>
            <div className="auth-gate-icon" aria-hidden="true">
              <ShieldAlert size={28} />
            </div>
            <p className="auth-gate-status">
              This workbench is restricted to <strong>{allowedDomainsLabel}</strong>{" "}
              accounts.
              {email ? (
                <>
                  <br />
                  You're signed in as <strong>{email}</strong>, which doesn't
                  have access.
                </>
              ) : null}
            </p>
            <button
              type="button"
              className="auth-gate-button"
              onClick={onSignOut}
              autoFocus
            >
              <LogOut size={16} aria-hidden="true" />
              Sign out and try another account
            </button>
          </>
        ) : (
          <>
            <p className="auth-gate-status">
              Sign in with your <strong>{allowedDomainsLabel}</strong> account to
              continue.
            </p>
            <button
              type="button"
              className="auth-gate-button"
              onClick={onSignIn}
              autoFocus
            >
              <LogIn size={16} aria-hidden="true" />
              Sign in
            </button>
          </>
        )}
      </div>
    </main>
  );
}

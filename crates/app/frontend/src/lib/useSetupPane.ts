import { useCallback, useEffect, useRef, useState } from "react";
import { ipc } from "./ipc";
import type { PreflightReport } from "./protocol";
import { useAppStore } from "./store";

export interface RunningFix {
  label: string;
  link: string | null;
  lines: string[];
  notes: string[];
  checkId: string;
  done: boolean;
  ok: boolean;
  stopping: boolean;
}

const FIX_LOG_LINES = 200;

function firstUrl(line: string): string | null {
  const httpsAt = line.indexOf("https://");
  const httpAt = line.indexOf("http://");
  const candidates = [httpsAt, httpAt].filter((at) => at >= 0);
  if (candidates.length === 0) return null;
  const at = Math.min(...candidates);
  let url = "";
  for (const ch of line.slice(at)) {
    if (/\s/.test(ch)) break;
    url += ch;
  }
  url = url.replace(/[.,:;)\]"']+$/, "");
  const scheme = url.startsWith("https://") ? "https://" : "http://";
  return url.length > scheme.length ? url : null;
}

export function deviceCode(url: string): string | null {
  const marker = "user_code=";
  const at = url.indexOf(marker);
  if (at < 0) return null;
  let code = "";
  for (const ch of url.slice(at + marker.length)) {
    if (/[A-Za-z0-9-]/.test(ch)) code += ch;
    else break;
  }
  return code || null;
}

export function displayArgv(argv: string[]): string {
  return argv
    .map((arg) => (arg.includes(" ") || arg.includes('"') ? `"${arg.replace(/"/g, '\\"')}"` : arg))
    .join(" ");
}

const STILL_FAILING_NOTE =
  "— It reported success but the check still fails. The output above is the best clue; the sidecar log below has the rest.";

export function useSetupPane() {
  const setBackendStart = useAppStore((state) => state.setBackendStart);
  const [report, setReport] = useState<PreflightReport | null>(null);
  const [checking, setChecking] = useState(false);
  const [fix, setFix] = useState<RunningFix | null>(null);
  const judgeAfterRecheck = useRef(false);
  const fixRef = useRef<RunningFix | null>(null);
  fixRef.current = fix;

  const runPreflight = useCallback(async () => {
    setChecking(true);
    try {
      const next = await ipc.runPreflight();
      setReport(next);
      if (judgeAfterRecheck.current) {
        judgeAfterRecheck.current = false;
        const running = fixRef.current;
        const stillFailing = next.checks.find((c) => c.id === running?.checkId)?.state === "Fail";
        if (running?.ok && stillFailing) {
          setFix((current) => (current ? { ...current, notes: [...current.notes, STILL_FAILING_NOTE] } : current));
        }
      }
      return next;
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    ipc.onFixEvent((event) => {
      if ("Line" in event) {
        const line = event.Line;
        setFix((current) => {
          if (!current) return current;
          const link = current.link ?? firstUrl(line);
          const lines = [...current.lines, line];
          if (lines.length > FIX_LOG_LINES) lines.shift();
          return { ...current, link, lines };
        });
      } else {
        const { ok, note } = event.Finished;
        setFix((current) => {
          if (!current) return current;
          const notes = [...current.notes, `— ${note}`];
          if (ok && current.label.includes("Sign in")) {
            notes.push("— Close and reopen the app: the backend reads your Asta sign-in when it starts.");
          }
          return { ...current, done: true, ok, notes };
        });
        if (ok) {
          judgeAfterRecheck.current = true;
          runPreflight();
        }
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [runPreflight]);

  const startFix = useCallback((label: string, argv: string[], checkId: string) => {
    setFix((current) =>
      current && !current.done
        ? current
        : { label, link: null, lines: [], notes: [], checkId, done: false, ok: false, stopping: false },
    );
    ipc.startFix(argv);
  }, []);

  const stopFix = useCallback(() => {
    setFix((current) => {
      if (!current || current.done || current.stopping) return current;
      ipc.cancelFix().then((stopped) => {
        setFix((c) =>
          c
            ? {
                ...c,
                notes: [
                  ...c.notes,
                  stopped ? "— asked to stop; waiting for the command to exit" : "— it had already finished",
                ],
              }
            : c,
        );
      });
      return { ...current, stopping: true };
    });
  }, []);

  const adoptCheckout = useCallback(
    async (dir: string) => {
      await ipc.adoptCheckout(dir);
      await runPreflight();
    },
    [runPreflight],
  );

  const restartBackend = useCallback(async () => {
    const started = await ipc.restartBackend();
    setBackendStart(started);
    await runPreflight();
    return started;
  }, [runPreflight, setBackendStart]);

  return { report, checking, fix, runPreflight, startFix, stopFix, adoptCheckout, restartBackend };
}

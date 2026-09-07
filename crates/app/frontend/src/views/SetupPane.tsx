import { useEffect, useState } from "react";
import { Actions, Button, Label } from "../components";
import { ipc } from "../lib/ipc";
import type { LogPaths as LogPathsType, PreflightCheck, PreflightFix, PreflightState } from "../lib/protocol";
import { deviceCode, displayArgv, type useSetupPane } from "../lib/useSetupPane";
import { useAppStore } from "../lib/store";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

type Setup = ReturnType<typeof useSetupPane>;
type Theme = ReturnType<typeof useTheme>["theme"];

function stateColour(state: PreflightState, theme: Theme): number {
  switch (state) {
    case "Pass":
      return theme.textMuted;
    case "Warn":
      return theme.accent;
    case "Fail":
      return theme.error;
    case "Skip":
      return theme.border;
  }
}

function glyph(state: PreflightState): string {
  switch (state) {
    case "Pass":
      return "✓";
    case "Warn":
      return "!";
    case "Fail":
      return "✗";
    case "Skip":
      return "–";
  }
}

function summarize(report: { checks: PreflightCheck[] }): string {
  const count = (state: PreflightState) => report.checks.filter((c) => c.state === state).length;
  const parts = [`${count("Pass")} ok`];
  for (const [state, word] of [
    ["Fail", "to fix"],
    ["Warn", "optional"],
    ["Skip", "skipped"],
  ] as [PreflightState, string][]) {
    const n = count(state);
    if (n > 0) parts.push(`${n} ${word}`);
  }
  return parts.join(" · ");
}

function useLogPaths() {
  const [paths, setPaths] = useState<LogPathsType | null>(null);
  useEffect(() => {
    ipc.getLogPaths().then(setPaths);
  }, []);
  return paths;
}

function FixRow({
  fix,
  onRun,
  onAdopt,
}: {
  fix: PreflightFix;
  onRun: (label: string, argv: string[]) => void;
  onAdopt: (dir: string) => void;
}) {
  if ("Run" in fix) {
    const { label, argv, note } = fix.Run;
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <Label muted size="compact">
          {note}
        </Label>
        <div style={{ display: "flex", flexDirection: "row", gap: 8 }}>
          <Button style="primary" onClick={() => onRun(label, argv)}>
            {label}
          </Button>
          <Button onClick={() => navigator.clipboard?.writeText(displayArgv(argv))}>Copy ⧉</Button>
        </div>
      </div>
    );
  }
  if ("Adopt" in fix) {
    const { label, dir } = fix.Adopt;
    return (
      <Button style="primary" onClick={() => onAdopt(dir)}>
        {label}
      </Button>
    );
  }
  return (
    <Label muted size="compact">
      {fix.Manual}
    </Label>
  );
}

export function SetupBody({ setup }: { setup: Setup }) {
  const { theme } = useTheme();
  const backendStart = useAppStore((state) => state.backendStart);
  const { report, checking, fix, startFix, stopFix, adoptCheckout } = setup;
  const paths = useLogPaths();
  const blocked = report?.checks.some((c) => c.state === "Fail") ?? false;
  const fixTone = fix ? (!fix.done ? theme.textMuted : !fix.ok ? theme.error : theme.textMuted) : theme.textMuted;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12, width: "100%", minWidth: 0 }}>
      {backendStart === "Attached" && (
        <div
          style={{
            padding: 8,
            borderRadius: 6,
            border: `1px solid ${hex(theme.warning)}`,
            color: hex(theme.warning),
            fontSize: 12,
          }}
        >
          This backend was already running when the app started, so it may be running an older
          version of the app's Python overlay. If something new does nothing, restart it below.
        </div>
      )}

      {!report ? (
        <Label muted>Checking this machine…</Label>
      ) : (
        <>
          <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
            <Label colour={checking ? theme.textMuted : blocked ? theme.error : theme.textMuted}>
              {checking ? "Re-checking…" : blocked ? `Not ready yet · ${summarize(report)}` : `Ready to run · ${summarize(report)}`}
            </Label>
            <Label muted size="compact">{`${report.location} · ${report.execution}`}</Label>
            <Label muted size="compact">
              {report.owned
                ? "Installed and maintained by this app."
                : "Your own checkout — the app runs it but never modifies it."}
            </Label>
          </div>

          {report.checks.map((check) => (
            <div
              key={check.id}
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 4,
                paddingLeft: 8,
                borderLeft: `2px solid ${hex(stateColour(check.state, theme))}`,
              }}
            >
              <Label colour={check.state === "Pass" ? theme.text : stateColour(check.state, theme)}>
                {glyph(check.state)} {check.label}
              </Label>
              <Label muted size="compact">
                {check.detail}
              </Label>
              {check.fixes.map((f, i) => (
                <FixRow key={i} fix={f} onRun={(label, argv) => startFix(label, argv, check.id)} onAdopt={adoptCheckout} />
              ))}
            </div>
          ))}
        </>
      )}

      {fix && (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 8,
            padding: 8,
            borderRadius: 8,
            border: `1px solid ${hex(!fix.done ? theme.accent : fix.ok ? theme.border : theme.error)}`,
          }}
        >
          <div style={{ display: "flex", flexDirection: "row", justifyContent: "space-between", alignItems: "center", gap: 8 }}>
            <Label>
              {fix.done ? `${fix.label} — ${fix.ok ? "done" : "failed"}` : fix.stopping ? `${fix.label} — stopping…` : `${fix.label}…`}
            </Label>
            {!fix.done && (
              <Button style="danger" disabled={fix.stopping} onClick={stopFix}>
                Stop
              </Button>
            )}
          </div>

          {fix.link && (
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              {deviceCode(fix.link) && <div style={{ fontSize: 16, color: hex(theme.accent) }}>{deviceCode(fix.link)}</div>}
              <div style={{ display: "flex", flexDirection: "row", gap: 8 }}>
                <Button style="primary" onClick={() => ipc.openUrl(fix.link!)}>
                  Open the sign-in page
                </Button>
                <Button onClick={() => navigator.clipboard?.writeText(fix.link!)}>Copy ⧉</Button>
              </div>
            </div>
          )}

          <div className="thin-scroll" style={{ display: "flex", flexDirection: "column", gap: 2, maxHeight: 200, overflowY: "auto" }}>
            {fix.lines.length === 0 ? (
              <Label muted size="compact">
                {fix.done ? "The command printed nothing. The sidecar log below may have more." : "starting…"}
              </Label>
            ) : (
              fix.lines.map((line, i) => (
                <Label key={i} muted size="compact">
                  {line}
                </Label>
              ))
            )}
          </div>

          {fix.notes.map((note, i) => (
            <Label key={i} size="compact" colour={fixTone}>
              {note}
            </Label>
          ))}
        </div>
      )}

      {paths && (
        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <Label muted size="compact">{`Sidecar log: ${paths.sidecar}`}</Label>
          <Label muted size="compact">{`App log: ${paths.app}`}</Label>
          <Label muted size="compact">{`Update log: ${paths.update}`}</Label>
        </div>
      )}
    </div>
  );
}

export function SetupActions({ setup, onClose }: { setup: Setup; onClose: () => void }) {
  const { checking, runPreflight, restartBackend } = setup;
  return (
    <Actions>
      <Button style="primary" disabled={checking} onClick={runPreflight}>
        {checking ? "Checking…" : "Re-check"}
      </Button>
      <Button onClick={restartBackend}>Restart backend</Button>
      <Button onClick={onClose}>Close</Button>
    </Actions>
  );
}

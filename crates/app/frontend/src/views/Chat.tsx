import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { ApprovalCard } from "./ApprovalCard";
import { Composer } from "./Composer";
import { useAppStore, type TraceStep } from "../lib/store";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

function foldSteps(steps: TraceStep[]): { label: string; count: number }[] {
  const folded: { label: string; count: number }[] = [];
  for (const step of steps) {
    const label = step.agent ? `${step.agent}: ${step.label}` : step.label;
    const last = folded[folded.length - 1];
    if (last && last.label === label) {
      last.count += 1;
    } else {
      folded.push({ label, count: 1 });
    }
  }
  return folded;
}

export function Chat() {
  const { theme } = useTheme();
  const { transcript, pendingApproval } = useAppStore((state) => ({
    transcript: state.transcript,
    pendingApproval: state.pendingApproval,
  }));

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        flexGrow: 1,
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        padding: "16px 24px",
        gap: 12,
      }}
    >
      <div className="thin-scroll" style={{ flexGrow: 1, minHeight: 0, overflowY: "auto", display: "flex", flexDirection: "column", gap: 20 }}>
        {transcript.length === 0 && (
          <div style={{ color: hex(theme.textFaint), fontSize: 13 }}>
            Ask something to start a conversation.
          </div>
        )}
        {transcript.map((message, index) => (
          <div key={index} style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <div style={{ fontSize: 11, color: hex(theme.textFaint) }}>
              {message.role === "user" ? "You" : "Mini-Me"}
            </div>
            {message.steps && message.steps.length > 0 && (
              <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                {foldSteps(message.steps).map((step, i) => (
                  <div key={i} style={{ fontSize: 12, color: hex(theme.textFaint) }}>
                    {step.label}
                    {step.count > 1 ? ` ×${step.count}` : ""}
                  </div>
                ))}
              </div>
            )}
            {message.subagentText &&
              Object.entries(message.subagentText).map(([agent, text]) => (
                <div
                  key={agent}
                  style={{
                    borderLeft: `2px solid ${hex(theme.border)}`,
                    paddingLeft: 8,
                    color: hex(theme.textMuted),
                    fontSize: 13,
                  }}
                >
                  <div style={{ fontSize: 11, color: hex(theme.textFaint) }}>{agent}</div>
                  <Markdown remarkPlugins={[remarkGfm]}>{text}</Markdown>
                </div>
              ))}
            <div style={{ color: hex(theme.text), fontSize: 14, lineHeight: 1.5 }} className="markdown">
              <Markdown remarkPlugins={[remarkGfm]}>{message.text}</Markdown>
            </div>
          </div>
        ))}
      </div>
      {pendingApproval && <ApprovalCard request={pendingApproval} />}
      <Composer />
    </div>
  );
}

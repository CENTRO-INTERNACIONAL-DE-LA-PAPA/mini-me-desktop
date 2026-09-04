import { Button } from "../components";
import { useAppStore } from "../lib/store";
import type { Answer, ApprovalRequest, Decision } from "../lib/protocol";
import { hex, CODE_FONT_STACK } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

function answersFor(request: ApprovalRequest, decision: Decision): Answer[] {
  return request.actions.map((action) => ({ interrupt: action.interrupt, decision }));
}

export function ApprovalCard({ request }: { request: ApprovalRequest }) {
  const { theme } = useTheme();
  const { answerApproval, setApproveConversation } = useAppStore((state) => ({
    answerApproval: state.answerApproval,
    setApproveConversation: state.setApproveConversation,
  }));

  const approve = () => answerApproval(answersFor(request, "Approve"));
  const reject = () => answerApproval(answersFor(request, { Reject: { message: "rejected by the user" } }));
  const approveConversation = () => {
    setApproveConversation(true);
    approve();
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 8,
        margin: 8,
        padding: 12,
        borderRadius: 10,
        border: `1px solid ${hex(theme.accent)}`,
        background: hex(theme.surface),
      }}
    >
      <div style={{ display: "flex", flexDirection: "row", justifyContent: "space-between" }}>
        <div style={{ fontSize: 11, color: hex(theme.accent) }}>RUN THIS ON YOUR MACHINE?</div>
        <div style={{ fontSize: 11, color: hex(theme.textFaint) }}>
          {request.actions.length <= 1
            ? (request.actions[0]?.tool ?? "")
            : `${request.actions.length} commands`}
        </div>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 8, maxHeight: 260, overflowY: "auto" }} className="thin-scroll">
        {request.actions.map((action, i) => (
          <div key={i} style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            {action.description && (
              <div style={{ fontSize: 12, color: hex(theme.textMuted) }}>{action.description}</div>
            )}
            <div
              style={{
                padding: 8,
                borderRadius: 6,
                background: hex(theme.background),
                border: `1px solid ${hex(theme.border)}`,
                color: hex(theme.text),
                fontFamily: CODE_FONT_STACK,
                fontSize: 12.5,
                lineHeight: "19px",
                whiteSpace: "pre-wrap",
              }}
            >
              {action.detail}
            </div>
          </div>
        ))}
      </div>

      <div style={{ display: "flex", flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
        <Button style="primary" onClick={approve}>
          Approve
        </Button>
        <Button onClick={reject}>Reject</Button>
        <div style={{ flexGrow: 1 }} />
        <Button onClick={approve}>Approve the rest of this turn</Button>
        <Button onClick={approveConversation}>Approve everything in this conversation</Button>
      </div>
    </div>
  );
}

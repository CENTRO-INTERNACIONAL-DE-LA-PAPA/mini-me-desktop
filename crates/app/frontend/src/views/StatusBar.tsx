import { Button, Label, Spinner } from "../components";
import { useAppStore } from "../lib/store";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

export function StatusBar() {
  const { theme } = useTheme();
  const { status, error, streaming, executionLabel, baseUrl, approveConversation, setApproveConversation } =
    useAppStore((state) => ({
      status: state.status,
      error: state.error,
      streaming: state.streaming,
      executionLabel: state.executionLabel,
      baseUrl: state.baseUrl,
      approveConversation: state.approveConversation,
      setApproveConversation: state.setApproveConversation,
    }));

  const statusText = error ?? status;
  const statusColour = error ? theme.error : theme.textMuted;
  const isLocal = executionLabel === "host (local)";

  return (
    <div
      style={{
        flex: "none",
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        gap: 12,
        width: "100%",
        minWidth: 0,
        padding: "4px 12px",
        borderTop: `1px solid ${hex(theme.border)}`,
        background: hex(theme.surface),
      }}
    >
      {streaming && <Spinner />}
      <Label colour={statusColour} ellipsis>
        {statusText}
      </Label>
      {approveConversation && (
        <Button
          style="primary"
          onClick={() => setApproveConversation(false)}
        >
          approving everything — click to stop
        </Button>
      )}
      <div style={{ flex: "none", fontSize: 12, color: hex(isLocal ? theme.accent : theme.textMuted) }}>
        {executionLabel}
      </div>
      <div style={{ flex: "none", fontSize: 12, color: hex(theme.textMuted) }}>ctrl-p commands</div>
      <div style={{ fontSize: 13, color: hex(theme.textMuted) }}>{baseUrl}</div>
    </div>
  );
}

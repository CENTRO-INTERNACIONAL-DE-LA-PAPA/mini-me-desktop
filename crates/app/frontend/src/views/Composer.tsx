import { open } from "@tauri-apps/plugin-dialog";
import { useRef, useState, type KeyboardEvent } from "react";
import { useShallow } from "zustand/react/shallow";
import { AgentMenu } from "./AgentMenu";
import { Button, Chip } from "../components";
import { useAppStore } from "../lib/store";
import type { AttachmentInput } from "../lib/protocol";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function Composer() {
  const { theme } = useTheme();
  const { streaming, submitTurn, cancelTurn, currentSubagent, setCurrentSubagent } = useAppStore(
    useShallow((state) => ({
      streaming: state.streaming,
      submitTurn: state.submitTurn,
      cancelTurn: state.cancelTurn,
      currentSubagent: state.currentSubagent,
      setCurrentSubagent: state.setCurrentSubagent,
    })),
  );
  const [text, setText] = useState("");
  const [attachments, setAttachments] = useState<AttachmentInput[]>([]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const hasText = text.trim() !== "";
  const send = () => {
    if (!hasText) return;
    submitTurn(text.trim(), attachments);
    setText("");
    setAttachments([]);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  const attachFiles = async () => {
    const selected = await open({ multiple: true });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    setAttachments((current) => [
      ...current,
      ...paths.map((path) => ({ label: fileName(path), path })),
    ]);
  };

  const removeAttachment = (path: string) => {
    setAttachments((current) => current.filter((a) => a.path !== path));
  };

  const sendIcon = streaming ? "icons/stop-circle.svg" : "icons/paper-plane-right.svg";
  const sendStyle = streaming ? "danger" : hasText ? "primary" : "secondaryWhite";

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: "none", width: "100%", minWidth: 0 }}>
      <div style={{ display: "flex", flexDirection: "row" }}>
        <AgentMenu current={currentSubagent} onChoose={setCurrentSubagent} />
      </div>

      {attachments.length > 0 && (
        <div style={{ display: "flex", flexDirection: "row", flexWrap: "wrap", gap: 6, padding: "6px 8px 0 8px" }}>
          {attachments.map((attachment) => (
            <Chip
              key={attachment.path}
              label={attachment.label}
              removable
              onClick={() => removeAttachment(attachment.path)}
            />
          ))}
        </div>
      )}

      <div
        style={{
          display: "flex",
          flexDirection: "row",
          alignItems: "flex-end",
          gap: 8,
          padding: "4px 8px",
          borderRadius: 10,
          background: hex(theme.surface),
          border: `1px solid ${hex(theme.border)}`,
        }}
      >
        <Button
          icon="icons/plus.svg"
          style="secondaryWhite"
          border={false}
          tooltip="Add a file from this computer"
          onClick={attachFiles}
        />
        <textarea
          ref={textareaRef}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Ask something…"
          rows={1}
          style={{
            flexGrow: 1,
            minWidth: 0,
            maxHeight: 160,
            resize: "none",
            border: "none",
            outline: "none",
            background: "transparent",
            color: hex(theme.text),
            fontSize: 13,
            fontFamily: "inherit",
            padding: "6px 0",
          }}
        />
        <Button
          icon={sendIcon}
          style={sendStyle as "danger" | "primary" | "secondaryWhite"}
          border={false}
          disabled={!hasText && !streaming}
          tooltip={streaming ? "Stop this turn" : hasText ? "Send" : "Type a question first"}
          onClick={() => (streaming ? cancelTurn() : send())}
        />
      </div>
    </div>
  );
}

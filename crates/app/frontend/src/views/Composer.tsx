import { useRef, useState, type KeyboardEvent } from "react";
import { Button } from "../components";
import { useAppStore } from "../lib/store";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

export function Composer() {
  const { theme } = useTheme();
  const { streaming, submitTurn, cancelTurn } = useAppStore((state) => ({
    streaming: state.streaming,
    submitTurn: state.submitTurn,
    cancelTurn: state.cancelTurn,
  }));
  const [text, setText] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const hasText = text.trim() !== "";
  const send = () => {
    if (!hasText) return;
    submitTurn(text.trim());
    setText("");
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  const sendIcon = streaming ? "icons/stop-circle.svg" : "icons/paper-plane-right.svg";
  const sendStyle = streaming ? "danger" : hasText ? "primary" : "secondaryWhite";

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "row",
        alignItems: "flex-end",
        gap: 8,
        flex: "none",
        padding: "4px 8px",
        borderRadius: 10,
        background: hex(theme.surface),
        border: `1px solid ${hex(theme.border)}`,
      }}
    >
      <Button icon="icons/plus.svg" style="secondaryWhite" border={false} tooltip="Add a file from this computer" onClick={() => {}} />
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
  );
}

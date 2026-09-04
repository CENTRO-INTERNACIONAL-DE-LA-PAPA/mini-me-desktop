import { useState, type ReactNode } from "react";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

export function Hint({ text, children }: { text?: string; children: ReactNode }) {
  const { theme } = useTheme();
  const [visible, setVisible] = useState(false);

  return (
    <span
      style={{ position: "relative", display: "inline-flex" }}
      onMouseEnter={() => setVisible(true)}
      onMouseLeave={() => setVisible(false)}
    >
      {children}
      {text && visible && (
        <span
          style={{
            position: "absolute",
            bottom: "calc(100% + 4px)",
            left: "50%",
            transform: "translateX(-50%)",
            whiteSpace: "nowrap",
            padding: "4px 8px",
            borderRadius: 6,
            background: hex(theme.overlay),
            border: `1px solid ${hex(theme.borderStrong)}`,
            color: hex(theme.text),
            fontSize: "12px",
            zIndex: 50,
          }}
        >
          {text}
        </span>
      )}
    </span>
  );
}

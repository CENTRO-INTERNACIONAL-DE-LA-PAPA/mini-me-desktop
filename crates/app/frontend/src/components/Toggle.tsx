import type { ReactNode } from "react";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";
import { Label } from "./Label";

export function SettingRow({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  const { theme } = useTheme();
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 16,
        width: "100%",
        minWidth: 0,
        padding: "8px 0",
        borderBottom: `1px solid ${hex(theme.border)}`,
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", flexGrow: 1, minWidth: 0, gap: 4 }}>
        <Label>{title}</Label>
        <Label muted size="compact">
          {description}
        </Label>
      </div>
      <div style={{ flex: "none" }}>{children}</div>
    </div>
  );
}

export function Toggle({ on, onClick }: { on: boolean; onClick?: () => void }) {
  const { theme } = useTheme();
  return (
    <div
      onClick={onClick}
      style={{
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        flex: "none",
        width: 34,
        height: 18,
        padding: 2,
        borderRadius: 999,
        border: `1px solid ${hex(on ? theme.accent : theme.borderStrong)}`,
        background: hex(on ? theme.accentSoft : theme.surface),
        justifyContent: on ? "flex-end" : "flex-start",
        cursor: "pointer",
      }}
    >
      <div
        style={{
          width: 12,
          height: 12,
          borderRadius: "50%",
          background: hex(on ? theme.accent : theme.textFaint),
        }}
      />
    </div>
  );
}

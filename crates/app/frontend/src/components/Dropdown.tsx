import type { MouseEvent, ReactNode } from "react";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";
import { Label } from "./Label";

export function DropdownPopup({ children }: { children: ReactNode }) {
  const { theme } = useTheme();
  return (
    <div
      style={{
        position: "absolute",
        top: "calc(100% + 4px)",
        left: 0,
        zIndex: 40,
        display: "flex",
        flexDirection: "column",
        width: 320,
        minWidth: 0,
        overflow: "hidden",
        gap: 8,
        padding: 8,
        borderRadius: 6,
        background: hex(theme.elevated),
        border: `1px solid ${hex(theme.borderStrong)}`,
      }}
    >
      {children}
    </div>
  );
}

export function Dropdown({
  value,
  open,
  children,
  onClick,
}: {
  value: string;
  open: boolean;
  children?: ReactNode;
  onClick?: (event: MouseEvent<HTMLDivElement>) => void;
}) {
  const { theme } = useTheme();
  return (
    <div style={{ position: "relative" }}>
      <div
        onClick={onClick}
        style={{
          display: "flex",
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 8,
          flex: "none",
          minWidth: 150,
          padding: "4px 8px",
          borderRadius: 6,
          border: `1px solid ${hex(open ? theme.accent : theme.borderStrong)}`,
          color: hex(theme.text),
          fontSize: 13,
          cursor: "pointer",
        }}
      >
        <Label ellipsis>{value}</Label>
        <div style={{ flex: "none", color: hex(theme.textFaint) }}>⌄</div>
      </div>
      {open && children && <DropdownPopup>{children}</DropdownPopup>}
    </div>
  );
}

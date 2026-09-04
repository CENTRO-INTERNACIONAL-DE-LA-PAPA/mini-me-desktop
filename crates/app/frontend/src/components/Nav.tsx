import type { MouseEvent, ReactNode } from "react";
import { hoverOver } from "../theme/apca";
import { useHover } from "../lib/useHover";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

export function NavEntry({
  label,
  selected,
  onClick,
}: {
  label: string;
  selected: boolean;
  onClick?: (event: MouseEvent<HTMLDivElement>) => void;
}) {
  const { theme } = useTheme();
  const { hovered, handlers } = useHover();

  const background = selected
    ? theme.elevated
    : hovered
      ? hoverOver(theme.elevated, theme)
      : undefined;

  return (
    <div
      onClick={onClick}
      {...handlers}
      style={{
        width: "100%",
        minWidth: 0,
        padding: "4px 8px",
        borderRadius: 6,
        fontSize: 13,
        color: hex(selected ? theme.text : theme.textMuted),
        background: background !== undefined ? hex(background) : undefined,
        cursor: "pointer",
      }}
    >
      {label}
    </div>
  );
}

export function NavRail({ children }: { children: ReactNode }) {
  const { theme } = useTheme();
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        flex: "none",
        width: 150,
        gap: 4,
        padding: 8,
        borderRight: `1px solid ${hex(theme.border)}`,
      }}
    >
      {children}
    </div>
  );
}

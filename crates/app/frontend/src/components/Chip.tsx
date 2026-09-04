import type { MouseEvent } from "react";
import { hoverOver, inkOn } from "../theme/apca";
import { useHover } from "../lib/useHover";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";
import { Label } from "./Label";

export function Chip({
  label,
  ink,
  border,
  bg,
  hoverBase,
  removable = false,
  onClick,
}: {
  label: string;
  ink?: number;
  border?: number;
  bg?: number;
  hoverBase?: number;
  removable?: boolean;
  onClick?: (event: MouseEvent<HTMLDivElement>) => void;
}) {
  const { theme } = useTheme();
  const { hovered, handlers } = useHover();

  const resolvedInk = ink ?? theme.textMuted;
  const resolvedBorder = border ?? theme.border;
  const resolvedHoverBase = hoverBase ?? theme.surface;

  let background = bg;
  let colour = resolvedInk;
  if (hovered) {
    const fill = hoverOver(resolvedHoverBase, theme);
    background = fill;
    colour = inkOn(fill, theme);
  }

  return (
    <div
      onClick={onClick}
      {...handlers}
      style={{
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        gap: 4,
        flex: "none",
        padding: "4px 8px",
        borderRadius: 6,
        border: `1px solid ${hex(resolvedBorder)}`,
        color: hex(colour),
        background: background !== undefined ? hex(background) : undefined,
        fontSize: 12,
        cursor: onClick ? "pointer" : "default",
      }}
    >
      <Label inherit size="compact" ellipsis>
        {label}
      </Label>
      {removable && <div style={{ flex: "none", color: hex(theme.textFaint) }}>×</div>}
    </div>
  );
}

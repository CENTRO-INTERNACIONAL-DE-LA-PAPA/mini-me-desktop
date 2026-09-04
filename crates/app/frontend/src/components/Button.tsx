import type { CSSProperties, MouseEvent, ReactNode } from "react";
import { useHover } from "../lib/useHover";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";
import { Hint } from "./Hint";
import { Icon, type IconSize } from "./Icon";
import type { Theme } from "../theme/theme";

export type ButtonStyle = "primary" | "secondary" | "secondaryWhite" | "danger";
export type ButtonAlignment = "left" | "center" | "right";

function textColour(style: ButtonStyle, theme: Theme): number {
  switch (style) {
    case "primary":
      return theme.accent;
    case "danger":
      return theme.error;
    default:
      return theme.textMuted;
  }
}

function borderColour(style: ButtonStyle, theme: Theme): number {
  switch (style) {
    case "primary":
      return theme.accent;
    case "danger":
      return theme.error;
    default:
      return theme.border;
  }
}

function bgColour(style: ButtonStyle, theme: Theme): number {
  switch (style) {
    case "primary":
      return theme.accentSoft;
    case "secondaryWhite":
      return theme.surface;
    default:
      return theme.background;
  }
}

function hoverBgColour(style: ButtonStyle, theme: Theme): number {
  switch (style) {
    case "primary":
    case "danger":
      return bgColour(style, theme);
    case "secondaryWhite":
      return theme.background;
    default:
      return theme.surface;
  }
}

const JUSTIFY: Record<ButtonAlignment, CSSProperties["justifyContent"]> = {
  left: "flex-start",
  center: "center",
  right: "flex-end",
};

export function Button({
  icon,
  iconSize = "small",
  children,
  style = "secondary",
  alignment = "left",
  toggle = false,
  active = false,
  border = true,
  disabled = false,
  tooltip,
  onClick,
}: {
  icon?: string;
  iconSize?: IconSize;
  children?: ReactNode;
  style?: ButtonStyle;
  alignment?: ButtonAlignment;
  toggle?: boolean;
  active?: boolean;
  border?: boolean;
  disabled?: boolean;
  tooltip?: string;
  onClick?: (event: MouseEvent<HTMLDivElement>) => void;
}) {
  const { theme } = useTheme();
  const { hovered, handlers } = useHover();
  const resolvedStyle = toggle && active ? "primary" : style;

  const text = disabled ? theme.textMuted : textColour(resolvedStyle, theme);
  const borderCol = disabled ? theme.border : borderColour(resolvedStyle, theme);
  const bg = hovered && !disabled ? hoverBgColour(resolvedStyle, theme) : bgColour(resolvedStyle, theme);

  const buttonStyle: CSSProperties = {
    display: "flex",
    flexDirection: "row",
    alignItems: "center",
    justifyContent: JUSTIFY[alignment],
    gap: 8,
    flex: "none",
    padding: icon ? "8px" : "6px 10px",
    borderRadius: 6,
    background: hex(bg),
    color: hex(text),
    fontSize: 13,
    border: border ? `1px solid ${hex(borderCol)}` : "none",
    cursor: disabled ? "default" : "pointer",
    opacity: disabled ? 0.7 : 1,
  };

  const content = (
    <div style={buttonStyle} onClick={disabled ? undefined : onClick} {...(disabled ? {} : handlers)}>
      {icon && <Icon path={icon} size={iconSize} colour={text} />}
      {children}
    </div>
  );

  return tooltip ? <Hint text={tooltip}>{content}</Hint> : content;
}

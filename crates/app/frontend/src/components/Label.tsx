import type { CSSProperties, ReactNode } from "react";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

export type LabelSize = "regular" | "compact" | "chip";

const FONT_SIZE: Record<LabelSize, string> = {
  regular: "13px",
  compact: "12px",
  chip: "12px",
};

export function Label({
  children,
  colour,
  inherit = false,
  size = "regular",
  ellipsis = false,
  muted = false,
}: {
  children: ReactNode;
  colour?: number;
  inherit?: boolean;
  size?: LabelSize;
  ellipsis?: boolean;
  muted?: boolean;
}) {
  const { theme } = useTheme();
  const resolvedColour = colour ?? (muted ? theme.textMuted : theme.text);

  const style: CSSProperties = {
    minWidth: 0,
    fontSize: FONT_SIZE[size],
  };
  if (!inherit) style.color = hex(resolvedColour);
  if (ellipsis) {
    style.width = "100%";
    style.flexGrow = 1;
    style.overflow = "hidden";
    style.textOverflow = "ellipsis";
    style.whiteSpace = "nowrap";
  } else {
    style.width = "100%";
  }

  return <div style={style}>{children}</div>;
}

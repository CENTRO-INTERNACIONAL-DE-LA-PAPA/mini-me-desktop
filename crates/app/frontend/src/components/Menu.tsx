import { useEffect, type CSSProperties, type MouseEvent, type ReactNode } from "react";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

export function menuCardStyle(theme: ReturnType<typeof useTheme>["theme"]): CSSProperties {
  return {
    display: "flex",
    flexDirection: "column",
    minWidth: 190,
    padding: "4px 0",
    borderRadius: 6,
    background: hex(theme.elevated),
    border: `1px solid ${hex(theme.border)}`,
  };
}

export function MenuItem({
  label,
  trailing,
  danger = false,
  disabled = false,
  onClick,
}: {
  label: string;
  trailing?: string;
  danger?: boolean;
  disabled?: boolean;
  onClick?: (event: MouseEvent<HTMLDivElement>) => void;
}) {
  const { theme } = useTheme();
  const textColour = disabled ? theme.textFaint : danger ? theme.error : theme.text;

  return (
    <div
      onClick={disabled ? undefined : onClick}
      onMouseEnter={(e) => {
        if (!disabled) e.currentTarget.style.background = hex(theme.background);
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
      }}
      style={{
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "space-between",
        width: "100%",
        minWidth: 0,
        gap: 16,
        padding: "4px 12px",
        fontSize: 13,
        color: hex(textColour),
        cursor: disabled ? "default" : "pointer",
      }}
    >
      <span>{label}</span>
      {trailing && <span style={{ color: hex(theme.textFaint), fontSize: 12 }}>{trailing}</span>}
    </div>
  );
}

export function Menu({
  at,
  ignoreRightClick = false,
  onDismiss,
  children,
}: {
  at: { x: number; y: number };
  ignoreRightClick?: boolean;
  onDismiss?: () => void;
  children: ReactNode;
}) {
  const { theme } = useTheme();

  useEffect(() => {
    if (!onDismiss) return;
    const handler = (event: globalThis.MouseEvent) => {
      if (ignoreRightClick && event.button === 2) return;
      onDismiss();
    };
    window.addEventListener("mousedown", handler, true);
    return () => window.removeEventListener("mousedown", handler, true);
  }, [onDismiss, ignoreRightClick]);

  return (
    <div
      onMouseDown={(e) => e.stopPropagation()}
      style={{
        position: "fixed",
        left: at.x,
        top: at.y,
        zIndex: 100,
        ...menuCardStyle(theme),
      }}
    >
      {children}
    </div>
  );
}

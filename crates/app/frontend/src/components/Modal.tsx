import type { CSSProperties, ReactNode } from "react";
import { isLight } from "../theme/apca";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

export function Actions({ children }: { children: ReactNode }) {
  const style: CSSProperties = {
    display: "flex",
    flexDirection: "row",
    flex: "none",
    gap: 12,
    width: "100%",
    minWidth: 0,
  };
  return <div style={style}>{children}</div>;
}

export function Modal({
  title,
  width = 520,
  nav,
  body,
  actions,
  footer,
  onDismiss,
}: {
  title: string;
  width?: number;
  nav?: ReactNode;
  body: ReactNode;
  actions?: ReactNode;
  footer?: ReactNode;
  onDismiss?: () => void;
}) {
  const { theme } = useTheme();

  const scrolling = (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        flexGrow: 1,
        minHeight: 0,
        minWidth: 0,
        overflowY: "auto",
        padding: 16,
        gap: 12,
      }}
    >
      {body}
    </div>
  );

  return (
    <div
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onDismiss?.();
      }}
      style={{
        position: "absolute",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: isLight(theme) ? "rgba(51,51,51,0.4)" : "rgba(0,0,0,0.6)",
      }}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          width,
          maxHeight: 720,
          minHeight: 0,
          borderRadius: 10,
          overflow: "hidden",
          background: hex(theme.overlay),
          border: `1px solid ${hex(theme.borderStrong)}`,
        }}
      >
        <div
          style={{
            flex: "none",
            padding: "16px 16px 0 16px",
            color: hex(theme.textFaint),
            fontSize: 12,
          }}
        >
          {title}
        </div>
        {nav ? (
          <div style={{ display: "flex", flexDirection: "row", flexGrow: 1, minHeight: 0, minWidth: 0 }}>
            {nav}
            {scrolling}
          </div>
        ) : (
          scrolling
        )}
        {actions && (
          <div style={{ flex: "none", padding: "0 16px 12px 16px" }}>{actions}</div>
        )}
        {footer && <div style={{ flex: "none", padding: "0 16px 12px 16px" }}>{footer}</div>}
      </div>
    </div>
  );
}

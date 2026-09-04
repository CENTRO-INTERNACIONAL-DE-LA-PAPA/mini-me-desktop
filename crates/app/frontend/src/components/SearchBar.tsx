import type { ChangeEvent } from "react";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";
import { Icon } from "./Icon";

export function SearchBar({
  value,
  placeholder,
  onChange,
}: {
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  const { theme } = useTheme();

  return (
    <div
      style={{
        flex: "none",
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        gap: 8,
        width: "100%",
        minWidth: 0,
        padding: "6px 10px",
        borderRadius: 6,
        fontSize: 13,
        color: hex(theme.textMuted),
        background: hex(theme.surface),
        border: `1px solid ${hex(theme.border)}`,
      }}
    >
      <Icon path="icons/magnifying-glass.svg" size="small" colour={theme.textMuted} />
      <input
        value={value}
        placeholder={placeholder}
        onChange={(event: ChangeEvent<HTMLInputElement>) => onChange(event.target.value)}
        style={{
          flexGrow: 1,
          minWidth: 0,
          border: "none",
          outline: "none",
          background: "transparent",
          color: hex(theme.text),
          fontSize: 13,
        }}
      />
    </div>
  );
}

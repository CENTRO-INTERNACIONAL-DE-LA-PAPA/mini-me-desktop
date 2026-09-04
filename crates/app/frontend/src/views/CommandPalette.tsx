import { useEffect, useMemo, useState } from "react";
import { ipc } from "../lib/ipc";
import { useAppStore } from "../lib/store";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

interface PaletteCommand {
  id: string;
  label: string;
  hint: string;
  run: () => void;
}

export function CommandPalette({
  onOpenSettings,
  onOpenAbout,
}: {
  onOpenSettings: () => void;
  onOpenAbout: () => void;
}) {
  const { theme } = useTheme();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const { startNewConversation, toggleSidebar } = useAppStore((state) => ({
    startNewConversation: state.startNewConversation,
    toggleSidebar: state.toggleSidebar,
  }));

  const commands: PaletteCommand[] = useMemo(
    () => [
      { id: "new-thread", label: "New conversation", hint: "⌘N", run: () => startNewConversation(null) },
      { id: "restart-backend", label: "Restart backend", hint: "", run: () => ipc.restartBackend() },
      { id: "open-settings", label: "Open settings", hint: "", run: onOpenSettings },
      { id: "toggle-sidebar", label: "Toggle sidebar", hint: "", run: toggleSidebar },
      { id: "open-about", label: "About Mini-Me", hint: "", run: onOpenAbout },
    ],
    [startNewConversation, toggleSidebar, onOpenSettings, onOpenAbout],
  );

  const matched = useMemo(
    () => commands.filter((c) => c.label.toLowerCase().includes(query.toLowerCase())),
    [commands, query],
  );

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "p") {
        e.preventDefault();
        setOpen((v) => !v);
        setQuery("");
        setSelected(0);
      } else if (e.key === "Escape" && open) {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open]);

  if (!open) return null;

  const runSelected = () => {
    const command = matched[selected];
    if (command) {
      setOpen(false);
      command.run();
    }
  };

  return (
    <div
      onClick={() => setOpen(false)}
      style={{ position: "fixed", inset: 0, display: "flex", flexDirection: "column", alignItems: "center", zIndex: 200 }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          marginTop: 96,
          width: 520,
          display: "flex",
          flexDirection: "column",
          background: hex(theme.surface),
          border: `1px solid ${hex(theme.border)}`,
        }}
      >
        <div style={{ padding: 8, borderBottom: `1px solid ${hex(theme.border)}` }}>
          <input
            autoFocus
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelected(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") setSelected((i) => Math.min(i + 1, matched.length - 1));
              if (e.key === "ArrowUp") setSelected((i) => Math.max(i - 1, 0));
              if (e.key === "Enter") runSelected();
            }}
            placeholder="Type a command…"
            style={{
              width: "100%",
              border: "none",
              outline: "none",
              background: "transparent",
              color: hex(theme.text),
              fontSize: 14,
            }}
          />
        </div>
        <div style={{ display: "flex", flexDirection: "column" }}>
          {matched.length === 0 && (
            <div style={{ padding: 8, fontSize: 13, color: hex(theme.textMuted) }}>No matching command.</div>
          )}
          {matched.map((command, index) => (
            <div
              key={command.id}
              onClick={() => {
                setOpen(false);
                command.run();
              }}
              style={{
                display: "flex",
                flexDirection: "row",
                justifyContent: "space-between",
                padding: "6px 8px",
                background: index === selected ? hex(theme.border) : "transparent",
                color: hex(index === selected ? theme.text : theme.textMuted),
                fontSize: 13,
                cursor: "pointer",
              }}
            >
              <span>{command.label}</span>
              <span style={{ fontSize: 11, color: hex(theme.textMuted) }}>{command.hint}</span>
            </div>
          ))}
        </div>
        <div
          style={{
            padding: "4px 8px",
            display: "flex",
            flexDirection: "row",
            gap: 4,
            borderTop: `1px solid ${hex(theme.border)}`,
            color: hex(theme.textMuted),
            fontSize: 11,
          }}
        >
          <span>↑↓ select · enter run · esc close</span>
        </div>
      </div>
    </div>
  );
}

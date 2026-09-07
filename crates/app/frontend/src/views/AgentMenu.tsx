import { useEffect, useState } from "react";
import { Icon, Label } from "../components";
import { ipc } from "../lib/ipc";
import type { Subagent } from "../lib/protocol";
import { subagentDisplay } from "../lib/subagentDisplay";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

export function AgentMenu({
  current,
  onChoose,
}: {
  current: string | null;
  onChoose: (name: string | null) => void;
}) {
  const { theme } = useTheme();
  const [agents, setAgents] = useState<Subagent[]>([]);
  const [pillHovered, setPillHovered] = useState(false);
  const [menuHovered, setMenuHovered] = useState(false);
  const open = pillHovered || menuHovered;

  useEffect(() => {
    ipc.listSubagents().then(setAgents);
  }, []);

  const currentDisplay = current ? subagentDisplay(current) : null;

  return (
    <div style={{ position: "relative", flex: "none" }}>
      <div
        onMouseEnter={() => setPillHovered(true)}
        onMouseLeave={() => setPillHovered(false)}
        style={{
          display: "flex",
          flexDirection: "row",
          alignItems: "center",
          gap: 8,
          flex: "none",
          padding: "4px 12px",
          borderRadius: "6px 6px 0 0",
          background: hex(theme.surface),
          border: `1px solid ${hex(theme.border)}`,
          borderBottom: "none",
          cursor: "pointer",
        }}
      >
        <Icon path="icons/agent-ellipse.svg" size="extraSmall" colour={currentDisplay?.colour ?? 0xffffff} />
        <span style={{ fontSize: 12, color: hex(theme.textMuted) }}>{currentDisplay?.label ?? "Auto"}</span>
      </div>

      {open && (
        <div
          onMouseEnter={() => setMenuHovered(true)}
          onMouseLeave={() => setMenuHovered(false)}
          className="thin-scroll"
          style={{
            position: "absolute",
            bottom: "100%",
            left: 0,
            width: 280,
            maxHeight: 220,
            overflowY: "auto",
            display: "flex",
            flexDirection: "column",
            padding: "4px 0",
            borderRadius: 6,
            background: hex(theme.elevated),
            border: `1px solid ${hex(theme.border)}`,
            zIndex: 30,
          }}
        >
          <div
            onClick={() => onChoose(null)}
            style={{
              display: "flex",
              flexDirection: "row",
              alignItems: "center",
              gap: 12,
              padding: "4px 12px",
              borderRadius: 6,
              cursor: "pointer",
            }}
          >
            <Icon path="icons/agent-ellipse.svg" size="extraSmall" colour={0xffffff} />
            <div style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
              <Label ellipsis>Auto</Label>
              <Label muted size="compact" ellipsis>
                Chooses the best specialist for each turn
              </Label>
            </div>
          </div>

          {agents.length === 0 && (
            <div style={{ padding: 8, fontSize: 13, color: hex(theme.textMuted) }}>
              No specialist list yet. It is written when the backend builds a coordinator, so ask one
              ordinary question first — and if you just updated the app, restart the backend (⌘P →
              Restart backend).
            </div>
          )}

          {agents.map((agent) => {
            const display = subagentDisplay(agent.name);
            return (
              <div
                key={agent.name}
                onClick={() => onChoose(agent.name)}
                style={{
                  display: "flex",
                  flexDirection: "row",
                  alignItems: "center",
                  gap: 12,
                  padding: "4px 12px",
                  cursor: "pointer",
                }}
              >
                <Icon path="icons/agent-ellipse.svg" size="extraSmall" colour={display.colour} />
                <div style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
                  <Label ellipsis>{display.label}</Label>
                  <Label muted size="compact" ellipsis>
                    {agent.description}
                  </Label>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

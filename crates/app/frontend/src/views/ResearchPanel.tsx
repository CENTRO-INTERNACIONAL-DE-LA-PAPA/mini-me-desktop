import { useState } from "react";
import { Button, Icon, Label } from "../components";
import { ipc } from "../lib/ipc";
import { useAppStore } from "../lib/store";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  const { theme } = useTheme();
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, width: "100%", minWidth: 0 }}>
      <Label size="compact" colour={theme.textFaint}>
        {title}
      </Label>
      {children}
    </div>
  );
}

function MissionBlock() {
  const { theme } = useTheme();
  const { snapshot, setSnapshotProject } = useAppStore((state) => ({
    snapshot: state.snapshot,
    setSnapshotProject: state.setSnapshotProject,
  }));
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(snapshot?.project?.mission ?? "");
  const mission = snapshot?.project?.mission ?? "";

  const save = async () => {
    const project = await ipc.setMission(text);
    setSnapshotProject(project);
    setEditing(false);
  };

  if (editing) {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 4, width: "100%", minWidth: 0 }}>
        <textarea
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              save();
            }
            if (e.key === "Escape") setEditing(false);
          }}
          rows={3}
          style={{
            width: "100%",
            padding: 8,
            borderRadius: 6,
            border: `1px solid ${hex(theme.accent)}`,
            background: "transparent",
            color: hex(theme.text),
            fontSize: 13,
            resize: "vertical",
          }}
        />
        <Label size="compact" muted>
          Enter to save · Esc to cancel. Mini-Me reads this on every turn.
        </Label>
      </div>
    );
  }

  return (
    <div
      onClick={() => {
        setText(mission);
        setEditing(true);
      }}
      style={{ width: "100%", minWidth: 0, padding: "4px 8px", borderRadius: 6, cursor: "pointer" }}
    >
      <Label colour={mission ? theme.text : theme.textMuted}>
        {mission || "No mission yet — press to write one, or it comes from your first question."}
      </Label>
    </div>
  );
}

function ItemRow({ title, subtitle }: { title: string; subtitle?: string }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", width: "100%", minWidth: 0, gap: 2 }}>
      <Label ellipsis>{title}</Label>
      {subtitle && (
        <Label muted size="compact" ellipsis>
          {subtitle}
        </Label>
      )}
    </div>
  );
}

export function ResearchPanel({ onClose }: { onClose: () => void }) {
  const { theme } = useTheme();
  const snapshot = useAppStore((state) => state.snapshot);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: 320,
        height: "100%",
        flex: "none",
        margin: 8,
        borderRadius: 10,
        overflow: "hidden",
        background: hex(theme.surface),
        border: `1px solid ${hex(theme.border)}`,
      }}
    >
      <div style={{ display: "flex", flexDirection: "row", padding: "8px 12px" }}>
        <Button icon="icons/sidebar-simple-right.svg" style="secondaryWhite" border={false} onClick={onClose} />
      </div>
      <div className="thin-scroll" style={{ display: "flex", flexDirection: "column", flexGrow: 1, minHeight: 0, overflowY: "auto", padding: 16, gap: 16 }}>
        <Section title="MISSION">
          <MissionBlock />
        </Section>

        {snapshot?.project && (snapshot.project.completed.length > 0 || snapshot.project.pending.length > 0) && (
          <Section title="PROGRESS">
            {snapshot.project.completed.map((item, i) => (
              <div key={`c-${i}`} style={{ display: "flex", flexDirection: "row", gap: 8 }}>
                <span style={{ color: hex(theme.success) }}>✓</span>
                <Label size="compact">{item}</Label>
              </div>
            ))}
            {snapshot.project.pending.map((item, i) => (
              <div key={`p-${i}`} style={{ display: "flex", flexDirection: "row", gap: 8 }}>
                <span style={{ color: hex(theme.textFaint) }}>○</span>
                <Label size="compact" muted>
                  {item}
                </Label>
              </div>
            ))}
          </Section>
        )}

        {snapshot && snapshot.datasets.length > 0 && (
          <Section title="DATASETS">
            {snapshot.datasets.map((dataset, i) => (
              <ItemRow key={i} title={dataset.title} subtitle={dataset.repository ?? undefined} />
            ))}
          </Section>
        )}

        {snapshot && snapshot.documents.length > 0 && (
          <Section title="LIBRARY">
            {snapshot.documents.map((document, i) => (
              <ItemRow key={i} title={document.title} subtitle={document.summary} />
            ))}
          </Section>
        )}

        {snapshot && snapshot.sources.length > 0 && (
          <Section title="SOURCES">
            {snapshot.sources.map((source, i) => (
              <ItemRow key={i} title={source.citation} />
            ))}
          </Section>
        )}

        {snapshot && snapshot.reports.length > 0 && (
          <Section title="REPORTS">
            {snapshot.reports.map((report, i) => (
              <ItemRow key={i} title={report.title} />
            ))}
          </Section>
        )}

        {snapshot && snapshot.jobs.length > 0 && (
          <Section title="RUNNING">
            {snapshot.jobs.map((job, i) => (
              <div key={i} style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 8 }}>
                <Icon path="icons/road.svg" size="small" colour={theme.running} />
                <ItemRow title={job.question} subtitle={job.status} />
              </div>
            ))}
          </Section>
        )}
      </div>
    </div>
  );
}

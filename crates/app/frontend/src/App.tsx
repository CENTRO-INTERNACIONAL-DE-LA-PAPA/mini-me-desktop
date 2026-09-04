import { useEffect, useState } from "react";
import { ipc } from "./lib/ipc";
import { useAppStore } from "./lib/store";
import { AboutModal } from "./views/AboutModal";
import { Chat } from "./views/Chat";
import { CommandPalette } from "./views/CommandPalette";
import { SettingsView } from "./views/SettingsView";
import { ResearchPanel } from "./views/ResearchPanel";
import { Sidebar } from "./views/Sidebar";
import { StatusBar } from "./views/StatusBar";

export function App() {
  const { applyTurnEvent, setExecutionInfo, setSnapshotProject } = useAppStore((state) => ({
    applyTurnEvent: state.applyTurnEvent,
    setExecutionInfo: state.setExecutionInfo,
    setSnapshotProject: state.setSnapshotProject,
  }));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [panelOpen, setPanelOpen] = useState(true);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    ipc.onTurnEvent(applyTurnEvent).then((fn) => {
      unlisten = fn;
    });
    Promise.all([ipc.getExecutionLabel(), ipc.getBaseUrl()]).then(([executionLabel, baseUrl]) =>
      setExecutionInfo(executionLabel, baseUrl),
    );
    ipc.warmUp();
    ipc.fetchProject().then(setSnapshotProject);
    return () => unlisten?.();
  }, [applyTurnEvent, setExecutionInfo, setSnapshotProject]);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh", width: "100vw" }}>
      <div style={{ display: "flex", flexDirection: "row", flexGrow: 1, minHeight: 0 }}>
        <Sidebar onOpenSettings={() => setSettingsOpen(true)} />
        <Chat />
        {panelOpen && <ResearchPanel onClose={() => setPanelOpen(false)} />}
        {settingsOpen && <SettingsView onClose={() => setSettingsOpen(false)} />}
        {aboutOpen && <AboutModal onClose={() => setAboutOpen(false)} />}
      </div>
      <StatusBar />
      <CommandPalette onOpenSettings={() => setSettingsOpen(true)} onOpenAbout={() => setAboutOpen(true)} />
    </div>
  );
}

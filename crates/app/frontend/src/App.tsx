import { useEffect, useState } from "react";
import { useShallow } from "zustand/react/shallow";
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
  const { applyTurnEvent, setExecutionInfo, setSnapshotProject, setBackendStart } = useAppStore(
    useShallow((state) => ({
      applyTurnEvent: state.applyTurnEvent,
      setExecutionInfo: state.setExecutionInfo,
      setSnapshotProject: state.setSnapshotProject,
      setBackendStart: state.setBackendStart,
    })),
  );
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<"model" | "setup">("model");
  const [aboutOpen, setAboutOpen] = useState(false);
  const [panelOpen, setPanelOpen] = useState(true);
  const [checkedOnStartup, setCheckedOnStartup] = useState(false);

  const openSettings = (section: "model" | "setup" = "model") => {
    setSettingsSection(section);
    setSettingsOpen(true);
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    ipc.onTurnEvent(applyTurnEvent).then((fn) => {
      unlisten = fn;
    });
    Promise.all([ipc.getExecutionLabel(), ipc.getBaseUrl()]).then(([executionLabel, baseUrl]) =>
      setExecutionInfo(executionLabel, baseUrl),
    );
    ipc.warmUp().then(setBackendStart);
    ipc.fetchProject().then(setSnapshotProject);
    return () => unlisten?.();
  }, [applyTurnEvent, setExecutionInfo, setSnapshotProject, setBackendStart]);

  useEffect(() => {
    if (checkedOnStartup) return;
    setCheckedOnStartup(true);
    ipc.runPreflight().then((report) => {
      if (report.checks.some((check) => check.state === "Fail")) openSettings("setup");
    });
  }, [checkedOnStartup]);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh", width: "100vw" }}>
      <div style={{ display: "flex", flexDirection: "row", flexGrow: 1, minHeight: 0 }}>
        <Sidebar onOpenSettings={() => openSettings()} />
        <Chat />
        {panelOpen && <ResearchPanel onClose={() => setPanelOpen(false)} />}
        {settingsOpen && <SettingsView onClose={() => setSettingsOpen(false)} initialSection={settingsSection} />}
        {aboutOpen && <AboutModal onClose={() => setAboutOpen(false)} />}
      </div>
      <StatusBar />
      <CommandPalette
        onOpenSettings={() => openSettings()}
        onOpenSetup={() => openSettings("setup")}
        onOpenAbout={() => setAboutOpen(true)}
      />
    </div>
  );
}

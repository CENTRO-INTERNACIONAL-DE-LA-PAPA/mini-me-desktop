import { useEffect, useState } from "react";
import { Button, Dropdown, Label, Modal, MenuItem, NavEntry, NavRail, SettingRow, Toggle } from "../components";
import { ipc } from "../lib/ipc";
import type { Provider, Settings } from "../lib/protocol";
import { useAppStore } from "../lib/store";
import { useSetupPane } from "../lib/useSetupPane";
import { hex, THEMES } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";
import { SetupActions, SetupBody } from "./SetupPane";
import { ThemeGallery } from "./ThemeGallery";

type Section = "appearance" | "model" | "research" | "backend" | "setup";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "appearance", label: "Appearance" },
  { id: "model", label: "Model" },
  { id: "research", label: "Research" },
  { id: "backend", label: "Backend" },
  { id: "setup", label: "Setup" },
];

function problems(settings: Settings, providers: Provider[], hasKey: boolean): string[] {
  const provider = providers.find((p) => p.id === settings.provider);
  if (!provider) return [`Unknown provider "${settings.provider}".`];
  const out: string[] = [];
  if (!settings.model_id.trim()) out.push("No model id.");
  if (provider.needs_base_url && !settings.base_url.trim()) out.push("A custom provider needs its base URL.");
  if (!hasKey) out.push(`No API key stored for ${provider.label}.`);
  return out;
}

export function SettingsView({ onClose, initialSection = "model" }: { onClose: () => void; initialSection?: Section }) {
  const { theme, setThemeName, installedThemes } = useTheme();
  const setExecutionInfo = useAppStore((state) => state.setExecutionInfo);
  const setup = useSetupPane();
  const [section, setSection] = useState<Section>(initialSection);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [settingsPath, setSettingsPath] = useState("");
  const [providers, setProviders] = useState<Provider[]>([]);
  const [keyTarget, setKeyTarget] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [hasKey, setHasKey] = useState(false);
  const [astaToken, setAstaToken] = useState("");
  const [astaTokenSet, setAstaTokenSet] = useState(false);
  const [astaApiKey, setAstaApiKey] = useState("");
  const [astaApiKeySet, setAstaApiKeySet] = useState(false);
  const [providerOpen, setProviderOpen] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);
  const [themeOpen, setThemeOpen] = useState(false);
  const [galleryOpen, setGalleryOpen] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    ipc.getSettings().then((loaded) => {
      setSettings(loaded);
      setKeyTarget(loaded.provider);
    });
    ipc.getSettingsPath().then(setSettingsPath);
    ipc.getProviders().then(setProviders);
    ipc.getSecret("ASTA_TOKEN").then((value) => setAstaTokenSet(value !== null));
    ipc.getSecret("ASTA_API_KEY").then((value) => setAstaApiKeySet(value !== null));
  }, []);

  useEffect(() => {
    if (!keyTarget) return;
    ipc.getSecret(`llm:${keyTarget}`).then((value) => {
      setApiKey(value ?? "");
      setHasKey(value !== null);
    });
  }, [keyTarget]);

  useEffect(() => {
    if (section === "setup") setup.runPreflight();
  }, [section]);

  if (!settings) return null;

  const provider = providers.find((p) => p.id === settings.provider);
  const update = (patch: Partial<Settings>) => setSettings({ ...settings, ...patch });

  const save = async () => {
    setSaving(true);
    try {
      if (apiKey.trim()) {
        await ipc.setSecretValue(`llm:${keyTarget}`, apiKey.trim());
      }
      if (astaToken.trim()) {
        await ipc.setSecretValue("ASTA_TOKEN", astaToken.trim());
      }
      if (astaApiKey.trim()) {
        await ipc.setSecretValue("ASTA_API_KEY", astaApiKey.trim());
      }
      await ipc.saveSettings(settings);
      setThemeName(settings.theme);
      const [executionLabel, baseUrl] = await Promise.all([ipc.getExecutionLabel(), ipc.getBaseUrl()]);
      setExecutionInfo(executionLabel, baseUrl);
      onClose();
    } finally {
      setSaving(false);
    }
  };

  if (galleryOpen) {
    return (
      <ThemeGallery
        onClose={() => setGalleryOpen(false)}
        onInstalled={(name) => {
          update({ theme: name });
          setGalleryOpen(false);
        }}
      />
    );
  }

  const footerProblems = problems(settings, providers, hasKey || apiKey.trim() !== "");

  return (
    <Modal
      title="SETTINGS"
      width={640}
      onDismiss={onClose}
      nav={
        <NavRail>
          {SECTIONS.map((s) => (
            <NavEntry key={s.id} label={s.label} selected={section === s.id} onClick={() => setSection(s.id)} />
          ))}
        </NavRail>
      }
      body={
        section === "appearance" ? (
          <SettingRow title="Theme" description="Which palette to draw the app with.">
            <div style={{ display: "flex", flexDirection: "row", gap: 8 }}>
              <Dropdown value={settings.theme} open={themeOpen} onClick={() => setThemeOpen((v) => !v)}>
                {[...THEMES, ...installedThemes].map(([name]) => (
                  <MenuItem
                    key={name}
                    label={name}
                    onClick={() => {
                      update({ theme: name });
                      setThemeOpen(false);
                    }}
                  />
                ))}
              </Dropdown>
              <Button onClick={() => setGalleryOpen(true)}>Browse more…</Button>
            </div>
          </SettingRow>
        ) : section === "model" ? (
          <>
            <SettingRow title="Provider" description="Which model provider to use.">
              <Dropdown value={provider?.label ?? settings.provider} open={providerOpen} onClick={() => setProviderOpen((v) => !v)}>
                {providers.map((p) => (
                  <MenuItem
                    key={p.id}
                    label={p.label}
                    onClick={() => {
                      update({ provider: p.id, model_id: p.suggested_model });
                      setProviderOpen(false);
                    }}
                  />
                ))}
              </Dropdown>
            </SettingRow>
            <SettingRow title="Model" description="Which model to send requests to.">
              <Dropdown value={settings.model_id} open={modelOpen} onClick={() => setModelOpen((v) => !v)}>
                {(provider?.models ?? []).map((model) => (
                  <MenuItem
                    key={model}
                    label={model}
                    onClick={() => {
                      update({ model_id: model });
                      setModelOpen(false);
                    }}
                  />
                ))}
              </Dropdown>
            </SettingRow>
            {provider?.needs_base_url && (
              <SettingRow title="Base URL" description="Required for an OpenAI-compatible endpoint.">
                <input
                  value={settings.base_url}
                  onChange={(e) => update({ base_url: e.target.value })}
                  style={{ padding: "4px 8px", borderRadius: 6 }}
                />
              </SettingRow>
            )}
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <Label muted size="compact">
                Keys — one per company, set them in any order
              </Label>
              <div style={{ display: "flex", flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
                {providers.map((p) => (
                  <div
                    key={p.id}
                    onClick={() => setKeyTarget(p.id)}
                    style={{
                      padding: "4px 10px",
                      borderRadius: 6,
                      border: `1px solid ${hex(keyTarget === p.id ? theme.accent : theme.border)}`,
                      background: keyTarget === p.id ? hex(theme.accentSoft) : "transparent",
                      color: hex(keyTarget === p.id ? theme.text : theme.textMuted),
                      fontSize: 12,
                      cursor: "pointer",
                    }}
                  >
                    {p.label}
                  </div>
                ))}
              </div>
            </div>
            <SettingRow title={`API key${hasKey ? " · stored" : " · not set"}`} description="Stored in the OS keychain, never in a file.">
              <input
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-…"
                style={{ padding: "4px 8px", borderRadius: 6, width: 220 }}
              />
            </SettingRow>
          </>
        ) : section === "research" ? (
          <>
            <SettingRow
              title={`Asta token${astaTokenSet ? " · stored" : " · not set"}`}
              description="From `asta auth login` — powers literature search and the theorizer."
            >
              <input
                type="password"
                value={astaToken}
                onChange={(e) => setAstaToken(e.target.value)}
                placeholder="paste to set"
                style={{ padding: "4px 8px", borderRadius: 6, width: 220 }}
              />
            </SettingRow>
            <SettingRow
              title={`Asta API key${astaApiKeySet ? " · stored" : " · not set"}`}
              description="The Asta CLI reads this from its environment when a command runs."
            >
              <input
                type="password"
                value={astaApiKey}
                onChange={(e) => setAstaApiKey(e.target.value)}
                placeholder="paste to set"
                style={{ padding: "4px 8px", borderRadius: 6, width: 220 }}
              />
            </SettingRow>
          </>
        ) : section === "backend" ? (
          <>
            <SettingRow title="Backend port" description="Which local port the sidecar talks to.">
              <input
                type="number"
                value={settings.backend_port}
                onChange={(e) => update({ backend_port: Number(e.target.value) })}
                style={{ padding: "4px 8px", borderRadius: 6, width: 100 }}
              />
            </SettingRow>
            <SettingRow title="Run code on this machine" description="Commands run in your own WSL distro rather than a remote sandbox.">
              <Toggle on={settings.local_execution} onClick={() => update({ local_execution: !settings.local_execution })} />
            </SettingRow>
            <SettingRow title="Ask before every command" description="Pause and show each command, so nothing runs without you seeing it.">
              <Toggle on={settings.approve_execute} onClick={() => update({ approve_execute: !settings.approve_execute })} />
            </SettingRow>
            <SettingRow title="Let work run in the background" description="Long jobs keep going while you carry on asking questions.">
              <Toggle on={settings.async_subagents} onClick={() => update({ async_subagents: !settings.async_subagents })} />
            </SettingRow>
            <SettingRow
              title="Show what ran and what was claimed"
              description="Adds two lines to Outputs comparing what the agent said it did against what is in this conversation's folder."
            >
              <Toggle on={settings.run_record} onClick={() => update({ run_record: !settings.run_record })} />
            </SettingRow>
          </>
        ) : (
          <SetupBody setup={setup} />
        )
      }
      actions={
        section === "setup" ? (
          <SetupActions setup={setup} onClose={onClose} />
        ) : (
          <div style={{ display: "flex", flexDirection: "row", gap: 12 }}>
            <Button style="primary" onClick={save} disabled={saving}>
              {saving ? "Saving…" : "Save"}
            </Button>
            <Button onClick={onClose}>Cancel</Button>
          </div>
        )
      }
      footer={
        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          {footerProblems.map((problem) => (
            <Label key={problem} size="compact" colour={theme.error}>
              {problem}
            </Label>
          ))}
          <Label size="compact" muted>{`Keys live in your OS keychain, never in a file. ${settingsPath}`}</Label>
        </div>
      }
    />
  );
}

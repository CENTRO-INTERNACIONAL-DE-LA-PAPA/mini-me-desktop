import { useEffect, useState } from "react";
import { Button, Dropdown, Label, Modal, MenuItem, NavEntry, NavRail, SettingRow, Toggle } from "../components";
import { ipc } from "../lib/ipc";
import type { Provider, Settings } from "../lib/protocol";
import { useAppStore } from "../lib/store";
import { THEMES } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";
import { ThemeGallery } from "./ThemeGallery";

type Section = "general" | "models";

export function SettingsView({ onClose }: { onClose: () => void }) {
  const { setThemeName, installedThemes } = useTheme();
  const setExecutionInfo = useAppStore((state) => state.setExecutionInfo);
  const [section, setSection] = useState<Section>("general");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [apiKey, setApiKey] = useState("");
  const [providerOpen, setProviderOpen] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);
  const [themeOpen, setThemeOpen] = useState(false);
  const [galleryOpen, setGalleryOpen] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    ipc.getSettings().then(setSettings);
    ipc.getProviders().then(setProviders);
  }, []);

  useEffect(() => {
    if (!settings) return;
    ipc.getSecret(`llm:${settings.provider}`).then((value) => setApiKey(value ?? ""));
  }, [settings?.provider]);

  if (!settings) return null;

  const provider = providers.find((p) => p.id === settings.provider);

  const update = (patch: Partial<Settings>) => setSettings({ ...settings, ...patch });

  const save = async () => {
    setSaving(true);
    try {
      if (apiKey.trim()) {
        await ipc.setSecretValue(`llm:${settings.provider}`, apiKey.trim());
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

  return (
    <Modal
      title="SETTINGS"
      width={640}
      onDismiss={onClose}
      nav={
        <NavRail>
          <NavEntry label="General" selected={section === "general"} onClick={() => setSection("general")} />
          <NavEntry label="Models" selected={section === "models"} onClick={() => setSection("models")} />
        </NavRail>
      }
      body={
        section === "general" ? (
          <>
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
            <SettingRow
              title="Host execution"
              description="Run the agent's code on this machine rather than in the remote sandbox."
            >
              <Toggle on={settings.local_execution} onClick={() => update({ local_execution: !settings.local_execution })} />
            </SettingRow>
            <SettingRow title="Approve every command" description="Ask before every execute. Off is for automation.">
              <Toggle on={settings.approve_execute} onClick={() => update({ approve_execute: !settings.approve_execute })} />
            </SettingRow>
          </>
        ) : (
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
            <SettingRow title="API key" description="Stored in the OS keychain, never in a file.">
              <input
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-…"
                style={{ padding: "4px 8px", borderRadius: 6, width: 220 }}
              />
            </SettingRow>
          </>
        )
      }
      actions={
        <div style={{ display: "flex", flexDirection: "row", gap: 12 }}>
          <Button style="primary" onClick={save} disabled={saving}>
            {saving ? "Saving…" : "Save"}
          </Button>
          <Button onClick={onClose}>Cancel</Button>
        </div>
      }
      footer={<Label size="compact" muted>{`settings.toml`}</Label>}
    />
  );
}

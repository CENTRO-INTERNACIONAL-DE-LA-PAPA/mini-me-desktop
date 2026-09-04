import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { ipc } from "../lib/ipc";
import { DEFAULT_THEME_NAME, fromRawTheme, hex, themeByName, type RawTheme, type Theme } from "./theme";

interface ThemeContextValue {
  theme: Theme;
  themeName: string;
  setThemeName: (name: string) => void;
  installedThemes: [string, Theme][];
  refreshInstalledThemes: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

const CSS_VAR_BY_FIELD: Record<keyof Theme, string> = {
  background: "--color-background",
  surface: "--color-surface",
  elevated: "--color-elevated",
  overlay: "--color-overlay",
  accentSoft: "--color-accent-soft",
  hover: "--color-hover",
  text: "--color-text",
  textMuted: "--color-text-muted",
  textFaint: "--color-text-faint",
  border: "--color-border",
  borderStrong: "--color-border-strong",
  accent: "--color-accent",
  accentHover: "--color-accent-hover",
  success: "--color-success",
  warning: "--color-warning",
  error: "--color-error",
  running: "--color-running",
};

export function ThemeProvider({
  initialThemeName,
  children,
}: {
  initialThemeName?: string;
  children: ReactNode;
}) {
  const [themeName, setThemeName] = useState(initialThemeName ?? DEFAULT_THEME_NAME);
  const [installedThemes, setInstalledThemes] = useState<[string, Theme][]>([]);
  const theme = useMemo(() => themeByName(themeName, installedThemes), [themeName, installedThemes]);

  const refreshInstalledThemes = () => {
    ipc.listInstalledThemes().then((raw: [string, RawTheme][]) =>
      setInstalledThemes(raw.map(([name, t]) => [name, fromRawTheme(t)])),
    );
  };

  useEffect(() => {
    refreshInstalledThemes();
  }, []);

  useEffect(() => {
    const root = document.documentElement.style;
    for (const [field, cssVar] of Object.entries(CSS_VAR_BY_FIELD) as [keyof Theme, string][]) {
      root.setProperty(cssVar, hex(theme[field]));
    }
  }, [theme]);

  const value = useMemo(
    () => ({ theme, themeName, setThemeName, installedThemes, refreshInstalledThemes }),
    [theme, themeName, installedThemes],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const value = useContext(ThemeContext);
  if (!value) throw new Error("useTheme must be used inside a ThemeProvider");
  return value;
}

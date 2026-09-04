export interface Theme {
  background: number;
  surface: number;
  elevated: number;
  overlay: number;
  accentSoft: number;
  hover: number;
  text: number;
  textMuted: number;
  textFaint: number;
  border: number;
  borderStrong: number;
  accent: number;
  accentHover: number;
  success: number;
  warning: number;
  error: number;
  running: number;
}

/** The wire shape `theme::Theme` serializes as (snake_case, no renaming). */
export interface RawTheme {
  background: number;
  surface: number;
  elevated: number;
  overlay: number;
  accent_soft: number;
  hover: number;
  text: number;
  text_muted: number;
  text_faint: number;
  border: number;
  border_strong: number;
  accent: number;
  accent_hover: number;
  success: number;
  warning: number;
  error: number;
  running: number;
}

export function fromRawTheme(raw: RawTheme): Theme {
  return {
    background: raw.background,
    surface: raw.surface,
    elevated: raw.elevated,
    overlay: raw.overlay,
    accentSoft: raw.accent_soft,
    hover: raw.hover,
    text: raw.text,
    textMuted: raw.text_muted,
    textFaint: raw.text_faint,
    border: raw.border,
    borderStrong: raw.border_strong,
    accent: raw.accent,
    accentHover: raw.accent_hover,
    success: raw.success,
    warning: raw.warning,
    error: raw.error,
    running: raw.running,
  };
}

export const VIOLET_POTATO: Theme = {
  background: 0x18141c,
  surface: 0x221d26,
  elevated: 0x2b2730,
  overlay: 0x35303a,
  accentSoft: 0x3a2722,
  hover: 0x572d3b,
  text: 0xe7e4eb,
  textMuted: 0xd1ccd6,
  textFaint: 0xc0bbc6,
  border: 0x3c3444,
  borderStrong: 0x594d64,
  accent: 0xe2c3ff,
  accentHover: 0xf3d3ff,
  success: 0xb1d396,
  warning: 0xe6c485,
  error: 0xffb7a7,
  running: 0x8ed0fb,
};

export const VIOLET_POTATO_LIGHT: Theme = {
  background: 0xf1eff3,
  surface: 0xf7f5f9,
  elevated: 0xfbf9fd,
  overlay: 0xfefcff,
  accentSoft: 0xf6e5db,
  hover: 0xf9dbef,
  text: 0x333135,
  textMuted: 0x545158,
  textFaint: 0x625d66,
  border: 0xe4e0e7,
  borderStrong: 0xc4bfc8,
  accent: 0x6a4688,
  accentHover: 0x512d6d,
  success: 0x456920,
  warning: 0x7d5800,
  error: 0x964738,
  running: 0x006492,
};

export const MAGENTA_POTATO: Theme = {
  ...VIOLET_POTATO,
  hover: 0x463256,
  accent: 0xffbdd4,
  accentHover: 0xffd1e9,
};

export const MAGENTA_POTATO_LIGHT: Theme = {
  ...VIOLET_POTATO_LIGHT,
  hover: 0xecdbf6,
  accent: 0x883b58,
  accentHover: 0x6b213f,
};

export const BENCH: Theme = {
  background: 0xedebe6,
  surface: 0xf6f5f1,
  elevated: 0xfcfcfa,
  overlay: 0xffffff,
  hover: 0xd8dfd9,
  accentSoft: 0xddede7,
  text: 0x2f343a,
  textMuted: 0x5b6268,
  textFaint: 0x5c6267,
  border: 0xdfddd6,
  borderStrong: 0xc3c1b8,
  accent: 0x1f6f63,
  accentHover: 0x17564d,
  success: 0x2f6b23,
  warning: 0x8a5d04,
  error: 0xa63a34,
  running: 0x2e6da5,
};

export const BENCH_NIGHT: Theme = {
  background: 0x23262a,
  surface: 0x2a2e33,
  elevated: 0x333840,
  overlay: 0x383e46,
  hover: 0x3c474d,
  accentSoft: 0x284840,
  text: 0xe3e5e2,
  textMuted: 0xb0b5b2,
  textFaint: 0xafb5b2,
  border: 0x383d42,
  borderStrong: 0x495057,
  accent: 0x6fc3ae,
  accentHover: 0x8fd6c4,
  success: 0x9cc96b,
  warning: 0xe3b95c,
  error: 0xe89a97,
  running: 0x85b8e8,
};

export const MINI_ME_DARK: Theme = {
  background: 0x16161a,
  surface: 0x1c1c21,
  elevated: 0x232329,
  overlay: 0x2a2a31,
  hover: 0x3d3132,
  accentSoft: 0x3a2419,
  text: 0xececf0,
  textMuted: 0xb0b0ba,
  textFaint: 0x9c9ca7,
  border: 0x2f2f37,
  borderStrong: 0x3f3f49,
  accent: 0xe8703a,
  accentHover: 0xf58b5c,
  success: 0x5bbd7a,
  warning: 0xd9a441,
  error: 0xf1676b,
  running: 0x6aa9e0,
};

export const SLATE: Theme = {
  background: 0x14171c,
  surface: 0x1a1e24,
  elevated: 0x222731,
  overlay: 0x2a303b,
  hover: 0x2f3948,
  accentSoft: 0x1c3048,
  text: 0xe6eaf0,
  textMuted: 0xacb4c0,
  textFaint: 0x9aa3b0,
  border: 0x2b313b,
  borderStrong: 0x3b434f,
  accent: 0x6cb0f5,
  accentHover: 0x93c6fa,
  success: 0x5cc08a,
  warning: 0xd9a441,
  error: 0xf87a7e,
  running: 0x8ab4f8,
};

export const PAPER: Theme = {
  background: 0xf1efea,
  surface: 0xf7f5f1,
  elevated: 0xfbfaf8,
  overlay: 0xffffff,
  hover: 0,
  accentSoft: 0xf7ddcd,
  text: 0x24242a,
  textMuted: 0x55555f,
  textFaint: 0x5b5b66,
  border: 0xdcd8d1,
  borderStrong: 0xc3beb5,
  accent: 0xa8451a,
  accentHover: 0x8c3813,
  success: 0x14663a,
  warning: 0x855c05,
  error: 0xb32431,
  running: 0x1f5fa8,
};

export const HIGH_CONTRAST: Theme = {
  background: 0x000000,
  surface: 0x0b0b0d,
  elevated: 0x17171b,
  overlay: 0x1f1f25,
  hover: 0,
  accentSoft: 0x442a12,
  text: 0xffffff,
  textMuted: 0xd8d8de,
  textFaint: 0xb9b9c2,
  border: 0x40404a,
  borderStrong: 0x5a5a66,
  accent: 0xffa05c,
  accentHover: 0xffbb85,
  success: 0x67e08d,
  warning: 0xf2c14e,
  error: 0xff7d80,
  running: 0x8cc2ff,
};

export const THEMES: [string, Theme][] = [
  ["Violet Native Potato", VIOLET_POTATO],
  ["Violet Native Potato Light", VIOLET_POTATO_LIGHT],
  ["Magenta Native Potato", MAGENTA_POTATO],
  ["Magenta Native Potato Light", MAGENTA_POTATO_LIGHT],
  ["Bench", BENCH],
  ["Bench Night", BENCH_NIGHT],
  ["Mini-Me Dark", MINI_ME_DARK],
  ["Slate", SLATE],
  ["Paper", PAPER],
  ["High Contrast", HIGH_CONTRAST],
];

export const DEFAULT_THEME: Theme = THEMES[0][1];
export const DEFAULT_THEME_NAME: string = THEMES[0][0];

const RENAMED: [string, string][] = [
  ["Papa Nativa", "Violet Native Potato"],
  ["Papa Nativa Light", "Violet Native Potato Light"],
];

export function canonicalThemeName(stored: string): string {
  const match = RENAMED.find(([was]) => was.toLowerCase() === stored.toLowerCase());
  return match ? match[1] : stored;
}

export function themeByName(name: string, extra: [string, Theme][] = []): Theme {
  const canonical = canonicalThemeName(name);
  const found = [...THEMES, ...extra].find(([n]) => n === canonical);
  return found ? found[1] : DEFAULT_THEME;
}

export function hex(colour: number): string {
  return `#${colour.toString(16).padStart(6, "0")}`;
}

export const CODE_FONT_STACK =
  "Menlo, Consolas, 'Cascadia Mono', 'DejaVu Sans Mono', 'Liberation Mono', 'Courier New', monospace";

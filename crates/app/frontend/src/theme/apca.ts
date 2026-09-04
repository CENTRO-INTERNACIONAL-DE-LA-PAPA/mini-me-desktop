import type { Theme } from "./theme";

function channel(value: number): number {
  const c = value / 255.0;
  return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

export function luminance(colour: number): number {
  const r = channel((colour >> 16) & 0xff);
  const g = channel((colour >> 8) & 0xff);
  const b = channel(colour & 0xff);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a: number, b: number): number {
  const x = luminance(a);
  const y = luminance(b);
  const [lighter, darker] = x > y ? [x, y] : [y, x];
  return (lighter + 0.05) / (darker + 0.05);
}

export function isLight(theme: Theme): boolean {
  return luminance(theme.background) > 0.5;
}

export function inkOn(fill: number, theme: Theme): number {
  if (contrast(theme.text, fill) >= 4.5) return theme.text;
  return contrast(0xffffff, fill) >= contrast(0x111111, fill) ? 0xffffff : 0x111111;
}

const HOVER_FLOOR = 1.12;

export function hoverOver(base: number, theme: Theme): number {
  const named = theme.hover;
  if (named !== 0 && contrast(named, base) >= HOVER_FLOOR) return named;
  return theme.elevated;
}

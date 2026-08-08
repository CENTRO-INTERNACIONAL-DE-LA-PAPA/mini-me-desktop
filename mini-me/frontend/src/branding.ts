/**
 * Central branding config for Mini-Me forks.
 *
 * Everything brand-facing — app name, tagline, logo, and the "About" copy —
 * lives here so a fork can re-brand from a single file. Change `appName` once
 * and it propagates to the top bar, the sign-in gate, the About modal, and the
 * browser tab title.
 *
 * Colors (accents, the logo gradient, and the background animation) are CSS
 * tokens, NOT set here — edit the `:root` / `.dark` blocks at the top of
 * `styles.css` (`--accent`, `--berry`, …). The background *design* lives in
 * `components/Background.tsx`. See the README "Restyle the UI" section.
 */

export interface Branding {
  /** Wordmark shown in the top bar, sign-in gate, About modal, and tab title. */
  appName: string;
  /** One-line subtitle under the wordmark and on the sign-in gate. */
  tagline: string;
  /**
   * Optional logo image. When `null`, the gradient `.brand-mark` (driven by the
   * `--accent` / `--moss` CSS tokens) is rendered. Set `{ src }` — e.g. a file in
   * `frontend/public/` referenced as `/logo.svg` — to render an `<img>` instead.
   */
  logo: { src: string; alt?: string } | null;
  /** Copy for the About modal. */
  about: {
    /** Intro paragraph. */
    lede: string;
    /** Closing attribution line (your institution / funder). */
    attribution: string;
  };
}

const appName = "Mini-Me";

export const branding: Branding = {
  appName,
  tagline: "Research acceleration workbench",
  logo: null,
  about: {
    lede: `${appName} is a multi-agent research workbench for scientists. A coordinator agent delegates work to specialized subagents that find literature, explore data catalogs, clean and analyze tabular data, build predictive models, and turn findings into publication-ready reports.`,
    attribution:
      "A work produced by the International Potato Center, Area of Work 3 (AoW3) under the CGIAR Initiative on Digital Transformation.",
  },
};

/**
 * Ambient background animation.
 *
 * The default is four slow-drifting "blobs" tinted by the `--accent`,
 * `--accent-2`, and `--berry` CSS tokens, so they re-color automatically when
 * you change the palette in `styles.css`.
 *
 * To use a DIFFERENT background design: open `docs/backgrounds.html` in a
 * browser, preview the 15 options, then (1) replace the markup below with the
 * chosen design's inner `<span>`s and (2) paste its CSS + `@keyframes` from the
 * catalog into `styles.css`. The wrapper stays `aria-hidden` — it's decorative.
 */
export function Background() {
  // The unified shell uses a single quiet brand tint painted via `body::before`
  // in styles.css, so the drifting ambient blobs are retired. The component and
  // its export are kept so existing imports continue to work.
  return null;
}

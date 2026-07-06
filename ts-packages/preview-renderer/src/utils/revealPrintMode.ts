/**
 * Force a rendered reveal.js deck into its paginated print layout
 * (issue #315, bd-vhdknrvl).
 *
 * reveal.js selects its multi-page PDF layout when the resolved config
 * has `view === "print"` — which is exactly what navigating to
 * `?print-pdf` sets internally. The printable deck is opened from a
 * `blob:` URL, whose query string is not reliably surfaced via
 * `window.location.search`, so the `?print-pdf` trigger is unavailable.
 *
 * Instead we inject `view:"print"` at the front of the deck's own
 * `Reveal.initialize({…})` config object (our generated code, emitted
 * by `crates/quarto-core/src/revealjs/assemble.rs`). Prepending makes
 * it the first key; a deck never emits its own `view`, so there is
 * nothing to conflict with.
 *
 * No-op (returns the input unchanged) when the HTML contains no
 * `Reveal.initialize({` call — i.e. it isn't a reveal deck — so it is
 * safe to call unconditionally.
 */
export function forceRevealPrintMode(html: string): string {
  const MARKER = 'Reveal.initialize({';
  if (!html.includes(MARKER)) return html;
  return html.split(MARKER).join(`${MARKER}view:"print",`);
}

/**
 * Append a conservative print stylesheet to a printable **document**
 * (issue #315, bd-vhdknrvl).
 *
 * `render_printable` renders through q2's HTML pipeline, whose template
 * inlines only the theme's `@media print` rules (mostly hiding website
 * nav chrome). It does **not** inline pandoc's default print partial,
 * so the standalone document lacks the everyday print-quality basics:
 * heading break-avoidance, orphan/widow control, keeping figures/code/
 * tables from splitting across pages, and sensible page margins. We add
 * them here so ⌘P produces a clean multi-page result.
 *
 * Documents only — reveal decks ship their own precise per-slide print
 * CSS (`reveal.css`), which this must not perturb; the caller applies
 * this to the non-slides branch.
 *
 * Rules are marked `!important` sparingly (only the background reset,
 * which fights themes that paint a dark page) and scoped to
 * `@media print` so screen rendering is untouched.
 */
const PRINT_CSS = `
@media print {
  @page { margin: 1.6cm; }
  html, body { background: #fff !important; }
  h1, h2, h3, h4, h5, h6 {
    break-after: avoid-page;
    page-break-after: avoid;
  }
  p, li, blockquote { orphans: 3; widows: 3; }
  pre, figure, table, img, .cell-output {
    break-inside: avoid;
    page-break-inside: avoid;
  }
}
`;

const STYLE_TAG = `<style data-q2-print>${PRINT_CSS}</style>`;

/**
 * Insert the print stylesheet immediately before `</head>` (so it wins
 * over earlier theme rules of equal specificity). Falls back to
 * prepending the `<style>` when the document has no `</head>`.
 */
export function injectPrintStylesheet(html: string): string {
  const idx = html.indexOf('</head>');
  if (idx === -1) return STYLE_TAG + html;
  return html.slice(0, idx) + STYLE_TAG + html.slice(idx);
}

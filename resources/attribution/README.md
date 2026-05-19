# Attribution Viewer Resources

Shared viewer CSS/JS for the per-node authorship attribution feature.

## Contents

- `viewer.css` — dotted underline on `[data-attr-actor]` plus the
  `.q2-attr-badge` / `.q2-attr-badge-dot` / `.q2-attr-badge-time` classes
  used for the hover badge.
- `viewer.js` — `mouseover` / `mouseout` listeners that build a floating
  badge from the per-element `data-attr-*` attributes.

## Consumers

- `quarto-core`'s `AttributionViewerTransform` reads both files via
  `include_str!` at compile time and injects them into
  `rendered.includes.{header,after-body}` whenever attribution is
  active for an HTML render (unless suppressed by YAML
  `attribution: { source: git, viewer: false }`).
- The hub-client imports `viewer.css` via Vite's `?raw` mechanism and
  injects it through `framework/attribution.tsx`'s `attributionStyles`
  export.

Edit either file in this directory; both surfaces will re-pick it up.
The class names form the stable contract between the CLI's static HTML
output and the hub-client's React preview — keep them in sync.

The JS file is intentionally CLI-only. The hub-client mounts hover
handlers through React props on component boundaries, which is a
different event-handling model from the raw DOM listeners shipped here.

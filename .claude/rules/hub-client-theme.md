# hub-client theming

## Avoid transparency in theme colors

When adding or changing colors in `hub-client` (theme.css tokens or
component CSS), use **solid colors** — no `rgba()`/alpha channels, no
`color-mix(..., transparent)`, no `opacity` used as a color tool.

**Why.** Translucent surface colors composite with whatever is behind
them, so the same token renders differently depending on how many
layers paint it. This caused real bugs: the sidebar once stacked a 4%
alpha tint two-to-three deep (column × sections × headers) and could
not be made to match the single-layer gutter next to it; the mismatch
took several rounds to hunt down (2026-08-28 session).

**Instead:**
- Pick the final color directly, or derive it with an opaque
  `color-mix()` toward the surface it sits on, e.g.
  `color-mix(in srgb, var(--border-color) 50%, var(--editor-bg))`.
- Legacy alpha tokens (`--sidebar-bg` in light mode, the
  `--input-bg-alpha` family, `--alpha-*`) still exist; don't extend the
  pattern, and prefer flattening them to solids when touching the areas
  they paint. (Deliberate exceptions: shadows, scrims/backdrops, and
  focus rings, where seeing through is the point.)

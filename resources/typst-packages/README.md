# Typst Packages & Fonts

This directory contains the Typst packages and Font Awesome font files
Quarto 1 bundles for its `typst` output format. These files are copied from
the TypeScript Quarto repository (`quarto-cli`) at the pinned tag and
maintained locally (pandoc-hybrid-typst Phase 1).

## Contents

- `packages/preview/` - the 5 vendored Typst packages, each at its own
  pinned version, in the exact `preview/<name>/<version>/` layout a real
  Typst package cache uses:
  - `fontawesome/0.5.0/` - Font Awesome icon set for Typst
  - `marginalia/0.3.1/` - margin notes (`definitions.typ` imports this
    **unconditionally**, so it must always be staged regardless of whether
    a document uses margin notes)
  - `octique/0.1.1/` - GitHub Octicons icon set
  - `showybox/2.0.4/` - styled call-out/box component
  - `theorion/0.4.1/` - theorem/proof environments
- `fonts/` - the 3 embedded Font Awesome font files plus their license:
  `Font Awesome 6 Free-Solid-900.otf`, `Font Awesome 6 Free-Regular-400.otf`,
  `Font Awesome 6 Brands-Regular-400.otf`, `LICENSE.txt`

## Source

These files are copied from:
- quarto-cli tag: `v1.11.3`
- `src/resources/formats/typst/packages/` and
  `src/resources/formats/typst/fonts/`

## Updating

To re-vendor these files when quarto-cli updates:

```bash
# From repository root, with quarto-cli checked out at external-sources/quarto-cli
# (or any clone) at the desired tag
cd /path/to/quarto-cli
git archive <tag> -- src/resources/formats/typst/packages src/resources/formats/typst/fonts | \
  tar -x -C /path/to/q2/resources/typst-packages --strip-components=4
```

`--strip-components=4` drops the archive's `src/resources/formats/typst/`
prefix, landing `packages/` and `fonts/` directly under this directory.

## Why Local Copy?

Same reasoning as `resources/scss/README.md` and
`resources/pandoc-filters/README.md`: build reproducibility (no
`external-sources/` checkout required), embedding at compile time via
`include_dir!`, version control, and CI/CD compatibility without a
`quarto-cli` checkout. See the `external-sources-in-macro` lint rule
(`CLAUDE.md`).

## Staging at render time (Phase 2)

Vendoring here is **bytes only** — how these packages and fonts reach the
`typst` compiler's package cache and font path at render time is Phase 2's
concern (`claude-notes/plans/2026-09-18-pandoc-hybrid-typst.md`), not this
directory's. In particular: do **not** stage the 5 packages via
`typst-gather`'s `Config.local` — its `gather_local()` writes to a
`local/{name}/{version}` destination that a `@preview/...` import in
generated `.typ` source will never resolve against. Pre-seed (or copy
directly) under a `preview/{name}/{version}/` cache destination instead,
matching the layout already used here.

## Licenses

Each package carries its own license, vendored alongside it:

- `fontawesome/0.5.0/LICENSE`
- `marginalia/0.3.1/UNLICENSE`
- `octique/0.1.1/LICENSE`
- `showybox/2.0.4/LICENSE`
- `theorion/0.4.1/LICENSE`

The Font Awesome fonts' license is `fonts/LICENSE.txt`.

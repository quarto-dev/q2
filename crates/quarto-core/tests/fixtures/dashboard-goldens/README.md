# Dashboard compatibility corpus

Phase 0 of the dashboard port (bd-dashboard-format-k21hd4vy; plan
`claude-notes/plans/2026-11-03-dashboard-format-port.md`). Q2's
`format: dashboard` output is compared against Quarto 1's, fixture by fixture,
as *dashboard skeletons*: an outline of the significant elements (grid
containers and templates, `html-fill-*` chain, bslib cards, value boxes,
sidebars, toolbars, page tabs, navbar) that ignores theme CSS, scripts and card
content. The extractor and its rules are in
`crates/quarto-output-extract/src/dashboard.rs`.

## Layout

| Path | What |
|---|---|
| `acceptance/` | Real-world dashboards from quarto-web (`docs/dashboards/_examples/`): `gapminder.qmd`, `navigation.qmd`, `valuebox.qmd`, copied unchanged. `penguins.png` (the navigation logo) is an 8×8 placeholder, not the 1.2 MB original: the skeleton records the logo element, never its pixels. |
| `contract/` | Engine-free fixtures written for the port, one per contract item (`# contract:` comment in each front matter). Cells are written as pre-executed `::: {.cell}` divs with `cell-output-*` children, the shape engines emit, so they run in CI without R or Python. |
| `q1/` | Quarto 1's own dashboard fixtures (`tests/docs/smoke-all/dashboard/`), copied unchanged except that `_content/` became `content/`. Mostly need R (`knitr`). Their `_quarto: tests:` blocks are inert here (this directory is not a smoke-all root). |
| `regressions/` | Quarto 1 regression fixtures for dashboard issues (`tests/docs/smoke-all/<date>/…`), copied unchanged. |
| `manifest.toml` | Every fixture, with the engines it needs, its status and a note. Schema: `crates/quarto-output-extract/src/dashboard_corpus.rs`. |
| `goldens/*.q1.skeleton` | Quarto 1 skeletons, written by `cargo xtask capture-dashboard-goldens`. |
| `goldens/*.q2.skeleton` | Expected Q2 skeletons for `status = "divergence"` fixtures, written by hand when the divergence lands. |
| `goldens/q1-provenance.txt` | The Quarto 1 binary the last capture used. |

## Provenance

Per the External Sources Policy, `q1/` and `regressions/` are one-time copies;
nothing reads `external-sources/` at test or capture time. They were copied on
2026-11-03 from quarto-cli commit `d3263862a1bcc02ce84a1b727d5057baf0e30c2c`
(2026-11-03, `git describe`: `v1.11.4-233-gd3263862a`). The goldens were
captured with a Quarto 1 dev build of the same commit (`quarto --version`
reports `99.9.9`); `goldens/q1-provenance.txt` records it.

`acceptance/` was copied the same day from quarto-web commit
`f63c190d28e55beff21c5f010123ffd6756a160c`.

The `github` nav button (`navigation.qmd`) resolves its link from the
repository's GitHub remote. Captures and the assertion test both render in a
scratch directory with no git remote, so neither side emits it.

Not copied: `_extensions/dragonstyle`, a local extension defining `lipsum` and
`unsplash` shortcodes. Four `q1/` fixtures call `lipsum`; without the extension
it resolves to the `lipsum` built into both Quarto 1 and Q2, and the goldens
were captured that way.

## Workflow

- **Assertion:** `cargo nextest run -p quarto-core --test integration -E 'test(dashboard_goldens::)'`.
  `pending` and `deferred` fixtures are skipped; `match` / `divergence` ones are
  rendered and compared.
- **Landing a phase:** flip its fixtures from `pending` to `match` (or to
  `divergence` with a `.q2.skeleton` and a note naming the plan decision).
- **Recapture:** `cargo xtask capture-dashboard-goldens [--quarto <bin>] [--only <substr>]`.
  Needs R (knitr, ggplot2, bslib, gt, palmerpenguins) and Python (pandas,
  matplotlib) for the engine fixtures. A fixture that Quarto 1 cannot render
  is reported; mark it `q1_golden = false` with a reason.
- **Adding a fixture:** add the file, add its manifest entry, capture its golden.
  The harness fails if the manifest and the tree disagree.

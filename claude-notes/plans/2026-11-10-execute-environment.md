---
title: "Execute environment variables and QUARTO_EXECUTE_INFO (bd-8fmuyh4d, GH #1131)"
---

## Overview

Give code cells the data Quarto 1 gives them: `QUARTO_PROJECT_ROOT`,
`QUARTO_DOCUMENT_PATH`, `QUARTO_DOCUMENT_FILE`, `QUARTO_EXECUTE_INFO` (path to
a JSON file holding `document-path` and the resolved six-bin `format`), and
`QUARTO_FIG_WIDTH`/`QUARTO_FIG_HEIGHT`/`QUARTO_FIG_DPI`/`QUARTO_FIG_FORMAT`.
The goal is full Quarto 1 compatibility through a temporary file, while q2
never mutates its own process environment.

Triage: `claude-notes/issue-reports/1131/triage.md`.

## Decisions (maintainer, 2026-11-10)

- Use a temporary file, the Quarto 1 contract, for full compatibility.
- Keep the `language` bin (Quarto 1 shape).
- `format.render.brand.dark` is present whenever the brand defines dark
  values, not only when dark mode is enabled for the document.
- Set `QUARTO_FIG_*` in the same change.
- Measure the file size on the `claude-notes` website (a temporary knitr
  probe document) to judge the cost.

## Design

- **One builder, pure.** `engine/execute_info.rs` partitions the lowered merged
  metadata into Quarto 1's six bins. It ports `metadataAsFormat`, including
  the key lists in `ts-packages/quarto-api/src/config/index.ts`; a test keeps
  the two lists in sync. The `language` bin comes from the resolved
  `quarto.language` table (removed from the `metadata` bin, so it is not
  written twice). The `execute` bin gets Quarto 1's base visibility defaults
  and the fig defaults the engines use. `render.brand` holds
  `{light, dark}` halves as `{data, brandDir}`.
- **Written once per document**, in `EngineExecutionStage`, into the stage
  temp dir, only when an engine other than `markdown` runs.
- **Delivered per engine, never with `set_var`:**
  - knitr: `Command::envs` on the per-document R process.
  - Jupyter: kernels are shared across documents, so a silent
    (`store_history = false`) setup request per document sets the variables
    inside the kernel, for Python, R and Julia kernels.
  - TS engines (julia, marimo): out of scope here; their Node host is
    long-lived, so this needs a protocol field. Follow-up strand.

## Checklist

### Phase 1: tests first

- [x] Unit: six-bin partition (stage 1 peel, stage 2 classify, `execute: false`, `quarto.language` → `language`)
- [x] Unit: key lists match `ts-packages/quarto-api/src/config/index.ts`
- [x] Unit: fig env (defaults, document overrides, `retina` → png with double dpi)
- [x] Unit: path variables (project and single-file)
- [x] Unit: brand halves (dark present iff the brand defines dark values)
- [x] Unit: per-language Jupyter setup snippets (escaping, unknown language)
- [x] End-to-end (R present): knitr cell sees all variables and a readable file (`crates/quarto/tests/integration/execute_env_cli_e2e.rs`)
- [x] End-to-end (python3 kernel present): Jupyter cell sees them; second document gets its own `QUARTO_DOCUMENT_FILE`

### Phase 2: implementation

- [x] `engine/execute_info.rs` builder
- [x] Brand variants with the dark-mode flag
- [x] `ExecutionContext.execute_env` + file write in `EngineExecutionStage`
- [x] knitr wiring
- [x] Jupyter setup request
- [x] Docs page
- [x] Follow-up strand: TS engines (bd-x3h4zjhl)

### Phase 3: measurement and verification

- [x] Size on `claude-notes` (knitr probe; probe not committed)
- [x] `q2 render` end-to-end for knitr and Jupyter, output inspected
- [x] `cargo nextest run -p quarto-core -p quarto-sass -p quarto-brand -p quarto`, clippy, WASM build (Windows cross-check not possible: C build scripts)

## Notes

- bd-i7yz08uh (found here): parallel project renders interleave cells of
  different documents on one shared Jupyter kernel. Per-document variables
  inherit that race, like all kernel state, until it is fixed.

## Results

### Size and cost of the execute-info file

Measured with `QUARTO_PERF_STATS=1` (gauge `perf.execute-info`), a
temporary knitr probe document (not committed), and a debug build.

| Project | Bytes | Build | Build + write | Largest parts |
|---------|------:|------:|--------------:|---------------|
| Lone document (GH #1131 repro) | 5,169 | n/a | n/a | `language` 4,824 |
| `claude-notes` website | 6,359 | 89 µs | 242 µs | `language` 4,585; `metadata.project` 1,070 |
| `docs` website | 19,406 | 143 µs | 349 µs | `metadata.website` 13,967 (navbar, sidebar); `language` 4,585 |
| Quarto 1, lone document | 6,513 | | | `language` 4,935 |

The cost is under 0.4 ms per document that executes code, against an
engine start that takes hundreds of milliseconds (R or a kernel).
Documents that run no code write nothing. The largest growth is a big
`website:` configuration, which Quarto 1 also copies into `metadata`.

### End-to-end checks

- `q2 render` of the GH #1131 repro (Python, Jupyter): all four variables
  set; the file has the six bins.
- The same with `brand: _brand.yml` and a light/dark brand:
  `format.render.brand.light.data.color.background` is `#ffffff`, `dark`
  is `#1b1b1b`; `QUARTO_FIG_*` are `7`, `5`, `96`, `png`.
- `execute_env_cli_e2e`: knitr in a subdirectory (paths, fig override,
  brand, language bin); two Jupyter documents on one shared kernel each see
  their own `QUARTO_DOCUMENT_FILE`.

## Found while implementing

- bd-i7yz08uh (P1): parallel project renders interleave cells of different
  documents on one shared Jupyter kernel. Per-document variables share the
  race until it is fixed; the e2e test runs with `QUARTO_JOBS=1`.
- bd-08hfh3ut (P1): a project-level `brand:` resolves against the document
  directory once code runs, for a document in a subdirectory. The theme stage
  fails; the execute-info file is unaffected (it resolves before the engine).
- bd-x3h4zjhl: TS engines (julia, marimo) do not receive the variables yet.

## Divergences from Quarto 1 (documented in `docs/guides/authoring/execution-context.qmd`)

- `brand.dark` is present whenever the brand defines dark values (decision
  above). Quarto 1 also adds it when the document turns dark mode on for a
  brand without dark values; that entry repeated the light one.
- Each brand mode has `data` and `brandDir`; Quarto 1 serializes its whole
  `Brand` object (`processedData` and others).
- `QUARTO_FIG_*` report q2's figure defaults (`png`, 96 dpi for HTML), not
  Quarto 1's `retina` (192 dpi).
- The `execute` bin holds the visibility and figure defaults only, not every
  per-format default Quarto 1 merges.

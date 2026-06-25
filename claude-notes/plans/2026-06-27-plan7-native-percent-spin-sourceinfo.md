# Plan 7 — native percent/spin script conversion + precise SourceInfo

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Date:** 2026-06-27
**Status:** PLAN — design backing is finalized (see References); work items below are
unstarted. Additive, post-Plan-4; **not on the critical path**.
**Depends on:** **Plan 1c** (the `claims_file` → `markdown_for_file` trait surface +
`EngineClaimsFileStage` Pass-1 wiring), **Plan 0** (SourceInfo `Concat`/`Original` infra),
**Plan 3** (the `@quarto/api/jupyter` percent helpers `isPercentScript` /
`percentScriptToMarkdown`, for the TS-engine path), and the **finalized SourceInfo design**
`2025-12-15-source-info-for-structured-formats.md`. **Does NOT depend on Plan 5 or Plan 6.**
**Sequence:** independent of Plans 5/6; can start any time after Plan 1c. Positioned after
Plan 4 in the default sequence (`4 → 5 → 6 → 7`), but pullable earlier if the non-`.qmd`
input capability or the provenance-correctness becomes higher priority than the (self-gated)
pooling/Pass-1 stubs.

## Overview

This plan does the one thing the epic has only ever **deferred in pieces**: implement
**percent scripts** (`.py`/`.jl`/`.r` with `# %%` markers) and **R spin scripts** (`.R`
with `#'`/`#+`) **natively in Rust**, wired into the appropriate built-in engines through
the epic's `claims_file`/`markdown_for_file` interfaces — and do it with **precise
`SourceInfo` (including column offsets) for every supported language**.

It supersedes a weaker assumption baked into the epic. Plan 0 currently records
(`2026-04-18-plan0-include-expansion-and-source-info.md` §"Percent scripts: engine-side,
not q2-side", L505-508): *"Source mapping for the conversion step is the engine's
responsibility (Quarto 1 doesn't do it either — percent script conversion **loses
provenance**, producing an identity-mapped MappedString with no filename)."* **Plan 7
overrides that:** q2 *can* and *should* preserve precise provenance, and the finalized
design (`2025-12-15`) shows it costs no new infrastructure for plain-text scripts (it
reuses `Concat`/`Original`). The scattered deferrals — 1c "Future Work", the knitr-engine
plan's "spin files: Not critical for MVP", Plan 0's engine-side framing — converge here.

**Why a dedicated plan:** this is the one place the "**carry the whole Q1 engine surface**"
mandate (`designs/engine-api-surface.md` § Governing principle) lands as real capability +
correctness rather than infrastructure. Built-in jupyter/knitr currently implement **no**
`claims_file`/`markdown_for_file` (Plan 0 L511); a real user with a `.py`/`.jl`/`.R` input
file cannot render it. And the validation target (`.qmd`) masks the whole surface.

## Two conversion paths (this plan owns both)

1. **Built-in engines → native Rust converters.** jupyter (`.py`/`.jl` percent),
   knitr (`.R` spin) run in-process; no Deno. They need the Rust converter + the
   `2025-12-15` SourceInfo. **This is the bulk of the plan.**
2. **TS-extension engines → wire `source_map` precision.** Julia/marimo convert Deno-side
   via `quarto.jupyter.percentScriptToMarkdown` (Plan 3); the conversion provenance crosses
   the protocol as `source_map`. Today 1c/plan1a-engine scope `markdown_for_file` to **"C′"**
   (converted-buffer provenance, ephemeral FileId) and defer **"A′"** (faithful remap back to
   the original `.py`/`.jl` bytes/columns). Plan 7 delivers A′ — the precise remap — so a TS
   engine's percent-script errors point at the original file with columns, matching the
   built-in path.

## Work Items

### Phase 7A — built-in jupyter percent conversion (`.py`/`.jl`)
- [ ] Implement `JupyterEngine::claims_file(".py"/".jl") → true` (content-inspecting:
  `# %%` markers) and `valid_extensions`, per the resolution model (note the static
  `claims-files:` content-inspection caveat from the 1c review — a content claim must be
  marked must-load, not a bare extension list).
- [ ] Implement `JupyterEngine::markdown_for_file` — a Rust port of Q1's
  `markdownFromJupyterPercentScript`: strip `# %%` cell markers, strip the `# ` prefix from
  markdown lines, wrap code in `{python}`/`{julia}` fences.
- [ ] Tests: convert a `.py`/`.jl` percent fixture → qmd; assert structure + that the Pass-1
  `EngineClaimsFileStage` routes it (no `.qmd` regression).

### Phase 7B — built-in knitr spin conversion (`.R`)
- [ ] Implement `KnitrEngine::claims_file(".r"/".R") → true` (spin-script detection) and
  `markdown_for_file` (port of `knitr::spin`'s `#'` markdown / `#+` chunk-option handling —
  via the R subprocess if a pure-Rust port is out of scope; decide during impl).
- [ ] Tests: convert an `.R` spin fixture → qmd; assert chunk options survive.

### Phase 7C — precise SourceInfo (implements the `2025-12-15` design)
- [ ] **Plain-text scripts (percent/spin):** build a `Concat` of **per-line `Original`
  pieces** that skip the stripped prefix (e.g. `# `, 2 bytes), so columns map exactly back
  into the original `.py`/`.jl`/`.R` (design §"Column Precision with Concat"). Content is
  verbatim post-prefix, so column mapping is a constant per-line shift — no intra-line
  transform (see "Scope" below).
- [ ] **ipynb:** add the first-class `SourceInfo::NotebookCell { notebook_path, cell_index,
  cell_id, cell_type, content_file_id, start_offset, end_offset }` variant + ephemeral
  cell-content files in `SourceContext`; error display renders `notebook.ipynb [cell 3,
  markdown]:2:5` (design §"New SourceInfo Variant" / §"Error Display Flow").
- [ ] **Sidecar storage:** the converted qmd's mapping lives in `.quarto/source-maps/` (the
  unified envelope: `plain_text` / `jupyter_notebook`), since the converted qmd is plain text
  on disk with nowhere to store mapping inline (design §"Sidecar File Format"); wire the
  `SourceContext.sourcemap_paths` field + staleness detection.
- [ ] Tests: an error in a percent-script markdown comment and in a code cell both report the
  **original file, line, and column**; an ipynb error reports cell coordinates.

### Phase 7D — TS-engine A′ remap (Julia/marimo parity)
- [ ] Replace the deferred "C′" converted-buffer provenance with the **A′ faithful remap**
  (plan1a-engine SEAM-3): the wire `source_map` from `markdown_for_file` maps converted-qmd
  positions back to the original non-qmd file, so TS-engine percent-script errors match the
  built-in column precision.
- [ ] Tests: a Julia `.jl` percent-script error reports the original `.jl` line+column.

### Phase 7E — Julia `.jl` validation in Plan 4
- [ ] Add to **Plan 4** (`julia-validation`) the now-removed exclusion: a `.jl` percent-script
  fixture that the Julia engine **claims** (`isPercentScript([".jl"])`), converts, renders,
  and whose error provenance is asserted. (Plan 4 currently states *"Julia engine claims by
  language only; no `claims_file` for `.jl` percent scripts in v1"* — L196-206; flip that.)
  Split by precision level: **functional + C′** can land with Plan 4 alone; the **precise A′**
  assertion lands with Phase 7D.

### Phase 7F — reconcile the scattered deferrals
- [ ] Point 1c "Future Work: Built-in engine percent/spin", the knitr-engine plan's deferred
  "spin files", and **Plan 0's "loses provenance" framing** at this plan; update Plan 0 §
  "Percent scripts" to record that provenance is now preserved (Plan 7), superseding the
  Q1-parity-loss note.

## Scope / boundaries

- **Column fidelity is exact through *conversion*** (constant per-line prefix shift, content
  verbatim). The harder **intra-line** column problem (escaping, delimiter canonicalization
  *within* a line) only arises if converted content is **re-serialized through the qmd
  writer** — and that is **not this plan's problem**: it is the deferred general problem in
  `2026-06-18-qmd-per-line-provenance.md` (the writer/output-serialization provenance plan,
  block-editing branch), which is *complementary* to this one. Plan 7 is the **input/converter**
  half; 2026-06-18 is the **output/writer** half. They compose at the engine `SourceInfo`; a
  truly general column mechanism would live in the writer and subsume 12-15's prefix case.
- **Out of scope:** LSP integration; sidecar caching/cleanup (design §"Out of Scope").

## References
- **Finalized SourceInfo design (the algorithm + column technique + `NotebookCell` + sidecar):**
  `claude-notes/plans/2025-12-15-source-info-for-structured-formats.md` (status "Design finalized").
- Earlier architecture proposal (the registry approach 1c declined; historical):
  `claude-notes/surface-syntax-converter-design.md`.
- The scattered deferrals this plan consolidates: Plan 1c "Future Work: Built-in engine
  percent/spin script support"; `2026-01-07-knitr-engine-implementation.md` (spin deferral);
  `2026-04-18-plan0-include-expansion-and-source-info.md` §"Percent scripts: engine-side".
- The governing mandate: `claude-notes/designs/engine-api-surface.md` § "Governing principle —
  the validation target is not the scope boundary".
- Complementary output-side provenance: `claude-notes/plans/2026-06-18-qmd-per-line-provenance.md`.
- Q1 source: `markdownFromJupyterPercentScript` / `percentScriptToMarkdown`; `knitr::spin`.

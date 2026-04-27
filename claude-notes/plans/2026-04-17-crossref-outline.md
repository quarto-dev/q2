# LSP Outline: Include Cross-Referenceable Elements

Beads: `bd-ascs`

## Overview

Make cross-referenceable elements (`{#fig-…}`, `{#thm-…}`, `{#tbl-…}`, `{#lst-…}`, etc.) appear as entries in the document outline used by hub-client and `quarto lsp`. A secondary consequence: suppress header entries whose semantic role is absorbed by a surrounding crossref target (e.g. the `## Line` inside `::: {#thm-line}` should not be a standalone outline item).

## Motivating example

```qmd
---
title: crossrefs
---

::: {#fig-1}

This is the payload for the figure.

This is the caption for the figure.

:::


::: {#thm-line}

## Line

The equation of any straight line, called a linear equation, can be written as:

$$
y = mx + b
$$

:::
```

**Current outline:**
- `Line`

**Desired outline (tentative):**
- `fig-1` — detail: `Figure 1: This is the caption for the figure.`
- `thm-line` — detail: `Theorem 1: Line`

The `## Line` header is *not* a separate entry: theorem sugaring moves it into the theorem's `title` slot, so walking the post-sugar AST naturally excludes it.

## Design: run the existing pipeline up to a pruned prefix

The render pipeline (`crates/quarto-core/src/pipeline.rs:133-145`) is already staged such that a **prefix** leaves the AST in an outline-ready state.

Full HTML pipeline:
```
1. ParseDocumentStage         (pampa → Pandoc AST)
2. MetadataMergeStage         (merges project/dir/doc/runtime metadata in memory)
3. PreEngineSugaringStage     (seeds RefTypeRegistry from crossref.custom, desugars shorthand)
4. EngineExecutionStage       (Jupyter / knitr)          ← SKIP for LSP
5. CompileThemeCssStage       (SCSS → CSS)               ← SKIP
6. UserFiltersStage::pre()    (user Lua filters)         ← SKIP
7. AstTransformsStage         (Callout, Theorem, Proof, FloatRefTarget, CrossrefIndex, CrossrefResolve, …)
8. UserFiltersStage::post()   (user Lua filters)         ← SKIP
9. RenderHtmlBodyStage        (pandoc subprocess)        ← SKIP
10. ApplyTemplateStage                                   ← SKIP
```

**Outline-ready prefix**: stages 1, 2, 3, 7. After stage 7, `CustomNode("FloatRefTarget")` and `CustomNode("Theorem")` are populated with `plain_data.order`, and `@fig-1` citations are already rewritten. The walker operates on that AST.

There's precedent for pruned pipelines — `build_wasm_html_pipeline()` at `pipeline.rs:198` already skips engine execution.

### MetadataMergeStage does not do filesystem I/O at pipeline time

`MetadataMergeStage` merges values out of `ProjectContext.config.metadata`, directory `_metadata.yml` layers (loaded via the `SystemRuntime` trait / VFS, not real FS), document frontmatter, and runtime metadata. In WASM the VFS is populated by JS before the pipeline runs. The merge itself is in-memory dict ops; the cost was paid upstream when `ProjectContext` was built.

So the constraint on our outline is not "can we avoid I/O inside the pipeline" but "what does the caller thread into `ProjectContext` before running it." Today the hub-client constructs a *minimal, single-file* `ProjectContext` with default (empty) `ProjectConfig` at `wasm-quarto-hub-client/src/lib.rs:558`. That means:

- Custom crossref types defined in a project's `_quarto.yml` (e.g. `crossref.custom: [{type: foo, prefix: "Fooref"}]`) **are already not applied in hub-client's render path**, independent of this work.
- Our outline inherits that same limitation for free — no new gap created.
- **Separate follow-up issue needed**: thread project `_quarto.yml` from the Automerge doc set into `ProjectContext.config.metadata`. Once done, both render and outline pick up custom types.

For native `quarto lsp` (future): load project config once at startup, watch `_quarto.yml` / `_metadata.yml`, rebuild `ProjectContext` on change, keep it alive across outline recomputes. Standard workspace-scan pattern. Not on this plan's critical path.

### Crate dependency shift

`quarto-lsp-core` currently depends on `quarto-analysis` only. Running the pipeline means adding a dependency on `quarto-core`. The WASM bundle already pulls in `quarto-core` (via `wasm-quarto-hub-client`), so hub-client bundle size is unaffected. A future standalone `quarto lsp` native binary pays the cost of pulling in sass, doctemplate, etc., but those are already transitive deps of the `quarto` CLI binary too — no new ecosystem weight.

## Open design questions

### Q1 — Numbering source

- **(I) Run the full prefix** (stages 1, 2, 3, 7). `CrossrefIndexTransform` runs in stage 7; `plain_data.order` carries correct section-scoped numbers. Label reads `Figure 2.1`, `Theorem 2.3`, etc., matching the rendered document.
- **(II) Ad-hoc scan-order numbering** in the walker. Simpler but numbers will diverge from render once sections are present.
- **(III) Identifier-only** — show `fig-1`, `thm-line`; defer label formatting to a follow-up.

**Recommendation: (I).** Running the pipeline is the whole point of the design above; ad-hoc numbering defeats it.

### Q2 — Display format and `SymbolKind`

User's tentative format: `fig-1 (Figure 1: This is …)`. Mapping options onto `Symbol { name, detail, kind }`:

- **(a) Name = identifier, Detail = label.** `name: "fig-1"`, `detail: "Figure 1: This is the caption…"`. LSP-conventional; identifiers are what go-to-symbol / Ctrl-click target.
- **(b) Name = label, Detail = identifier.** Reads more naturally in a flat list; worse for quick lookup by id.
- **(c) Combined name string.** `name: "fig-1 (Figure 1: …)"`, detail empty.

**Recommendation: (a).**

For `SymbolKind`: `Class` (◇) reads well as "a named, self-contained region." Alternatives: `Struct`, `Namespace`, `Object`. The hub-client `OutlinePanel.tsx::getSymbolIcon` already has an icon for `class`. User sign-off needed.

### Q3 — Scope of ref types for v1

All 12 built-ins (`fig`, `tbl`, `thm`, `lem`, `cor`, `prp`, `cnj`, `def`, `exm`, `exr`, `lst`, `eq`) fall out automatically from the pipeline approach. No per-type opt-in needed.

**Recommendation: all built-ins in v1.** Custom types (via `crossref.custom`) also work as soon as the hub-client `ProjectContext` wiring is fixed — no additional outline code needed.

### Q4 — Headers nested deeper than the title slot

Theorem sugaring absorbs the *first* header into `title`. A second `### Something` further inside a theorem body would still be reached by the walker recursing into the `content` slot.

**Recommendation: skip recursion into `FloatRefTarget` / `Theorem` / `Proof` custom slots entirely.** Nested outline entries inside a figure or theorem don't carry their weight — the target itself is the interesting outline item. User sign-off needed.

## Work plan

### Phase 0 — alignment (BEFORE implementation)

- [x] User confirms Q1: **(I) full-pipeline numbering**
- [x] User picks Q2: **(a) name = identifier, detail = label; SymbolKind = Class**
- [x] User confirms Q3: **all 12 built-in ref types**
- [x] User confirms Q4: **stop recursing into crossref CustomNodes (FloatRefTarget/Theorem/Proof)**

### Phase 1 — tests first (TDD) ✓

Tests live at `crates/quarto-lsp-core/tests/crossref_outline.rs` and
`hub-client/src/services/lspCrossrefOutline.wasm.test.ts`.

- [x] Document with one figure div → one symbol emitted; caption in detail; no stray header
- [x] Document with one theorem div containing `## Line` → one symbol; `Line` does not appear separately
- [x] Document mixing a top-level header, a figure, a theorem → header hierarchy preserved; crossref targets as siblings
- [x] Two figures → numbered `Figure 1`, `Figure 2`
- [x] Malformed id (`{#fig-}`) → no panic, not treated as a crossref target
- [x] Equation block `$$ … $$ {#eq-foo}` → outline entry with id `eq-foo`
- [x] Table with `{#tbl-foo}` → outline entry
- [x] Run tests, verify they fail for the right reason (6/8 failed before implementation)

Deferred to a follow-up check (still worth adding once we iterate):

- [ ] Figure inside a `## Section` where section numbering is on → label reads `Figure 2.1` (matching the render). Section-scoped numbering lands once `CrossrefIndexTransform` picks up the chapters/sections config; tests for that live alongside the render-parity suite.

### Phase 2 — implementation ✓

- [x] Add `build_analysis_pipeline()` + `build_analysis_transform_pipeline()` in `crates/quarto-core/src/pipeline.rs`. Pipeline runs `[Parse, MetadataMerge, PreEngineSugaring, AstTransforms(analysis subset)]`; transform subset is sugaring + crossref-index only (no shortcode Lua, no TOC, no finalization).
- [x] Add `quarto-core`, `quarto-pandoc-types`, `quarto-system-runtime`, `pollster` to `quarto-lsp-core`.
- [x] Rewrite `quarto-lsp-core/src/analysis.rs::analyze_document()` against the pipeline. Sync entry blocks on `analyze_document_async` via `pollster::block_on` (the analysis-subset transforms never suspend, so this is free on both native and WASM).
- [x] Collapse `symbols.rs::get_symbols()` into a thin wrapper.
- [x] Teach the walker to recognize crossref `CustomNode`s (block and inline) via `crossref_target_view` / new `crossref_target_view_inline`. Detail format matches `CrossrefRenderTransform`: `"<Kind> <n>: <caption or title>"`, or `"<Kind> <n>"` when unlabeled, or `<Kind>` alone when unnumbered with no caption.
- [x] Stop recursing into crossref custom nodes (Q4); gate empty-suffix ids out of the outline since they are not navigable targets.
- [x] Add inline Paragraph/Plain walking for equation crossref targets (`Inline::Custom("Equation")`).
- [ ] `OutlinePanel.tsx::getSymbolIcon` — tracked as `bd-w66j`; no change needed for v1 since `SymbolKind::Class` already has an icon.

### Phase 3 — verification ✓

- [x] `cargo nextest run --workspace` → 7433 passed, 195 skipped.
- [x] `cargo xtask verify --skip-rust-tests --skip-hub-tests` → all builds green (incl. WASM).
- [x] `npm run test:wasm` → 56 passed (52 existing + 4 new LSP crossref outline tests).
- [ ] Manual smoke test in hub-client browser with the motivating example.
- [ ] `hub-client/changelog.md` entry (two-commit workflow: code → changelog).

### Phase 4 — follow-up issues filed ✓

- [x] `bd-gucj` — hub-client: thread project `_quarto.yml` into `ProjectContext`.
- [x] `bd-t9zb` — share crossref label formatting between `CrossrefRenderTransform` and the LSP outline.
- [x] `bd-yttl` — expose `CrossrefIndex` from LSP `analyze_document` for future features.
- [x] `bd-w66j` — polish `OutlinePanel` icon for crossref targets.
- [ ] Native `quarto lsp` project-load / file-watch story (not yet filed; scoped for when the native LSP binary lands).

## Files that will change

| File | Change |
|---|---|
| `crates/quarto-core/src/pipeline.rs` | Add `build_analysis_pipeline()` |
| `crates/quarto-lsp-core/Cargo.toml` | Add `quarto-core` dep |
| `crates/quarto-lsp-core/src/analysis.rs` | Drive the analysis pipeline; extend walker for crossref CustomNodes |
| `crates/quarto-lsp-core/src/symbols.rs` | Collapse into thin wrapper around `analyze_document` |
| `crates/quarto-lsp-core/tests/*.rs` | New integration tests |
| `hub-client/src/components/OutlinePanel.tsx` | Icon mapping if new SymbolKind introduced |
| `hub-client/changelog.md` | New entry |

## Out of scope

- Localization of crossref labels (hardcoded English throughout the codebase today).
- Go-to-definition for `@fig-1` references (different LSP feature).
- Hover previews that render caption content.
- Diagnostics for dangling crossrefs.
- Fixing hub-client's missing project-metadata wiring (tracked as a follow-up; independent).

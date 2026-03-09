# Plan: CSS in Pipeline — Part C: Integration & E2E Tests (Phases 5-6)

Parent plan: `claude-notes/plans/2026-03-09-css-in-pipeline.md`
Prerequisite: `claude-notes/plans/2026-03-09-css-in-pipeline-b-migration.md`

This sub-plan adds integration and E2E tests that verify the full theme
inheritance chain works end-to-end, then runs final verification.

## Phase 5: Integration and E2E tests

### Native integration tests (`crates/quarto-core/`)

Using full pipeline with `NativeRuntime` + grass SASS compiler:

- [ ] `test_render_pipeline_theme_from_project` — `_quarto.yml` has
  `format: { html: { theme: darkly } }`, bare `doc.qmd`. Assert CSS artifact
  is NOT `DEFAULT_CSS` and contains darkly-specific values.
- [ ] `test_render_pipeline_theme_from_document_overrides_project` — project
  has `theme: darkly`, document has `theme: flatly`. Assert CSS contains
  flatly values, not darkly.
- [ ] `test_render_pipeline_no_theme_uses_compiled_default` — no theme
  anywhere. Assert CSS is compiled Bootstrap (from `compile_default_css`).

### WASM E2E tests (`hub-client/src/services/`)

New file `themeInheritance.wasm.test.ts` following existing patterns:

- [ ] **Project theme**: `_quarto.yml` has `theme: darkly`, `doc.qmd` has none.
  Assert CSS artifact contains darkly-specific values.
- [ ] **Document overrides project**: `_quarto.yml` has `theme: darkly`,
  `doc.qmd` has `theme: flatly`. Assert CSS contains flatly, not darkly.
- [ ] **Directory metadata theme**: `chapters/_metadata.yml` has `theme: sketchy`,
  `chapters/doc.qmd` has none. Assert CSS contains sketchy.
- [ ] **No theme anywhere**: Assert CSS is default Bootstrap.
- [ ] **Runtime metadata overrides all**: `vfs_set_runtime_metadata` with
  `theme: darkly`, document has `theme: flatly`. Assert CSS contains darkly.

**Detection strategy**: Each Bootswatch theme produces distinctive CSS. Before
writing tests, compile a few themes to identify reliable detection strings
(e.g., darkly uses `$body-bg: #222`, sketchy has hand-drawn borders).

## Phase 6: Verification

- [ ] `cargo nextest run --workspace` — all tests pass
- [ ] `cargo xtask verify` — WASM and hub-client build and test
- [ ] Manual: `theme: darkly` in `_quarto.yml`, verify in hub-client
- [ ] Manual: `theme: sketchy` in frontmatter overrides project theme
- [ ] Manual: native CLI `quarto render` with theme in `_quarto.yml`

## Reference

See parent plan for:
- Cache key correctness and known limitations (Risk 2)
- Custom .scss file resolution in WASM (Risk 3)

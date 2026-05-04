# Plan 1 — q2-preview pipeline + integration

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** M1 (visible q2-preview rendering, read-only)

## Goal

Introduce a new `q2-preview` document format. Documents with `format: q2-preview`
in the YAML frontmatter render in the React-iframe-based preview using the same
transform-and-filter pipeline that the HTML pipeline runs, *minus* the destructive
"resolve-and-flatten" transforms and the HTML rendering stages. The resulting AST
goes through the JSON wrapper-encoding for CustomNodes and is rendered by the
existing `AstIframe` plumbing.

This is the foundation milestone. After Plan 1 lands, Lua filters and shortcodes
are visibly applied in q2-preview's React rendering. CustomNodes (Callout,
Theorem, etc.) reach React as `__quarto_custom_node` wrapper Divs (Plan 2 adds
type-specific React components for them).

Edit-back is **read-only** in v1 — `ReactPreview.tsx`'s `handleSetAst`
early-returns with a console warning for `q2-preview` format. Plan 7 removes
this guard once the writer-side round-trip lands.

## Scope

### In scope

- Format detection: `q2-preview` recognized as a new pseudo-format mapping to HTML
  base. Edit `crates/quarto-core/src/format.rs::builtin_pseudo_format`.
- New pipeline builder `build_q2_preview_pipeline_stages` mirroring
  `build_html_pipeline_stages_with_options` but stopping short of
  `CodeHighlightStage` / `RenderHtmlBodyStage` / `ApplyTemplateStage`.
- New transform pipeline builder `build_q2_preview_transform_pipeline`.
  This is a new pipeline, not a "subtraction" from `build_transform_pipeline`
  — written as a fresh sequence of explicit `pipeline.push(...)` calls listing
  exactly the transforms that run in q2-preview.
- New Pass2Renderer impl `RenderToPreviewAstRenderer` returning a new
  `WasmPassTwoPreviewOutput` (ast JSON instead of HTML).
- New WASM entry point `render_page_in_project_to_preview_ast` mirroring
  `render_page_in_project`'s shape (discover ProjectContext; single-file
  branch via `render_single_doc_to_preview_response`; multi-doc branch via
  `ProjectPipeline` with `RenderToPreviewAstRenderer`).
- TypeScript wrapper `renderPageInProjectToPreviewAst` in
  `hub-client/src/services/wasmRenderer.ts`.
- `ReactPreview.tsx` routes `format === 'q2-preview'` through `AstIframe`
  (alongside the existing `q2-debug` path).
- Read-only guard: `handleSetAst` in `ReactPreview.tsx` no-ops with
  `console.warn` for q2-preview format.

### Out of scope (deferred to other plans)

- React component implementations for CustomNodes (Plan 2).
- Filter idempotence verification (Plan 3).
- Provenance type changes (Plans 4/5/6).
- Edit-back round-trip via `incremental_write_qmd_for_preview` (Plan 7).
- Include round-trip via wrapper CustomNodes (Plan 8).
- q2-slides upgrade (separate future plan; see §Decision A).

## Design decisions (settled in conversation)

- **q2-preview is a distinct format** from q2-debug. q2-debug stays exactly as
  it is (minimal pipeline, raw AST view). Existing demos using `format: q2-debug`
  continue to work unchanged.
- **Read-only in v1**: silent no-op + `console.warn` for `setLocalAst`. Avoids
  source corruption while round-trip work is in progress.
- **q2-preview routes through the existing iframe sandbox** introduced by Elliot
  (commit `72ef918c`). Same `AstIframe` component, same `ast-renderer-entry.tsx`
  iframe entry, same postMessage protocol.
- **The transform pipeline is a new explicit list**, not `build_transform_pipeline`
  with deletions. Concretely, every transform that runs in q2-preview is named
  in `build_q2_preview_transform_pipeline` via `pipeline.push(...)`.
- **`Pass2Renderer` trait already exists** (Carlos, Phase 9 of feature/websites).
  We add a third impl alongside `RenderToFileRenderer` (native) and
  `RenderToHtmlRenderer` (WASM HTML).

## Transform list for q2-preview

Explicitly enumerated in `build_q2_preview_transform_pipeline`, in order:

1. `CalloutTransform` (sugar: Div → CustomNode)
2. `ShortcodeResolveTransform::with_lua_support` (resolves shortcodes via Lua engine)
3. `MetadataNormalizeTransform`
4. `WebsiteTitlePrefixTransform`
5. `WebsiteBootstrapIconsTransform`
6. `WebsiteCanonicalUrlTransform`
7. `SectionizeTransform`
8. `TheoremSugarTransform`
9. `ProofSugarTransform`
10. `FloatRefTargetSugarTransform`
11. `EquationLabelTransform`
12. `CrossrefIndexTransform`
13. `CrossrefResolveTransform`
14. `TocGenerateTransform`
15. `NavbarGenerateTransform`
16. `SidebarGenerateTransform`
17. `PageNavGenerateTransform`
18. `FooterGenerateTransform`
19. `ResourceCollectorTransform`

Excluded (with rationale):

- `CalloutResolveTransform` — preserve CustomNode for React.
- `WebsiteFaviconTransform` — no favicon concept in React preview.
- `TitleBlockTransform` — React layout reads `meta.title` directly (Plan 2).
- `FootnotesTransform` — synthesizes a non-source-bearing container; defer to
  future plan with wrapper-CustomNode round-trip story.
- `TocRenderTransform`, `NavbarRenderTransform`, `SidebarRenderTransform`,
  `PageNavRenderTransform`, `FooterRenderTransform` — produce HTML strings;
  React components consume structured metadata directly (Plan 2 reimplements
  Sidebar body-classes and Navbar brand-fallback in JS).
- `LinkRewriteTransform` — rewrites `.qmd` hrefs to `.html`; wrong target for
  the editor. Editor navigates to `.qmd` directly via `onNavigateToDocument`.
- `AppendixStructureTransform` — synthesizes a non-source-bearing container;
  defer like FootnotesTransform.
- `CrossrefRenderTransform` — preserve CustomNodes (Theorem, Proof,
  FloatRefTarget, Equation, CrossrefResolvedRef) for React.

The exclusions reflect three categories:
- **Preserve CustomNodes** (Callout/Crossref-render): React custom.tsx renders
  type-specific.
- **Synthesize-with-no-preimage** (TitleBlock/Footnotes/Appendix): defer; not
  needed for q2-preview's body rendering.
- **Output-format-specific** (5x render transforms / LinkRewrite / Favicon):
  HTML-pipeline-only outputs.

## Open questions for implementation

- **WasmPassTwoPreviewOutput shape**: parallel to `WasmPassTwoOutput` but with
  `ast_json: String` instead of `html: String`. Source_context, diagnostics,
  page_artifacts the same. Pin in implementation.
- **Single-file flush**: mirror the bd-87fu artifact-flush logic (`lib_dir`
  empty branch calls `flush_site_libs` in-place). Pattern lifted from
  `RenderToHtmlRenderer.render`.
- **AST JSON serialization config**: probably `JsonConfig { include_inline_locations: true }`
  matching today's `parse_qmd_to_ast`. Confirm during implementation.
- **Drift-protection test**: a unit test that asserts the q2-preview transform
  list contains exactly N specific names (the 19 listed above). When a future
  PR adds a transform to `build_transform_pipeline`, this test fails until the
  contributor explicitly classifies it (include or exclude in
  `build_q2_preview_transform_pipeline`). Catches silent inheritance/omission.

## References

- `crates/quarto-core/src/pipeline.rs:170-300` — existing pipeline builders.
- `crates/quarto-core/src/pipeline.rs:677` — `build_transform_pipeline` (don't
  reuse; just reference for transform names).
- `crates/quarto-core/src/project/pass2_renderer.rs` — Pass2Renderer trait
  (line 75) and existing impls.
- `crates/wasm-quarto-hub-client/src/lib.rs:1202` (approximate, post-merge)
  — `render_page_in_project` entry point and its single-file branch
  `render_single_doc_to_response` (~1252).
- `hub-client/src/components/render/ReactRenderer.tsx` — format dispatch,
  `AstIframe` mounting.
- `hub-client/src/components/render/ReactPreview.tsx` — `handleSetAst`
  (where the read-only guard lands).
- `hub-client/src/services/wasmRenderer.ts` — TS wrappers.
- `crates/quarto-core/src/format.rs:106` — `builtin_pseudo_format`.

## Test plan

- **Format detection unit test**: `Format::from_format_string("q2-preview")`
  returns a Format with HTML base and pseudo-format target name.
- **Transform list assertion test**: `build_q2_preview_transform_pipeline` produces
  a pipeline containing exactly the 19 named transforms, in the documented order.
- **Pipeline structural test**: `build_q2_preview_pipeline_stages` produces a
  pipeline with the expected stage names and order; ends after
  `UserFiltersStage::post` / `ResourceReportStage`.
- **End-to-end fixture test**: a fixture qmd with `format: q2-preview`, a
  callout, a theorem, a `{{< meta foo >}}` shortcode, and a Lua filter.
  Run through the WASM entry point. Assert the resulting AST JSON contains:
  - The Callout encoded as `__quarto_custom_node` Div with `data-custom-type: Callout`.
  - The Theorem encoded similarly with `data-custom-type: Theorem`.
  - The shortcode resolved to its expected text (Plan 6 makes it a wrapper).
  - Lua filter visibly applied.
- **JS routing test** (vitest): mounting `ReactPreview` with
  `format="q2-preview"` content routes through `AstIframe` (matches q2-debug's
  test pattern).
- **End-to-end browser smoke** (playwright): open a fixture in hub-client,
  switch format to `q2-preview`, assert the iframe renders without error
  (visual fidelity is Plan 2's responsibility).

## Dependencies

- Depends on: nothing (this is the first plan to land).
- Blocks: Plan 2 (which decorates the AST shape this plan produces).
- Independent of: Plans 4/5/6/7/8 (they extend the writer / type system).

## Risk areas

- **`render_page_in_project_to_preview_ast` divergence**: the new entry point
  must mirror `render_page_in_project`'s single-file/project branching exactly,
  or else q2-preview behaves differently for default-projects vs. websites.
  Lift the shape directly from Carlos's existing function rather than rewriting.
- **ResourceReportStage and `extract_resource_report`**: the new
  `RenderToPreviewAstRenderer` should override `extract_resource_report` to
  return `None` (matching `RenderToHtmlRenderer` — engines don't run in
  WASM, so engine-emitted resources have nowhere to land).
- **Format detection ordering**: `Format::from_format_string` tries known base
  formats first, then extensions, then pseudo-formats. `q2-preview` lives in
  step 3 (pseudo). Confirm this doesn't get shadowed.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `build_q2_preview_transform_pipeline` + tests | ~80 |
| `build_q2_preview_pipeline_stages` + tests | ~80 |
| `RenderToPreviewAstRenderer` + WasmPassTwoPreviewOutput | ~150 |
| `render_page_in_project_to_preview_ast` + helpers | ~200 |
| Format detection update | ~20 |
| TS wrapper + ReactPreview routing + read-only guard | ~50 |
| End-to-end fixture and tests | ~200 |
| **Total** | **~780** |

Likely fits in one focused implementation session if we don't get sidetracked.
Risk: the WASM entry point's project-vs-single-file branching is the trickiest
part; budget time for getting that right.

## Notes

The user clarified: do NOT call this a "deny-list" approach. The transform
pipeline is a new pipeline, written from scratch with explicit `pipeline.push(...)`
calls. The fact that it happens to coincide with most of `build_transform_pipeline`
minus a few transforms is incidental to its construction — the reader of
`build_q2_preview_transform_pipeline` should see a clear, self-contained list.

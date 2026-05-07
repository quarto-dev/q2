# Plan 1 — q2-preview pipeline + integration

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions resolved; ready to build)
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
Theorem, etc.) reach React as `__quarto_custom_node` wrapper Divs (Plan 2B adds
type-specific React components for them; Plan 2A lands the iframe foundation
those components depend on).

Edit-back is **read-only** in v1 — `ReactPreview.tsx`'s `handleSetAst`
early-returns with a console warning for `q2-preview` format. Plan 7 removes
this guard once the writer-side round-trip lands.

## Scope

### In scope

- Format detection: `q2-preview` recognized as a new pseudo-format mapping to HTML
  base. Edit `crates/quarto-core/src/format.rs::builtin_pseudo_format`.
- New pipeline builder `build_q2_preview_pipeline_stages` in
  `crates/quarto-core/src/pipeline.rs` alongside
  `build_html_pipeline_stages_with_options`. Includes everything up
  through `UserFiltersStage::post` and `ResourceReportStage`.
  `CompileThemeCssStage` **is included** (see §"Multi-plan contract:
  theme CSS artifact"). `CodeHighlightStage`, `RenderHtmlBodyStage`,
  and `ApplyTemplateStage` are **excluded** (these turn the AST into
  HTML; q2-preview returns the AST). The exact stage list and order
  is asserted by the structural test in §"Test plan".
- New transform pipeline builder `build_q2_preview_transform_pipeline`
  in `pipeline.rs` alongside `build_transform_pipeline` and
  `build_analysis_transform_pipeline`. Same constructor signature as
  `build_transform_pipeline` (`shortcode_paths`, `extensions`,
  `runtime`, `target_format`) so the drift-protection helper can
  compare the two pipelines apples-to-apples. This is a new pipeline,
  not a "subtraction" from `build_transform_pipeline` — written as a
  fresh sequence of explicit `pipeline.push(...)` calls listing
  exactly the transforms that run in q2-preview.
- Extend `AstTransformsStage::run()` (`crates/quarto-core/src/stage/stages/ast_transforms.rs:134`)
  to dispatch on `ctx.format.target_format == "q2-preview"` and call
  `build_q2_preview_transform_pipeline` instead of `build_transform_pipeline`
  in that case. This is the seam that gets the new transform list to
  the stage at run-time, when `shortcode_paths` from `doc.ast.meta` is
  finally available. See §"Resolved decisions" for why this is the
  chosen design (option A). **Important: the existing JIT branch reads
  `ctx.format.identifier.as_str()` (which returns `"html"` for any
  HTML-based pseudo-format including q2-preview); it must be changed
  to read `ctx.format.target_format` (which preserves `"q2-preview"`).
  This is a temporary fix that lands together with the temporary
  `doRender` format switch in `ReactPreview.tsx` (described later in
  this §Scope list) — both will be cleaned up in **Plan 7**, which is
  already filling in placeholder/stub code in the same area (read-only
  guard removal). See §"Multi-plan contract: cleanup owed to Plan 7".**
- New entry-point function `render_qmd_to_preview_ast` in
  `crates/quarto-core/src/pipeline.rs` (alongside `render_qmd_to_html`:
  takes content + ctx + config + runtime, returns an `AstOutput`-like
  struct carrying the serialized AST JSON instead of HTML). AST JSON
  serialization uses `pampa::writers::json::JsonConfig {
  include_inline_locations: true }` — lifted verbatim from
  `parse_qmd_to_ast` at `wasm-quarto-hub-client/src/lib.rs:914-916`.
- **`WasmPassTwoOutput` gains a `Pass2Payload` enum** in place of
  its `html: String` field (see §"Resolved decisions"). Both
  `RenderToHtmlRenderer` and the new `RenderToPreviewAstRenderer`
  share `type Output = WasmPassTwoOutput;`; they differ only in
  which `Pass2Payload` variant they construct.
- New Pass2Renderer impl `RenderToPreviewAstRenderer` in
  `crates/quarto-core/src/project/pass2_renderer.rs` (alongside
  `RenderToHtmlRenderer`), calling `render_qmd_to_preview_ast` per page
  and constructing `WasmPassTwoOutput { payload:
  Pass2Payload::AstJson(...), ... }`.
- **Internal dispatch through the unified single-doc helper** — *not* a
  new wasm-bindgen export. After format detection inside
  `render_single_doc_to_response` (post-prep-refactor; see
  §"Resolved decisions" / "Single-doc helper unification"), branch
  on `format.target_format == "q2-preview"` to a new
  `render_single_doc_to_preview_response` helper. Mirror the same
  pattern in `render_project_active_page_to_response` with a new
  `render_project_active_page_to_preview_response`. Both new
  helpers live in `crates/wasm-quarto-hub-client/src/lib.rs`
  alongside their HTML siblings. Because `render_qmd`,
  `render_qmd_content`, and `render_page_in_project`'s single-file
  branch all delegate to the same single-doc helper after the prep
  refactor, **q2-preview routing is added at exactly two seams**
  (single-doc + project-active), not five. See §"Resolved decisions"
  for the rationale (Option B over a new export).
- **`RenderResponse` gains `ast_json: Option<String>`**. The existing
  JSON envelope at `lib.rs:~1283` keeps its shape; a new optional
  field carries q2-preview's payload. All three producers (`render_qmd`,
  `render_qmd_content`, `render_page_in_project`) populate the new
  field — `None` for non-preview paths. The doc-comment at
  `lib.rs:1283-1285` ("response shape is the same") needs updating to
  reflect the optional field. **No new TypeScript wrapper** — the
  existing `renderPageInProject` at `wasmRenderer.ts:396` works as-is;
  the TS `RenderResponse` type grows `astJson?: string` and consumers
  pick the right field based on format.
- `ReactRenderer.tsx` routes `format === 'q2-preview'` through `AstIframe`
  (alongside the existing `format === 'q2-debug'` branch at line ~141).
  Note: this is `ReactRenderer.tsx`, not `ReactPreview.tsx` — the latter
  passes `format` down but doesn't pick the renderer.
- **`ReactPreview.tsx`'s `doRender` gains a temporary format switch**:
  q2-debug / q2-slides keep using `parseQmdToAst(content)` (path-less,
  in-memory content); q2-preview calls `renderPageInProject(currentFile.path)`
  and reads `astJson` from the response (path-based, reads from VFS —
  same pattern `Preview.tsx` uses for HTML preview). The current
  `doRender` does *not* switch on format; this is a new branch, not an
  extension. Marked temporary because the interface will be cleaned up
  in **Plan 7** alongside the read-only-guard removal (see §"Multi-plan
  contract: cleanup owed to Plan 7"). Update the
  `format` prop's type comment in `ReactPreview.tsx` (`format: string;
  // 'q2-slides' or 'q2-debug'` at line ~39 of that file) to include
  `'q2-preview'`.
- **Read-only guard** in `ReactPreview.tsx::handleSetAst`. No-op the
  rewrite path and log a warning when `format === 'q2-preview'`:
  ```tsx
  if (format === 'q2-preview') {
    console.warn('q2-preview is read-only in v1; AST edit dropped');
    return;
  }
  ```
  Add `format` to the `useCallback` dependency array. The guard is a
  one-block diff for Plan 7 to delete. The contract this protects is
  forward-compat for **Plan 2B**: Plan 2B's CustomNode React components
  are the things that will eventually call `setLocalAst` for
  kanban-style edits. Without the guard, those components could
  silently corrupt source through a writer path that hasn't been
  validated for q2-preview yet.

### Out of scope (deferred to other plans)

- React component implementations for CustomNodes (Plan 2B; Plan 2A
  lands the iframe foundation).
- Filter idempotence verification (Plan 3).
- Provenance type changes (Plans 4/5/6).
- Edit-back round-trip via `incremental_write_qmd_for_preview` (Plan 7).
- Include round-trip via wrapper CustomNodes (Plan 8).
- q2-slides upgrade to the q2-preview pipeline (future plan; see
  §"Decision A: q2-slides reuses the q2-preview pattern").

## Design decisions (settled in conversation)

- **q2-preview is a distinct format** from q2-debug. q2-debug stays exactly as
  it is (parser-only, raw AST view via `parseQmdToAst`). Existing demos
  using `format: q2-debug` continue to work unchanged. q2-preview is a
  starting-point compatible with q2-debug at the iframe surface (same
  `AstIframe` component, same postMessage protocol) but architecturally
  unrelated at the data-source layer — q2-debug skips the entire transform
  pipeline; q2-preview runs ~19 of the 31 transforms in
  `build_transform_pipeline`. The "mirror q2-debug" framing applies only
  to the React-side routing; the WASM entry point and renderer are new.
- **Read-only in v1**: `handleSetAst` early-returns and logs a
  `console.warn` when `format === 'q2-preview'`. Avoids source
  corruption while round-trip work is in progress. The guard is
  one-block, deletable in Plan 7.
- **q2-preview routes through the existing iframe sandbox** introduced by Elliot
  (commit `72ef918c`). Same `AstIframe` component, same `ast-renderer-entry.tsx`
  iframe entry, same postMessage protocol.
- **The transform pipeline is a new explicit list**, not `build_transform_pipeline`
  with deletions. Concretely, every transform that runs in q2-preview is named
  in `build_q2_preview_transform_pipeline` via `pipeline.push(...)`.
- **`Pass2Renderer` trait already exists** (Carlos, Phase 9 of feature/websites).
  We add a third impl alongside `RenderToFileRenderer` (native) and
  `RenderToHtmlRenderer` (WASM HTML).

### Decision A: q2-slides reuses the q2-preview pattern (future)

Today `q2-slides` and `q2-debug` have **no backend difference** —
both go through `parseQmdToAst`, both render in the iframe via
`AstIframe`, neither runs any of the transform pipeline. Once Plan 1
lands, the same pipeline-and-renderer plumbing it introduces for
q2-preview should apply to q2-slides: the AST runs through
`build_q2_preview_transform_pipeline` (or a slides-specific variant
if divergences emerge), then a slides-specific React layout consumes
the result. Proving this on q2-preview first is deliberate — q2-slides
follows once q2-preview is stable, on a separate future plan.

**Future naming (straw man, not a Plan 1 commitment):** the
"format / preview-mode" duality may eventually be cleaner expressed
as a render-target modifier rather than a pseudo-format:

- `format: html` + `preview: true` for what is currently `q2-preview`.
- `format: revealjs` + `preview: true` for what is currently `q2-slides`.

This factors the preview mode out of the format taxonomy, so any
real format can request its preview view without inventing a new
pseudo-format name. Plan 1 ships under the `q2-preview` pseudo-format
name to keep this plan small; the naming reorganization is
deliberately out of scope and would land in the same future plan that
brings q2-slides onto the q2-preview pipeline.

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
- `TitleBlockTransform` — React layout reads `meta.title` directly (Plan 2B).
- `FootnotesTransform` — synthesizes a non-source-bearing container; defer to
  future plan with wrapper-CustomNode round-trip story.
- `TocRenderTransform`, `NavbarRenderTransform`, `SidebarRenderTransform`,
  `PageNavRenderTransform`, `FooterRenderTransform` — produce HTML strings;
  q2-preview elides them. The originally-planned Sidebar body-classes
  and Navbar brand-fallback JS reimplementation was deferred during the
  2026-05-06 review (Plan 2A §"Out of scope: layout chrome") and now
  ships with the future "q2-preview layout chrome" plan.
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

## Resolved decisions

(This section was originally "Open questions for implementation" — every
item has been resolved in conversation; the list below preserves the
rationale for each decision.)

- **`WasmPassTwoOutput` payload becomes an enum**: instead of
  introducing a parallel `WasmPassTwoPreviewOutput` struct, the
  existing `WasmPassTwoOutput` (`pass2_renderer.rs:250`) gets a
  small refactor — its `html: String` field becomes
  `payload: Pass2Payload`, where `Pass2Payload` is a new enum:
  ```rust
  #[derive(Debug)]
  pub enum Pass2Payload {
      Html(String),
      AstJson(String),
  }
  ```
  `RenderToHtmlRenderer` and `RenderToPreviewAstRenderer` both
  set `type Output = WasmPassTwoOutput;` and differ only in the
  variant they construct. The orchestrator
  (`render_project_active_page_to_response`, `lib.rs:1416`) stays
  **one function** — it dispatches on the variant at the response
  tail (`html: Some(...)` or `ast_json: Some(...)`), not by
  forking the whole helper. Every shared field
  (`source_path`, `diagnostics`, `source_context`,
  `page_artifacts`) stays in the parent struct and remains
  payload-agnostic.

  Tradeoffs vs. the parallel-struct approach: enum adds a
  one-line `match` at the response builder; parallel-struct would
  require a near-clone of the ~80-line orchestrator helper for
  every additional payload type (q2-slides, dashboards, future
  formats). With only one shared HTML-specific access in the
  current orchestrator (line 1538), the enum's runtime ceremony
  is cheaper than the structural duplication.

  Native test fixtures that construct `WasmPassTwoOutput` (gate-free
  per the existing module comment) need a one-token update:
  `html: "..."` → `payload: Pass2Payload::Html("...".into())`.

  The single-doc path (`render_single_doc_to_response` post
  prep-refactor) does **not** construct `WasmPassTwoOutput` — it
  calls `render_qmd_to_html` or the new
  `render_qmd_to_preview_ast` directly. The format dispatch there
  lives one level higher and writes results into the appropriate
  `RenderResponse` field without an enum hop. The enum is a
  WASM-orchestrator boundary type only; `RenderOutput`
  (`pipeline.rs:131`) stays HTML-specific.

- **`PreviewAstOutput` (new entry-point return type)**:
  `render_qmd_to_preview_ast` is the q2-preview sibling of
  `render_qmd_to_html`. Returns:
  ```rust
  pub struct PreviewAstOutput {
      pub ast_json: String,
      pub diagnostics: Vec<DiagnosticMessage>,
      pub source_context: SourceContext,
  }
  ```
  `pub` for cross-crate use, `#[derive(Debug)]` to match
  `RenderOutput`. JSON serialization happens **inside** this
  function (not in the renderer): the function builds an
  `ASTContext` lifted from `parse_qmd_to_ast`'s consumer
  (`wasm-quarto-hub-client/src/lib.rs:905-910`) plus the
  `JsonConfig { include_inline_locations: true }` from
  `lib.rs:914-916`, then calls
  `pampa::writers::json::write_with_config` and stores the
  resulting string in `ast_json`. The renderer
  (`RenderToPreviewAstRenderer`) just plumbs the field through
  to `Pass2Payload::AstJson`.

  Existing `AstOutput` (`pipeline.rs:140`) is **not** reused — it
  carries a typed `Pandoc`, not JSON, and is consumed by q2-debug's
  thinner pipeline. A separately-named struct keeps the two
  output flavors distinct.
- **Boundary type (wasm-bindgen)**: `RenderResponse` (`lib.rs:~1283`)
  gains `ast_json: Option<String>` alongside the existing optional
  `html`. Doc-comment at `lib.rs:1283-1285` updated to reflect the new
  optional field. JS-side TS type grows `astJson?: string`. Format
  dispatch happens at the consumer (`ReactPreview.doRender`).
- **AST JSON serialization config**: `pampa::writers::json::JsonConfig
  { include_inline_locations: true }` — verbatim lift from
  `wasm-quarto-hub-client/src/lib.rs:914-916` (`parse_qmd_to_ast`).
- **Single-file behavior of navigation-generate transforms**: the five
  transforms `TocGenerateTransform`, `NavbarGenerateTransform`,
  `SidebarGenerateTransform`, `PageNavGenerateTransform`,
  `FooterGenerateTransform` produce structured navigation metadata
  only when a `ProjectIndex` is present (i.e., a `_quarto.yml` ancestor
  was discovered). In q2-preview's single-file branch
  (`render_single_doc_to_preview_response`), these transforms run but
  no-op cleanly because the HTML pipeline behaves the same way today —
  the existing single-doc path at `lib.rs:1341` already invokes them
  through `render_qmd_to_html` without project context, and they
  short-circuit on the missing index. q2-preview inherits that
  behavior. The single-file e2e test (§Test plan) verifies the absence
  of regressions.
- **Single-file artifact flush**: mirror the `RenderToHtmlRenderer`
  loop at `pass2_renderer.rs:370-382` for Project-scoped artifacts
  (`drain_project_scoped` → `flush_site_libs` if `lib_dir` is empty,
  else `merge_into_project`) and the loop at `lib.rs:1386-1391` for
  Page-scoped artifacts (per-key VFS write). Both are required to
  honor the multi-plan contracts below.
- **Page-scoped artifact handling**: mirror `RenderToHtmlRenderer`'s
  Page-scoped loop. The loop runs the same artifact-flush as the
  HTML pipeline; for theme CSS / icon CSS / fonts (artifacts with
  real bytes via `Artifact::from_bytes`) this puts loadable bytes
  in VFS at `vfs_root.join(artifact_path)`. **For image artifacts
  this is a no-op-or-clobber**: `ResourceCollectorTransform` uses
  `Artifact::from_path`, which leaves `content` empty; the flush
  writes those empty bytes to the resolved path. `<img src>` in the
  AST stays as the user wrote it (no transform mutates
  `Image::target.0`), and the image bytes the iframe ultimately
  reads come from the user's original VFS upload (written by the
  hub-client's `automergeSync`), not from the renderer's flush. See
  §"Multi-plan contract: page-scoped image artifacts" for the full
  contract and the latent-bug note (Plan 2A §"Risk areas →
  Empty-content artifact overwrite").
- **Drift-protection test**: a single helper `assert_filtered_subset`
  asserts that `build_q2_preview_transform_pipeline` is exactly
  `build_transform_pipeline` filtered by an explicit exclusion list,
  preserving order. Catches every drift mode in one shot: new
  transform added to full pipeline, transform renamed, transform
  reordered on either side, transform removed from subset. The helper
  is reusable — `build_analysis_transform_pipeline`
  (`pipeline.rs:397`) is a second subset pipeline today, protected
  only by a doc-comment; **a follow-up beads issue applies the same
  helper to it**. See §"Test plan" for the helper signature, the
  exclusion list (12 names), and a complementary order test.
- **JIT seam for `AstTransformsStage`** (option A): extend the existing
  JIT branch in `AstTransformsStage::run()` to dispatch on
  `ctx.format.target_format`. When the format is `"q2-preview"`, build
  the pipeline via `build_q2_preview_transform_pipeline`; otherwise call
  `build_transform_pipeline` (today's path). Why option A over a factory
  closure (B) or a sibling `Q2PreviewAstTransformsStage` (C): cheapest
  to implement, keeps the stage list identical between formats, and
  doesn't introduce a new construction mode that has to coexist with
  the JIT path. The cost is that `AstTransformsStage` now knows two
  format names — acceptable until a third arrives, at which point we
  revisit.
- **WASM entry-point shape (Option B over Option A)**: the q2-preview
  dispatch lives **inside** the existing `render_page_in_project`
  function rather than as a separate `render_page_in_project_to_preview_ast`
  export. Why: format detection already happens internally at
  `lib.rs:1352, 1434`; a separate export would duplicate ~50 lines of
  project-discovery scaffolding for no boundary-type win (the
  `RenderResponse` envelope already discriminates payload). It also
  keeps Plan 7's eventual write-back work local to one function, not
  three. The doc-comment at `lib.rs:1283-1285` ("response shape is the
  same") evolves to "response shape's payload is selected by format"
  rather than being broken outright.

  Note: `Format::from_format_string` is actually called from **five**
  sites in `wasm-quarto-hub-client/src/lib.rs` today — `parse_qmd_to_ast`
  (865), `render_qmd` (1045), `render_qmd_content` (1166),
  `render_single_doc_to_response` (1352), and
  `render_project_active_page_to_response` (1434). Site 1
  (`parse_qmd_to_ast`) returns `AstResponse`, not `RenderResponse`,
  and is q2-debug's existing pre-pipeline path — it stays
  HTML-pipeline-untouched. The other four are unified in the prep
  refactor below so q2-preview routing has only **two** seams:
  the single-doc helper and the project-active helper.

- **Single-doc helper unification (prep refactor)**: Today
  `render_qmd` (`lib.rs:1005`) and `render_qmd_content`
  (`lib.rs:1152`) each carry near-duplicate bodies of the
  single-doc render flow that `render_single_doc_to_response`
  (`lib.rs:1341`) already implements. The doc-comment on that
  helper claims `render_qmd` already routes through it, but the
  source still duplicates. Plan 1's **first commit** is a
  behavior-preserving prep refactor: factor `render_qmd` and
  `render_qmd_content` to delegate to
  `render_single_doc_to_response` with a synthesized
  `(path, content, project)` triple — no signature change to the
  helper, no behavior change for HTML. After this lands, the
  q2-preview branch added inside the helper covers all three
  wasm-bindgen entry points (`render_qmd`, `render_qmd_content`,
  and `render_page_in_project`'s single-file branch) at once. A
  fixture using `render_qmd <path>` with `format: q2-preview`
  Just Works as a side effect.

  Helper signature is unchanged from today:
  ```rust
  async fn render_single_doc_to_response(
      path: &Path,
      content: &[u8],
      project: &ProjectContext,
      user_grammars: Option<JsUserGrammars>,
  ) -> String
  ```

  Pre-requisites of the helper's contract are already met by both
  call sites' preludes:
  - `render_qmd`: `runtime.file_read(path)` for content +
    `ProjectContext::discover(path, runtime)` for project.
  - `render_qmd_content`: `Path::new("/input.qmd")` synthetic +
    `create_wasm_project_context(path)` minimal. The unused
    `_template_bundle` arg stays on the public signature for
    ABI compat; it doesn't reach the helper.

  Net effect on `RenderResponse` construction sites: drops from
  ~16 inline literals today to ~5 (two success paths in the two
  helpers + three error helpers `error_response` /
  `render_error_response` / `pass_failure_response`). The
  `ast_json: Option<String>` field added later in Plan 1 only
  needs populating at those ~5 sites instead of every producer.

  The orchestrator path
  (`render_project_active_page_to_response`, `lib.rs:1416`) is
  **separate** — `ProjectPipeline<RenderToHtmlRenderer>`-driven,
  not reachable through the single-doc helper. Its q2-preview
  branch is added independently per the original Option-B
  internal-dispatch decision.

## References

- `crates/quarto-core/src/pipeline.rs:170-300` — existing pipeline builders.
- `crates/quarto-core/src/pipeline.rs:677` — `build_transform_pipeline` (don't
  reuse; just reference for transform names).
- `crates/quarto-core/src/pipeline.rs:397` — `build_analysis_transform_pipeline`,
  the existing sibling subset pipeline whose drift-protection follow-up
  applies the same helper this plan introduces.
- `crates/quarto-core/src/project/pass2_renderer.rs:75` — `Pass2Renderer`
  trait. Existing impls: `RenderToFileRenderer` (native, line 188),
  `RenderToHtmlRenderer` (line 297). `WasmPassTwoOutput` struct at
  line 250.
- `crates/wasm-quarto-hub-client/src/lib.rs:1292` — `render_page_in_project`
  entry point (the function we extend internally rather than fork).
- `crates/wasm-quarto-hub-client/src/lib.rs:1341` — `render_single_doc_to_response`
  (the single-doc branch we mirror as `render_single_doc_to_preview_response`).
- `crates/wasm-quarto-hub-client/src/lib.rs:1416` — `render_project_active_page_to_response`
  (the project branch we mirror as `render_project_active_page_to_preview_response`).
- `crates/wasm-quarto-hub-client/src/lib.rs:1283-1285` — the doc-comment
  contract that `RenderResponse` is the same shape across branches;
  evolves to "payload selected by format."
- `crates/wasm-quarto-hub-client/src/lib.rs:914-916` — `parse_qmd_to_ast`'s
  `JsonConfig`, lifted verbatim for q2-preview AST serialization.
- `crates/wasm-quarto-hub-client/src/lib.rs:1005, 1152` — `render_qmd`
  and `render_qmd_content`, the two wasm-bindgen entry points that
  the prep refactor collapses into thin preludes delegating to
  `render_single_doc_to_response`.
- `crates/wasm-quarto-hub-client/src/lib.rs:1555, 1569, 1596` —
  `error_response`, `render_error_response`, `pass_failure_response`
  helpers; each constructs `RenderResponse` and needs the new
  `ast_json: None` field.
- `hub-client/src/components/render/ReactRenderer.tsx` — format dispatch,
  `AstIframe` mounting (the `format === 'q2-debug'` branch ~line 141 is
  where the q2-preview branch joins).
- `hub-client/src/components/render/ReactPreview.tsx` — `doRender` (where
  the data-source switch by format lands) and `handleSetAst` (where the
  read-only guard lands).
- `crates/quarto-core/src/stage/stages/ast_transforms.rs` — `AstTransformsStage::run()`
  JIT branch (where the format-dispatch seam lands per §"Resolved decisions").
- `crates/quarto-core/src/pipeline.rs::DEFAULT_CSS_ARTIFACT_PATH` —
  the well-known VFS path the theme CSS artifact lands at.
- `hub-client/src/services/wasmRenderer.ts` — TS wrappers.
- `crates/quarto-core/src/format.rs:106` — `builtin_pseudo_format`.

## Test plan

- **Format detection unit test**: `Format::from_format_string("q2-preview")`
  returns a Format with HTML base and `target_format == "q2-preview"`.
- **Drift-protection test (subset + order in one assertion)**: a small
  helper in `pipeline.rs` (cfg-test):
  ```rust
  fn assert_filtered_subset(
      full: &TransformPipeline,
      subset: &TransformPipeline,
      expected_excluded: &[&str],
      drift_doc_pointer: &str,
  ) {
      let full_names: Vec<&str> = full.iter().map(|t| t.name()).collect();
      let subset_names: Vec<&str> = subset.iter().map(|t| t.name()).collect();
      let excluded: HashSet<&str> = expected_excluded.iter().copied().collect();

      // Catch typos / renames in the exclusion list early.
      let unknown: Vec<_> = expected_excluded
          .iter()
          .filter(|n| !full_names.contains(n))
          .collect();
      assert!(
          unknown.is_empty(),
          "expected_excluded names not in full pipeline: {unknown:?}. \
           See {drift_doc_pointer}."
      );

      let computed: Vec<&str> = full_names
          .iter()
          .copied()
          .filter(|n| !excluded.contains(n))
          .collect();
      assert_eq!(
          subset_names, computed,
          "Subset pipeline drift. See {drift_doc_pointer}."
      );
  }
  ```
  The q2-preview test passes the 12-name exclusion list:
  `CalloutResolveTransform`, `WebsiteFaviconTransform`,
  `TitleBlockTransform`, `FootnotesTransform`, `TocRenderTransform`,
  `NavbarRenderTransform`, `SidebarRenderTransform`,
  `PageNavRenderTransform`, `FooterRenderTransform`,
  `LinkRewriteTransform`, `AppendixStructureTransform`,
  `CrossrefRenderTransform`. The helper assumes `subset ⊆ full`
  — document this in its doc-comment. (Note: every `Transform` impl
  already has `fn name(&self) -> &str`, so the trait surface is in
  place; no new accessor needed.) **Follow-up beads issue applies
  this helper to `build_analysis_transform_pipeline` against the
  exclusion list documented in its doc-comment at
  `pipeline.rs:381-396`.**
- **Pipeline structural test**: `build_q2_preview_pipeline_stages` produces a
  pipeline with the expected stage names and order; ends after
  `UserFiltersStage::post` / `ResourceReportStage`. Excludes
  `CodeHighlightStage`, `RenderHtmlBodyStage`, `ApplyTemplateStage`.
- **End-to-end fixture tests** (two fixtures, both `format: q2-preview`):
  - **Single-file fixture**: no `_quarto.yml` ancestor. Includes a
    callout, a theorem, a `{{< meta foo >}}` shortcode, and a Lua
    filter. Routes through the single-file branch
    (`render_single_doc_to_preview_response`). Assert:
    - The Callout encoded as `__quarto_custom_node` Div with
      `data-custom-type: Callout`.
    - The Theorem encoded similarly with `data-custom-type: Theorem`.
    - The shortcode resolved to its expected text (Plan 6 makes it a
      wrapper).
    - Lua filter visibly applied.
    - The five navigation-generate transforms ran without producing
      stale metadata (verifies the no-op behavior from §"Resolved
      decisions").
  - **Website fixture with embedded image**: in a project with
    `_quarto.yml`, navbar, sidebar. Includes a callout, a theorem,
    and an embedded image. Routes through the project branch
    (`render_project_active_page_to_preview_response`). Subsumes the
    "Page-scoped image artifact regression test". Assert all of:
    1. **AST preserves the user-written URL**: parse the AST JSON
       and locate the `Image` node; assert `target.0 == "hero.png"`
       (no transform mutates `Image::target.0`).
    2. **Manifest entry exists with the expected resolved path**:
       `output.page_artifacts` contains an entry whose `path` field
       equals `project_dir.join("hero.png")` — verifies the
       `ResourceCollectorTransform` visitor's `base_dir.join(url)`
       resolution matches what `automergeSync` would have used to
       upload the image.
    3. **Manifest entry has empty content** (today's reality —
       `ResourceCollectorTransform` uses `Artifact::from_path`).
       This assertion is fragile-by-design: it documents current
       behavior and will need flipping when the empty-content
       overwrite bug fix lands (see Plan 2A §"Risk areas →
       Empty-content artifact overwrite"). Comment in the test
       points at the beads issue.
    4. **Navbar / sidebar metadata populated** in `meta` —
       `meta["navigation"]["navbar"]` and `meta["navigation"]["sidebar"]`
       are non-empty.

    Once the empty-content bug is fixed, add:
    5. (Post-bug-fix) **User upload bytes survive the render**:
       in a WASM-runtime test, pre-populate VFS with non-empty bytes
       at `project_dir.join("hero.png")`, run the render, assert the
       bytes are unchanged. This is the assertion that fully closes
       the iframe-finds-bytes contract; it requires the bug fix
       because today the flush would clobber the upload.

    Guards the contract Plan 2A consumes (§"Multi-plan contract:
    page-scoped image artifacts").
- **JS routing test** (vitest): mounting `ReactPreview` with
  `format="q2-preview"` content routes through `AstIframe` (matches q2-debug's
  test pattern).
- **End-to-end browser smoke** (playwright): open a fixture in hub-client,
  switch format to `q2-preview`, assert the iframe renders without error
  (visual fidelity is Plan 2B's responsibility; Plan 2A delivers theme
  CSS + image rendering).
- **Theme CSS artifact regression test**: after a q2-preview render of a
  fixture that triggers theme compilation, assert
  `/.quarto/project-artifacts/styles.css` exists in VFS and is non-empty.
  Plan 1 doesn't read this artifact, but the test guards the contract
  Plan 2A consumes (see §"Multi-plan contract: theme CSS artifact").
- **Format-switch behavior — manual verification only**: switching
  a live `ReactPreview` from `format="html"` to `format="q2-preview"`
  (and back) without remounting is exercised interactively by the
  user when shipping Plan 1, not via vitest. The data-shape
  divergence between formats and the `doRender` format switch are
  load-bearing here; an automated test would either deeply mock
  the iframe (low value) or drive a real iframe (Playwright-only),
  and the latter overlaps with the playwright smoke test below.
  Documented as manual so anyone running through Plan 1 acceptance
  knows to flip the format toggle live.

## Dependencies

- Depends on: nothing (this is the first plan to land).
- Blocks: Plan 2A (iframe foundation), then Plan 2B (decorates the AST
  shape this plan produces).
- Independent of: Plans 4/5/6/7/8 (they extend the writer / type system).

## Multi-plan contracts

The decisions Plan 1 makes commit it to forward-compatibility with
several other plans. Each subsection below names a contract Plan 1
upholds, the plan that consumes it, and the regression test that
guards it (where applicable).

### Multi-plan contract: read-only mode lifts at Plan 7

Plan 1 ships q2-preview in **read-only mode**: `ReactPreview.tsx`'s
`handleSetAst` early-returns for the q2-preview format with a
`console.warn`. The rewrite path no-ops; the warning logs once per
edit attempt. This is a deliberate placeholder, not a bug — Plan 7
lifts the guard once `incremental_write_qmd_for_preview`'s round-trip
machinery is in place. The guard is a one-block diff for Plan 7 to
delete.

The forward-compat contract this protects is **Plan 2B**: Plan 2B's
CustomNode React components are the things that will eventually call
`setLocalAst` for kanban-style edits. Without the guard, those
components could silently corrupt source through a writer path that
hasn't been validated for q2-preview yet.

Between Plan 1 and Plan 7 landing, q2-preview is **viewable but not
editable**. User-facing behavior:
- Document renders correctly via the q2-preview pipeline + React layer.
- Component-driven edits (kanban drag, comment buttons, etc.) call
  `setLocalAst`, which no-ops with a `console.warn`. The action's
  on-screen affordance may *appear* to succeed (a card visibly drops
  into a new column) but the underlying source is unchanged and the
  next render reverts the visual state. **This is acceptable post-Plan-2B**
  — interactive components fail soft until Plan 7 wires the writer
  round-trip, and the user accepts that UX gap explicitly.
- The user can still navigate, scroll, and observe filter/shortcode
  output.

This contract is intentional for the M1/M2 milestones. M3 lands when
Plan 7 ships and edits round-trip correctly.

### Multi-plan contract: theme CSS artifact

Plan 1's pipeline includes `CompileThemeCssStage`, and the
`RenderToPreviewAstRenderer` drains Project-scoped artifacts the same
way `RenderToHtmlRenderer` does (`flush_site_libs` for default
projects, accumulator + `WebsiteProjectType::post_render` for
websites). Net effect: after every q2-preview render, the compiled
theme CSS lands in VFS at `/.quarto/project-artifacts/styles.css`
(per `pipeline.rs::DEFAULT_CSS_ARTIFACT_PATH`).

**This artifact is unread in Plan 1.** The React iframe used by
q2-preview (`ast-renderer.html`) ships only a system-font reset; it
does not load any project-emitted CSS. The artifact write is a
forward-compatibility commitment that Plan 2A consumes.

**Expected user-visible state between Plan 1 and Plan 2A landing**:
q2-preview renders **unstyled** — no Bootstrap classes are
applied, no theme colors, no typography. The iframe shows raw
semantic markup (Callout, Theorem, Section divs, etc.) over the
system-font reset. This is intentional and not a bug. Anyone
testing Plan 1 in isolation should expect this and not chase it
as a styling regression. The styling story lands with Plan 2A's
theme-CSS injection (Plan 2A §"In scope" item 10, "AstWithAssets
wrapper component").

**Resolved by Plan 2A**: the visual-fidelity strategy is
**class-compatible-with-bootstrap** — Plan 2B's components emit the
same class names as Rust's HTML output, and Plan 2A's iframe entry
reads the VFS artifact at first AST receive and injects the bytes
as an inline `<style>` element in `document.head`. (The HTML
iframe's `<link>` rewrite at `iframePostProcessor.ts:137-147`
doesn't carry over: the AST iframe never has a `<link>` element to
rewrite — Pandoc nodes don't produce stylesheet links — so one-shot
inline injection replaces the rewrite for CSS. When service-worker
resource resolution lands, both paths converge.) Plan 1's artifact
write feeds this contract directly. The earlier "component-local
styling" alternative was discussed and rejected.

A regression test asserts the artifact exists in VFS after a
q2-preview render (see §Test plan).

### Multi-plan contract: page-scoped image artifacts

The contract here is subtler than "renderer writes, iframe reads"
and was clarified after a code-trace during the 2A plan review.

**The renderer does not contribute image bytes.**
`ResourceCollectorTransform` walks the AST immutably (it takes
`&Block` / `&Inline` references) and stores artifact entries via
`Artifact::from_path`, which sets `content: Vec::new()` (see
`crates/quarto-core/src/artifact.rs:108-116`). The WASM flush loop
at `crates/wasm-quarto-hub-client/src/lib.rs:1208-1214` (single-doc)
and `:1364-1369` (project) writes those empty bytes to the
resolver's on-disk path. So for image artifacts the flush is at
best a no-op manifest entry, at worst an overwrite of whatever
bytes were at the resolved path. (See the latent-bug note in the
2A plan's §"Risk areas" — `Path::join` with an absolute second arg
replaces the first, so the resolved path collapses to the absolute
artifact path, which collides with the user's upload location. A
follow-up beads issue tracks the one-line guard fix.)

The image bytes the iframe actually reads come from the **user's
original VFS upload**, written by the hub-client's `automergeSync`
via `vfsAddFile` / `vfsAddBinaryFile` whenever a project file
syncs. The iframe rewriter uses
`resolveRelativePath(currentFilePath, src) + vfsReadBinaryFile`
to read those bytes back. The "agreement" the rewriter relies on
is that *the user uploaded the image at the same project-relative
path the qmd references it by* — not that the renderer flushed
bytes anywhere.

The AST preserves the user's URL unchanged. `LinkRewriteTransform`
explicitly leaves `Image::target.0` alone (per its line 29 doc
comment, and it's excluded from the q2-preview pipeline anyway);
no other transform mutates image URLs.

Plan 2A's iframe rewriter consumes the user's upload directly
(Plan 2A §"In scope: image rewriter helper"). The contract is
**asymmetric** with the theme-CSS contract: theme CSS is genuinely
"Plan 1 writes, Plan 2A reads"; image bytes are "user uploads,
Plan 2A reads." Non-image Page-scoped artifacts that follow the
theme-CSS shape (with real bytes via `Artifact::from_bytes`) ride
the same channel as theme CSS.

A regression test asserts the AST preserves the user-written URL
unchanged (no transform mutates `Image::target.0`) and that
`output.page_artifacts` contains an entry for the image (the
manifest entry; bytes empty, as expected). Once the latent-bug
guard lands, an additional assertion can verify the user's upload
bytes are not clobbered by the flush.

### Multi-plan contract: cleanup owed to Plan 7

Plan 1 ships two pieces of scaffolding deliberately marked
**temporary**, both in tension with the longer-term abstraction the
team will want once a third format (q2-slides per Decision A, or
something else) joins the pipeline:

1. **String-literal dispatch in `AstTransformsStage::run()`** —
   `if ctx.format.target_format == "q2-preview" { ... }`. Correct for
   today's two-format world (HTML + q2-preview); will need a cleaner
   abstraction (e.g. a `pipeline_kind` field on `Format`, or a
   stage-construction-time choice) before adding a third dispatch
   branch.
2. **String-literal format switch in `ReactPreview.tsx::doRender`** —
   `if (format === 'q2-preview') { renderPageInProject(...) } else
   { parseQmdToAst(...) }`. Same shape, same drift risk. Cleanup is a
   data-source abstraction that doesn't need to grep on format
   strings.

Plan 7 absorbs these cleanups for two reasons. First, Plan 7 is
already removing related placeholder/stub code in the same files
(`ReactPreview.tsx`'s read-only guard; the WASM round-trip wiring),
so the touch surface overlaps. Second, the dispatch interface
naturally settles when q2-preview becomes editable — Plan 7's
`pipeline_kind: "preview"` parameter already implies a structured
notion of "which pipeline are we in," which is the abstraction the
string-literal dispatches above are placeholders for.

**Pinned structural choice for the eventual cleanup**: when Plan 7
lands the cleanup, the structured selector lives as a new field
`pipeline_kind: Option<&'static str>` on `Format`
(`crates/quarto-core/src/format.rs`). `Format::from_format_string`
populates it from the same lookup table that drives
`builtin_pseudo_format` (q2-preview → `Some("preview")`; q2-debug
→ `None`; q2-slides → `Some("preview")` once Decision A lands).
`AstTransformsStage::run()` reads `ctx.format.pipeline_kind`
instead of comparing on `target_format`. The TS side mirrors this
with a thin helper (`pipelineKindForFormat(format) -> 'baseline' |
'preview'`) that's the single source of truth on the JS side; both
`ReactPreview.tsx::doRender`'s data-source switch and Plan 7's
edit-back wiring read through it. Plan 7's `incremental_write_qmd`
parameter is the same value, flowing through the write side.

Pinning this now means Plan 1 doesn't need to anticipate Plan 7's
field name — it can write today's `if
ctx.format.target_format == "q2-preview"` and Plan 7's cleanup is
a localized swap to `if ctx.format.pipeline_kind == Some("preview")`.

### Pass-1 caching and VFS-state contract

q2-preview shares Pass-1 with the HTML render path because the
divergence is purely in the transform pipeline, which runs in
Pass-2. The IndexedDB-backed `cache_get`/`cache_set` infra
(`wasm-quarto-hub-client/CLAUDE.md` §"VFS state contract") is
unaffected; cache hit-rate is preserved.

The same CLAUDE.md says **"Do not call `vfs_clear` between
renders."** q2-preview's writes (theme CSS, page-scoped artifacts)
ride the same VFS channel and obey the same contract. Stale
page-scoped entries from prior renders persist harmlessly — the
iframe simply stops referencing them.

## Risk areas

- **Branch divergence inside `render_page_in_project`**: the q2-preview
  dispatch lives inside the existing function (Option B in §"Resolved decisions").
  Both new helpers (`render_single_doc_to_preview_response`,
  `render_project_active_page_to_preview_response`) must mirror their
  HTML siblings' single-file/project branching exactly, or else
  q2-preview behaves differently for default-projects vs. websites.
  Lift the shape directly from the HTML versions; share factored
  pieces wherever practical.
- **`RenderResponse` producer fan-out**: After the prep refactor
  (single-doc helper unification, see §"Resolved decisions"),
  `RenderResponse` is constructed at five seams: two success
  paths (`render_single_doc_to_response`,
  `render_project_active_page_to_response`) and three error
  helpers (`error_response`, `render_error_response`,
  `pass_failure_response`). All five must populate the new
  `ast_json: Option<String>` field — `None` for every HTML and
  error path; `Some(...)` only in the two success paths' new
  q2-preview branches. The mechanical sweep is smaller after the
  prep refactor than before — but easy to miss one of the helper
  sites. Get the struct field and all five seams updated before
  flipping the consumer in `ReactPreview`.
- **Format-detection mismatch in `AstTransformsStage`**: today's JIT
  branch reads `ctx.format.identifier.as_str()` which returns
  `"html"` for q2-preview. Plan 1 must change this to
  `ctx.format.target_format`. Easy to miss because the existing code
  looks correct in isolation.
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
| Prep refactor: `render_qmd` + `render_qmd_content` → `render_single_doc_to_response` (HTML-only, behavior-preserving; lands as first commit) | ~50 (net negative) |
| `build_q2_preview_transform_pipeline` + drift helper + tests | ~100 |
| `build_q2_preview_pipeline_stages` + tests | ~80 |
| `Pass2Payload` enum + `WasmPassTwoOutput` field rename + native-test-fixture updates | ~30 |
| `RenderToPreviewAstRenderer` impl + `PreviewAstOutput` + `render_qmd_to_preview_ast` entry point | ~120 |
| Orchestrator response-tail dispatch (single match arm at `lib.rs:1538`) + single-doc format dispatch | ~30 |
| `RenderResponse` `ast_json` field + producer updates (5 seams post-refactor) | ~30 |
| Format detection update + `AstTransformsStage` dispatch | ~25 |
| TS `RenderResponse` type + `ReactPreview` doRender switch + read-only guard | ~40 |
| End-to-end fixture and tests (incl. page-scoped artifact regression) | ~220 |
| **Total** | **~725** |

Likely fits in one focused implementation session if we don't get sidetracked.
The enum payload + prep refactor together preserve the original
~725 estimate: the prep refactor saves ~50 LOC, the enum approach
saves the parallel-orchestrator-helper duplication (~80 LOC vs.
the parallel-struct alternative). Risk: the `RenderResponse`
extension touches five seams (two success paths + three error
helpers) — get all five populating `ast_json: None` for HTML/error
paths before flipping the consumer. The format-switch in
`ReactPreview.doRender` is temporary and will be cleaned up in
Plan 7.

## Notes

The user clarified: do NOT call this a "deny-list" approach. The transform
pipeline is a new pipeline, written from scratch with explicit `pipeline.push(...)`
calls. The fact that it happens to coincide with most of `build_transform_pipeline`
minus a few transforms is incidental to its construction — the reader of
`build_q2_preview_transform_pipeline` should see a clear, self-contained list.

# q2-preview attribution wiring

## Overview

Extend the attribution pipeline (`claude-notes/plans/2026-05-06-attribution-pipeline.md`)
to the q2-preview render path. Same JSON wire format as q2-debug, same Option A
producer (`PreBuiltAttributionProvider`), no new transform design — just the
small amount of glue needed to drive the existing stages from
`render_qmd_to_preview_ast` and to expose an attribution-aware WASM entry point.

**Prerequisite:** the attribution-pipeline plan lands first. This plan
references its types (`AttributionData`, `AttributionRecord`,
`PreBuiltAttributionProvider`, `attribution_lookup` / `attribution_actors`
on `JsonConfig`, the two transforms) and assumes Phase 0–4a of that plan
are merged. Nothing here changes its design; the work is purely additive.

## Why this is short

The attribution-pipeline plan already does the hard parts:

- `AttributionGenerateTransform` registers at the end of the Navigation Phase
  and `AttributionRenderTransform` at the end of the Finalization Phase
  inside `build_transform_pipeline`. `build_q2_preview_transform_pipeline`
  is `build_transform_pipeline` with a small exclusion list
  (`Q2_PREVIEW_TRANSFORM_EXCLUDED`, `pipeline.rs:1053`). Neither attribution
  stage appears in that list, so registration is **automatic for q2-preview**
  the day the attribution PR lands.
- `JsonConfig` already carries `attribution_lookup` /
  `attribution_actors` (added by the attribution plan's Phase 4a),
  and `render_qmd_to_preview_ast` already serialises via
  `pampa::writers::json::write_with_config`. Plumbing is one extra
  field read.

The only producer-side seam is the WASM boundary: q2-preview's WASM
entry is `render_page_in_project`, which does not today accept an
attribution payload.

## Work items

### Phase 0 — failing tests

- [x] **Native end-to-end (preview pipeline, no WASM).** Test
  `render_qmd_to_preview_ast_surfaces_attribution_when_provider_installed`
  added next to `render_qmd_to_preview_ast_preserves_callout_custom_node`
  in `crates/quarto-core/src/pipeline.rs`. Installs a
  `PreBuiltAttributionProvider` on `ctx.attribution_provider`,
  runs `render_qmd_to_preview_ast` against `"Hello world!"`,
  asserts both `astContext.attribution` and
  `astContext.attributionActors` keys present and carry the
  expected `actor` / `name` / `color`. The same test also runs
  a baseline render with no provider and asserts neither key
  appears — the byte-identicality regression guard.
- [x] **WASM boundary test (native equivalent).**
  `wasm-quarto-hub-client` is `cdylib`-only — its bindings can't
  be exercised by native tests. Both branches of
  `render_page_in_project_with_attribution` converge on
  `RenderToPreviewAstRenderer::with_attribution(json)` for the
  multi-doc case, so the equivalent native contract was added as
  `render_to_preview_ast_renderer_with_attribution_surfaces_keys`
  in `crates/quarto-core/tests/render_page_in_project.rs`.
  Drives `ProjectPipeline<RenderToPreviewAstRenderer>` with
  `RenderMode::ActivePage` and asserts the resulting
  `Pass2Payload::AstJson` carries the expected keys.

Both tests were red before Phase 1 implementation. The first
red-pass cycle additionally surfaced an underlying issue not
called out in the plan: `run_pipeline` was not transferring
`stage_ctx.format_options` back to `ctx.format_options`, so the
attribution data populated inside `AstTransformsStage` was
invisible to the JSON writer that runs *outside* the pipeline.
Fixed alongside Phase 1; see "Phase 1 deviation note" below.

### Phase 1 — plumb `JsonConfig` in `render_qmd_to_preview_ast`

- [x] **Plumbed.** At `crates/quarto-core/src/pipeline.rs:835` the
  `JsonConfig` literal now reads
  `ctx.format_options.json.attribution_by_node` and
  `ctx.format_options.json.attribution_actors`, converting from
  `quarto_core::AttributionRecord` / `Identity` to
  `pampa::JsonAttributionRecord` / `JsonAttributionIdentity` at
  the crate boundary. Conversion mirrors the inline pattern in
  `wasm-quarto-hub-client/src/lib.rs` (the q2-debug WASM entry).
  When no provider was installed, both fields stay `None` and
  the JSON output is byte-identical to baseline (verified by
  the no-provider arm of the Phase 0 test).

**Phase 1 deviation note.** The plan describes the change as
"one struct-literal expansion, no new types". That alone was
insufficient. `AttributionRenderTransform` runs *inside*
`AstTransformsStage`, which receives a `StageContext` (not the
outer `RenderContext`); the transform writes to
`stage_ctx.format_options.json.*`. `run_pipeline` previously only
transferred `stage_ctx.artifacts` and `stage_ctx.resource_report`
back to the outer ctx — `format_options` was silently dropped.
The HTML pipeline doesn't notice because its writer
(`RenderHtmlBodyStage`) runs *inside* the same pipeline and reads
`stage_ctx.format_options` directly. The q2-preview JSON writer
runs *outside* the pipeline, so without the transfer the writer
saw a default `format_options` with both attribution fields
`None`. The fix adds `ctx.format_options = stage_ctx.format_options`
after the pipeline run, alongside the existing artifact /
resource-report transfers. Pre-pipeline callers don't write
`ctx.format_options`, so the overwrite is safe (audited:
`grep ctx.format_options.` across `crates/` finds no
pre-`run_pipeline` writers).

Field-name divergence from the plan's pseudocode: the actual slot
is named `attribution_by_node` (pointer-keyed map for the writer)
plus `attribution_actors`. The HTML/JSON sibling fields under
`ctx.format_options.html` use slightly different names (e.g.
`attribution_identities`); the JSON side is what mattered here.

### Phase 2 — new WASM entry point

- [x] **Builder + ctx install on `RenderToPreviewAstRenderer`.**
  Added `attribution_json: Option<String>` field and
  `with_attribution(json) -> Self` builder on
  `RenderToPreviewAstRenderer`
  (`crates/quarto-core/src/project/pass2_renderer.rs`).
  `render()` installs a `PreBuiltAttributionProvider` on the
  per-page ctx right after `RenderContext::new`, using
  `Arc::new(...)` to match the actual
  `Option<Arc<dyn AttributionSourceProvider>>` type on
  `RenderContext` (the plan's pseudocode showed `Box`, but
  q2-debug uses `Arc`; matched the existing pattern).
- [x] **WASM entry point split.** Added
  `render_page_in_project_with_attribution(path, user_grammars,
  attribution_json)` as the real implementation and converted
  `render_page_in_project(path, user_grammars)` into a one-line
  wrapper forwarding `attribution_json = None`. Single-doc branch
  installs the provider directly on `ctx`; multi-doc branch
  threads the JSON into `RenderToPreviewAstRenderer::with_attribution`.
  `render_single_doc_to_response` and
  `render_project_active_page_to_response` each gained an
  `attribution_json: Option<String>` parameter; the only callers
  passing `Some` are the new entry point's branches, every other
  caller passes `None`.

Mirror the `parse_qmd_to_ast_with_attribution` shape from the
attribution plan (its Phase 5):

```rust
#[wasm_bindgen]
pub async fn render_page_in_project_with_attribution(
    path: &str,
    user_grammars: Option<JsUserGrammars>,
    attribution_json: Option<String>,
) -> String { ... }
```

Body: identical to today's `render_page_in_project`
(`crates/wasm-quarto-hub-client/src/lib.rs:1097`) except that the
provider gets installed on the active-page `RenderContext` before
the pipeline runs. The two branches differ in *where* that install
happens, because the active-page ctx is constructed in different
places:

- **Single-doc branch** (`render_single_doc_to_response`,
  `lib.rs:1146`). The ctx is built in-line at line 1169. Install
  directly after construction:

  ```rust
  let mut ctx = RenderContext::new(...).with_options(options);
  if let Some(json) = attribution_json {
      ctx.attribution_provider =
          Some(Box::new(PreBuiltAttributionProvider::new(json)));
  }
  ```

- **Multi-doc branch** (`render_project_active_page_to_response`,
  `lib.rs:1275`). The active-page ctx is built *inside*
  `RenderToPreviewAstRenderer::render()`
  (`crates/quarto-core/src/project/pass2_renderer.rs:586`), so the
  WASM entry point can't reach it. Mirror the existing
  `RenderToHtmlRenderer::with_user_grammars` pattern (`lib.rs:1347`):
  add a builder method on the renderer, and let it install the
  provider on the ctx it constructs:

  ```rust
  // crates/quarto-core/src/project/pass2_renderer.rs
  pub struct RenderToPreviewAstRenderer {
      vfs_root: std::path::PathBuf,
      attribution_json: Option<String>,
  }

  impl RenderToPreviewAstRenderer {
      pub fn with_attribution(mut self, json: String) -> Self {
          self.attribution_json = Some(json);
          self
      }
  }

  // inside render(), right after `let mut ctx = RenderContext::new(...)`:
  if let Some(json) = self.attribution_json.clone() {
      ctx.attribution_provider =
          Some(Box::new(PreBuiltAttributionProvider::new(json)));
  }
  ```

  And at the WASM call site (`lib.rs:1330`):

  ```rust
  let mut renderer = RenderToPreviewAstRenderer::new("/.quarto/project-artifacts");
  if let Some(ref json) = attribution_json {
      renderer = renderer.with_attribution(json.clone());
  }
  ```

`render_page_in_project` becomes a thin wrapper:

```rust
pub async fn render_page_in_project(
    path: &str,
    user_grammars: Option<JsUserGrammars>,
) -> String {
    render_page_in_project_with_attribution(path, user_grammars, None).await
}
```

The wrapper shape — no extra side effects between the two entry
points — is the same byte-identicality contract the attribution plan
pins on `parse_qmd_to_ast`. Every existing caller silently routes
through the new function; a regression on the `None` branch would
break all q2-preview renders, not just attributed ones.

Multi-doc note: re-discovery from the project root (`lib.rs:1133`)
happens *before* the renderer is constructed, so the renderer's
`with_attribution` builder is the correct attachment point for the
active-page ctx. Sibling Pass-1 ctxs do not receive a provider —
sidebar / cross-doc machinery never reads `ctx.attribution_provider`,
and the TS replay only produces a payload for the active edited
doc (see Resolved question #2 below).

### Phase 3 — TS caller

Originally framed as out-of-scope, but completed in this session
to make the colored text / mouseover actually appear in the
q2-preview iframe. The minimum producer-side TS:

- [x] **WASM type interface + TS wrapper.** Added
  `render_page_in_project_with_attribution` to the
  `WasmModuleExtended` interface in
  `hub-client/src/services/wasmRenderer.ts`, plus an exported
  `renderPageInProjectWithAttribution(path, userGrammars,
  attributionJson)` TS function. The existing
  `renderPageInProject` was converted into a one-line wrapper
  that forwards `attributionJson = null`, mirroring the WASM-side
  wrapper relationship.
- [x] **q2-preview branch wiring.** Updated
  `hub-client/src/components/render/ReactPreview.tsx`'s
  `doRender` to call `renderPageInProjectWithAttribution` and
  pass `options.attributionJson` through. The same `useAttribution`
  hook that produces the q2-debug payload was already running for
  q2-preview (it's format-agnostic — keyed on
  `attributionEnabled` + `currentFile.path`), so the React side
  needed no other changes. `<Ast>` (`framework/Ast.tsx`)
  automatically picks up `astContext.attribution*` keys and
  threads them through `AttributionLookupContext`, which the
  leaf renderers consume to paint per-author backgrounds and
  tooltips — the same machinery that already serves q2-debug.

**Initial Phase 3 underestimate — consumer side was missing.**
The first pass at Phase 3 routed the attribution payload through
the new WASM entry point and assumed the consumer side was free
because `<Ast>` (from `framework/Ast.tsx`) was already wiring up
`AttributionLookupContext`. That turned out to be necessary but
not sufficient. Manual testing in the live UI showed no colored
text and no hover effect on q2-preview. Cause: only q2-debug's
`Block` / `Inline` dispatchers were calling `useNodeAttribution`
and wrapping nodes in `.q2-attr-wrap`. q2-preview's dispatchers
(`q2-preview/dispatchers.tsx`) and its document-root component
(`PreviewDocument.tsx`) had no equivalent wiring. The
`AttributionLookupContext.Provider` was populated, but nothing
in q2-preview was consuming it.

Consumer-side wiring added in a second pass:

- [x] **Shared widget moved to framework.** `attribution.tsx`
  (which exports `AttributionBadge` + `attributionStyles` +
  `formatRelativeTime`) was moved from `q2-debug/` to
  `framework/` and re-exported through `framework/index.ts`.
  `q2-debug/components.tsx`'s import path updated to
  `from '../framework'`.
- [x] **q2-preview dispatchers wrap on hit.** `Block` / `Inline`
  / `CustomBlock` / `CustomInline` in
  `q2-preview/dispatchers.tsx` now call `useNodeAttribution`
  and, on hit, wrap the dispatched output in a
  `.q2-attr-wrap` div/span with `data-sid` + inline `color`.
  Off-path the dispatchers stay byte-identical (early return
  on `!attribution`). `CustomBlock` / `CustomInline` carry the
  wrap too because q2-preview preserves CustomNodes
  (Callout/Theorem/...) that span larger source ranges than
  Pandoc primitives.
- [x] **`PreviewDocument` mounts styles + hover handlers.** When
  `AttributionLookupContext` is populated the document-root
  component injects `<style>{attributionStyles}</style>`,
  attaches event-delegated mouseover/mouseout to its outer
  div, and renders a single floating `AttributionBadge` for
  the hovered `.q2-attr-wrap[data-sid]`. Mirrors q2-debug's
  `AstRenderer`. Off-path the stylesheet and handlers are
  skipped, leaving the DOM byte-identical to today's. The
  minimal-mode branch (Fragment return) wraps in a div only
  when attribution is on, preserving byte-identicality in the
  off-path minimal case.
- [x] **q2-preview integration test.**
  `q2-preview/attribution.integration.test.tsx` mirrors the
  q2-debug counterpart: four scenarios (off path; on path
  wrapping; hover surfaces badge; missing actor identity
  falls through) against `previewRegistry`. All four pass.

Without this second pass, the Authorship toggle was visibly a
no-op for q2-preview documents in the live UI even though the
WASM entry point was correctly shipping `astContext.attribution*`
in the AST JSON.

### Phase 4 — verification

- [x] `cargo nextest run --workspace` — 8859 tests pass, 195
  skipped. Includes the two new Phase 0 tests.
- [x] `cd hub-client && npm run build:all` — WASM build +
  TypeScript build both succeed. Hub-client `npm run test:ci`
  also passes (84 tests).
- [ ] **Browser end-to-end check not exercised — no browser
  available in this environment.** Per `CLAUDE.md` ("End-to-end
  verification before declaring success" → "If you cannot test a
  feature end-to-end (e.g. no access to a browser for a
  hub-client change), say so explicitly"), this is reported
  rather than claimed. Layered coverage substitutes:
  - Pipeline-level:
    `render_qmd_to_preview_ast_surfaces_attribution_when_provider_installed`
    pins the JSON wire format.
  - Renderer-level:
    `render_to_preview_ast_renderer_with_attribution_surfaces_keys`
    pins the orchestrator path through
    `RenderToPreviewAstRenderer::with_attribution`.
  - Hub-client `npm run test:ci` — 84 tests pass including the
    existing q2-debug attribution integration test; q2-preview
    consumes the same `<Ast>` + `AttributionLookupContext`
    machinery, so the consumer-side rendering is shared.

  The user should still open a q2-preview document with the
  Authorship toggle on in the live UI to confirm visually.
- [x] **Byte-identicality spot check** is covered by the no-provider
  baseline arm of the Phase 0 pipeline test.

## Out of scope

- HTML inline `data-attr-*` for q2-preview. q2-preview emits AST JSON,
  not HTML; the inline form is only for the HTML CLI path. The
  AST-iframe React renderer consumes the JSON's
  `astContext.attribution*` table directly.
- q2-slides. Slated to migrate to the q2-preview pipeline pattern
  in a future plan (Plan 1 §"Decision A"); attribution comes along
  for free once that migration lands.
- Editing attribution in q2-preview. q2-preview is read-only in
  v1 (Plan 1 §"q2-preview routing").
- Linking `automerge-rs` into wasm-quarto-hub-client (Option B in
  the attribution plan). Locked to Option A for v1.

## Resolved questions

Both questions had implicit answers in earlier drafts. Locking
them in here so the implementer doesn't have to re-derive the
rationale, and so a future v2 has a written record of what was
considered.

### 1. WASM entry shape — two functions, not one with `Option<String>`

**Decision:** ship `render_page_in_project_with_attribution(path,
user_grammars, attribution_json)` as the real implementation and
keep `render_page_in_project(path, user_grammars)` as a one-line
wrapper that forwards `None`. Same shape as
`parse_qmd_to_ast` / `parse_qmd_to_ast_with_attribution` (the
attribution plan's Phase 3b).

Rationale:

- **Byte-identicality at the function boundary.** Existing TS
  callers don't pass `attributionJson` and never learn of the new
  argument. Any regression on the `attribution_json = None` path
  immediately breaks *all* q2-preview renders (not just attributed
  ones), so the wrapper itself is the regression alarm. A single
  `Option<String>` parameter would still satisfy the contract, but
  it offers no extra protection and forces every TS call site to
  thread an explicit `null`/`undefined`.
- **Symmetry with `parse_qmd_to_ast_*`.** The two WASM surfaces
  (q2-debug and q2-preview) become directly comparable: same naming
  convention, same wrapper relationship, same byte-identicality
  story. A reader who understands one understands the other.
- **wasm-bindgen ergonomics.** Two clearly-named exports are easier
  to grep, type, and discover from TS than a third optional argument
  whose null/undefined/string semantics aren't statically obvious
  at the boundary.
- **Zero churn for existing TS callers.** `Editor.tsx`,
  `ReactPreview.tsx`, `PreviewRouter.tsx` etc. keep calling
  `render_page_in_project(path, grammars)` unchanged. The
  Authorship-on q2-preview branch (Phase 3) is the *only* new
  caller, and it calls the `_with_attribution` shim directly.

The only conceivable reason to diverge would be if `Option<String>`
parameter handling at the wasm-bindgen boundary became materially
cheaper than a wrapper call, which is not the case today.

### 2. No provider plumbing during Pass-1

**Decision:** the multi-doc branch installs the provider only on
the Pass-2 active-page ctx (via
`RenderToPreviewAstRenderer::with_attribution`, see Phase 2).
Sibling Pass-1 ctxs receive no provider and produce no attribution.

Rationale:

- **The generate stage already no-ops without a provider.**
  `AttributionGenerateTransform`'s skip ladder (attribution plan
  Phase 2, rule 3) bails on `ctx.attribution_provider.is_none()`.
  So even if attribution-generate ran during sibling Pass-1 (it
  would, since the stage registers in the Navigation Phase tail
  of `build_transform_pipeline`), the stage exits immediately with
  zero work. No special-casing required.
- **No Pass-1 consumer reads attribution.** Sidebar, navbar,
  cross-doc link rewriting, and the profile checkpoint all read
  `ProjectIndex` / `DocumentProfile` data; none reads
  `ctx.attribution_data`. The output of `AttributionRenderTransform`
  is consumed only by the JSON/HTML writers, and Pass-1 doesn't
  invoke either writer (it produces profiles, not rendered output).
- **No data to ship anyway.** The TS replay
  (`useAttribution.ts`, attribution plan Phase 5) produces a JSON
  payload only for the *active edited document* — Automerge
  history is per-doc, and the hub-client only opens history for
  the file currently in the editor. There is no
  sibling-doc-attribution payload to plumb.
- **Future cross-doc author features belong on `ProjectIndex`.**
  A "show contributor list per page in the sidebar" feature would
  not extend the Pass-1 provider plumbing; it would add a field to
  `ProjectIndex` populated by a project-scope stage that aggregates
  identities. That's a v2 plan, not a v1 expansion of this one.

Reopening this would require both (a) a TS source that produces
sibling-doc attribution payloads and (b) a downstream consumer
beyond the writers. Neither exists today.

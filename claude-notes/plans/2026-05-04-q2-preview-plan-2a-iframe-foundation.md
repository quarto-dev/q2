# Plan 2A — q2-preview iframe foundation

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (resolved during the 2026-05-06 review session)
**Milestone:** M2-foundation (iframe is ready to host type-specific React components)

## Goal

Land the iframe-side plumbing that makes q2-preview ready to host the
type-specific React components shipped in Plan 2B. After 2A:

- Theme CSS produced by `CompileThemeCssStage` reaches the AST iframe
  via the same `data:` URI rewrite pattern the HTML iframe uses today.
- Page-scoped image artifacts produced by `ResourceCollectorTransform`
  reach the iframe the same way.
- Documents with `format: q2-preview` can use a `render-components: [...]`
  YAML key to load custom `.tsx` files (the gate in `ReactRenderer.tsx`
  is currently q2-debug-only).
- The source-info pool emitted by the JSON writer is parsed, typed, and
  threaded through the iframe via React context, ready for Plan 2B's
  atomic-aware `setLocalAst` gating to consume it.
- `hub-client/src/utils/atomicCustomNodes.ts` ships as the JS-side
  hand-mirror of Plan 7's atomic registry, with the initial built-in
  set (`["CrossrefResolvedRef"]`).
- The four duplicate `PandocAST` definitions in `hub-client/src/components/render/`
  are consolidated into `hub-client/src/types/pandoc.ts`, and the dead
  `ReactAstRenderer.tsx` is removed.
- `ast-renderer.html`'s inline `<style>` is rewritten with `:where()` so
  theme CSS can override it without source-order coincidence.

No new visible UI ships in 2A. CustomNodes still render as the bare
`__quarto_custom_node` wrapper Divs the iframe receives today —
visually identical to the post-Plan-1 state for that markup, but with
theme CSS and image rendering working around them. **2B is what makes
those wrapper Divs render as Callouts, Theorems, etc.**

## Scope

### In scope

- **`render-components` YAML key gate extension** in
  `hub-client/src/components/render/ReactRenderer.tsx:103`. Today the
  gate is `if (format !== 'q2-debug') { return ''; }`; q2-preview
  gets added so that demos using `format: q2-preview` can specify
  custom `.tsx` files in the same way. ~5 LOC + a regression test.
- **Shared VFS asset rewriter**
  (`hub-client/src/utils/iframeAssetRewriter.ts`, new file). Factor
  the `<link rel="stylesheet">` and `<img src>` data-URI rewrite logic
  out of `iframePostProcessor.ts:130-216` into a function that takes
  a `Document` and walks it. The HTML iframe's existing
  post-processor calls it; the AST iframe gains a new
  `useEffect` in `ast-renderer-entry.tsx` (after `root.render()`)
  that calls the same function against `iframe.contentDocument`.
  Both call sites stay duplicated-by-design until the service-worker
  resource-resolution work lands (per `iframePostProcessor.ts:24`)
  — at which point both call sites delete together.
- **Source-info pool TS type mirror**
  (`hub-client/src/types/sourceInfo.ts`, new file). Mirrors the wire
  format defined in `crates/pampa/src/writers/json.rs:54-91`:
  ```ts
  export interface By { kind: string; data?: unknown }
  export type SourceInfoEntry =
    | { t: 0; r: [number, number]; d: number }
    | { t: 1; r: [number, number]; d: number }
    | { t: 2; r: [number, number]; d: Array<[number, number, number]> }
    | { t: 3; r: [number, number]; d: [string, number] }
    | { t: 4; r: [0, 0];           d: By }
    | { t: 5; r: [0, 0];           d: { from: number; by: By } };
  export type SourceInfoPool = readonly SourceInfoEntry[];
  export interface AstContext {
    files: Array<{ name: string; lineBreaks?: number[]; totalLength?: number }>;
    metaTopLevelKeySources?: unknown;
    sourceInfoPool?: SourceInfoPool;
  }
  ```
  Codes 4 and 5 are dormant on the wire today — Plan 5 wires them up
  when it lands. The TS type already accepts them so 2A doesn't need
  amendment when Plan 5 ships.
- **Source-info accessor module**
  (`hub-client/src/utils/sourceInfo.ts`, new file). Pure functions,
  no React:
  - `entryFor(node, pool): SourceInfoEntry | undefined` — looks up
    a node's pool entry by its `s` field.
  - `isDerived(node, pool): boolean` — returns true iff the entry
    is type code 5. Plan 6's shortcode resolutions populate Derived.
    Until Plan 6 lands, this never fires.
  - `isAtomicSourceInfo(node, pool, atomicKinds): boolean` — true
    iff `isDerived` OR `(entry.t === 4 && atomicKinds.has(entry.d.kind))`.
    The `atomicKinds` set is empty in 2A; Plan 4 introduces atomic
    `By` kinds and Plan 6 emits them.
  - `ATOMIC_SYNTHETIC_KINDS: ReadonlySet<string>` — exported empty
    set today, with a comment pointing at Plan 4's `By::is_atomic_synthesizer()`
    for the synchronization contract. Plan 4 / 6 fill it.
- **Extend `RegistryContext`** in `hub-client/src/components/render/ReactAstDebugRenderer.tsx`
  to carry `sourceInfoPool?: SourceInfoPool` alongside `registry`.
  The `<Ast>` component reads `astContext?.sourceInfoPool` from the
  parsed AST and provides it. 2A consumers don't read it yet
  (`Block` / `Inline` dispatchers stay unchanged); 2B's atomic-aware
  gating consumes it.
- **`hub-client/src/utils/atomicCustomNodes.ts`** (new file).
  Hand-mirror of Plan 7's `crates/quarto-core/src/.../ATOMIC_CUSTOM_NODES`
  Rust const, owned by 2A because 2A is the first consumer
  (Plan 2B's atomic-aware dispatcher reads it; Plan 7 ships the
  Rust counterpart later). Initial built-in set:
  `["CrossrefResolvedRef"]`. Plan 8 amends this file to add
  `"IncludeExpansion"`. Header comment names the Rust source of
  truth and the sync convention (matches `types/diagnostic.ts` ↔
  `DiagnosticMessage` and `types/intelligence.ts` ↔ `quarto-lsp-core`).
- **PandocAST type consolidation**: pull the four duplicate
  `PandocAST` / `BlockNode` / `InlineNode` definitions from
  `ReactAstRenderer.tsx` (dead — see "Out of scope" rationale
  inverted), `ReactRenderer.tsx`, `ReactAstSlideRenderer.tsx`,
  `ReactAstDebugRenderer.tsx` into a single `hub-client/src/types/pandoc.ts`.
  Add the `astContext?: AstContext` field. The consolidated type
  also includes **placeholder discriminants for CustomBlockNode
  (`t: 'CustomBlock'`) and CustomInlineNode (`t: 'CustomInline'`)**
  in the `BlockNode` / `InlineNode` unions — Plan 2B's
  `unwrapCustomNodes` walk produces these at render time but the
  shapes are pre-declared so 2B doesn't have to re-edit foundational
  types. Three live consumers (`ReactRenderer.tsx`,
  `ReactAstSlideRenderer.tsx`, `ReactAstDebugRenderer.tsx`) update
  to import from the new file. The dead `ReactAstRenderer.tsx`
  is **deleted** (zero importers).
- **`ast-renderer.html` style fix**: wrap the existing inline `<style>`
  body rule in `:where()` to drop its specificity to 0,0,0:
  ```html
  <style>
    :where(body) {
      margin: 0; padding: 0;
      font-family: -apple-system, BlinkMacSystemFont, ...;
      -webkit-font-smoothing: antialiased;
      -moz-osx-font-smoothing: grayscale;
    }
    #root { width: 100%; height: 100vh; overflow: auto; }
  </style>
  ```
  Properties: q2-debug / q2-slides (no theme CSS loaded) keep their
  system-font reset because the `:where(body)` rule is the only one
  targeting `body`. q2-preview's loaded Bootstrap (`body { font-family: ... }`,
  spec 0,0,1) cleanly defeats the `:where()` rule (spec 0,0,0)
  regardless of source order. `#root` stays unwrapped because it's
  iframe-private and theme CSS doesn't touch it.

### Out of scope (deferred to Plan 2B)

- Type-specific React components for the seven CustomNode types
  (Callout, Theorem, Proof, FloatRefTarget, Equation,
  CrossrefResolvedRef, IncludeExpansion-stub).
- The `unwrap` / `rewrap` walks (`hub-client/src/utils/customNode.ts`).
- Pandoc base-type gap fills in `html.tsx` (LineBlock, DefinitionList,
  Table family, Underline, Strikeout, Superscript, Subscript,
  SmallCaps, Cite, RawInline, Note).
- Atomic-aware `setLocalAst` gating in `Block` / `Inline` dispatchers
  (the formerly-named `MaybeReadOnlyInline`).
- Class-name constants module mirroring Rust's class taxonomy.
- Component snapshot tests, round-trip property tests, generic
  fallback tests.

### Out of scope (deferred to a future "q2-preview layout chrome" plan)

The HTML pipeline runs `SidebarRenderTransform`,
`NavbarRenderTransform`, `FooterRenderTransform`,
`PageNavRenderTransform`, and `TocRenderTransform` to produce HTML
strings for page chrome; q2-preview's pipeline excludes all five
(Plan 1). The structured *Generate* metadata reaches React but isn't
rendered as page chrome. **The following HTML-pipeline behaviors are
not yet replicated in q2-preview**:

- **Sidebar body-class derivation**: `SidebarRenderTransform` adds
  layout classes (`docs-sidebar-{none,floating,docked}`, etc.) to
  the `<body>` element. q2-preview's iframe `<body>` does not yet
  receive these classes.
- **Navbar brand-title fallback**: `navbar.title || website.title || document.title`
  resolution. Done in Rust during NavbarRender; not surfaced.
- **Sidebar / Navbar / Footer / PageNav / TOC rendering**: all five
  render transforms produce styled HTML chrome that q2-preview elides.
- **Page-nav strip**: previous/next navigation links between pages.

Until the "q2-preview layout chrome" plan lands, q2-preview renders
the document body only. The original Plan 2's "JS reimpl: sidebar
body-classes, navbar brand-fallback ~30 LOC" item was discussed in
the 2026-05-06 review session and deferred — implementing the
metadata-derivation utilities ahead of their consumers would leave
dormant ~25 LOC of unused code.

## User-visible state after 2A lands

q2-preview's runtime behavior changes in three observable ways
between today's "post-Plan-1, pre-Plan-2A" state and "post-Plan-2A,
pre-Plan-2B":

1. **Theme CSS is applied.** Documents with `theme: flatly` (or any
   theme) render with the compiled Bootstrap + theme CSS. Typography,
   colors, spacing match the HTML format for the document body.
2. **Images render.** `<img>` elements pointing at
   `/.quarto/project-artifacts/<stem>_files/...` resolve to the
   in-VFS image and display correctly.
3. **Custom `.tsx` files load** for `format: q2-preview` documents
   when listed under the `render-components: [...]` YAML key. Pasting
   Elliot's existing `html.tsx` into a q2-preview demo produces a
   visibly different render — the wrapper Divs still pass through as
   wrapper Divs, but the surrounding paragraphs use the user's
   styled components.

Things that **don't** change in 2A:

- CustomNodes still render as `<div class="__quarto_custom_node">`
  boxes. The user sees the wrapper class as a styled Div until 2B's
  unwrap + type-specific components ship.
- Edit-back is still read-only (Plan 7 lifts that guard).
- No new layout chrome (sidebar / navbar / footer / TOC / page-nav).

This is **strictly better than today** — no styling regression, two
new affordances — and is a natural pause point for manual QA before
2B lands.

## Design decisions (settled in conversation, 2026-05-06 review)

- **`:where()` over per-format style branching**. Considered: two
  HTML files (`ast-renderer.html` for debug, `ast-renderer-preview.html`
  for q2-preview) or JS-side conditional `<style>` injection.
  Rejected: both add structural moving parts to achieve what
  specificity demotion does in one line. `:where(body)` cleanly
  loses against any user theme rule at spec ≥ 0,0,1 regardless of
  source order, while still applying when no theme CSS is loaded.
- **Asset rewrite via post-render DOM walk, not AST-walk**. The
  shared helper takes a `Document` and rewrites `<link>` / `<img>`
  in place. Mirrors the existing HTML iframe pattern exactly,
  enabling code share and identical removal when service-worker
  resource resolution lands. AST-walk alternative was discussed
  and rejected (would require re-rewriting on every change and
  diverges from the proven HTML-iframe path).
- **`atomicCustomNodes.ts` ownership moves from Plan 7 to 2A**.
  Plan 7's original §"is_atomic_custom_node registry" decision
  named the file but assumed Plan 7 ships it. Plan 2B is the first
  consumer (atomic-aware dispatcher), so 2A absorbs ownership.
  Plan 7 still ships the Rust side (`ATOMIC_CUSTOM_NODES` const +
  `is_atomic_custom_node()` function). The TS file's header comment
  documents this and points at the Rust source for the sync
  contract. Plan 8 amends the file to add `"IncludeExpansion"`.
- **PandocAST consolidation lands in 2A, not 2B**. The motivation
  is forward-compat for 2B: 2B's `unwrapCustomNodes` walk produces
  `CustomBlockNode` / `CustomInlineNode` shapes, which need to be
  in the `BlockNode` / `InlineNode` unions for type-checking. If
  2A ships the consolidated types with placeholder discriminants
  for these shapes, 2B doesn't have to re-edit foundational types.
  Cost in 2A: ~10 extra LOC of placeholder declarations whose
  runtime constructors don't exist until 2B.
- **Dead-code deletion bundled here**. `ReactAstRenderer.tsx` is
  unimported anywhere in the tree (verified via grep). It's a
  near-duplicate of `ReactAstDebugRenderer.tsx`. Cleanup is bundled
  into 2A's consolidation pass because both touch the same file
  set and consolidating the type definitions is cleaner with the
  dead file gone.

## Soft activation dependencies

2A lands inert wiring that activates organically as later plans
land:

- **Plan 4** introduces the `Synthetic { by: By }` and
  `Derived { from, by }` SourceInfo variants. 2A's accessor
  recognizes wire codes 4 and 5 already; until Plan 4 / 5 wire
  them up, no entry has those codes.
- **Plan 5** adds wire format codes 4 and 5 to the JSON writer.
  After Plan 5, the codes start appearing in the pool. 2A's
  accessor handles them. Plan 2B consumes via the dispatcher
  modification.
- **Plan 6** populates Derived source_info on shortcode
  resolutions. After Plan 6, individual inlines start having
  `t: 5` source-info entries. 2A's `isDerived` accessor returns
  true for them; until Plan 2B's dispatcher consumes the value,
  nothing visible happens.
- **Plan 7** ships the Rust `ATOMIC_CUSTOM_NODES` const +
  `is_atomic_custom_node()` function. The TS hand-mirror in 2A
  is the JS side of the same data; the two sides stay in sync
  via the file header comment + code review. 2A's
  `["CrossrefResolvedRef"]` is correct from the day Plan 1
  shipped (CrossrefResolveTransform is in Plan 1's transform list).
- **Plan 8** introduces `"IncludeExpansion"` CustomNode and
  amends `atomicCustomNodes.ts` to add it. 2A's file structure
  accepts the amendment without additional rework.

## Multi-plan contracts

### Consumed: theme CSS artifact (from Plan 1)

Plan 1's `RenderToPreviewAstRenderer` writes the compiled theme
CSS to `/.quarto/project-artifacts/styles.css` (per
`pipeline.rs::DEFAULT_CSS_ARTIFACT_PATH`) on every q2-preview
render. 2A's iframe asset rewriter resolves this VFS path to a
`data:text/css;base64,...` URI in the iframe's DOM, mirroring
`iframePostProcessor.ts:137-147`. The Rust→VFS contract from
Plan 1 is unchanged; 2A is the first reader.

### Consumed: page-scoped image artifacts (from Plan 1)

Plan 1's renderer also writes page-scoped artifacts (images via
`ResourceCollectorTransform`) to `/.quarto/project-artifacts/<stem>_files/`.
2A's iframe asset rewriter resolves `<img src>` referencing those
paths to `data:` URIs, mirroring `iframePostProcessor.ts:177-210`.
The contract is symmetric to the theme-CSS contract — Plan 1
writes; 2A reads.

### Provided: source-info pool accessor (for Plan 2B and beyond)

2A ships typed access to the source-info pool:
- `types/sourceInfo.ts` for the wire-format types.
- `utils/sourceInfo.ts` for the accessor functions.
- `RegistryContext` extension for in-iframe distribution.

Plan 2B's atomic-aware dispatcher reads these. Future features
(preimage navigation, source-mapped diagnostics in the iframe)
can also build on the same accessors.

### Provided: atomicCustomNodes hand-mirror (for Plan 2B and Plan 7)

2A ships `hub-client/src/utils/atomicCustomNodes.ts` with the
initial built-in set. Plan 2B's atomic-aware dispatcher imports
`isAtomicCustomNode(typeName)` from this file. Plan 7 ships the
Rust counterpart and the sync convention is documented in 2A's
file header comment.

## References

### Rust side (read-only — 2A doesn't modify Rust)

- `crates/pampa/src/writers/json.rs:54-91` — wire format types
  (AstContextJson, SourceInfoJson, NodeJson, etc.).
- `crates/pampa/src/writers/json.rs:300-330` — `add_source_info`
  on each node.
- `crates/pampa/src/writers/json.rs:1297` — `write_custom_block`
  (block CustomNodes wrapped as Div).
- `crates/pampa/src/writers/json.rs:1380` — `write_custom_inline`
  (inline CustomNodes wrapped as Span).
- `crates/quarto-source-map/src/source_info.rs:22-55` — SourceInfo
  enum (extended by Plan 4).
- `crates/quarto-core/src/pipeline.rs::DEFAULT_CSS_ARTIFACT_PATH`
  — VFS path for the theme CSS artifact.

### hub-client side

- `hub-client/src/components/render/ReactRenderer.tsx:101-111` —
  `render-components` gate (q2-preview added by 2A).
- `hub-client/src/components/render/ReactRenderer.tsx:148` —
  format dispatch for AstIframe (q2-debug + q2-preview both route
  through here today; unchanged by 2A).
- `hub-client/src/components/render/ReactAstDebugRenderer.tsx` —
  `RegistryContext` definition; consolidate `PandocAST` here too.
- `hub-client/src/components/render/AstIframe.tsx` — postMessage
  protocol (unchanged by 2A).
- `hub-client/src/ast-renderer-entry.tsx` — iframe entry; 2A adds
  the asset-rewriter useEffect after `root.render()`.
- `hub-client/public/ast-renderer.html:7-22` — inline `<style>`
  to wrap with `:where()`.
- `hub-client/src/utils/iframePostProcessor.ts:130-216` — source
  for the asset-rewrite logic to extract.
- `hub-client/src/components/render/ReactAstRenderer.tsx` —
  dead file to delete.
- `hub-client/src/types/diagnostic.ts`,
  `hub-client/src/types/intelligence.ts`,
  `hub-client/src/utils/pipelineKind.ts` — existing TS↔Rust mirror
  patterns to follow.

## Test plan

- **Source-info accessor unit tests**: build representative
  `astJson` strings containing each wire code (0–5), parse them,
  assert `entryFor` / `isDerived` / `isAtomicSourceInfo` return
  correct values. Codes 4–5 use hand-constructed JSON until Plan 5
  ships writer support.
- **Source-info pool integration test**: render a fixture through
  q2-preview's pipeline, parse the resulting `astJson`, assert
  `astContext.sourceInfoPool` is non-empty and well-formed.
- **`render-components` gate regression test** (vitest): mount
  `ReactRenderer` with `format: q2-preview` and a
  `render-components: [foo.tsx]` AST; assert `customComponentsCode`
  is populated (today's behavior is empty for non-debug formats).
  Sibling regression test for q2-debug confirms behavior unchanged.
- **Asset rewriter unit tests**: build a representative `Document`
  with `<link>` and `<img>` elements pointing at `/.quarto/...`
  paths, mock VFS reads, run the helper, assert the resulting DOM
  has `data:` URIs in place of the original paths.
- **Asset rewriter integration test (HTML iframe)**: existing
  iframe post-processor tests (`iframePostProcessor.test.ts`,
  `iframePostProcessor.integration.test.ts`) should continue to
  pass after the helper extraction. Treats the refactor as
  behavior-preserving.
- **Asset rewriter integration test (AST iframe)**: render an
  AST containing an `<img src="/.quarto/project-artifacts/foo.png">`,
  populate the VFS with a fake image, mount the iframe, assert the
  rendered `<img>` has a `data:` URI src.
- **`:where()` style regression test**: render q2-debug content
  in the iframe, assert the body computed style still applies the
  system-font reset. Render q2-preview content with theme CSS
  loaded, assert Bootstrap's font-family wins. Either two snapshot
  tests or one DOM-inspection test — pick whichever fits the
  existing iframe-test patterns.
- **PandocAST consolidation build-pass**: `npm run build:all`
  succeeds after the consolidation. `npm run test:ci` passes for
  hub-client. `cargo xtask verify --skip-rust-tests` succeeds end-to-end.
- **`atomicCustomNodes.ts` smoke test**: assert
  `isAtomicCustomNode('CrossrefResolvedRef')` returns `true`,
  `isAtomicCustomNode('Callout')` returns `false`. The list itself
  is the test's source of truth — when Plan 8 adds
  `"IncludeExpansion"`, the test gets one new assertion.

## Dependencies

### Hard dependencies

- **Plan 1** — pipeline, format detection, `RenderResponse.ast_json`,
  `pipeline_kind` dispatch, theme-CSS / page-scoped-image VFS
  contracts. All shipped (commits `fcc5ea4b…a5e00b20`).

### Blocks

- **Plan 2B** — type-specific component renderers. 2B consumes
  every artifact 2A ships (PandocAST consolidation, source-info
  accessor, atomicCustomNodes.ts, asset rewriter). 2B cannot land
  before 2A.
- Independent of Plans 4 / 5 / 6 / 7 / 8 — they extend the writer
  / type system / wire format. 2A's source-info wiring is forward-
  compatible with all of them.

## Risk areas

- **`iframePostProcessor.ts` refactor regression**. The helper
  extraction must be behavior-preserving for the HTML iframe.
  Mitigation: the existing `iframePostProcessor.test.ts` and
  `iframePostProcessor.integration.test.ts` suites pass before
  and after. Don't change extraction shape mid-refactor.
- **`PandocAST` consolidation type drift**. The four duplicate
  definitions have drifted slightly (the `ReactRenderer.tsx`
  variant is the most spartan; `ReactAstDebugRenderer.tsx` has
  the richest type tree). Use the richest one as the canonical
  shape, plus the new `astContext?` field, plus the
  `CustomBlockNode` / `CustomInlineNode` placeholders. Run
  `tsc -b` after each consumer's import update.
- **`render-components` gate change visibility**. The current
  one-line gate is buried in a `useMemo`; easy to miss when
  reading the diff. Add a comment explaining the gate's
  semantics now that q2-preview is also covered.
- **`:where()` browser support**. Modern (Chrome 88+, Firefox 78+,
  Safari 14+, all 2021 or earlier). Hub-client targets evergreen
  browsers; this is fine. If a baseline-browser concern surfaces,
  the alternative is splitting `ast-renderer.html` (rejected
  above) or removing the body rule entirely (acceptable but
  causes UA-default 8px body margin in q2-debug).

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `render-components` gate extension + regression test | ~20 |
| `iframeAssetRewriter.ts` extraction + caller updates | ~120 |
| AST-iframe asset-rewriter useEffect + integration test | ~50 |
| `types/sourceInfo.ts` (mirror types) | ~50 |
| `utils/sourceInfo.ts` (accessors + tests) | ~120 |
| `RegistryContext` extension + AST entry threading | ~30 |
| `utils/atomicCustomNodes.ts` (TS hand-mirror) | ~30 |
| `types/pandoc.ts` consolidation + 3-consumer migration + delete dead file | ~150 (net negative after deletion) |
| `ast-renderer.html` `:where()` wrap + regression test | ~30 |
| **Total** | **~600** |

Likely fits in one focused implementation session. The
asset-rewriter extraction is the highest-effort item; the rest is
mechanical.

## Notes

- This plan replaces the foundation half of the original Plan 2
  (`2026-05-04-q2-preview-plan-2-builtin-components.md`), which
  was split into 2A (foundation) + 2B (components) during the
  2026-05-06 review session. The split was driven by scope
  realism: research raised the original ~970 LOC estimate to
  ~1415 LOC and added items (source-info plumbing, asset-rewriter
  share, render-components gate, `:where()` fix) that are
  logically separable from the type-specific component work.
- The "rename `MaybeReadOnlyInline`" from the original Plan 2 is
  resolved in 2B: there's no such wrapper component; the atomic-
  aware `setLocalAst` gating folds into the existing `Block` /
  `Inline` dispatchers. 2A ships the prerequisites
  (`isAtomicSourceInfo`, `atomicCustomNodes.ts`); 2B ships the
  consumers.
- Forward-compat dormancy is the explicit pattern for 2A's
  source-info wiring vs. Plan 5 wire-format codes 4/5. The plan-7
  cleanup pattern (Plan 1's `pipeline_kind` field landed dormant
  for Plan 7 to consume) is the same idea.

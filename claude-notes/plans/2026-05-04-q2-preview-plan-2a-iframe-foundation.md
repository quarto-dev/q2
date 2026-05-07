# Plan 2A — q2-preview iframe foundation

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (resolved during the 2026-05-06 review session)
**Milestone:** M2-foundation (iframe is ready to host type-specific React components)

## Goal

Land the iframe-side plumbing that makes q2-preview ready to host the
type-specific React components shipped in Plan 2B. After 2A:

- Theme CSS produced by `CompileThemeCssStage` reaches the AST iframe
  by inlining the VFS bytes into a `<style>` in `document.head` at
  iframe init. The HTML iframe's `<link>`-rewrite pattern doesn't
  carry over: the AST iframe has no `<link>` to rewrite (Pandoc nodes
  don't produce stylesheet links), so one-shot inline injection is
  the natural alternative until service-worker resource resolution
  lands.
- Page-scoped image artifacts produced by `ResourceCollectorTransform`
  reach the iframe via a DOM walk that swaps `<img src>` for `data:`
  URIs sourced from the VFS — same pattern as the HTML iframe, but
  using the project-relative branch of the existing rewriter (image
  paths in q2-preview's AST are user-written paths like `hero.png`,
  not `/.quarto/...`; see §"Multi-plan contract: page-scoped image
  artifacts" for why).
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

The list below is in **implementation order**. Items 1–5 are the
type / data foundation (most consumed by 2B and by later items in
this plan); items 6–7 are independent one-liners; items 8–9 are
behavior-preserving HTML-iframe refactors; item 10 is the AST-iframe
wrapper that consumes 8 and 9.

#### 1. Source-info pool TS type mirror

`hub-client/src/types/sourceInfo.ts`, new file. Mirrors the wire
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
amendment when Plan 5 ships. Defining `AstContext` here (not in
`pandoc.ts`) makes the dependency direction `pandoc.ts → sourceInfo.ts`,
which matches consumption order downstream.

**`By` is intentionally coarse in 2A** — `{ kind: string; data?: unknown }`
mirrors Plan 4's open Rust struct (`{ kind: String, data: serde_json::Value }`)
and accepts every kind 4 / 5 / 6 / 7 / 8 will produce. Plan 4
introduces specific builder methods per kind (`By::filter`,
`By::sectionize`, `By::shortcode`, `By::include`, etc.) with
distinct `data` shapes. Once those land and consumers branch on
`kind`, this TS type becomes a candidate for narrowing to a
discriminated union (`type By = { kind: 'filter'; data: { filter_path: string; line: number } } | { kind: 'sectionize' } | ...`).
Not in 2A's scope; flagged here so the future refinement isn't
forgotten. Until then, kind-specific consumers (e.g. preimage
navigation) cast `data` to the expected shape locally.

#### 2. PandocAST type consolidation

Pull the four duplicate `PandocAST` / Block / Inline definitions
from `ReactRenderer.tsx`, `ReactAstRenderer.tsx` (dead),
`ReactAstSlideRenderer.tsx`, and `ReactAstDebugRenderer.tsx` into a
single `hub-client/src/types/pandoc.ts`. **Naming**: adopt
`BlockNode` / `InlineNode` as the canonical names (the richest
existing definition is `ReactAstDebugRenderer.tsx`'s, which uses
these names; `Block`/`Inline` are too generic for grep). The
slide-side files currently export `Block` / `Inline` and have three
external importers (`RevealjsReactAstSlideRenderer.tsx`,
`hooks/useCursorToSlide.ts`, `hooks/useSlideThumbnails.tsx`) — all
three migrate to `BlockNode` / `InlineNode` from `types/pandoc.ts`
in the same pass; no compat re-export retained.

Add `astContext?: AstContext` to the consolidated `PandocAST`
(import from item 1). The type also includes **placeholder
discriminants for CustomBlockNode (`t: 'CustomBlock'`) and
CustomInlineNode (`t: 'CustomInline'`)** in the `BlockNode` /
`InlineNode` unions — Plan 2B's `unwrapCustomNodes` walk produces
these at render time but the shapes are pre-declared so 2B doesn't
have to re-edit foundational types.

**Six consumers update** to import from the new file:
`ReactRenderer.tsx`, `ReactAstSlideRenderer.tsx`,
`ReactAstDebugRenderer.tsx`, `RevealjsReactAstSlideRenderer.tsx`,
`hooks/useCursorToSlide.ts`, `hooks/useSlideThumbnails.tsx`. The
dead `ReactAstRenderer.tsx` is **deleted** (zero importers).
This item touches the broadest file set in 2A; landing it second
(after the foundational types) keeps later items rebasing cleanly.

#### 3. Source-info accessor module

`hub-client/src/utils/sourceInfo.ts`, new file. Pure functions, no
React:

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
  set today, with a comment pointing at Plan 4's
  `By::is_atomic_synthesizer()` for the synchronization contract.
  Plan 4 / 6 fill it.

#### 4. `atomicCustomNodes.ts`

`hub-client/src/utils/atomicCustomNodes.ts`, new file. Hand-mirror
of Plan 7's `crates/quarto-core/src/.../ATOMIC_CUSTOM_NODES` Rust
const, owned by 2A because 2A is the first consumer (Plan 2B's
atomic-aware dispatcher reads it; Plan 7 ships the Rust counterpart
later). Initial built-in set: `["CrossrefResolvedRef"]`. Plan 8
amends this file to add `"IncludeExpansion"`. Header comment names
the Rust source of truth and the sync convention (matches
`types/diagnostic.ts` ↔ `DiagnosticMessage` and
`types/intelligence.ts` ↔ `quarto-lsp-core`).

#### 5. Extend `RegistryContext`

In `hub-client/src/components/render/ReactAstDebugRenderer.tsx`,
extend `RegistryContext` to carry `sourceInfoPool?: SourceInfoPool`
(from item 1) alongside `registry`. The `<Ast>` component reads
`astContext?.sourceInfoPool` (from item 2's consolidated type) and
provides it. 2A consumers don't read it yet — the existing
`registry[node.t]` lookups inside `<Ast>` stay unchanged. 2B's
atomic-aware `setLocalAst` gating folds into those registry lookups
(or into a new dispatcher component introduced by 2B; the exact
shape is 2B's call). 2A only ships the data source.

#### 6. `ast-renderer.html` style fix

Wrap the existing inline `<style>` body rule in `:where()` to drop
its specificity to 0,0,0,0:

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
targeting `body`. q2-preview's loaded Bootstrap reboot (`body
{ font-family: var(--bs-body-font-family); ... }` at
`resources/scss/bootstrap/dist/scss/_reboot.scss:49-60`, plus
Quarto's own `body { margin: 0 }` at `_bootstrap-rules.scss:27`,
both at spec 0,0,0,1) cleanly defeats the `:where()` rule
(spec 0,0,0,0). `#root` stays unwrapped because it's iframe-private
and theme CSS doesn't touch it.

#### 7. `render-components` YAML key gate extension

In `hub-client/src/components/render/ReactRenderer.tsx:103`. Today
the gate is `if (format !== 'q2-debug') { return ''; }`; q2-preview
gets added so that demos using `format: q2-preview` can specify
custom `.tsx` files in the same way. ~5 LOC + a regression test.

#### 8. Image rewriter helper

`hub-client/src/utils/iframeImageRewriter.ts`, new file. Extract
only the `<img>` rewrite block from `iframePostProcessor.ts:177-210`
into `rewriteImages(doc: Document, opts: { currentFilePath: string })`.
Preserves both branches that exist today: `/.quarto/...` paths via
`vfsReadFile`, and project-relative paths via `resolveRelativePath`
+ `vfsReadBinaryFile`. The project-relative branch is the one
q2-preview actually exercises (see §"Multi-plan contract:
page-scoped image artifacts"); the `/.quarto/...` branch stays
because the HTML iframe still uses it.

The HTML iframe's `postProcessIframe` calls this helper for the
image rewrite, retaining its `<link>` rewrite (lines 137-147),
external-link `target="_blank"` (lines 213-215), and qmd-link
click handler (line 218+) inline — none of those ride along to the
AST iframe. Behavior-preserving for the HTML iframe; existing
`iframePostProcessor.test.ts` and `.integration.test.ts` suites
guard the refactor.

#### 9. AST-iframe link handlers

`hub-client/src/utils/iframeLinkHandlers.ts`, new file. Extract
external-new-tab, `.qmd` click, same-doc-anchor click, and
`Ctrl+S`/`Cmd+S` save logic from `iframePostProcessor.ts:212-281`
into `installLinkHandlers(doc: Document, opts)`. The AST iframe
uses event delegation — a single `click` listener on
`document.body` plus the `keydown` listener, attached once at
mount:

```ts
doc.body.addEventListener('click', (e) => {
  const a = (e.target as HTMLElement).closest('a');
  if (!a) return;
  const href = a.getAttribute('href');
  if (!href) return;
  if (href.startsWith('http://') || href.startsWith('https://')) {
    e.preventDefault();
    window.open(href, '_blank', 'noopener,noreferrer');
    return;
  }
  if (href.startsWith('#')) { /* → onQmdLinkClick({ anchor }) */ }
  /* .qmd → resolveRelativePath + onQmdLinkClick({ path, anchor }) */
});
```

External new-tab is handled via `window.open` + `preventDefault`
rather than the HTML iframe's per-element `target="_blank"`
attribute write — delegation can't set attributes on every `<a>`
without a re-walk per render.

The HTML iframe's per-element listener pattern (lines 222-237,
240-251) stays as-is; only the AST iframe uses delegation. The
artifact-rooted `.html` reverse-mapping (bd-lnd3, lines 253-272) is
**not extracted** — it only matters when `LinkRewriteTransform`
ran, which q2-preview's pipeline excludes. q2-preview's AST keeps
`.qmd` paths verbatim; the `.qmd` branch handles them natively.

Also export `injectPreviewStyles` from `iframePostProcessor.ts`
(lines 294-326) so the AST iframe can call it. The HTML iframe
still calls it inline; the export is mechanical.

#### 10. `AstWithAssets` wrapper component

In `hub-client/src/ast-renderer-entry.tsx`, wrap the existing
`<Ast>` mount in a small container component that holds three
`useEffect`s — one per concern from the items above. Glue layer
that brings 8 (image rewrite), 9 (link handlers), and the theme
CSS injection together at the iframe boundary:

```tsx
function AstWithAssets(props: AstProps) {
  // [astJson, currentFilePath]: image rewrite re-runs after each
  // commit because React replaces <img src="data:..."> with raw
  // paths each render.
  useEffect(() => {
    rewriteImages(document, { currentFilePath: props.currentFilePath });
  }, [props.astJson, props.currentFilePath]);

  // []: link handlers attach once at mount via event delegation.
  useEffect(() => {
    installLinkHandlers(document, {
      currentFilePath: props.currentFilePath,
      onQmdLinkClick: props.onNavigateToDocument,
    });
  }, []);

  // []: theme CSS + responsive overrides inject once at mount.
  useEffect(() => {
    const css = vfsReadFile(DEFAULT_CSS_ARTIFACT_PATH);
    if (css.success && css.content) {
      const style = document.createElement('style');
      style.textContent = css.content;
      document.head.appendChild(style);
    }
    injectPreviewStyles(document);
  }, []);

  return <Ast {...props} />;
}
```

Why the wrapper rather than effects inside `<Ast>`: keeps `Ast`
focused on rendering the AST tree; isolates the iframe-only
concerns (postMessage, blob-URL component loading, asset glue)
where the rest of the iframe-only glue already lives.

When service-worker resource resolution lands, items 8, 9, and the
theme CSS injection all delete together (per
`iframePostProcessor.ts:24`). The wrapper itself becomes a thin
`<Ast {...props} />` shell at that point — or the `<Ast>` mount
re-flattens.

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
2. **Images render.** `<img>` elements (with the user's original
   `src` from the qmd, e.g. `hero.png` or `images/foo.png`) resolve
   to the in-VFS upload via the project-relative branch of
   `rewriteImages` (`resolveRelativePath(currentFilePath, src)` +
   `vfsReadBinaryFile`) and display correctly. The bytes come from
   the user's automergeSync upload — no `/.quarto/...` paths appear
   in q2-preview's body AST (see §"Multi-plan contract: page-scoped
   image artifacts" for the full mechanism).
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
  loses against any user theme rule at spec ≥ 0,0,0,1 (Bootstrap's
  reboot is 0,0,0,1; comfortably wins) while still applying when no
  theme CSS is loaded.
- **Image rewrite via post-render DOM walk, not AST-walk**. The
  helper takes a `Document` and rewrites `<img>` in place. Mirrors
  the HTML iframe pattern for images and enables code share with
  identical removal when service-worker resource resolution lands.
  AST-walk alternative was discussed and rejected (would require
  re-rewriting on every change and diverges from the proven
  HTML-iframe path).
- **Inline `<style>` for theme CSS, not `<link>` rewrite**. The HTML
  iframe's `<link>` rewrite works because the renderer emits
  `<link rel="stylesheet" href="/.quarto/...">` as part of the HTML
  body; the AST iframe never has a `<link>` to rewrite (Pandoc nodes
  don't produce them). Three alternatives considered: (a) a static
  `<link>` in `ast-renderer.html` rewritten on iframe-init — risks
  flash-of-unstyled-content while the browser tries to load the VFS
  path before the rewriter intercepts; (b) React-19 hoisted `<link>`
  — more machinery for the same effect; (c) inline `<style>` from
  VFS bytes — chosen. When service-worker resource resolution
  lands, alternative (a) becomes flash-free and we can revisit.
- **Event delegation for AST-iframe link handlers**. The HTML iframe
  attaches per-element click listeners after each `srcdoc` load,
  which works because the iframe document is fresh on each load. The
  AST iframe mutates a React-managed tree incrementally; per-element
  re-attachment per render would require idempotency tracking and is
  fragile. A single delegated `click` listener on `document.body`
  keys off href shape and dispatches; set once at iframe init, no
  re-walk needed.
- **`BlockNode` / `InlineNode` as canonical names** (over
  `Block` / `Inline`). The richest existing definition
  (`ReactAstDebugRenderer.tsx`) uses these names; `Block`/`Inline`
  are too generic for grep. The migration affects three slide-side
  importers plus the slide renderer itself; all change in this pass
  with no compat re-export.
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
  accessor handles them. Plan 2B consumes via the unified
  Block / Inline dispatcher it introduces.
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
render. 2A's iframe entry reads the bytes once on first AST receive
and injects them as an inline `<style>` element in `document.head`
(the AST iframe has no `<link>` to rewrite). The Rust→VFS contract
from Plan 1 is unchanged; 2A is the first reader.

### Consumed: page-scoped image artifacts (from Plan 1)

The contract here is subtler than "renderer writes, iframe reads."
The renderer does **not** contribute image bytes. `ResourceCollectorTransform`
walks the AST immutably and stores artifact entries with empty
content (`Artifact::from_path` at `artifact.rs:108-116` sets
`content: Vec::new()`). The WASM flush loop at
`wasm-quarto-hub-client/src/lib.rs:1208-1214` (single-doc) and
`:1364-1369` (project) writes those empty bytes to the resolver's
on-disk path. Net effect for images: at best a no-op manifest entry,
at worst a clobber of the user's upload (see §"Risk areas →
Empty-content artifact overwrite" below).

The image bytes the iframe actually reads come from the **user's
original VFS upload**, written by the hub-client's `automergeSync`
via `vfsAddFile` / `vfsAddBinaryFile` whenever a file appears in
the synced project. So the agreement that lets the rewriter find
the bytes is: *the user uploaded the image at the same
project-relative path the qmd references it by*. The iframe
computes that path via `resolveRelativePath(currentFilePath, src)`
and reads via `vfsReadBinaryFile`. The renderer is not in the loop
for image bytes.

`<img src>` in q2-preview's AST keeps the user's original path
(`hero.png`, `images/foo.png`, etc.) — `LinkRewriteTransform`
explicitly leaves `Image::target.0` alone, and no other transform
mutates it. External URLs (`http`, `https`, `data:`, `//`) and
absolute paths (`/foo.png`) follow today's HTML-iframe behavior.
**No `/.quarto/...` paths appear in q2-preview's body AST** — the
`/.quarto/...` branch of `rewriteImages` is dormant for q2-preview
but kept for the HTML iframe's use of the same helper.

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
  `RegistryContext` definition (extended in 2A) and one of the
  four sites whose `PandocAST` definition moves to
  `types/pandoc.ts`.
- `hub-client/src/components/render/AstIframe.tsx` — postMessage
  protocol (unchanged by 2A).
- `hub-client/src/ast-renderer-entry.tsx` — iframe entry; 2A adds
  the `AstWithAssets` wrapper component (image rewriter useEffect,
  link handler useEffect, theme CSS injection useEffect) here.
- `hub-client/public/ast-renderer.html:7-22` — inline `<style>`
  to wrap with `:where()`.
- `hub-client/src/utils/iframePostProcessor.ts:177-210` — source
  for the image-rewrite logic to extract into
  `iframeImageRewriter.ts`.
- `hub-client/src/utils/iframePostProcessor.ts:212-281` — source
  for the link-handler logic (external new-tab, `.qmd` clicks,
  same-doc anchor, `Ctrl+S`/`Cmd+S` save) to extract into
  `iframeLinkHandlers.ts`. The artifact-rooted `.html`
  reverse-mapping at lines 253-272 stays in
  `iframePostProcessor.ts` (HTML-only; not extracted).
- `hub-client/src/components/render/ReactAstRenderer.tsx` —
  dead file to delete.
- `hub-client/src/types/diagnostic.ts`,
  `hub-client/src/types/intelligence.ts`,
  `hub-client/src/utils/pipelineKind.ts` — existing TS↔Rust mirror
  patterns to follow.

## Test plan

### TDD discipline per work-item

The TDD discipline in `CLAUDE.md` ("write test → verify failure →
implement → verify pass") applies **per work-item**, not per-plan.
Items 1–7 are greenfield additions or single-purpose changes — true
failing-test-first applies (write the test, verify it fails for the
expected reason, implement, verify it passes). Items 2 (PandocAST
consolidation), 8 (image rewriter extraction), and 9 (link handlers
extraction) are **behavior-preserving refactors** — the canonical
"failing test" doesn't exist because there's no behavior change to
test for. The test gate for these items is **"existing tests pass
before AND after the refactor"** (specifically the
`iframePostProcessor.test.ts` and `.integration.test.ts` suites for
items 8 / 9; the workspace `tsc -b` for item 2). Structure each
refactor commit so it is independently verifiable: run the relevant
test suite at the start of the commit (should pass), make the
mechanical move, run the suite again (should still pass).

The temptation on a refactor is to skip the "before" run and trust
the diff. Don't — running before catches a pre-existing failure
that would otherwise look like a refactor regression.

### Tests

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
- **Image rewriter unit tests**: build a representative `Document`
  with `<img>` elements pointing at (a) project-relative paths
  (`hero.png`), (b) `/.quarto/...` paths (legacy/HTML branch),
  (c) external URLs (skipped), and (d) `data:` URIs (skipped).
  Mock VFS reads, run `rewriteImages(doc, { currentFilePath })`,
  assert the resulting DOM has `data:` URIs in place of the
  rewritable paths and no change to the others.
- **Image rewriter integration test (HTML iframe)**: existing
  `iframePostProcessor.test.ts` and `.integration.test.ts` suites
  pass before and after the extraction. The refactor is
  behavior-preserving.
- **Image rewriter integration test (AST iframe)**: render an AST
  with an `<img src="hero.png">`, populate the VFS at the
  resolved project-relative path with a fake image, mount the
  iframe, assert the rendered `<img>` has a `data:` URI src.
  Re-render with a different image src, assert the new image is
  also rewritten (verifies the `[astJson]` dependency on the
  useEffect).
- **Link handler unit tests** (vitest): build a representative
  `Document`, attach `installLinkHandlers(doc, { onQmdLinkClick,
  currentFilePath: '/foo.qmd' })`, dispatch synthetic clicks on:
  `<a href="https://example.com">` (assert `window.open` called
  with `_blank`), `<a href="other.qmd#sec">` (assert
  `onQmdLinkClick({ path: '/other.qmd', anchor: 'sec' })`),
  `<a href="#sec">` (assert `onQmdLinkClick({ anchor: 'sec' })`),
  and a non-`.qmd` non-anchor href (assert no handler call,
  default click behavior preserved). Dispatch a synthetic
  `Cmd+S` keydown, assert the parent postMessage fires.
- **Theme CSS injection unit test**: render an AST through
  `AstWithAssets`, populate the VFS with a fake `styles.css`,
  assert a `<style>` element with the CSS bytes appears in
  `document.head`. Re-render — assert the `<style>` is not
  duplicated (one-shot guarantee).
- **`:where()` style regression test** (DOM-inspection, not
  snapshot): mount the iframe with no theme CSS loaded
  (q2-debug); assert `getComputedStyle(document.body).fontFamily`
  contains the system-font reset prefix. Mount with theme CSS
  injected; assert the computed font-family contains
  `var(--bs-body-font-family)`'s resolved stack (or, more
  robustly, assert it differs from the q2-debug value).
  DOM-inspection over snapshots because computed-style snapshots
  drift across browser/UA-default updates.
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
  every artifact 2A ships (PandocAST consolidation with
  `BlockNode`/`InlineNode` naming, source-info accessor,
  atomicCustomNodes.ts, image rewriter, link handlers, theme CSS
  injection). 2B cannot land before 2A.
- Independent of Plans 4 / 5 / 6 / 7 / 8 — they extend the writer
  / type system / wire format. 2A's source-info wiring is forward-
  compatible with all of them.

## Risk areas

- **`iframePostProcessor.ts` refactor regression**. The image
  rewriter and link-handler extractions must be behavior-preserving
  for the HTML iframe. Mitigation: the existing
  `iframePostProcessor.test.ts` and `iframePostProcessor.integration.test.ts`
  suites pass before and after. Don't change extraction shape
  mid-refactor.
- **`PandocAST` consolidation type drift**. The four duplicate
  definitions have drifted on naming (`Block`/`Inline` vs
  `BlockNode`/`InlineNode`) and inline-variant coverage. The
  consolidation picks `BlockNode`/`InlineNode` (richest existing
  shape) plus the `astContext?` field and `CustomBlockNode` /
  `CustomInlineNode` placeholders. The three slide-side importers
  (`RevealjsReactAstSlideRenderer.tsx`, `useCursorToSlide.ts`,
  `useSlideThumbnails.tsx`) get rename + import updates in the same
  pass. Run `tsc -b` after each consumer's import update.
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
- **AST-iframe `useEffect` re-run on every commit**. The image
  rewriter useEffect keys on `[astJson, currentFilePath]`. If
  another React state change triggers a re-render without changing
  `astJson`, the effect skips correctly. If a future change makes
  the AST mutate without `astJson` changing reference (e.g. an
  in-place edit), the rewriter would miss the update. Mitigation:
  always replace `astJson` with a new string on AST mutation
  (today's `setAst` flow already does this via postMessage
  serialization).
- **Empty-content artifact overwrite (latent bug discovered during
  plan review)**. The WASM flush loop at
  `wasm-quarto-hub-client/src/lib.rs:1208-1214` and `:1364-1369`
  writes `artifact.content.clone()` to VFS without checking for
  empty content. `ResourceCollectorTransform` produces empty-content
  artifacts whose `path` field is the absolute resolved
  `base_dir.join(url)`. `Path::join` with an absolute second arg
  replaces the first, so the resolver's `vfs_root.join(absolute_path)`
  collapses to the absolute path itself — which is also where the
  hub-client's `automergeSync` uploaded the user's image. The flush
  therefore overwrites the user's bytes with `Vec::new()` on every
  render. In current production this hasn't bitten anyone yet (HTML
  preview has been the test surface and the iframe rewrite still
  happens to work after the overwrite because… it doesn't, actually
  — verifying this in HTML preview is a follow-up). Plan 2A doesn't
  fix the bug, but the iframe rewriter must keep using the user's
  upload as the source of truth and treat the renderer's image flush
  as not-load-bearing. **TODO**: file a beads issue with the
  one-line fix (`if !artifact.content.is_empty() { runtime.add_file(...) }`)
  once we're on the main repo; reference it from the plan when filed.
- **Wire-format code 3 back-compat (minor)**. 2A's TS type for
  code 3 covers the post-Plan-5 reader's expected
  `[filter_path, line]` shape (FilterProvenance). Plan 5's reader
  also accepts the legacy Transformed shape `[parent_id, ...]`,
  but no fresh writer emits that anymore. If old AST JSON predating
  Plan 5 ever reaches the iframe, the type would not cover it; in
  production the iframe only sees fresh writer output, so this is
  not a real risk.

## Estimated scope

Items in implementation order (matching §"In scope" above).

| # | Component | Lines (rough) |
|---|---|---|
| 1 | `types/sourceInfo.ts` (mirror types) | ~50 |
| 2 | `types/pandoc.ts` consolidation + 6-consumer migration + delete dead file | ~180 (net negative after deletion) |
| 3 | `utils/sourceInfo.ts` (accessors + tests) | ~120 |
| 4 | `utils/atomicCustomNodes.ts` (TS hand-mirror) | ~30 |
| 5 | `RegistryContext` extension + AST entry threading | ~30 |
| 6 | `ast-renderer.html` `:where()` wrap + regression test | ~30 |
| 7 | `render-components` gate extension + regression test | ~20 |
| 8 | `iframeImageRewriter.ts` extraction + HTML caller update | ~80 |
| 9 | `iframeLinkHandlers.ts` extraction + HTML caller unchanged (HTML keeps inline pattern) | ~120 |
| 10 | `AstWithAssets` wrapper (3 useEffects: image, link, theme CSS + injectPreviewStyles) + integration tests | ~120 |
| | **Total** | **~780** |

Two focused sessions are realistic. A natural split is **items
1–5** (type/data foundation; inert wiring for 2B) and **items
6–10** (iframe glue; lights up theme CSS, images, and link
navigation visibly). Either session can land independently of the
other — items 1–5 don't visibly change anything, and items 6–10
don't depend on the source-info types existing.

## Notes

- This plan replaces the foundation half of the original Plan 2
  (`2026-05-04-q2-preview-plan-2-builtin-components.md`), which
  was split into 2A (foundation) + 2B (components) during the
  2026-05-06 review session. The split was driven by scope
  realism: research raised the original ~970 LOC estimate, and
  added items (source-info plumbing, image rewriter / link
  handlers extraction, theme CSS injection, render-components
  gate, `:where()` fix) that are logically separable from the
  type-specific component work.
- The "rename `MaybeReadOnlyInline`" from the original Plan 2 is
  resolved in 2B: there's no such wrapper component; the atomic-
  aware `setLocalAst` gating folds into the existing
  `registry[node.t]` lookups inside `<Ast>` (or into a new
  dispatcher component introduced by 2B; final shape is 2B's
  call). 2A ships the prerequisites (`isAtomicSourceInfo`,
  `atomicCustomNodes.ts`); 2B ships the consumers.
- Forward-compat dormancy is the explicit pattern for 2A's
  source-info wiring vs. Plan 5 wire-format codes 4/5. The plan-7
  cleanup pattern (Plan 1's `pipeline_kind` field landed dormant
  for Plan 7 to consume) is the same idea.

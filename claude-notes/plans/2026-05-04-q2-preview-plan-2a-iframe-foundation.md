# Plan 2A — q2-preview iframe foundation

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (resolved during the 2026-05-06 review session)
**Milestone:** M2-foundation (iframe is ready to host type-specific React components)

## Goal

Land the iframe-side plumbing that makes q2-preview ready to host the
type-specific React components shipped in Plan 2B. After 2A:

- Theme CSS produced by `CompileThemeCssStage` reaches the AST
  iframe by inlining the VFS bytes into a single
  `<style data-q2-theme>` element in `document.head`,
  fingerprint-keyed for live reload (item 11 surfaces
  `theme_fingerprint(css)` from Rust so theme swaps trigger
  re-injection without remounting the iframe). The HTML iframe's
  `<link>`-rewrite pattern doesn't carry over: the AST iframe has
  no `<link>` to rewrite (Pandoc nodes don't produce stylesheet
  links), so inline injection is the natural alternative until
  service-worker resource resolution lands.
- Page-scoped image artifacts produced by `ResourceCollectorTransform`
  reach the iframe via **render-time resolution** in the `Image`
  component renderer: it reads `currentFilePath` from `RegistryContext`,
  resolves user-written `<img src>` paths like `hero.png` against the
  current document's directory, and reads bytes synchronously from the
  VFS into a `data:` URI on the React-emitted `<img>`. No post-render
  DOM walk, no useEffect, no flicker. The HTML iframe keeps its
  existing inline rewriter (post-`srcdoc` DOM walk) since it doesn't
  own a React render path. See §"Multi-plan contract: page-scoped image
  artifacts" for why q2-preview's body AST never carries `/.quarto/...`
  paths.
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
type / data foundation (most consumed by Plan 2B and by later items
in this plan); items 6–7 are independent one-liners; item 8 is the
`Image` renderer change that consumes `currentFilePath` from the
context plumbed in item 5; item 9 is the link-handlers extraction;
item 10 is the AST-iframe wrapper that consumes 9 plus theme CSS
injection. Item 11 is a small Rust-side change that surfaces
`themeFingerprint` for live theme reload — items 1–10 can ship
without it.

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

Pull the duplicate `PandocAST` definitions from `ReactRenderer.tsx`,
`ReactAstSlideRenderer.tsx`, and `ReactAstDebugRenderer.tsx` (plus
the dead `ReactAstRenderer.tsx`) into a single
`hub-client/src/types/pandoc.ts`. Pull the `Block` / `Inline`
definitions from `ReactRenderer.tsx` and `ReactAstDebugRenderer.tsx`
under the names `BlockNode` / `InlineNode` (the richest existing
definition is `ReactAstDebugRenderer.tsx`'s, which already uses
these names; `Block`/`Inline` are too generic for grep).

**Slide-side scope is intentionally minimal.** q2-slides stays on
the q2-debug render path (no q2-preview upgrade in this plan), so
`ReactAstSlideRenderer.tsx` keeps its locally-exported `Block` /
`Inline` types — no rename, no consolidation of those types. The
slide hooks (`hooks/useCursorToSlide.ts`,
`hooks/useSlideThumbnails.tsx`) and `RevealjsReactAstSlideRenderer.tsx`
do not import `Block` or `Inline` directly today; they import
`PandocAST` (and `parseSlides` / `renderBlock` / `renderSlide`),
and their only change is to update the `PandocAST` import path to
`types/pandoc.ts`. Light-touch refactor for the slide side; deeper
consolidation for the q2-debug / q2-preview side.

Add `astContext?: AstContext` to the consolidated `PandocAST`
(import from item 1). The type also includes **placeholder
discriminants for CustomBlockNode (`t: 'CustomBlock'`) and
CustomInlineNode (`t: 'CustomInline'`)** in the `BlockNode` /
`InlineNode` unions — Plan 2B's `unwrapCustomNodes` walk produces
these at render time but the shapes are pre-declared so 2B doesn't
have to re-edit foundational types.

**Consumers updated** to import from the new file:

- `ReactRenderer.tsx` — `PandocAST` and the (renamed)
  `BlockNode` / `InlineNode`.
- `ReactAstDebugRenderer.tsx` — `PandocAST`, `BlockNode`,
  `InlineNode`.
- `ReactAstSlideRenderer.tsx` — `PandocAST` only (keeps local
  `Block` / `Inline`).
- `RevealjsReactAstSlideRenderer.tsx`, `hooks/useCursorToSlide.ts`,
  `hooks/useSlideThumbnails.tsx` — `PandocAST` import path update
  only.

The dead `ReactAstRenderer.tsx` is **deleted** (zero importers).
Landing this item second (after the foundational types) keeps
later items rebasing cleanly.

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

**Discovery mechanism.** The dispatcher recovers the wrapped
CustomNode's kind from the `data-custom-type` attribute that the
JSON writer attaches to every wrapper Div (block) / Span (inline)
— see `crates/pampa/src/writers/json.rs:1297-1325` (block) and
`:1381+` (inline). The hand-mirror's strings match the writer's
emitted `type_name` byte-for-byte; `"CrossrefResolvedRef"`
corresponds to the literal `type_name` Rust emits. 2B's dispatcher
reads `wrapper.attr.kvs["data-custom-type"]` and looks the value
up in this set.

#### 5. Extend `RegistryContext`

In `hub-client/src/components/render/ReactAstDebugRenderer.tsx`,
extend `RegistryContext` to carry `sourceInfoPool?: SourceInfoPool`
(from item 1) and `currentFilePath: string` alongside `registry`.
`<Ast>` wraps its rendered children in the Provider:

```tsx
<RegistryContext.Provider value={{
  registry,
  sourceInfoPool: astContext?.sourceInfoPool,
  currentFilePath,
}}>
  {/* rendered tree */}
</RegistryContext.Provider>
```

`currentFilePath` reaches `<Ast>` as a prop from
`ast-renderer-entry.tsx` (set on each `setAst` postMessage). Item 8
consumes it for render-time `<img src>` resolution in the `Image`
renderer.

`sourceInfoPool` consumers don't read it yet — the existing
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

#### 8. Image renderer render-time resolution

In `hub-client/src/components/render/ReactAstDebugRenderer.tsx`'s
registry, the `Image` renderer is updated to resolve `<img src>`
synchronously at render time, consuming `currentFilePath` from
`RegistryContext` (item 5). The component reads VFS bytes via
`vfsReadBinaryFile(resolveRelativePath(currentFilePath, src))`,
encodes them as a `data:` URI, and emits `<img src={dataUri}>`
directly. No `useEffect`, no post-render DOM walk, no flicker.

External URLs (`http`, `https`, `data:`, `//`) and paths that fail
VFS resolution pass through unchanged — GIGO; mirrors the HTML
iframe's existing fallthrough behavior. The legacy `/.quarto/...`
branch from `iframePostProcessor.ts:177-210` is **not** ported
here: q2-preview's body AST never carries `/.quarto/...` image
paths (see §"Multi-plan contract: page-scoped image artifacts").

The HTML iframe is **unchanged** by this item. Its inline rewriter
in `iframePostProcessor.ts:177-210` stays where it is, with both
branches intact; existing `iframePostProcessor.test.ts` and
`.integration.test.ts` suites continue to guard it. The
"shared rewriter helper" approach from earlier drafts of this plan
is dropped — there is no second consumer to share with, and the
HTML iframe is on its own deletion timeline (the rewriter goes
away when service-worker resource resolution lands; same fate,
parallel implementations).

**Knock-on for q2-debug.** q2-debug uses the same registry, so it
also picks up render-time image resolution. Today q2-debug has no
image-rewrite path at all; under this plan, q2-debug images render
correctly as a side effect. Not a regression.

#### 9. AST-iframe link handlers

`hub-client/src/utils/iframeLinkHandlers.ts`, new file. Extract
external-new-tab, `.qmd` click, same-doc-anchor click, and
`Ctrl+S`/`Cmd+S` save logic from `iframePostProcessor.ts:212-281`
into `installLinkHandlers(doc: Document, ctxRef: { current: Ctx })`,
where `Ctx = { currentFilePath: string; onQmdLinkClick: (...) => void }`.
The AST iframe uses event delegation with a **ref-based context**
to avoid stale closures across document navigation (the iframe is
mounted once per session and persists across docs; see "Design
decisions → Iframe lifetime model"):

```ts
// click on document.body (delegation)
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
  const { currentFilePath, onQmdLinkClick } = ctxRef.current;
  if (href.startsWith('#')) { /* → onQmdLinkClick({ anchor }) */ }
  /* .qmd → resolveRelativePath(currentFilePath, href)
            + onQmdLinkClick({ path, anchor }) */
});

// keydown on window (document.body doesn't reliably receive
// keyboard focus; HTML iframe attaches its keydown to window too)
window.addEventListener('keydown', (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === 's') {
    e.preventDefault();
    parent.postMessage({ kind: 'save' }, '*');
  }
});
```

Reading `currentFilePath` and `onQmdLinkClick` from `ctxRef.current`
inside the listener (rather than capturing them at attach time)
keeps the handlers correct after the user navigates to a different
document — `ctxRef.current` is updated by the wrapper on each
render; no listener re-attachment needed.

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
(lines 294-326) so the AST iframe can call it, **and add an
idempotency guard inside the function**: early-return if
`doc.head.querySelector('style[data-hub-client]')` is non-null
(~3 LOC). Without the guard, React 18 StrictMode's
mount→unmount→mount cycle in dev would inject the styles twice.
The HTML iframe's existing call is unaffected (it calls
`injectPreviewStyles` exactly once per `srcdoc` load).

#### 10. `AstWithAssets` wrapper component

In `hub-client/src/ast-renderer-entry.tsx`, wrap the existing
`<Ast>` mount in a small container component that holds two
`useEffect`s plus a context ref. Glue layer that brings item 9
(link handlers) and theme CSS injection together at the iframe
boundary. Image resolution is **not** here — it lives in the
`Image` renderer (item 8), so the wrapper has no image effect to
manage:

```tsx
function AstWithAssets(props: AstProps) {
  // Ref-based context for link handlers — keeps the closure inside
  // installLinkHandlers reading current props instead of stale
  // mount-time values. See item 9 for the why.
  const linkCtxRef = useRef({
    currentFilePath: props.currentFilePath,
    onQmdLinkClick: props.onNavigateToDocument,
  });
  linkCtxRef.current = {
    currentFilePath: props.currentFilePath,
    onQmdLinkClick: props.onNavigateToDocument,
  };

  // []: link handlers attach once at mount via event delegation.
  useEffect(() => {
    installLinkHandlers(document, linkCtxRef);
  }, []);

  // [props.themeFingerprint]: re-inject when the compiled CSS bytes
  // change. The fingerprint is surfaced from Rust (item 11) on each
  // RenderResponse; the styles.css path itself is constant
  // (DEFAULT_CSS_ARTIFACT_PATH), so the bytes are the change signal.
  // The data-q2-theme marker doubles as a StrictMode idempotency
  // guard (mount→unmount→mount in dev would otherwise duplicate the
  // <style>).
  useEffect(() => {
    const css = vfsReadFile(DEFAULT_CSS_ARTIFACT_PATH);
    if (!css.success || !css.content) return;
    let style = document.head.querySelector<HTMLStyleElement>('style[data-q2-theme]');
    if (!style) {
      style = document.createElement('style');
      style.setAttribute('data-q2-theme', '1');
      document.head.appendChild(style);
    }
    style.textContent = css.content;
    injectPreviewStyles(document);  // idempotent per item 9
  }, [props.themeFingerprint]);

  return <Ast {...props} />;
}
```

Why the wrapper rather than effects inside `<Ast>`: keeps `Ast`
focused on rendering the AST tree; isolates the iframe-only
concerns (postMessage, blob-URL component loading, asset glue)
where the rest of the iframe-only glue already lives.

Until item 11 lands, `props.themeFingerprint` is `undefined` and
the theme effect runs once on mount (no live reload, but theme
CSS still injected — strictly better than today's "no theme CSS
at all" state).

When service-worker resource resolution lands, both effects delete
together (per `iframePostProcessor.ts:24`). The `Image` renderer
in item 8 also drops its VFS read at that point, emitting raw
paths the SW intercepts. The wrapper becomes a thin
`<Ast {...props} />` shell at that point — or the `<Ast>` mount
re-flattens.

#### 11. Rust-side `themeFingerprint` surfacing

Plumb `theme_fingerprint(css)` (already computed at
`crates/quarto-core/src/stage/stages/compile_theme_css.rs:447`,
already used as the artifact key `css:theme:<fingerprint>`) onto
`RenderResponse`, through the WASM bridge, into the postMessage
payload that `ast-renderer-entry.tsx` consumes as
`props.themeFingerprint`. Path:

1. **`quarto-core`** (or wherever Plan 1 places the render-output
   type): add `theme_fingerprint: Option<String>` to
   `RenderResponse`. Populate from the active theme artifact's key
   at the point where the response is constructed. ~5 LOC.
2. **`wasm-quarto-hub-client/src/lib.rs`**: surface the field on
   the JS-facing return shape. Mechanical. ~3 LOC.
3. **hub-client TS types** (e.g. `types/render.ts` or wherever
   `RenderResponse` is mirrored): add the field. ~2 LOC.
4. **`AstIframe.tsx`** postMessage payload: pass
   `themeFingerprint` through to the iframe alongside `astJson`
   and `currentFilePath`. ~2 LOC.
5. **`ast-renderer-entry.tsx`**: receive and forward to
   `<AstWithAssets>` as a prop.

Items 1–10 can ship without item 11; item 10's wrapper handles
`themeFingerprint === undefined` as a no-op (no re-injection on
theme change). Item 11 lights up live theme reload as the final
piece.

Because this item touches `quarto-core`, full
`cargo xtask verify` (not `--skip-hub-build`) is required before
merging — the `wasm-quarto-hub-client` crate depends on
`quarto-core` types and the WASM build is the only check that
catches drift.

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
   at render time inside the `Image` component renderer
   (`resolveRelativePath(currentFilePath, src)` + `vfsReadBinaryFile`,
   reading `currentFilePath` from `RegistryContext`). The bytes
   come from the user's automergeSync upload — no `/.quarto/...`
   paths appear in q2-preview's body AST (see §"Multi-plan contract:
   page-scoped image artifacts" for the full mechanism). q2-debug
   picks up image rendering as a side effect (the same registry
   serves both surfaces).
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
- **Render-time `<img src>` resolution in the `Image` component,
  not a post-render DOM walk**. Earlier drafts proposed mirroring
  the HTML iframe's DOM-walk pattern. That works for the HTML iframe
  because it doesn't own a React render path (`srcdoc` is opaque).
  The AST iframe DOES own its render path, so the idiomatic React
  fix is "render the right output" rather than "fix what render
  produced." Render-time resolution eliminates flicker, eliminates
  the `[astJson]`-dep concern that haunted earlier drafts, and
  produces fewer effects in the wrapper. Trade-off: the AST iframe
  and HTML iframe no longer share a rewriter helper; they're on
  parallel deletion timelines anyway (both remove their image
  logic when service-worker resource resolution lands).
- **Inline `<style>` for theme CSS, fingerprint-keyed for live
  reload**. The HTML iframe's `<link>` rewrite works because the
  renderer emits `<link rel="stylesheet" href="/.quarto/...">` as
  part of the HTML body; the AST iframe never has a `<link>` to
  rewrite (Pandoc nodes don't produce them). Three alternatives
  considered: (a) a static `<link>` in `ast-renderer.html` rewritten
  on iframe-init — risks flash-of-unstyled-content; (b) React-19
  hoisted `<link>` — more machinery for the same effect; (c) inline
  `<style>` from VFS bytes — chosen. The bytes change when the user
  swaps themes, but the artifact path is constant
  (`DEFAULT_CSS_ARTIFACT_PATH`), so the wrapper's effect keys on
  `props.themeFingerprint` (item 11) to re-inject when the
  underlying CSS changes. Single `<style data-q2-theme="1">` element
  is reused across reloads; idempotent under React StrictMode. When
  service-worker resource resolution lands, alternative (a) becomes
  flash-free and we can revisit.
- **Iframe lifetime model.** The AST iframe is mounted exactly once
  per session and persists across document navigation — the user
  switching between files updates `astJson` and `currentFilePath`
  via postMessage, but the iframe DOM (and its internal React tree)
  does not unmount and remount. This is the architectural fact that
  motivates the ref-based context for link handlers (props would go
  stale otherwise) and the fingerprint-keyed theme reload (a
  one-shot mount-time effect would never re-fire on theme change).
  Stated explicitly because several decisions in this plan hinge
  on it.

- **Event delegation with ref-based context for AST-iframe link
  handlers**. The HTML iframe attaches per-element click listeners
  after each `srcdoc` load, which works because the iframe document
  is fresh on each load. The AST iframe mutates a React-managed
  tree incrementally; per-element re-attachment per render would
  require idempotency tracking and is fragile. A single delegated
  `click` listener on `document.body` (plus a `keydown` listener on
  `window`) keys off href shape and dispatches; set once at iframe
  init, no re-walk needed. Because the iframe persists across
  document navigation (per "Iframe lifetime model" above), the
  listener reads `currentFilePath` and `onQmdLinkClick` via a ref
  that the wrapper updates on each render — closure capture of
  those props at mount time would go stale on doc switch.
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

### Consumed: theme CSS artifact + fingerprint signal (from Plan 1 + item 11)

Plan 1's `RenderToPreviewAstRenderer` writes the compiled theme
CSS to `/.quarto/project-artifacts/styles.css` (per
`pipeline.rs::DEFAULT_CSS_ARTIFACT_PATH`) on every q2-preview
render. The path is constant across theme swaps; only the bytes
change. 2A's wrapper's theme effect reads the bytes when it fires
and injects them into a single `<style data-q2-theme="1">` element
in `document.head` (the AST iframe has no `<link>` to rewrite).

Item 11 surfaces `theme_fingerprint(css)` (computed at
`compile_theme_css.rs:447`, already used as the artifact key
`css:theme:<fingerprint>`) onto `RenderResponse` as
`themeFingerprint`. The wrapper's theme effect keys on this value
so live theme swaps trigger re-injection. The Rust→VFS contract
from Plan 1 is unchanged; 2A is the first reader and adds the
fingerprint surfacing as a small Rust-side change.

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
the synced project. So the agreement that lets the renderer find
the bytes is: *the user uploaded the image at the same
project-relative path the qmd references it by*. The `Image`
component renderer (item 8) computes that path via
`resolveRelativePath(currentFilePath, src)` and reads via
`vfsReadBinaryFile` synchronously during render, emitting an
`<img src="data:...">` directly. The renderer is not in the loop
for image bytes.

`<img src>` in q2-preview's AST keeps the user's original path
(`hero.png`, `images/foo.png`, etc.) — `LinkRewriteTransform`
explicitly leaves `Image::target.0` alone, and no other transform
mutates it. External URLs (`http`, `https`, `data:`, `//`) and
paths that fail VFS resolution fall through unchanged (GIGO; same
as the HTML iframe today). **No `/.quarto/...` paths appear in
q2-preview's body AST** — the `/.quarto/...` branch in the HTML
iframe's inline rewriter has no analog in the AST iframe.

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
initial built-in set (`["CrossrefResolvedRef"]`). Plan 2B's
atomic-aware dispatcher imports `isAtomicCustomNode(typeName)` from
this file. Plan 7 ships the Rust counterpart and the sync
convention is documented in 2A's file header comment.

**Discovery mechanism for the dispatcher.** The JSON writer
serializes CustomNodes as wrapper Divs (block) or Spans (inline)
with a `data-custom-type` attribute carrying the literal
`type_name` — see `crates/pampa/src/writers/json.rs:1297-1325`
(block) and `:1381+` (inline). The dispatcher recovers the kind
by reading `wrapper.attr.kvs["data-custom-type"]` and looking it
up in the atomic set. The hand-mirror's strings match the writer's
emitted `type_name` byte-for-byte; `"CrossrefResolvedRef"` is the
exact string Rust emits.

## References

### Rust side

2A modifies Rust in one place — item 11's `themeFingerprint` field
on `RenderResponse`, plumbed through `wasm-quarto-hub-client`. The
references below are otherwise read-only (2A consumes their wire
format / pipeline behavior but does not change them).

- `crates/pampa/src/writers/json.rs:54-91` — wire format types
  (AstContextJson, SourceInfoJson, NodeJson, etc.). Allocation
  policy lives at
  `claude-notes/designs/wire-format-source-info-codes.md`.
- `crates/pampa/src/writers/json.rs:300-330` — `add_source_info`
  on each node.
- `crates/pampa/src/writers/json.rs:1297` — `write_custom_block`
  (block CustomNodes wrapped as Div with `data-custom-type`).
- `crates/pampa/src/writers/json.rs:1380` — `write_custom_inline`
  (inline CustomNodes wrapped as Span with `data-custom-type`).
- `crates/quarto-source-map/src/source_info.rs:22-55` — SourceInfo
  enum (extended by Plan 4).
- `crates/quarto-core/src/pipeline.rs::DEFAULT_CSS_ARTIFACT_PATH`
  — VFS path for the theme CSS artifact.
- `crates/quarto-core/src/stage/stages/compile_theme_css.rs:447`
  — `theme_fingerprint(css)` (item 11 surfaces the value onto
  `RenderResponse`).

### hub-client side

- `hub-client/src/components/render/ReactRenderer.tsx:101-111` —
  `render-components` gate (q2-preview added by 2A).
- `hub-client/src/components/render/ReactRenderer.tsx:148` —
  format dispatch for AstIframe (q2-debug + q2-preview both route
  through here today; unchanged by 2A).
- `hub-client/src/components/render/ReactAstDebugRenderer.tsx` —
  `RegistryContext` definition (extended in item 5 to carry
  `sourceInfoPool` and `currentFilePath`); `Image` renderer
  updated by item 8 for render-time `<img src>` resolution; one
  of the sites whose `PandocAST` / `BlockNode` / `InlineNode`
  definitions move to `types/pandoc.ts`.
- `hub-client/src/components/render/AstIframe.tsx` — postMessage
  protocol; item 11 adds `themeFingerprint` to the payload.
- `hub-client/src/ast-renderer-entry.tsx` — iframe entry; item 10
  adds the `AstWithAssets` wrapper (link-handler effect via ref,
  fingerprint-keyed theme CSS effect) and forwards
  `themeFingerprint` from the postMessage payload.
- `hub-client/public/ast-renderer.html:7-22` — inline `<style>`
  to wrap with `:where()`.
- `hub-client/src/utils/iframePostProcessor.ts:212-281` — source
  for the link-handler logic (external new-tab, `.qmd` clicks,
  same-doc anchor, `Ctrl+S`/`Cmd+S` save) to extract into
  `iframeLinkHandlers.ts`. The artifact-rooted `.html`
  reverse-mapping at lines 253-272 stays in
  `iframePostProcessor.ts` (HTML-only; not extracted). Item 9
  also adds an idempotency guard to `injectPreviewStyles`
  (lines 294-326) and exports it.
- `hub-client/src/utils/iframePostProcessor.ts:177-210` — image
  rewriter for the HTML iframe. **Unchanged** by 2A; item 8 ships
  a separate render-time approach for the AST iframe rather than
  extracting a shared helper.
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
Most items (1, 3, 4, 5, 6, 7, 8, 10, 11) are greenfield additions
or single-purpose changes — true failing-test-first applies (write
the test, verify it fails for the expected reason, implement,
verify it passes). Items 2 (PandocAST consolidation) and 9 (link
handlers extraction) are **behavior-preserving refactors** — the
canonical "failing test" doesn't exist because there's no behavior
change to test for. The test gate for these items is **"existing
tests pass before AND after the refactor"** (specifically the
`iframePostProcessor.test.ts` and `.integration.test.ts` suites for
item 9; the workspace `tsc -b` for item 2). Structure each refactor
commit so it is independently verifiable: run the relevant test
suite at the start of the commit (should pass), make the mechanical
move, run the suite again (should still pass).

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
- **Image renderer component tests** (vitest): mount `<Ast>` with a
  fixture containing `Image` nodes pointing at (a) project-relative
  paths (`hero.png`), (b) `/.quarto/...` paths (shouldn't appear
  in q2-preview but the renderer should pass them through), (c)
  external URLs, and (d) `data:` URIs. Mock VFS via the registry
  context's `currentFilePath`. Assert the rendered `<img>` for case
  (a) has a `data:` URI src; cases (b), (c), (d) keep their
  original src.
- **Image rendering integration test (AST iframe)**: render an AST
  with `<img src="hero.png">`, populate the VFS at the resolved
  project-relative path with a fake image, mount the iframe, assert
  the rendered `<img>` has a `data:` URI src on first paint. No
  flicker assertion needed — the rewrite happens during render, not
  after. Re-render with a different image src and assert the new
  image is also resolved.
- **HTML iframe image rewriter regression**: existing
  `iframePostProcessor.test.ts` and `.integration.test.ts` suites
  pass without modification. The HTML iframe rewriter is
  **unchanged** by this plan; this gate confirms no accidental
  collateral damage.
- **Link handler unit tests** (vitest): build a representative
  `Document`, build `ctxRef = { current: { onQmdLinkClick,
  currentFilePath: '/foo.qmd' } }`, attach `installLinkHandlers(doc,
  ctxRef)`, dispatch synthetic clicks on:
  `<a href="https://example.com">` (assert `window.open` called
  with `_blank`), `<a href="other.qmd#sec">` (assert
  `onQmdLinkClick({ path: '/other.qmd', anchor: 'sec' })`),
  `<a href="#sec">` (assert `onQmdLinkClick({ anchor: 'sec' })`),
  and a non-`.qmd` non-anchor href (assert no handler call,
  default click behavior preserved). Dispatch a synthetic
  `Cmd+S` keydown on `window`, assert the parent postMessage
  fires. **Stale-closure regression**: mutate
  `ctxRef.current.currentFilePath` to `'/bar.qmd'`, re-dispatch a
  click on `<a href="other.qmd">`, assert `onQmdLinkClick`
  resolves relative to the *new* base.
- **Theme CSS fingerprint-keyed re-injection test** (vitest):
  mount `<AstWithAssets>` with `themeFingerprint='abc'` and a fake
  `styles.css` containing bytes A; assert one
  `<style data-q2-theme>` element with bytes A in `document.head`.
  Re-render with the same `themeFingerprint='abc'` — assert the
  same single `<style>` element (no duplication; effect short-
  circuits on unchanged dep). Re-render with
  `themeFingerprint='def'` and bytes B in VFS — assert the same
  single `<style>` element but with bytes B (textContent updated,
  not appended). Mount/unmount/mount under StrictMode dev — assert
  exactly one `<style data-q2-theme>` element exists (the marker
  doubles as a StrictMode idempotency guard).
- **`injectPreviewStyles` idempotency unit test** (vitest):
  call `injectPreviewStyles(doc)` twice on the same document;
  assert exactly one `<style data-hub-client>` element exists in
  `doc.head`.
- **Rust-side `themeFingerprint` surfacing test** (cargo nextest):
  construct a `RenderResponse` for a single render with a known
  theme; assert `response.theme_fingerprint == Some(theme_fingerprint(css))`
  for the response's theme CSS. Render the same fixture twice with
  the same theme; assert fingerprints are byte-identical. Render
  with a different theme; assert fingerprints differ.
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

- **`iframePostProcessor.ts` refactor regression**. Only the
  link-handler extraction (item 9) and the `injectPreviewStyles`
  export + idempotency guard (item 9) touch this file. The image
  rewriter is **not** extracted — item 8 ships a different
  approach (render-time resolution in the `Image` component), so
  `iframePostProcessor.ts:177-210` is left alone. The HTML iframe
  must remain behavior-identical: existing
  `iframePostProcessor.test.ts` and
  `iframePostProcessor.integration.test.ts` suites pass before and
  after.
- **`PandocAST` consolidation type drift**. The duplicate
  definitions have drifted on naming (`Block`/`Inline` vs
  `BlockNode`/`InlineNode`) and inline-variant coverage. The
  consolidation picks `BlockNode`/`InlineNode` (richest existing
  shape) plus the `astContext?` field and `CustomBlockNode` /
  `CustomInlineNode` placeholders. Slide-side files keep their
  local `Block`/`Inline` exports (q2-slides stays on q2-debug
  path); they only update their `PandocAST` import path. Run
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
- **Empty-content artifact overwrite (out of scope; tracked at
  bd-3gtn)**. The WASM flush loop at
  `wasm-quarto-hub-client/src/lib.rs:1208-1214` and `:1364-1369`
  writes `artifact.content.clone()` to VFS without checking for
  empty content. `ResourceCollectorTransform` produces empty-content
  artifacts whose `path` resolves (via `Path::join` semantics on
  absolute paths) to the same VFS location where `automergeSync`
  writes user uploads — overwriting them with `Vec::new()` on each
  render. The `Image` renderer (item 8) reads user uploads as the
  source of truth, so the bug is parallel to 2A's image story
  rather than blocking it. Fix is one line in each flush loop
  (`if !artifact.content.is_empty() { ... }`); lives in **bd-3gtn**,
  not 2A scope.
- **Wire-format codes**. See
  `claude-notes/designs/wire-format-source-info-codes.md` for the
  allocation policy. Codes 0–3 are stable in production; codes 4–5
  are forward-declared in 2A's TS types and inert until Plan 5
  writes them. Adding new codes later requires synchronized writer
  + reader updates.

## Estimated scope

Items in implementation order (matching §"In scope" above).

| # | Component | Lines (rough) |
|---|---|---|
| 1 | `types/sourceInfo.ts` (mirror types) | ~50 |
| 2 | `types/pandoc.ts` consolidation + 5-consumer migration + delete dead file | ~150 (net negative after deletion) |
| 3 | `utils/sourceInfo.ts` (accessors + tests) | ~120 |
| 4 | `utils/atomicCustomNodes.ts` (TS hand-mirror) | ~30 |
| 5 | `RegistryContext` extension (`sourceInfoPool` + `currentFilePath`) + AST entry threading + Provider plumbing | ~50 |
| 6 | `ast-renderer.html` `:where()` wrap + regression test | ~30 |
| 7 | `render-components` gate extension + regression test | ~20 |
| 8 | `Image` renderer render-time resolution + component tests | ~50 |
| 9 | `iframeLinkHandlers.ts` extraction (ref-based context) + `injectPreviewStyles` idempotency guard | ~130 |
| 10 | `AstWithAssets` wrapper (2 useEffects: link, fingerprint-keyed theme CSS) + integration tests | ~80 |
| 11 | Rust-side `themeFingerprint` surfacing (`quarto-core` + WASM bridge + TS types + postMessage payload) + Rust test | ~30 |
| | **Total** | **~740** |

Two focused sessions are realistic. A natural split is **items
1–5** (type/data foundation including the `RegistryContext` plumbing
2B and item 8 consume; inert wiring for 2B) and **items 6–11**
(iframe glue plus the small Rust-side fingerprint surfacing; lights
up theme CSS, images, and link navigation visibly). Either session
can land independently of the other — items 1–5 don't visibly change
anything beyond the Provider's new value shape, and items 6–11 don't
depend on the source-info types existing.

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

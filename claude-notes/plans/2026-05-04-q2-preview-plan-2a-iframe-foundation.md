# Plan 2A — q2-preview iframe foundation (revised post-2pre)

**Date:** 2026-05-04 (revised 2026-05-07)
**Branch:** feature/q2-preview
**Status:** Implementation plan
**Milestone:** M2-foundation. q2-preview is a working iframe surface, parallel to q2-debug, ready for Plan 2B to fill its component registry.

## Goal

Stand up q2-preview as a sibling of q2-debug under the directory layout Plan 2pre establishes. After 2A:

- `hub-client/src/components/render/q2-preview/` exists with:
  - `entry.tsx` — iframe entry, mirrors q2-debug's pattern.
  - `PreviewIframe.tsx` — iframe wrapper, parallel to `q2-debug/DebugIframe.tsx`.
  - `PreviewContext.tsx` — q2-preview-specific React context (carries `currentFilePath`).
  - `registry.ts` — q2-preview registry skeleton with a minimal `'Ast'` entry (the document-root wrapper) and a generic "Not registered" fallback for unknown node types. **No leaf components ship in 2A.** Plan 2B fills them.
- `hub-client/public/q2-preview.html` — iframe HTML page parallel to `ast-renderer.html`.
- `ReactRenderer.tsx` routes `format: q2-preview` through `PreviewIframe`; q2-debug stays on `DebugIframe`.
- The framework's `RegistryContext` carries `sourceInfoPool` (added here, used by Plan 2B's atomic-aware dispatcher gate).
- Theme CSS produced by `CompileThemeCssStage` is injected into q2-preview's iframe head, fingerprint-keyed for live reload.
- Link handlers (external new-tab, `.qmd` clicks, anchor clicks, `Cmd+S` save) are extracted into a shared utility and installed by q2-preview's entry.
- `render-components` gate covers q2-preview alongside q2-debug.
- `__Q2_PREVIEW_RENDERER__` global is set on q2-preview's iframe window for user TSX overrides.
- TypeScript types for the source-info pool and `atomicCustomNodes` ship as shared utilities (used by 2B).

q2-preview at the end of 2A renders every node as a "Not registered: T" placeholder. Theme CSS is loaded; links navigate. **No content is visibly readable yet.** That is intentional — 2A's job is the iframe surface; 2B's job is the leaves.

## Scope

### In scope

The list below is in implementation order. Items 1–4 are shared utilities (consumed by both 2A and 2B). Items 5–9 stand up the q2-preview surface. Items 10–11 light up theme CSS and Rust-side fingerprint plumbing.

#### 0. `artifactPaths.ts` TS↔Rust mirror

`hub-client/src/types/artifactPaths.ts`, new file. Mirrors the constant defined in `crates/quarto-core/src/pipeline.rs:81`:

```ts
/**
 * VFS path for the compiled theme CSS artifact. Mirrors
 * `DEFAULT_CSS_ARTIFACT_PATH` in `crates/quarto-core/src/pipeline.rs:81`.
 *
 * Sync convention: when the Rust constant changes, update this file
 * and re-run hub-client tests. Matches the `types/diagnostic.ts` ↔
 * `DiagnosticMessage` pattern.
 */
export const DEFAULT_CSS_ARTIFACT_PATH = '/.quarto/project-artifacts/styles.css';
```

q2-preview's entry (item 9) imports this for the theme CSS read. Resolves the original open question about how the constant reaches the JS side: **option chosen — TS hand-mirror file, matching the existing TS↔Rust mirror pattern** (`types/diagnostic.ts`, `types/intelligence.ts`, `utils/atomicCustomNodes.ts`). Light, contained, follows convention.

#### 1. Source-info pool TS type mirror

`hub-client/src/types/sourceInfo.ts`, new file. Mirrors the wire format defined in `crates/pampa/src/writers/json.rs:54-91`:

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

Codes 4 and 5 are dormant on the wire today — Plan 5 wires them up. The TS type already accepts them so 2A doesn't need amendment when Plan 5 ships.

`By` is intentionally coarse in 2A — `{ kind: string; data?: unknown }` matches Plan 4's open Rust struct. Plan 4 introduces specific builder methods per kind; once consumers branch on `kind`, this TS type becomes a candidate for narrowing to a discriminated union. Out of scope.

#### 2. Source-info accessor module

`hub-client/src/utils/sourceInfo.ts`, new file. Pure functions, no React:

- `entryFor(node, pool): SourceInfoEntry | undefined` — lookup by `s` field.
- `isDerived(node, pool): boolean` — true iff entry is type code 5 (Plan 6 populates).
- `isAtomicSourceInfo(node, pool, atomicKinds): boolean` — true iff `isDerived` OR `(entry.t === 4 && atomicKinds.has(entry.d.kind))`.
- `ATOMIC_SYNTHETIC_KINDS: ReadonlySet<string>` — exported empty set today; Plan 4 / 6 fill it. Header comment points at Plan 4's `By::is_atomic_synthesizer()` for the sync contract.

#### 3. `atomicCustomNodes.ts`

`hub-client/src/utils/atomicCustomNodes.ts`, new file. Hand-mirror of Plan 7's `crates/quarto-core/src/.../ATOMIC_CUSTOM_NODES` Rust const, owned by 2A because Plan 2B's atomic-aware dispatcher (in framework) is the first consumer. Initial built-in set: `["CrossrefResolvedRef"]`. Plan 8 amends to add `"IncludeExpansion"`. Header comment names the Rust source of truth and the sync convention (matches `types/diagnostic.ts` ↔ `DiagnosticMessage` and `types/intelligence.ts` ↔ `quarto-lsp-core`).

**Discovery mechanism**: the dispatcher recovers a wrapped CustomNode's kind from the `data-custom-type` attribute the JSON writer attaches to every wrapper Div (block) / Span (inline) — see `crates/pampa/src/writers/json.rs:1297-1325` (block) and `:1381+` (inline). The hand-mirror's strings match the writer's emitted `type_name` byte-for-byte.

#### 4. Extend framework's `RegistryContext` with `sourceInfoPool`

In `framework/RegistryContext.tsx` (created by Plan 2pre), extend the value type from:

```ts
{ registry: Record<string, ...> }
```

to:

```ts
{ registry: Record<string, ...>; sourceInfoPool?: SourceInfoPool }
```

(Plan 2pre dropped the `| null` from the value type as part of its dispatcher-fallback removal — see 2pre §"Dispatcher fallback removal." The default value is `{ registry: {} }`; 2A's extension is purely additive — `sourceInfoPool` is optional, so existing call sites that pass only `{ registry }` continue to type-check.)

Both q2-debug and q2-preview pass `sourceInfoPool` when available. q2-debug doesn't read it today; Plan 2B's atomic-aware dispatcher gate (in `framework/dispatchers.tsx`) reads it and benefits both formats.

#### 5. q2-preview iframe HTML page

`hub-client/public/q2-preview.html`, new file. Mirrors `ast-renderer.html` (q2-debug's page). Differences:

- Imports `q2-preview/entry.tsx` instead of `q2-debug/entry.tsx`.
- Body styles: minimal — no debug-specific reset. Use Bootstrap's body styling once theme CSS loads. (No `:where()` workaround needed; q2-preview's HTML can have its own minimal `<style>` since theme CSS will dominate.)

#### 6. q2-preview iframe wrapper

`hub-client/src/components/render/q2-preview/PreviewIframe.tsx`, new file. Parallel to `q2-debug/DebugIframe.tsx` (renamed in 2pre). Differences:

- `src="/q2-preview.html"` instead of `/ast-renderer.html`.
- Posts `themeFingerprint` to the iframe alongside `astJson` and `currentFilePath` (from item 11 — until 11 lands, this is `undefined` and is harmless).
- Otherwise structurally identical to `DebugIframe`.

#### 7. q2-preview context

`hub-client/src/components/render/q2-preview/PreviewContext.tsx`, new file. Carries q2-preview-specific values that don't belong on the framework context:

```tsx
export interface PreviewContextValue {
  currentFilePath: string;
}
export const PreviewContext = createContext<PreviewContextValue | null>(null);
```

Plan 2B's `Image` and other leaf components read `currentFilePath` via `useContext(PreviewContext)`.

#### 8. q2-preview registry skeleton

`hub-client/src/components/render/q2-preview/registry.ts`, new file. Empty registry plus:

- An `Ast` registry entry that simply calls `renderChildren({ node: ast, setLocalAst: setAst, ... })` with no debug wrapper. This is what `framework/Ast.tsx` looks up via `registry['Ast']`. (The registry key stays `'Ast'` for both formats — see 2pre §"What stays exactly the same." q2-preview's component implementing this entry can be named whatever — e.g. `PreviewDocument` — only the registry key needs to be `'Ast'`.)
- A fallback that returns `<div>Not registered: {nodeType}</div>` for any unknown `node.t`. Implemented by the framework's `Block` / `Inline` dispatchers when `registry[node.t]` is undefined — no per-format work needed; just don't register any leaves yet.

q2-preview at this point boots and renders every node as a "Not registered" placeholder. Plan 2B fills the registry.

#### 9. q2-preview entry

`hub-client/src/components/render/q2-preview/entry.tsx`, new file. Parallel to `q2-debug/entry.tsx`. Differences:

- Imports `framework` + `q2-preview/registry`.
- Sets `window.__Q2_PREVIEW_RENDERER__ = { ...framework, ...preview }` (parallel to q2-debug's `__REACT_AST_DEBUG_RENDERER__`).
- Mirrors q2-debug's `loadCustomComponents` pattern: when `LOAD_CUSTOM_COMPONENTS` arrives, sets `window.React = React`, `window.katex = katex` (Plan 2B's Math component will use it), and any other globals user TSX expects, then dynamically imports each transpiled blob and merges exports into the active registry. **Pre-existing bug to be aware of**: today's q2-debug `loadCustomComponents` accumulates wrong (each iteration overwrites `customRegistry` with `{ ...componentRegistry, ...module }` instead of `{ ...customRegistry, ...module }`, so only the last loaded file's exports survive). Mirror the pattern in q2-preview but **fix the bug** — q2-preview should accumulate correctly. Tracked at **bd-3day** (back-port the fix to q2-debug).
- Wraps the `<Ast>` mount in a small wrapper component that:
  - Provides `<PreviewContext.Provider value={{ currentFilePath }}>`.
  - Installs link handlers via `installLinkHandlers(document, ctx)` (item 10) — handlers capture mount-time props, no ref needed (the iframe remounts on doc switch; see §"Iframe lifecycle, researched" below).
  - Injects theme CSS into `document.head`, keyed on `themeFingerprint` (item 11; until 11 lands, `themeFingerprint` is `undefined` and the effect runs once on mount).

```tsx
function PreviewRoot(props: PreviewRootProps) {
  // Link handlers capture mount-time props. The iframe remounts on
  // doc switch (ReactPreview's previewState reset → ReactRenderer
  // unmount → AstIframe unmount → fresh iframe), so closures are
  // always fresh. See §"Iframe lifecycle, researched" for the chain.
  useEffect(() => {
    installLinkHandlers(document, {
      currentFilePath: props.currentFilePath,
      onQmdLinkClick: props.onNavigateToDocument,
    });
  }, []);  // [] is correct: closures will not go stale within one mount

  // Theme can change WITHIN a single document mount (user edits
  // `theme:` in YAML). Same iframe, same React tree, but
  // `themeFingerprint` changes → re-inject CSS bytes.
  useEffect(() => {
    const css = vfsReadFile(DEFAULT_CSS_ARTIFACT_PATH);  // from types/artifactPaths.ts (item 0)
    if (!css.success || !css.content) return;
    let style = document.head.querySelector<HTMLStyleElement>('style[data-q2-theme]');
    if (!style) {
      style = document.createElement('style');
      style.setAttribute('data-q2-theme', '1');
      document.head.appendChild(style);
    }
    style.textContent = css.content;
  }, [props.themeFingerprint]);

  return (
    <PreviewContext.Provider value={{ currentFilePath: props.currentFilePath }}>
      <Ast {...props} registry={previewRegistry} />
    </PreviewContext.Provider>
  );
}
```

#### 10. Link handlers extraction

`hub-client/src/utils/iframeLinkHandlers.ts`, new file. Extract external-new-tab, `.qmd` click, same-doc-anchor click, and `Ctrl+S`/`Cmd+S` save logic from `iframePostProcessor.ts:212-281` into:

```ts
export function installLinkHandlers(
  doc: Document,
  ctx: { currentFilePath: string; onQmdLinkClick: (...) => void }
): void;
```

Implementation uses **event delegation on `doc.body`** for clicks (single delegated listener avoids per-element re-attachment as React mutates the AST tree on content edits), and `doc.addEventListener('keydown', ...)` for the save shortcut (matching the HTML iframe's pattern at `iframePostProcessor.ts:276`).

Closures over `ctx` are captured at attach time. The iframe is remounted on doc switch (q2-debug's existing pattern; see §"Iframe lifecycle, researched"), so a stale closure is structurally impossible within a single mount — `currentFilePath` doesn't change underneath the listener.

The save shortcut posts `{ type: 'hub-client-save' }` — the existing protocol (see `App.tsx:391`).

External new-tab is handled via `window.open` + `preventDefault` rather than per-element `target="_blank"` attribute writes — delegation can't set attributes on every `<a>` without a re-walk per render.

The HTML iframe's per-element listener pattern (`iframePostProcessor.ts:222-237, 240-251`) stays as-is — only q2-preview's iframe uses delegation. The artifact-rooted `.html` reverse-mapping (`iframePostProcessor.ts:253-272`) is **not extracted** — it only matters when `LinkRewriteTransform` ran, which q2-preview's pipeline excludes.

#### 11. Rust-side `themeFingerprint` surfacing

Plumb `theme_fingerprint(css)` (already computed at `crates/quarto-core/src/stage/stages/compile_theme_css.rs:447`) onto `RenderResponse`, through the WASM bridge, into the postMessage payload that q2-preview's entry consumes:

1. **`quarto-core`**: add `theme_fingerprint: Option<String>` to `RenderResponse`. Populate from the active theme artifact's key. ~5 LOC.
2. **`wasm-quarto-hub-client/src/lib.rs`**: surface the field on the JS-facing return shape. ~3 LOC.
3. **hub-client TS types** (`types/render.ts` or wherever `RenderResponse` is mirrored): add the field. ~2 LOC.
4. **`PreviewIframe.tsx`** postMessage payload: pass `themeFingerprint` through alongside `astJson` and `currentFilePath`. ~2 LOC.
5. **`q2-preview/entry.tsx`**: receive and pass to `<PreviewRoot>` as a prop.

Items 1–10 can ship without 11; item 9's wrapper handles `themeFingerprint === undefined` as a no-op (no re-injection on theme change). Item 11 lights up live theme reload.

Because this item touches `quarto-core`, full `cargo xtask verify` (not `--skip-hub-build`) is required before merging — `wasm-quarto-hub-client` depends on `quarto-core` types and the WASM build is the only check that catches drift.

#### 12. Format dispatch in `ReactRenderer.tsx`

Update `ReactRenderer.tsx` to route q2-preview through `PreviewIframe`. **Preserve the existing `ErrorBoundary` + sizing-div wrapper around each branch** — it's currently shared by both formats and should stay shared:

```tsx
if (format === 'q2-debug') {
  return (
    <ErrorBoundary>
      <div style={{ width: '100%', height: '100%', position: 'absolute', top: 0, left: 0, right: 0, bottom: 0 }}>
        <DebugIframe astJson={astJson} currentFilePath={currentFilePath}
          onNavigateToDocument={onNavigateToDocument} setAst={setAst}
          customComponentsCode={customComponentsCode} />
      </div>
    </ErrorBoundary>
  );
}
if (format === 'q2-preview') {
  return (
    <ErrorBoundary>
      <div style={{ width: '100%', height: '100%', position: 'absolute', top: 0, left: 0, right: 0, bottom: 0 }}>
        <PreviewIframe astJson={astJson} currentFilePath={currentFilePath}
          themeFingerprint={themeFingerprint}
          onNavigateToDocument={onNavigateToDocument} setAst={setAst}
          customComponentsCode={customComponentsCode} />
      </div>
    </ErrorBoundary>
  );
}
```

`ErrorBoundary` catches render errors from user-supplied TSX (Plan 2A item 13's `render-components`). Both formats need it.

#### 13. `render-components` YAML key gate extension

In `ReactRenderer.tsx:103`, extend the gate from `format !== 'q2-debug'` to `format !== 'q2-debug' && format !== 'q2-preview'` so q2-preview demos can specify custom `.tsx` files. Add a comment explaining the dual coverage. ~5 LOC + a regression test.

q2-preview's user TSX overrides target the `__Q2_PREVIEW_RENDERER__` global; q2-debug's overrides target `__REACT_AST_DEBUG_RENDERER__`. Format determines which iframe loads which entry, and each entry sets its own global.

### Out of scope

Moved to **Plan 2B**:

- All q2-preview leaf components (Para, Plain, Header, ..., **Image, Figure**, Str, Space, Emph, Strong, Link, etc.) as real HTML.
- Type-specific CustomNode components (Callout, Theorem, Proof, FloatRefTarget, Equation, CrossrefResolvedRef, IncludeExpansion-stub).
- The unwrap / rewrap walks (`utils/customNode.ts`).
- Atomic-aware `setLocalAst` gating in framework's Block / Inline dispatchers.
- Pandoc base-type gap fills (LineBlock, DefinitionList, Table family, Underline, Strikeout, Superscript, Subscript, SmallCaps, Cite, RawInline, Note).
- Class-name constants module.
- Component snapshot tests, round-trip property tests.

Moved to **Plan 2pre** (already shipped before 2A):

- Directory restructure (`framework/` + `q2-debug/`).
- Deletion of dead `ReactAstRenderer.tsx`.
- Deletion of dead `transpileAndImportTSX` from `tsxTranspiler.ts`.
- PandocAST consolidation into `framework/types.ts`.
- Slide-side `Block`/`Inline` → `BlockNode`/`InlineNode` rename.
- Dispatcher `?? componentRegistry` fallback removal; `RegistryContext` default → `{ registry: {} }`; `<Ast>`'s `registry` prop made required.

Deferred to a future "q2-preview layout chrome" plan:

- Sidebar / Navbar / Footer / PageNav / TOC rendering.
- Body-class derivation (`docs-sidebar-{none,floating,docked}`, etc.).
- Navbar brand-title fallback.

## Design decisions (settled in 2026-05-07 review)

- **Parallel formats with shared framework**, not extension/override. q2-debug and q2-preview each own their registry and iframe entry; they share the `framework/` plumbing extracted by Plan 2pre. q2-debug's behavior is unchanged.
- **q2-preview gets its own iframe HTML page** (`q2-preview.html`) and its own iframe wrapper component (`PreviewIframe`). Two HTML pages + two wrappers, ~30 LOC of duplication, full clarity. The plan rejected URL-param dispatch as more coupling for no real saving.
- **`'Ast'` registry key is preserved across both formats.** q2-debug registers `'Ast': AstRenderer` (the bordered debug wrapper); q2-preview registers `'Ast': PreviewDocument` (or whatever name the component takes — registry key is what matters). Each format owns its own document-root component; the shared key just means `framework/Ast.tsx` does a single `registry['Ast']` lookup that resolves per-format. Preserving the key also keeps user TSX overrides like `~/docs/demo-playground/elliot/slide.tsx`'s `export const Ast` working unchanged. (See 2pre §"Dispatcher fallback removal" for the registry-injection invariant — `<Ast>`'s `registry` prop is now required.)
- **Keep `__REACT_AST_DEBUG_RENDERER__` global** for q2-debug's existing demos. Add `__Q2_PREVIEW_RENDERER__` for q2-preview. q2-debug demos keep working unchanged.
- **`PreviewContext` for `currentFilePath`**, not on the framework's `RegistryContext`. q2-debug doesn't need `currentFilePath`; only q2-preview's leaves do (e.g. `Image` resolution).
- **`sourceInfoPool` ON the framework's `RegistryContext`**. The atomic-aware dispatcher gate (Plan 2B) is correctness-level, lives in `framework/dispatchers.tsx`, and benefits both formats. Framework needs the pool.
- **Iframe lifecycle, researched.** q2-preview's AST iframe **remounts on every document switch**, matching q2-debug's existing behavior. The chain (verified in `ReactPreview.tsx:258-261`, `ReactRenderer.tsx:148`, `PreviewRouter.tsx:41-46, 100-109`):
  1. User switches file → `currentFile.path` changes.
  2. `ReactPreview` resets `previewState` to `'START'` (`ReactPreview.tsx:258-261`).
  3. The conditional render at `ReactPreview.tsx:290` flips false → `<ReactRenderer>` unmounts → `<AstIframe>` unmounts → `<iframe>` element destroyed.
  4. `PreviewRouter` additionally returns "Loading preview..." while `checkedPath !== currentFile?.path` — `<ReactPreview>` itself unmounts during the gap.
  5. After the new render completes (`setPreviewState('GOOD')`, `setAst(newAst)`), `<ReactRenderer>` mounts **fresh** — new iframe, new React root inside, fresh entry script execution.
  
  **Implications for q2-preview's design:**
  - Link handlers can use **prop-captured closures** — fresh mount means fresh closure. No ref-based context needed. Removes complexity vs. earlier draft.
  - Theme CSS effect's `themeFingerprint` dep is still useful, but for a different reason than originally framed: it handles **theme changes within a single document mount** (user edits `theme: flatly` → `theme: cosmo` in YAML). Same iframe, same React tree, fingerprint changes → effect re-fires, CSS re-injected. Cross-document theme changes are handled by the fresh mount (the new iframe reads the new theme on its mount-time effect).
  
  **Future possibility (out of 2A's scope).** The HTML preview path (`MorphIframe.tsx`, `DoubleBufferedIframe.tsx`) deliberately persists iframes across renders for performance (preserve scroll, DOM state, avoid re-init cost). The same pattern could be applied to the AST iframe later if perf demands it. Doing so would require switching to ref-based handlers at that point; until then, prop-captured closures are correct.

- **Inline `<style>` for theme CSS, fingerprint-keyed for theme switches within a document mount.** q2-preview's AST never carries `<link>` to rewrite (Pandoc nodes don't produce stylesheet links). The wrapper's effect keys on `themeFingerprint` to re-inject when underlying CSS changes. Single `<style data-q2-theme="1">` element reused across renders within one mount; the data attribute doubles as a StrictMode idempotency guard.
- **Cmd+S protocol uses `{ type: 'hub-client-save' }`**, matching `App.tsx:391` and `iframePostProcessor.ts:279`. Earlier drafts of this plan used `{ kind: 'save' }`; that would silently no-op against the existing parent listener.
- **Empty registry in 2A is intentional.** The "fallback only" state lets 2A be a pure surface plumbing concern. Visible content rendering belongs to 2B's component work.

## Soft activation dependencies

2A lands inert wiring that activates organically as later plans land:

- **Plan 4** introduces `Synthetic { by: By }` and `Derived { from, by }` SourceInfo variants. 2A's accessor recognizes wire codes 4 and 5; until Plan 4 / 5 wire them up, no entry has those codes.
- **Plan 5** adds wire format codes 4 and 5 to the JSON writer.
- **Plan 6** populates Derived source_info on shortcode resolutions.
- **Plan 7** ships the Rust `ATOMIC_CUSTOM_NODES` const + `is_atomic_custom_node()` function.
- **Plan 8** amends `atomicCustomNodes.ts` to add `"IncludeExpansion"`.
- **Plan 2B** ships the atomic-aware dispatcher gate in `framework/dispatchers.tsx`. Until 2B lands, 2A's `sourceInfoPool` plumbing on `RegistryContext` is unread.

## Multi-plan contracts

### Consumed: theme CSS artifact + fingerprint signal (from Plan 1 + item 11)

Plan 1's `RenderToPreviewAstRenderer` writes the compiled theme CSS to `/.quarto/project-artifacts/styles.css` (per `pipeline.rs::DEFAULT_CSS_ARTIFACT_PATH`) on every q2-preview render. The path is constant across theme swaps; only the bytes change. q2-preview's entry's theme effect reads the bytes when it fires and injects them into a single `<style data-q2-theme="1">` element.

Item 11 surfaces `theme_fingerprint(css)` onto `RenderResponse` as `themeFingerprint`. The wrapper's effect keys on this so live theme swaps trigger re-injection.

### Consumed: page-scoped image artifacts (from Plan 1)

The renderer does not contribute image bytes. Image bytes come from the user's original VFS upload (`automergeSync` → `vfsAddBinaryFile`). Plan 2B's `Image` component reads `currentFilePath` from `PreviewContext`, resolves user-written `src` paths against the document's directory, and reads VFS bytes synchronously into a `data:` URI.

`<img src>` in q2-preview's AST keeps the user's original path — `LinkRewriteTransform` explicitly leaves `Image::target.0` alone, no other transform mutates it. **No `/.quarto/...` paths appear in q2-preview's body AST** — the `/.quarto/...` branch in the HTML iframe's inline rewriter has no analog here.

### Provided: source-info pool accessor (for Plan 2B and beyond)

2A ships typed access to the source-info pool:
- `types/sourceInfo.ts` for the wire-format types.
- `utils/sourceInfo.ts` for the accessor functions.
- Framework's `RegistryContext` extension for in-iframe distribution.

Plan 2B's atomic-aware dispatcher reads these. Future features (preimage navigation, source-mapped diagnostics in the iframe) can build on the same accessors.

### Provided: atomicCustomNodes hand-mirror (for Plan 2B and Plan 7)

2A ships `utils/atomicCustomNodes.ts` with the initial built-in set (`["CrossrefResolvedRef"]`). Plan 2B's atomic-aware dispatcher imports `isAtomicCustomNode(typeName)` from this file. Plan 7 ships the Rust counterpart; sync convention documented in the file's header comment.

## Open questions / decisions for implementation

- **`DEFAULT_CSS_ARTIFACT_PATH` JS-side mirror — resolved.** Item 0 ships `hub-client/src/types/artifactPaths.ts` as a TS hand-mirror, matching the existing TS↔Rust pattern. q2-preview's entry (item 9) imports `DEFAULT_CSS_ARTIFACT_PATH` from there.

- **Iframe lifetime claim — resolved.** Researched in 2026-05-07 session. The AST iframe remounts on every document switch (driven by `ReactPreview.tsx:258-261`'s previewState reset). q2-preview uses prop-captured closures, not ref-based context. See §"Iframe lifecycle, researched" in Design decisions.

- **`Image` alt-text and Figure block handling are 2B's concern.** Mentioned here because 2A's `PreviewContext` plumbs the `currentFilePath` they need. Full Pandoc Image semantics (alt-from-inlines, `width`/`height` from kvs, title attribute, id/classes) are 2B scope.

## RenderResponse change risk (item 11) — researched

`RenderResponse` is defined in `crates/wasm-quarto-hub-client/src/lib.rs:766` with `#[derive(Serialize, Default)]`. It is **not constructed outside that file** — every `RenderResponse { ... }` literal in the codebase is in `lib.rs` (5 sites: lines 1217, 1397, 1419, 1441, 1471). Adding `theme_fingerprint: Option<String>` to the struct:

- Is non-breaking at the struct level (Default derive auto-defaults new optional fields to `None`).
- Requires updating all 5 constructors to set the new field, because they list every field explicitly rather than using `..Default::default()`. Mechanical, contained.
- Does not affect any other crate (no external consumers).

CLI render path is unaffected — it doesn't use `RenderResponse` (which is a JS-bridge serialization shape, not a render pipeline output type).

## References

### Rust side

2A modifies Rust in one place — item 11's `themeFingerprint` field on `RenderResponse`, plumbed through `wasm-quarto-hub-client`. Other references are read-only.

- `crates/pampa/src/writers/json.rs:54-91` — wire format types.
- `crates/pampa/src/writers/json.rs:300-330` — `add_source_info` on each node.
- `crates/pampa/src/writers/json.rs:1297` — `write_custom_block`.
- `crates/pampa/src/writers/json.rs:1380` — `write_custom_inline`.
- `crates/quarto-source-map/src/source_info.rs:22-55` — SourceInfo enum.
- `crates/quarto-core/src/pipeline.rs:81` — `DEFAULT_CSS_ARTIFACT_PATH`.
- `crates/quarto-core/src/stage/stages/compile_theme_css.rs:447` — `theme_fingerprint(css)` (item 11 surfaces).

### hub-client side (post-2pre paths)

- `hub-client/src/components/render/framework/RegistryContext.tsx` — extended by item 4 with `sourceInfoPool`.
- `hub-client/src/components/render/q2-preview/` — new directory.
- `hub-client/public/q2-preview.html` — new.
- `hub-client/src/components/render/ReactRenderer.tsx:101-111` — `render-components` gate (q2-preview added by item 13).
- `hub-client/src/components/render/ReactRenderer.tsx:148` — format dispatch (q2-preview routed to PreviewIframe by item 12).
- `hub-client/src/utils/iframePostProcessor.ts:212-281` — source for link-handler logic to extract.
- `hub-client/src/types/diagnostic.ts`, `hub-client/src/types/intelligence.ts` — existing TS↔Rust mirror patterns to follow for `atomicCustomNodes` and (option 2) `artifactPaths`.

## Test plan

### TDD discipline per work-item

The TDD discipline applies per work-item, not per-plan. Item 10 (link handlers extraction) is a behavior-preserving extraction — gate is "existing tests pass before AND after." Other items are greenfield additions; failing-test-first applies.

### Tests

- **Source-info accessor unit tests**: build representative `astJson` strings containing each wire code (0–5), parse them, assert `entryFor` / `isDerived` / `isAtomicSourceInfo` return correct values. Codes 4–5 use hand-constructed JSON until Plan 5 ships writer support.
- **`render-components` gate regression test** (vitest): mount `ReactRenderer` with `format: q2-preview` and a `render-components: [foo.tsx]` AST; assert `customComponentsCode` is populated. Sibling regression test for q2-debug confirms behavior unchanged.
- **`PreviewIframe` boot smoke test**: mount with a minimal `astJson` and `currentFilePath`, assert the iframe loads `/q2-preview.html` and reaches `IFRAME_READY`.
- **q2-preview registry fallback**: mount the iframe with an AST containing a `Para`, assert it renders as `<div>Not registered: Para</div>` (or whatever the fallback shape is). This confirms 2A's "empty registry" state is what we said it is.
- **Link handler unit tests** (vitest): build a representative `Document`, attach `installLinkHandlers(doc, { currentFilePath: '/foo.qmd', onQmdLinkClick })`, dispatch synthetic clicks on:
  - `<a href="https://example.com">` — assert `window.open` called with `_blank`.
  - `<a href="other.qmd#sec">` — assert `onQmdLinkClick({ path: '/other.qmd', anchor: 'sec' })`.
  - `<a href="#sec">` — assert `onQmdLinkClick({ anchor: 'sec' })`.
  - Non-`.qmd` non-anchor href — assert no handler call, default click behavior preserved.
  - Synthetic `Cmd+S` keydown on `doc` — assert parent postMessage `{ type: 'hub-client-save' }` fires.
  
  No stale-closure regression test — closures are captured at attach time and the iframe's lifetime equals the document's mount; `currentFilePath` cannot change underneath a single installation.
- **Theme CSS fingerprint-keyed re-injection test** (vitest): mount `<PreviewRoot>` with `themeFingerprint='abc'` and a fake `styles.css` containing bytes A; assert one `<style data-q2-theme>` element with bytes A in `document.head`. Re-render with the same fingerprint — assert no duplication. Re-render with `themeFingerprint='def'` and bytes B — assert single `<style>` element, content updated. Mount/unmount/mount under StrictMode — assert exactly one element.
- **Rust-side `themeFingerprint` surfacing test** (cargo nextest): construct a `RenderResponse` for a single render with a known theme; assert `response.theme_fingerprint == Some(theme_fingerprint(css))`. Render twice with the same theme; assert fingerprints byte-identical. Render with a different theme; assert fingerprints differ.
- **HTML iframe link handler regression**: existing `iframePostProcessor.test.ts` and `.integration.test.ts` suites pass without modification.

## Dependencies

### Hard dependencies

- **Plan 2pre** — directory restructure. 2A's items 4, 6, 7, 9, 12 reference paths and renames Plan 2pre establishes.
- **Plan 1** — pipeline, format detection, `RenderResponse.ast_json`, `pipeline_kind` dispatch, theme-CSS / page-scoped-image VFS contracts. All shipped.

### Blocks

- **Plan 2B** — q2-preview registry contents. 2B consumes every artifact 2A ships (PreviewIframe, PreviewContext, registry skeleton, sourceInfoPool plumbing, atomicCustomNodes utility, link handlers, theme CSS injection, render-components gate).
- Independent of Plans 4 / 5 / 6 / 7 / 8 — 2A's source-info wiring is forward-compatible with all of them.

## Risk areas

- **Iframe lifecycle (resolved, not a risk)**. Researched in 2026-05-07: the AST iframe remounts on doc switch. Plan now uses prop-captured closures and doesn't depend on persistence. Listed here for cross-reference; no implementation gotcha remains. See §"Iframe lifecycle, researched" in Design decisions.
- **`DEFAULT_CSS_ARTIFACT_PATH` JS-side mirror choice**. Whichever option (above) is picked, document the sync convention.
- **`render-components` gate change visibility**. The current one-line gate is buried in a `useMemo`; easy to miss. Add a comment explaining the gate's semantics now that q2-preview is also covered.
- **Empty-content artifact overwrite (out of scope; tracked at bd-3gtn)**. The WASM flush loop at `wasm-quarto-hub-client/src/lib.rs:1208-1214` and `:1364-1369` writes empty bytes to VFS without checking. `ResourceCollectorTransform` produces empty-content artifacts whose path resolves to the user's upload location. Plan 2B's `Image` reads user uploads as the source of truth, so the bug is parallel to 2A's image story rather than blocking it.
- **Wire-format codes**. See `claude-notes/designs/wire-format-source-info-codes.md`. Codes 0–3 are stable; codes 4–5 are forward-declared in 2A's TS types and inert until Plan 5 writes them.

## Estimated scope

| # | Component | Lines (rough) |
|---|---|---|
| 0 | `types/artifactPaths.ts` (TS hand-mirror) | ~15 |
| 1 | `types/sourceInfo.ts` | ~50 |
| 2 | `utils/sourceInfo.ts` (accessors + tests) | ~120 |
| 3 | `utils/atomicCustomNodes.ts` | ~30 |
| 4 | Framework `RegistryContext` extension | ~5 |
| 5 | `q2-preview.html` | ~30 |
| 6 | `PreviewIframe.tsx` | ~80 |
| 7 | `PreviewContext.tsx` | ~15 |
| 8 | `q2-preview/registry.ts` skeleton (`'Ast'` entry only) | ~25 |
| 9 | `q2-preview/entry.tsx` (PreviewRoot, theme CSS, link handlers wiring) | ~120 |
| 10 | `utils/iframeLinkHandlers.ts` extraction | ~120 |
| 11 | Rust-side `themeFingerprint` surfacing — 5 constructor sites + new field + test | ~40 |
| 12 | `ReactRenderer.tsx` format dispatch update | ~10 |
| 13 | `render-components` gate extension + regression test | ~20 |
| | **Total** | **~680** |

One focused session is realistic; possibly two. Natural split:
- **Session A**: items 1–4 (shared utilities) + items 5–8 (q2-preview surface scaffolding). Verifies the iframe boots empty.
- **Session B**: items 9–13 (entry wrapper, link handlers, themeFingerprint, format dispatch, gate). Verifies theme CSS loads and links navigate.

## Notes

- This plan replaces the original Plan 2A. The original assumed q2-preview would extend q2-debug's component registry; the 2026-05-07 review established the parallel-formats / shared-framework architecture, codified in **Plan 2pre** which lands first.
- Image rendering moved to **Plan 2B** as the first concrete leaf in q2-preview's registry. Image needs full Pandoc semantics (alt-text, attrs, title, kvs) and pairs with Figure block-level handling — both fit naturally in 2B's "fill the registry" scope.
- The "rename `MaybeReadOnlyInline`" question from the original Plan 2 remains resolved in 2B: there's no separate wrapper component; atomic-aware `setLocalAst` gating folds into the framework's `Block` / `Inline` dispatchers (Plan 2B work, in `framework/`).
- Forward-compat dormancy is the explicit pattern for 2A's source-info wiring vs. Plan 5 wire-format codes 4/5.

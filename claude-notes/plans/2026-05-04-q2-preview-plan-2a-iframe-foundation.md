# Plan 2A — q2-preview iframe foundation (revised post-2pre)

**Date:** 2026-05-04 (revised 2026-05-07, 2026-05-08)
**Branch:** feature/q2-preview
**Status:** Implementation plan
**Milestone:** M2-foundation. q2-preview is a working iframe surface, parallel to q2-debug, ready for Plan 2B to fill its component registry.

## Goal

Stand up q2-preview as a sibling of q2-debug under the directory layout Plan 2pre establishes. After 2A:

- `hub-client/src/components/render/q2-preview/` exists with:
  - `entry.tsx` — iframe entry, mirrors q2-debug's pattern.
  - `Q2PreviewIframe.tsx` — iframe wrapper, parallel to `q2-debug/Q2DebugIframe.tsx`.
  - `PreviewContext.tsx` — q2-preview-specific React context (carries `currentFilePath`).
  - `dispatchers.tsx` — q2-preview's `Block` / `Inline` dispatchers (parallel to `q2-debug/dispatchers.tsx`, established in Plan 2pre). Each does the standard `registry[node.t]` lookup; on miss renders a muted-gray "X (not yet implemented)" placeholder — `<span style={{ color: '#888', fontStyle: 'italic' }}>{t} (not yet implemented)</span>` for inlines, the block equivalent for blocks. Required because Plan 2pre's refined architecture moves `Block`/`Inline` out of framework into format-owned files; framework's `Node` cannot dispatch without them. ~30 LOC.
  - `PreviewDocument.tsx` — q2-preview's document-root wrapper, registered into `registry.ts` under the `'Ast'` key. Calls `renderChildren({ node: ast, setLocalAst: setAst, ... })` with no debug styling. ~15 LOC.
  - `registry.ts` — q2-preview registry skeleton with `'Ast'` (`PreviewDocument`), `'Block'`/`'Inline'` (from `dispatchers.tsx` above), and nothing else. **No leaf components ship in 2A.** Plan 2B fills them; until then, every node renders as the muted-gray "not yet implemented" placeholder.
- `hub-client/public/q2-preview.html` — iframe HTML page parallel to `q2-debug.html`.
- `ReactRenderer.tsx` routes `format: q2-preview` through `Q2PreviewIframe`; q2-debug stays on `Q2DebugIframe`. The current combined `if (format === 'q2-debug' || format === 'q2-preview')` branch is split into two distinct branches.
- The framework's `RegistryContext` carries `sourceInfoPool` (added here, used by Plan 2B's atomic-aware dispatcher gate).
- Theme CSS produced by `CompileThemeCssStage` is read by `Q2PreviewIframe` (parent) via `vfsReadFile`, wrapped in a blob URL, and the URL string is posted to the iframe via a separate `UPDATE_THEME` message. Iframe consumes it as `<link rel="stylesheet" href={blobUrl}>` — no CSS bytes cross the postMessage boundary. Three-way `themeFingerprint` (`string | null | undefined`) distinguishes "theme present" / "no theme intended" / "render failed-or-pending" so transient errors don't strip styling. The same blob-URL pattern is the contract for image bytes (Plan 2B's image manifest), giving q2-preview a unified URL-based asset story that maps cleanly onto a future service-worker swap-in.
- Link handlers (external new-tab, `.qmd` clicks, anchor clicks, `Cmd+S` save) are extracted into a shared utility and installed by q2-preview's entry.
- `render-components` gate covers q2-preview alongside q2-debug.
- `__Q2_PREVIEW_RENDERER__` global is set on q2-preview's iframe window for user TSX overrides. Framework primitives are mirrored across both globals byte-identically (see §"TSX globals: portability and parity").
- TypeScript types for the source-info pool and `atomicCustomNodes` ship as shared utilities (used by 2B).

q2-preview at the end of 2A renders every node as a muted-gray "T (not yet implemented)" placeholder. The plumbing for theme CSS injection and link navigation is in place but **not user-visible** — there is no real-HTML content to apply theme CSS to, and no `<a>` elements for the link handlers to fire on. **No content is visibly readable yet.** That is intentional — 2A's job is the iframe surface; 2B's job is the leaves. Most of 2A's value is verified by unit tests; see §"Test plan" for what is end-to-end observable vs. unit-test-only.

## Scope

### In scope

The list below is in implementation order. Items 1–4 are shared utilities (consumed by both 2A and 2B). Items 5–9 stand up the q2-preview surface. Items 10–11 light up theme CSS and Rust-side fingerprint plumbing.

#### 0. `artifactPaths.ts` TS↔Rust mirror

`hub-client/src/types/artifactPaths.ts`, new file. Mirrors the constant defined in `crates/quarto-core/src/pipeline.rs:85`:

```ts
/**
 * VFS path for the compiled theme CSS artifact. Mirrors
 * `DEFAULT_CSS_ARTIFACT_PATH` in `crates/quarto-core/src/pipeline.rs:85`.
 *
 * Consumer: parent-side `Q2PreviewIframe` (item 6), which reads the
 * VFS bytes, mints a blob URL, and posts the URL via `UPDATE_THEME`. The
 * iframe entry never imports this constant — it only handles whatever
 * CSS bytes the parent posts.
 *
 * Sync convention: when the Rust constant changes, update this file
 * and re-run hub-client tests. Matches the `types/diagnostic.ts` ↔
 * `DiagnosticMessage` pattern.
 */
export const DEFAULT_CSS_ARTIFACT_PATH = '/.quarto/project-artifacts/styles.css';
```

Resolves the original open question about how the constant reaches the JS side: **option chosen — TS hand-mirror file, matching the existing TS↔Rust mirror pattern** (`types/diagnostic.ts`, `types/intelligence.ts`, `utils/atomicCustomNodes.ts`). Light, contained, follows convention.

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

Both q2-debug and q2-preview pass `sourceInfoPool` when available. q2-debug doesn't read it today; Plan 2B's atomic-aware gate (in framework's `Node` component inside `framework/dispatch.tsx` — the single recursion chokepoint that runs before each format's `Block`/`Inline` dispatcher) reads it and benefits both formats automatically.

#### 5. q2-preview iframe HTML page

`hub-client/public/q2-preview.html`, new file. Mirrors `q2-debug.html` (q2-debug's page, renamed in Plan 2pre). Differences:

- Imports `q2-preview/entry.tsx` instead of `q2-debug/entry.tsx`.
- Body styles: minimal — no debug-specific reset. Use Bootstrap's body styling once theme CSS loads. (No `:where()` workaround needed; q2-preview's HTML can have its own minimal `<style>` since theme CSS will dominate.)
- Add `'q2-preview': path.resolve(__dirname, 'public/q2-preview.html')` to `vite.config.ts` rollup inputs alongside the existing `'q2-debug'` entry (mirrors Plan 2pre task 2.3's pattern).

#### 6. q2-preview iframe wrapper

`hub-client/src/components/render/q2-preview/Q2PreviewIframe.tsx`, new file. Parallel to `q2-debug/Q2DebugIframe.tsx` (Plan 2pre tasks 2.4–2.5). Differences:

- `src="/q2-preview.html"` instead of `/q2-debug.html`.
- Accepts a new `themeFingerprint?: string | null` prop (three-way semantics — see below). Until item 11 lands, this is always `undefined` and the theme effect is a no-op.
- Adds a parent-side theme effect that owns the `UPDATE_THEME` lifecycle: reads VFS bytes, creates a blob URL, revokes the previous URL, posts the URL string. Same two-message pattern as `Q2DebugIframe`'s `LOAD_CUSTOM_COMPONENTS` + `UPDATE_AST` split — only URL strings (not bytes) ever cross the postMessage boundary, and the iframe consumes them via a native `<link rel="stylesheet">`.

  **Three-way `themeFingerprint` semantics** (set by item 11's plumbing in `ReactPreview.tsx`):

  | Value | Meaning | Effect |
  |---|---|---|
  | `string` | render succeeded; theme present | read bytes, create blob URL, post `{ cssUrl: blobUrl, fingerprint: s }` |
  | `null` | render succeeded; no theme intended (user has no `theme:` YAML key, or removed it) | post `{ cssUrl: null, fingerprint: null }` to clear |
  | `undefined` | render failed, errored before `CompileThemeCssStage`, or pre-first-render | **skip post** — last-good CSS persists in iframe |

  The distinction matters because a transient render error (YAML parse error, pipeline crash) shouldn't strip Bootstrap from the user's view while they edit. `undefined` means "we don't know what the theme should be right now"; `null` means "we know there's no theme."

  ```tsx
  // Track the last fingerprint we sent and the blob URL we created
  // for it. The URL is revoked when replaced (new fingerprint, or null)
  // and on iframe unmount. Both reset on IFRAME_READY so a fresh iframe
  // (post-doc-switch remount) gets a fresh blob URL unconditionally.
  const lastSentThemeFingerprintRef = useRef<string | null | undefined>(undefined);
  const currentThemeBlobUrlRef = useRef<string | null>(null);

  // Cleanup on iframe unmount: revoke any outstanding blob URL.
  useEffect(() => {
    return () => {
      if (currentThemeBlobUrlRef.current) {
        URL.revokeObjectURL(currentThemeBlobUrlRef.current);
        currentThemeBlobUrlRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    if (event.data.type === 'IFRAME_READY') {
      setIframeReady(true);
      lastSentThemeFingerprintRef.current = undefined;
      // Iframe restart: any prior blob URL we held is no longer
      // referenced by the new iframe, but the bytes are still ours
      // until revoked.
      if (currentThemeBlobUrlRef.current) {
        URL.revokeObjectURL(currentThemeBlobUrlRef.current);
        currentThemeBlobUrlRef.current = null;
      }
    }
    // ... other handlers as in Q2DebugIframe
  }, [...]);

  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;
    // undefined ⇒ unknown; skip the post entirely so last-good CSS persists.
    if (themeFingerprint === undefined) return;
    if (lastSentThemeFingerprintRef.current === themeFingerprint) return;

    // Replace the prior blob URL (if any). The iframe is about to swap
    // its <link href>; once it does the browser may still need the old
    // URL during the swap, but in practice <link> swap is synchronous
    // enough that revoking immediately after posting is safe — the
    // browser internally retains the bytes until the new <link> lands.
    if (currentThemeBlobUrlRef.current) {
      URL.revokeObjectURL(currentThemeBlobUrlRef.current);
      currentThemeBlobUrlRef.current = null;
    }

    let cssUrl: string | null = null;
    if (themeFingerprint !== null) {
      // string ⇒ known theme; read VFS bytes and mint a blob URL.
      const result = vfsReadFile(DEFAULT_CSS_ARTIFACT_PATH);
      if (result.success && result.content) {
        const blob = new Blob([result.content], { type: 'text/css' });
        cssUrl = URL.createObjectURL(blob);
        currentThemeBlobUrlRef.current = cssUrl;
      }
    }
    // null ⇒ render succeeded with no theme; cssUrl stays null to
    // clear the <link> element on the iframe side.

    iframeRef.current.contentWindow.postMessage(
      { type: 'UPDATE_THEME', cssUrl, fingerprint: themeFingerprint },
      '*'
    );
    lastSentThemeFingerprintRef.current = themeFingerprint;
  }, [iframeReady, themeFingerprint]);
  ```

  Message shape: `{ type: 'UPDATE_THEME', cssUrl: string | null, fingerprint: string | null }`. `null` is a wire-level value (clear the stylesheet); `undefined` only exists at the parent and never crosses the postMessage boundary (the effect early-returns instead of posting).

- Otherwise structurally identical to `Q2DebugIframe`: same `IFRAME_READY` handshake, same `UPDATE_AST` payload (`{ astJson, currentFilePath }` — no `themeFingerprint` field on this message), same `LOAD_CUSTOM_COMPONENTS` + custom-components flow.

#### 7. q2-preview context

`hub-client/src/components/render/q2-preview/PreviewContext.tsx`, new file. Carries q2-preview-specific values that don't belong on the framework context:

```tsx
export interface PreviewContextValue {
  currentFilePath: string;
}
export const PreviewContext = createContext<PreviewContextValue | null>(null);
```

Plan 2B's `Image` and other leaf components read `currentFilePath` via `useContext(PreviewContext)`.

#### 8. q2-preview registry skeleton + dispatchers

Two new files.

**`hub-client/src/components/render/q2-preview/dispatchers.tsx`** — q2-preview's `Block` and `Inline` dispatchers, parallel to `q2-debug/dispatchers.tsx` (Plan 2pre). Under Plan 2pre's refined architecture, framework reserves the `'Block'`/`'Inline'` registry keys but provides no implementations; each format must register its own. Both dispatchers do the standard `registry[node.t]` lookup; on miss they render a muted-gray "(not yet implemented)" placeholder:

```tsx
const placeholderStyle: React.CSSProperties = { color: '#888', fontStyle: 'italic' };

export const Block = (args: NodeArgs<BlockNode>) => {
  const ctx = useContext(RegistryContext);
  const Component = ctx.registry[args.node.t];
  return Component
    ? <Component {...args} />
    : <div style={placeholderStyle}>{args.node.t} (not yet implemented)</div>;
};

export const Inline = (args: NodeArgs<InlineNode>) => {
  const ctx = useContext(RegistryContext);
  const Component = ctx.registry[args.node.t];
  return Component
    ? <Component {...args} />
    : <span style={placeholderStyle}>{args.node.t} (not yet implemented)</span>;
};
```

The placeholder aesthetic is deliberately quiet — q2-preview's eventual goal is Quarto-Bootstrap parity, and the "not yet implemented" state should read as "quietly not here yet" rather than "alert" or "debug noise."

**`hub-client/src/components/render/q2-preview/registry.ts`** — registry assembly:

```ts
import type { FormatRegistry } from '../framework';
import { Block, Inline } from './dispatchers';
import { PreviewDocument } from './PreviewDocument';  // the 'Ast' entry component

export const previewRegistry: FormatRegistry = {
  Block,
  Inline,
  Ast: PreviewDocument,
};
```

The `FormatRegistry` annotation is the typed-format-registry contract introduced by Plan 2pre (§"Typed format-registry contract"). `Block` and `Inline` must satisfy `DispatcherComponent`; `PreviewDocument` must satisfy `AstComponent`. TypeScript catches register-time mistakes at this site.

`PreviewDocument` is q2-preview's document-root wrapper (registered under `'Ast'`). It calls `renderChildren({ node: ast, setLocalAst: setAst, ... })` with no debug wrapper. The registry key stays `'Ast'` for both formats — see 2pre §"What stays exactly the same"; only the registered component differs per format.

q2-preview at this point boots and renders every node as the muted-gray "(not yet implemented)" placeholder. Plan 2B fills the registry with real-HTML leaves.

#### 9. q2-preview entry

`hub-client/src/components/render/q2-preview/entry.tsx`, new file. Parallel to `q2-debug/entry.tsx`. Differences:

- Imports `framework` + `q2-preview/registry`.
- Sets `window.__Q2_PREVIEW_RENDERER__` to an explicit object (parallel to q2-debug's `__REACT_AST_DEBUG_RENDERER__` pattern from 2pre — *not* a wholesale `{ ...framework, ...preview }` spread). The minimal 2A surface:

  ```ts
  import { renderChildren, renderNode, Node } from '../framework';
  import { Block, Inline } from './dispatchers';
  import { previewRegistry } from './registry';

  (window as any).__Q2_PREVIEW_RENDERER__ = {
    renderChildren, renderNode, Node,
    Block, Inline,
    previewRegistry,
  };
  ```

  Plan 2B extends this object with q2-preview's leaf components as they ship. The explicit-object form locks the public surface and prevents framework internals (`renderChildrenRegistry`, `RegistryContext`) from leaking onto the global by accident — same rationale as 2pre's q2-debug global.

  **Set the `__Q2_PREVIEW_RENDERER__` global at module top, not inside `loadCustomComponents`.** The renderer-surface object is import-time-stable (no dynamic dependencies), so attaching it to `window` at the top of the module makes the surface available before any postMessage arrives. This is what enables the framework-primitive parity test in §"Test plan" to import the entry module and inspect `window.__Q2_PREVIEW_RENDERER__` directly — no message-firing setup required.

  **q2-debug parallel refactor (bundle with item 9).** q2-debug's existing entry sets its global inside `loadCustomComponents` alongside `window.React` and `window.katex`. Refactor: move the renderer-surface assignment to module top; leave `React` / `katex` lazy in `loadCustomComponents` since those are tied to dynamic user-TSX imports and that's the right time to set them. ~10 LOC moved, no runtime behavior change in production (the global ends up set in both cases by the time user TSX runs); enables the parity test to read both globals symmetrically.
- Mirrors q2-debug's `loadCustomComponents` pattern: when `LOAD_CUSTOM_COMPONENTS` arrives, sets `window.React = React`, `window.katex = katex` (Plan 2B's Math component will use it), and any other globals user TSX expects, then dynamically imports each transpiled blob and uses `buildCustomRegistry(loadedModules)` (`hub-client/src/utils/customRegistry.ts`) to merge exports. The accumulator bug originally tracked at bd-3day was fixed and back-ported to q2-debug in commit `1e5a930f` (2026-05-08); both formats now consume the same shared helper. No new bug-fix work in this plan.
- Wraps the `<Ast>` mount in a small wrapper component that:
  - Provides `<PreviewContext.Provider value={{ currentFilePath }}>`.
  - Installs link handlers via `installLinkHandlers(document, ctx)` (item 10) — handlers capture mount-time props, no ref needed (the iframe remounts on doc switch; see §"Iframe lifecycle, researched" below).
  - Subscribes to `UPDATE_THEME` postMessages (from `Q2PreviewIframe`, item 6) and stores `{ cssUrl, fingerprint }` in state.
  - Imperatively manages a single `<link rel="stylesheet" data-q2-theme>` element in `document.head`. Sets its `href` to the posted URL, or removes the element when `cssUrl === null`.

  No `vfsReadFile` import — the iframe never touches the parent's WASM context. The parent owns VFS reads and URL minting; the iframe is a pure URL consumer. URLs are tiny strings on the wire; CSS bytes never ride on postMessage.

  **Iframe-side `cssUrl` is two-way (`string | null`), not three-way.** The parent's `undefined` case (render failed / pre-first-render) is handled by the parent skipping the post entirely, so `undefined` never crosses the postMessage boundary. On the iframe side: `string` ⇒ set `<link href>` to that URL (browser fetches via the `blob:` protocol — synchronous-feeling, no network); `null` ⇒ remove the `<link>` element (or clear its `href`) to drop styling. The initial pre-first-message state is `themeUrl === null` (no element yet), distinct from a received `cssUrl: null` (explicit clear).

```tsx
type Theme = { cssUrl: string | null; fingerprint: string | null };

function PreviewRoot(props: PreviewRootProps) {
  const [theme, setTheme] = useState<Theme | null>(null);

  // Subscribe to UPDATE_THEME from the parent. Decoupled from
  // UPDATE_AST so the theme lifecycle is independent of AST updates.
  useEffect(() => {
    const handler = (event: MessageEvent) => {
      if (event.data?.type === 'UPDATE_THEME') {
        setTheme({ cssUrl: event.data.cssUrl, fingerprint: event.data.fingerprint });
      }
    };
    window.addEventListener('message', handler);
    return () => window.removeEventListener('message', handler);
  }, []);

  // Mirror the stored URL into a <link> in document.head. Single
  // element identified by data-q2-theme, replaced in-place when the
  // fingerprint changes. data-q2-theme also doubles as a StrictMode
  // idempotency guard.
  useEffect(() => {
    if (theme === null) return;
    let link = document.head.querySelector<HTMLLinkElement>('link[data-q2-theme]');
    if (theme.cssUrl === null) {
      // Explicit clear: remove the element.
      if (link) link.remove();
      return;
    }
    if (!link) {
      link = document.createElement('link');
      link.setAttribute('rel', 'stylesheet');
      link.setAttribute('data-q2-theme', '1');
      document.head.appendChild(link);
    }
    link.setAttribute('href', theme.cssUrl);
  }, [theme?.fingerprint]);

  // Link handlers capture mount-time props. The iframe remounts on
  // doc switch (ReactPreview's previewState reset → ReactRenderer
  // unmount → Q2PreviewIframe unmount → fresh iframe), so closures are
  // always fresh. See §"Iframe lifecycle, researched" for the chain.
  useEffect(() => {
    installLinkHandlers(document, {
      currentFilePath: props.currentFilePath,
      onQmdLinkClick: props.onNavigateToDocument,
    });
  }, []);  // [] is correct: closures will not go stale within one mount

  return (
    <PreviewContext.Provider value={{ currentFilePath: props.currentFilePath }}>
      <Ast {...props} registry={previewRegistry} />
    </PreviewContext.Provider>
  );
}
```

#### 10. Link handlers extraction

`hub-client/src/utils/iframeLinkHandlers.ts`, new file. Extract external-new-tab, `.qmd` click, same-doc-anchor click, and `Ctrl+S`/`Cmd+S` save logic from `iframePostProcessor.ts:213-281` into:

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

The HTML iframe's per-element listener pattern (`iframePostProcessor.ts:222-237, 240-251`) stays as-is — the contrast is *one-shot HTML walk vs continuously-re-rendered React DOM*, not q2-debug vs q2-preview. `iframePostProcessor` runs once after a `fetch`-and-`srcdoc` of server-rendered HTML; the DOM is walked once and listeners attach once. q2-preview is React DOM that re-renders on every keystroke, so per-element re-walks would compound. Delegation amortizes that cost. q2-debug currently has no link handlers at all (it's a debug view, not a navigation target) and is unaffected. The artifact-rooted `.html` reverse-mapping (`iframePostProcessor.ts:253-272`) is **not extracted** — it only matters when `LinkRewriteTransform` ran, which q2-preview's pipeline excludes.

#### 11. Rust-side `themeFingerprint` surfacing

Plumb `theme_fingerprint(css)` (already computed at `crates/quarto-core/src/stage/stages/compile_theme_css.rs:447`) onto `RenderResponse`, through the WASM bridge, through `ReactPreview` and `ReactRenderer` state, into the prop `Q2PreviewIframe` reads:

1. **`quarto-core` `RenderResponse`** (`wasm-quarto-hub-client/src/lib.rs:766`): add `theme_fingerprint: Option<String>`. Populate from the active theme artifact's key. Update all 5 constructor sites listed in §"RenderResponse change risk" — they list every field explicitly. ~10 LOC including constructors.
2. **hub-client TS types** (`hub-client/src/types/diagnostic.ts`, the `RenderResponse` interface at line 56): add `theme_fingerprint?: string` (snake_case to match Rust serde defaults, consistent with existing `ast_json` and `pass1_failures` fields). ~2 LOC.
3. **`ReactPreview.tsx` extraction** (~line 96, the q2-preview branch): extract `result.theme_fingerprint` from the `RenderResponse` and add `themeFingerprint: string | null` to the normalized object the function returns at ~line 106. **Three-way mapping**: render `success === false` ⇒ omit `themeFingerprint` from the returned object (item 4 will leave state alone); `success === true && result.theme_fingerprint === undefined` ⇒ `themeFingerprint: null`; `success === true && result.theme_fingerprint` is a string ⇒ pass through. ~5 LOC.
4. **`ReactPreview.tsx` state** (~line 176, alongside `const [ast, setAst] = useState<string>('')`): add `const [themeFingerprint, setThemeFingerprint] = useState<string | null | undefined>(undefined)`. **Initial state is `undefined`** (pre-first-render). At ~line 224 (next to `setAst(result.astJson)`), call `setThemeFingerprint(result.themeFingerprint)` only when the normalized object includes the field — this preserves last-good `themeFingerprint` across render errors (the `success === false` branch from step 3 omits the field, so this branch doesn't fire). ~5 LOC.
5. **`ReactPreview.tsx` prop** (~line 292, the `<ReactRenderer ... />` invocation): pass `themeFingerprint={themeFingerprint}`. ~1 LOC.
6. **`ReactRenderer.tsx` prop** (interface at ~line 58): accept `themeFingerprint?: string | null`. Forward to `Q2PreviewIframe` only (the q2-debug branch ignores it). ~3 LOC.
7. **`Q2PreviewIframe.tsx`**: receive `themeFingerprint?: string | null`. The CSS-posting effect from item 6 keys on this and implements the three-way semantics (early-return on `undefined`, post empty on `null`, post bytes on string). No further changes — the effect already exists.

**Three-way semantics rationale.** A transient render error (YAML parse error, pipeline crash mid-flight) leaves `themeFingerprint` state untouched (`undefined` if pre-first-render, otherwise its last-good value), so the iframe keeps showing the previous theme rather than dropping to Bootstrap-less rendering. A user removing `theme:` from YAML is a successful render that produces no theme artifact ⇒ `null` ⇒ explicit wipe.

Items 1–10 can ship without 11; item 6's effect treats `themeFingerprint === undefined` as skip-post, so 2A boots inert until 11 lights up theme plumbing.

Because this item touches `quarto-core`, full `cargo xtask verify` (not `--skip-hub-build`) is required before merging — `wasm-quarto-hub-client` depends on `quarto-core` types and the WASM build is the only check that catches drift.

#### 12. Format dispatch in `ReactRenderer.tsx`

`ReactRenderer.tsx:147` today routes both formats through a single combined branch:

```tsx
if (format === 'q2-debug' || format === 'q2-preview') {
  return (
    <ErrorBoundary>
      <div style={{...}}>
        <Q2DebugIframe ... />
      </div>
    </ErrorBoundary>
  );
}
```

**Split this combined branch into two distinct format-specific branches.** Preserve the `ErrorBoundary` + sizing-div wrapper inside each branch — both formats need it (it catches render errors from user-supplied TSX, see item 13):

```tsx
if (format === 'q2-debug') {
  return (
    <ErrorBoundary>
      <div style={{ width: '100%', height: '100%', position: 'absolute', top: 0, left: 0, right: 0, bottom: 0 }}>
        <Q2DebugIframe astJson={astJson} currentFilePath={currentFilePath}
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
        <Q2PreviewIframe astJson={astJson} currentFilePath={currentFilePath}
          themeFingerprint={themeFingerprint}
          onNavigateToDocument={onNavigateToDocument} setAst={setAst}
          customComponentsCode={customComponentsCode} />
      </div>
    </ErrorBoundary>
  );
}
```

The existing comment at `ReactRenderer.tsx:140-146` ("Both q2-debug and q2-preview render through the same Q2DebugIframe…") describes the *pre-split* behavior and should be removed in this commit.

**Bundle the smoke-all `PreviewIframeKind` extension into this commit as plumbing.** The smoke-all infrastructure on main today supports `'html' | 'q2-debug'`. Add `'q2-preview'` as the third kind in lockstep with the format-dispatch update:

- `hub-client/e2e/helpers/previewExtraction.ts:23` — `PreviewIframeKind` union grows `'q2-preview'`; `previewIframeSelector` (line 25) returns `'iframe[src*="q2-preview.html"]'` for the new kind.
- `hub-client/e2e/helpers/smokeAllDiscovery.ts` — fixture format allow-list grows `'q2-preview'`.
- `hub-client/e2e/smoke-all.spec.ts` — dispatch on the q2-preview kind (skip diagnostic-fetching, target the q2-preview iframe).

Mirrors the q2-debug extension that landed in commit `059dfeab`; ~10 LOC across three files. **No smoke-all spec asserts q2-preview content in 2A** — the extension is plumbing only, consumed by Plan 2B's fixtures once they land. The 2A vitest "Q2PreviewIframe boot smoke test" (in §"Test plan") is the only iframe-boot regression gate at 2A.

#### 13. `render-components` YAML key gate extension

In `ReactRenderer.tsx:98` (the `useMemo` early-return at the top of `componentPathsKey`), extend the gate from `format !== 'q2-debug'` to `format !== 'q2-debug' && format !== 'q2-preview'` so q2-preview demos can specify custom `.tsx` files. Add a comment explaining the dual coverage. ~5 LOC + a regression test.

q2-preview's user TSX overrides target the `__Q2_PREVIEW_RENDERER__` global; q2-debug's overrides target `__REACT_AST_DEBUG_RENDERER__`. Format determines which iframe loads which entry, and each entry sets its own global. Override files that consume only framework primitives can detect either global with a `??` fallback (see §"TSX globals: portability and parity").

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
- Framework-vs-format split: `Block`/`Inline` dispatchers move to `q2-debug/dispatchers.tsx`; framework reserves the registry keys but provides no implementations. (Drives 2A's new q2-preview `dispatchers.tsx` requirement.)
- Single `framework/dispatch.tsx` housing `Node`, `renderChildren`, `renderNode` (collapsed from the earlier three-file split to avoid cross-file circularity).
- Deletion of dead `ReactAstRenderer.tsx` and the parent `ReactAstDebugRenderer.tsx` (after carve-up).
- Deletion of dead `transpileAndImportTSX` from `tsxTranspiler.ts`.
- PandocAST consolidation into `framework/types.ts`.
- Slide-side `Block`/`Inline` → `BlockNode`/`InlineNode` rename.
- Dispatcher `?? componentRegistry` fallback removal; `RegistryContext` default → `{ registry: {} }`; `<Ast>`'s `registry` prop made required.
- `__REACT_AST_DEBUG_RENDERER__` global converted from wholesale spread to explicit-object form (sets the pattern 2A's `__Q2_PREVIEW_RENDERER__` follows).

Deferred to a future "q2-preview layout chrome" plan:

- Sidebar / Navbar / Footer / PageNav / TOC rendering.
- Body-class derivation (`docs-sidebar-{none,floating,docked}`, etc.).
- Navbar brand-title fallback.

## Design decisions (settled in 2026-05-07 review)

- **Parallel formats with shared framework**, not extension/override. q2-debug and q2-preview each own their registry and iframe entry; they share the `framework/` plumbing extracted by Plan 2pre. q2-debug's behavior is unchanged.
- **q2-preview gets its own iframe HTML page** (`q2-preview.html`) and its own iframe wrapper component (`Q2PreviewIframe`). Two HTML pages + two wrappers, ~30 LOC of duplication, full clarity. The plan rejected URL-param dispatch as more coupling for no real saving.
- **`'Ast'` registry key is preserved across both formats.** q2-debug registers `'Ast': AstRenderer` (the bordered debug wrapper); q2-preview registers `'Ast': PreviewDocument` (or whatever name the component takes — registry key is what matters). Each format owns its own document-root component; the shared key just means `framework/Ast.tsx` does a single `registry['Ast']` lookup that resolves per-format. Preserving the key also keeps user TSX overrides like `~/docs/demo-playground/elliot/slide.tsx`'s `export const Ast` working unchanged. (See 2pre §"Dispatcher fallback removal" for the registry-injection invariant — `<Ast>`'s `registry` prop is now required.)
- **Keep `__REACT_AST_DEBUG_RENDERER__` global** for q2-debug's existing demos. Add `__Q2_PREVIEW_RENDERER__` for q2-preview. q2-debug demos keep working unchanged.
- **`PreviewContext` for `currentFilePath`**, not on the framework's `RegistryContext`. q2-debug doesn't need `currentFilePath`; only q2-preview's leaves do (e.g. `Image` resolution).
- **`sourceInfoPool` ON the framework's `RegistryContext`**. The atomic-aware gate (Plan 2B) is correctness-level and lives in framework's `Node` component (the single recursion chokepoint, in `framework/dispatch.tsx`) — under 2pre's refined architecture, `Node` runs before each format's `Block`/`Inline` dispatcher and can no-op `setLocalAst` for atomic content centrally. Both formats benefit automatically. Framework needs the pool.
- **Iframe lifecycle, researched.** q2-preview's AST iframe **remounts on every document switch**, matching q2-debug's existing behavior. The chain (verified in `ReactPreview.tsx:257-260`, `ReactRenderer.tsx:147`, `PreviewRouter.tsx:42-43, 100-108`):
  1. User switches file → `currentFile.path` changes.
  2. `ReactPreview` resets `previewState` to `'START'` (`ReactPreview.tsx:257-260`).
  3. The conditional render at `ReactPreview.tsx:287` flips false → `<ReactRenderer>` unmounts → `<Q2PreviewIframe>` unmounts → `<iframe>` element destroyed.
  4. `PreviewRouter` additionally returns "Loading preview..." while `checkedPath !== currentFile?.path` — `<ReactPreview>` itself unmounts during the gap.
  5. After the new render completes (`setPreviewState('GOOD')`, `setAst(newAst)`), `<ReactRenderer>` mounts **fresh** — new iframe, new React root inside, fresh entry script execution.
  
  **Implications for q2-preview's design:**
  - Link handlers can use **prop-captured closures** — fresh mount means fresh closure. No ref-based context needed.
  - Theme CSS lifecycle (item 6) tracks the `lastSentThemeFingerprintRef` per iframe instance, resetting on each `IFRAME_READY` so a fresh iframe (post-doc-switch remount) gets the current CSS unconditionally. Within a single document mount, the same ref dedupes re-sends when only the AST changes; theme changes within a mount (user edits `theme: flatly` → `theme: cosmo` in YAML) trigger a re-send because `themeFingerprint` differs from the ref.
  
  **Future possibility (out of 2A's scope).** The HTML preview path (`MorphIframe.tsx`, `DoubleBufferedIframe.tsx`) deliberately persists iframes across renders for performance (preserve scroll, DOM state, avoid re-init cost). The same pattern could be applied to the AST iframe later if perf demands it. Doing so would require switching to ref-based handlers at that point; until then, prop-captured closures are correct.

- **Asset bytes flow as blob URLs, not as bytes-on-the-wire.** The iframe is sandboxed and does not initialize WASM, so it cannot call `vfsReadFile` directly. The parent (`Q2PreviewIframe`) reads bytes via `vfsReadFile`/`vfsReadBinaryFile`, wraps them in `new Blob(...)`, calls `URL.createObjectURL()`, and posts the resulting `blob:` URL string to the iframe. The iframe consumes URL strings via native browser primitives (`<link rel="stylesheet">` for CSS, `<img src>` for images — Plan 2B). No bytes ever ride on the postMessage channel.

  This unifies theme CSS (single artifact) and Plan 2B's image manifest (many per-page) under one architecture. Both consume URLs; both have their bytes minted by the parent and revoked when no longer referenced. The pattern maps onto a future service-worker swap-in: replace blob-URL minting with SW request interception, and the iframe's `<link>`/`<img>` semantics are unchanged.

  **Theme CSS specifics (item 6).** Parent reads `vfsReadFile(DEFAULT_CSS_ARTIFACT_PATH)` once per fingerprint change, mints a blob URL, revokes the previous URL, posts `{ type: 'UPDATE_THEME', cssUrl, fingerprint }`. The iframe maintains a single `<link data-q2-theme>` element in `document.head` whose `href` is the posted URL. The `data-q2-theme` attribute doubles as a StrictMode idempotency guard.

  **Three-way `themeFingerprint` (items 6 + 11) handles the no-theme vs error distinction.** `null` (render succeeded with no theme intended) clears the `<link>` element; `undefined` (render failed or pre-first-render) skips the post entirely so the last-good theme persists across transient errors. On the iframe side the element is removed when `cssUrl === null` and re-created when bytes return — the brief "no styling" gap is a deliberate signal that the user's `theme:` config produces no theme, distinct from the longer pre-first-render gap.

  **Blob URL revocation lifecycle.** Parent owns revocation. Three triggers: (1) replacement — when the fingerprint changes and a new URL is minted, the prior URL is revoked immediately after posting (browsers internally retain the bytes during a `<link>` swap, so revocation is safe); (2) `IFRAME_READY` — fresh iframe means any prior URL we held is no longer referenced, revoke it; (3) iframe unmount — useEffect cleanup revokes whatever URL is current.

  **Independence from Plan 2B's atomic-aware gate.** The `UPDATE_THEME` message handler lives in `entry.tsx`'s top-level `window.addEventListener('message')`. Plan 2B's atomic-aware gate lives inside `framework/dispatch.tsx`'s `Node` component, reading `sourceInfoPool` from `RegistryContext` and gating `setLocalAst` on atomic-CustomNode kinds. The two surfaces don't interact — one mutates `document.head` styling, the other gates AST mutations during dispatch. Adding this message type is invisible to 2B's gate work.
- **Cmd+S protocol uses `{ type: 'hub-client-save' }`**, matching `App.tsx:391` and `iframePostProcessor.ts:279`. Earlier drafts of this plan used `{ kind: 'save' }`; that would silently no-op against the existing parent listener.
- **Empty registry in 2A is intentional.** The "fallback only" state lets 2A be a pure surface plumbing concern. Visible content rendering belongs to 2B's component work.

## TSX globals: portability and parity

Both formats expose a `__REACT_AST_DEBUG_RENDERER__` (q2-debug) or `__Q2_PREVIEW_RENDERER__` (q2-preview) global on the iframe's `window`. They are not identical, but they share enough structure that one user TSX file can serve both formats — provided it stays within the framework-primitive subset.

**The framework-primitive subset is mirrored byte-identically across both globals.** Specifically: `renderChildren`, `renderNode`, `Node`, plus the dispatcher contract (`Block`, `Inline` accept `NodeArgs<T>`, lookup by `node.t`, return `<Component {...args} />` on hit, render a placeholder on miss). Both iframes import these from the same `framework/` source — different bundles, different identity at runtime, but identical behavior.

**The format-specific subset diverges by design.** q2-debug exports debug-style leaf components (`Para`, `Plain`, ..., `Quoted`) that render bordered debug strips; q2-preview (in 2B) will export real-HTML leaves under the same names. q2-debug also exports debug-only helpers (`blockStyle`, `inlineStyle`, `q2DebugRegistry`); q2-preview exports `previewRegistry`. These are not interchangeable — wrapping q2-debug's bordered `Para` in extra styling produces visually different output from wrapping q2-preview's `<p>{children}</p>`.

**Pattern A — semantic CustomNode override (works in both formats):** Use a `??` fallback and consume only framework primitives. The component renders semantic markup; the format wraps the children.

```tsx
const renderer = window.__Q2_PREVIEW_RENDERER__ ?? window.__REACT_AST_DEBUG_RENDERER__;
const { renderChildren } = renderer;

export const Callout = (args) => (
  <div className="callout">{renderChildren(args)}</div>
);
```

A single `callout.tsx` file referenced by both `format: q2-debug` and `format: q2-preview` qmd docs works in either iframe.

**Pattern B — format-specific helper consumption:** Target the matching global by name. Used for wrapping a format's existing leaves, or consuming format-specific helpers like `blockStyle`.

```tsx
const { Para: BasePara, blockStyle } = window.__REACT_AST_DEBUG_RENDERER__;
export const Para = (args) => <div style={{ ...blockStyle, color: 'red' }}><BasePara {...args} /></div>;
```

Format-agnostic CustomNode overrides that need format-specific look should be written as two files (one per format) following Quarto's existing format-specific resources convention.

**Stable contract: framework-primitive parity across globals.** Any framework primitive added in the future (e.g. a new dispatcher helper, a new context utility) MUST land on **both** globals in lockstep. This prevents silent drift where Pattern A code starts working in one format but not the other. Reviewers should treat asymmetric additions as a bug.

## Soft activation dependencies

2A lands inert wiring that activates organically as later plans land:

- **Plan 4** introduces `Synthetic { by: By }` and `Derived { from, by }` SourceInfo variants. 2A's accessor recognizes wire codes 4 and 5; until Plan 4 / 5 wire them up, no entry has those codes.
- **Plan 5** adds wire format codes 4 and 5 to the JSON writer.
- **Plan 6** populates Derived source_info on shortcode resolutions.
- **Plan 7** ships the Rust `ATOMIC_CUSTOM_NODES` const + `is_atomic_custom_node()` function.
- **Plan 8** amends `atomicCustomNodes.ts` to add `"IncludeExpansion"`.
- **Plan 2B** ships the atomic-aware gate inside framework's `Node` (in `framework/dispatch.tsx`). Until 2B lands, 2A's `sourceInfoPool` plumbing on `RegistryContext` is unread.

## Multi-plan contracts

### Consumed: theme CSS artifact + fingerprint signal (from Plan 1 + item 11)

Plan 1's `RenderToPreviewAstRenderer` writes the compiled theme CSS to `/.quarto/project-artifacts/styles.css` (per `pipeline.rs::DEFAULT_CSS_ARTIFACT_PATH`) on every q2-preview render. The path is constant across theme swaps; only the bytes change.

`Q2PreviewIframe` (parent) reads those bytes via `vfsReadFile`, wraps them in `new Blob([bytes], { type: 'text/css' })`, mints a blob URL via `URL.createObjectURL`, and posts the URL string to the iframe in an `UPDATE_THEME` message. Iframe consumes the URL via `<link rel="stylesheet" href={cssUrl} data-q2-theme>`. No CSS bytes cross the postMessage boundary.

Item 11 surfaces `theme_fingerprint(css)` onto `RenderResponse` as `themeFingerprint`, plumbed through `ReactPreview`/`ReactRenderer` to `Q2PreviewIframe`. The parent's effect keys on this so live theme swaps trigger a fresh blob URL + post; old URLs are revoked on each replacement.

### Provided: blob-URL asset contract (for Plan 2B's image manifest)

The same parent-mints-URL / iframe-consumes-URL pattern is the contract for image bytes. Plan 2A defines the contract; Plan 2B implements the image side.

**Contract:**
- **VFS reads happen on the parent**, not in the iframe. The iframe is sandboxed and does not initialize WASM; `vfsReadFile` / `vfsReadBinaryFile` only work in the parent context.
- **The parent mints blob URLs** via `URL.createObjectURL(new Blob([bytes], { type }))` and posts URL strings to the iframe.
- **The iframe consumes URLs natively** — `<link>` for stylesheets, `<img src>` for images. No JS-side byte handling.
- **The parent owns revocation**, with three triggers: replacement (URL changes for the same logical resource), `IFRAME_READY` (fresh iframe instance), iframe unmount.

**Plan 2B's image manifest applies this contract to images:**

`<img src>` in q2-preview's AST keeps the user's original path — `LinkRewriteTransform` explicitly leaves `Image::target.0` alone, no other transform mutates it. **No `/.quarto/...` paths appear in q2-preview's body AST.** Image bytes come from the user's original VFS upload (`automergeSync` → `vfsAddBinaryFile`).

Plan 2B specifies a parent-side asset walker (lives in `Q2PreviewIframe`) that runs once per `UPDATE_AST` cycle: walks the AST for `Image` nodes, resolves `Image::target.0` against `currentFilePath`, reads VFS bytes, mints blob URLs (memoized by path + content hash so unchanged images keep the same URL), and produces a `Record<origPath, blobUrl>` manifest. The manifest rides on the `UPDATE_AST` payload; iframe distributes it via a new `AssetManifestContext`. Plan 2B's `Image` component reads the context and looks up `node.target.0` in the manifest, falling back to the original URL for external paths and unresolved entries.

The `/.quarto/...` reverse-mapping branch from `iframePostProcessor.ts:177-210` (HTML-iframe inline rewriter) has no analog here — q2-preview's pipeline excludes `LinkRewriteTransform`, so this case never arises.

### Provided: source-info pool accessor (for Plan 2B and beyond)

2A ships typed access to the source-info pool:
- `types/sourceInfo.ts` for the wire-format types.
- `utils/sourceInfo.ts` for the accessor functions.
- Framework's `RegistryContext` extension for in-iframe distribution.

Plan 2B's atomic-aware dispatcher reads these. Future features (preimage navigation, source-mapped diagnostics in the iframe) can build on the same accessors.

### Provided: atomicCustomNodes hand-mirror (for Plan 2B and Plan 7)

2A ships `utils/atomicCustomNodes.ts` with the initial built-in set (`["CrossrefResolvedRef"]`). Plan 2B's atomic-aware dispatcher imports `isAtomicCustomNode(typeName)` from this file. Plan 7 ships the Rust counterpart; sync convention documented in the file's header comment.

## Open questions / decisions for implementation

- **`DEFAULT_CSS_ARTIFACT_PATH` JS-side mirror — resolved.** Item 0 ships `hub-client/src/types/artifactPaths.ts` as a TS hand-mirror, matching the existing TS↔Rust pattern. The constant is consumed by the parent (`Q2PreviewIframe`, item 6), not the iframe entry.

- **Iframe lifetime claim — resolved.** Researched in 2026-05-07 session. The AST iframe remounts on every document switch (driven by `ReactPreview.tsx:257-260`'s previewState reset). q2-preview uses prop-captured closures, not ref-based context. See §"Iframe lifecycle, researched" in Design decisions.

- **Where does the iframe get theme CSS bytes from? — resolved (2026-05-09).** The iframe is sandboxed and does not initialize WASM, so `vfsReadFile` cannot run there. The parent (`Q2PreviewIframe`) reads VFS bytes, wraps them in a blob URL, and posts the URL string in a separate `UPDATE_THEME` message; the iframe consumes via `<link rel="stylesheet" href={cssUrl}>`. Same blob-URL pattern is the contract for Plan 2B's image bytes. See §"Asset bytes flow as blob URLs…" in Design decisions, and items 6 + 9.

- **Framework-primitive parity contract — adopted.** Both iframe globals expose the framework primitives (`renderChildren`, `renderNode`, `Node`, `Block`, `Inline`) byte-identically; format-specific helpers diverge. Any future framework primitive MUST land on both globals in lockstep. See §"TSX globals: portability and parity".

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
- `crates/pampa/src/writers/json.rs:1381` — `write_custom_inline`.
- `crates/quarto-source-map/src/source_info.rs:22-55` — SourceInfo enum.
- `crates/quarto-core/src/pipeline.rs:85` — `DEFAULT_CSS_ARTIFACT_PATH`.
- `crates/quarto-core/src/stage/stages/compile_theme_css.rs:447` — `theme_fingerprint(css)` (item 11 surfaces).
- `crates/wasm-quarto-hub-client/src/lib.rs:766` — `RenderResponse` struct (item 11 extends).
- `crates/wasm-quarto-hub-client/src/lib.rs:1217, 1397, 1419, 1441, 1471` — five `RenderResponse` constructor sites (item 11 updates each).

### hub-client side (post-2pre paths)

- `hub-client/src/components/render/framework/RegistryContext.tsx` — extended by item 4 with `sourceInfoPool`.
- `hub-client/src/components/render/framework/dispatch.tsx` — Plan 2B's atomic gate lands inside `Node` here; 2A doesn't modify it but item 4 distributes the pool that 2B reads.
- `hub-client/src/components/render/q2-preview/` — new directory (`dispatchers.tsx`, `registry.ts`, `PreviewContext.tsx`, `Q2PreviewIframe.tsx`, `entry.tsx`, `PreviewDocument.tsx`).
- `hub-client/public/q2-preview.html` — new.
- `hub-client/src/components/render/ReactRenderer.tsx:98` — `render-components` gate (q2-preview added by item 13).
- `hub-client/src/components/render/ReactRenderer.tsx:147` — format dispatch (combined branch split by item 12; q2-preview routed to `Q2PreviewIframe`).
- `hub-client/src/components/render/ReactPreview.tsx:96, 176, 224, 292` — themeFingerprint extraction, state, and prop plumbing (item 11).
- `hub-client/src/utils/iframePostProcessor.ts:213-281` — source for link-handler logic to extract.
- `hub-client/src/utils/customRegistry.ts:14` — `buildCustomRegistry` helper consumed by both formats' entries.
- `hub-client/src/types/diagnostic.ts:56` — `RenderResponse` interface (item 11 adds `theme_fingerprint?: string`).
- `hub-client/src/types/intelligence.ts` — existing TS↔Rust mirror pattern to follow for `atomicCustomNodes` and `artifactPaths`.

## Test plan

### TDD discipline per work-item

The TDD discipline applies per work-item, not per-plan. Item 10 (link handlers extraction) is a behavior-preserving extraction — gate is "existing tests pass before AND after." Other items are greenfield additions; failing-test-first applies.

### What's observable end-to-end vs. unit-test-only in 2A

2A is mostly inert plumbing. Be honest about which checks verify what:

**End-to-end visible (smoke-able through the running app):**
- Iframe loads `/q2-preview.html` and reaches `IFRAME_READY`.
- Every node renders as muted-gray "T (not yet implemented)" placeholders, visually distinct from q2-debug's bordered boxes.
- `<link data-q2-theme>` element present in the iframe's `document.head` with `href` resolving to a `blob:` URL (after item 11 lands). Visible in DevTools. **The visual *effect* of theme CSS is not observable in 2A** — there is no real-HTML content for the CSS to style; that lands in 2B.
- Editing `theme: cosmo` → `theme: flatly` in YAML changes the `<link>`'s `href` to a new blob URL; the old URL is revoked. Same caveat — no visual change without 2B's real-HTML leaves.

**Unit-test-only (vitest with synthetic Document):**
- Link handler routing on synthetic clicks. The plan's Goal previously claimed "links navigate" — in 2A that is true at the handler level only. `Link` AST nodes render as placeholder text with no `<a>` element, so no link can be clicked end-to-end until 2B's `Link` component ships.
- `__Q2_PREVIEW_RENDERER__` global shape and framework-primitive parity with `__REACT_AST_DEBUG_RENDERER__`.
- `UPDATE_THEME` postMessage handling (parent-side blob-URL minting and revocation; iframe-side `<link href>` management).
- `render-components` gate behavior for q2-preview format.

### Tests

- **Source-info accessor unit tests**: build representative `astJson` strings containing each wire code (0–5), parse them, assert `entryFor` / `isDerived` / `isAtomicSourceInfo` return correct values. Codes 4–5 use hand-constructed JSON until Plan 5 ships writer support.
- **`render-components` gate regression test** (vitest): mount `ReactRenderer` with `format: q2-preview` and a `render-components: [foo.tsx]` AST; assert `customComponentsCode` is populated. Sibling regression test for q2-debug confirms behavior unchanged.
- **`Q2PreviewIframe` boot smoke test** (vitest): mount with a minimal `astJson` and `currentFilePath`, assert the iframe loads `/q2-preview.html` and reaches `IFRAME_READY`.
- **q2-preview dispatcher placeholder**: mount the iframe with an AST containing a `Para` (block) and a `Str` (inline), assert each renders as the muted-gray "(not yet implemented)" placeholder produced by `q2-preview/dispatchers.tsx`. Confirms `Block`/`Inline` are wired and that the registry containing only `'Block'`/`'Inline'`/`'Ast'` produces the expected 2A output. Sibling test asserts the placeholder DOM uses `color: #888` and `font-style: italic` so the aesthetic stays muted.
- **Framework-primitive parity test** (vitest, jsdom env). Locks the contract from §"TSX globals: portability and parity" so future drift fails CI. Assertion: for each framework primitive in `{ renderChildren, renderNode, Node }`, the value placed on `__REACT_AST_DEBUG_RENDERER__` is **reference-equal** (`expect(...).toBe(...)`) to the value placed on `__Q2_PREVIEW_RENDERER__`, and both are reference-equal to the export from the framework module. `Block` and `Inline` are deliberately NOT part of the assertion — those are format-specific (different miss-path styling) and intentionally diverge despite sharing names.

  Sketch:

  ```ts
  // hub-client/src/components/render/parity.test.tsx
  import * as framework from './framework';
  import './q2-debug/entry';   // side effect: sets __REACT_AST_DEBUG_RENDERER__
  import './q2-preview/entry'; // side effect: sets __Q2_PREVIEW_RENDERER__

  const FRAMEWORK_PRIMITIVES = ['renderChildren', 'renderNode', 'Node'] as const;

  describe('framework-primitive parity across iframe globals', () => {
    test.each(FRAMEWORK_PRIMITIVES)('%s is reference-equal across both globals and framework', (name) => {
      const debug   = (window as any).__REACT_AST_DEBUG_RENDERER__[name];
      const preview = (window as any).__Q2_PREVIEW_RENDERER__[name];
      expect(debug).toBe((framework as any)[name]);
      expect(preview).toBe((framework as any)[name]);
      expect(debug).toBe(preview);  // transitive
    });
  });
  ```

  **Implementation prerequisite**: each entry must set its renderer-surface global at module top level (not lazily inside `loadCustomComponents`) so importing the module is sufficient to populate `window`. q2-preview's `entry.tsx` (item 9) does this in its initial form. q2-debug's entry currently sets the global inside `loadCustomComponents` (alongside `window.React` and `window.katex`); refactor it to set the renderer-surface object at module top — leave only the React/katex globals lazy, since those are tied to dynamic user-TSX imports. The refactor is small (~10 LOC moved) and improves testability without changing runtime behavior in production (the global ends up set in both cases by the time user TSX runs). Bundle the refactor with item 9.
- **Link handler unit tests** (vitest): build a representative `Document`, attach `installLinkHandlers(doc, { currentFilePath: '/foo.qmd', onQmdLinkClick })`, dispatch synthetic clicks on:
  - `<a href="https://example.com">` — assert `window.open` called with `_blank`.
  - `<a href="other.qmd#sec">` — assert `onQmdLinkClick({ path: '/other.qmd', anchor: 'sec' })`.
  - `<a href="#sec">` — assert `onQmdLinkClick({ anchor: 'sec' })`.
  - Non-`.qmd` non-anchor href — assert no handler call, default click behavior preserved.
  - Synthetic `Cmd+S` keydown on `doc` — assert parent postMessage `{ type: 'hub-client-save' }` fires.
  
  No stale-closure regression test — closures are captured at attach time and the iframe's lifetime equals the document's mount; `currentFilePath` cannot change underneath a single installation.
- **Theme three-way posting + blob-URL lifecycle test (parent side, vitest)**: mount `<Q2PreviewIframe>` with a mock iframe `contentWindow.postMessage`, mock `vfsReadFile` to return `{ success: true, content: 'A' }`, and spy on `URL.createObjectURL` / `URL.revokeObjectURL`. Drive `IFRAME_READY`, then drive each prop transition and assert (a) the post payload and (b) the URL lifecycle:
  - `themeFingerprint='abc'` → one `createObjectURL` call (with a Blob whose contents match `'A'`); one post `{ cssUrl: <minted URL>, fingerprint: 'abc' }`; no `revokeObjectURL` yet.
  - `themeFingerprint='abc'` again → no post, no new mint, no revoke (dedup).
  - `themeFingerprint='def'` (mock returns `'B'`) → one revoke (of the prior `'abc'` URL); one new mint; one post with the new URL.
  - `themeFingerprint=null` → one revoke (of the `'def'` URL); one post `{ cssUrl: null, fingerprint: null }`; no new mint.
  - `themeFingerprint=undefined` → **no post**, no mint, no revoke. Last URL state is `null` (from prior step).
  - `themeFingerprint='ghi'` (mock returns `'C'`) → one mint, one post; no revoke (no URL was outstanding).
  - Drive a second `IFRAME_READY` (simulating doc-switch remount): assert any outstanding URL is revoked. Then `themeFingerprint='ghi'` → one mint, one post (ref reset, even though `'ghi'` was previously posted on the old iframe instance).
  - Unmount the component → assert the current URL is revoked.
- **Theme injection test (iframe side, vitest)**: mount `<PreviewRoot>` and dispatch synthetic `UPDATE_THEME` `MessageEvent` with `{ cssUrl: 'blob:abc', fingerprint: 'abc' }`. Assert one `<link data-q2-theme rel=stylesheet>` element with `href="blob:abc"` in `document.head`. Dispatch again with same fingerprint — assert no duplication, `href` unchanged. Dispatch with `fingerprint: 'def', cssUrl: 'blob:def'` — assert single `<link>` element, `href` updated to `'blob:def'`. Dispatch with `fingerprint: null, cssUrl: null` — assert `<link>` element is removed from `document.head`. Subsequent dispatch with `fingerprint: 'ghi', cssUrl: 'blob:ghi'` — assert a new `<link>` element appears with the new `href`. Mount/unmount/mount under StrictMode — assert exactly one element after each mount cycle. (No iframe-side test for `undefined` — by contract the parent never posts that value.)
- **`ReactPreview.tsx` three-way extraction test (vitest)**: drive the q2-preview render path with three mock `RenderResponse` shapes — `{ success: false, error: '...' }`, `{ success: true, ast_json: '...', theme_fingerprint: 'abc' }`, and `{ success: true, ast_json: '...' }` (no `theme_fingerprint`). Assert the resulting `themeFingerprint` state transitions: initial `undefined` → fail leaves it `undefined` → succeed-with-theme sets `'abc'` → succeed-without-theme sets `null` → subsequent fail keeps `null` (last-good preserved).
- **Rust-side `themeFingerprint` surfacing test** (cargo nextest): construct a `RenderResponse` for a single render with a known theme; assert `response.theme_fingerprint == Some(theme_fingerprint(css))`. Render twice with the same theme; assert fingerprints byte-identical. Render with a different theme; assert fingerprints differ.
- **HTML iframe link handler regression**: existing `iframePostProcessor.test.ts` and `.integration.test.ts` suites pass without modification.

## Dependencies

### Hard dependencies

- **Plan 2pre** — directory restructure. 2A's items 4, 6, 7, 9, 12 reference paths and renames Plan 2pre establishes.
- **Plan 1** — pipeline, format detection, `RenderResponse.ast_json`, `pipeline_kind` dispatch, theme-CSS / page-scoped-image VFS contracts. All shipped.

### Blocks

- **Plan 2B** — q2-preview registry contents. 2B consumes every artifact 2A ships (Q2PreviewIframe, PreviewContext, registry skeleton, sourceInfoPool plumbing, atomicCustomNodes utility, link handlers, theme CSS injection, render-components gate).
- Independent of Plans 4 / 5 / 6 / 7 / 8 — 2A's source-info wiring is forward-compatible with all of them.

## Risk areas

- **Iframe lifecycle (resolved, not a risk)**. Researched in 2026-05-07: the AST iframe remounts on doc switch. Plan now uses prop-captured closures and doesn't depend on persistence. Listed here for cross-reference; no implementation gotcha remains. See §"Iframe lifecycle, researched" in Design decisions.
- **WASM context boundary (resolved by design).** The iframe is a separate `Window` and does not initialize WASM. Any feature needing VFS reads, source-map lookups, or other WASM-backed services from inside the iframe must use the parent-mints-URL pattern (parent reads bytes, mints blob URL, posts URL string; iframe consumes URLs natively via `<link>` / `<img>`). Theme CSS (items 6 + 9) is the canonical example; Plan 2B's image manifest uses the same pattern. Listed here for cross-reference; not an open risk.
- **Blob URL revocation timing.** Revoking too early (before `<link>` actually fetches) breaks the stylesheet swap; revoking too late or never leaks bytes (worst case: many MB per long editing session). Mitigation: revoke the prior URL only when a new one is minted to replace it (`<link>` swap is synchronous enough that the browser internally retains bytes during the transition), plus revoke-everything on iframe unmount. Lifecycle tested in §"Test plan" — the parent-side test asserts the exact create/revoke pattern.
- **`render-components` gate change visibility**. The current one-line gate is buried in a `useMemo`; easy to miss. Add a comment explaining the gate's semantics now that q2-preview is also covered.
- **Empty-content artifact overwrite (out of scope; tracked at bd-3gtn)**. The WASM flush loop at `wasm-quarto-hub-client/src/lib.rs:1208-1214` and `:1364-1369` writes empty bytes to VFS without checking. `ResourceCollectorTransform` produces empty-content artifacts whose path resolves to the user's upload location. Plan 2B's `Image` reads user uploads as the source of truth, so the bug is parallel to 2A's image story rather than blocking it.
- **Wire-format codes**. See `claude-notes/designs/wire-format-source-info-codes.md`. Codes 0–3 are stable; codes 4–5 are forward-declared in 2A's TS types and inert until Plan 5 writes them.
- **Framework-primitive parity drift**. If a future framework primitive lands on only one global, Pattern A user TSX silently breaks in the unfixed format. Mitigated by the parity test in §"Test plan"; reviewers should treat asymmetric global additions as a bug.

## Estimated scope

| # | Component | Lines (rough) |
|---|---|---|
| 0 | `types/artifactPaths.ts` (TS hand-mirror) | ~15 |
| 1 | `types/sourceInfo.ts` | ~50 |
| 2 | `utils/sourceInfo.ts` (accessors + tests) | ~120 |
| 3 | `utils/atomicCustomNodes.ts` | ~30 |
| 4 | Framework `RegistryContext` extension | ~5 |
| 5 | `q2-preview.html` + `vite.config.ts` rollup input | ~35 |
| 6 | `Q2PreviewIframe.tsx` (incl. theme-CSS posting effect + fingerprint dedup ref) | ~110 |
| 7 | `PreviewContext.tsx` | ~15 |
| 8a | `q2-preview/dispatchers.tsx` (`Block`/`Inline` with muted-gray miss path) | ~30 |
| 8b | `q2-preview/registry.ts` skeleton (`'Ast'`, `'Block'`, `'Inline'` entries) + `PreviewDocument.tsx` | ~40 |
| 9 | `q2-preview/entry.tsx` (PreviewRoot, `UPDATE_THEME` handler + `<link>` injection, link handlers wiring, module-top global set) + q2-debug entry refactor (move global to module top) | ~120 |
| 10 | `utils/iframeLinkHandlers.ts` extraction | ~120 |
| 11 | Rust + parent-side `themeFingerprint` plumbing — 5 constructor sites, TS interface, ReactPreview state, ReactRenderer prop, Rust test | ~50 |
| 12 | `ReactRenderer.tsx` format-dispatch split + smoke-all `PreviewIframeKind` extension (3 files in `e2e/helpers/` + `e2e/smoke-all.spec.ts`) | ~25 |
| 13 | `render-components` gate extension + regression test | ~20 |
| 14 | Framework-primitive parity test (`parity.test.tsx`) | ~30 |
| | **Total** | **~815** |

One focused session is realistic; possibly two. Natural split:
- **Session A**: items 1–4 (shared utilities) + items 5–8 (q2-preview surface scaffolding). Verifies the iframe boots empty with placeholders.
- **Session B**: items 9–13 (entry wrapper, link handlers, themeFingerprint plumbing, format-dispatch split, gate). Verifies the iframe's `<link data-q2-theme>` element resolves a `blob:` URL. Visual styling effect requires 2B.

## Notes

- This plan replaces the original Plan 2A. The original assumed q2-preview would extend q2-debug's component registry; the 2026-05-07 review established the parallel-formats / shared-framework architecture, codified in **Plan 2pre** which lands first.
- Image rendering moved to **Plan 2B** as the first concrete leaf in q2-preview's registry. Image needs full Pandoc semantics (alt-text, attrs, title, kvs) and pairs with Figure block-level handling — both fit naturally in 2B's "fill the registry" scope.
- The "rename `MaybeReadOnlyInline`" question from the original Plan 2 remains resolved in 2B: there's no separate wrapper component; atomic-aware `setLocalAst` gating folds into the framework's `Block` / `Inline` dispatchers (Plan 2B work, in `framework/`).
- Forward-compat dormancy is the explicit pattern for 2A's source-info wiring vs. Plan 5 wire-format codes 4/5.

### Future-self notes (not 2A concerns; flag for future plans)

- **`iframeLinkHandlers.ts` is implicitly shared with the HTML-iframe path.** Item 10 extracts link-handler logic from `iframePostProcessor.ts:213-281` into a reusable utility used by q2-preview today. The HTML iframe's per-element listeners (kept as-is per §"Link handlers" rationale) and q2-preview's delegated listeners share the same `installLinkHandlers` signature. If a future feature needs HTML-iframe and q2-preview link semantics to diverge — e.g. different `Cmd+S` behavior per surface, or different anchor-link routing — the extraction will need to grow a config parameter. Not anticipated in 2A or 2B; flag for 2C / future layout-chrome work.
- **The two iframe globals are now lifecycle-locked to their respective iframes.** `__REACT_AST_DEBUG_RENDERER__` lives only in the q2-debug iframe; `__Q2_PREVIEW_RENDERER__` lives only in the q2-preview iframe. If a future plan ever wants a single global serving both formats (e.g. URL-param dispatch where one HTML page hosts either format and inspects `?format=...`), it will have to undo Plan 2pre's "explicit-object form" decision and rework user TSX expectations. The §"TSX globals: portability and parity" `?? fallback` pattern was the response to the per-iframe lock-in; it's not a substitute for unified globals. Out of scope for any current plan.

### Revision history

- **2026-05-08**: Major revision after pre-implementation review:
  - Filename normalization: `PreviewIframe.tsx`/`DebugIframe.tsx` → `Q2PreviewIframe.tsx`/`Q2DebugIframe.tsx` to match Plan 2pre's renames; `ast-renderer.html` references updated to `q2-debug.html`.
  - Theme CSS architecture changed (option (a)/A2): parent reads VFS bytes and posts via separate message (was: iframe `vfsReadFile` directly, which would not work — iframe lacks WASM). Items 6 and 9 reshape. (Wire shape later changed to blob URL — see 2026-05-09 entry.)
  - **Three-way `themeFingerprint` semantics** (`string | null | undefined`) distinguishes "theme present" / "no theme intended" / "render failed-or-pending"; the third case skips the post so transient errors don't strip styling. Items 6, 9, 11 updated.
  - Item 11 expanded with explicit `ReactPreview.tsx` and `ReactRenderer.tsx` plumbing edits.
  - Item 12 reframed as "split existing combined branch," not "add new branch."
  - bd-3day note dropped from item 9 — fix already back-ported in commit `1e5a930f`; both formats now consume `buildCustomRegistry`.
  - New §"TSX globals: portability and parity" formalizes Pattern A (`?? fallback` for framework-primitive overrides) vs. Pattern B (format-specific helpers) and adopts the framework-primitive parity contract.
  - §"Test plan" reorganized to distinguish end-to-end-observable vs. unit-test-only outcomes, since most of 2A is plumbing not user-visible UX.
  - Drifted line numbers updated throughout (all references re-verified against current `feature/q2-preview-work` HEAD).
- **2026-05-09 (parallel-review follow-ups)**:
  - Confirmed three-way `themeFingerprint` semantics: `undefined` skips post (last-good CSS persists across transient render errors), `null` posts explicit clear (user removed `theme:` from YAML). This is the design already encoded in items 6 + 11; review confirmed the choice rather than changing it.
  - Strengthened the framework-primitive parity test (test plan): now asserts `expect(...).toBe(...)` reference-equality between `__REACT_AST_DEBUG_RENDERER__`, `__Q2_PREVIEW_RENDERER__`, and the framework module's exports for `renderChildren`, `renderNode`, `Node`. `Block`/`Inline` excluded — format-specific by design.
  - **Bundled q2-debug entry refactor with item 9**: move the `__REACT_AST_DEBUG_RENDERER__` assignment from inside `loadCustomComponents` to module top, leaving only `window.React`/`window.katex` lazy. Enables the parity test to read both globals via plain module imports without firing `LOAD_CUSTOM_COMPONENTS` setup messages. Also constrains q2-preview's entry to set its global at module top from the start.
  - Added §"Future-self notes" with two carry-forward items: `iframeLinkHandlers.ts` HTML-iframe sharing (config param needed if semantics diverge later) and iframe-locked globals (cost of any future "single global serves both formats" plan).
- **2026-05-09**: Asset-transport architecture unified under blob URLs (Design B):
  - Theme CSS: `UPDATE_THEME_CSS { css, fingerprint }` (bytes-on-the-wire) → `UPDATE_THEME { cssUrl, fingerprint }` (URL-on-the-wire). Parent mints blob URL via `URL.createObjectURL`; iframe consumes via `<link rel="stylesheet" href={cssUrl}>` instead of `<style>{cssText}</style>`.
  - Items 6 and 9 rewritten for the new wire shape; blob-URL revocation lifecycle (replacement / `IFRAME_READY` / unmount) tracked via parent-side ref.
  - §"Multi-plan contracts" theme + image sections rewritten and merged into a single §"Provided: blob-URL asset contract" — Plan 2A defines the parent-mints-URL / iframe-consumes-URL pattern; Plan 2B applies it to images via an asset manifest. Plan 2B reshapes accordingly (separate revision).
  - Risk areas: WASM context boundary downgraded from active risk to resolved design decision; new explicit risk on blob-URL revocation timing.
  - Test plan: parent-side test extended to assert `URL.createObjectURL` / `URL.revokeObjectURL` lifecycle; iframe-side test asserts `<link href>` swap and removal.
  - Goal section updated to reflect URL-on-the-wire wording.
  - Rationale: aligns with future service-worker design (URL fetches all the way down) and unifies theme + image asset transport under one pattern. Plan 2B's image story becomes a thin manifest-consumer instead of a parallel postMessage-bytes design.

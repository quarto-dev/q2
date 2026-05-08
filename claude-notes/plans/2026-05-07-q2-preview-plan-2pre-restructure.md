# Plan 2pre — Restructure render directory for parallel formats

**Date:** 2026-05-07
**Branch:** feature/q2-preview
**Status:** Implementation plan
**Milestone:** Behavior-preserving carve-up that lets q2-debug and q2-preview coexist as siblings. Foundation for revised Plans 2A and 2B.

## Goal

Carve `hub-client/src/components/render/ReactAstDebugRenderer.tsx` into a format-agnostic `framework/` plus a debug-specific `q2-debug/`. q2-debug renders byte-identically before and after; the existing demos in `~/docs/demo-playground/elliot/` continue to work unchanged.

After 2pre:

- `framework/` contains AST types, `RegistryContext`, the `Ast` component, `Block` / `Inline` / `Node` dispatchers, `renderChildren`, `renderNode` — none of it knows about debug-vs-preview.
- `q2-debug/` contains the bordered-box leaf components, the `AstRenderer` root wrapper, the registry, the iframe wrapper, and the entry.
- `q2-preview/` does **not** exist yet. The revised Plan 2A creates it.
- The `'Ast'` registry key is preserved as-is (no rename). The framework's `Ast` component looks up `registry['Ast']`; q2-debug's registry maps `'Ast': AstRenderer`.
- Slide-side block/inline type names are consolidated onto the framework's `BlockNode`/`InlineNode` (slide-side currently uses bare `Block`/`Inline`).
- Dead `ReactAstRenderer.tsx` is deleted.
- Dead `transpileAndImportTSX` is deleted from `tsxTranspiler.ts` (along with the imports that only existed to support it).
- The defensive `?? componentRegistry` fallback at four dispatcher sites is dropped; `RegistryContext`'s default becomes `{ registry: {} }` and `<Ast>`'s `registry` prop becomes required.

## Why now

Plans 2A and 2B were originally written assuming q2-preview would extend q2-debug's component registry. The 2026-05-07 review established that q2-debug and q2-preview are parallel formats with separate registries: q2-debug's bordered-box components are not useful real-world defaults, and q2-preview's real-HTML components are not useful for AST debugging. The natural split is **framework vs. registry**, not "real defaults vs. debug overrides."

2pre is the structural prerequisite that lets that split exist in code. With it in place, 2A and 2B simplify substantially.

## Scope

### Framework extraction

Extract from `ReactAstDebugRenderer.tsx` into new files under `hub-client/src/components/render/framework/`:

| File | Contents |
|---|---|
| `framework/types.ts` | `PandocAST`, `BlockNode` + all Block variants, `InlineNode` + all Inline variants (**including `MathInline`** — see §"Slide-side type compatibility"), `NodeArgs<T>`, plus the typed format-registry contracts `AstProps` / `AstComponent` / `DispatcherComponent` / `FormatRegistry` (see §"Typed format-registry contract") |
| `framework/RegistryContext.tsx` | `RegistryContext` — promoted from file-private const to exported context. Default value changes from `null` to `{ registry: {} }` so dispatchers can read `useContext(RegistryContext).registry` directly without a fallback (see §"Dispatcher fallback removal"). (Plan 2A adds `sourceInfoPool?` to the value shape.) |
| `framework/Ast.tsx` | `Ast` component (parses `astJson`, sets up Provider, looks up `registry['Ast']`). `registry` prop is **required** (no default — see §"Dispatcher fallback removal"). |
| `framework/dispatch.tsx` | `Node` component; `renderChildrenRegistry`, `renderChildren`; `renderNode`; `blockTypes` array (was inlined in two places — `renderNode` at `ReactAstDebugRenderer.tsx:328` and `Node` at `:582`). Recursive-descent code is co-located here because `renderChildrenRegistry`'s entries reference `<Node>` — splitting them across files would introduce cross-file coupling for no separation gain. `renderNode` calls `useContext(RegistryContext)` and is therefore a hook-equivalent: it must be invoked inside an `<Ast>`-set Provider (user TSX always does, via the registered `'Ast'` component). `renderNode`'s defensive fallback (when neither `registry['Block']`/`['Inline']` nor `registry[node.t]` resolve) becomes plain unstyled text — the styled "Not registered" UI lives in the format's `Block`/`Inline` dispatchers, which always run in normal flow. **`Block` and `Inline` are not framework components** — see §"q2-debug extraction" and §"Framework reserves the registry keys" below. |
| `framework/index.ts` | Re-exports for siblings (everything in `types.ts`, `RegistryContext`, `Ast`, `Node`, `renderChildren`, `renderNode`, `blockTypes`). `renderChildrenRegistry` is **not** re-exported — see §"`renderChildrenRegistry` is framework-internal" below for why this is a deliberate contract, not a casual omission. |

### q2-debug extraction

Extract from `ReactAstDebugRenderer.tsx` into new files under `hub-client/src/components/render/q2-debug/`:

| File | Contents |
|---|---|
| `q2-debug/styles.ts` | `blockStyle`, `inlineStyle` constants |
| `q2-debug/dispatchers.tsx` | `Block`, `Inline` — the bordered dispatchers. Each does `registry[node.t]` lookup; on hit renders the leaf, on miss renders `<div style={blockStyle}><strong>Not registered: {t}</strong></div>` (and inline equivalent). Registered in `q2-debug/registry.ts` under the framework-reserved keys `'Block'` and `'Inline'`. The `?? componentRegistry` fallback drops along with framework's (see §"Dispatcher fallback removal") — the Provider is always set above. Code is byte-equivalent to the current `Block` (`ReactAstDebugRenderer.tsx:454`) and `Inline` (`:533`); only the file location moves. |
| `q2-debug/components.tsx` | All bordered-box leaves (`Para`, `Plain`, `Header`, `CodeBlock`, `BulletList`, `OrderedList`, `BlockQuote`, `Div`, `HorizontalRule`, `RawBlock`, `Figure`, `Str`, `Space`, `SoftBreak`, `LineBreak`, `Emph`, `Strong`, `Code`, `Link`, `Image`, `Span`, `Quoted`); plus `AstRenderer` (the document-root wrapper, name unchanged) |
| `q2-debug/registry.ts` | `componentRegistry` assembly (`BlockComponents`, `InlineComponents`, `Block`, `Inline`, `Ast: AstRenderer`) |

Move and rename:

| From | To | Note |
|---|---|---|
| `hub-client/src/ast-renderer-entry.tsx` (top-level under `src/`) | `hub-client/src/components/render/q2-debug/entry.tsx` (two levels deeper) | Two-level move; all relative imports inside the entry update accordingly. Updated imports for new framework + q2-debug locations. `window.__REACT_AST_DEBUG_RENDERER__` continues to expose all the names user TSX expects. The `<script type="module" src=…>` in `public/ast-renderer.html` updates to the new path (see §"Update consumers"). |
| `hub-client/src/components/render/AstIframe.tsx` | `hub-client/src/components/render/q2-debug/DebugIframe.tsx` | Functionality unchanged. Component renamed `AstIframe` → `DebugIframe`. Iframe URL (the value of `src=`) stays `/ast-renderer.html` — only the served HTML's internal `<script>` source path changes. |

### Typed format-registry contract

Today the registry is `Record<string, (props: any) => React.ReactNode>`. With three reserved keys (`'Block'`, `'Inline'`, `'Ast'`) whose prop shapes are concrete and asymmetric, that's loose enough to let format-side mistakes (typo'd prop names, wrong shape on the `Ast` component) reach runtime. 2pre adds three small types to `framework/types.ts` plus a `FormatRegistry` alias:

```ts
export type AstProps = {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
};
export type AstComponent = (props: AstProps) => React.ReactNode;
export type DispatcherComponent = (args: NodeArgs<BlockNode | InlineNode>) => React.ReactNode;

export type FormatRegistry = Record<string, (props: any) => React.ReactNode> & {
    Ast: AstComponent;
    Block: DispatcherComponent;
    Inline: DispatcherComponent;
};
```

**Where the typing actually catches mistakes.** The narrow types are applied at the *format-side construction site*:

```ts
// q2-debug/registry.ts
export const componentRegistry: FormatRegistry = {
    ...BlockComponents,
    ...InlineComponents,
    Block,                    // checked: must satisfy DispatcherComponent
    Inline,                   // checked: must satisfy DispatcherComponent
    Ast: AstRenderer,         // checked: must satisfy AstComponent
};
```

Plan 2A's `q2-preview/registry.ts` does the same with `previewRegistry: FormatRegistry`. The framework's `<Ast>` keeps its loose `registry: Record<string, ...>` prop type — user TSX overrides loaded via `import(blob)` are babel-stripped of types and can't be checked anyway, so tightening the prop type would force a cast at the entry-side merge for no real gain. The split is honest: format-side defaults are TS-checked, user-supplied overrides are runtime-trusted.

**Cost.** Three exported types, one cast-free annotation per format registry. No change at the entry merge point. No new dependencies on the user-TSX side.

**Benefit.** Format authors get TS errors at registration time if they register a non-conforming component, the framework's `<AstComponent>` invocation can drop the `(props: any)` typing, and the Ast/Block/Inline contract is documented in code rather than living implicitly in entry glue.

### Framework reserves the registry keys; formats provide the components

The architectural contract that emerges from the framework / format split:

- **Framework reserves three registry keys**: `'Block'`, `'Inline'`, `'Ast'`. The framework's `Node` looks up `'Block'` or `'Inline'`; the framework's `<Ast>` looks up `'Ast'`. The framework provides **no implementations** under any of these keys.
- **Each format must register all three.** q2-debug registers its bordered `Block`/`Inline` dispatchers + bordered `AstRenderer`. q2-preview (Plan 2A) registers its own `Block`/`Inline` with a muted-gray "not yet implemented" miss-fallback + `PreviewDocument` for `'Ast'`.
- **The format-specific aesthetic of the "Not registered" miss path** lives in the registered `Block`/`Inline` component — never in framework. Framework code never references format-specific styling constants.

This is what makes q2-debug byte-identical across the migration: today's bordered "Not registered: X" comes from `Block`/`Inline`'s else branch (current `ReactAstDebugRenderer.tsx:459, 538`); tomorrow's bordered "Not registered: X" comes from the same code, just relocated to `q2-debug/dispatchers.tsx`. Framework's `renderNode`'s defensive fallback (when even `registry['Block']` is missing) becomes plain unstyled text, but that branch never fires in normal operation — both shipped formats register `'Block'`/`'Inline'`.

### PandocAST consolidation (extended scope)

Plan 2pre subsumes the original Plan 2A item 2 at full breadth. Beyond extracting `PandocAST` / `BlockNode` / `InlineNode` from `ReactAstDebugRenderer.tsx`, also migrate every other duplicate in the tree to import from `framework/types.ts`:

- `ReactAstSlideRenderer.tsx:11` — its local `PandocAST` definition is dropped; the file imports from `framework/types.ts` instead. Local `Block` / `Inline` types in `ReactAstSlideRenderer.tsx:41,69` **stay** — slide-side scope is intentionally minimal (slides remain on their existing render path; no q2-preview migration).
- `ReactRenderer.tsx:57` — its inline `PandocAST` interface is dropped; the file imports from `framework/types.ts`.
- `RevealjsReactAstSlideRenderer.tsx:13` — `PandocAST` import path updates from `./ReactAstSlideRenderer` to `framework/types.ts`. (Other type/function imports from the slide renderer stay.)
- `hooks/useCursorToSlide.ts:9` — `PandocAST` import path updates.
- `hooks/useSlideThumbnails.tsx:12` — `PandocAST` import path updates.

After 2pre, every `PandocAST` reference in `hub-client/src/` traces to `framework/types.ts`.

#### Slide-side type compatibility

Slide-side's local `PandocAST.blocks: Block[]` (slide-local `Block`) is structurally similar to framework's `BlockNode` but the slide-local `Inline` union **includes `MathInline`** (line 65 of `ReactAstSlideRenderer.tsx`) which the framework's `InlineNode` (extracted from `ReactAstDebugRenderer.tsx`) does not.

Resolution: add `MathInline = { t: 'Math'; c: [{ t: 'DisplayMath' | 'InlineMath' }, string] }` to framework's `InlineNode` union as part of the framework extraction. This is the single Pandoc inline the debug renderer never had. Adding it to the framework type doesn't change q2-debug's behavior — q2-debug's registry doesn't register a `Math` component, so Math nodes still render as "Not registered: Math" exactly as they do today. Plan 2B's q2-preview registry registers a real `Math` component (KaTeX-rendered).

This single type addition resolves the consolidation cleanly. Without it, slide-side code that walks `pandoc.blocks` and pattern-matches on `Inline` variants would lose narrowing for Math.

### `renderChildrenRegistry` is framework-internal

Plan 2B writes to `renderChildrenRegistry` — it adds entries for `'CustomBlock'` and `'CustomInline'` (see Plan 2B §"`framework/dispatch.tsx` — CustomBlock / CustomInline traversal" and the multi-plan-contracts list there). Those mutations happen *inside* `framework/dispatch.tsx` itself; they are framework-evolves-itself changes, not consumer extensions. The structure is never exposed via `framework/index.ts` or the format-side globals (`__REACT_AST_DEBUG_RENDERER__`, `__Q2_PREVIEW_RENDERER__`).

**Two registries with different growth shapes.** Custom-node growth lives in a *different* registry — `customNodeRegistry`, keyed by `type_name`, owned by the format (Plan 2B §"Two registries"). The split is:

| Registry | Keyed by | Lives in | Mutated by | Grows with |
|---|---|---|---|---|
| `renderChildrenRegistry` | Pandoc tag (`Para`, `Header`, …) + the abstract `'CustomBlock'` / `'CustomInline'` categories | `framework/dispatch.tsx` (private) | Framework only | New Pandoc base types (rare); category-level entries (one-time, in 2B) |
| `customNodeRegistry` | `type_name` (`Callout`, `Theorem`, …, user-defined) | `q2-preview/registry.ts` (per-format) | Format + user overrides | Per-custom-node-type growth (open-ended) |

So per-type custom-node extensibility is real, but it's not via `renderChildrenRegistry`. The 2B `'CustomBlock'` / `'CustomInline'` entries are *generic* — they iterate slot contents without per-type knowledge. The type-aware logic lives in the registered component (`Callout`, `Theorem`, …), which reads its named slots and calls `renderSlot(...)` — a per-format utility, not a framework registry. A new custom-node type adds *one* entry in `customNodeRegistry` and *zero* entries in `renderChildrenRegistry`.

This is why 2pre keeps the structure private: there is no future user-facing extension scenario that requires it to be public.

### Figure `renderChildrenRegistry` fix (deliberate non-byte-identical change)

The current `renderChildrenRegistry.Figure` entry (`ReactAstDebugRenderer.tsx:274-296`) has three pre-existing bugs that the framework extraction is the right moment to fix:

**Bug A — `// TODO:` rendered as DOM text.** Line 285 has `// TODO: doesn't totally make sense to have this here:` placed *between* two `{...}` JSX expressions inside a `<>...</>` fragment. In JSX text position, `//` is literal characters, not a comment, so every figure rendered today shows that string in the DOM.

**Bug B — Wrong half of the Caption.** Pandoc's Figure shape is `c = [Attr, Caption, [Block]]` with `Caption = (ShortCaption, [Block])` = `[InlineNode[] | null, BlockNode[]]`. The registry entry renders `c[1][0]` (the ShortCaption — alt-text used for things like list-of-figures) and ignores `c[1][1]` (the visible caption blocks). Elliot's `html.tsx:113` does the opposite — destructures `[, captionBlocks] = c[1]` and renders `c[1][1]` (correct convention). Documents with both fields populated will get *two* captions when html.tsx is loaded — one from the debug entry's "Caption: ..." line, one from html.tsx's `<figcaption>`.

**Bug C — Mixed concerns.** Every other entry in `renderChildrenRegistry` (Para, Plain, Header, Emph, BlockQuote, Div, lists, …) renders only the children list. Figure additionally embeds caption presentation. Any registered Figure component that calls `renderChildren(args)` (the natural pattern, used by `html.tsx:130`) inherits the unwanted "Caption:" wrapper.

**Impact today.** Hits q2-debug on every Figure render. Hits q2-preview *transitively* because q2-preview currently routes through the same `AstIframe` + `componentRegistry` (`ReactRenderer.tsx:148`). After 2A, q2-preview gets its own registry, and after 2B registers a real Figure component (which will call `renderChildren(args)` for body blocks — see Plan 2B §"`q2-preview/blocks/Figure.tsx`"), it inherits Bugs A and C unless the framework entry is fixed.

**Fix.** In `framework/dispatch.tsx`, collapse the Figure entry to the standard pattern:

```ts
Figure: ({ node, setLocalAst, onNavigateToDocument }) =>
    (node as FigureBlock).c[2].map((child, i) => (
        <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
            setLocalAst={(newChild) => {
                const newChildren = [...(node as FigureBlock).c[2]];
                newChildren[i] = newChild as BlockNode;
                setLocalAst({ t: 'Figure', c: [(node as FigureBlock).c[0], (node as FigureBlock).c[1], newChildren] });
            }}
        />
    )),
```

The caption is the registered Figure component's responsibility from now on. q2-debug's `Figure` in `q2-debug/components.tsx` becomes a one-line bordered wrapper that just calls `renderChildren(args)` — no caption. q2-preview's eventual Figure (Plan 2B) reads `c[1][1]` and renders `<figcaption>` as already specified there.

**Behavior change.** q2-debug's bordered Figure will *not* be byte-identical to today's:

- The literal `// TODO: doesn't totally make sense to have this here:` text disappears.
- The "Caption: ..." line for short-captioned figures disappears.

This is a deliberate exception to 2pre's "byte-identical for q2-debug" claim. It is recorded explicitly in §"What stays exactly the same" so the deviation is not a surprise during review.

### Block/Inline naming consolidation

Slide-side `ReactAstSlideRenderer.tsx` declares its block/inline unions as bare `Block` and `Inline`. Debug-side declares them as `BlockNode` and `InlineNode`. Plan 2pre consolidates on the `Node`-suffixed names: slide-side renames `Block → BlockNode` and `Inline → InlineNode` (~25 mechanical refs in `ReactAstSlideRenderer.tsx`), then drops its local declarations and imports the unions from `framework/types.ts`. Debug-side keeps its existing `BlockNode`/`InlineNode` (no churn).

**Why this direction (rather than dropping the `Node` suffix):**

- The user-facing demo `~/docs/demo-playground/elliot/comment.tsx` already uses `BlockNode`/`InlineNode` extensively (`(args: NodeArgs<BlockNode>)`, `comments: InlineNode[]`, `structuredClone(block) as BlockNode`). Renaming in the other direction would break a demo we don't control.
- The dispatcher React components in framework are named `Block` and `Inline`. If the union types were also `Block`/`Inline`, the type and the component would collide in any file that imports both. The `Node` suffix on the unions keeps that namespace clean — and is also why the current `ReactAstDebugRenderer.tsx` uses the suffixed form internally.
- `ReactAstRenderer.tsx` (the dead file deleted in 2pre) also uses local `Block`/`Inline`, but it's deleted anyway; no migration cost.
- `RevealjsReactAstSlideRenderer.tsx`, `useCursorToSlide.ts`, and `useSlideThumbnails.tsx` reference `PandocAST` only — no `Block`/`Inline` symbols — so they need no work beyond the import-path update already noted in §"PandocAST consolidation."

After consolidation, `ReactAstSlideRenderer.tsx` declares no block/inline types of its own and inherits framework's `MathInline` extension to `InlineNode` for free.

### Dispatcher fallback removal

The current renderer has a defensive fallback at four dispatcher sites and one prop default:

- `Block` (`ReactAstDebugRenderer.tsx:454`), `Inline` (`:533`), `Node` (`:579`), `renderNode` (`:323`) all read `const registry = registries?.registry ?? componentRegistry;`.
- `Ast` (`:93`) has the prop default `registry = componentRegistry`.

After the split, three of these sites land in framework (`Node`, `renderNode`, `Ast`'s prop default) and two land in q2-debug (`Block`, `Inline` dispatchers). The fallback drops at all five:

- **Framework sites** *must* drop because `componentRegistry` doesn't exist in framework — it lives in q2-debug, and re-importing it would re-introduce the cross-format coupling 2pre exists to break.
- **q2-debug sites** drop as redundant defensive code: `componentRegistry` is in scope locally, but the `<Ast>` Provider is always set above the dispatchers in real flow, so `useContext(RegistryContext)` returns the registered registry — the `?? componentRegistry` branch never executes.

**Resolution:** drop all five.

- Change `RegistryContext`'s default from `null` to `{ registry: {} }` in `framework/RegistryContext.tsx`. Dispatchers read `useContext(RegistryContext).registry` directly with no `??`.
- Make `Ast`'s `registry` prop **required** (no default). Each format's entry passes its own registry: `q2-debug/entry.tsx` continues to pass `componentRegistry` explicitly (it already does — `ast-renderer-entry.tsx:124`); q2-preview's entry (Plan 2A) will pass its own.

**q2-debug behavior is preserved.** The fallbacks are dead code in the current iframe flow — every dispatch happens inside `<Ast>`'s `<RegistryContext.Provider>`, so `useContext` always returns a non-null value with `mergedRegistry` already in hand. The `?? componentRegistry` branch never executes today. Removing it changes the lines that actually run in q2-debug: zero. The bordered Para, the user's html.tsx Para, "Not registered: SomeWeirdType" — all reach the same code paths after the change as before.

**Rationale for removal (rather than re-pointing the framework-side fallbacks to a framework-local default):**

1. *Coupling.* The whole purpose of 2pre is to give q2-debug and q2-preview parallel, independent registries. A framework-level default registry would either embed q2-debug's components (defeating the split) or embed a third "fallback" registry of synthetic defaults — a third format the codebase has to maintain.
2. *Dead code.* History (below) confirms the fallback has never been load-bearing. Pre-iframe `ReactRenderer.tsx`, post-iframe `ast-renderer-entry.tsx`, and every user TSX in `external-sources/`, `experimental-components/`, and `~/docs/demo-playground/` mount via `<Ast registry={...}>`. No present or historical call site mounts a dispatcher outside an `<Ast>` ancestor. No test imports a dispatcher in isolation.
3. *Diagnostic equivalence.* When a registry truly lacks a node-type entry, each format's `Block`/`Inline` dispatcher renders its own "Not registered: X" path with the format's aesthetic (q2-debug bordered, q2-preview muted gray). With the empty framework default `{ registry: {} }`, that path becomes the *only* path for unregistered types — which is exactly q2-preview's intended Plan-2A behavior (registry containing only `'Block'`/`'Inline'`/`'Ast'` → muted-gray "not yet implemented" until 2B fills in leaves).

**History (for the curious / for anyone who asks why we removed it):**

- `1e901f03` (2026-03-18, "Add `q2-debug` format with comment prototype") — the file's earliest version. Dispatch used hard-coded `BlockRegistry`/`InlineRegistry` constants imported directly. **No context, no fallback, no dispatcher functions.**
- `d6eb0604` (2026-03-20, "Experimental q2-debug custom render components") — introduced `RegistryContext`, `<Ast>`'s `registry` prop default, and the first two fallback sites (`renderNode` and `Inline`) wholesale, alongside the entire pluggable-registry architecture for user TSX overrides.
- `02721668` (2026-04-15, "Add support for slide render component") — added the `Block` dispatcher (promoted to registry-lookup form) and the new unified `Node`, with the same fallback pattern.

The four fallback sites are character-for-character identical (`const registries = useContext(RegistryContext); const registry = registries?.registry ?? componentRegistry;`). No commit message explains the fallback or describes a standalone-mount use case. No PR exists for either commit. The pattern is defensive copy-paste from `<Ast>`'s prop default, propagated to dispatchers as new ones were added — never load-bearing, never explained.

### Renames

- **File**: `hub-client/src/components/render/AstIframe.tsx` → `hub-client/src/components/render/q2-debug/DebugIframe.tsx`.
- **File**: `hub-client/src/ast-renderer-entry.tsx` → `hub-client/src/components/render/q2-debug/entry.tsx`.

(No registry-key renames. The `'Ast'` key stays. See §"What stays exactly the same.")

### Deletion

**`hub-client/src/components/render/ReactAstDebugRenderer.tsx`** — the file at the center of the migration. After the framework + q2-debug splits, every export has been relocated; the file is empty and is deleted at the end of the migration. The deletion is the natural last step of the carve-up, but is called out explicitly so it doesn't get forgotten.

**`hub-client/src/components/render/ReactAstRenderer.tsx`** — dead, no importers (verified by grep). Plan 2A originally bundled this deletion; moves to 2pre as a code-organization concern.

**`transpileAndImportTSX` and its supporting imports in `hub-client/src/services/tsxTranspiler.ts`** — dead since the iframe move (commit `72ef918c`, 2026-05-01).

*History.* Introduced in `d6eb0604` (2026-03-20) alongside the original in-page q2-debug renderer. It was the in-page implementation that transpiled user TSX, injected `React`, `__REACT_AST_DEBUG_RENDERER__`, `Deck`+`Slide`, and `katex` as window globals so the dynamically-imported blob URL could resolve them, and returned the user module's exports. The single caller was `ReactRenderer.tsx`. `72ef918c` moved q2-debug into an iframe, relocated the global injection into `ast-renderer-entry.tsx`, and rewired `ReactRenderer.tsx` to use the new sibling `transpileTSX` (pure transpile, no globals, no dynamic import). The function has had zero callers anywhere in the tree (`src/`, tests, `external-sources/`, `~/docs/demo-playground/`) since 2026-05-01.

*Why delete instead of leaving in place.* Vite tree-shaking cannot drop side-effectful CSS imports or `import * as` bindings consumed by an exported function. The dead function's supporting imports — `import * as ReactAstDebugRendererModule`, `Deck`/`Slide` from `@revealjs/react`, `'reveal.js/reveal.css'`, `'reveal.js/theme/white.css'`, `katex`, `'katex/dist/katex.min.css'` — all live at module top level. Every consumer of `transpileTSX` (today: `ReactRenderer.tsx`) pulls reveal.js + KaTeX + their CSS bundles + the entire renderer module into its bundle, even though `transpileTSX` itself uses none of them. After the 2pre split the situation worsens: keeping the dead function would force `tsxTranspiler.ts` to import `* as` from the new `q2-debug` barrel, coupling the transpiler service to one format — exactly the cross-format coupling 2pre exists to undo. `transpileTSX` is also the eventual transpile entry point for q2-preview's custom components, so the service must stay format-agnostic.

*Commit message for the deletion commit:*

> Remove dead `transpileAndImportTSX` (superseded by the iframe move in 72ef918c). The function and its supporting global-injection imports have had no callers since the iframe refactor; deleting them lets `tsxTranspiler.ts` stop pulling reveal.js + KaTeX + the q2-debug renderer into every consumer's bundle.

After deletion, `tsxTranspiler.ts` keeps only `transpileTSX` and the imports it actually uses (`{ transform } from '@babel/standalone'`).

### Update consumers

- `hub-client/src/components/render/ReactRenderer.tsx`: import `DebugIframe` from `./q2-debug/DebugIframe`; import `PandocAST` from `./framework/types`. Format-dispatch logic unchanged in 2pre — `q2-debug || q2-preview → DebugIframe` (Plan 2A reroutes q2-preview through a new `PreviewIframe`). The existing comment at `ReactRenderer.tsx:141-147` describing q2-preview-via-AstIframe is **left as-is** in 2pre — it becomes inaccurate in wording (`AstIframe` is renamed `DebugIframe`) but the *behavior* it describes is still current. Plan 2A rewrites it when format dispatch actually changes.
- `hub-client/src/services/tsxTranspiler.ts`: delete `transpileAndImportTSX` and its supporting top-level imports (see §"Deletion" for the full list and rationale). After deletion, the file imports only `{ transform } from '@babel/standalone'` and exports only `transpileTSX`. No new import path from the post-restructure framework or q2-debug barrels is needed.
- `hub-client/public/ast-renderer.html`: update `<script type="module" src="/src/ast-renderer-entry.tsx">` to `<script type="module" src="/src/components/render/q2-debug/entry.tsx">`. The HTML file path and the iframe URL (`/ast-renderer.html`) are unchanged; only the inner `<script>` source path changes to track the entry move.
- `hub-client/vite.config.ts`: the `'ast-renderer': path.resolve(__dirname, 'public/ast-renderer.html')` rollup input stays as-is — the HTML location and URL haven't changed.
- `hub-client/src/components/render/ReactAstSlideRenderer.tsx`: rename slide-side `Block → BlockNode`, `Inline → InlineNode` (~25 mechanical refs); drop the local block/inline type declarations; import `BlockNode`/`InlineNode` from `framework/types` (see §"Block/Inline naming consolidation").
- Slide-side hook imports (`hooks/useCursorToSlide.ts`, `hooks/useSlideThumbnails.tsx`) and `RevealjsReactAstSlideRenderer.tsx` get their `PandocAST` import paths updated as listed in §"PandocAST consolidation (extended scope)" above. They reference `PandocAST` only — no `Block`/`Inline` symbol references — so they need no rename work.
- **Test fixtures and snapshots**: confirmed clean. There is no `__snapshots__/` directory under `hub-client/src/components/render/`. `iframePostProcessor.test.ts` and `iframePostProcessor.integration.test.ts` do not import any of the moved files; verify they pass before and after. Vitest config (`hub-client/vitest.config.ts`) does not reference the renderer module directly.
- **`ReactPreview.tsx`, `Preview.tsx`, `PreviewRouter.tsx`** — verified by grep: none import from `ReactAstDebugRenderer`, `AstIframe`, or `ast-renderer-entry`. No changes needed.
- **`hub-client/src/components/render/experimental-components/*.tsx.txt` and `experimental-components/new/*.jsx`** — surveyed. Several reference `BlockNode`, `InlineNode`, `NodeArgs`, `SpanInline`, `ParaBlock`, etc. They are documentation/template files (`.tsx.txt` extension; not built, not imported anywhere) and need no edits. Listed here so future readers see they were surveyed and intentionally skipped.

### Documentation sweep

Several documents currently reference paths that 2pre changes. Update as part of the same PR so doc drift doesn't accumulate:

- **`~/docs/demo-playground/elliot/render_components.qmd`** — references `/hub-client/src/components/render/ReactAstDebugRenderer.tsx` (4 references at lines 7, 15, 33/37, 51) and `/hub-client/src/components/render/ReactRenderer.tsx`. **2pre does the path-only edit, but not blanket:**
  - Lines 7, 15, 33, 37 — references to the renderer-as-implementation. Repoint to `/hub-client/src/components/render/q2-debug/` (the new q2-debug barrel directory).
  - Line 51 — *"Most/all of that plumbing is in `renderChildrenRegistry` in [ReactAstDebugRenderer.tsx]"*. After 2pre, `renderChildrenRegistry` lives in `framework/dispatch.tsx`, **not** `q2-debug/`. Repoint this one specifically to `/hub-client/src/components/render/framework/dispatch.tsx`.
  - `ReactRenderer.tsx`'s path is unchanged; references to it stay.

  The doc's *behavioral* description of the `format !== 'q2-debug'` gating logic is **deliberately left as-is** — 2A reroutes format dispatch and will rewrite that description when format dispatch actually changes. Elliot's doc continues to describe q2-debug only; the q2-preview equivalent is forked to `~/docs/demo-playground/gordon/render-components/render_components.qmd` in Plan 2B, which rewrites the doc for the new format, the new format global, and the built-ins / overrides model.
- **`claude-notes/research/`** and **`claude-notes/designs/`** — light grep for `ReactAstDebugRenderer`, `AstIframe`, `ast-renderer-entry`, `ReactAstRenderer`. Update where matches are found. Most likely candidates: any architecture / overview docs that name files.
- **`claude-notes/plans/2026-05-04-q2-preview-plan-1*.md`** (and any other shipped Plan 1 docs) — if Plan 1 references the iframe rendering path by file name, update for documentation hygiene. Plan 1 is shipped, so this is doc-only.

### What stays exactly the same

- The render pipeline.
- `hub-client/public/ast-renderer.html` — file path and iframe URL (`/ast-renderer.html`) are unchanged. The `<script type="module" src=…>` *inside* the HTML updates to track the entry move (see §"Update consumers").
- The postMessage protocol (`IFRAME_READY` / `UPDATE_AST` / `SET_AST` / `LOAD_CUSTOM_COMPONENTS` / `NAVIGATE_TO_DOCUMENT`).
- The bytes / DOM produced by q2-debug's render path, **with one deliberate exception**: the buggy `renderChildrenRegistry.Figure` entry is fixed. The literal `// TODO:` text and the "Caption:" line for short-captioned figures both disappear from q2-debug's output. See §"Figure `renderChildrenRegistry` fix" for the rationale and the fix.
- `window.__REACT_AST_DEBUG_RENDERER__` global name and the names it exposes.
- The `'Ast'` registry key — no rename. User TSX that exports a component named `Ast` (e.g. `~/docs/demo-playground/elliot/slide.tsx`) continues to override the document root.
- Elliot's demos in `~/docs/demo-playground/elliot/` — including `slide.tsx`'s `export const Ast = …`, which keeps working because the registry key is preserved.

## `__REACT_AST_DEBUG_RENDERER__` continuity

`q2-debug/entry.tsx` sets `window.__REACT_AST_DEBUG_RENDERER__` to a deliberately-shaped object — not a wholesale `import *` spread. This pins the public surface to what user TSX actually consumes and prevents internal contracts (`renderChildrenRegistry`, `RegistryContext`) from leaking onto the global by accident:

```ts
import { renderChildren, renderNode, Node } from '../framework';
import {
  Block, Inline,                                 // q2-debug dispatchers
  Para, Plain, Header, CodeBlock, BulletList,    // q2-debug block leaves
  OrderedList, BlockQuote, Div, HorizontalRule,
  RawBlock, Figure,
  Str, Space, SoftBreak, LineBreak,              // q2-debug inline leaves
  Emph, Strong, Code, Link, Image, Span, Quoted,
  componentRegistry, blockStyle, inlineStyle,
} from '.';

(window as any).__REACT_AST_DEBUG_RENDERER__ = {
  renderChildren, renderNode, Node,
  Block, Inline,
  Para, Plain, Header, CodeBlock, BulletList,
  OrderedList, BlockQuote, Div, HorizontalRule,
  RawBlock, Figure,
  Str, Space, SoftBreak, LineBreak,
  Emph, Strong, Code, Link, Image, Span, Quoted,
  componentRegistry, blockStyle, inlineStyle,
};
```

**Surface researched.** A grep across `~/docs/demo-playground/elliot/` confirms the names actually destructured at runtime today are `renderChildren` (`html.tsx`, `kanban.tsx`, `drag.tsx`), `renderNode` (`slide.tsx`, `html.tsx`), `Block` (`comment.tsx`), and `blockStyle` (`drag.tsx`). All four are in the explicit object literal above (`blockStyle` via the `blockStyle, inlineStyle` line). Type names (`BlockNode`, `InlineNode`, `NodeArgs`, `SpanInline`, `ParaBlock`, …) appear in `comment.tsx` but `babel-preset-typescript` erases them at transpile, so they don't need runtime values. The leaf components (`Para`, `Str`, …) are exposed for users who want to compose with them, matching today's accidental-but-relied-on availability via the wholesale spread.

`renderChildrenRegistry` and `RegistryContext` are deliberately excluded — they are framework internals; if a future demo needs them, that's a deliberate API extension, not a side effect of `import *`.

Plan 2A introduces `__Q2_PREVIEW_RENDERER__` for q2-preview with the same explicit-object pattern. q2-debug's global is preserved indefinitely; the parallel global is additive.

## Out of scope (deferred to 2A or 2B)

- Creating `q2-preview/` (Plan 2A).
- Adding `sourceInfoPool` to `RegistryContext` (Plan 2A).
- The atomic-aware dispatcher gate (Plan 2B; will live in framework's `Node` component inside `framework/dispatch.tsx` — the single recursion chokepoint that runs before each format's `Block`/`Inline` dispatcher).
- Any change to leaf component output (q2-debug stays byte-identical; q2-preview leaves come in 2B).
- Splitting iframe HTML pages (Plan 2A creates `/q2-preview.html`).
- Format dispatch for q2-preview routing through a separate iframe (Plan 2A).
- New tests (the existing suite is the contract).

## Pre-flight

Before starting the framework split proper, run a single ~30-minute check on a throwaway feature branch (e.g. `pre-flight/slide-rename`) to learn the slide-side type-compatibility answer cheaply, before any framework code has moved.

1. In `hub-client/src/components/render/ReactAstSlideRenderer.tsx`, mechanically rename `Block → BlockNode`, `Inline → InlineNode` (~25 refs). Drop the local block/inline type declarations.
2. Add `import type { BlockNode, InlineNode } from './ReactAstDebugRenderer';` (a temporary import path that resolves against the current pre-split file; the real 2pre work will move it to `framework/types.ts`).
3. Run `cd hub-client && npm run build:all`. The TypeScript compile is the actual gate (`tsc -b && vite build` is stricter than `vitest` or `tsc --noEmit`).
4. **Discard the branch regardless of outcome.** This is a learning exercise, not a step toward landed work.

**Interpreting the result:**

- If `npm run build:all` passes, the rename is safe and the framework extraction step in 2pre proper proceeds with confidence.
- If it fails because slide-side's `Inline` union loses the `Math` discriminant (slide-side's local `MathInline` doesn't exist on the imported `InlineNode`), that confirms the §"Slide-side type compatibility" addition is on target — add `MathInline` to the framework union as planned and re-check.
- If it fails because slide-side's `Quoted` discriminant narrows from `{ t: string }` (slide-side line 66) to `{ t: 'SingleQuote' | 'DoubleQuote' }` (framework form) and that surfaces somewhere — the runtime check in `renderInline` is fine (compares to `'SingleQuote'` literally), but if a function-boundary type-check rejects the narrowing, capture the site and either widen at the boundary or accept the narrower type. Same family of safe-narrowing fix as `MathInline`.
- If it fails because slide-side's `Math` discriminant narrows from `{ t: string }` (slide-side line 65) to `{ t: 'DisplayMath' | 'InlineMath' }` (framework form) and that surfaces somewhere — the runtime check in `renderInline` at `ReactAstSlideRenderer.tsx:892` is fine (`mathType.t === 'DisplayMath'` is a literal compare), parallel to the `Quoted` case above. If a function-boundary type-check rejects the narrowing, same fix.
- If it fails for any *other* reason — e.g. structural mismatch at `splitByHeaders` / `extractSections` / `flattenBlocks` boundaries because the unions don't unify cleanly — the failure pinpoints where a cast or union widening is needed. Capture the diagnosis in the plan; resolve as part of 2pre's framework extraction.

The goal of pre-flight is to eliminate the TypeScript-narrowing risks that survive review — everything else in 2pre is mechanical splitting and string-substitution work that `npm run build:all` will catch in real time during implementation.

## Test plan

Behavior preservation is the entire contract. Verify:

1. **`cargo xtask verify --skip-rust-tests` passes** end-to-end. The hub-client TypeScript build is the strictest gate.
2. **`npm run test:ci` (from hub-client/) passes** unchanged.
3. **Browser smoke test (manual)**: open `~/docs/demo-playground/elliot/index.qmd` in hub-client. q2-debug renders identically to pre-2pre — confirmed by:
   - The bordered-box debug aesthetic for un-overridden leaves.
   - `html.tsx` / `simple.tsx` / `comment.tsx` / `kanban.tsx` overrides still load and apply.
   - DevTools console: `window.__REACT_AST_DEBUG_RENDERER__` exposes `renderChildren`, `renderNode`, `componentRegistry`, individual leaf components.

No new tests. Adding tests for the moved code is Plan 2A's / 2B's concern (when the new q2-preview surface is added).

## Risk areas

- **Import-path drift across the codebase.** `npm run build:all` is the canonical safety net; if it passes, the moves are wired correctly.
- **`__REACT_AST_DEBUG_RENDERER__` global completeness.** Anything user TSX reaches for must resolve. The recommended `{ ...framework, ...debug }` spread is trivially complete. Name-collision details are documented in §"`__REACT_AST_DEBUG_RENDERER__` continuity"; the short version is that q2-debug wins on `Block`/`Inline` (which is the current behavior).
- **Slide-side type compatibility after the `Block`/`Inline` → `BlockNode`/`InlineNode` rename.** The rename itself is mechanical, but the import switch from local declarations to framework's union may surface a TypeScript narrowing issue at function boundaries (`splitByHeaders`, `extractSections`, `flattenBlocks`). Both unions terminate in `UnknownBlock`/`UnknownInline` so structural compatibility should hold; if not, the fix is a single cast at the entry point. Mitigated by the §"Pre-flight" check (~30 min) — run that before starting 2pre proper.
- **`tsxTranspiler.ts` deletion.** Removing `transpileAndImportTSX` deletes ~40 lines of code plus several top-level imports (reveal.js, KaTeX, the renderer module). Verify `transpileTSX`'s single caller (`ReactRenderer.tsx:131`) still resolves and that no test, hub-client component, or external-source TSX references the deleted function (grep confirms zero hits).
- **Dispatcher fallback removal.** The `?? componentRegistry` branches at `Block`/`Inline`/`Node`/`renderNode` and the `Ast` prop default are dropped. Behavior is preserved because the Provider is always set inside `<Ast>`. If somewhere in the tree turns out to mount a dispatcher outside `<Ast>` (none found in present or historical code), it would now render the framework's plain "Not registered" fallback (or, if `'Block'`/`'Inline'` are registered but `'X'` isn't, render the format's own miss path) — a diagnostic improvement, not a regression.

- **`Math` byte-fidelity edge case.** The bordered "Not registered: Math" path fires today only when no user TSX is loaded *and* the AST contains a `Math` node. None of the current Elliot demos exercise this combination — `html.tsx` registers `Math`. The architecture preserves correctness regardless (q2-debug's `Inline` dispatcher still wraps misses in `inlineStyle`), but the byte-identical claim isn't strongly testable through the existing demo set. Listed for honesty rather than as a blocker.

- **`__REACT_AST_DEBUG_RENDERER__` surface researched only against `~/docs/demo-playground/elliot/`.** The switch from wholesale `import *` spread to an explicit object is based on a survey of that one demo tree. No other consumers are known, but if other demo trees, internal experiments, or external user TSX exist that destructure names not in the explicit list (`Ast`, `RegistryContext`, types like `BlockNode`, etc.), they'll break with `undefined`. Mitigation: survey any additional consumer trees before merging, or — if the explicit list turns out to be too tight — add the missing names to the entry's object literal. The wholesale spread is a cheap rollback option if a surprise consumer appears.
- **Snapshot tests that encode old paths.** Verified: no `__snapshots__/` directory under `hub-client/src/components/render/`. Risk doesn't materialize.

## Estimated scope

| Step | Lines (rough) |
|---|---|
| Move framework into `framework/*.ts` (`types.ts`, `RegistryContext.tsx`, `Ast.tsx`, `dispatch.tsx`, `index.ts`) | ~220 (mechanical splits) |
| Move q2-debug into `q2-debug/*.ts(x)` (`styles.ts`, `dispatchers.tsx`, `components.tsx`, `registry.ts`, `entry.tsx`, `DebugIframe.tsx`) | ~430 (mechanical splits; +`Block`/`Inline` carved into `dispatchers.tsx`) |
| PandocAST consolidation (slide-side, ReactRenderer, 3 hook files) | ~30 (path/import updates; 1 PandocAST deletion) |
| Slide-side `Block`/`Inline` → `BlockNode`/`InlineNode` rename | ~25 (mechanical refs in `ReactAstSlideRenderer.tsx`) |
| Update `ReactRenderer.tsx` imports | ~3 |
| Update `public/ast-renderer.html` `<script src>` | ~1 |
| Drop dispatcher `?? componentRegistry` fallbacks (4 dispatcher sites + `Ast` prop default + `RegistryContext` default change) | ~6 |
| Add typed format-registry contracts (`AstProps`, `AstComponent`, `DispatcherComponent`, `FormatRegistry` in `framework/types.ts`; one annotation each on `componentRegistry`) | ~12 |
| Fix `renderChildrenRegistry.Figure` (Bugs A/B/C); update q2-debug's `Figure` to a one-line wrapper | ~15 |
| Replace wholesale `__REACT_AST_DEBUG_RENDERER__` spread with explicit object | ~30 |
| Delete `transpileAndImportTSX` and supporting top-level imports in `tsxTranspiler.ts` | -50 |
| Documentation sweep (`render_components.qmd` path updates incl. line-51 fix, claude-notes grep) | ~30 (path-only edits across multiple docs) |
| Delete `ReactAstRenderer.tsx` | -344 |
| Delete `ReactAstDebugRenderer.tsx` (after carve-up; should be empty) | -600 (offsets the moves above; net is structural relocation) |
| **Net behavior change** | **0** |

One focused session. The PR is large in file-count terms but tiny in logical-diff terms — every line of moved code is identical to its pre-move source after import-path adjustment.

The Elliot-demo fork to `~/docs/demo-playground/gordon/render-components/` is **not in 2pre's scope** — it lands alongside Plan 2B when q2-preview's built-in components exist and the "remove anything that is now a built-in" pruning is meaningful. See Plan 2B's notes.

## Dependencies

### Hard dependencies

None. Plan 1 has shipped; nothing else is required.

### Blocks

- **Plan 2A (revised)** — q2-preview surface scaffolding. Cannot land before 2pre because the directory pattern needs to exist.
- **Plan 2B (revised)** — q2-preview registry contents + atomic-aware dispatcher gate. Both depend on the framework/registry separation 2pre establishes.

## Notes

- 2pre runs in the worktree at `.worktrees/q2-preview-work/` on branch `feature/q2-preview`.
- The plan was discussed and confirmed in the 2026-05-07 review session that produced the parallel-formats / shared-framework architecture decision.
- After 2pre, the **revised Plan 2A** stands up the q2-preview surface (entry, iframe, HTML page, format dispatch in `ReactRenderer.tsx`, theme CSS, link handlers, render-components gate, `themeFingerprint`). q2-preview's registry is empty / fallback-only at the end of 2A.
- The **revised Plan 2B** fills q2-preview's registry: real-HTML Pandoc base types (Para, Header, lists, tables, **Image, Figure**, etc.); CustomNode components (Callout, Theorem, …); the atomic-aware dispatcher gate (in framework, benefits both formats); class-name constants module.

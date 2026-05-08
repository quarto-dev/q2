# Plan 2pre — Restructure render directory for parallel formats

**Date:** 2026-05-07
**Branch:** feature/q2-preview-work
**Status:** Implementation plan
**Milestone:** Behavior-preserving carve-up that lets q2-debug and q2-preview coexist as siblings. Foundation for revised Plans 2A and 2B.

## Goal

Carve `hub-client/src/components/render/ReactAstDebugRenderer.tsx` into a format-agnostic `framework/` plus a debug-specific `q2-debug/`. q2-debug renders byte-identically before and after (with one deliberate Figure DOM-text change called out below); the existing demos in `~/docs/demo-playground/elliot/` continue to work unchanged.

After 2pre:

- `framework/` contains AST types, `RegistryContext`, the `Ast` component, `Block` / `Inline` / `Node` dispatchers, `renderChildren`, `renderNode` — none of it knows about debug-vs-preview.
- `q2-debug/` contains the bordered-box leaf components, the `AstRenderer` root wrapper, the registry, the iframe wrapper, and the entry.
- `q2-preview/` does **not** exist yet. The revised Plan 2A creates it.
- The `'Ast'` registry key is preserved as-is (no rename). The framework's `Ast` component looks up `registry['Ast']`; q2-debug's registry maps `'Ast': AstRenderer`.
- Slide-side block/inline type names are consolidated onto the framework's `BlockNode`/`InlineNode` (slide-side currently uses bare `Block`/`Inline`).
- Dead `ReactAstRenderer.tsx` is deleted.
- Dead `transpileAndImportTSX` is deleted from `tsxTranspiler.ts` (along with the imports that only existed to support it).
- The defensive `?? componentRegistry` fallback at four dispatcher sites is dropped; `RegistryContext`'s default becomes `{ registry: {} }` and `<Ast>`'s `registry` prop becomes required.
- Internal hub-client names rename for cohabitation with q2-preview: `componentRegistry` → `q2DebugRegistry`, `AstIframe` → `Q2DebugIframe`, `/ast-renderer.html` → `/q2-debug.html`. The window global `__REACT_AST_DEBUG_RENDERER__` is preserved (public API consumed by user TSX).

## Why now

Plans 2A and 2B were originally written assuming q2-preview would extend q2-debug's component registry. The 2026-05-07 review established that q2-debug and q2-preview are parallel formats with separate registries: q2-debug's bordered-box components are not useful real-world defaults, and q2-preview's real-HTML components are not useful for AST debugging. The natural split is **framework vs. registry**, not "real defaults vs. debug overrides."

2pre is the structural prerequisite that lets that split exist in code. With it in place, 2A and 2B simplify substantially.

## Migration strategy: two-phase via re-export shim

The naive carve-up is one big-bang commit that creates `framework/` + `q2-debug/`, deletes `ReactAstDebugRenderer.tsx`, and rewires every consumer in a single un-revertable step. We avoid that by going through a barrel shim:

- **Phase 1** (additive, no consumer changes). Create `framework/` and `q2-debug/` as new files. Convert `ReactAstDebugRenderer.tsx` into a re-export barrel that exposes the new internals under their **old** names (`componentRegistry`, etc.). Every existing importer keeps compiling. The shim is the only "throwaway" code in the migration.
- **Phase 2** (one consumer at a time). Migrate each consumer to the new locations — slide-side, ReactRenderer, hooks, the iframe component, the entry, the HTML page, vite config — and at the end of Phase 2 delete the shim. Each step is its own commit; each commit leaves the tree green and the binary functional.

Phase 1 is the most code-volume step but it's the safest: the new files are pure copies, the shim re-exports under old names, and `npm run build:all` is the gate. Phase 2 is split into 16 small commits each gated on `npm run build:all` plus a hub-client smoke test.

This plan's checklist below is organized around that ordering.

## Scope

### Framework extraction

Extract from `ReactAstDebugRenderer.tsx` into new files under `hub-client/src/components/render/framework/`:

| File | Contents |
|---|---|
| `framework/types.ts` | `PandocAST`, `BlockNode` + all Block variants, `InlineNode` + all Inline variants (**including `MathInline`** — see §"Slide-side type compatibility"), `NodeArgs<T>`, plus the typed format-registry contracts `AstProps` / `AstComponent` / `DispatcherComponent` / `FormatRegistry` (see §"Typed format-registry contract") |
| `framework/RegistryContext.tsx` | `RegistryContext` — promoted from file-private const to exported context. Default value changes from `null` to `{ registry: {} }` so dispatchers can read `useContext(RegistryContext).registry` directly without a fallback (see §"Dispatcher fallback removal"). (Plan 2A adds `sourceInfoPool?` to the value shape.) |
| `framework/Ast.tsx` | `Ast` component (parses `astJson`, sets up Provider, looks up `registry['Ast']`). `registry` prop is **required** (no default — see §"Dispatcher fallback removal"). |
| `framework/dispatch.tsx` | `Node` component (**promoted from file-private `const Node` to `export function Node`** — currently file-scoped at `ReactAstDebugRenderer.tsx:569`); `renderChildrenRegistry`, `renderChildren`; `renderNode`; `blockTypes` array (was inlined in two places — `renderNode` at `ReactAstDebugRenderer.tsx:328` and `Node` at `:582`). Recursive-descent code is co-located here because `renderChildrenRegistry`'s entries reference `<Node>` — splitting them across files would introduce cross-file coupling for no separation gain. `renderNode` calls `useContext(RegistryContext)` and is therefore a hook-equivalent: it must be invoked inside an `<Ast>`-set Provider (user TSX always does, via the registered `'Ast'` component). `renderNode`'s defensive fallback (when neither `registry['Block']`/`['Inline']` nor `registry[node.t]` resolve) becomes plain unstyled text — the styled "Not registered" UI lives in the format's `Block`/`Inline` dispatchers, which always run in normal flow. **`Block` and `Inline` are not framework components** — see §"q2-debug extraction" and §"Framework reserves the registry keys" below. |
| `framework/index.ts` | Re-exports for siblings (everything in `types.ts`, `RegistryContext`, `Ast`, `Node`, `renderChildren`, `renderNode`, `blockTypes`). `renderChildrenRegistry` is **not** re-exported — see §"`renderChildrenRegistry` is framework-internal" below for why this is a deliberate contract, not a casual omission. |

### q2-debug extraction

Extract from `ReactAstDebugRenderer.tsx` into new files under `hub-client/src/components/render/q2-debug/`:

| File | Contents |
|---|---|
| `q2-debug/styles.ts` | `blockStyle`, `inlineStyle` constants |
| `q2-debug/dispatchers.tsx` | `Block`, `Inline` — the bordered dispatchers. Each does `registry[node.t]` lookup; on hit renders the leaf, on miss renders `<div style={blockStyle}><strong>Not registered: {t}</strong></div>` (and inline equivalent). Registered in `q2-debug/registry.ts` under the framework-reserved keys `'Block'` and `'Inline'`. The `?? componentRegistry` fallback drops along with framework's (see §"Dispatcher fallback removal") — the Provider is always set above. Code is byte-equivalent to the current `Block` (`ReactAstDebugRenderer.tsx:454`) and `Inline` (`:533`); only the file location moves. |
| `q2-debug/components.tsx` | All bordered-box leaves (`Para`, `Plain`, `Header`, `CodeBlock`, `BulletList`, `OrderedList`, `BlockQuote`, `Div`, `HorizontalRule`, `RawBlock`, `Figure`, `Str`, `Space`, `SoftBreak`, `LineBreak`, `Emph`, `Strong`, `Code`, `Link`, `Image`, `Span`, `Quoted`); plus `AstRenderer` (the document-root wrapper, name unchanged). The new `Figure` component preserves today's bordered "Caption: ShortCaption" rendering (see §"Figure entry: bug fixes vs. scope decisions"). |
| `q2-debug/registry.ts` | `q2DebugRegistry` assembly (renamed from `componentRegistry`; typed `FormatRegistry`). Exports: `BlockComponents`, `InlineComponents`, `Block`, `Inline`, `Ast: AstRenderer`. |

Move and rename:

| From | To | Note |
|---|---|---|
| `hub-client/src/ast-renderer-entry.tsx` (top-level under `src/`) | `hub-client/src/components/render/q2-debug/entry.tsx` (two levels deeper) | Two-level move; all relative imports inside the entry update accordingly. Updated imports for new framework + q2-debug locations. `window.__REACT_AST_DEBUG_RENDERER__` continues to expose all the names user TSX expects. The `<script type="module" src=…>` in the new `public/q2-debug.html` points at the new path. While migrating, the `mergedRegistry` is annotated `FormatRegistry` for compile-time validation of the `'Block'`/`'Inline'`/`'Ast'` keys (see §"Typed format-registry contract"). The `customRegistry` accumulator bug (bd-3day) is fixed in passing — single character change `componentRegistry` → `customRegistry` at the spread line. |
| `hub-client/src/components/render/AstIframe.tsx` | `hub-client/src/components/render/q2-debug/Q2DebugIframe.tsx` | Functionality unchanged. Component renamed `AstIframe` → `Q2DebugIframe`. Iframe `src` updates from `/ast-renderer.html` to `/q2-debug.html`. |
| `hub-client/public/ast-renderer.html` | `hub-client/public/q2-debug.html` | The HTML page is renamed for symmetry with `/q2-preview.html` (Plan 2A). The `<script type="module" src=…>` updates to `/src/components/render/q2-debug/entry.tsx`. The vite rollup input in `vite.config.ts:51` updates from `'ast-renderer'` to `'q2-debug'`. |

### Naming consistency for q2-debug ↔ q2-preview cohabitation

2pre cleans up legacy names so q2-debug and q2-preview look like siblings, not "the AST renderer + a new format." The window global stays `__REACT_AST_DEBUG_RENDERER__` because user TSX in `~/docs/demo-playground/` reaches for it by name — that's a public API. Everything else is internal to hub-client and renames freely.

| Concern | Today | After 2pre | Plan 2A adds |
|---|---|---|---|
| HTML route | `/ast-renderer.html` | `/q2-debug.html` | `/q2-preview.html` |
| Iframe component | `AstIframe` | `Q2DebugIframe` | `Q2PreviewIframe` |
| Entry file | `src/ast-renderer-entry.tsx` | `q2-debug/entry.tsx` | `q2-preview/entry.tsx` |
| Format registry | `componentRegistry` | `q2DebugRegistry` | `q2PreviewRegistry` |
| Window global (public API) | `__REACT_AST_DEBUG_RENDERER__` | unchanged | `__Q2_PREVIEW_RENDERER__` |

User TSX consumers across `~/docs/demo-playground/` (surveyed: `elliot/{html,kanban,drag,comment,slide,simple}.tsx` plus `gordon/tldraw-shortcode/html.tsx`) destructure exclusively by name from the window global — `renderChildren`, `renderNode`, `Block`, `blockStyle`. None of them reach for `componentRegistry` or `AstIframe` directly. The rename is transparent to user TSX.

The doc at `~/docs/demo-playground/elliot/render_components.qmd` quotes `componentRegistry` by name in a code block (lines 25-32) and references the file paths of `ReactAstDebugRenderer.tsx` and `ReactRenderer.tsx`. The doc sweep updates both. See §"Documentation sweep."

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

**Where the typing actually catches mistakes.** The narrow types are applied at two sites: the format-side construction site, and the entry-side merge.

```ts
// q2-debug/registry.ts
export const q2DebugRegistry: FormatRegistry = {
    ...BlockComponents,
    ...InlineComponents,
    Block,                    // checked: must satisfy DispatcherComponent
    Inline,                   // checked: must satisfy DispatcherComponent
    Ast: AstRenderer,         // checked: must satisfy AstComponent
};

// q2-debug/entry.tsx — merged registry passed to <Ast>
const mergedRegistry: FormatRegistry = {
    ...q2DebugRegistry,
    ...customRegistry,
} as FormatRegistry;
// User overrides via dynamic import are babel-stripped of types and runtime-trusted;
// the cast asserts the merged result satisfies the contract while letting overrides
// flow through without per-key type assertions.
```

Plan 2A's `q2-preview/registry.ts` and `q2-preview/entry.tsx` do the same. The framework's `<Ast>` keeps its loose `registry: Record<string, ...>` prop type — the entry's annotation is the meaningful gate, and tightening the prop type would force a downstream cast for no real gain.

**Cost.** Three exported types, one annotation per format registry, one annotation per format entry. No new dependencies on the user-TSX side.

**Benefit.** Format authors get TS errors at registration time if they register a non-conforming component or forget one of the reserved keys. The Ast/Block/Inline contract is documented in code rather than living implicitly in entry glue.

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

Resolution: add `MathInline = { t: 'Math'; c: [{ t: 'DisplayMath' | 'InlineMath' }, string] }` to framework's `InlineNode` union as part of the framework extraction. This is the single Pandoc inline the debug renderer never had. Adding it to the framework type doesn't change q2-debug's behavior — q2-debug's registry doesn't register a `Math` component, so Math nodes still render as "Not registered: Math" exactly as they do today (when no user TSX is loaded; with `elliot/html.tsx` loaded the user's Math component takes over). Plan 2B's q2-preview registry registers a real `Math` component (KaTeX-rendered).

The framework form is **narrower** than slide-side's existing types in one further place: the `Quoted` discriminant. Slide-side has `c: [{ t: string }, Inline[]]` (line 66); framework has `c: [{ t: 'SingleQuote' | 'DoubleQuote' }, InlineNode[]]`. The slide-side runtime check at line 892 uses literal compare (`mathType.t === 'DisplayMath'`), so the narrowing is safe at runtime; the pre-flight check below is the gate that confirms TypeScript accepts the narrowing at function boundaries. If a boundary rejects it, the fix is a single cast at the entry point — not a redesign.

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

### Figure entry: bug fixes vs. scope decisions

The current `renderChildrenRegistry.Figure` entry (`ReactAstDebugRenderer.tsx:274-296`) contains three issues. The framework extraction is the right moment to address them, but they split into two true bugs and one scope decision:

**Bug A — `// TODO:` rendered as DOM text** (genuine bug). Line 285 has `// TODO: doesn't totally make sense to have this here:` placed *between* two `{...}` JSX expressions inside a `<>...</>` fragment. In JSX text position, `//` is literal characters, not a comment, so every figure rendered today shows that string in the DOM. The framework's `renderChildrenRegistry.Figure` drops the literal text.

**Bug C — Mixed concerns** (genuine bug). Every other entry in `renderChildrenRegistry` (Para, Plain, Header, Emph, BlockQuote, Div, lists, …) renders only the children list. Figure additionally embeds caption presentation. Any registered Figure component that calls `renderChildren(args)` (the natural pattern, used by `html.tsx:130`) inherits the unwanted caption wrapper. The framework's `renderChildrenRegistry.Figure` collapses to the body-blocks-only standard pattern; caption rendering moves out of the framework entry entirely.

**Scope decision (formerly framed as "Bug B") — which half of the Caption to show**. Pandoc's Figure shape is `c = [Attr, Caption, [Block]]` with `Caption = (ShortCaption, [Block])` = `[InlineNode[] | null, BlockNode[]]`. Today's debug entry renders `c[1][0]` (the ShortCaption — alt-text used for things like list-of-figures). Elliot's `html.tsx:113` does the opposite — destructures `[, captionBlocks] = c[1]` and renders `c[1][1]` as `<figcaption>`. These are both legitimate choices for their respective use cases. q2-debug shows the alt-text because alt-text is the part you'd want to inspect when debugging; q2-preview will show the visible caption blocks because that's what users see in the rendered document.

To preserve q2-debug's current bordered "Caption: ShortCaption" rendering after the framework Figure entry collapses to body-only, q2-debug's `Figure` component takes over the caption rendering locally. Today q2-debug's `Figure` is a one-line `<div style={blockStyle}><strong>Figure:</strong>{renderChildren(args)}</div>` (lines 432-437). After 2pre it grows to call `renderChildren(args)` for body blocks **and** render a bordered caption branch port-for-port from the current registry entry — minus the `// TODO:` text:

```tsx
// q2-debug/components.tsx — Figure
const Figure = (args: NodeArgs<FigureBlock>) => (
    <div style={blockStyle}>
        <strong>Figure:</strong>
        {renderChildren(args)}
        {args.node.c[1][0] && (
            <div><em>Caption:</em> {args.node.c[1][0]!.map((inline, i) => (
                <Node key={i} node={inline} onNavigateToDocument={args.onNavigateToDocument}
                    setLocalAst={(newInline) => {
                        const newCaption = [...args.node.c[1][0]!];
                        newCaption[i] = newInline as InlineNode;
                        args.setLocalAst({ t: 'Figure', c: [args.node.c[0], [newCaption, args.node.c[1][1]], args.node.c[2]] });
                    }}
                />
            ))}</div>
        )}
    </div>
);
```

**Framework `renderChildrenRegistry.Figure` after the fix**:

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

q2-preview's eventual Figure (Plan 2B) reads `c[1][1]` and renders `<figcaption>` as already specified there.

**Net behavior change to q2-debug.** The literal `// TODO: doesn't totally make sense to have this here:` text disappears. The visible bordered "Caption: ..." line for short-captioned figures is preserved. This is the only deliberate exception to 2pre's "byte-identical for q2-debug" claim and is recorded explicitly in §"What stays exactly the same" so the deviation isn't a surprise during review.

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
- **q2-debug sites** drop as redundant defensive code: `q2DebugRegistry` is in scope locally, but the `<Ast>` Provider is always set above the dispatchers in real flow, so `useContext(RegistryContext)` returns the registered registry — the `?? q2DebugRegistry` branch never executes.

**Resolution:** drop all five.

- Change `RegistryContext`'s default from `null` to `{ registry: {} }` in `framework/RegistryContext.tsx`. Dispatchers read `useContext(RegistryContext).registry` directly with no `??`.
- Make `Ast`'s `registry` prop **required** (no default). Each format's entry passes its own registry: `q2-debug/entry.tsx` continues to pass `mergedRegistry` (typed `FormatRegistry`); q2-preview's entry (Plan 2A) will pass its own.

**No consumer breaks.** The fallbacks are dead code in every present and historical call site:

- All user TSX in `~/docs/demo-playground/` (verified across `elliot/{html,kanban,drag,comment,slide,simple}.tsx` and `gordon/tldraw-shortcode/html.tsx`) destructures names off `__REACT_AST_DEBUG_RENDERER__` and uses them inside React components that are themselves registered in the merged registry. By the time those components run, the framework's `<Ast>` has already mounted `<RegistryContext.Provider value={{registry: mergedRegistry}}>` above them, so `useContext(RegistryContext)` returns the merged registry.
- `renderNode` calls `useContext` directly, so it's a hook by React's rules. Calling it outside a React render cycle already fails ("Hooks can only be called inside the body of a function component"). No demo does this; no demo could without already breaking.

After fallback removal, if someone *did* mount a dispatcher outside an `<Ast>` ancestor, today they'd get the prop-default `componentRegistry` and render q2-debug bordered output; tomorrow they'd get an empty registry and render the framework's plain "Not registered" miss path. That's a diagnostic improvement, not a regression — and zero present demos exercise that path.

**Rationale for removal (rather than re-pointing the framework-side fallbacks to a framework-local default):**

1. *Coupling.* The whole purpose of 2pre is to give q2-debug and q2-preview parallel, independent registries. A framework-level default registry would either embed q2-debug's components (defeating the split) or embed a third "fallback" registry of synthetic defaults — a third format the codebase has to maintain.
2. *Dead code.* History (below) confirms the fallback has never been load-bearing. Pre-iframe `ReactRenderer.tsx`, post-iframe `ast-renderer-entry.tsx`, and every user TSX in `external-sources/`, `experimental-components/`, `~/docs/demo-playground/elliot/`, and `~/docs/demo-playground/gordon/` mount via `<Ast registry={...}>`. No present or historical call site mounts a dispatcher outside an `<Ast>` ancestor. No test imports a dispatcher in isolation.
3. *Diagnostic equivalence.* When a registry truly lacks a node-type entry, each format's `Block`/`Inline` dispatcher renders its own "Not registered: X" path with the format's aesthetic (q2-debug bordered, q2-preview muted gray). With the empty framework default `{ registry: {} }`, that path becomes the *only* path for unregistered types — which is exactly q2-preview's intended Plan-2A behavior (registry containing only `'Block'`/`'Inline'`/`'Ast'` → muted-gray "not yet implemented" until 2B fills in leaves).

**History (for the curious / for anyone who asks why we removed it):**

- `1e901f03` (2026-03-18, "Add `q2-debug` format with comment prototype") — the file's earliest version. Dispatch used hard-coded `BlockRegistry`/`InlineRegistry` constants imported directly. **No context, no fallback, no dispatcher functions.**
- `d6eb0604` (2026-03-20, "Experimental q2-debug custom render components") — introduced `RegistryContext`, `<Ast>`'s `registry` prop default, and the first two fallback sites (`renderNode` and `Inline`) wholesale, alongside the entire pluggable-registry architecture for user TSX overrides.
- `02721668` (2026-04-15, "Add support for slide render component") — added the `Block` dispatcher (promoted to registry-lookup form) and the new unified `Node`, with the same fallback pattern.

The four fallback sites are character-for-character identical (`const registries = useContext(RegistryContext); const registry = registries?.registry ?? componentRegistry;`). No commit message explains the fallback or describes a standalone-mount use case. No PR exists for either commit. The pattern is defensive copy-paste from `<Ast>`'s prop default, propagated to dispatchers as new ones were added — never load-bearing, never explained.

### Renames

- **File**: `hub-client/src/components/render/AstIframe.tsx` → `hub-client/src/components/render/q2-debug/Q2DebugIframe.tsx`. Component renamed `AstIframe` → `Q2DebugIframe`. Iframe `src` updates from `/ast-renderer.html` to `/q2-debug.html`.
- **File**: `hub-client/src/ast-renderer-entry.tsx` → `hub-client/src/components/render/q2-debug/entry.tsx`.
- **File**: `hub-client/public/ast-renderer.html` → `hub-client/public/q2-debug.html`.
- **Symbol**: `componentRegistry` → `q2DebugRegistry` (file move + symbol rename — see §"q2-debug extraction"). Legacy alias `componentRegistry` kept by the Phase-1 shim for backward compatibility during the migration; deleted with the shim at the end of Phase 2. User TSX does not destructure this name (verified across `~/docs/demo-playground/`).
- **Vite rollup input**: `'ast-renderer'` → `'q2-debug'` in `hub-client/vite.config.ts:51`. Plan 2A adds a sibling `'q2-preview'` input.

(No registry-key renames. The `'Ast'` key stays. See §"What stays exactly the same.")

### Deletion

**`hub-client/src/components/render/ReactAstDebugRenderer.tsx`** — the file at the center of the migration. After Phase 1 it's a re-export barrel. After Phase 2, every consumer has migrated to the new locations and the shim is deleted. The deletion is the natural last step of Phase 2.

**`hub-client/src/components/render/ReactAstRenderer.tsx`** — dead, no importers (verified by grep). Plan 2A originally bundled this deletion; moves to 2pre as a code-organization concern.

**`hub-client/public/ast-renderer.html`** — replaced by `/q2-debug.html`. After the iframe component (`Q2DebugIframe`) and the entry have migrated, no consumer references `/ast-renderer.html`.

**`hub-client/src/ast-renderer-entry.tsx`** — replaced by `q2-debug/entry.tsx`. After `q2-debug.html` is rewired to point at the new entry, no consumer references the old top-level entry file.

**`transpileAndImportTSX` and its supporting imports in `hub-client/src/services/tsxTranspiler.ts`** — dead since the iframe move (commit `72ef918c`, 2026-05-01).

*History.* Introduced in `d6eb0604` (2026-03-20) alongside the original in-page q2-debug renderer. It was the in-page implementation that transpiled user TSX, injected `React`, `__REACT_AST_DEBUG_RENDERER__`, `Deck`+`Slide`, and `katex` as window globals so the dynamically-imported blob URL could resolve them, and returned the user module's exports. The single caller was `ReactRenderer.tsx`. `72ef918c` moved q2-debug into an iframe, relocated the global injection into `ast-renderer-entry.tsx`, and rewired `ReactRenderer.tsx` to use the new sibling `transpileTSX` (pure transpile, no globals, no dynamic import). The function has had zero callers anywhere in the tree (`src/`, tests, `external-sources/`, `~/docs/demo-playground/`) since 2026-05-01.

*Why delete instead of leaving in place.* Vite tree-shaking cannot drop side-effectful CSS imports or `import * as` bindings consumed by an exported function. The dead function's supporting imports — `import * as ReactAstDebugRendererModule`, `Deck`/`Slide` from `@revealjs/react`, `'reveal.js/reveal.css'`, `'reveal.js/theme/white.css'`, `katex`, `'katex/dist/katex.min.css'` — all live at module top level. Every consumer of `transpileTSX` (today: `ReactRenderer.tsx`) pulls reveal.js + KaTeX + their CSS bundles + the entire renderer module into its bundle, even though `transpileTSX` itself uses none of them. After the 2pre split the situation worsens: keeping the dead function would force `tsxTranspiler.ts` to import `* as` from the new `q2-debug` barrel, coupling the transpiler service to one format — exactly the cross-format coupling 2pre exists to undo. `transpileTSX` is also the eventual transpile entry point for q2-preview's custom components, so the service must stay format-agnostic.

*Commit message for the deletion commit:*

> Remove dead `transpileAndImportTSX` (superseded by the iframe move in 72ef918c). The function and its supporting global-injection imports have had no callers since the iframe refactor; deleting them lets `tsxTranspiler.ts` stop pulling reveal.js + KaTeX + the q2-debug renderer into every consumer's bundle.

After deletion, `tsxTranspiler.ts` keeps only `transpileTSX` and the imports it actually uses (`{ transform } from '@babel/standalone'`).

### `customRegistry` accumulator fix (bd-3day, in passing)

`hub-client/src/ast-renderer-entry.tsx:72` has a long-standing bug tracked as **bd-3day**: `customRegistry = { ...componentRegistry, ...module }` overwrites `customRegistry` with each iteration, so multi-file render-components configurations only keep the last file's exports. The fix is a single-character change: `componentRegistry` → `customRegistry` at the spread line, so subsequent iterations accumulate. Since the entry file is being rewritten as part of 2pre, the fix is rolled into the same commit; the commit message references bd-3day so the issue can be closed.

The accumulator fix is independent of the carve-up and could ship separately, but doing it during the entry rewrite avoids a second touch on the same lines.

### Update consumers

- `hub-client/src/components/render/ReactRenderer.tsx`: import `Q2DebugIframe` from `./q2-debug/Q2DebugIframe`; import `PandocAST` from `./framework/types`. Format-dispatch logic unchanged in 2pre — `q2-debug || q2-preview → Q2DebugIframe` (Plan 2A reroutes q2-preview through `Q2PreviewIframe`). The existing comment at `ReactRenderer.tsx:141-147` describing q2-preview-via-AstIframe is **left as-is** in 2pre — it becomes inaccurate in wording (`AstIframe` is renamed `Q2DebugIframe`) but the *behavior* it describes is still current. Plan 2A rewrites it when format dispatch actually changes.
- `hub-client/src/services/tsxTranspiler.ts`: delete `transpileAndImportTSX` and its supporting top-level imports (see §"Deletion" for the full list and rationale). After deletion, the file imports only `{ transform } from '@babel/standalone'` and exports only `transpileTSX`. No new import path from the post-restructure framework or q2-debug barrels is needed.
- `hub-client/public/q2-debug.html` (new): mirrors the structure of the old `/ast-renderer.html`, but `<script type="module" src="/src/components/render/q2-debug/entry.tsx">` points at the new entry location.
- `hub-client/vite.config.ts`: update rollup input from `'ast-renderer': path.resolve(__dirname, 'public/ast-renderer.html')` to `'q2-debug': path.resolve(__dirname, 'public/q2-debug.html')`.
- `hub-client/src/components/render/ReactAstSlideRenderer.tsx`: rename slide-side `Block → BlockNode`, `Inline → InlineNode` (~25 mechanical refs); drop the local block/inline type declarations; import `BlockNode`/`InlineNode` from `framework/types` (see §"Block/Inline naming consolidation").
- Slide-side hook imports (`hooks/useCursorToSlide.ts`, `hooks/useSlideThumbnails.tsx`) and `RevealjsReactAstSlideRenderer.tsx` get their `PandocAST` import paths updated as listed in §"PandocAST consolidation (extended scope)" above. They reference `PandocAST` only — no `Block`/`Inline` symbol references — so they need no rename work.
- **Test fixtures and snapshots**: confirmed clean. There is no `__snapshots__/` directory under `hub-client/src/components/render/`. `iframePostProcessor.test.ts` and `iframePostProcessor.integration.test.ts` do not import any of the moved files; verify they pass before and after. Vitest config (`hub-client/vitest.config.ts`) does not reference the renderer module directly.
- **`ReactPreview.tsx`, `Preview.tsx`, `PreviewRouter.tsx`** — verified by grep: none import from `ReactAstDebugRenderer`, `AstIframe`, or `ast-renderer-entry`. No changes needed.
- **`hub-client/src/components/render/experimental-components/*.tsx.txt` and `experimental-components/new/*.jsx`** — surveyed. Several reference `BlockNode`, `InlineNode`, `NodeArgs`, `SpanInline`, `ParaBlock`, etc. They are documentation/template files (`.tsx.txt` extension; not built, not imported anywhere) and need no edits. Listed here so future readers see they were surveyed and intentionally skipped.

### Documentation sweep

Several documents currently reference paths that 2pre changes. Update as part of the same PR so doc drift doesn't accumulate:

- **`~/docs/demo-playground/elliot/render_components.qmd`** — references `/hub-client/src/components/render/ReactAstDebugRenderer.tsx` (4 references at lines 7, 15, 33/37, 51) and `/hub-client/src/components/render/ReactRenderer.tsx`, plus the `componentRegistry` symbol name in the code-block snippet (lines 25-32). **2pre does the path-and-symbol edit, but not blanket:**
  - Lines 7, 15, 33, 37 — references to the renderer-as-implementation. Repoint to `/hub-client/src/components/render/q2-debug/` (the new q2-debug barrel directory).
  - Line 51 — *"Most/all of that plumbing is in `renderChildrenRegistry` in [ReactAstDebugRenderer.tsx]"*. After 2pre, `renderChildrenRegistry` lives in `framework/dispatch.tsx`, **not** `q2-debug/`. Repoint this one specifically to `/hub-client/src/components/render/framework/dispatch.tsx`.
  - Lines 25-32 (code snippet) — rename `componentRegistry` → `q2DebugRegistry` to match the source.
  - `ReactRenderer.tsx`'s path is unchanged; references to it stay.

  The doc's *behavioral* description of the `format !== 'q2-debug'` gating logic is **deliberately left as-is** — 2A reroutes format dispatch and will rewrite that description when format dispatch actually changes. Elliot's doc continues to describe q2-debug only; the q2-preview equivalent is forked to `~/docs/demo-playground/gordon/render-components/render_components.qmd` in Plan 2B, which rewrites the doc for the new format, the new format global, and the built-ins / overrides model.
- **`claude-notes/research/`** and **`claude-notes/designs/`** — light grep for `ReactAstDebugRenderer`, `AstIframe`, `ast-renderer-entry`, `ReactAstRenderer`, `componentRegistry`. Update where matches are found. Most likely candidates: any architecture / overview docs that name files.
- **`claude-notes/plans/2026-05-04-q2-preview-plan-1*.md`** (and any other shipped Plan 1 docs) — if Plan 1 references the iframe rendering path by file name, update for documentation hygiene. Plan 1 is shipped, so this is doc-only.

### What stays exactly the same

- The render pipeline.
- The iframe `<src>` URL attribute and the served HTML's structure — only the route name and the `<script>` source path inside the HTML change. Plan 1's iframe protocol is untouched.
- The postMessage protocol (`IFRAME_READY` / `UPDATE_AST` / `SET_AST` / `LOAD_CUSTOM_COMPONENTS` / `NAVIGATE_TO_DOCUMENT`).
- The bytes / DOM produced by q2-debug's render path, **with one deliberate exception**: the literal `// TODO:` text in q2-debug's bordered Figure output disappears (Bug A). The "Caption: ShortCaption" line is preserved by porting it to q2-debug's `Figure` component (see §"Figure entry"). All other DOM is identical.
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
  q2DebugRegistry, blockStyle, inlineStyle,
} from '.';

(window as any).__REACT_AST_DEBUG_RENDERER__ = {
  renderChildren, renderNode, Node,
  Block, Inline,
  Para, Plain, Header, CodeBlock, BulletList,
  OrderedList, BlockQuote, Div, HorizontalRule,
  RawBlock, Figure,
  Str, Space, SoftBreak, LineBreak,
  Emph, Strong, Code, Link, Image, Span, Quoted,
  q2DebugRegistry, blockStyle, inlineStyle,
};
```

**Surface researched.** A grep across `~/docs/demo-playground/elliot/` and `~/docs/demo-playground/gordon/tldraw-shortcode/` confirms the names actually destructured at runtime today are `renderChildren` (`elliot/{html,kanban,drag}.tsx`, `gordon/tldraw-shortcode/html.tsx`), `renderNode` (`elliot/{slide,html}.tsx`, `gordon/tldraw-shortcode/html.tsx`), `Block` (`elliot/comment.tsx`), and `blockStyle` (`elliot/drag.tsx`). All four are in the explicit object literal above. Type names (`BlockNode`, `InlineNode`, `NodeArgs`, `SpanInline`, `ParaBlock`, …) appear in `comment.tsx` but `babel-preset-typescript` erases them at transpile, so they don't need runtime values. The leaf components (`Para`, `Str`, …) are exposed for users who want to compose with them, matching today's accidental-but-relied-on availability via the wholesale spread. The registry is exposed under its new name `q2DebugRegistry`; no user TSX destructures it (verified).

`renderChildrenRegistry` and `RegistryContext` are deliberately excluded — they are framework internals; if a future demo needs them, that's a deliberate API extension, not a side effect of `import *`.

Plan 2A introduces `__Q2_PREVIEW_RENDERER__` for q2-preview with the same explicit-object pattern. q2-debug's global is preserved indefinitely; the parallel global is additive.

## Out of scope (deferred to 2A or 2B)

- Creating `q2-preview/` (Plan 2A).
- Adding `sourceInfoPool` to `RegistryContext` (Plan 2A).
- The atomic-aware dispatcher gate (Plan 2B; will live in framework's `Node` component inside `framework/dispatch.tsx` — the single recursion chokepoint that runs before each format's `Block`/`Inline` dispatcher).
- Any change to leaf component output (q2-debug stays byte-identical; q2-preview leaves come in 2B).
- Splitting iframe HTML pages by format (Plan 2A creates `/q2-preview.html`).
- Format dispatch for q2-preview routing through a separate iframe (Plan 2A).
- New tests (the existing suite is the contract).
- Migrating the `~/docs/demo-playground/gordon/tldraw-shortcode/` demo. It uses `__REACT_AST_DEBUG_RENDERER__` exclusively, so the rename does not affect it. Out of 2pre's scope.

## Phase 0: Pre-flight (throwaway branch)

Before starting Phase 1, run a single ~30-minute check on a throwaway branch (e.g. `pre-flight/slide-rename`) to learn the slide-side type-compatibility answer cheaply, before any framework code has moved.

1. In `hub-client/src/components/render/ReactAstSlideRenderer.tsx`, mechanically rename `Block → BlockNode`, `Inline → InlineNode` (~25 refs). Drop the local block/inline type declarations.
2. Add `import type { BlockNode, InlineNode } from './ReactAstDebugRenderer';` (a temporary import path that resolves against the current pre-split file; the real Phase 1 work will move it to `framework/types.ts`).
3. Run `cd hub-client && npm run build:all`. The TypeScript compile is the actual gate (`tsc -b && vite build` is stricter than `vitest` or `tsc --noEmit`).
4. **Discard the branch regardless of outcome.** This is a learning exercise, not a step toward landed work.

**Interpreting the result:**

- If `npm run build:all` passes, the rename is safe and Phase 1 proceeds with confidence.
- If it fails because slide-side's `Inline` union loses the `Math` discriminant (slide-side's local `MathInline` doesn't exist on the imported `InlineNode`), that confirms the §"Slide-side type compatibility" addition is on target — add `MathInline` to the framework union as planned and re-check.
- If it fails because slide-side's `Quoted` discriminant narrows from `{ t: string }` to `{ t: 'SingleQuote' | 'DoubleQuote' }` (framework form) and that surfaces somewhere — the runtime check in `renderInline` is fine (compares to `'SingleQuote'` literally), but if a function-boundary type-check rejects the narrowing, capture the site and either widen at the boundary or accept the narrower type. Same family of safe-narrowing fix as `MathInline`.
- If it fails because slide-side's `Math` discriminant narrows from `{ t: string }` to `{ t: 'DisplayMath' | 'InlineMath' }` (framework form) and that surfaces somewhere — the runtime check in `renderInline` at `ReactAstSlideRenderer.tsx:892` is fine (`mathType.t === 'DisplayMath'` is a literal compare), parallel to the `Quoted` case above. If a function-boundary type-check rejects the narrowing, same fix.
- If it fails for any *other* reason — e.g. structural mismatch at `splitByHeaders` / `extractSections` / `flattenBlocks` boundaries because the unions don't unify cleanly — the failure pinpoints where a cast or union widening is needed. Capture the diagnosis in the plan; resolve as part of Phase 1.

The goal of pre-flight is to eliminate the TypeScript-narrowing risks that survive review — everything else is mechanical splitting and string-substitution work that `npm run build:all` will catch in real time during implementation.

## Implementation checklist

Each item is one commit. Each commit must leave `npm run build:all` green from inside `hub-client/`. Items that touch q2-debug runtime behavior must additionally be smoke-tested in a hub-client browser session against `~/docs/demo-playground/elliot/index.qmd`.

### Phase 0 — Pre-flight

- [ ] **0.1** Run the slide-side rename on a throwaway branch per §"Phase 0: Pre-flight." Discard the branch.

### Phase 1 — Build new directory structure behind a re-export shim

Goal at the end of Phase 1: `framework/` and `q2-debug/` exist with all final code; `ReactAstDebugRenderer.tsx` is a thin barrel re-exporting them under old names; every existing importer in the tree still compiles unchanged. q2-debug renders identically (modulo the Bug A `// TODO:` text disappearance).

- [ ] **1.1** Create `framework/types.ts`. Move `PandocAST`, all Block variant types and `BlockNode` union, all Inline variant types and `InlineNode` union (**including the new `MathInline` variant**), `NodeArgs<T>`. Add `AstProps`, `AstComponent`, `DispatcherComponent`, `FormatRegistry` (§"Typed format-registry contract").
- [ ] **1.2** Create `framework/RegistryContext.tsx`. Default value `{ registry: {} }` (no `null`).
- [ ] **1.3** Create `framework/dispatch.tsx`. Move `renderChildrenRegistry` (with the **Bug A + Bug C fix on the Figure entry** — collapse to body-only, no `// TODO:` text), `renderChildren`, `renderNode`, `blockTypes`, and `Node` (**promoted from `const Node` to `export function Node`**). Drop the four `?? componentRegistry` fallbacks from `renderNode` and `Node`.
- [ ] **1.4** Create `framework/Ast.tsx`. `registry` prop required (no default). Drop the `?? componentRegistry` prop default.
- [ ] **1.5** Create `framework/index.ts`. Re-export everything in `types.ts`, plus `RegistryContext`, `Ast`, `Node`, `renderChildren`, `renderNode`, `blockTypes`. Do **not** re-export `renderChildrenRegistry`.
- [ ] **1.6** Create `q2-debug/styles.ts`. Move `blockStyle`, `inlineStyle`.
- [ ] **1.7** Create `q2-debug/dispatchers.tsx`. Move `Block` and `Inline` dispatchers. Drop the `?? componentRegistry` fallback at both sites.
- [ ] **1.8** Create `q2-debug/components.tsx`. Move all bordered-box leaves (`Para`, `Plain`, …, `Quoted`) and `AstRenderer`. **Update `Figure`** to render body via `renderChildren(args)` *plus* the bordered "Caption: ShortCaption" branch port-for-port from the current registry entry, minus the `// TODO:` text (§"Figure entry").
- [ ] **1.9** Create `q2-debug/registry.ts`. Define `q2DebugRegistry: FormatRegistry` (renamed from `componentRegistry`) assembled from `BlockComponents`, `InlineComponents`, `Block`, `Inline`, `Ast: AstRenderer`. Re-export `BlockComponents`, `InlineComponents`, `Block`, `Inline`.
- [ ] **1.10** Convert `ReactAstDebugRenderer.tsx` into a re-export barrel. Re-export from `framework` and `q2-debug` under **old names** so existing importers keep compiling: `export { Ast, renderChildren, renderNode } from './framework';` `export type { PandocAST, BlockNode, InlineNode, NodeArgs, FigureBlock, … } from './framework';` `export { Block, blockStyle, inlineStyle } from './q2-debug';` `export { q2DebugRegistry as componentRegistry } from './q2-debug';`. The shim is the only Phase-1-throwaway file.
- [ ] **1.11** **Verification.** `cd hub-client && npm run build:all` passes. Browser smoke test of q2-debug against `~/docs/demo-playground/elliot/index.qmd`: bordered debug aesthetic intact, all elliot overrides load, `__REACT_AST_DEBUG_RENDERER__` exposes the expected names in DevTools. Single Bug-A behavior change (the literal `// TODO:` text) confirmed gone from the rendered Figure DOM.

### Phase 2 — Migrate consumers, rename for cohabitation, delete the shim

Each step is one commit, each gated by `npm run build:all` plus (where indicated) a browser smoke test. The shim from step 1.10 keeps every consumer compileable until step 2.14 deletes it.

- [ ] **2.1** PandocAST import-path consolidation. In `ReactRenderer.tsx`, `useCursorToSlide.ts`, `useSlideThumbnails.tsx`, `RevealjsReactAstSlideRenderer.tsx`, `ReactAstSlideRenderer.tsx`: drop any local `PandocAST` declaration; import `PandocAST` from `./framework/types` (or relative equivalent). One commit. Build.
- [ ] **2.2** Slide-side `Block`/`Inline` → `BlockNode`/`InlineNode` rename in `ReactAstSlideRenderer.tsx`. ~25 mechanical refs. Drop slide-side local block/inline type declarations; import the unions from `./framework/types`. Slide-side now inherits framework's `MathInline` extension. Build. (Pre-flight in 0.1 should have de-risked this.)
- [ ] **2.3** Create `public/q2-debug.html` mirroring `public/ast-renderer.html` but with `<script type="module" src="/src/ast-renderer-entry.tsx">` (still pointing at the OLD entry path; entry hasn't moved yet). Add `'q2-debug': path.resolve(__dirname, 'public/q2-debug.html')` to `vite.config.ts` rollup inputs *alongside* the existing `'ast-renderer'` entry. Now both routes are served and load the same entry. Build. Smoke test that `/q2-debug.html` loads identically to `/ast-renderer.html`.
- [ ] **2.4** Create `q2-debug/Q2DebugIframe.tsx` (verbatim port of `AstIframe.tsx`, renamed component, `src` updated to `/q2-debug.html`). Build.
- [ ] **2.5** Update `ReactRenderer.tsx` to import `Q2DebugIframe` from `./q2-debug/Q2DebugIframe` and use it in place of `AstIframe`. q2-debug now runs through `/q2-debug.html`. Build. **Smoke test** all elliot demos.
- [ ] **2.6** Delete `hub-client/src/components/render/AstIframe.tsx`. Verify no remaining importers via grep. Build.
- [ ] **2.7** Create `q2-debug/entry.tsx` as the new entry. Use the explicit `__REACT_AST_DEBUG_RENDERER__` object literal from §"`__REACT_AST_DEBUG_RENDERER__` continuity." Annotate `mergedRegistry: FormatRegistry` (with cast at the spread, since `customRegistry` is babel-transpiled user code). **Fix bd-3day in the same commit**: change `customRegistry = { ...componentRegistry, ...module }` to `customRegistry = { ...customRegistry, ...module }` so subsequent iterations accumulate. Commit message references bd-3day. Build. (No runtime change yet — `q2-debug.html` still points at the old entry.)
- [ ] **2.8** Update `public/q2-debug.html` to point its `<script type="module" src=…>` at `/src/components/render/q2-debug/entry.tsx`. Remove the `'ast-renderer'` rollup input from `vite.config.ts`; only `'q2-debug'` remains. Build. **Smoke test** all elliot demos including a multi-component override (`render-components: [simple.tsx, html.tsx, comment.tsx]` if available, otherwise add a temporary fixture) to confirm bd-3day's accumulator fix landed.
- [ ] **2.9** Delete `hub-client/src/ast-renderer-entry.tsx` (replaced by `q2-debug/entry.tsx`; nothing references it after 2.8). Build.
- [ ] **2.10** Delete `hub-client/public/ast-renderer.html` (no consumers). Build.
- [ ] **2.11** Delete `transpileAndImportTSX` and supporting top-level imports in `hub-client/src/services/tsxTranspiler.ts` (see §"Deletion" for the full list). After deletion the file imports only `{ transform } from '@babel/standalone'`. Verify `transpileTSX`'s single caller (`ReactRenderer.tsx:131`) still resolves. Build.
- [ ] **2.12** Delete `hub-client/src/components/render/ReactAstRenderer.tsx` (already dead; verified by grep). Build.
- [ ] **2.13** Documentation sweep: update `~/docs/demo-playground/elliot/render_components.qmd` paths and the `componentRegistry` → `q2DebugRegistry` symbol mention; grep `claude-notes/research/` and `claude-notes/designs/` for the old file/symbol names and update; update `claude-notes/plans/2026-05-04-q2-preview-plan-1*.md` if any path references slipped in. Build (no code changes; doc commit only).
- [ ] **2.14** Delete the Phase-1 shim `hub-client/src/components/render/ReactAstDebugRenderer.tsx`. Verify no remaining importers via grep. Build. **Smoke test** all elliot demos one last time.
- [ ] **2.15** Final full verification: `cargo xtask verify --skip-rust-tests` end-to-end. Browser smoke test against `~/docs/demo-playground/elliot/index.qmd` and `slides.qmd`: q2-debug bordered aesthetic intact, all elliot demo overrides apply, `__REACT_AST_DEBUG_RENDERER__` exposes the expected names in DevTools. Update `hub-client/changelog.md` per the project's hub-client commit instructions (one entry per landed commit hash, but a final summary entry can also be added for the carve-up).

After 2.15, `framework/` + `q2-debug/` are the only renderer code paths; `ReactAstDebugRenderer.tsx`, `ReactAstRenderer.tsx`, `AstIframe.tsx`, `ast-renderer-entry.tsx`, `public/ast-renderer.html`, and `transpileAndImportTSX` are gone; q2-debug is byte-equivalent (with the Bug-A `// TODO:` text deletion); the codebase is ready for Plan 2A to scaffold q2-preview as a sibling.

## Test plan

Behavior preservation is the entire contract. Verify at every checklist step:

1. **`npm run build:all` passes** from `hub-client/`. The hub-client TypeScript build is the strictest gate.
2. **`npm run test:ci` passes** unchanged.
3. **Browser smoke test (manual)** at the steps marked **Smoke test** in the checklist: open `~/docs/demo-playground/elliot/index.qmd` in hub-client. q2-debug renders identically to pre-2pre (modulo Bug A's `// TODO:` text disappearance) — confirmed by:
   - The bordered-box debug aesthetic for un-overridden leaves.
   - `html.tsx` / `simple.tsx` / `comment.tsx` / `kanban.tsx` overrides still load and apply.
   - DevTools console: `window.__REACT_AST_DEBUG_RENDERER__` exposes `renderChildren`, `renderNode`, `Node`, `Block`, `Inline`, `q2DebugRegistry`, individual leaf components, and `blockStyle`/`inlineStyle`.
   - Multi-component override (after 2.8) accumulates contributions from all listed files (bd-3day regression).

No new tests. Adding tests for the moved code is Plan 2A's / 2B's concern (when the new q2-preview surface is added).

## Risk areas

- **Import-path drift across the codebase.** `npm run build:all` is the canonical safety net at every checklist step; if it passes, the moves are wired correctly. Phase 1's shim guarantees that even when not-yet-migrated consumers still reference old names, they keep compiling.
- **`__REACT_AST_DEBUG_RENDERER__` global completeness.** Anything user TSX reaches for must resolve. Survey covers `~/docs/demo-playground/elliot/` and `~/docs/demo-playground/gordon/tldraw-shortcode/`; the explicit object in §"`__REACT_AST_DEBUG_RENDERER__` continuity" includes every name destructured at runtime in those trees. If a surprise consumer appears with a name not in the explicit object literal, they'll get `undefined`. Mitigation: the wholesale spread is a cheap rollback option. Risk is low: the surveyed set is comprehensive.
- **Slide-side type compatibility after the `Block`/`Inline` → `BlockNode`/`InlineNode` rename (step 2.2).** The rename is mechanical, but the import switch from local declarations to framework's union may surface a TypeScript narrowing issue at function boundaries (`splitByHeaders`, `extractSections`, `flattenBlocks`). Both unions terminate in `UnknownBlock`/`UnknownInline` so structural compatibility should hold; if not, the fix is a single cast at the entry point. Mitigated by Phase 0 pre-flight (~30 min) before Phase 1 starts.
- **`tsxTranspiler.ts` deletion (step 2.11).** Removing `transpileAndImportTSX` deletes ~40 lines of code plus several top-level imports (reveal.js, KaTeX, the renderer module). Verify `transpileTSX`'s single caller (`ReactRenderer.tsx:131`) still resolves and that no test, hub-client component, or external-source TSX references the deleted function (grep confirms zero hits today).
- **Dispatcher fallback removal.** Behavior is preserved because the Provider is always set inside `<Ast>`. See §"Dispatcher fallback removal" — the conclusion is that no consumer breaks; if a future caller mounts a dispatcher outside `<Ast>`, today's silent q2-debug fallback becomes tomorrow's framework "Not registered" path, which is a diagnostic improvement.
- **Math edge case** (no longer a worry — stating the conclusion). When no user TSX is loaded *and* the AST contains a `Math` node, q2-debug's `Inline` dispatcher matches no registry entry for `'Math'` and falls into its else branch — `<span style={inlineStyle}><strong>Not registered: Math</strong></span>`. That is identical before and after 2pre (same code, different file). With user TSX loaded (`elliot/html.tsx` registers `Math`), the user's component takes over and the fallback path is unreached.
- **Snapshot tests that encode old paths.** Verified: no `__snapshots__/` directory under `hub-client/src/components/render/`. Risk doesn't materialize.

## Estimated scope

| Step | Lines (rough) |
|---|---|
| Phase 1 — framework files (`types.ts`, `RegistryContext.tsx`, `dispatch.tsx`, `Ast.tsx`, `index.ts`) | ~220 (mechanical splits) |
| Phase 1 — q2-debug files (`styles.ts`, `dispatchers.tsx`, `components.tsx`, `registry.ts`) | ~410 (mechanical splits + Figure caption-branch port) |
| Phase 1 — barrel shim in `ReactAstDebugRenderer.tsx` | ~30 (re-exports under old names; thrown away in 2.14) |
| Phase 2 — PandocAST consolidation imports (5 files) | ~15 (path/import updates) |
| Phase 2 — Slide-side `Block`/`Inline` → `BlockNode`/`InlineNode` rename | ~25 (mechanical refs in `ReactAstSlideRenderer.tsx`) |
| Phase 2 — Create `Q2DebugIframe.tsx`, `q2-debug.html`, `q2-debug/entry.tsx` (with bd-3day fix) | ~250 (port + rewrite) |
| Phase 2 — Update `ReactRenderer.tsx` imports | ~3 |
| Phase 2 — Update `vite.config.ts` rollup input | ~1 |
| Phase 2 — Drop dispatcher `?? componentRegistry` fallbacks (already done in Phase 1's framework + q2-debug new files) | (zero — covered above) |
| Phase 2 — Add typed format-registry contracts (already in Phase 1's `types.ts`) | (zero — covered above) |
| Phase 2 — Replace wholesale `__REACT_AST_DEBUG_RENDERER__` spread with explicit object (in q2-debug/entry.tsx) | ~30 |
| Phase 2 — Delete `transpileAndImportTSX` and supporting imports | -50 |
| Phase 2 — Documentation sweep | ~30 (path-only edits) |
| Phase 2 — Delete `ReactAstRenderer.tsx` | -344 |
| Phase 2 — Delete `ast-renderer-entry.tsx` (replaced by `q2-debug/entry.tsx`) | -140 |
| Phase 2 — Delete `public/ast-renderer.html` (replaced by `q2-debug.html`) | -34 |
| Phase 2 — Delete `AstIframe.tsx` (replaced by `Q2DebugIframe.tsx`) | -86 |
| Phase 2 — Delete `ReactAstDebugRenderer.tsx` (the Phase-1 shim) | -30 |
| **Net behavior change** | **0** (modulo Bug A `// TODO:` text deletion) |

The PR series is large in file-count and commit-count terms but tiny in logical-diff terms — every line of moved code is identical to its pre-move source after import-path adjustment, plus the Figure caption-branch port (preserves visible behavior) and the bd-3day single-character fix.

The Elliot-demo fork to `~/docs/demo-playground/gordon/render-components/` is **not in 2pre's scope** — it lands alongside Plan 2B when q2-preview's built-in components exist and the "remove anything that is now a built-in" pruning is meaningful. See Plan 2B's notes.

## Dependencies

### Hard dependencies

None. Plan 1 has shipped; nothing else is required.

### Blocks

- **Plan 2A (revised)** — q2-preview surface scaffolding. Cannot land before 2pre because the directory pattern needs to exist.
- **Plan 2B (revised)** — q2-preview registry contents + atomic-aware dispatcher gate. Both depend on the framework/registry separation 2pre establishes.

### Out-of-scope tracked work consumed in passing

- **bd-3day** — `customRegistry` accumulator bug. Fixed in step 2.7's single-character change. Commit message references bd-3day so the issue can be closed.

## Notes

- 2pre runs in the worktree at `.worktrees/q2-preview-work/` on branch `feature/q2-preview-work`.
- The plan was discussed and confirmed in the 2026-05-07 review session that produced the parallel-formats / shared-framework architecture decision. The 2026-05-07 follow-up review tightened the migration into the two-phase shim ordering, the `q2DebugRegistry` / `Q2DebugIframe` / `/q2-debug.html` naming for cohabitation with q2-preview, the `mergedRegistry: FormatRegistry` annotation, the Bug B reframing as a scope decision (preserve q2-debug's "Caption: ShortCaption" via a port to q2-debug's `Figure` component), and folding bd-3day into step 2.7.
- After 2pre, the **revised Plan 2A** stands up the q2-preview surface (entry, iframe, HTML page, format dispatch in `ReactRenderer.tsx`, theme CSS, link handlers, render-components gate, `themeFingerprint`). q2-preview's registry is empty / fallback-only at the end of 2A.
- The **revised Plan 2B** fills q2-preview's registry: real-HTML Pandoc base types (Para, Header, lists, tables, **Image, Figure**, etc.); CustomNode components (Callout, Theorem, …); the atomic-aware dispatcher gate (in framework, benefits both formats); class-name constants module.

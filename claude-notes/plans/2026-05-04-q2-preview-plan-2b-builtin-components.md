# Plan 2B — q2-preview built-in component registry (revised post-2pre)

**Date:** 2026-05-04 (revised 2026-05-07)
**Branch:** feature/q2-preview
**Status:** Implementation plan
**Milestone:** M2. q2-preview reaches visual parity with the HTML format for documents that use Pandoc base types, callouts, theorems, proofs, figures, equations, images, and cross-references.

## Goal

Fill q2-preview's empty registry (created by Plan 2A) with real-HTML leaf components, plus the framework-level plumbing they depend on:

- **q2-preview's built-in registry** — every Pandoc base type rendered as real HTML (Para → `<p>`, Header → `<h1>`-`<h6>`, BulletList → `<ul>`, **Image → `<img>`**, **Figure → `<figure>` + `<figcaption>`**, etc.). Includes Pandoc gap fills (LineBlock, DefinitionList, Table family, Underline, Strikeout, Superscript, Subscript, SmallCaps, Cite, RawInline, Note).
- **Type-specific CustomNode components** — Callout, Theorem, Proof, FloatRefTarget, Equation, CrossrefResolvedRef, IncludeExpansion. Class-compatible with Rust's HTML output so Quarto's compiled theme CSS produces matching visuals.
- **Framework: atomic-aware gate** — framework's `Node` component (in `framework/dispatch.tsx`) gains a gate that no-ops `setLocalAst` for atomic content (Derived source_info, atomic Synthetic kinds, atomic CustomNode types). Located at the single recursion chokepoint, before each format's `Block`/`Inline` dispatcher receives `args`. Benefits both q2-debug and q2-preview automatically — neither format's dispatcher needs modification.
- **Framework: unwrap / rewrap walks** — `framework/customNode.ts` translates between wire-format wrapper Divs/Spans and JS-native `CustomBlockNode` / `CustomInlineNode` shapes. Both formats can consume.
- **Class-name constants module** — pinned class taxonomy mirroring Rust's HTML output, so loading Quarto's CSS produces matching visuals.

q2-preview's leaf components ship as part of the **built-in registry**, not as drafts pasted into demos. The render-components override mechanism (Plan 2A item 13) still works for users who want to override q2-preview leaves; the built-ins are simply the default registry.

Elliot's existing `~/docs/demo-playground/elliot/html.tsx` is the seed for q2-preview's built-in real-HTML leaves. 2B's work is to fill its base-type gaps, port it from `__REACT_AST_DEBUG_RENDERER__` to `__Q2_PREVIEW_RENDERER__`, ship it as `q2-preview/blocks/` and `q2-preview/inlines/` rather than as a single user file, and add the CustomNode components alongside.

## Scope

### In scope

#### Framework changes (apply to both formats)

##### `framework/customNode.ts` — unwrap / rewrap walks

Pure functions, no React, no context:

```ts
export function unwrapCustomNodes(ast: PandocAST): PandocAST;
export function rewrapCustomNodes(ast: PandocAST): PandocAST;
```

**Forward path** (wire → render):
1. `framework/Ast.tsx` parses `astJson`.
2. Calls `unwrapCustomNodes(parsed)` — single tree walk that replaces wrapper Divs / Spans (identified by the `__quarto_custom_node` class) with `CustomBlockNode` / `CustomInlineNode` shapes.
3. Walk reads `type_name` from `data-custom-type` (canonical discovery mechanism per Plan 2A §"Provided: atomicCustomNodes hand-mirror"), reads slot metadata from `data-custom-slots`, reads `plain_data` from `data-custom-data`. Strips wrapper class and `data-custom-*` kvs from `attr`; strips the `Plain` wrapper from Inline / Inlines slots; recurses into slot contents (for nested CustomNodes — Plan 8 case).
4. After unwrap, the AST contains zero `__quarto_custom_node` references. The registry's `Div` / `Span` entries only see real Divs / Spans.
5. The framework's `Node` dispatcher gets one new entry in its `blockTypes` array: `'CustomBlock'`.

**q2-debug input assumption.** The unwrap walk runs unconditionally inside framework's `Ast` — both formats see it. q2-debug today renders the **raw, pre-pipeline AST**, which never contains `__quarto_custom_node` wrappers (CustomNodes are produced by transforms in q2-preview's pipeline, not q2-debug's). Under that assumption, unwrap is a no-op for q2-debug and the unconditional placement is safe. **If that assumption ever changes** — e.g. q2-debug is ever pointed at post-pipeline AST — q2-debug's bordered-Div rendering would become bordered "Not registered: CustomBlock" instead, since q2-debug doesn't register CustomBlock/CustomInline. The fix at that point is to gate the unwrap call on format (move it from framework's `Ast` into each format's `'Ast'` registry component, where the format opts in). Documented here so the assumption is recoverable from the plan rather than buried in code.

**Reverse path** (setLocalAst → setAst → postMessage):
1. Components freely propagate JS-native shapes through `setLocalAst`.
2. Reassembly walks back up to the root `setAst` (already wired in entry.tsx).
3. Before postMessage, `rewrapCustomNodes(ast)` walks once and rewrites JS-native CustomNodes back to wire-format Div / Span; re-adds `__quarto_custom_node` class and `data-custom-{type,slots,data}` kvs; re-wraps Inline / Inlines slots in `Plain`.
4. Wire-format AST goes to parent.

The handler change in q2-preview's entry (and q2-debug's, if/when it grows interactive editing):

```ts
setAst={(newAst) => {
  window.parent.postMessage({
    type: 'SET_AST',
    ast: rewrapCustomNodes(newAst),
  }, '*');
}}
```

##### `framework/types.ts` — concrete CustomNode shapes

The placeholder `CustomBlockNode` / `CustomInlineNode` discriminants in `framework/types.ts` (added by Plan 2pre) get filled in:

```ts
import type { Attr, BlockNode, InlineNode } from './types';

export type Slot =
  | { kind: 'block';   value: BlockNode }
  | { kind: 'inline';  value: InlineNode }
  | { kind: 'blocks';  value: BlockNode[] }
  | { kind: 'inlines'; value: InlineNode[] };

interface CustomNodeBase {
  type_name: string;
  slots: Record<string, Slot>;
  plain_data: unknown;
  attr: Attr;
  s?: number;
}

export interface CustomBlockNode extends CustomNodeBase { t: 'CustomBlock' }
export interface CustomInlineNode extends CustomNodeBase { t: 'CustomInline' }
export type CustomNode = CustomBlockNode | CustomInlineNode;
```

The `'CustomBlock'` / `'CustomInline'` discriminator (not a single `'Custom'` + `variant` field) is chosen because (a) the framework's `Node` dispatcher uses a hardcoded `blockTypes` array, and adding two distinct `t` values fits with a one-line addition; (b) block-vs-inline becomes a static type property; (c) round-trip is unambiguous.

##### `framework/dispatch.tsx` — atomic-aware gate inside `Node`

Plan 2pre's refined architecture moves `Block`/`Inline` dispatchers out of framework into format-owned files (`q2-debug/dispatchers.tsx`, `q2-preview/dispatchers.tsx`). Putting the atomic gate in either format's dispatcher would either duplicate the code or only protect one format. The cleaner home is framework's `Node` component (in `framework/dispatch.tsx`) — the single recursion chokepoint that runs *before* either format's dispatcher receives `args`.

`Node`'s body gains the gate:

```tsx
const NOOP = () => {};

const Node = ({ node, setLocalAst, onNavigateToDocument }: NodeProps) => {
  const ctx = useContext(RegistryContext);
  const pool = ctx.sourceInfoPool;

  const isAtomic = isAtomicSourceInfo(node, pool, ATOMIC_SYNTHETIC_KINDS)
                || ((node.t === 'CustomBlock' || node.t === 'CustomInline')
                    && isAtomicCustomNode(node.type_name));

  const effectiveSetLocalAst = isAtomic ? NOOP : setLocalAst;

  const isBlock = blockTypes.includes(node.t);
  const Dispatcher = ctx.registry[isBlock ? 'Block' : 'Inline'];
  if (!Dispatcher) {
    // Programmer error: format shipped a registry without 'Block'/'Inline'.
    // Both shipped formats register them; this branch never fires in normal flow.
    return <>{`Dispatcher not registered: ${isBlock ? 'Block' : 'Inline'}`}</>;
  }
  return <Dispatcher node={node} setLocalAst={effectiveSetLocalAst} onNavigateToDocument={onNavigateToDocument} />;
};
```

The format's `Block` / `Inline` dispatcher receives already-gated `args` and continues with its own `registry[node.t]` lookup unchanged — q2-debug's bordered-box leaves and q2-preview's real-HTML leaves both see a no-op `setLocalAst` for atomic content without any per-format awareness.

Three atomic detection paths converge into one gate:

1. **Derived source_info** (Plan 6's shortcode resolutions) — via `isAtomicSourceInfo`'s `isDerived` arm.
2. **Atomic Synthetic source_info** (Plan 4's `By::is_atomic_synthesizer()`) — via `ATOMIC_SYNTHETIC_KINDS`.
3. **Atomic CustomNode types** (`CrossrefResolvedRef` today; `IncludeExpansion` post-Plan-8) — via `isAtomicCustomNode`.

The gate is correctness-level: atomic content's source AST and rendered output diverge (e.g. `@fig-1` source vs. "Figure 1" rendered), so editing into rendered atomic content would corrupt the source. Both formats benefit automatically; q2-debug picks it up "for free" if it ever grows editing affordances, without modifying its dispatcher.

##### `framework/dispatch.tsx` — CustomBlock / CustomInline traversal

Add entries to `renderChildrenRegistry` (which lives in `framework/dispatch.tsx` after Plan 2pre's collapse, framework-internal) for `CustomBlock` and `CustomInline` so child traversal works for slot contents. The gate above ensures atomic children don't get a usable `setLocalAst`.

These two new entries are *generic* — they iterate slots without per-type knowledge. The type-aware logic lives in the registered component (`Callout`, `Theorem`, …), which reads its named slots and calls `renderSlot(...)` from `q2-preview/utils.ts`. **`renderChildrenRegistry` does not grow per custom-node type.** A new custom-node type adds *one* entry in `customNodeRegistry` (per-format, keyed by `type_name`) and *zero* entries in `renderChildrenRegistry`. The framework table has Pandoc-base-type entries plus exactly two abstract-category entries (`'CustomBlock'`, `'CustomInline'`) — no further entries are anticipated. See 2pre §"`renderChildrenRegistry` is framework-internal" for the contract this preserves.

Also extend `blockTypes` to include `'CustomBlock'`. `Node`'s isBlock test already routes via this array, so the addition is a one-line change.

#### q2-preview leaf components

##### `q2-preview/blocks/`

Real-HTML implementations of every Pandoc Block variant. Seeded from Elliot's `html.tsx`; gap fills from cross-referencing `crates/quarto-pandoc-types/src/block.rs`:

| File | Pandoc node | Renders as |
|---|---|---|
| `q2-preview/blocks/Para.tsx` | `Para` | `<p>` |
| `q2-preview/blocks/Plain.tsx` | `Plain` | Fragment (no wrapper) |
| `q2-preview/blocks/Header.tsx` | `Header` | `<h1>`-`<h6>` with id, classes, data-* attrs |
| `q2-preview/blocks/CodeBlock.tsx` | `CodeBlock` | `<pre><code>` with id, classes, attrs |
| `q2-preview/blocks/BulletList.tsx` | `BulletList` | `<ul>` |
| `q2-preview/blocks/OrderedList.tsx` | `OrderedList` | `<ol start={N}>` |
| `q2-preview/blocks/BlockQuote.tsx` | `BlockQuote` | `<blockquote>` |
| `q2-preview/blocks/Div.tsx` | `Div` | `<div>` with id, classes, data-* attrs |
| `q2-preview/blocks/HorizontalRule.tsx` | `HorizontalRule` | `<hr>` |
| `q2-preview/blocks/RawBlock.tsx` | `RawBlock` | If `format === 'html'`, `dangerouslySetInnerHTML`; else `<pre>` |
| `q2-preview/blocks/Figure.tsx` | `Figure` | `<figure>` + body blocks + `<figcaption>` |
| `q2-preview/blocks/LineBlock.tsx` | `LineBlock` (gap) | `<div class="line-block">` with each line as a `<div>` of inlines |
| `q2-preview/blocks/DefinitionList.tsx` | `DefinitionList` (gap) | `<dl><dt>...</dt><dd>...</dd></dl>` |
| `q2-preview/blocks/Table.tsx` | `Table` (gap) | `<table>` + `<caption>` + `<thead>` + `<tbody>` + `<tfoot>` |

##### `q2-preview/inlines/`

Real-HTML implementations of every Pandoc Inline variant:

| File | Pandoc node | Renders as |
|---|---|---|
| `q2-preview/inlines/Str.tsx` | `Str` | text node |
| `q2-preview/inlines/Space.tsx` | `Space` | `' '` |
| `q2-preview/inlines/SoftBreak.tsx` | `SoftBreak` | `'\n'` |
| `q2-preview/inlines/LineBreak.tsx` | `LineBreak` | `<br>` |
| `q2-preview/inlines/Emph.tsx` | `Emph` | `<em>` |
| `q2-preview/inlines/Strong.tsx` | `Strong` | `<strong>` |
| `q2-preview/inlines/Code.tsx` | `Code` | `<code>` with id, classes, attrs |
| `q2-preview/inlines/Link.tsx` | `Link` | `<a href title>` |
| **`q2-preview/inlines/Image.tsx`** | `Image` | `<img>` with full Pandoc semantics (see below) |
| `q2-preview/inlines/Span.tsx` | `Span` | `<span>` with id, classes, attrs |
| `q2-preview/inlines/Quoted.tsx` | `Quoted` | `'…'` or `"…"` characters around children |
| `q2-preview/inlines/Math.tsx` | `Math` | KaTeX-rendered `<span>` (DisplayMath / InlineMath) |
| `q2-preview/inlines/Underline.tsx` | `Underline` (gap) | `<u>` |
| `q2-preview/inlines/Strikeout.tsx` | `Strikeout` (gap) | `<s>` |
| `q2-preview/inlines/Superscript.tsx` | `Superscript` (gap) | `<sup>` |
| `q2-preview/inlines/Subscript.tsx` | `Subscript` (gap) | `<sub>` |
| `q2-preview/inlines/SmallCaps.tsx` | `SmallCaps` (gap) | `<span style="font-variant: small-caps">` |
| `q2-preview/inlines/RawInline.tsx` | `RawInline` (gap) | If `format === 'html'`, `dangerouslySetInnerHTML`; else `<code>` |
| `q2-preview/inlines/Cite.tsx` | `Cite` (gap) | Visible inlines (second-position content); citations array provides metadata |
| `q2-preview/inlines/Note.tsx` | `Note` (gap) | `<sup>[fn]</sup>` marker with hover tooltip; full footnote pane is layout chrome |

##### `q2-preview/inlines/Image.tsx` — full Pandoc semantics

```tsx
import { useContext } from 'react';
import { PreviewContext } from '../PreviewContext';
import { resolveImageSrc, inlinesToPlainText } from '../utils';
import type { ImageInline } from '../../framework/types';

export function Image({ node }: { node: ImageInline }) {
  const [[id, classes, kvs], altInlines, [url, title]] = node.c;
  const { currentFilePath } = useContext(PreviewContext) ?? { currentFilePath: '' };

  const src = resolveImageSrc(url, currentFilePath);
  const alt = inlinesToPlainText(altInlines);
  const kvMap = Object.fromEntries(kvs);

  return (
    <img
      src={src}
      alt={alt}
      {...(title ? { title } : {})}
      {...(id ? { id } : {})}
      {...(classes.length ? { className: classes.join(' ') } : {})}
      {...(kvMap.width ? { width: kvMap.width } : {})}
      {...(kvMap.height ? { height: kvMap.height } : {})}
    />
  );
}
```

`resolveImageSrc` (in `q2-preview/utils.ts`) handles VFS lookup + `data:` URI for project-relative paths; passes through `http`, `https`, `data:`, `//` URLs unchanged. `inlinesToPlainText` recursively walks inlines (`Str`, `Space`, `Code`, `SoftBreak`, `LineBreak`, etc.) into a plain string for the `alt` attribute.

External URLs and paths that fail VFS resolution pass through unchanged. The legacy `/.quarto/...` branch from `iframePostProcessor.ts:177-210` is **not** ported — q2-preview's body AST never carries `/.quarto/...` image paths (per Plan 2A §"Multi-plan contract: page-scoped image artifacts").

##### `q2-preview/blocks/Figure.tsx` — `<figure>` + `<figcaption>`

```tsx
export function Figure({ node }: { node: FigureBlock }) {
  const [[id, classes, _kvs], [_short, captionBlocks], bodyBlocks] = node.c;
  return (
    <figure
      {...(id ? { id } : {})}
      {...(classes.length ? { className: classes.join(' ') } : {})}
    >
      {bodyBlocks.map((b, i) => <Block key={i} node={b} />)}
      {captionBlocks.length > 0 && (
        <figcaption>
          {captionBlocks.map((b, i) => <Block key={i} node={b} />)}
        </figcaption>
      )}
    </figure>
  );
}
```

Crossref-numbered captions (`Figure 1: …`) are already baked into the caption blocks by `CrossrefResolveTransform` (in q2-preview's pipeline at `pipeline.rs:881, :977`); q2-preview gets that for free.

This component renders body blocks via `<Block />` and reads `c[1][1]` directly for the caption — it does **not** call `renderChildren(args)` for the figure as a whole. That avoids any interaction with `renderChildrenRegistry.Figure`, which was rewritten in 2pre to render only `c[2]` (the main body) and to drop the buggy short-caption / `// TODO:` interleaving. Either pattern is correct after 2pre; this component happens to slot blocks individually for the caption hairsplit.

##### `q2-preview/custom/` — type-specific CustomNode components

- **`Callout.tsx`** — header (icon + title), body. Class-compatible with Rust's callout HTML (`callout`, `callout-{type}`, `callout-header`, `callout-title-container`, `callout-body`).
- **`Theorem.tsx`** — labeled block with title from `formatRefLabel(kind, number, title?)`. Class-compatible with `theorem`, `theorem-title`.
- **`Proof.tsx`** — labeled block. Class-compatible with `proof`, `proof-title`.
- **`FloatRefTarget.tsx`** — figure-like wrapper for `#fig-foo` / `#tbl-foo` content with caption.
- **`Equation.tsx`** — Math content with optional `\tag{N}` (appended by `CrossrefIndex`); slot-renders the inner Math through `q2-preview/inlines/Math.tsx`.
- **`CrossrefResolvedRef.tsx`** — inline CustomNode (Span wrapper). Renders the resolved reference text. Atomic — slot children do not receive `setLocalAst` (the framework gate handles this; the component itself just renders).
- **`IncludeExpansion.tsx`** — dormant placeholder until Plan 8 produces these. Atomic per `atomicCustomNodes.ts`. v1 renders as a transparent passthrough (renders the slot blocks); registration in place from the start so when Plan 8's wrapper starts appearing in the AST, it renders correctly.
- **Generic fallback** — `customNodeRegistry['__fallback__']` for unknown `type_name` values. Styled box with `data-custom-type` displayed and slot contents nested. Useful for extension-defined CustomNodes.

#### `q2-preview/registry.ts` assembly

```ts
import * as Blocks from './blocks';
import * as Inlines from './inlines';
import * as Custom from './custom';
import { Block, Inline } from './dispatchers';  // q2-preview's own; created in Plan 2A
import { PreviewDocument } from './PreviewDocument';

export const previewRegistry: Record<string, ComponentType<any>> = {
  ...Blocks,
  ...Inlines,
  Block,
  Inline,
  Ast: PreviewDocument,  // q2-preview's root wrapper, registered under the 'Ast' key (no debug styling)
  // CustomBlock and CustomInline dispatch to customNodeRegistry by type_name.
  // These entries are q2-preview-specific — q2-debug has no customNodeRegistry
  // and would render an unhandled CustomBlock/CustomInline as the q2-debug
  // dispatcher's bordered "Not registered" fallback if it ever encountered one.
  CustomBlock: ({ node, ...args }) => {
    const Comp = customNodeRegistry[node.type_name] ?? customNodeRegistry['__fallback__'];
    return <Comp node={node} {...args} />;
  },
  CustomInline: ({ node, ...args }) => {
    const Comp = customNodeRegistry[node.type_name] ?? customNodeRegistry['__fallback__'];
    return <Comp node={node} {...args} />;
  },
};

export const customNodeRegistry: Record<string, ComponentType<any>> = {
  ...Custom,
  __fallback__: GenericFallback,
};
```

Plan 2A's `q2-preview/dispatchers.tsx` ships `Block` and `Inline` with the muted-gray "(not yet implemented)" miss path. 2B's leaves under `Blocks` / `Inlines` populate the registry so the miss path stops firing for Pandoc base types; CustomNodes that the user-extended registry hasn't covered fall through to the `__fallback__` component instead of the muted-gray placeholder (since `CustomBlock`/`CustomInline` keys *are* registered, just generically).

#### `q2-preview/utils.ts` — shared component utilities

- `resolveImageSrc(url, currentFilePath): string` — VFS lookup + `data:` URI; pass-through for external URLs.
- `inlinesToPlainText(inlines): string` — Stringify pass for alt text and other plain-text contexts.
- `formatRefLabel(kind, number, title?): string` — produces "Theorem 1 (Pythagoras)"-style labels.
- `composeAttr(originalAttr, extraClasses, extraKvs): Attr` — adds classes/attrs without mutating original.
- `renderSlot(slot, setSlot, ctx): ReactNode` — slot dispatcher for CustomNode components:

```ts
function renderSlot(slot, setSlot, ctx) {
  switch (slot.kind) {
    case 'block':   return <Node node={slot.value} setLocalAst={n => setSlot({ kind: 'block', value: n })} {...ctx}/>;
    case 'inline':  return <Node node={slot.value} setLocalAst={n => setSlot({ kind: 'inline', value: n })} {...ctx}/>;
    case 'blocks':  return slot.value.map((b, i) => <Node key={i} node={b} setLocalAst={n => { const next = [...slot.value]; next[i] = n; setSlot({ kind: 'blocks', value: next }); }} {...ctx}/>);
    case 'inlines': return slot.value.map((inl, i) => <Node key={i} node={inl} setLocalAst={n => { const next = [...slot.value]; next[i] = n; setSlot({ kind: 'inlines', value: next }); }} {...ctx}/>);
  }
}
```

#### `q2-preview/quartoClasses.ts` — class-name constants

Pinned class taxonomy mirroring Rust's HTML output. Long-term candidate for code-generation; v1 hand-written. Categories:

- **Callout**: from `crates/quarto-core/src/transforms/callout_resolve.rs`.
- **Theorem / Proof / FloatRefTarget / Equation / CrossrefResolvedRef**: from `crates/quarto-core/src/transforms/crossref_render.rs`.
- **Section / Header levels**: from `crates/pampa/src/transforms/sectionize.rs`.

The exact list is enumerated during 2B implementation by reading the referenced Rust functions. The first commit of the implementation phase is the enumeration commit.

#### Update q2-preview entry to use unwrap / rewrap

`q2-preview/entry.tsx` (created by Plan 2A) is updated to call `unwrapCustomNodes` after parsing `astJson` and `rewrapCustomNodes` before posting `SET_AST`.

#### Fork Elliot's demos to `gordon/render-components`

The original Plan 2 framing was that q2-preview's components ship as pasted-into-demos `html.tsx` and `custom.tsx` drafts. Under the restructure, those components are q2-preview's built-in registry — pasted demos are no longer needed for basic rendering. The demo-playground role shifts from "this is how to render real HTML" to "here are the genuine custom-component overrides worth showcasing."

Action items, all under `~/docs/demo-playground/gordon/render-components/` (new directory, parallel to `elliot/`):

- **Fork**: copy Elliot's TSX and qmd files into `gordon/render-components/` as a starting point.
- **Rebase for q2-preview**: change `format: q2-debug` → `format: q2-preview` in qmd files where appropriate; change `window.__REACT_AST_DEBUG_RENDERER__` → `window.__Q2_PREVIEW_RENDERER__` in TSX files.
- **Prune the now-built-in**: remove TSX files / individual exports that q2-preview ships natively after 2B. Most of `html.tsx`'s contents (Para, Header, Str, Space, Emph, Strong, Code, Link, Image, Figure, Span, Quoted, Math, Div, RawBlock, etc.) become redundant. Keep only the components that demonstrate genuine *override* behavior beyond the built-ins.
- **Keep live demos**: `comment.tsx` (Slack-like commenting UI), `kanban.tsx` (drag-and-drop kanban), `drag.tsx` (generic drag helper), and any `slide.tsx` if applicable — these are real extensions, not gap-fillers.
- **Update docs**: rewrite `index.qmd` and `render_components.qmd` to reference the new path, the new format, the new global, and the post-2B "what's built-in vs. what you can override" model. The originals at `~/docs/demo-playground/elliot/` stay unchanged — q2-debug demos keep working there.
- **Confirm the override path actually works for q2-preview** end-to-end: pasting `comment.tsx` (or another genuine override) into a `format: q2-preview` doc with `render-components: [...]` should override the built-in registry's matching component name and render the user's version.

This fork lands as part of Plan 2B's PR (or a closely-following PR) because it depends on:
- Plan 2A's `format: q2-preview` routing and `__Q2_PREVIEW_RENDERER__` global.
- Plan 2B's built-in components (so the "remove now-built-in" pruning is meaningful).

### Out of scope

- Layout / chrome components (TOC sidebar, navbar, footer, page-nav strip rendering as page chrome). Deferred per Plan 2A.
- Edit affordances (theorem-rename UI, callout-type changer, etc.). v1 is structural-only rendering.
- Drift-detection contract test (Rust HTML output ↔ React render). Useful long-term; defer.
- Body-classes derivation, navbar brand-fallback. Deferred per Plan 2A.
- Quarto-specific Image extensions: `fig-align`, `fig-link`, `fig-alt`, `lightbox`, subfigures, `fig-cap-location`. Tier 3 — defer to a follow-up plan parallel to "q2-preview layout chrome."
- `BlockMetadata` (Quarto extension, structured config blocks — not user-visible content), rendered as fallback in v1.
- `NoteDefinitionPara`, `NoteDefinitionFencedBlock` (Quarto reference-style note definitions), rendered as fallback in v1.

### Defensive variants

- **Out-of-band**: `Shortcode` (desugared by `ShortcodeResolveTransform`), `NoteReference` / `InlineAttr` / `CaptionBlock` (defensive errors Q-3-21 / Q-3-31 / Q-3-32). If they appear, it's a bug elsewhere; v1 renders fallback.
- **Critic markup**: `Insert` / `Delete` / `Highlight` / `EditComment` are defensively serialized as `<span class="critic-{type}">` in the AST and pass through the existing `Span` component.

## Design decisions

- **Real-HTML leaves as q2-preview's built-in registry** — not "drafts pasted into demos." Pasted-demo overrides via `render-components: [...]` (Plan 2A item 13) still work; they layer on top of the built-ins instead of replacing missing defaults.
- **q2-preview/blocks/, q2-preview/inlines/, q2-preview/custom/ as a directory tree of one component per file**. Easier to navigate, override, and test than a single `html.tsx`. Barrel files (`q2-preview/blocks/index.ts` etc.) provide name-keyed re-exports for the registry.
- **Atomic-aware gate in framework's `Node`, not in either format's `Block`/`Inline`.** Plan 2pre moves the dispatchers out of framework into format-owned files; `Node` is the only remaining cross-format chokepoint where the gate can sit once. Correctness-level concern; benefits both formats. q2-debug picks up the gate "for free."
- **Two registries**: `componentRegistry` keyed by `node.t`, `customNodeRegistry` keyed by `type_name`. User overrides target one or the other explicitly.
- **CustomBlock / CustomInline dispatch**: registry's `CustomBlock` / `CustomInline` entries look up `customNodeRegistry[node.type_name]` and render with `CustomNodeArgs`. The framework's `Node` dispatcher gets `'CustomBlock'` added to `blockTypes`.
- **`html.tsx` and `custom.tsx` paste-in pattern still works** for users who want to override q2-preview's defaults. The 2B build-out makes the registry no longer require pasting to be useful.
- **The `'Ast'` registry entry in q2-preview is minimal**: just calls `renderChildren({ node: ast, setLocalAst: setAst, ... })` with no debug wrapper. The format-specific outer wrapper (PreviewContext provider, etc.) is in `q2-preview/entry.tsx` (`PreviewRoot`), not in the registry. (The registry key `'Ast'` is shared with q2-debug — see 2pre §"What stays exactly the same"; only the registered component differs per format.)
- **Image alt-text via Stringify**, not just `Str` filtering. Elliot's `html.tsx` had a `Str`-only filter; a real Pandoc Stringify pass handles `Emph` / `Code` / `SoftBreak` / etc. inside alt text correctly.
- **Visual fidelity tier**: class-compatible. Same CSS class names as Rust's HTML output where the AST shape diverges. DOM structure may differ where it doesn't affect CSS.

## Encode / decode / unwrap / rewrap (terminology)

The CustomNode lifecycle has four operations across the system:

- **Wrap (Rust → wire)**: `pampa/src/writers/json.rs::write_custom_block` / `write_custom_inline`. Rust CustomNode → wire-format Div / Span with `__quarto_custom_node` class.
- **Decode (Rust read)**: `pampa/src/readers/json.rs::read_custom_block_from_div` / `read_custom_inline_from_span`. Wire-format → Rust CustomNode.
- **Unwrap (JS, in iframe)**: NEW in 2B. Wire-format → JS-native `CustomBlockNode` / `CustomInlineNode`. Mirrors Rust's decode. Lives in `framework/customNode.ts`.
- **Rewrap (JS, in iframe before postMessage)**: NEW in 2B. JS-native CustomNode → wire-format Div / Span. Mirrors Rust's wrap. Lives in `framework/customNode.ts`.

Round-trip property: `unwrap(rewrap(x)) === x` and `wrap(unwrap(wireDiv)) === wireDiv`.

## Soft activation dependencies

- **Plan 4** introduces `Synthetic { by: By }` and `Derived { from, by }` SourceInfo variants. Until Plan 4 lands, no inline can have Derived source_info.
- **Plan 6** populates Derived source_info on shortcode resolutions. After Plan 6, the dispatcher's atomic detection activates for shortcode-resolved inlines.
- **Plan 8** introduces `IncludeExpansion` CustomNode and amends `atomicCustomNodes.ts` to add it. 2B's `IncludeExpansion` component is registered from the start.

## Multi-plan contracts

### Consumed: Plans 2pre and 2A foundation

- `framework/types.ts` — `BlockNode`, `InlineNode`, `PandocAST`, `Attr`, `Slot`, `CustomBlockNode` placeholder (filled in 2B).
- `framework/RegistryContext.tsx` — exported context with `sourceInfoPool` (added by 2A).
- `framework/Ast.tsx`, `framework/dispatch.tsx` (the consolidated recursion-and-render module from 2pre — houses `Node`, `renderChildren`, `renderNode`, `blockTypes`, and the framework-internal `renderChildrenRegistry`) — used unchanged from 2A; 2B modifies `Node` (atomic gate) and adds `CustomBlock`/`CustomInline` entries to `renderChildrenRegistry`, plus extends `blockTypes` with `'CustomBlock'`. All inside `dispatch.tsx`. The mutations to `renderChildrenRegistry` are framework-evolves-itself changes — the structure is not exposed via `framework/index.ts` or any format global. See 2pre §"`renderChildrenRegistry` is framework-internal" for the contract.
- `q2-preview/PreviewIframe.tsx`, `q2-preview/PreviewContext.tsx`, `q2-preview/registry.ts` skeleton, `q2-preview/entry.tsx` — extended by 2B with leaves and CustomNode components.
- `hub-client/public/q2-preview.html` — unchanged.
- `hub-client/src/types/sourceInfo.ts`, `hub-client/src/utils/sourceInfo.ts`, `hub-client/src/utils/atomicCustomNodes.ts` — read by the framework dispatcher gate.
- `hub-client/src/utils/iframeLinkHandlers.ts` — installed by 2A; unchanged.

### Consumed: Plan 1's page-scoped image artifacts

q2-preview's AST keeps `<img src>` as the user wrote it. `Image.tsx` resolves at render time via `resolveImageSrc(currentFilePath, src)` + `vfsReadBinaryFile`, reading bytes from the user's original VFS upload. The renderer does not contribute image bytes (per Plan 2A §"Multi-plan contract: page-scoped image artifacts" — bd-3gtn note).

### Provided: visual parity for q2-preview

After 2B lands, documents using callouts, theorems, proofs, figures, equations, images, and cross-references render with visual fidelity matching the HTML format. Plans 4 / 6 / 7 / 8 add to this incrementally without 2B needing amendment.

## Open questions for implementation

- **Inline-level Note rendering**: v1 renders `Note` as `<sup>[fn]</sup>` with hover tooltip. Confirm whether the marker numbering should match the Rust pipeline's numbering or just count inline order.
- **Cite rendering**: v1 renders the visible inline content (second position). Full citation rendering with bibliography is layout chrome — out of scope.
- **Equation tagging**: KaTeX handles `\tag` natively; confirm during implementation that the appended `\tag` survives intact through slot dispatch.
- **Quarto Image extensions** (`fig-align`, `fig-link`, etc.): Tier 3, deferred. Document the gap explicitly so users know what's missing.
- **`Math` component placement**: lives in `q2-preview/inlines/Math.tsx` (Pandoc base inline). Equation CustomNode wraps Math with crossref tagging; both render via the same KaTeX path.

## References

### Rust side (read during implementation; not modified by 2B)

- `crates/quarto-pandoc-types/src/{block,inline,custom}.rs` — canonical Block / Inline / CustomNode / Slot enums.
- `crates/pampa/src/writers/json.rs::write_custom_block` (line 1297), `write_custom_inline` (line 1380) — wire format for unwrap to mirror.
- `crates/pampa/src/readers/json.rs::read_custom_block_from_div` (line 2220), `read_custom_inline_from_span` (line 2358) — Rust-side decode to mirror in JS unwrap.
- `crates/quarto-core/src/transforms/callout_resolve.rs` — Callout HTML structure source.
- `crates/quarto-core/src/transforms/crossref_render.rs` — Theorem/Proof/FloatRefTarget/Equation/CrossrefResolvedRef HTML rendering.
- `crates/pampa/src/transforms/sectionize.rs` — Section / levelN classes.

### hub-client side (modified by 2B)

- `hub-client/src/components/render/framework/types.ts` — fill in `CustomBlockNode` / `CustomInlineNode` shapes.
- `hub-client/src/components/render/framework/dispatch.tsx` — atomic-aware gate inside `Node`; add CustomBlock / CustomInline traversal entries to `renderChildrenRegistry`; extend `blockTypes` with `'CustomBlock'`.
- `hub-client/src/components/render/framework/customNode.ts` (NEW) — unwrap / rewrap walks.
- `hub-client/src/components/render/q2-preview/blocks/*.tsx` (NEW) — every Pandoc Block.
- `hub-client/src/components/render/q2-preview/inlines/*.tsx` (NEW) — every Pandoc Inline (incl. Image).
- `hub-client/src/components/render/q2-preview/custom/*.tsx` (NEW) — type-specific CustomNode components.
- `hub-client/src/components/render/q2-preview/registry.ts` — populate.
- `hub-client/src/components/render/q2-preview/utils.ts` (NEW) — `resolveImageSrc`, `inlinesToPlainText`, `formatRefLabel`, `composeAttr`, `renderSlot`.
- `hub-client/src/components/render/q2-preview/quartoClasses.ts` (NEW) — class-name constants.
- `hub-client/src/components/render/q2-preview/entry.tsx` — call unwrap/rewrap.

### Demo files

- Elliot's existing `~/docs/demo-playground/elliot/html.tsx` is the seed for the q2-preview blocks/inlines registry. Files in `q2-preview/blocks/` and `q2-preview/inlines/` adopt his approach with the gap fills enumerated above and the alt-text-via-Stringify improvement.

## Test plan

### Unit / vitest

- **Unwrap / rewrap round-trip property**: for each known CustomNode type, `unwrap(rewrap(node)) === node` and `rewrap(unwrap(wireDiv)) === wireDiv`. Catches drift.
- **Rust → JS → Rust round-trip**: build a CustomNode in Rust, wrap to JSON, ship to JS, unwrap, rewrap, ship back, decode in Rust, assert structural equality.
- **Image renderer component tests**: mount `<Image>` with fixtures pointing at:
  - Project-relative path (`hero.png`) — assert `<img>` has `data:` URI src.
  - External URL (`https://...`) — assert pass-through.
  - `data:` URI — assert pass-through.
  - Non-existent project path — assert pass-through (GIGO).
  - Image with `width` / `height` kvs — assert attrs on `<img>`.
  - Image with id, classes, title — assert all attributes on `<img>`.
  - Image with non-`Str` alt inlines (`Emph`, `Code`) — assert alt text contains the expanded plain text.
- **Figure renderer**: mount `<Figure>` with fixture containing body Image and caption blocks; assert `<figure>` + `<figcaption>` structure with body recursion.
- **Component snapshot tests**: render each base-type component and each CustomNode component with a fixed input; snapshot the rendered DOM.
- **Generic fallback test**: render a wrapper Div with `type_name: "Unknown"` via the renderer plumbing; assert the fallback component renders with the type name visible.
- **Class-compatibility test**: for each component, assert the rendered classes match the documented class taxonomy.
- **Atomic CustomNode read-only test**: render a `CrossrefResolvedRef` wrapper; assert children don't receive a usable `setLocalAst`.
- **Derived inline read-only test**: render a Para containing inlines with `Derived` source_info (a shortcode-resolved title); confirm setLocalAst is no-op (shortcode populating a Derived entry — until Plan 6, this test uses hand-constructed pool entries).

### Pandoc base-type gap-fill tests

- One per new component (LineBlock, DefinitionList, Table family, Underline, Strikeout, Superscript, Subscript, SmallCaps, RawInline, Cite, Note). Render representative AST node, snapshot DOM.
- **Table family integration**: render a real markdown pipe table through q2-preview pipeline, assert `<table>` / `<thead>` / `<tbody>` structure with correct cell alignment classes.

### Browser smoke (playwright, marked as e2e)

- Open a fixture in hub-client containing a callout, a theorem, a cross-reference, an equation, and an embedded image; switch format to `q2-preview`; assert each visual element renders with the expected class set and text content.
- Open a fixture containing `![alt](hero.png){width=400}` with a real image uploaded; assert `<img>` rendered with width=400 and alt text.

## Dependencies

### Hard dependencies

- **Plan 2pre** — directory restructure. 2B's framework changes (atomic-aware gate inside `Node`, customNode.ts, types.ts CustomNode shapes, CustomBlock entries in `renderChildrenRegistry`, all in `framework/dispatch.tsx`) reference paths and structures Plan 2pre establishes.
- **Plan 2A** — q2-preview surface scaffolding. 2B fills the registry skeleton 2A creates; consumes PreviewContext, registry barrel, entry.tsx.
- **Plan 1** — pipeline + format detection (already shipped).

### Soft / activation dependencies

(See §"Soft activation dependencies" above.) Plans 4, 6, 7, 8 add to the AST shape 2B watches for; until they land, the relevant detection arms stay dormant.

### Blocks

Nothing structurally. Plans 4 / 5 / 6 / 7 / 8 can land in parallel with 2B; they decorate the AST that 2B's components render.

## Risk areas

- **Round-trip correctness in unwrap / rewrap.** The two functions must be exact mirrors of each other and of Rust's `write_custom_block` / `read_custom_block_from_div`. Property tests catch drift.
- **`CrossrefResolvedRef` Span vs Div wrapper handling**. Inline CustomNode, wire-format wraps in Span. Unwrap / rewrap walks must handle both wrappers uniformly.
- **Math (KaTeX) inside Equation CustomNode**. Tagged equations need to round-trip cleanly through KaTeX. Browser smoke test is the safety net.
- **Drift between Rust's HTML output and our React rendering**. Class-compatible commitment, not DOM-equivalent. Where DOM differs, CSS may need adjustment.
- **`__quarto_custom_node` class polluting rendered DOM after user override**. Resolved by design: unwrap is the single forward-path conversion, runs before any registry dispatch. The `Div` registry slot only sees real Divs.
- **Class-taxonomy enumeration completeness**. First implementation commit enumerates classes from the named Rust source files. Mitigation: cross-check against actual q2-preview demo renders.
- **Image alt-text edge cases**: Stringify pass must handle every Pandoc inline that can appear in alt context; missing one degrades alt to empty. Test coverage explicitly walks the inline taxonomy.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| Framework: `customNode.ts` (unwrap + rewrap) | ~160 |
| Framework: `types.ts` CustomNode shapes | ~60 |
| Framework: atomic gate inside `Node` (`framework/dispatch.tsx`) + tests | ~50 |
| Framework: CustomBlock/CustomInline entries in `renderChildrenRegistry` (in `dispatch.tsx`) + `blockTypes` extension | ~30 |
| q2-preview/blocks/*.tsx (14 files; 11 existing-pattern + 3 gap fills) | ~250 |
| q2-preview/inlines/*.tsx (20 files; 12 existing-pattern + 8 gap fills) | ~220 |
| q2-preview/custom/*.tsx (7 files + fallback) | ~360 |
| q2-preview/utils.ts (resolveImageSrc, inlinesToPlainText, formatRefLabel, composeAttr, renderSlot) | ~120 |
| q2-preview/quartoClasses.ts | ~80 |
| q2-preview/registry.ts assembly | ~50 |
| q2-preview/entry.tsx unwrap/rewrap wiring | ~10 |
| Tests (round-trip, component snapshots, atomic, Derived, Image edge cases) | ~300 |
| **Total** | **~1690** |

Larger than the original Plan 2B's ~1190 LOC because Image / Figure (originally in Plan 2A item 8) plus the explicit Pandoc base-type leaves are now in 2B's scope. Reasonable for two focused sessions:

- **Session A**: Framework changes (customNode.ts, types.ts, dispatcher gate, renderChildren entries) + q2-preview/blocks + q2-preview/inlines (incl. Image, Figure, Math, gap fills). Verifies end-to-end rendering of basic Quarto docs.
- **Session B**: q2-preview/custom + utils + quartoClasses + registry assembly + tests. Visual parity for callouts / theorems / cross-references.

Risk: Table family is the highest-effort single component (~80 LOC). Budget extra time.

## Notes

- This plan replaces the original Plan 2B, which framed `html.tsx` and `custom.tsx` as "drafts pasted into demos." The 2026-05-07 review established that q2-preview is a sibling format with its own built-in registry; the paste-in pattern still works for user overrides but is no longer the default delivery mechanism.
- The atomic-aware gate moved from "modify q2-debug's dispatcher" to framework's `Node` (the single recursion chokepoint, in `framework/dispatch.tsx`) — benefits both formats automatically without modifying either format's dispatcher.
- Image and Figure moved into 2B as the natural place for "Pandoc base type leaves with full semantics."
- Following the user's lead: q2-preview is intended to evolve toward a system component (likely a Quarto extension), but the bundling / distribution mechanics are out of scope for 2B.

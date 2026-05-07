# Plan 2B — q2-preview built-in components (html.tsx + custom.tsx)

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (resolved during the 2026-05-06 review session)
**Milestone:** M2 (q2-preview reaches visual parity with the HTML format
  for documents that use callouts, theorems, proofs, figures, equations,
  and cross-references)

## Goal

Deliver the type-specific React components that render q2-preview's
post-pipeline AST faithfully:

- **`html.tsx`** — fills the Pandoc base-type gaps in Elliot's existing
  draft so any post-pipeline node renders correctly (LineBlock,
  DefinitionList, Table family, Underline, Strikeout, Superscript,
  Subscript, SmallCaps, Cite, RawInline, Note).
- **`custom.tsx`** — renders the seven CustomNode `type_name` values
  q2-preview produces (Callout, Theorem, Proof, FloatRefTarget,
  Equation, CrossrefResolvedRef, IncludeExpansion).
- **Renderer plumbing** — `unwrapCustomNodes` and `rewrapCustomNodes`
  walks at the iframe boundary, plus atomic-aware `setLocalAst`
  gating in the `Block` / `Inline` dispatchers.
- **Class-name constants module** — pinned class taxonomy mirroring
  Rust's HTML output, so Quarto's compiled theme CSS produces matching
  visuals.

The two TSX files are delivered as **drafts** that get manually pasted
into a demo's `render-components: [...]` YAML key. (Plan 2A extended
that gate to cover q2-preview.) Bundling them as a system component —
likely as a Quarto extension — is a future effort.

The intention is that this is a draft of something that will become a
system component, so the design choices matter even though the
distribution mechanism is informal.

## Scope

### In scope

#### `html.tsx` — Pandoc base-type gap fills

Elliot's existing `html.tsx` covers eleven Block variants and twelve
Inline variants. After cross-referencing
`crates/quarto-pandoc-types/src/{block,inline}.rs` (the canonical
enums) against the pipeline's transform output (Plan 1's transform
list), 2B fills the following gaps:

**Blocks (3 new components, ~100 LOC):**

| Variant | Wire shape | Component sketch |
|---|---|---|
| `LineBlock` | `{t: 'LineBlock', c: Inline[][]}` | `<div class="line-block">` with each line as a `<div>` of inlines |
| `DefinitionList` | `{t: 'DefinitionList', c: [[Inline[], Block[][]]]}` | `<dl><dt>...</dt><dd>...</dd>...</dl>` |
| `Table` family | `{t: 'Table', c: [attr, caption, colspecs, head, bodies, foot]}` plus `attrS` / `captionS` / `headS` / `bodiesS` / `footS` source-info siblings | `<table>` with `<caption>`, `<thead><tr><th>...`, `<tbody><tr><td>...`, `<tfoot>`. Cells are `[attr, alignment, rowSpan, colSpan, Block[]]` arrays. ColSpec is `[alignment, colwidth]`. ~80 LOC for the family. |

**Inlines (8 new components, ~50 LOC):**

| Variant | Wire shape | Component |
|---|---|---|
| `Underline` | `{t: 'Underline', c: Inline[]}` | `<u>` |
| `Strikeout` | `{t: 'Strikeout', c: Inline[]}` | `<s>` |
| `Superscript` | `{t: 'Superscript', c: Inline[]}` | `<sup>` |
| `Subscript` | `{t: 'Subscript', c: Inline[]}` | `<sub>` |
| `SmallCaps` | `{t: 'SmallCaps', c: Inline[]}` | `<span style="font-variant: small-caps">` |
| `RawInline` | `{t: 'RawInline', c: [format, content]}` | If `format === 'html'`, `dangerouslySetInnerHTML`; else `<code>` |
| `Cite` | `{t: 'Cite', c: [Citation[], Inline[]]}` plus `citationIdS` source-info per citation | Render the visible inlines (second-position content); citations array provides metadata |
| `Note` | `{t: 'Note', c: Block[]}` | Inline footnote marker; v1 renders as `<sup>[fn]</sup>` with hover tooltip — full footnote pane is layout chrome (deferred per Plan 2A) |

**Deferred (rendered as fallback "Not registered" boxes in v1):**

- `BlockMetadata` (Quarto extension, structured config blocks — not
  user-visible content).
- `NoteDefinitionPara`, `NoteDefinitionFencedBlock` (Quarto reference-
  style note definitions).

**Out-of-band variants (don't appear post-pipeline):**

- `Shortcode` — desugared by `ShortcodeResolveTransform`.
- `NoteReference`, `InlineAttr`, `CaptionBlock` — defensive errors
  (Q-3-21 / Q-3-31 / Q-3-32). If they appear, it's a bug elsewhere.
- `Insert` / `Delete` / `Highlight` / `EditComment` — defensively
  serialized as `<span class="critic-{insert,delete,highlight,comment}">`,
  pass through the existing `Span` component.

#### `custom.tsx` — type-specific CustomNode components

- **`Callout`** — header (icon + title), body. Class-compatible with
  Rust's callout HTML (`callout`, `callout-{type}`, `callout-header`,
  `callout-title-container`, `callout-body`).
- **`Theorem`** — labeled block with title from `formatRefLabel(kind, number, title?)`.
  Class-compatible with `theorem`, `theorem-title`.
- **`Proof`** — labeled block. Class-compatible with `proof`,
  `proof-title`.
- **`FloatRefTarget`** — figure-like wrapper for `#fig-foo` /
  `#tbl-foo` content with caption.
- **`Equation`** — Math content with optional `\tag{N}` (appended by
  `CrossrefIndex`); slot-renders the inner Math through `html.tsx`'s
  existing `Math` component (`/Users/gordon/docs/demo-playground/elliot/html.tsx:259-279`),
  which uses `window.katex`.
- **`CrossrefResolvedRef`** — inline CustomNode (Span wrapper, not
  Div). Renders the resolved reference text. Atomic — slot children
  do not receive `setLocalAst`.
- **`IncludeExpansion`** — dormant placeholder until Plan 8 produces
  these. Atomic per `atomicCustomNodes.ts`. v1 renders as a
  transparent passthrough (just renders the slot blocks); 2B's
  registration is in place from the start, so when Plan 8's wrapper
  starts appearing in the AST, it renders correctly.
- **Generic fallback** for unknown `type_name` values — styled box
  with `data-custom-type` displayed and slot contents nested. Useful
  for extension-defined CustomNodes that q2-preview encounters
  without a specific component. Registered as `customNodeRegistry['__fallback__']`.

#### JS-native CustomNode TS interface (`hub-client/src/types/customNode.ts`)

```ts
import type { Attr, BlockNode, InlineNode } from './pandoc';

/**
 * Slot contents in a JS-native CustomNode. Mirrors quarto-pandoc-types::Slot.
 * Wire format wraps Inline / Inlines slots in a Plain block; the
 * unwrap walk strips the Plain wrapper so components see the same
 * shape Rust uses.
 */
export type Slot =
  | { kind: 'block';   value: BlockNode }
  | { kind: 'inline';  value: InlineNode }
  | { kind: 'blocks';  value: BlockNode[] }
  | { kind: 'inlines'; value: InlineNode[] };

interface CustomNodeBase {
  type_name: string;
  /** Insertion order matters for round-trip; preserved via object property order. */
  slots: Record<string, Slot>;
  plain_data: unknown;
  /** Pandoc-shape attr triple, with `__quarto_custom_node` class and
   *  `data-custom-{type,slots,data}` kvs already stripped by unwrap. */
  attr: Attr;
  /** Source-info pool ID. The pool itself lives on the AST root via context. */
  s?: number;
}

/** Custom node parsed from a wrapper Div. Lives in the BlockNode union. */
export interface CustomBlockNode extends CustomNodeBase {
  t: 'CustomBlock';
}

/** Custom node parsed from a wrapper Span. Lives in the InlineNode union. */
export interface CustomInlineNode extends CustomNodeBase {
  t: 'CustomInline';
}

export type CustomNode = CustomBlockNode | CustomInlineNode;
```

The `'CustomBlock'` / `'CustomInline'` discriminator (not a single
`'Custom'` + `variant` field) is chosen because (a) the existing
`Node` dispatcher in `ReactAstDebugRenderer.tsx:582` uses a
hardcoded `blockTypes` array, and adding two distinct `t` values
fits with a one-line addition; (b) block-vs-inline becomes a
static type property — `CustomBlockNode extends BlockNode`
cleanly; (c) round-trip is unambiguous: `'CustomBlock'` rewraps
to a Div, `'CustomInline'` to a Span. The placeholder discriminants
were pre-declared in 2A's `types/pandoc.ts`; 2B's unwrap walk is
the first runtime producer.

#### Unwrap / rewrap plumbing (`hub-client/src/utils/customNode.ts`)

Pure functions, no React, no context. Mirror Rust's
`write_custom_block` / `read_custom_block_from_div` plus their
inline siblings:

```ts
export function unwrapCustomNodes(ast: PandocAST): PandocAST;
export function rewrapCustomNodes(ast: PandocAST): PandocAST;
```

**Forward path** (wire → render):
1. `<Ast>` parses `astJson` (today).
2. `<Ast>` calls `unwrapCustomNodes(parsed)` — single tree walk
   that replaces wrapper Div / Span (identified by the
   `__quarto_custom_node` class) by `CustomBlockNode` /
   `CustomInlineNode` shapes. The walk reads `type_name` from the
   `data-custom-type` attribute (the canonical discovery mechanism
   per Plan 2A §"Provided: atomicCustomNodes hand-mirror"), reads
   slot metadata from `data-custom-slots`, and reads `plain_data`
   from `data-custom-data`. It strips the wrapper class and
   `data-custom-*` kvs from `attr`; strips the `Plain` wrapper from
   Inline / Inlines slots; recurses into slot contents (for nested
   CustomNodes — Plan 8 case).
3. After unwrap, the AST contains zero `__quarto_custom_node`
   references. `componentRegistry['Div']` / `['Span']` only see
   real Divs / Spans.
4. `componentRegistry['CustomBlock']` and `['CustomInline']` are
   thin dispatchers that look up `customNodeRegistry[node.type_name]`
   and render with `CustomNodeArgs`. The `Node` dispatcher gets
   one new entry in its `blockTypes` array: `'CustomBlock'`.

**Reverse path** (setLocalAst → setAst → postMessage):
1. Components freely propagate JS-native shapes through `setLocalAst`.
2. Reassembly walks back up to the root `setAst` (already wired in
   `ast-renderer-entry.tsx:118-123`).
3. Before postMessage, `rewrapCustomNodes(ast)` walks once and
   rewrites `CustomBlockNode` → wire-format Div,
   `CustomInlineNode` → wire-format Span; re-adds
   `__quarto_custom_node` class and `data-custom-{type,slots,data}`
   kvs; re-wraps Inline / Inlines slots in `Plain`.
4. Wire-format AST goes to parent via postMessage.

The handler change in `ast-renderer-entry.tsx`:

```ts
setAst={(newAst) => {
  window.parent.postMessage({
    type: 'SET_AST',
    ast: rewrapCustomNodes(newAst),    // ← new
  }, '*');
}}
```

#### `renderSlot` helper

```ts
function renderSlot(
  slot: Slot,
  setSlot: (newSlot: Slot) => void,
  ctx: { onNavigateToDocument?: ...; },
): ReactNode {
  switch (slot.kind) {
    case 'block':
      return <Node node={slot.value} setLocalAst={(n) => setSlot({ kind: 'block', value: n as BlockNode })} {...ctx}/>;
    case 'inline':
      return <Node node={slot.value} setLocalAst={(n) => setSlot({ kind: 'inline', value: n as InlineNode })} {...ctx}/>;
    case 'blocks':
      return slot.value.map((b, i) => (
        <Node key={i} node={b} setLocalAst={(n) => {
          const next = [...slot.value]; next[i] = n as BlockNode;
          setSlot({ kind: 'blocks', value: next });
        }} {...ctx}/>
      ));
    case 'inlines':
      return slot.value.map((inl, i) => (
        <Node key={i} node={inl} setLocalAst={(n) => {
          const next = [...slot.value]; next[i] = n as InlineNode;
          setSlot({ kind: 'inlines', value: next });
        }} {...ctx}/>
      ));
  }
}
```

The `dispatch` parameter the original Plan 2 hand-waved is just
`Node`. No new context needed — `Node` resolves the registry
through `RegistryContext` already.

#### Atomic-aware `setLocalAst` gating in dispatchers

The original Plan 2's `MaybeReadOnlyInline` wrapper is **not** a
new component. 2B introduces unified `Block` / `Inline` dispatcher
components in `ReactAstDebugRenderer.tsx` that wrap the existing
`registry[node.t]` lookup with the atomic-aware gate. (Plan 2A
left the registry pattern in place untouched; 2B is what
introduces the dispatcher seam where the gate lives.)

```ts
const Inline = (args: NodeArgs<InlineNode>) => {
  const registries = useContext(RegistryContext);
  const registry = registries?.registry ?? componentRegistry;
  const pool = registries?.sourceInfoPool;       // from 2A

  const isAtomic = isAtomicSourceInfo(args.node, pool, ATOMIC_SYNTHETIC_KINDS)
                || (args.node.t === 'CustomInline' && isAtomicCustomNode(args.node.type_name));

  const effectiveArgs = isAtomic
    ? { ...args, setLocalAst: NOOP }
    : args;

  const Component = registry[args.node.t];
  return Component ? <Component {...effectiveArgs} /> : <span>...</span>;
};
```

Same modification for `Block` (handles atomic blocks like
`IncludeExpansion`). Three atomic detection paths converge into one
helper:

1. **Derived source_info** (Plan 6's shortcode resolutions) — via
   `isAtomicSourceInfo`'s `isDerived` arm.
2. **Atomic Synthetic source_info** (Plan 4's `By::is_atomic_synthesizer()`)
   — via the `ATOMIC_SYNTHETIC_KINDS` set.
3. **Atomic CustomNode types** (`CrossrefResolvedRef` today;
   `IncludeExpansion` post-Plan-8) — via `isAtomicCustomNode`.

#### Class-name constants module (`hub-client/src/utils/quartoClasses.ts`)

Pinned class taxonomy mirroring Rust's HTML output. Long-term
candidate for code-generation from the Rust source; for v1 a
hand-written const set, with header comment naming Rust source
files. Categories:

- **Callout**: extracted from
  `crates/quarto-core/src/transforms/callout_resolve.rs`.
- **Theorem / Proof / FloatRefTarget / Equation /
  CrossrefResolvedRef**: extracted from
  `crates/quarto-core/src/transforms/crossref_render.rs` (functions
  `render_theorem`, `render_proof`, `render_float_ref_target`,
  `render_equation`, `render_resolved_ref`).
- **Section / Header levels**: extracted from
  `crates/pampa/src/transforms/sectionize.rs`.

The exact list — every class string used by Rust's HTML output for
the seven CustomNode types — is enumerated during 2B implementation
by reading the referenced Rust functions. The first commit of 2B's
implementation phase is the enumeration commit; subsequent commits
land the components that consume the constants.

#### Shared utilities

- `formatRefLabel(kind, number, title?)` — produces "Theorem 1
  (Pythagoras)"-style labels.
- `composeAttr(originalAttr, extraClasses, extraKvs)` — adds
  classes / attrs to a Pandoc Attr without mutating original.
- `renderSlot(slot, setSlot, ctx)` — see above.

### Out of scope

- Layout / chrome components (TOC sidebar, navbar, footer, page-nav
  strip rendering as page chrome). Plan 2A §Out-of-scope documents
  what's not yet replicated from the HTML pipeline.
- Edit affordances (theorem-rename UI, callout-type changer, etc.).
  v1 is structural-only rendering.
- Bundling / distribution. Files are pasted into demos via
  `render-components: [...]` YAML.
- Drift-detection contract test (Rust HTML output ↔ React render).
  Useful long-term; defer.
- Body-classes derivation, navbar brand-fallback (deferred per
  Plan 2A).

## Design decisions (settled in 2026-05-06 review session)

- **`html.tsx` uses raw wrapper (Option A)**: components receive
  raw AST nodes. By the time html.tsx components fire, unwrap has
  replaced wrapper Divs with `CustomBlockNode` shapes — so
  html.tsx's `Div` component never sees `__quarto_custom_node`
  classes.
- **`custom.tsx` uses unwrapped CustomNode form (Option B)**:
  components receive the JS-native `CustomBlockNode` /
  `CustomInlineNode` shape so they can read `node.slots.title.value`
  directly.
- **Renderer plumbing intercepts wrapper Divs / Spans before
  component dispatch**. Both files stay independent of each other
  — the unwrap / rewrap layer lives in `utils/customNode.ts` and
  is invoked at the iframe boundary in `ast-renderer-entry.tsx`
  and `<Ast>`.
- **Two registries**: `componentRegistry` keyed by `node.t`,
  `customNodeRegistry` keyed by `type_name`. User overrides target
  one or the other explicitly. Generic fallback in
  `customNodeRegistry['__fallback__']` handles unknown type_names.
- **Atomic content is read-only**, enforced via the dispatcher
  modification described above. No separate `MaybeReadOnlyInline`
  wrapper component.
- **Visual fidelity tier**: class-compatible. Same CSS class names
  as Rust's HTML output where the AST shape diverges, so loading
  Quarto's CSS produces matching visuals. DOM structure may differ
  where it doesn't affect CSS.
- **Class-name constants live in `hub-client/src/utils/quartoClasses.ts`**
  for v1. Long-term the constants are a candidate for code-generation
  from a single Rust source.

## Encode / decode / rewrap (terminology and operations)

The CustomNode lifecycle has four operations across the system:

- **Wrap (Rust → wire)**: `pampa/src/writers/json.rs::write_custom_block`
  / `write_custom_inline`. Rust CustomNode → wire-format Div / Span
  with `__quarto_custom_node` class.
- **Decode (Rust read)**: `pampa/src/readers/json.rs::read_custom_block_from_div`
  / `read_custom_inline_from_span`. Wire-format → Rust CustomNode.
- **Unwrap (JS, in iframe)**: NEW in 2B. Wire-format → JS-native
  `CustomBlockNode` / `CustomInlineNode`. Mirrors Rust's decode.
  Lives in `utils/customNode.ts`.
- **Rewrap (JS, in iframe before postMessage)**: NEW in 2B.
  JS-native CustomNode → wire-format Div / Span. Mirrors Rust's
  wrap. Lives in `utils/customNode.ts`.

The wire format is the lingua franca; typed shapes are local
conveniences on each side. Rust's wrap and the JS rewrap produce
the same wire format from either typed shape; Rust's decode and
JS's unwrap produce typed shapes from the wire format. Round-trip
property: `unwrap(rewrap(x)) === x` and `wrap(unwrap(wireDiv)) === wireDiv`.

## Soft activation dependencies

2B's atomic-detection wiring is dormant until later plans populate
the AST shapes it watches:

- **Plan 4** introduces the `Synthetic { by: By }` and
  `Derived { from, by }` SourceInfo variants. Until Plan 4 lands,
  no inline can have Derived source_info, so `isAtomicSourceInfo`'s
  Derived arm never fires.
- **Plan 6** populates Derived source_info on shortcode resolutions.
  After Plan 6, the dispatcher's atomic detection activates for
  shortcode-resolved inlines.
- **Plan 8** introduces `IncludeExpansion` CustomNode and amends
  `atomicCustomNodes.ts` to add it. 2B's `IncludeExpansion`
  component is registered from the start; until Plan 8, no
  IncludeExpansion CustomNodes appear in the AST so the component
  is never instantiated.

This dormant-wiring pattern matches Plan 2A's source-info wiring
vs. Plan 5 wire-format codes 4/5.

## Multi-plan contracts

### Consumed: Plan 2A's iframe foundation

2B builds on 2A's plumbing without modifying it:
- `hub-client/src/types/sourceInfo.ts` (wire-format types).
- `hub-client/src/types/pandoc.ts` (consolidated `PandocAST` with
  `astContext?` field and `CustomBlockNode` / `CustomInlineNode`
  placeholders).
- `hub-client/src/utils/sourceInfo.ts` (`entryFor`, `isDerived`,
  `isAtomicSourceInfo`, `ATOMIC_SYNTHETIC_KINDS`).
- `hub-client/src/utils/atomicCustomNodes.ts`
  (`isAtomicCustomNode`); `type_name` matching against the
  `data-custom-type` attribute the JSON writer emits — see Plan 2A
  §"Provided: atomicCustomNodes hand-mirror" for the discovery
  mechanism.
- `RegistryContext` extension carrying `sourceInfoPool` and
  `currentFilePath`.
- Render-time `<img src>` resolution in the existing `Image`
  registry component (reads `currentFilePath` from
  `RegistryContext`, emits `<img src="data:...">` directly). 2B's
  `html.tsx` does not override `Image`; if a future override exists,
  it must preserve or replicate this behavior to keep images
  loading.
- `iframeLinkHandlers.ts` (external new-tab, `.qmd` clicks, anchor
  clicks, `Ctrl+S` save — installed once via event delegation;
  reads current props via a ref so handlers stay correct across
  document navigation).
- Theme CSS injection in `AstWithAssets`, fingerprint-keyed
  (`themeFingerprint` from `RenderResponse` triggers re-injection
  on theme swap; the `<style data-q2-theme>` marker doubles as a
  StrictMode idempotency guard).
- `:where()`-wrapped `ast-renderer.html` styling.
- `render-components` gate covering q2-preview.

### Consumed: Plan 1's page-scoped image artifacts

q2-preview's AST keeps `<img src>` as the user wrote it (e.g.
`hero.png`); `ResourceCollectorTransform` does not rewrite the
target. Plan 2A's `Image` component renderer resolves these at
render time via `resolveRelativePath(currentFilePath, src)` +
`vfsReadBinaryFile`, reading bytes from the user's original VFS
upload (the renderer does not contribute image bytes — see Plan
2A §"Multi-plan contract: page-scoped image artifacts" for the
full contract and the latent-bug note, bd-3gtn). 2B's
type-specific components don't emit `<img>` directly — when an
`Image` node appears in a slot, slot rendering routes through the
`Image` registry component, which 2A wired for render-time
resolution. No `/.quarto/...` paths appear in q2-preview's body
AST.

### Provided: visual parity for q2-preview

After 2B lands: documents using callouts, theorems, proofs,
figures, equations, and cross-references render with visual
fidelity matching the HTML format. Plan 4 / 6 / 7 / 8 add to this
incrementally without 2B needing amendment.

## Open questions for implementation

- **Inline-level Note rendering**: v1 renders `Note` as a
  `<sup>[fn]</sup>` marker with a hover tooltip showing the note
  content. Full footnote pane (collected at end of document) is
  layout chrome — out of scope. Confirm during implementation
  whether the marker numbering should match the Rust pipeline's
  numbering or just count inline order.
- **Cite rendering**: v1 renders the visible inline content
  (second position). Full citation rendering with bibliography is
  layout chrome — and is HTML-pipeline-only behavior the q2-preview
  pipeline excludes. The visible inlines are correct text;
  references to a bibliography are not rendered as resolved
  citation links in v1.
- **Equation tagging**: Equation CustomNode contains a Math inline
  with optional `\tag{N}` appended by `CrossrefIndex`. KaTeX
  handles `\tag` natively; confirm during implementation that the
  appended `\tag` survives intact through the slot dispatch.

## References

### Rust side (read during implementation; not modified by 2B)

- `crates/quarto-pandoc-types/src/{block,inline,custom}.rs` —
  canonical Block / Inline / CustomNode / Slot enums.
- `crates/pampa/src/writers/json.rs::write_custom_block` (line
  1297), `write_custom_inline` (line 1380) — wire format for
  unwrap to mirror.
- `crates/pampa/src/readers/json.rs::read_custom_block_from_div`
  (line 2220), `read_custom_inline_from_span` (line 2358) —
  Rust-side decode to mirror in JS unwrap.
- `crates/quarto-core/src/transforms/callout_resolve.rs` — Callout
  HTML structure source.
- `crates/quarto-core/src/transforms/crossref_render.rs::render_theorem`
  (line ~321), `render_proof` (~534), `render_float_ref_target`
  (~223), `render_equation` (~601), `render_resolved_ref` (~657)
  — type-specific HTML rendering to mirror in TSX.
- `crates/pampa/src/transforms/sectionize.rs` — Section / levelN
  classes.

### hub-client side (consumed from Plan 2A; modified by 2B)

- `hub-client/src/types/pandoc.ts` (consolidated by 2A) — extend
  with concrete `CustomBlockNode` / `CustomInlineNode` shapes
  (placeholders are already there).
- `hub-client/src/types/sourceInfo.ts` (from 2A) — read by
  `isAtomicSourceInfo`.
- `hub-client/src/utils/sourceInfo.ts` (from 2A) — call
  `isAtomicSourceInfo` from dispatcher.
- `hub-client/src/utils/atomicCustomNodes.ts` (from 2A) — call
  `isAtomicCustomNode` from dispatcher.
- `hub-client/src/components/render/ReactAstDebugRenderer.tsx` —
  introduce unified `Block` / `Inline` dispatcher components
  wrapping the existing `registry[node.t]` lookup with the
  atomic-aware gate; add `'CustomBlock'` to `blockTypes`; add
  `Custom*` registry entries.
- `hub-client/src/ast-renderer-entry.tsx` — call
  `unwrapCustomNodes` post-parse, `rewrapCustomNodes` pre-postMessage.
- `hub-client/src/utils/customNode.ts` (NEW) — unwrap / rewrap.
- `hub-client/src/types/customNode.ts` (NEW) — JS-native CustomNode types.
- `hub-client/src/utils/quartoClasses.ts` (NEW) — class-name constants.

### Demo files

- Elliot's existing `~/docs/demo-playground/elliot/html.tsx` is
  the starting point. 2B's `html.tsx` adds the Pandoc base-type
  gap fills enumerated in §"In scope".
- New `custom.tsx` is delivered as a Plan 2B artifact for paste-in
  demo use.

## Test plan

### Unit / vitest

- **Unwrap / rewrap round-trip property**: for each known
  CustomNode type, `unwrap(rewrap(node)) === node` (deep structural
  equality on the JS-native shape) and
  `rewrap(unwrap(wireDiv)) === wireDiv` (deep equality on the wire
  shape). Catches drift between unwrap / rewrap.
- **Rust → JS → Rust round-trip**: build a CustomNode in Rust, wrap
  to JSON, ship to JS (mock the iframe boundary), unwrap, rewrap,
  ship back, decode in Rust, assert structural equality with the
  original Rust node.
- **Component snapshot tests**: render each of the 7 CustomNode
  components with a fixed input, snapshot the rendered DOM. Detect
  unintended changes.
- **Generic fallback test**: render a wrapper Div with
  `type_name: "Unknown"` via the renderer plumbing; assert the
  fallback component renders with the type name visible.
- **Class-compatibility test**: for each component, assert the
  rendered classes match the documented class taxonomy
  (`utils/quartoClasses.ts`).
- **Atomic CustomNode read-only test**: render an
  `IncludeExpansion` (synthesized) or `CrossrefResolvedRef`
  wrapper; assert children don't receive a usable `setLocalAst`
  prop.
- **Derived inline read-only test**: render a Para containing
  inlines with `Derived` source_info (a shortcode-resolved title);
  confirm typing into the resolved text doesn't propagate
  `setLocalAst`.

### Pandoc base-type gap-fill tests

- **html.tsx gap-fill snapshot tests**: one per new component
  (LineBlock, DefinitionList, Table family, Underline, Strikeout,
  Superscript, Subscript, SmallCaps, RawInline, Cite, Note).
  Render a representative AST node, snapshot the resulting DOM.
- **Table family integration**: render a real markdown pipe table
  through the q2-preview pipeline, assert the rendered DOM has
  `<table>` / `<thead>` / `<tbody>` structure with correct cell
  alignment classes.

### Browser smoke (playwright, marked as e2e)

- Open a fixture in hub-client containing a callout, a theorem, a
  cross-reference, and an embedded image; switch format to
  `q2-preview`; assert each visual element renders with the
  expected class set and text content.
- Confirm that pasting `html.tsx` and `custom.tsx` into a demo's
  `render-components: [...]` produces the same result as a future
  bundled extension would.

## Dependencies

### Hard dependencies

- **Plan 2A** — iframe foundation. 2B consumes every artifact 2A
  ships (PandocAST consolidation with `BlockNode`/`InlineNode`
  naming, source-info accessor, atomicCustomNodes.ts including
  the `data-custom-type` discovery mechanism, render-time `Image`
  renderer with `currentFilePath` in `RegistryContext`, link
  handlers via ref-based context, fingerprint-keyed theme CSS
  injection, render-components gate). 2B cannot land before 2A.
- **Plan 1** — pipeline + format detection (already shipped). 2B
  renders the AST shape Plan 1's pipeline produces.

### Soft / activation dependencies

(See §"Soft activation dependencies" above.) Plans 4, 6, 7, 8
add to the AST shape 2B watches for; until they land, the
relevant detection arms stay dormant.

### Blocks

Nothing structurally. Plans 4 / 5 / 6 / 7 / 8 can land in parallel
with 2B; they decorate the AST that 2B's components render.

## Risk areas

- **Round-trip correctness in `unwrap` / `rewrap`**. The two
  functions must be exact mirrors of each other (and of Rust's
  `write_custom_block` / `read_custom_block_from_div`). Property
  tests catch drift; the tests should run on every CI build.
- **`CrossrefResolvedRef` Span vs Div wrapper handling**. It's an
  inline CustomNode, wire-format wraps in Span, not Div. The
  unwrap / rewrap walks must handle both wrappers uniformly. Test
  coverage explicitly includes both.
- **Math (KaTeX) inside Equation CustomNode**. Tagged equations
  (`\tag{N}` from `CrossrefIndex`) need to round-trip cleanly
  through KaTeX. Browser smoke test is the safety net.
- **Drift between Rust's HTML output and our React rendering**.
  Real but bounded. We commit to class-compatible (same class
  names), not DOM-equivalent. Where DOM differs, CSS may need
  adjustment. Catch via visual inspection during M2; formalize
  a contract test in a future plan if it becomes a maintenance
  burden.
- **`__quarto_custom_node` class polluting rendered DOM after
  user override**. Resolved by design: unwrap is the single
  forward-path conversion, runs before any registry dispatch.
  The `Div` registry slot only sees real Divs. A user-supplied
  `Div` override never sees the wrapper class.
- **Class-taxonomy enumeration completeness**. 2B's first
  implementation commit enumerates classes from the named Rust
  source files. Missing one means a styling gap. Mitigation:
  cross-check the enumerated list against actual q2-preview demo
  renders; visual differences from the HTML format flag any
  missing classes.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `html.tsx` Pandoc base-type gap fills (3 blocks + 8 inlines) | ~150 |
| `custom.tsx` components (7 type-specific) | ~360 |
| `types/customNode.ts` (CustomBlockNode / CustomInlineNode + Slot) | ~60 |
| `utils/customNode.ts` (unwrap + rewrap walks) | ~160 |
| Block / Inline dispatcher components (introduction + atomic-aware gate) + tests | ~50 |
| Shared utilities (formatRefLabel, composeAttr, renderSlot) | ~80 |
| `utils/quartoClasses.ts` (class-name constants) | ~80 |
| Tests (round-trip, component snapshots, atomic, Derived) | ~250 |
| **Total** | **~1190** |

Larger than the original Plan 2's estimate for the same
components-only slice (~970 LOC) because the Pandoc base-type
research uncovered ~100 LOC of additional gap fills, and the
unwrap / rewrap walks are now specified at full depth (~160 LOC
vs the original ~100 LOC plumbing item). A focused implementation
session with TDD discipline — round-trip tests written before the
walks — should fit comfortably.

Risk: the `Table` family is the highest-effort single component
(~80 LOC including the cell / row / colspec / caption supporting
shapes). Budget extra time for it.

## Notes

- This plan replaces the components half of the original Plan 2
  (`2026-05-04-q2-preview-plan-2-builtin-components.md`), which
  was split into 2A (foundation) + 2B (components) during the
  2026-05-06 review session.
- Following the user's lead: this is a *draft* that will eventually
  become a system component shipped as a Quarto extension. The
  plans treat it as such — design conventions matter (two
  registries, class taxonomy), but bundling / distribution
  mechanics don't need solving here.
- The "rename `MaybeReadOnlyInline`" question from the original
  Plan 2 is resolved here: there's no separate wrapper component;
  atomic-aware `setLocalAst` gating folds into the existing
  `Block` / `Inline` dispatchers. Plan 2A ships the prerequisites
  (`isAtomicSourceInfo`, `atomicCustomNodes.ts`); 2B ships the
  consumers.
- `html.tsx` and `custom.tsx` don't import each other.
  Cross-cutting concerns (the unwrap / rewrap plumbing in
  `utils/customNode.ts`, class-name constants in
  `utils/quartoClasses.ts`) live in their own modules that both
  reference.

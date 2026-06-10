# Block editing — Plan 2a: SourceInfo-value index + structural editability gate

**Date:** 2026-06-08
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Spec:** `claude-notes/designs/2026-06-06-block-editing-design.md`
**Phase:** 2a (core substrate). `sourceIndex` in `PreviewContext`; `untransformedAstJson` plumbing.
**Depends on:** Plan 1 (the `content`/`untransformedAst` plumbing + pool).

**No Rust changes.** Plan 2a is frontend only.

## Overview

Make the **untransformed AST a first-class input to q2-preview's editing logic**, so
every editable block can resolve its `sourceNode` (the untransformed counterpart) and
`reachabilityClass` (structural position in the untransformed tree) without touching the
shared framework or other format renderers. The correspondence is built once per render
into `PreviewContext.sourceIndex` and consumed by q2-preview's own leaf components. This
**replaces** the earlier opt-out `InsideContainerContext` / closed-container-audit gate
with a direct structural check.

**Win:** any q2-preview block determines editability from a single map lookup on its pool
entry; q2-debug and q2-slides are completely unaffected.

## Why this exists (and why it replaces the old gate)

A node's editability is "can the commit path reach it in the untransformed AST?"
The old design *inferred* that by pushing an `InsideContainerContext` flag down
the **transformed** tree (with an opt-out default + a per-container audit to avoid
a dangerous false-positive). Shipping the untransformed AST to the iframe lets us
read the answer **directly** from the indexed position, so the entire
context/opt-out/audit subsystem is unnecessary. The three old gate conjuncts
collapse:

- `t==0` (Original) ⟺ **present in `sourceIndex`** (a non-Original/Generated node's
  `s` value matches nothing in the untransformed index).
- `d==0` (active file) ⟺ also **present in `sourceIndex`** (an included-file node won't
  match the active-file untransformed AST).
- `!insideContainer` (reachability) ⟺ **`reachabilityClass`** stored in the index entry
  (top-level? inside a descendable container? inside an opaque one?).

## Correspondence model (precise)

- Ship `untransformedAstJson` into the iframe on `UPDATE_AST`, **in lockstep**
  with `astJson` + `renderedContent` (same compound state — it already exists on
  the parent post-Plan-1; forward it so it can never skew vs. the pool).
- Build a **SourceInfo-value index** in q2-preview's `PreviewContext`:
  compact-serialized SourceInfo value → `{ sourceNode, reachabilityClass }`.
  Key format: `"${t}:${r[0]}-${r[1]}:${d}"` (e.g. `"0:42-87:0"`) — deterministic,
  no JSON parse overhead, unambiguous. Keying is by value (not pool id) because
  the two trees have separate pools but share SourceInfo values.
- `PreviewContext` gains `sourceIndex?: Map<string, SourceIndexEntry> | null` where:
  ```typescript
  type ReachabilityClass = 'TopLevel' | 'Descendable' | 'Opaque';
  type SourceIndexEntry = { sourceNode: PandocBlock; reachabilityClass: ReachabilityClass };
  ```
  Built once per render via `useMemo(buildSourceIndex, [untransformedAstJson])` in
  `PreviewRoot`. Absence from the map — block is Generated, Substring, Concat, or
  from an included file — means "not source-backed."
- q2-preview components (Para, Header, …) call `ctx.resolveSource(args.node)` to get
  a `ResolvedSource | null` in one step — no manual pool lookup or serialization at
  the call site. **`NodeArgs` (`framework/types.ts`) is unchanged** — the shared
  framework type carries no q2-preview-specific state.
  - **`resolved.sourceNode`** is the block object from the **untransformed AST**
    (stored in the index by `buildSourceIndex`). This is the canonical object for
    edits — unexpanded, shortcodes/refs intact.
  - **`resolved.sourceEntry`** is the SourceInfo metadata (byte offsets) taken from
    the **transformed pool** (`pool[node.s]`) as a convenience (it is already in
    hand). The value is identical to the untransformed pool's entry by the
    value-equality invariant; `apply_node_edit` finds the splice point by value, not
    by pool id, so the source is immaterial.
- `reachabilityClass` is assigned during index construction by walking the untransformed
  AST with an inherited context (absorbing: once inside an Opaque container all
  descendants are Opaque):
  - **`TopLevel`** — direct child of the document's top-level block list.
  - **`Descendable`** — nested only within `Div`(non-section)/`BlockQuote`/
    `BulletList`/`OrderedList`/`DefinitionList`/`Figure`-body.
  - **`Opaque`** — reached by crossing a `Table` cell/caption or `Figure` caption.
    Absorbing. (Authoritative container-shape and boundary details are in Plan 3's
    "Limitations / container shapes" table.)

**`sourceNode` is always a plain primitive** (`Div`/`Para`/list/…), **never a
CustomNode** — Callout/Theorem/Proof/FloatRefTarget are *transform* products and
do not exist in the untransformed AST (verified 2026-06-08: `::: {.callout-note}`
parses to `Div.callout-note`, no `type_name`).

q2-debug and q2-slides render through their own registries and leaf components;
they never receive `untransformedAstJson` and never consult `sourceIndex`. No
changes to those formats or to the shared framework are needed.

## Structural editability gate

```
editable(node) = ctx.resolveSource(node)?.reachabilityClass ∈ allowed(plan)
```

`null` from `resolveSource` (Generated/included-file/non-Original) means not editable.

- Plan 2a/2b: `allowed = { TopLevel }`. Plan 3: `{ TopLevel, Descendable }`.
  (`Opaque` never editable in this series.)
- `editableType` (the block-type filter: Para/Header/CodeBlock/…) is a Plan 2b
  concept and is **not** part of Plan 2a's gate. Plan 2a's gate admits any block
  that is source-backed and TopLevel.

## TDD work items (tests first)

### Tests
- [x] **Index construction:** given an untransformed AST, `buildSourceIndex` produces
  a map with the correct `{ sourceNode, reachabilityClass }` entries:
  - An Original top-level Para → key `"0:<s>-<e>:0"`, `reachabilityClass: 'TopLevel'`.
  - A block moved by a transform still resolves via value match (not position).
  - A sectionize `Div` (Generated `s`) → absent from the index.
  - An included-file block (`d≠0`) → absent from the index.
- [x] **reachabilityClass:** top-level block → `TopLevel`; a Para inside a fenced
  `Div`/`BlockQuote`/list → `Descendable`; a Para inside a `Table` cell →
  `Opaque`; a Para inside a callout (untransformed `Div.callout-*`) →
  `Descendable` (it's a Div, not opaque). Add a Figure fixture cross-referencing
  Plan 3's container shapes: Figure body → `Descendable`, Figure caption →
  `Opaque`.
- [x] **Gate predicate (pure):** `editable` true only for `sourceIndex entry present
  + TopLevel`; false for absent entry, `Descendable`, `Opaque`. (No block-type
  filter in Plan 2a — that is Plan 2b's `editableType`.)
- [x] **Refactor Plan 1's gate:** Para/Header editability now reads
  `ctx.resolveSource(args.node)?.reachabilityClass === 'TopLevel'`
  instead of the interim `t==0 && d==0`; slicing range uses `resolved.sourceEntry.r`;
  existing Plan 1 editing tests stay green.

### Implementation
- [x] **`untransformedAstJson` forwarding chain** — add to payload and thread through:
  - `hub-client/src/components/render/ReactPreview.tsx` — `rendered.untransformedAstJson`
    is already in compound state; pass it to `ReactRenderer` as a new prop.
  - `hub-client/src/components/render/ReactRenderer.tsx` — accept
    `untransformedAstJson?: string | null`, forward to `Q2PreviewIframe`.
  - `ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx` — add
    `untransformedAstJson?: string | null` to `Q2PreviewIframeProps` and
    `UpdateAstPayload`; include in the `UPDATE_AST` postMessage alongside `astJson`
    and `renderedContent` (all three in the same compound-state lockstep).
  - `q2-preview-spa/src/PreviewApp.tsx` — `state.untransformedAstJson` is already in
    compound state; pass it to `Q2PreviewIframe` directly (no intermediate component).
- [x] **`ts-packages/preview-renderer/src/q2-preview/entry.tsx`** — add
  `untransformedAstJson?: string` to `UpdateAstPayload`; `PreviewRoot` builds
  `sourceIndex` via `useMemo(buildSourceIndex, [untransformedAstJson])` and supplies
  it through `PreviewContext.Provider`.
- [x] **`ts-packages/preview-renderer/src/q2-preview/PreviewContext.tsx`** — add:
  - `sourceIndex?: Map<string, SourceIndexEntry> | null`
  - `resolveSource?: (node: BlockNode) => ResolvedSource | null`
  Types defined in co-located `sourceIndex.ts`.
  `resolveSource` is provided by `PreviewRoot` via `useCallback([pool, sourceIndex])`.
- [x] **`buildSourceIndex` utility** — walks the untransformed AST, assigns
  `reachabilityClass` during traversal (absorbing: Opaque propagates to all
  descendants), keys entries by `serializeSourceEntry(sourceEntry)` =
  `"${t}:${r[0]}-${r[1]}:${d}"`.
- [x] **`ts-packages/preview-renderer/src/q2-preview/blocks/Para.tsx`, `Header.tsx`**
  — replace manual pool lookup + `t===0 && d===0` gate with a single
  `const resolved = ctx.resolveSource(args.node)` call; gate becomes
  `resolved?.reachabilityClass === 'TopLevel'`; slicing range becomes
  `resolved.sourceEntry.r[0]` / `resolved.sourceEntry.r[1]`.
- [x] **Delete** the planned `InsideContainerContext` / opt-out / container-audit
  machinery — superseded (it was never built; this records that it is not to be).

## End-to-end verification
- [x] `npm run build:wasm` + dev server: in a doc with a heading, a paragraph,
  a fenced div containing a paragraph, and a table, confirm via console that
  `sourceIndex` entries resolve as expected, and that Plan 1's Para/Header editing
  still works through the refactored gate.
- [x] `npm run build:all` — verified 2026-06-08, build succeeds cleanly.

## Risks / watch-items
- **Value-equality invariant:** correspondence relies on a transformed Original
  node's SourceInfo value exactly equaling its untransformed counterpart's. If a
  transform re-spans a node's source info, the match fails → absent from
  `sourceIndex` → that node reads as non-editable (safe degradation, never a
  wrong commit).
- **Payload size / index cost:** the untransformed AST is shipped each render and
  indexed O(n); keep it in the compound state so it can't skew vs. the pool.

## References
- Spec "Editability gate — the structural (dual-node) model", "Format routing".
- Plan 3 "Limitations / container shapes" table — authoritative for Figure body vs.
  caption and Table cell/caption Opaque classification.
- `node_lookup.rs` (the backend analogue of the value match — **not changed in
  Plan 2a**; Pass-2 removal is Plan 2b).
- `ts-packages/preview-renderer/src/q2-preview/{entry,PreviewContext,blocks/}`.
- `ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx`.
- `hub-client/src/components/render/{ReactPreview,ReactRenderer}.tsx`.
- `q2-preview-spa/src/PreviewApp.tsx`.

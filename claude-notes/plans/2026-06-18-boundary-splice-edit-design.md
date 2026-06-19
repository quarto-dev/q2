# Boundary-addressed splice — generalizing `apply_node_edit` to insert / range

**Date:** 2026-06-18 (updated 2026-06-19)
**Branch:** `feature/block-editing-improvements` (worktree `.worktrees/block-editing`)
**Status:** DESIGN — approved. Implementation plan:
`claude-notes/plans/2026-06-19-boundary-splice-implementation.md`.

**2026-06-19 corrections (after backend + frontend research):**
- `ContainerRef = DocRoot | Node(si)` only — dropped `listItem`/`defBody` and all
  positional container indices. Everything item-shaped moved to the deferred
  **item plane** (`2026-06-19-item-plane-research.md`).
- API is a **clean break**, not additive: `usePreviewEdit` → `{ resolveSource,
  commit }`; the three demo components get updated.
- `setAst` is a **direct React prop callback**, not postMessage.
- Backend research confirmed: `compute_reconciliation` + `incremental_write` are
  diff-driven, so insert/range already work (top-level: new-block `Rewrite` +
  separator; nested: whole-container `Rewrite`). No writer surgery.

## Overview

`apply_node_edit` today does exactly one thing: **replace one block with 0..N
blocks**. We want the same machinery to also express:

- **insert** at a position (0 → N blocks), including *at the end of a
  container* and *into an empty container* — without having to find a node to
  anchor against;
- **range replace / `setAstRange`** (M → N blocks) over a contiguous run of
  sibling blocks.

The backend already routes every edit through
`compute_reconciliation(&A_u, &A_u') → incremental_write`, which diffs two whole
ASTs and does not care whether `A_u'` came from a replace, an insert, or a range
edit. **So generalization is purely an addressing problem**, not a change to the
reconcile/write machinery and not a change to the backend's data layer (it stays
AST-only; markdown is parsed at the edge exactly as it is today).

This document specifies a **boundary-addressed splice**: every edit becomes one
primitive — `splice(parent_blocks, from..to, replacement)` — where `from`/`to`
are *boundaries* (gaps between blocks), and the verb the caller uses is sugar
that lowers to a `{ from, to, content }` triple.

## The unifying primitive

All operations are one splice over a half-open gap range of one container's
child `Blocks` slice:

| Operation | `from .. to` | `replacement` |
|---|---|---|
| replace-1 (today) | one block's two boundaries | N blocks |
| insert (0→N) | a single gap (`from == to`) | N blocks |
| range (M→N) | first..last span | N blocks |
| delete | any span | `[]` |

`from == to` ⇒ insert; `from < to` ⇒ replace/delete.

## `Boundary` vs `SourceInfo` (the core distinction)

- **`SourceInfo` names an existing *node*** — a byte range into the source; every
  AST node has exactly one. N blocks → N source infos. Answers *"which block?"*.
  This is all `apply_node_edit` ever needed, because replace-1 only points at the
  block it replaces. It is matched **by exact byte-range value** against `A_u`,
  which is what makes targeting robust to a stale `A_u`.

- **`Boundary` names a *position* — a gap.** A child list of N blocks has **N+1**
  gaps (before each block, plus the end). Answers *"which slot?"*. A position is
  not always a node — "after the last block", "top of the document", "into an
  empty div" have no node to name — so `SourceInfo` alone cannot address them.
  `Boundary` is the thin layer that can.

`Boundary` is the richer concept; `SourceInfo` is one of its ingredients. The two
node-relative boundary kinds keep `SourceInfo`'s exact-match robustness;
`startOf`/`endOf` resolve to gap `0` / `len` of a resolved container — no index
arithmetic anywhere (that was the rejected "anchor + delta" alternative; see
Non-goals).

Document `[A, B, C]` with gaps `g0 A g1 B g2 C g3`:

```
        g0     A     g1     B     g2     C     g3
SourceInfo points at  ↑A          ↑B          ↑C        (the boxes)
Boundary   points at g0    g1    g2    g3                (the slots)
```

- `g0` = `startOf(doc)` (= `beforeNode(A)`, but you need not name A)
- `g2` = `afterNode(B)` = `beforeNode(C)`
- `g3` = `endOf(doc)` — the **insert-at-end** case, named without touching C

## Types

### Component-facing (inside the iframe)

```ts
// Replacement content — TWO FLAVORS, first-class and orthogonal to the verb.
// This is the explicit form of today's PreviewNodeEditPayload `channel`.
type Content =
  | { kind: 'markdown'; text: string }     // built-in editors send this
  | { kind: 'ast'; blocks: BlockNode[] };   // render components send this

const md  = (text: string): Content => ({ kind: 'markdown', text });
const ast = (...blocks: BlockNode[]): Content => ({ kind: 'ast', blocks });
const EMPTY: Content = ast();               // zero blocks → delete (kind irrelevant)

// Container identity for the container-relative boundaries.
// v1: SINGLE-SLICE containers only — DocRoot or a container with exactly one
// child Blocks slice (Div / BlockQuote / Figure). No positional index anywhere.
// Lists / def-lists / tables are NOT startOf/endOf targets (multiple item-slices,
// no per-slice SI); their interiors are reached only via node-relative boundaries
// (before/after a contained block's SI, which works at any nesting depth).
// Anything needing a positional slot among sibling item-slices is the ITEM PLANE
// (deferred — see Non-goals + 2026-06-19-item-plane-research.md).
type ContainerRef =
  | { kind: 'docRoot' }
  | { kind: 'node'; si: SourceInfoJson };                       // div / blockquote / figure

type Boundary =
  | { kind: 'beforeNode'; si: SourceInfoJson }   // gap built FROM a node's SourceInfo
  | { kind: 'afterNode';  si: SourceInfoJson }
  | { kind: 'startOf'; container: ContainerRef } // gap at a container edge — no node
  | { kind: 'endOf';   container: ContainerRef };
```

### Wire payload (iframe → parent, the flavor rides here)

```ts
interface Splice {
  from: Boundary;
  to: Boundary;
  content: Content;     // normalized at the parent (md → parse, ast → serialize)
}
```

`SourceInfoJson` is exactly today's value: `JSON.stringify(resolveSource(node).sourceEntry)`.

## Action vocabulary — FULL lowering

Every verb is a pure descriptor-builder (no WASM, no tree) that returns a
`Splice`. `Content` is a parameter of every verb, so *which action* and *which
flavor* are chosen independently.

```ts
// internal boundary helpers (NOT public — verbs are the surface)
const before = (si): Boundary => ({ kind: 'beforeNode', si });
const after  = (si): Boundary => ({ kind: 'afterNode',  si });
const startOf = (c): Boundary => ({ kind: 'startOf', container: c });
const endOf   = (c): Boundary => ({ kind: 'endOf',   container: c });

// ── replace one block (M=1 → N) ─────────────────────────────────────
replaceNode(si, content)
  → { from: before(si), to: after(si), content }

// ── insert relative to a block (0 → N) ──────────────────────────────
insertAfter(si, content)
  → { from: after(si),  to: after(si),  content }      // from == to
insertBefore(si, content)
  → { from: before(si), to: before(si), content }      // from == to

// ── replace a sibling range (M → N) ─────────────────────────────────
replaceRange(firstSi, lastSi, content)
  → { from: before(firstSi), to: after(lastSi), content }
//   first & last must resolve to siblings in the SAME container; from ≤ to

// ── append / prepend to the document ────────────────────────────────
appendToDoc(content)
  → { from: endOf({ kind: 'docRoot' }),   to: endOf({ kind: 'docRoot' }),   content }
prependToDoc(content)
  → { from: startOf({ kind: 'docRoot' }), to: startOf({ kind: 'docRoot' }), content }

// ── append / prepend into a container (incl. an EMPTY one) ──────────
appendTo(container, content)
  → { from: endOf(container),   to: endOf(container),   content }
prependTo(container, content)
  → { from: startOf(container), to: startOf(container), content }
//   container = { kind:'docRoot' } | { kind:'node', si }   (div / blockquote / figure)
//   insert into an EMPTY div: appendTo({kind:'node', si}, …)
//     → from == to == gap 0; identical to every other insert, NO special case
//   append to the body of a NON-EMPTY list item: anchor on its last child block —
//     insertAfter(siOfLastChildBlock, …). The EMPTY list-item body (no anchor) and
//     "add a new item at position N" are ITEM PLANE (deferred).

// ── deletes are empty-content spans ─────────────────────────────────
deleteNode(si)
  → replaceNode(si, EMPTY)              // { from: before(si),    to: after(si) }
deleteRange(firstSi, lastSi)
  → replaceRange(firstSi, lastSi, EMPTY) // { from: before(first), to: after(last) }
```

### Worked example — document `[A, B, C]`, gaps `g0 A g1 B g2 C g3`

| Call | `from` | `to` | resolves to | effect |
|---|---|---|---|---|
| `replaceNode(B, …)` | `before(B)` | `after(B)` | `g1 .. g2` | B → N blocks |
| `insertAfter(B, …)` | `after(B)` | `after(B)` | `g2 .. g2` | insert at g2 |
| `insertBefore(B, …)` | `before(B)` | `before(B)` | `g1 .. g1` | insert at g1 |
| `replaceRange(A, B, …)` | `before(A)` | `after(B)` | `g0 .. g2` | A,B → N blocks |
| `appendToDoc(…)` | `endOf(docRoot)` | `endOf(docRoot)` | `g3 .. g3` | insert at end |
| `prependToDoc(…)` | `startOf(docRoot)` | `startOf(docRoot)` | `g0 .. g0` | insert at top |
| `deleteNode(C)` | `before(C)` | `after(C)` | `g2 .. g3` | remove C |
| `deleteRange(A, B)` | `before(A)` | `after(B)` | `g0 .. g2` | remove A,B |

Usage — both flavors, any action:

```ts
// render component — AST-native:
commit(replaceNode(siOf(node), ast(newNode)));
commit(insertAfter(siOf(card), ast(newCard)));
commit(replaceRange(siOf(first), siOf(last), ast(merged)));

// built-in editor — markdown-native:
commit(replaceNode(si, md('**bold** text')));
commit(insertAfter(si, md('## New section')));
commit(appendToDoc(md('## Appendix')));
```

## Three levels (where each piece lives)

1. **Component-facing API — inside the iframe.** Render components and built-in
   editors call `usePreviewEdit()`. The verbs + `Content` + `commit` live here.
   `resolveSource(node) → si` is here.
2. **Component → parent payload.** The verbs lower to a `Splice` passed to the
   parent via the existing `setAst` **prop callback** (a direct React call in the
   same JS context — NOT postMessage; today it's cast `as unknown as PandocAST`).
   So the `Splice` need not be serialization-safe; we give the prop a real type
   instead of the cast. Generalizes today's `PreviewNodeEditPayload`; the md/ast
   flavor rides along (it is the `channel` discriminator made first-class).
3. **Parent handler — `hub-client/src/components/render/ReactPreview.tsx`
   (`handleSetAst`, ~lines 653–718).** Receives the `Splice`, **normalizes** the
   flavor (markdown → `parseQmdContentSync`; ast → serialize) — exactly where
   parsing lives today — then calls WASM `apply_node_splice` and routes the result
   through `onContentRewrite(newQmd)` (→ VFS / Automerge). **The backend stays
   AST-only; markdown→AST conversion does not move.**

## Backend: resolver + `splice_range`

New `crates/pampa/src/apply_node_splice.rs`:

```rust
pub fn apply_node_splice(content: &str, untransformed_ast_json: &str, splice_json: &str)
    -> Result<String, ApplyNodeEditError>
```

Steps (mirrors `apply_node_edit`, generalizing steps 2–5):

1. Deserialize `A_u` (unchanged).
2. Deserialize the `Splice` (`from`, `to`, and the replacement blocks — the
   parent has already normalized `Content` to Pandoc JSON blocks; the wire to
   WASM carries `replacement`, not `Content`).
3. **Resolve each boundary** → `(steps, gap_idx)`:
   - `beforeNode(si)` / `afterNode(si)`: `lookup_block(&A_u, si)` → `NodePath
     { steps, leaf_idx }`; gap = `leaf_idx` (before) or `leaf_idx + 1` (after).
   - `startOf(container)` / `endOf(container)`: resolve `ContainerRef` to the
     `steps` that land on a child `Blocks` slice — `DocRoot → []`; `Node(si)` →
     the container's `lookup_block` path + a single `ContainerStep::Blocks` descent
     into `.content` (Div/BlockQuote/Figure each have exactly one such slice);
     gap = `0` (start) or `slice.len()` (end). No list/def container resolver in v1.
4. **Invariants** (else degrade — see below): `from.steps == to.steps` (same
   container — enforces siblings-only) and `from.gap_idx <= to.gap_idx`.
5. **Splice**: navigate `steps` to the parent `Blocks` slice (reuse the existing
   `splice_in_blocks` descent), then `slice.splice(from_gap..to_gap, replacement)`.
6. `compute_reconciliation` + `incremental_write` (unchanged).

`splice_range(root, steps, from_gap..to_gap, replacement)` replaces both
`splice_at_path` and the hard-coded `leaf_idx..=leaf_idx`. `apply_node_edit`
becomes a one-line shim: `apply_node_splice` of `{ before(si), after(si),
replacement }`.

**`preserve_leaf_variant` carries over, narrowed.** The Plain↔Para coercion
(tight-list fidelity) applies only when the span is **exactly one block**
(`to_gap - from_gap == 1`) and that block was `Plain` and the replacement is a
single `Paragraph`. For inserts (`from == to`, no original leaf) and for
multi-block spans it must NOT fire — same `len() == 1` guard as today, plus the
new single-block-span guard.

## Stale-AST degrade rules

Today, a missing target returns the original `content` unchanged (graceful
stale-AST race; `apply_node_edit.rs` ~line 142). Generalize uniformly: if **any**
boundary fails to resolve (node `si` not found in `A_u`; `ContainerRef` node not
found; list/def coordinate out of range), or an invariant fails (different
containers, `from > to`), return `content` unchanged and log to stderr. No
partial edits.

## Migration (CLEAN BREAK — the render-component API is ours to shape)

The `usePreviewEdit` surface is a small experiment with exactly three consumers
(all ours), so we replace — not wrap — the old functions. Decided 2026-06-19.

- `apply_node_edit` (Rust + WASM) → thin shim over `apply_node_splice`, then
  retire once callers move.
- `usePreviewEdit()` returns `{ resolveSource, commit }`. The named
  `commitTextEdit` / `commitSubtreeEdit` are **removed**, not kept as wrappers.
- The three demo render components are updated to the new verb API:
  - `~/docs/demo-playground/gordon/render-components2/drag.tsx`,
    `kanban.tsx`, `comment.tsx` — each `commitSubtreeEdit(si, modified)` →
    `commit(replaceNode(si, ast(modified)))`. (These live outside the repo; their
    hub-client e2e specs — `q2-preview-render-components-{drag,kanban,comment}.spec.ts`
    — depend on the updated versions.)
- Old call sites inside the repo: `EditTextarea` (`dispatchers.tsx`) and
  delete-by-emptying → `commit(replaceNode(si, md(text)))` /
  `commit(deleteNode(si))`.

## Non-goals (explicit — do not "discover" and wire these up)

- **The ITEM PLANE** — inserting / moving / deleting *items* (list items, table
  rows, def-list entries), i.e. splices on a list's `Vec<Vec<Block>>` rather than
  on a `Blocks` slice. Different element type, different content shape (an item's
  payload is "the blocks that make up the item"), and the only thing that wants a
  *positional slot among sibling item-slices*. This is why v1's `ContainerRef` is
  `DocRoot | Node(si)` with no `listItem`/`defBody` and no positional index:
  everything item-shaped — including filling an empty list item — belongs here.
  Deferred to a separate research-level plan:
  **`claude-notes/plans/2026-06-19-item-plane-research.md`**. The split is additive
  — the item plane adds its own boundary family later without changing v1.
- **`replaceWholeDoc`** (`startOf(docRoot)..endOf(docRoot)`). The model *admits*
  it trivially, but it is the one case where splice/reconcile is the **wrong**
  tool: a total replacement has no surrounding source to preserve, so reconcile
  just burns work aligning two unrelated trees and likely yields a worse diff
  than writing the new qmd directly. Whole-document replacement already has its
  proper home at the framework level (`AstProps.setAst(newAst)` → fresh render).
- **Cross-container ranges** (a span whose endpoints have different parents). The
  same-container invariant rejects these; selections that spill out of a
  container are ill-defined for a sibling splice.
- **Backend `Replacement::Qmd`** (parsing markdown inside the splice backend).
  Markdown→AST stays at the parent edge where it already lives; the backend
  remains AST-only.
- **Batching** — a `commit(edits: Splice[])` that resolves all boundaries against
  the original `A_u` and applies back-to-front (descending gap order) before a
  single reconcile/write. A clean future extension (the verb vocabulary is
  unchanged); build it only when a compound gesture needs it.

## Work items

> The detailed, step-by-step, fail-on-revert-bound task breakdown lives in the
> implementation plan: **`claude-notes/plans/2026-06-19-boundary-splice-implementation.md`**.
> The summary below is the shape; the implementation plan is the contract.

### Phase 1 — backend (TDD)

- [ ] `splice_range` for insert (`from==to`), range (M→N), delete, replace-1
      (parity with `apply_node_edit`) — top level **and** nested (a block inside a
      div / blockquote / list item / def body, addressed by the block's own `si`;
      `lookup_block` already descends `ListItem`/`DefBody` steps).
- [ ] Boundary resolution — `beforeNode`/`afterNode` (node-relative) and
      `startOf`/`endOf` for `ContainerRef = DocRoot | Node(si)` only
      (append-to-doc, append-to-empty-div). No list/def container resolver in v1.
- [ ] Stale-AST degrade (unresolvable boundary, cross-container, `from > to`).
- [ ] `preserve_leaf_variant` fires for single-block Plain→Para replace, NOT for
      inserts or multi-block spans.
- [ ] Implement `apply_node_splice` + `splice_range`; make `apply_node_edit` a shim.
- [ ] WASM export `apply_node_splice`; keep `apply_node_edit` export during migration.

### Phase 2 — client (TDD)

- [ ] Unit-test the verb builders (pure; assert exact `{ from, to, content }`).
- [ ] `Content` (`md`/`ast`), `Boundary`, `ContainerRef`, `Splice` wire types.
- [ ] Parent (`ReactPreview.tsx`): normalize `Content` → blocks, call
      `apply_node_splice`; type the `setAst` prop properly.
- [ ] `usePreviewEdit` → `{ resolveSource, commit }`; **remove**
      `commitTextEdit`/`commitSubtreeEdit`; port `EditTextarea` + delete-by-emptying.
- [ ] Update the three demo components + re-run their e2e specs (back-compat-free).
- [ ] End-to-end: exercise insert + range through a harness driving the new
      `commit` path; inspect the resulting qmd diff (minimal, as for replace today).
```

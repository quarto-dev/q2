# Block editing — Plan 3: nested-block descent

**Date:** 2026-06-06
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Spec:** `claude-notes/designs/2026-06-06-block-editing-design.md`
**Phase:** 3. Rust (`pampa`) + frontend: `Block`/`CustomBlock` dispatchers own textarea substitution; affordance extended to all source-backed block components.
**Depends on:** Plans 1, 2a, 2b.

> **Review-applied (2026-06-09, session 1).** This plan was reviewed against the
> canonical tree at `2ecf09c4` and corrected: DefinitionList path arity (C1), a
> null-safety bug in the gate flip (C2), a stale marker-fidelity watch-item (C3),
> the recursion/descent spec (U2), the `NodePath` shape (U3), separator-fidelity
> wording (OQ2), nested-`Table` acceptance (OQ3), and three contract changes that
> landed after Plan 2b (lenient subtree reader, `s`+`a` stripping, deprecated
> `SourceInfo::default()`).
>
> **Review-applied (2026-06-09, session 2).** Corrected: `ContainerStep` variants
> were missing the container-block index in the current `Blocks` level — `Blocks`
> → `Blocks(usize)`, `ListItem(usize)` → `ListItem(usize, usize)`,
> `DefBody(usize, usize)` → `DefBody(usize, usize, usize)`; added
> `NodePath::top_level(i)` convenience ctor; updated C1 coordinate count (four,
> not three). OQ4 decided: N→0 splice is valid, test it. Frontend scope expanded:
> gate centralized into two hooks (`useBlockAffordance`, `useBlockEditor`); overlay
> extended to all 14 block components (not just Para/Header); full affected-file
> list corrected (BlockQuote, DefinitionList, CodeBlock, RawBlock, Table were
> missing); custom components (Callout/Theorem/Proof/FloatRefTarget) get affordance
> update only — whole-custom-block textarea is Plan 4. Callout fixture test must
> use full WASM pipeline.
>
> **Review-applied (2026-06-09, session 3).** Major frontend architecture
> correction: drop `useBlockAffordance`/`useBlockEditor` hooks; instead centralize
> textarea substitution in the `Block`/`CustomBlock` dispatchers
> (`q2-preview/dispatchers.tsx`). `useEditableBlock.tsx` is deleted; its logic
> moves into the dispatchers with the widened C2-safe gate. `data-block-pool-id`
> stays on individual semantic elements (no wrapper div — wrapper divs in the
> inactive view interfere with Bootstrap/Quarto `>` child-combinator CSS).
> Dispatcher centralization means Figure, LineBlock, and the four custom components
> (Callout/Theorem/Proof/FloatRefTarget) get full textarea editing for free: they
> only need `data-block-pool-id` on their outer element (one line each). The "Plan
> 4 deferral" for custom-component editing was wrong — it is identical to Div
> editing and belongs here. Callout fixture test corrected: `CalloutTransform` uses
> `std::mem::take` preserving inner `source_info` fields, so the backend test is a
> plain nested-Div fixture; no `quarto_core` dependency. `NoteDefinitionFencedBlock`
> exclusion documented. `resolve_to_blocks` error handling: `unreachable!`. Nested
> test helpers: purpose-specific per container shape. Plain.tsx added as stretch
> item.

## Overview

Let edits reach blocks **nested inside descend-able containers** (fenced `:::`
divs, list items, block quotes, definition-list bodies, figure bodies, **and
callout/theorem/proof bodies** — those are plain `Div`s in the untransformed AST).
The frontend payload is **unchanged** — a nested block carries its own `Original`
`source_info` — so the core Rust work is: `lookup_block` must **recurse** and
return a *path*, and `apply_node_edit` must **splice at that path**.

The frontend work centralizes textarea substitution into the `Block` and
`CustomBlock` dispatchers (`q2-preview/dispatchers.tsx`), which already wrap every
block render. The gate widens from `allowed = {TopLevel}` to `{TopLevel,
Descendable}` with a null-safe guard (see C2). Plan 2a already stores
`reachabilityClass` in `PreviewContext.sourceIndex` for every block, so the
expanded gate is all that is needed to unlock nested editing. `Opaque` regions
(`Table` cells/captions) stay gated off — a **selective** admission, **not**
"drop a gate." `data-block-pool-id` stays on individual semantic elements (no
wrapper div added by the dispatcher — a dispatcher-level wrapper would sit in the
inactive rendered view and break Bootstrap/Quarto CSS `>` child-combinator rules).

**Win:** edit a paragraph inside a fenced div / list item / block quote / callout.

**Note on whole-container editing.** A top-level container is editable *as a
whole* in Plan 2 (e.g. to change a fenced div's attrs). Once Plan 3 makes its
contents editable, the deepest-target hover resolves to the inner block, so the
whole-container affordance is **shadowed**. Reaching the container as a single
unit is the Plan 4 background-vs-text geometry; accepted.

## Investigation result (pre-coding)

**Finding:** the writer always wholesales block containers.

`coarsen` in `incremental.rs` (the "Phase 5" strategy) handles
`BlockAlignment::RecurseIntoContainer` by checking for an inline plan.
`InlineSplice` is only produced for inline-content blocks (Paragraph, Header).
For every block container — `Div`, `BlockQuote`, `BulletList`, `OrderedList`,
`DefinitionList` — no inline plan exists, so the code falls through to
`CoarsenedEntry::Rewrite` (`incremental.rs:~217-222`). The writer re-serializes
the **entire** container. (Confirmed at the reconcile level too:
`compute_reconciliation_for_blocks` gives a container with changed children a
`RecurseIntoContainer` alignment — `compute.rs:152/174`.)

**Consequence:** editing a block nested in a list item renumbers/re-bullets
all sibling items; editing a block inside a `BlockQuote` reflows all `>` lines.
This is Tier-2 reformatting one level down. All "sibling fidelity" tests must
be snapshot-based — do **not** assert sibling byte-identity.

**Decision (settled with user):** wholesale rewrites are acceptable for v1.
The guarantee that holds: bytes **outside the enclosing container and its
boundary separators** are verbatim. (The qualifier matters — OQ2: the container
becomes a `Rewrite` entry, which carries no `orig_idx`, so `compute_separator`
falls back to the standard `"\n"` for the gaps *adjacent* to the container;
a two-blank-line gap next to it can collapse to one. This is the same Tier-2
behavior as editing a whole top-level container in Plan 2b, not new to Plan 3.)
A follow-up to make the writer recurse into containers (extending
`InlineSplice` to block-level) is possible but is not a v1 requirement.

## Container shapes (from `quarto-pandoc-types/src/block.rs`)
- `Div`, `BlockQuote`, `Figure` → single `Blocks` (`Vec<Block>`): step is
  `Blocks(i)` (container at index `i` in current level); leaf is `leaf_idx` within
  `container.content`. Two coordinates total.
- `BulletList`, `OrderedList` → `Vec<Blocks>`: step is `ListItem(i, item)` (list at
  index `i`, item at `item`); leaf is `leaf_idx` within the item's `Blocks`. Three
  coordinates total.
- `DefinitionList` → `Vec<(Inlines, Vec<Blocks>)>`: step is
  `DefBody(i, def, body)` (list at index `i`, term `def`, body `body`); leaf is
  `leaf_idx` within that body's `Blocks`. Four coordinates total. Term `Inlines`
  not descendable.

A **path** is a sequence of these steps from the top-level block list to the
target block.

**`NoteDefinitionFencedBlock` is excluded.** It also has `content: Blocks` but is
not a descendable container — consistent with `sourceIndex.ts` which has no
`NoteDefinitionFencedBlock` case in `descendBlock`. It falls out of the DFS
naturally: no `ContainerStep` variant covers it, so the recursive lookup never
enters it.

## Limitations — block containers Plan 3 does **not** descend into
Confirmed against `quarto-pandoc-types` (`block.rs`, `table.rs`, `caption.rs`).
These nested-`Blocks` regions stay editable only as a **whole top-level block**
(Plan 2, Tier-2 reformatting), never block-by-block inside:

- **Table cells** — `Cell.content: Blocks`, reached via
  `TableHead`/`TableBody`/`TableFoot` → `Row` → `Cell`. Deep, table-specific
  path steps; a table is edited whole (Plan 2).
- **Table caption** — `Table.caption.long: Blocks`.
- **Figure caption** — `Figure.caption.long: Blocks`. Plan 3 descends a figure's
  *body* (`Figure.content`) but **not** its caption blocks.

Definition lists **are** covered: a definition's *body* blocks
(`DefinitionList` → `DefBody(i, def, body)` + `leaf_idx` — four coordinates, per
the Container-shapes section; `content: Vec<(Inlines, Vec<Blocks>)>`) are
descendable.
What is *not* editable is the **term** (`Inlines`) — but that is the global "no inline editing"
limit (design Out-of-scope), not a Plan-3 container gap. Same for short captions
(`Caption.short: Inlines`) and `CaptionBlock` (Inlines): not editable because
they're inline, not because of nesting.

Cell-level / caption-block descent (extra `Table`/`Caption` path steps) is a
possible follow-up, explicitly out of this series.

**Accepted side-effect (OQ3): a `Table` *nested in* a descendable container becomes
editable-as-whole.** Only a table's *cells/captions* are `Opaque`; the `Table`
block itself, when nested in a `Div`/list/etc., classifies `Descendable`. So the
widened gate makes a nested table editable **as a whole** (Tier-2, like a
top-level table) while its cells stay uneditable. This is accepted behavior, not
a goal — there is no nested-table fixture, so it is untested-but-allowed.

**Callout / theorem / proof bodies ARE covered here.** Although they *render*
through `custom/` CustomNode components, in the **untransformed** AST they are
plain `Div`s (`Div.callout-note`, `Div.theorem`, …) — the CustomNodes are
transform products that don't exist pre-pipeline (verified 2026-06-08). So their
`sourceNode` is a descendable `Div`, their bodies are `Descendable`, and editing a
paragraph inside a callout is just nested-Div descent — no special writer support
(this is why the former Plan 5 dissolved). Add a callout fixture to the tests
below.

## TDD work items (tests first)

### Tests (`crates/pampa/tests/integration/node_edit_tests.rs`)
- [x] New fixtures: paragraph inside a fenced `Div`; inside a `BlockQuote`;
  inside a bullet-list item; inside an ordered-list item; a `Div` nested in a
  `Div`; **a paragraph inside a definition-list body** (exercises the
  four-coordinate `[list_idx, def_idx, body_idx, block_idx]` path — the arity most
  likely to be mis-coded); **a paragraph inside a `Figure` body** (Descendable)
  with the figure's **caption** confirmed *not* editable (Opaque); **a paragraph
  inside a callout-style Div** (a fenced `:::` with class `callout-note`). The
  callout test is a plain nested-Div test at the backend level — `CalloutTransform`
  uses `std::mem::take` to move inner blocks to slots, so `source_info` fields are
  preserved intact and no `quarto_core` pipeline is needed. The frontend concern
  (does the inner Para's `s` field survive the transform for sourceIndex lookup?)
  is already covered by the existing `sourceIndex.test.ts` test `'Para inside a
  callout (Div.callout-*) → Descendable'`.
- [x] **Add nested-target traversal helpers** (U4) in the test file for extracting
  `SourceInfo` from known fixture positions without re-using the top-level
  `block_idx` shortcut. One small helper per container shape is sufficient — e.g.
  `div_inner_block(ast, div_idx, inner_idx) -> &Block` and
  `list_item_block(ast, list_idx, item_idx, inner_idx) -> &Block`. Tests call
  `.source_info().clone()` on the result. Keep helpers explicit rather than
  building a generic path-spec API — fixture structures are known, named helpers
  are clearer.
- [x] **Migrate the existing flat-lookup tests** to the `Option<NodePath>` return
  type. Concrete: `node_edit_tests.rs` asserts `Some(0)`/`Some(1)` at the lookup
  tests (≈ lines 181, 190, 212–213) — these become
  `Some(NodePath::top_level(0))` / `Some(NodePath::top_level(1))`; the `edit_block`
  helper that indexes `a_u.blocks[block_idx]` remains correct for existing top-level
  fixtures.
- [x] `lookup_block` (recursive) returns the correct **path** for each; `None`
  for a target not present. Add a uniqueness assertion: a target that matches a
  *nested* block is not also matched by its enclosing container (distinct ranges).
- [x] `apply_node_edit` edits a nested block: the target is replaced and the
  edited block round-trips; a 1→N nested edit splices N blocks into the
  container. Bytes **outside the enclosing container and its boundary separators**
  are always verbatim.
- [x] **Attr-strip at depth (since-2b):** a subtree-channel edit of a nested
  *attr-bearing* block (e.g. a `Div` with classes inside a `Div`, or a callout
  body) must round-trip with `s` **and** `a` (AttrSourceInfo) stripped — confirm
  the lenient reader backfills and the writer re-emits cleanly.
- [x] **Deletion / N→0 (OQ4 — decided):** built-in editing cannot reach this
  (D10: empty commit = cancel). A subtree-channel edit with an empty replacement
  (`blocks: []`) calls `parent.splice(leaf_idx..=leaf_idx, vec![])`, removing the
  target block and leaving the container with n−1 blocks (possibly empty, emitting
  e.g. `:::\n:::\n`). This is valid output. Add a test that a nested Para replaced
  by an empty subtree produces the correct QMD with the container still present but
  one block shorter.
- [x] **Sibling fidelity.** The investigation confirmed the writer always
  wholesales containers, so there is only one test shape: assert that bytes
  **outside the enclosing container and its boundary separators** are verbatim,
  and **snapshot** the reformatted container body (sibling renumbering / bullet
  regeneration / `>` prefix reflow accepted). Do **not** assert sibling
  byte-identity — that would presume a recurse-into-container writer that does
  not exist.
- [x] Register the file in `tests/integration/main.rs` if not already.

### Implementation
- [x] Before touching `node_lookup.rs`, `grep` for all Rust callers of
  `lookup_block` (the return type changes from `Option<usize>` to
  `Option<NodePath>`) and list them so none are missed. **Verified @ 2ecf09c4 —
  the call set is small:** `crates/pampa/src/apply_node_edit.rs:143` (the only
  non-test caller) and `crates/pampa/tests/integration/node_edit_tests.rs` (lookup
  tests + the `edit_block` helper; the `Some(0)`/`Some(1)` asserts at ~181/190/
  212–213 — see the U4 test item). **No `quarto-core`/WASM callers**, and
  `apply_node_edit`'s WASM signature is unchanged (the path is internal). Re-grep
  to confirm nothing new landed.
- [x] `crates/pampa/src/node_lookup.rs` — recursive lookup returning a path.
  **`NodePath` shape (U3)** — a sequence of container steps plus a final leaf
  index, so the apply-side splice stays uniform:
  ```rust
  enum ContainerStep {
      Blocks(usize),               // root[i] is Div/BlockQuote/Figure → descend .content
      ListItem(usize, usize),      // root[i] is BulletList/OrderedList → .content[item]
      DefBody(usize, usize, usize),// root[i] is DefinitionList → .content[def].1[body]
  }
  struct NodePath { steps: Vec<ContainerStep>, leaf_idx: usize }
  // NodePath::top_level(i) == NodePath { steps: vec![], leaf_idx: i }
  ```
  Each variant's **first `usize`** is the index of the container block in the
  *current* `Blocks` level; without it `resolve_to_blocks` cannot know which block
  to enter. `NodePath::top_level(i)` covers the existing top-level case unchanged:
  `steps = []`, `leaf_idx = i`, `resolve_to_blocks(root, &[])` returns `root`
  directly. The `DefBody(list_idx, def_idx, body_idx)` three-tuple + `leaf_idx` is
  the four-coordinate DefinitionList path (C1); get this one right.
  **Search strategy (U2 — pin this down).** Distinguish the two uses of ranges:
  *matching* a node uses **equality only** (`source_info == target`) — keep the
  Pass-2-free rule Plan 2b established — while *deciding to descend* is a separate
  question the "exact-match only" phrasing does **not** answer. A container never
  equals a target nested inside it (parent range ⊋ child), so match-only logic
  alone can never reach a nested block. Resolve it consistently with "no
  `preimage_in` anywhere": do an **unconditional pre-order DFS** — visit each node,
  compare by equality, and recurse into **every** descendable container regardless
  of ranges (do *not* reintroduce containment/`preimage_in` to gate descent). The
  match is **unique** (distinct byte ranges per the
  `duplicate_blocks_have_distinct_source_info` invariant), so there is at most one
  hit in the whole tree; the smallest-pre-order-path tiebreak is dead-defensive and
  never fires in a well-formed source map. A `Generated` node is not editable and
  is rejected upstream, so it never reaches here.
- [x] `crates/pampa/src/apply_node_edit.rs` — navigate `A_u` to the parent
  `Blocks` via the path and `splice` the replacement there. Added
  `fn splice_at_path(root: &mut Blocks, path: &NodePath, replacement: Vec<Block>)`
  and `fn splice_in_blocks(current, steps, leaf_idx, replacement)` recursive helper.
  With `steps = []` splices at `root[leaf_idx]` directly. `unreachable!` on variant
  mismatch. The existing `a_u_prime.blocks.splice(idx..=idx, repl)` became
  `splice_at_path(&mut a_u_prime.blocks, &path, subtree.blocks)`. All 3929 pampa
  tests pass.
- [x] No WASM signature change. `PreviewNodeEditPayload` is already a
  `channel`-discriminated union from Plan 2b (`'text'` | `'subtree'`); Plan 3
  adds no new variants — both channels work for Descendable targets via the
  path-resolved lookup.
- [x] **Frontend: centralize textarea substitution in `Block`/`CustomBlock`
  dispatchers; extend affordance to all source-backed block components.**

  **Architecture.** The dispatch stack for every block is:
  `Node (framework)` → `Block` or `CustomBlock` (`q2-preview/dispatchers.tsx`)
  → individual leaf component. `Block`/`CustomBlock` already wrap every render.
  Textarea substitution belongs there; `data-block-pool-id` stays on individual
  semantic elements (no wrapper div at the dispatcher level — a dispatcher-level
  wrapper sits in the inactive view and breaks Bootstrap/Quarto `>` child-combinator
  CSS).

  **`useEditableBlock.tsx` — delete.** Before removing the file, grep for all
  importers (`grep -rn useEditableBlock ts-packages/ hub-client/src/`) to confirm
  nothing outside the 14 listed components imports it; any stray importer must be
  cleaned up in the same pass. Then absorb its logic into `Block` and `CustomBlock`
  with the widened **C2-safe gate:**
  ```
  resolved != null
    && resolved.reachabilityClass !== 'Opaque'
    && poolId !== undefined
    && ctx?.commitTextEdit !== undefined
    && ctx?.content != null
  ```
  When this gate passes AND `ctx.editTarget?.poolId === poolId`, render
  `<textarea>` directly (same sizing/commit/keydown logic as the deleted hook).
  Both `Block` and `CustomBlock` get identical substitution logic. Add
  `import { PreviewContext } from './PreviewContext'` to `dispatchers.tsx`
  (currently only `RegistryContext` is imported there).

  **14 existing components — update gate, remove editor logic.**
  Each of the 14 files that currently has `isEditable` / `useEditableBlock` /
  `{editor ?? ...}`:
  - **Update `isEditable`** to the C2-safe widened gate:
    `resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined`
    (drop the `commitTextEdit`/`content` guards — those now live in the dispatcher).
    This widening is what makes nested (Descendable) blocks show a hover ring;
    without it the affordance stays gated to TopLevel and the dispatcher's textarea
    logic is unreachable for nested blocks.
  - Remove the `useEditableBlock` call and `{editor ?? normalContent}` substitution
    (Para.tsx and Header.tsx only — other 12 components never called this hook).
  - The `data-block-pool-id` expression stays on the same semantic element and
    continues to use the (now-updated) `isEditable` to decide whether to set it.

  The 14 files are: `blocks/Para.tsx`, `blocks/Header.tsx`, `blocks/Div.tsx`,
  `blocks/BulletList.tsx`, `blocks/OrderedList.tsx`, `blocks/BlockQuote.tsx`,
  `blocks/DefinitionList.tsx`, `blocks/CodeBlock.tsx`, `blocks/RawBlock.tsx`,
  `blocks/Table.tsx`, `custom/Callout.tsx`, `custom/Theorem.tsx`,
  `custom/Proof.tsx`, `custom/FloatRefTarget.tsx`.

  **2 new components — add full affordance boilerplate.** These files currently
  have no `PreviewContext` import, no `poolId` extraction, and no `resolveSource`
  call. Add the same ~4-line pattern used by Div.tsx:
  `poolId`, `resolved`, `isEditable` (C2-safe gate), and `data-block-pool-id` on
  the wrapper element:
  - `blocks/Figure.tsx` — `data-block-pool-id` on the `<figure>` element
  - `blocks/LineBlock.tsx` — `data-block-pool-id` on the outer `<div class="line-block">`

  Both get full textarea editing via the dispatcher at no additional per-component
  cost.

  **C2 null-safety (required).** `resolved != null` must be an explicit check.
  A naive `resolved?.reachabilityClass !== 'Opaque'` evaluates `undefined !==
  'Opaque'` → `true` when `resolved` is `null`, making Generated/included-file
  blocks editable. Concrete victim: the appendix-structure wrapper `Div` resolves
  to `null` and is not caught by the `classes.includes(SECTION)` early-return at
  `Div.tsx:72`. The gate in the dispatcher must reject `null` before comparing
  `reachabilityClass`.

  **Plain.tsx — stretch item.** Plain renders `<>{renderChildren(args)}</>` (a
  fragment), so it has no wrapper element for `data-block-pool-id`. The stretch
  goal is to add a `<span data-block-pool-id={poolId}>` wrapper when editable,
  giving per-item granularity inside tight lists (the textarea, as always, comes
  from `Block`). Tight list items currently have no editing surface at all; any
  working implementation — even if imperfect — is preferable to the gap. Accept
  known rough edges (hover ring on a `<span>` inside `<li>`, span-in-li HTML).
  If the span approach causes layout problems, fall back to no affordance on Plain
  for v1 and document explicitly.
- [x] **U1 (resolved — no work, just don't regress it).** The backend miss-guard
  is `lookup_block` → `None` ⇒ no-op + original content (tested:
  `stale_ast_miss_noops_and_returns_original_content`,
  `apply_node_edit_noops_for_synthetic_target`, `..._inside_include`). It is *not*
  a top-level-exact gate, so it does not reject `Descendable` edits — the recursive
  lookup returns `Some(path)` for source-backed nested blocks, so the no-op branch
  never fires for them.

## End-to-end verification
- [x] `cargo nextest run -p pampa` (node_edit_tests) green — 44 tests pass (21 new Plan 3 + 23 existing).
- [x] `cargo xtask verify --skip-hub-build` — Rust-only compile + tests green (exit code 0).
- [x] From `hub-client/`: `npm run test:ci` — vitest suite: 556 unit + 66 integration tests pass; `useEditableBlock.integration.test.tsx` updated to test via `Block` dispatcher.
- [x] From `hub-client/`: `npm run build:all` — TypeScript type-check + WASM + production bundle green.
- [x] Dev server: verify the hover ring now appears on nested Descendable blocks
  (Para inside fenced div, list items, blockquote) — intended but a visible UX
  change. Then edit a paragraph inside a callout / fenced div end-to-end; confirm
  only that paragraph changed in the resulting `.qmd`. Also verify whole-block
  textarea for a callout, Figure, and LineBlock. Record output.

## Risks / watch-items
- **Wholesale container rewrite** (see Investigate) — the main quality risk.
- **Nested matches are unique, not ambiguous** (U2). Distinct byte ranges
  guarantee at most one exact match in the tree; the smallest-pre-order-path
  tiebreak is dead-defensive. `None` (read-only) is the safe default.
- **C2 null-safety** (above) — the one change that, if mis-written, turns
  non-editable blocks editable. The gate lives in `Block`/`CustomBlock`
  dispatchers; a single correct implementation covers all block types automatically.
  Value-equality at depth degrades *safe* (null → not editable) **only because**
  the gate explicitly excludes `null`.
- ~~**List source ranges** — verify item/marker bytes are preserved~~ **(C3,
  removed.)** Under the settled wholesale rewrite, the writer regenerates the
  whole list including the edited item's *own* marker, so marker bytes are **not**
  preserved — this watch-item presumed the abandoned recurse-into-container writer.
  Sibling/marker fidelity is snapshot-tested (see Tests), never byte-asserted.

## References
- Spec (Plan 3 section, the recursion investigation note); `node_lookup.rs`,
  `apply_node_edit.rs`, `writers/incremental.rs`, `block.rs`.

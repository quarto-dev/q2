# Block editing — Plan 3: nested-block descent

**Date:** 2026-06-06
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Spec:** `claude-notes/designs/2026-06-06-block-editing-design.md`
**Phase:** 3. Rust (`pampa`) + a one-line frontend gate change.
**Depends on:** Plans 1, 2a, 2b.

## Overview

Let edits reach blocks **nested inside descend-able containers** (fenced `:::`
divs, list items, block quotes, definition-list bodies, figure bodies, **and
callout/theorem/proof bodies** — those are plain `Div`s in the untransformed AST).
The frontend payload is **unchanged** — a nested block carries its own `Original`
`source_info` — so the only real work is in Rust: `lookup_block` must **recurse**
and return a *path*, and `apply_node_edit` must **splice at that path**.

The frontend change is **one line**: widen Plan 2a's structural gate from
`allowed = {TopLevel}` to `{TopLevel, Descendable}`. Plan 2a already stores
`reachabilityClass` in `PreviewContext.sourceIndex`, so Plan 3 just starts
admitting `Descendable`. `Opaque` regions (`Table`
cells/captions) stay gated off — a **selective** admission, **not** "drop a gate."

**Win:** edit a paragraph inside a fenced div / list item / block quote / callout.

**Note on whole-container editing.** A top-level container is editable *as a
whole* in Plan 2 (e.g. to change a fenced div's attrs). Once Plan 3 makes its
contents editable, the deepest-target hover resolves to the inner block, so the
whole-container affordance is **shadowed**. Reaching the container as a single
unit is the Plan 4 background-vs-text geometry; accepted.

## Investigation result (pre-coding)

**Finding:** the writer always wholesales block containers.

`coarsen_plan_phase5` in `incremental.rs` handles `BlockAlignment::RecurseIntoContainer`
by checking for an inline plan. `InlineSplice` is only produced for
inline-content blocks (Paragraph, Header). For every block container —
`Div`, `BlockQuote`, `BulletList`, `OrderedList`, `DefinitionList` — no
inline plan exists, so the code falls through to `CoarsenedEntry::Rewrite`
(`incremental.rs:218-220`). The writer re-serializes the **entire** container.

**Consequence:** editing a block nested in a list item renumbers/re-bullets
all sibling items; editing a block inside a `BlockQuote` reflows all `>` lines.
This is Tier-2 reformatting one level down. All "sibling fidelity" tests must
be snapshot-based — do **not** assert sibling byte-identity.

**Decision (settled with user):** wholesale rewrites are acceptable for v1.
The guarantee that holds: bytes **outside** the enclosing container are
verbatim. A follow-up to make the writer recurse into containers (extending
`InlineSplice` to block-level) is possible but is not a v1 requirement.

## Container shapes (from `quarto-pandoc-types/src/block.rs`)
- `Div`, `BlockQuote`, `Figure` → single `Blocks` (`Vec<Block>`): path step is one index.
- `BulletList`, `OrderedList` → `Vec<Blocks>`: path step is `[item_idx, block_idx]`.
- `DefinitionList` → `Vec<(Inlines, Vec<Blocks>)>`: path step is
  `[def_idx, body_idx, block_idx]` — each term has a `Vec<Blocks>` (multiple
  definition bodies), so three coordinates are needed. Term `Inlines` not
  descendable.

A **path** is a sequence of these steps from the top-level block list to the
target block.

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
(`DefinitionList` → `[def_idx, block_idx]`) are descendable. What is *not*
editable is the **term** (`Inlines`) — but that is the global "no inline editing"
limit (design Out-of-scope), not a Plan-3 container gap. Same for short captions
(`Caption.short: Inlines`) and `CaptionBlock` (Inlines): not editable because
they're inline, not because of nesting.

Cell-level / caption-block descent (extra `Table`/`Caption` path steps) is a
possible follow-up, explicitly out of this series.

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
- [ ] New fixtures: paragraph inside a fenced `Div`; inside a `BlockQuote`;
  inside a bullet-list item; inside an ordered-list item; a `Div` nested in a
  `Div`.
- [ ] `lookup_block` (recursive) returns the correct **path** for each; `None`
  for a target not present.
- [ ] `apply_node_edit` edits a nested block: the target is replaced and the
  edited block round-trips; a 1→N nested edit splices N blocks into the
  container. Bytes **outside the enclosing container** are always verbatim.
- [ ] **Sibling fidelity — test shape set by the investigation.** If the writer
  *recurses* into the container, assert sibling blocks / sibling list items are
  **byte-verbatim** (no renumbering). If it rewrites the container *wholesale*
  (Tier-2), assert outside-container verbatim + **snapshot** the reformatted
  container (sibling renumber / bullet / `>` / padding accepted). Do **not**
  assert sibling byte-identity unconditionally — that presumes the recurse
  branch.
- [ ] Register the file in `tests/integration/main.rs` if not already.

### Implementation
- [ ] Before touching `node_lookup.rs`, `grep` for all Rust callers of
  `lookup_block` (the return type changes from `Option<usize>` to
  `Option<NodePath>`) and list them in this file so none are missed.
- [ ] `crates/pampa/src/node_lookup.rs` — recursive lookup returning a path
  (define a `NodePath` type, e.g. `Vec<PathStep>` where a step is a block
  index or a `(container_idx, block_idx)` / `(def_idx, body_idx, block_idx)`
  for lists). Use exact-match only at each level — **no `preimage_in` fallback**
  at any level (Plan 2b removed Pass-2 from the top-level lookup; the same
  rationale applies recursively: a source-backed Descendable node always has an
  exact Original match; a Generated node is not editable).
- [ ] `crates/pampa/src/apply_node_edit.rs` — navigate `A_u` to the parent
  `Blocks` via the path and `splice` the replacement there (generalizing
  `blocks.splice(idx..=idx, …)`); then `compute_reconciliation` +
  `incremental_write` as today. A `&mut Blocks` "resolve path to container"
  helper.
- [ ] No WASM signature change. `PreviewNodeEditPayload` is already a
  `channel`-discriminated union from Plan 2b (`'text'` | `'subtree'`); Plan 3
  adds no new variants — `Descendable` edits use the existing `'subtree'`
  channel with a path-resolved target.
- [ ] Frontend: widen Plan 2a's gate from `allowed = {TopLevel}` to
  `{TopLevel, Descendable}` — change the predicate on `sourceIndex` lookup from
  `reachabilityClass === 'TopLevel'` to `reachabilityClass !== 'Opaque'`. No
  other frontend change — 2a already stores `reachabilityClass` in `PreviewContext
  .sourceIndex` for every indexed block; `Opaque` stays gated. The 2b Pass-1
  commit guard is `TopLevel`-scoped, so it does not reject `Descendable` edits
  (those resolve by path, not by top-level exact match).

## End-to-end verification
- [ ] `cargo nextest run -p pampa` (node_edit_tests) green.
- [ ] `cargo xtask verify --skip-hub-build` (Rust-only) then `npm run build:wasm`
  + dev server: edit a paragraph inside a callout / fenced div end-to-end;
  confirm only that paragraph changed in the resulting `.qmd`. Record output.

## Risks / watch-items
- **Wholesale container rewrite** (see Investigate) — the main quality risk.
- **Ambiguous nested matches** — reuse the documented smallest-index tiebreak;
  `None` (read-only) is the safe default.
- **List source ranges** — verify item/marker bytes are preserved when editing
  an item's inner block.

## References
- Spec (Plan 3 section, the recursion investigation note); `node_lookup.rs`,
  `apply_node_edit.rs`, `writers/incremental.rs`, `block.rs`.

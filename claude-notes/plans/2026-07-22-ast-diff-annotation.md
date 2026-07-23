# AST diff → annotated AST + hub-client snapshot/compare debug UI

> **braid**: the `braid` CLI is not installed on this machine, so no strand was
> created for this work. File one when braid is available.

## Overview

Convert AST diffs produced by `quarto-ast-reconcile` (the AST differ: it
computes a `ReconciliationPlan` between two Pandoc ASTs) into a **new annotated
AST** that represents the changes inline, using nodes qmd can already
round-trip:

| change kind      | annotation                                    | qmd rendering        |
| ---------------- | --------------------------------------------- | -------------------- |
| added inlines    | wrap run in `Inline::Insert`                  | `[++ …]`             |
| removed inlines  | wrap run in `Inline::Delete`                  | `[-- …]`             |
| added blocks     | wrap run in `Div` with class `added`          | `::: {.added} … :::` |
| removed blocks   | wrap run in `Div` with class `removed`        | `::: {.removed} … :::` |

The qmd writer already emits Insert/Delete as `[++ …]` / `[-- …]`
(`crates/pampa/src/writers/qmd.rs:2307-2336`) and Divs as fenced `:::` divs, so
no writer changes are needed.

Test UI (hub-client): a **Snapshot** button that saves the current document's
AST JSON (already held in `Editor.tsx` state as `astJson`), and a **Compare**
button that diffs the snapshot against the current AST and `console.log`s the
annotated AST written as a qmd string.

## Design

### New module: `crates/quarto-ast-reconcile/src/annotate.rs`

`pub fn annotate_diff(before: &Pandoc, after: &Pandoc) -> Pandoc`

Computes `compute_reconciliation(before, after)` and walks the plan mirroring
`apply.rs`'s traversal, but instead of merging source locations it produces a
change-annotated AST:

- **Block walk** (per `block_alignments`, which is in *after* order):
  - `KeepBefore(i)` → clone `before[i]` unchanged.
  - `UseAfter(k)` → collect the *run* of consecutive `UseAfter`s into one
    `Div {.added}`.
  - **Deletions are implicit** in the plan: before-indices never referenced by
    `KeepBefore`/`RecurseIntoContainer`. Positioning heuristic: while walking,
    before emitting an alignment that references before-index `j`, first flush
    (as a `Div {.removed}` run) all unemitted unmatched before-indices `< j`;
    flush the remainder at the end. (Reorderings degrade gracefully.)
  - `RecurseIntoContainer` → dispatch exactly like `apply.rs`:
    - `block_container_plans` → rebuild container (Div/BlockQuote/Figure;
      Bullet/OrderedList via `list_item_alignments`) with recursively diffed
      children.
    - `inline_plans` → rebuild Paragraph/Plain/Header with diffed inlines.
    - `custom_node_plans` / `table_plans` / DefinitionList → **v1 fallback**:
      emit `Div {.removed}[before]` + `Div {.added}[after]` pair. (Slot/cell
      granularity can come later.)
- **Inline walk** (per `inline_alignments`):
  - `KeepBefore` → clone; `UseAfter` run → one `Inline::Insert`; removed run →
    one `Inline::Delete` (same positional heuristic).
  - `RecurseIntoContainer` → recurse into content of matching container
    variants (Emph, Strong, Span, Link, … — mirror
    `apply_inline_container_reconciliation`); `Note` via `note_block_plans`;
    otherwise fallback `Delete[before] + Insert[after]`.
- **Meta**: after's meta wins; meta diffs are out of scope for v1.
- New nodes use `SourceInfo::default()` / `AttrSourceInfo::empty()` /
  `empty_attr()`-style attrs with classes `added` / `removed`.

### WASM export: `crates/wasm-quarto-hub-client/src/lib.rs`

`diff_asts_to_qmd(before_ast_json, after_ast_json) -> String` (sync, returns
`AstResponse` JSON with `qmd` set) — pattern-copied from `ast_to_qmd`
(lib.rs:2825): `pampa::readers::json::read` both, `annotate_diff`, write via
`pampa::writers::qmd::write`.

### hub-client

- `ts-packages/preview-runtime/src/wasmRenderer.ts`: `diffAstsToQmd()` wrapper
  (mirrors `writeQmd`), plus `.d.ts` declaration in
  `hub-client/src/types/wasm-quarto-hub-client.d.ts`.
- `hub-client/src/components/Editor.tsx`: Snapshot/Compare debug buttons in the
  preview pane (near `PreviewStatusBar`); snapshot stored in a ref; Compare
  logs the qmd diff to the console.

## Work items

### Phase 1 — tests first (TDD)

- [x] Unit tests in `annotate.rs` (`#[cfg(test)]`): identical docs → no
      annotations; added block → `Div .added`; removed block → `Div .removed`
      at correct position; changed paragraph → Insert/Delete inlines inside
      the paragraph; type-changed block → removed+added pair; list item
      added/removed; nested Div recursion. (8 tests, all pass.)
- [x] `crates/pampa/tests/integration/ast_diff_annotate.rs`: parse two qmd
      strings → `annotate_diff` → `writers::qmd::write` → assert `[++ `,
      `[-- `, `::: {.added}`, `::: {.removed}` appear as expected (end-to-end
      for the Rust side). (4 tests, all pass.)
- [x] Run tests. (Tests + implementation landed together in one module.)

### Phase 2 — implementation

- [x] Implement `annotate.rs`; export `annotate_diff` from lib.rs.
- [x] Tests pass. `cargo nextest run --workspace`: 10320 passed; the 38
      failures (`bootstrap_sh`, pampa `test::` corpus, `grid_tables_test`,
      `test_section_divs`) are **pre-existing environment failures** on this
      machine (no `pandoc` installed — verified by stashing the change and
      re-running on the clean tree).

### Phase 3 — WASM + UI

- [x] `diff_asts_to_qmd` export in wasm-quarto-hub-client.
- [x] `diffAstsToQmd` in preview-runtime + type declarations (both `.d.ts`
      files and `WasmModuleExtended`).
- [x] Snapshot/Compare buttons: new `AstDiffDebugControls` component rendered
      in the Editor preview pane (below `PreviewStatusBar`), wired to
      `astJson` state; Compare logs the annotated qmd to the console.
- [x] `npm run typecheck` and `npm run build:all` (hub-client) succeed.

### Phase 4 — verification

- [x] WASM e2e: `hub-client/src/services/astDiff.wasm.test.ts` (6 vitest
      tests through the real WASM module, incl. the `diffAstsToQmd` wrapper).
      Observed output for `The cat sat…` → `The dog sat…`:
      `The [-- cat][++ dog] sat on the mat.`
- [ ] Full `cargo xtask verify` — not run this session (user deprioritized;
      pre-existing pandoc-dependent failures block a green run anyway).
- [ ] Manual browser check of the buttons — dev server started; user is
      checking the UI themselves.

## Finding worth remembering

Two same-position paragraphs that share **any** inline (even a Space) get
diffed at inline granularity (`compute_reconciliation` phase 2 recursion), so
block-level `.added`/`.removed` divs only appear on type changes, block
count changes, or fully-dissimilar single-word paragraphs. This is the
desired fine-grained behavior, but fixtures asserting block-level divs must
avoid shared inlines at the same position.

## Notes / limitations (v1)

- Table-cell and custom-node slot diffs degrade to whole-node removed+added.
- Reordered blocks show as remove+add at the old/new positions (the plan
  matches them as `KeepBefore`, so actually pure moves show as *unchanged*
  content at the new position — acceptable for a diff-view v1).
- Meta changes are not visualized.

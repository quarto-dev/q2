# Task-list rendering fix + interactive checkboxes (bd-obkvhlam)

**Date:** 2026-07-21
**Strands:** bd-obkvhlam (rendering bug, this plan's core), interactive-toggle
feature strand (filed from this plan, see below)
**Discovered from:** bd-eias3e39 audit

## Overview

`- [ ] todo` renders as `<li><span></span> todo item</li>` — the tree-sitter
grammar has no task-marker support, so `[ ]`/`[x]` parses as a bare bracketed
Span. Fix end-to-end: grammar → reader → HTML writer → qmd writer round-trip →
SCSS. Then (separate strand) make the checkboxes live in hub-client and
`q2 preview --allow-edit`: toggling a checkbox in the preview edits the qmd
source through the existing `apply_node_edit` incremental-write machinery.

## Design decisions

- **AST representation: Pandoc's convention.** Task items are ordinary
  `BulletList`/`OrderedList` items whose first inline is `Str "☐"` (U+2610,
  unchecked) or `Str "☒"` (U+2612, checked) followed by `Space`. This is
  byte-compatible with Pandoc JSON, costs no new AST types, and the `Str`'s
  source-info covers exactly the `[ ]`/`[x]` bytes — which is what the
  interactive toggle needs for a minimal source splice.
- **Grammar approach: upstream tree-sitter-markdown's.** Marker tokens
  `task_list_marker_checked: prec(1, /\[[xX]\]/)` /
  `task_list_marker_unchecked: prec(1, /\[[ \t]\]/)`, a third
  `_list_item_content` choice `seq(marker, _whitespace, paragraph, repeat(_block))`,
  and word-level regex escapes so `[x]` elsewhere still lexes as text. Adapted
  to the unified grammar (inline `[` tokens exist in the same lexer — corpus
  tests must pin `- [x](url)` = link, `- [xx]` = span, mid-paragraph `[x]`
  unchanged).
- **HTML output matches vendored Pandoc:** `<ul class="task-list">` (all-task
  bullet lists only) and `<li><label><input type="checkbox" [checked=""]
  />…</label>` replacing the marker Str + Space. No `disabled` attribute —
  current Pandoc doesn't emit one (checkboxes are enabled-but-inert in static
  HTML); the preview renderer disables them only when the surface can't
  commit edits.
- **qmd writer round-trips** leading ☐/☒+Space in list items back to
  `[ ] `/`[x] ` (same special-case Pandoc's markdown writer has).

## Phase 0 — Tests first (all must fail before implementation)

- [x] tree-sitter corpus `test/corpus/task_lists.txt`: unchecked/checked/`[X]`,
      `+`/`*` markers, ordered-list task item, nested, NOT-a-task cases
      (`- [x](url)` link, `- [xx]` span, `[x]` mid-paragraph, `[ ]` not
      followed by whitespace).
- [x] pampa reader test: AST = Str ☐/☒ + Space first inlines; marker
      source-info spans the bracket bytes.
- [x] pampa HTML writer test: `ul.task-list` + checkbox inputs, checked/
      unchecked, disabled.
- [x] pampa qmd writer round-trip test: source → AST → source identity.
- [x] quarto-sass compile test: compiled CSS contains the `ul.task-list`
      padding + checkbox margin rules.

## Phase 1 — Grammar

- [x] Add tokens + `_list_item_content` choice; `tree-sitter generate`,
      `tree-sitter build`, `tree-sitter test` in
      `crates/tree-sitter-qmd/tree-sitter-markdown/`.

## Phase 2 — Reader (`crates/pampa/src/pandoc/treesitter.rs`)

- [x] Handle `task_list_marker_{checked,unchecked}` in `process_list_item`:
      prepend Str ☐/☒ + Space to the item's first Plain/Paragraph inlines,
      with the marker node's source range on the Str.

## Phase 3 — HTML writer (`crates/pampa/src/writers/html.rs`)

- [x] Detect task items (first inline Str ☐/☒) in list emission; emit
      `class="task-list"` on the list and the checkbox input in place of the
      marker Str + Space.

## Phase 4 — qmd writer (`crates/pampa/src/writers/qmd.rs`)

- [x] Round-trip leading ☐/☒ in list items to `[ ] `/`[x] `.

## Phase 5 — SCSS (the bd-obkvhlam CSS follow-through)

- [x] Port `ul.task-list { padding-left: 1em }` (_quarto-rules.scss:338) and
      `input[type="checkbox"] { margin-right: 0.5ch }` (:697) with provenance
      comments; re-capture phase5 baseline hash if styles.css shifts.

## Phase 6 — Verification

- [x] `cargo nextest run --workspace` (10355/10355; no .snap churn; one error-corpus
      `_autogen-table.json` regen + one phase5 baseline re-capture) (report snapshot churn per policy).
- [x] `cargo xtask verify` (Rust legs + manual hub legs: `npm run build:all`,
      `test:ci`, preview-renderer suites; only known-environmental live-sync
      failures) (WASM leg affected — pampa changes).
- [x] End-to-end: `cargo run --bin q2 -- render` the audit kitchen-sink
      fixture; inspect emitted `<ul class="task-list">` markup + CSS.

## Phase 7 — Interactive toggle (bd-tvtknbhx) — IMPLEMENTED same session

Landed in `5852d0ee` (`ts-packages/preview-renderer/src/q2-preview/blocks/
taskList.tsx` + BulletList/OrderedList wiring): checkboxes in the React
renderer, toggle → `commitSubtreeEdit` (flip ☐/☒ in the untransformed
sourceNode) → `apply_node_edit` → qmd-writer round-trip → disk. Live-verified
against `q2 preview --allow-edit` (both directions, editor-activation guard,
label-click guard, rich-text commit preserves the ascii marker). Strand stays
in_progress for: hub-client live verification, loose-item rendering, editor
ballot-glyph polish, bullet canonicalization note (see strand comment).

### Original phase sketch (for reference)

Ride the block-editing substrate
(`claude-notes/designs/2026-06-06-block-editing-design.md`;
`apply_node_edit` splice → reconcile → `incremental_write`; `--allow-edit`
already exists on `q2 preview`). Checkbox click in an edit-capable surface →
splice the marker's source range `[ ]`↔`[x]` → normal re-render loop. Scope,
DOM→source-range plumbing (the marker Str's source-info), and hub-client wiring
to be designed in that strand's session. Requires WASM rebuild chain for
preview verification.

## Notes

- Ordered-list task items are accepted (upstream grammar shares
  `_list_item_content` across list types; Pandoc accepts them too).
- User-authored literal ☐/☒ at item start becomes `[ ]`/`[x]` on qmd
  round-trip — same ambiguity Pandoc accepts.

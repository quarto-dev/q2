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
- **HTML output matches Pandoc/Q1:** `<ul class="task-list">` and
  `<li><input type="checkbox" disabled="" [checked=""] />` replacing the
  marker Str. Static render keeps `disabled` (Q1 parity); the interactive
  strand un-disables under edit-capable surfaces.
- **qmd writer round-trips** leading ☐/☒+Space in list items back to
  `[ ] `/`[x] ` (same special-case Pandoc's markdown writer has).

## Phase 0 — Tests first (all must fail before implementation)

- [ ] tree-sitter corpus `test/corpus/task_lists.txt`: unchecked/checked/`[X]`,
      `+`/`*` markers, ordered-list task item, nested, NOT-a-task cases
      (`- [x](url)` link, `- [xx]` span, `[x]` mid-paragraph, `[ ]` not
      followed by whitespace).
- [ ] pampa reader test: AST = Str ☐/☒ + Space first inlines; marker
      source-info spans the bracket bytes.
- [ ] pampa HTML writer test: `ul.task-list` + checkbox inputs, checked/
      unchecked, disabled.
- [ ] pampa qmd writer round-trip test: source → AST → source identity.
- [ ] quarto-sass compile test: compiled CSS contains the `ul.task-list`
      padding + checkbox margin rules.

## Phase 1 — Grammar

- [ ] Add tokens + `_list_item_content` choice; `tree-sitter generate`,
      `tree-sitter build`, `tree-sitter test` in
      `crates/tree-sitter-qmd/tree-sitter-markdown/`.

## Phase 2 — Reader (`crates/pampa/src/pandoc/treesitter.rs`)

- [ ] Handle `task_list_marker_{checked,unchecked}` in `process_list_item`:
      prepend Str ☐/☒ + Space to the item's first Plain/Paragraph inlines,
      with the marker node's source range on the Str.

## Phase 3 — HTML writer (`crates/pampa/src/writers/html.rs`)

- [ ] Detect task items (first inline Str ☐/☒) in list emission; emit
      `class="task-list"` on the list and the checkbox input in place of the
      marker Str + Space.

## Phase 4 — qmd writer (`crates/pampa/src/writers/qmd.rs`)

- [ ] Round-trip leading ☐/☒ in list items to `[ ] `/`[x] `.

## Phase 5 — SCSS (the bd-obkvhlam CSS follow-through)

- [ ] Port `ul.task-list { padding-left: 1em }` (_quarto-rules.scss:338) and
      `input[type="checkbox"] { margin-right: 0.5ch }` (:697) with provenance
      comments; re-capture phase5 baseline hash if styles.css shifts.

## Phase 6 — Verification

- [ ] `cargo nextest run --workspace` (report snapshot churn per policy).
- [ ] `cargo xtask verify` (WASM leg affected — pampa changes).
- [ ] End-to-end: `cargo run --bin q2 -- render` the audit kitchen-sink
      fixture; inspect emitted `<ul class="task-list">` markup + CSS.

## Phase 7 — Interactive toggle (separate feature strand)

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

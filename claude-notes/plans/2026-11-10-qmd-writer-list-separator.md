---
title: 'qmd writer: ordered-list separator doesn''t survive re-reading inside list items (bd-2bcre8cr)'
date: 2026-11-10
description: 'The qmd writer places a bare `<!-- -->` tight against lists inside list items, where the reader absorbs it or drops a list; scope a writer-side fix and close the test gap that hid it.'
status: active
braid:
  strand: bd-2bcre8cr
---

**Checkout:** main checkout, branch
`braid/bd-2bcre8cr-qmd-reader-empty-ordered` (made with `switch-task` off
`main` @ `665f9b588`; the slug predates the retitle). The design was
aligned with the user on 2026-11-10 (see Decisions).

## Triage verdict

**Ready to design.** The defect reproduces at HEAD and is wider than
the strand's title. The evidence points to a **writer-side** fix, since
the reader matches Quarto 1 (pandoc markdown) on most of the failing
shapes. The open questions are about separator form and scope, not
about missing information.

## Issue context

bd-2bcre8cr (P2 bug, `pampa` and `qmd-reader`) was filed 2026-11-10 from
a CI failure on PR #1137, which was unrelated to it. On one macOS run
the randomized test
`list_looseness::sampled_list_documents_round_trip_through_the_qmd_writer`
found the input `- 1.\n  1)\n`. The writer emits
`* 1.\n  <!-- -->\n  1)`, and the re-read turns `1)` into
`Plain [Str "1)"]`. Proptest's seed line is in the strand. The title
blames the reader; this investigation says it is mostly the writer.

## Dependency graph

- **related** bd-az53ojvo (closed, PR #1098): CommonMark loose/tight
  lists. It built the `list_looseness` harness (spec examples, an
  enumerated corpus of 18,446 documents, proptest, and round trip) and
  made the decision that **"CommonMark is the target over
  pandoc-markdown"** for looseness. Its follow-ups are the sibling
  open strands bd-se0yhp3o (quote merge), bd-9xnwn3z7 (a tab on a blank
  line), bd-qgswtht1 (items starting with a blank line), and bd-6522kqyw
  (the writer emitting `* * *`). Each has its own `known_generated` or
  `known_roundtrip` exemption in the harness. This bug has none, so it
  shows up as a flaky proptest.
- Not linked, but load-bearing: the
  `2026-09-04-block-html-adjacent-markdown-unparsed.md` plan (bd-block-html…).
  It decided that block-level html `RawBlock`s are written **bare**,
  not fenced, and recorded that "our reader's HTML block extent is the
  paragraph".
- Nothing blocks on this strand. There is no incoming pressure apart
  from CI flakiness, which it causes about once per `PROPTEST_CASES`
  draw.

## What the code looks like today

Details, the probe script, and its output are in
`qmd-writer-ordered-list-separator-investigation/` (`findings.md`,
`roundtrip.sh`, `roundtrip-output.txt`, `pandoc-writer-output.txt`).

- **Reader.** `crates/pampa/src/pandoc/treesitter*`, together with the
  scanner's `dont_interrupt` logic for empty items in
  `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`. A
  comment-only line directly after paragraph text is a lazy
  continuation (`RawInline`); a comment line on its own becomes a
  `RawBlock` (`treesitter_utils/html_block.rs`). This matches
  `pandoc -f markdown` (Quarto 1) for every probed shape except the
  empty-item case, where all three readers disagree.
- **Writer.**
  - `crates/pampa/src/writers/qmd_prepass.rs`, `separate_ordered_lists`:
    inserts `<!-- -->` between **any** two adjacent `OrderedList`s.
  - The block writer then emits that bare `RawBlock` tight inside tight
    items, and with blank lines elsewhere. The blank lines make the
    enclosing list loose under CommonMark rules.
  - The header comment ("a qmd-read AST never contains either shape")
    is wrong for adjacent lists with different delimiters, which the
    qmd reader produces routinely.
- **Failure classes found** (the numbers are the cases in `findings.md`):
  1. A bare separator tight before an empty item: the item can't
     interrupt the comment's "paragraph" (case 1, the strand).
  2. A bare separator tight after item text: absorbed as a `RawInline`
     (case 2). This is silent, because the round-trip test compares
     shapes only.
  3. Blank lines around the separator flip the enclosing list to loose
     (case 3).
  4. An **author-written** `<!-- -->` between two lists in an item is
     re-written tight, and the two lists **merge** (case 7). This is
     data loss in ordinary qmd, with no import involved.
  5. A separator is inserted where the reader would split the lists
     anyway, because the delimiters differ (cases 1, 2, 3 and 5).
- **Tests.** `check_roundtrip` compares `shape_of` before and after,
  which hides class 2. The enumerated corpus never builds adjacent
  sublists; only the proptest sampler does, which is why this shows up
  as a flake rather than a deterministic failure.

## Decisions (2026-11-10)

1. **Fix the writer, not the reader.** The strand is retitled "qmd
   writer: ordered-list separator doesn't survive re-reading inside
   list items". The reader-only oddity (an empty item after a
   comment-only line) is filed as **bd-go7corj6** at P4.
2. **Separator form, option (a).** Inside list items, write the
   separator as a fenced ```` ```{=html} ```` block. At top level, keep
   today's form (bare, blank lines, the same as pandoc).
3. **Include author-written raw blocks next to lists** (class 4, the
   list merge): the same rule applies to any bare html `RawBlock` that
   directly follows a list inside an item.
4. **No separator between adjacent ordered lists whose delimiters
   differ.** The reader splits them by itself.
5. **Round-trip test:** keep the shape comparison, and also fail on any
   *new* `RawInline` or `RawBlock`. The full-AST comparison is deferred,
   because it would likely surface many proptest failures we don't yet
   understand well enough to fix.
6. **Generator coverage:** add adjacent sublists (same and differing
   delimiters, with and without an authored separator) to the
   enumerated corpus.
7. **Branch:** made with `switch-task` (above).

### Deferred: provenance tagging for writer-inserted separators

The user suggested recording the separator's provenance, for example
through a deliberate choice of `quarto-source-map` feature or a new
`Trivia` variant, so that round trips can handle it properly. Notes
from checking `quarto-source-map` 0.3.1 and pampa's writers:

- **An extension point already exists.**
  `SourceInfo::Generated { by: By { kind, data }, from }` takes
  arbitrary kinds, with `By::raw(kind, data)` and `By::is_kind`. The
  reader already uses it for parser-synthesized Spaces
  (`By::tree_sitter_postprocess()`). A new `SourceInfo` variant would
  change a published crate's enum and its JSON encoding; a new `By` kind
  would not.
- **Provenance doesn't survive the text round trip on its own.** Once
  written as qmd and re-read, the `<!-- -->` gets `Original` source
  info. For the tag to persist, the reader would have to re-derive it
  from content: "an empty `<!-- -->` RawBlock between two ordered
  lists". That rule would also tag an author's identical separator.
  Arguably that is fine, since the meaning is the same.
- **What it would buy:**
  - Exact round trips for imported ASTs: `[OL, OL]` →
    `[OL, RawBlock, OL]` stops counting as a difference.
  - The writer could drop an orphaned separator (lists deleted, or
    delimiters changed) and regenerate one where needed.
  - Downstream writers could skip it.
- **The cost is in the incremental writer.** Today the prepass gives
  the inserted separator the *previous block's* source info, on
  purpose, so the hub's incremental writer can still tile its output.
  A `Generated` node without an `Invocation` anchor is reported there
  as `GeneratedNoInvocation`. Moving the separator to a `Generated`
  kind needs an anchor (for example, an `Invocation` pointing at the
  preceding list) or special handling in `incremental.rs`.

Decisions 1–6 don't depend on this. Fencing inside items and dropping
the unnecessary separators fix the reported failures either way.
**Decided 2026-11-10:** the costs outweigh the benefits for this
failure, so tagging is filed as a follow-up, **bd-bq4mjc6b** (P4).

The user also confirmed the 2026-09-04 bare-HTML rule in its narrow
scope (block-level HTML from naked-HTML input, plus comments). It is not
being revisited. Option (a) is a list-item exception to it.

## Checklist

### Phase 0: tests first (must fail on HEAD)

- [x] Deterministic regression table for classes 1–5 (the cases in
  `findings.md`), run through the qmd writer round trip.
- [x] The proptest `cc` line in
  `crates/pampa/tests/proptest-regressions/list_looseness.txt`.
- [x] `check_roundtrip`: fail on any `RawInline` or `RawBlock` count
  that grows from the first read to the re-read (decision 5).
- [x] Enumerated corpus: adjacent sublists with same and differing
  delimiters, with and without an authored separator (decision 6).

### Phase 1: skip unnecessary separators

- [x] `separate_ordered_lists` only separates neighbours with the
  same *written* marker (`needs_list_separator`): `Default` and
  `Period` both write `1.`, `OneParen` and `TwoParens` both write `1)`,
  and Example (`(@)`) lists are always separated, because of
  bd-bumesax9, a reader parse error on consecutive `(@)` items found
  here. The trigger scan uses the same predicate, so qmd-read
  documents take the borrowed fast path again. The header comment is
  fixed.

### Phase 2: robust separator inside items

- [x] Inside a list item, a bare html `RawBlock` that directly follows a
  list (inserted or authored) is written as a fenced ```` ```{=html} ````
  block, tight. Top level is unchanged.
- [x] Re-check the 2026-09-04 writer adjacency rules (`RawBlock` tight
  against `Plain`) for interactions. A comment directly after item
  *text* is outside this rule, and benign: written tight, the reader
  already absorbs it into the text on the first read, so no `RawBlock`
  exists; with blank lines the item is loose and the bare form is safe.
- [x] Remove the prepass's `loosen_items_with_separators`. It was an
  earlier workaround for the same defect: it turned an item's leading
  `Plain` into a `Para` (case 3), and it missed items that start with
  a list (cases 1 and 2). Fencing makes it unnecessary.
  `qmd_writer_pandoc_shapes::adjacent_ordered_lists_in_a_tight_item_are_not_swallowed`
  now checks for an exact round trip and the fence, instead of
  erasing the Plain/Para difference.

### Phase 3: verification

- [x] `list_looseness` with turned-up `PROPTEST_CASES` (200,000 cases
  each for the oracle and round-trip samplers, about 10 s, all pass),
  the qmd writer tests, and the import round-trip tests.
- [x] `cargo xtask verify --skip-hub-build`. Every non-Rust step passes,
  including the hub-client WASM suite on a rebuilt bundle. The Rust
  suite runs 19,209 tests; the only failure is
  `knitr::capture_classifies_pieces`, which depends on the local R
  install.

### Verification record

- Investigation probe after the fix:
  `qmd-writer-ordered-list-separator-investigation/roundtrip-output-after-fix.txt`.
  All eight cases round-trip unchanged. The strand's case now writes
  `* 1.\n  1)` with no comment, and an author-written separator inside
  an item is written as a fence.
- Before the fix, the new tests failed for the predicted reasons. In
  `adjacent_lists_round_trip_through_the_qmd_writer`, 7 of 8 cases
  failed (the passing one is a loose item).
  `imported_same_delimiter_lists_stay_apart` failed with
  `[OL, RawBlock, Plain]`. The enumerated round trip failed on the new
  adjacent-sublist sweep, and the oracle tests kept passing with that
  sweep, so the readers already agree on these shapes.

## Risks / tradeoffs (draft)

- **The imported-document round trip.** Changing how the separator is
  written changes the qmd output for imported documents with adjacent
  same-delimiter lists (docx and pandoc imports). Existing snapshots
  may pin the bare form.
- **The incremental writer** (hub edits) relies on the prepass being a
  copy-on-trigger no-op for qmd-read ASTs. Question 4 restores that
  property; options (a) and (b) in question 2 must keep inserted
  blocks' source info tiling intact, as the current code does.
- **Q1 parity of the written text.** Option (a) produces text that Quarto
  1 reads correctly as well (checked with `pandoc -f markdown`).

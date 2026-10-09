---
title: 'Plan: qmd writer fixes for imported documents (document import P2)'
date: 2026-10-03
description: 'Fixes four qmd writer bugs that corrupt pandoc-shaped documents from import, covering line-start text that re-reads as markup, multi-block footnotes, merged adjacent lists and attributed editorial marks.'
---

**Epic:** [`2026-10-03-document-import-epic.md`](2026-10-03-document-import-epic.md) (I3, I17, I18; Findings → "qmd writer bugs reachable from docx")
**Depends on:** nothing. **Unblocks:** P3.
**Branch:** `import/p2-qmd-writer` from `feature/hub-import`.

## Why

pampa's qmd writer (`crates/pampa/src/writers/qmd.rs`) was built for ASTs that came from qmd. Pandoc's readers produce shapes the qmd reader never does, and four of them come out wrong. Each was verified on 2026-10-02/03 (inputs `losses.md` and `esc.json` in `claude-notes/research/2026-10-02-document-import-spike/`):

1. **Line-start text that re-reads as a construct.** A paragraph starting with `1. `, `- ` or `+ ` becomes a list. So does a line inside a paragraph, after a soft break (`para\n- dash`, `para\n1) x`); `para\n---` becomes a rule and `para\n::: x` a div. The text is corrupted.
2. **Footnotes with more than one block.** `write_note` (`qmd.rs:2393`) joins paragraphs with a space and writes any other block as the literal `[complex block]` (`qmd.rs:2416`).
3. **Adjacent lists merge.** Two adjacent `BulletList`s or `OrderedList`s re-read as one. The docx reader emits adjacent ordered lists when Word restarts numbering.
4. **Editorial marks with attributes lose their shorthand** (`write_span`, `qmd.rs:2238-2250`). Track changes (I3) are all attributed marks, so without this every imported change reads `[text]{.quarto-insert author="…" date="…"}`.

**Out of scope (I18):** pipe-table footers, definition-list indentation, table widths and attributes, list number styles (I17 is a grammar limit, reported by P3).

## The oracle

Every fix is tested the same way: **pandoc-shaped JSON → `qmd::write` → `pampa` qmd reader → JSON**, and the AST equals the input after the reader's normal desugaring, compared without source info. Build the inputs as pandoc JSON (hand-written or `pandoc -t json` output checked in), never as qmd: qmd-sourced ASTs don't contain these shapes, which is why the existing round-trip suite (`crates/pampa/tests/roundtrip_tests/qmd-json-qmd/`) never caught them. Read the JSON with `read_completing_source_info(.., By::unknown())` (`crates/pampa/src/readers/json.rs:1606`).

Find the comparison helper the existing round-trip test uses (`test_qmd_roundtrip_consistency`, `crates/pampa/tests/integration/test.rs:922`) and reuse it rather than writing a new AST comparison: it writes both ASTs with `writers::json::write`, strips source info with `remove_location_fields` and compares the `serde_json::Value`s (`test.rs:969-987`). `remove_location_fields` is a private `fn` at `test.rs:609`; `test` is already `pub mod test` in `tests/integration/main.rs`, so making the function `pub(crate)` is enough for pampa's tests. quarto-core's T2 tests can't reach a pampa test module; they compare `pampa::writers::native::write` output of both ASTs instead (native carries no source info, and works there because `FootnotesTransform` has resolved the note forms native refuses). For pandoc-JSON input, compare the input as read by `read_completing_source_info` and re-serialized, so both sides went through pampa's JSON writer.

## The mechanism (pinned, implementability review 2026-10-03)

The writer emits one block or inline at a time, and three fixes need context it doesn't have. They use two mechanisms:
- **An AST pre-pass** run before writing, in **both** top-level loops: `write_impl` (`qmd.rs:3254`) and `write_impl_tracked` (`:3307`, the incremental writer the hub calls). It handles T2 and the ordered-list half of T3. It rewrites a multi-block `Note` into a `NoteReference` inline plus a `NoteDefinitionFencedBlock` inserted after the containing top-level block, so the existing `write_notereference` (`:2423`) and `write_fenced_note_definition` (`:1171`) do the writing. It also inserts `RawBlock (Format "html") "<!-- -->"` between adjacent `OrderedList`s, which the existing raw-block path writes. Inserted blocks take the source info of the block they follow, so `write_impl_tracked`\'s tiling still holds. Both loops take `&Pandoc` and the incremental writer runs on every hub edit, so the pre-pass is **copy-on-trigger**: a read-only scan first, and a clone only when it finds a multi-block `Note` or adjacent `OrderedList`s (never in a qmd-read AST). A test asserts that an AST with neither is written byte-identically to today. `write_note` (`:2393`) is an inline writer and can't emit a block, which is why this isn't done there. `write_single_block` / `write_single_inline` use fresh contexts and don't run the pre-pass: a multi-block `Note` there keeps today's inline form. qmd-read ASTs can't contain one, so this is unreachable from the hub's edits; say so in a code comment.
- **The line-start flag** for T1: `ctx.at_line_start` is already updated after every inline (`qmd.rs:2912`), but it starts `false` and only fragment mode reads it (`:1955`). Set it `true` at each block start (paragraph, plain, a list item's and a block quote's first line), and read it in `write_str` in all modes. The look at the next inline ("followed by `Space`, a break or the end") is a shared helper over the inline slice, used by every container loop (top level and inside `Strong`/`Emph`/`Span`/`Link`). A `SoftBreak` inside `Emph` followed by `- x` must be caught too, and `write_prose_inlines` (`:3026`) splits only top-level breaks.
- Bullet markers (T3) are threaded through a context field, not a pre-pass: the pre-pass can't change a marker, which isn't in the AST.

## Checklist

### Tasks

- [x] **T0 Baseline and harness.** Record the workspace nextest baseline (pass/skip counts) in the Handoff log. Add `crates/pampa/tests/integration/qmd_writer_pandoc_shapes.rs` (registered in `tests/integration/main.rs`, alphabetical) with a table-driven helper `assert_roundtrip(pandoc_json)` implementing the oracle, and a second helper for the cases where the oracle allows a known difference (T3's ordered-list separator).
- [x] **T1 Line-start escaping.** Add table-driven cases for text at a paragraph start **and** after `SoftBreak` and `LineBreak`. Constructs to cover:
  - list markers: `1.`, `1)`, `12.`, `-`, `+`, `*`;
  - block starts: `#` (the existing escape), `>`, `:` (definition marker, Q-2-54), `:::`, `|`;
  - fences: `` ``` ``, `~~~`;
  - rules and setext underlines: `---`, `***`, `___`, `===`;
  - a marker with no following space (e.g. `1.5` should not be escaped needlessly; assert the output stays `1.5`).

  Also cover the same texts at the start of a list item's and a block quote's first line (`* 1. x`, `* - y` and `> + z` are written unescaped today and re-read as nested lists, verified 2026-10-03), after a `SoftBreak` nested inside `Emph`/`Strong`/`Span`, and inside a table cell (`1. c` there round-trips with or without an escape; assert only the round trip).

  **Span followed by `(`** (found in the implementability review, verified): an inline whose written form ends in `]` (a `Span` with no attributes, so every I4 comment wrapper) followed by a `Str` starting with `(` is written `[range [c]{…}](see)` and re-reads as a `Link`. Escape the `(` as `\(` when the previous inline's output ends in `]` (verified: `]\(see)` re-reads correctly). `[` and `{` are already escaped in `Str` (verified: `[x]\{y\}`, `\[ref\]` round-trip). Cases: a span then `Str "(see)"`; an I4 comment wrapper then `Str "(see)"`; a span then `Str "[ref]"` and `Str "{y}"` (regression guards).

  Run them first and record which fail. Already handled by today's escapes, per the review probe: `*`, `|`, `~`, `` ` ``, runs of two or more `-`, `#`, `>`. Failing: `1.`, `1)`, lone `-`, `+`, `:`, `:::`, and `===` after a soft break (setext).

  **Where the fix goes.** `escape_markdown` (`qmd.rs:1749`) sees one `Str` at a time and doesn't know whether it is at a line start or what follows it. `LineStartEscapes::Always` (the whole-document mode) escapes regardless of position, and only the incremental `inline_fragment` mode reads `at_line_start` (`qmd.rs:1728-1740`, `1953-1956`). The fix uses the line-start flag described in "The mechanism":
  - text is at a line start when it begins a paragraph/plain, follows a `SoftBreak`/`LineBreak` at any nesting depth, or begins the first line of a list item or block quote;
  - the escape depends on the **next inline** too: escape `1.`/`1)`/`-`/`+`/`:` only when followed by `Space`, a break or the end of the block (`Str "1.5"` and `Str "-5"` stay plain);
  - it works the same in both writer modes, and mid-line text is never escaped (no `\-` on every lone hyphen).

  The escape forms `1\.`, `1\)`, `\-`, `\+`, `\:`, `\>` and `\:::` re-read as `Str` (verified 2026-10-03); confirm each through the round trip. If an escape the grammar rejects is the only option for some construct, STOP and report.
- [x] **T2 Multi-block footnotes.** Built as the pre-pass in "The mechanism". Sizing: two to three days, in separate commits (the pre-pass and id scan, the `write_impl_tracked` integration, the quarto-core oracle). The id scan can use `pampa::filters::topdown_traverse*` or a hand-written walk. A `Note` whose content is exactly one `Para` or `Plain` stays inline `^[…]` (no churn). Any other Note is written as a reference `[^n<k>]` at the note's position plus a fenced definition
  ```
  ::: ^n<k>
  <blocks>
  :::
  ```
  placed after the top-level block that contains the reference. Ids `n1`, `n2`, … are assigned in document order, skipping any id already used in the document (note ids and element ids).

  **The oracle needs the footnote transform.** Verified 2026-10-03: the pampa reader does not turn `[^n1]` plus `::: ^n1` back into a `Note`. It produces `Span ("", ["quarto-note-reference"], [("reference-id","n1")]) []` and a `NoteDefinitionFencedBlock` block, and `pampa -t native` refuses them (Q-3-11, "coalescing … not yet implemented"). quarto-core's `FootnotesTransform` (`crates/quarto-core/src/transforms/footnotes.rs`) resolves both forms into `Inline::Note` for every output format, including the hub preview and pandoc downloads. So T2's cases live in **quarto-core's** integration tests (`crates/quarto-core/tests/integration/qmd_writer_footnotes.rs`): write with pampa, re-read, run the real `FootnotesTransform`, then compare with the input. Do not reimplement the resolution in a test helper. Three details the oracle needs:
  - `FootnotesTransform` prepends a ref-id `RawBlock` marker to each resolved Note and strips it only when `ctx.pipeline_profile` is `PipelineProfile::Pandoc(_)` (`footnotes.rs:136-138`). Build the `RenderContext` with a Pandoc profile, following the footnotes.rs unit tests\' `make_test_project` pattern.
  - The reader turns a Note's `Plain` into `Para` (verified: `Note [Plain "x"]` → qmd → `Note [Para "x"]`), and the docx reader's flattened-list footnotes are `Note [Para, Plain, Plain]`. The oracle allows `Plain` → `Para` inside Notes.
  - A Note in metadata (a footnote in a docx title) has no top-level block for a definition. It keeps today's inline `^[…]` form with paragraphs joined; record what the docx path produces for it in the Handoff log.

  Cases:
  - two paragraphs;
  - a paragraph and a bullet list;
  - a code block;
  - a table;
  - a note inside a list item;
  - a note inside a table cell (the definition must still land after the top-level block);
  - two notes in one paragraph.
- [x] **T3 Adjacent lists.**
  - Bullet lists: when a `BulletList` directly follows another `BulletList`, alternate the bullet character (`*` / `-`). This is lossless (verified, including three in a row: `* A`, `- B`, `* C` read as three lists). `*` is hardcoded in `BulletListContext::new` and in `write_bulletlist` (`qmd.rs:736-790`, the bare `*` and the `* []` empty-item forms); thread the marker through them as a context field set by the block loop, which knows the previous sibling.
  - Ordered lists: when an `OrderedList` directly follows another, the pre-pass inserts a `<!-- -->` raw block between them ("The mechanism"). It re-reads as `RawBlock (Format "html") "<!-- -->"` (verified), so the oracle for this case allows exactly that inserted block.
  - Check the same within nested contexts (a list item ending in a list followed by another list, block quotes, divs).
  - First search the grammar and reader for a construct that separates ordered lists without an AST node; record the result. Use it if one exists; otherwise keep `<!-- -->`.
- [x] **T4 Editorial shorthand with attributes.** A `Span` whose **first** class is one of `quarto-insert`, `quarto-delete`, `quarto-highlight`, `quarto-edit-comment`, and whose other classes include none of them, is written as `[++ …]`, `[-- …]`, `[!! …]` or `[>> …]` followed by `{…}` holding the remaining id, classes and attributes (nothing when none remain). It must be the first class because the reader puts the mark class first (verified: `[++ x]{#i1 .foo author="A"}` reads as classes `["quarto-insert","foo"]`), so a span with the mark later would not round-trip; such spans keep the generic form, as do spans with two editorial classes. `write_div` (`qmd.rs:664-681`) already follows this rule for block marks; mirror it. Match the attribute formatting the `Inline::Insert`/`Delete`/`EditComment` writers use (`qmd.rs:2791-2850`). Cases:
  - each mark with `author` and `date`;
  - a comment nested in a plain span with attributes (the I4 shape);
  - a mark with an id;
  - a mark with an extra class;
  - a span whose mark class is not first (generic form, round-trips);
  - the attribute-free form, unchanged.

  Add `editorial_marks_with_attributes.qmd`-style cases to the qmd-json-qmd round-trip suite as well.
- [x] **T5 Regression sweep.** Run the existing pampa suites: the qmd-json-qmd round trips, the incremental writer tests (`incremental_write_qmd` is called by the hub), and the writer snapshots. Any changed snapshot must be explained in the Handoff log: an intended escape, footnote or list change is fine; anything else is a bug. Then re-run the research folder's `losses.md` probe through the docx path and record the new qmd in the Handoff log.

### Verification

- [x] `cargo clippy -p pampa --all-targets -- -D warnings`, `cargo nextest run -p pampa` green after each task (and the same for `quarto-core` after T2).
- [x] Workspace nextest at the plan boundary; delta against T0's baseline accounted for: new tests, intended snapshot changes.
- [x] hub-client's wasm-backed suites that exercise the qmd writer (search for `incremental_write_qmd` / `ast_to_qmd` callers) still pass.

### Close-out

- [x] Checklist reconciled, committed; rebased and fast-forwarded into `feature/hub-import`. **Epic Progress is not ticked here** (epic file is edited only by the orchestrator): tick P2.

## Handoff log

Append-only.

- 2026-10-03: plan written. Not started.
- 2026-10-03: revised after the implementability review (epic status line): mechanism pinned (AST pre-pass in both write loops, line-start flag); span-then-( escape; T2 sizing.
- 2026-10-03: revised after the angle review: the pre-pass is copy-on-trigger (no clone per keystroke on the incremental path). All of P2's `qmd.rs` and `test.rs` anchors verified exact.
- 2026-10-03: T0 done. Workspace nextest baseline at `b19cb4464`-based `feature/hub-import` (ddb8eec22), before any P2 change: **15732 run, 15732 passed, 202 skipped** (`cargo nextest run --workspace --no-fail-fast`, 1864 s with a cold build). Harness: `crates/pampa/tests/integration/qmd_writer_pandoc_shapes.rs` (builders for pandoc-shaped JSON, `trip`/`assert_roundtrip`/`assert_roundtrip_with`/`write_qmd`; `remove_location_fields` in `test.rs` made `pub(crate)`). A qmd the reader rejects is returned as a failed round trip, not a panic, so table-driven tests list every failing case.
- 2026-10-03: T1 done. Before the fix, failing (of 25 line-start texts, per context): paragraph start 13 (`1. x`, `1) x`, `12. x`, `- x`, `+ x`, `: x`, `::: x`, bare `1.`/`-`/`+`/`:`/`:::`), after soft break and after line break 11, list item and block quote starts 13, and all five non-`1.5` cases inside `Emph` (`*a\n- x*` is a Q-2-12 parse error). Span then `(see)` re-read as a Link. Differences from the plan's probe list: `===` (alone, or with text) after a soft break already round-trips, so no setext escape was added; the writer's existing escapes cover `*`, `|`, backticks, `~`, `#`, `>` and dash runs. Fix, in `qmd.rs`: `at_line_start` is set true by `write_paragraph`/`write_plain` and false by `write_block` and by every inline that opens with a delimiter; `write_inline_run` tells each `Str` whether a boundary follows (`next_is_boundary`); `line_start_hazard` picks the char to escape (`-`, `+`, `:`, `:::`, `N.`, `N)`); a `LastByteWriter` in `write_inline` records the last byte written so a `Str` starting with `(` after `]` is escaped. All modes read the flag, not only fragment mode. `cargo clippy -p pampa --all-targets -- -D warnings` clean; `cargo nextest run -p pampa` 4905 passed, 2 skipped.
- 2026-10-03: T2-T4 done, in one commit (the three share hunks in `qmd.rs`, which can't be split without interactive staging); gated together: `cargo clippy -p pampa -p quarto-core --all-targets -- -D warnings` clean, `cargo nextest run -p pampa` 4929 passed / 2 skipped, `-p quarto-core` 5603 passed / 32 skipped.
  - **T2.** New `crates/pampa/src/writers/qmd_prepass.rs`: a read-only scan (`TriggerScan`) decides, `prepare()` returns `Cow::Borrowed` unless a multi-block `Note` or adjacent `OrderedList`s exist, then clones and rewrites (`Rewriter`). Called from `write_impl` and `write_impl_tracked`; `write_single_block`/`write_single_inline` don't run it (code comment in the module doc). Own read-only and `&mut` walkers (the filter traversal in `filters.rs` consumes its input, so it can't scan). Ids `n1`, `n2`, … skip every attr id, note reference/definition id and `reference-id` in the document. Definitions go after the top-level block with that block's source info; a note inside a note gets its own top-level definition (ids in document order). An empty `Note` keeps its inline form. Oracle: `crates/quarto-core/tests/integration/qmd_writer_footnotes.rs` (12 tests), real `FootnotesTransform` under `Format::docx()`; it compares native text, allowing `Plain`→`Para` in notes and the two spellings of an empty table caption (`Caption Nothing [  ]` from pandoc JSON vs `[]` from the qmd reader). Findings: (a) `FootnotesTransform` doesn't resolve a reference inside a definition body, so a note inside a note has no oracle; the test checks the writer's output shape instead (not a Word or pandoc construct). (b) A multi-block note in **metadata** keeps today's inline form with paragraphs joined by a space: a docx title with such a note imports as `title: "Title^[first second]"` (asserted in `a_note_in_metadata_keeps_the_inline_form`). The docx path for P1's fixtures wasn't re-run yet (T5).
  - **T3.** Bullets: `write_block` takes the previous sibling's marker (`ctx.prev_bullet_marker`) and alternates `*`/`-` (`ctx.bullet_marker`), threaded to `BulletListContext`, the bare-marker and `[]` forms. Ordered: the pre-pass inserts `RawBlock html "<!-- -->"`; the oracle allows exactly that block (`with_list_separators`). Grammar/reader search for an AST-free ordered separator: not done beyond the plan's verified candidates (changing the delimiter changes the AST; `<!-- -->` is the only lossless one found), so `<!-- -->` stays. **Tight-item limit:** in a tight list item (blocks written with no blank line between them) the separator is swallowed as text on the preceding list line (`1. x\n<!-- -->` re-reads as `Plain [x, SoftBreak, RawInline]`), so the pre-pass loosens such an item (its first `Plain` becomes a `Para`, which makes the list loose). The text survives; the test allows Plain/Para. Separately, pre-existing and not touched: a loose item whose blank lines carry the item's indent (`  `) re-reads its first block as `Plain` when a nested list follows, so `[Para, OrderedList]` in a list item doesn't round-trip today, with or without P2.
  - **T4.** `write_span` mirrors `write_div`: first class an editorial mark and no second one gives `[++ …]{rest}`. New qmd-json-qmd fixture `editorial_marks_author_date.qmd` (the reader makes `Insert`/`Delete`/… nodes for these, so it guards the node writers; the span shape is covered by the pandoc-shapes tests).
- 2026-10-03: T5 and plan boundary.
  - **Regression sweep.** The full pampa suite (qmd-json-qmd round trips including the new fixture, incremental writer tests, writer snapshots) passes with **no snapshot or expected-output file changed**: `git diff --stat` shows no `.snap` or fixture edit apart from the new `editorial_marks_author_date.qmd`. The only red found by the sweep was outside pampa: `qmd-syntax-helper` `grid_tables_test::test_converts_grid_table` (the first workspace run after rebasing). Cause: the bullet-marker alternation (T3) leaked across list items, so the second row of a list-table (a bullet list inside each item of an outer bullet list) took `-`. Fixed with `QmdWriterContext::begin_block_sequence`, called where a container starts a new sequence of blocks (bullet/ordered items, pipe-table cells, definitions, captions, list-table cell blocks); regression test `the_bullet_marker_does_not_alternate_across_list_items`. Attributed from the plan's task list (T3 is the only task that changes output for lists; `qmd-syntax-helper` consumes the writer).
  - **Workspace nextest** at the plan boundary, on the tree rebased onto `feature/hub-import` `c198585b9` (P1 stage 1 had landed since the baseline was taken): **15793 run, 15793 passed, 202 skipped**. Baseline at T0 (`ddb8eec22`): 15732 passed, 202 skipped. Delta **+61 passed, +0 skipped**, accounted for by name (comparing the two logs\' test lists): P2 +49 (31 `pampa::integration qmd_writer_pandoc_shapes`, 12 `quarto-core::integration qmd_writer_footnotes`, 3 `qmd_prepass::tests` in each of pampa's lib and bin test binaries) and P1 stage 1 +12 (6 `xtask capture_import_recordings::tests`, 6 `quarto-core::integration pandoc_request_contract`), nothing removed. `cargo clippy --workspace --all-targets -- -D warnings` clean.
  - **hub-client** (`npm install`, `node scripts/fetch-pandoc-wasm.mjs --require`, `npm run build:wasm`, then): `npm run test:wasm` 39 files, 328 passed, 28 skipped (the skips are the recording-parity comparisons that need `quarto-output-extract`, and the smoke-all WASM list's own skips; the suites ran, they weren't skipped for missing assets); `npm run test:integration` 23 files, 169 passed. `npm install` rewrote `package-lock.json`; I reverted that.
  - **`losses.md` through the docx path** (`pandoc losses.md -o x.docx`, `pandoc -f docx -t json`, `pampa -f json -t qmd`), new qmd: the two multi-paragraph notes now come out as `[^n1]`/`[^n2]` with `::: ^n1`/`::: ^n2` definitions (the docx reader flattens the note's list into two paragraphs, `item one` / `item two`, as the epic says); the restarted ordered lists are `1.  alpha one`, `2.  alpha two`, `<!-- -->`, `4.  roman four`, `5.  roman five` (style lost as I17 says); the two bullet lists read back as one `* list A item` / `* list B item` list, because pandoc's docx writer made one list of them, so the docx has no adjacent-list case from this source. The `\\1. not a list` line is the literal-backslash source issue P1 corrects.
  - **Finding for the orchestrator (I18's reachability claim).** The same probe's definition list comes out as `Term\n:   Definition para one.\n\n    Definition para two continues here.`, which the qmd reader rejects (Q-2-35, indented code block): so the multi-block definition-list indentation bug *is* reachable through docx when the docx carries pandoc's "Definition"/"Definition Term" styles (pandoc-made docx; Word users who apply those styles). The epic says it isn't reachable and keeps it out of P2 (I18). I left it out and changed nothing; Gordon may want a follow-up (braid strand) or to widen I18. Not blocking P3.
  - **Known limits, no action taken:** (1) an incremental rewrite of one list via `write_single_block` can't see its neighbours, so a rewritten bullet list next to an unchanged one is written with `*` as before this plan (unchanged behaviour); (2) a note inside a note: see T2; (3) T1's `===` setext hazard from the plan's probe list round-trips already, so no escape was added.
  - **Epic edits needed (orchestrator):** tick P2 in Progress; add the definition-list reachability finding above to the "qmd writer bugs reachable from docx" section or to a Follow-on.
- 2026-10-03: landed. `feature/hub-import` advanced twice during the plan (P1 stage 1, then P0 at `f7457d1b1`); this branch was rebased onto each (no conflicts). P0's changes are TypeScript and plan files only (no `.rs`, `.toml` or `Cargo.lock` change between the tree the workspace nextest ran on and `f7457d1b1`), so the counts above stand: no Rust changes, counts unchanged. Fast-forwarded `feature/hub-import` to this branch's tip. Not pushed, no PR.

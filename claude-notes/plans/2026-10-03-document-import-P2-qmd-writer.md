# Plan: qmd writer fixes for imported documents (document import P2)

**Date:** 2026-10-03
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
- **An AST pre-pass** run before writing, in **both** top-level loops: `write_impl` (`qmd.rs:3254`) and `write_impl_tracked` (`:3307`, the incremental writer the hub calls). It handles T2 and the ordered-list half of T3. It rewrites a multi-block `Note` into a `NoteReference` inline plus a `NoteDefinitionFencedBlock` inserted after the containing top-level block, so the existing `write_notereference` (`:2423`) and `write_fenced_note_definition` (`:1171`) do the writing. It also inserts `RawBlock (Format "html") "<!-- -->"` between adjacent `OrderedList`s, which the existing raw-block path writes. Inserted blocks take the source info of the block they follow, so `write_impl_tracked`'s tiling still holds. Both loops take `&Pandoc` and the incremental writer runs on every hub edit, so the pre-pass is **copy-on-trigger**: a read-only scan first, and a clone only when it finds a multi-block `Note` or adjacent `OrderedList`s (never in a qmd-read AST). A test asserts that an AST with neither is written byte-identically to today. `write_note` (`:2393`) is an inline writer and can't emit a block, which is why this isn't done there. `write_single_block` / `write_single_inline` use fresh contexts and don't run the pre-pass: a multi-block `Note` there keeps today's inline form. qmd-read ASTs can't contain one, so this is unreachable from the hub's edits; say so in a code comment.
- **The line-start flag** for T1: `ctx.at_line_start` is already updated after every inline (`qmd.rs:2912`), but it starts `false` and only fragment mode reads it (`:1955`). Set it `true` at each block start (paragraph, plain, a list item's and a block quote's first line), and read it in `write_str` in all modes. The look at the next inline ("followed by `Space`, a break or the end") is a shared helper over the inline slice, used by every container loop (top level and inside `Strong`/`Emph`/`Span`/`Link`). A `SoftBreak` inside `Emph` followed by `- x` must be caught too, and `write_prose_inlines` (`:3026`) splits only top-level breaks.
- Bullet markers (T3) are threaded through a context field, not a pre-pass: the pre-pass can't change a marker, which isn't in the AST.

## Checklist

### Tasks

- [ ] **T0 Baseline and harness.** Record the workspace nextest baseline (pass/skip counts) in the Handoff log. Add `crates/pampa/tests/integration/qmd_writer_pandoc_shapes.rs` (registered in `tests/integration/main.rs`, alphabetical) with a table-driven helper `assert_roundtrip(pandoc_json)` implementing the oracle, and a second helper for the cases where the oracle allows a known difference (T3's ordered-list separator).
- [ ] **T1 Line-start escaping.** Add table-driven cases for text at a paragraph start **and** after `SoftBreak` and `LineBreak`. Constructs to cover:
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
- [ ] **T2 Multi-block footnotes.** Built as the pre-pass in "The mechanism". Sizing: two to three days, in separate commits (the pre-pass and id scan, the `write_impl_tracked` integration, the quarto-core oracle). The id scan can use `pampa::filters::topdown_traverse*` or a hand-written walk. A `Note` whose content is exactly one `Para` or `Plain` stays inline `^[…]` (no churn). Any other Note is written as a reference `[^n<k>]` at the note's position plus a fenced definition
  ```
  ::: ^n<k>
  <blocks>
  :::
  ```
  placed after the top-level block that contains the reference. Ids `n1`, `n2`, … are assigned in document order, skipping any id already used in the document (note ids and element ids).

  **The oracle needs the footnote transform.** Verified 2026-10-03: the pampa reader does not turn `[^n1]` plus `::: ^n1` back into a `Note`. It produces `Span ("", ["quarto-note-reference"], [("reference-id","n1")]) []` and a `NoteDefinitionFencedBlock` block, and `pampa -t native` refuses them (Q-3-11, "coalescing … not yet implemented"). quarto-core's `FootnotesTransform` (`crates/quarto-core/src/transforms/footnotes.rs`) resolves both forms into `Inline::Note` for every output format, including the hub preview and pandoc downloads. So T2's cases live in **quarto-core's** integration tests (`crates/quarto-core/tests/integration/qmd_writer_footnotes.rs`): write with pampa, re-read, run the real `FootnotesTransform`, then compare with the input. Do not reimplement the resolution in a test helper. Three details the oracle needs:
  - `FootnotesTransform` prepends a ref-id `RawBlock` marker to each resolved Note and strips it only when `ctx.pipeline_profile` is `PipelineProfile::Pandoc(_)` (`footnotes.rs:136-138`). Build the `RenderContext` with a Pandoc profile, following the footnotes.rs unit tests' `make_test_project` pattern.
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
- [ ] **T3 Adjacent lists.**
  - Bullet lists: when a `BulletList` directly follows another `BulletList`, alternate the bullet character (`*` / `-`). This is lossless (verified, including three in a row: `* A`, `- B`, `* C` read as three lists). `*` is hardcoded in `BulletListContext::new` and in `write_bulletlist` (`qmd.rs:736-790`, the bare `*` and the `* []` empty-item forms); thread the marker through them as a context field set by the block loop, which knows the previous sibling.
  - Ordered lists: when an `OrderedList` directly follows another, the pre-pass inserts a `<!-- -->` raw block between them ("The mechanism"). It re-reads as `RawBlock (Format "html") "<!-- -->"` (verified), so the oracle for this case allows exactly that inserted block.
  - Check the same within nested contexts (a list item ending in a list followed by another list, block quotes, divs).
  - First search the grammar and reader for a construct that separates ordered lists without an AST node; record the result. Use it if one exists; otherwise keep `<!-- -->`.
- [ ] **T4 Editorial shorthand with attributes.** A `Span` whose **first** class is one of `quarto-insert`, `quarto-delete`, `quarto-highlight`, `quarto-edit-comment`, and whose other classes include none of them, is written as `[++ …]`, `[-- …]`, `[!! …]` or `[>> …]` followed by `{…}` holding the remaining id, classes and attributes (nothing when none remain). It must be the first class because the reader puts the mark class first (verified: `[++ x]{#i1 .foo author="A"}` reads as classes `["quarto-insert","foo"]`), so a span with the mark later would not round-trip; such spans keep the generic form, as do spans with two editorial classes. `write_div` (`qmd.rs:664-681`) already follows this rule for block marks; mirror it. Match the attribute formatting the `Inline::Insert`/`Delete`/`EditComment` writers use (`qmd.rs:2791-2850`). Cases:
  - each mark with `author` and `date`;
  - a comment nested in a plain span with attributes (the I4 shape);
  - a mark with an id;
  - a mark with an extra class;
  - a span whose mark class is not first (generic form, round-trips);
  - the attribute-free form, unchanged.

  Add `editorial_marks_with_attributes.qmd`-style cases to the qmd-json-qmd round-trip suite as well.
- [ ] **T5 Regression sweep.** Run the existing pampa suites: the qmd-json-qmd round trips, the incremental writer tests (`incremental_write_qmd` is called by the hub), and the writer snapshots. Any changed snapshot must be explained in the Handoff log: an intended escape, footnote or list change is fine; anything else is a bug. Then re-run the research folder's `losses.md` probe through the docx path and record the new qmd in the Handoff log.

### Verification

- [ ] `cargo clippy -p pampa --all-targets -- -D warnings`, `cargo nextest run -p pampa` green after each task (and the same for `quarto-core` after T2).
- [ ] Workspace nextest at the plan boundary; delta against T0's baseline accounted for: new tests, intended snapshot changes.
- [ ] hub-client's wasm-backed suites that exercise the qmd writer (search for `incremental_write_qmd` / `ast_to_qmd` callers) still pass.

### Close-out

- [ ] Checklist reconciled, committed; rebased and fast-forwarded into `feature/hub-import`; epic Progress ticked.

## Handoff log

Append-only.

- 2026-10-03: plan written. Not started.
- 2026-10-03: revised after the implementability review (epic status line): mechanism pinned (AST pre-pass in both write loops, line-start flag); span-then-( escape; T2 sizing.
- 2026-10-03: revised after the angle review: the pre-pass is copy-on-trigger (no clone per keystroke on the incremental path). All of P2's `qmd.rs` and `test.rs` anchors verified exact.

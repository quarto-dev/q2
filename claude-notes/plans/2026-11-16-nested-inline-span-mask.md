---
title: 'Mask an executable span shown inside a code span'
date: 2026-11-16
description: 'Extends the nested-cell mask to inline code, so a double-backtick code span that contains an executable span is displayed instead of run (bd-unt56ts1).'
status: complete
---

**Strand:** bd-unt56ts1 (branch `braid/bd-unt56ts1-inline-expr-in-code-span`, workspace-4)
**Related:** [EXR-18](2026-10-28-exr-18-docs-e2e.md), the plan whose docs verification found the bug.

> **For implementers:** TDD. Write the tests in Phase 1, run them, and confirm each red test
> fails for the stated reason before starting Phase 2. Tests marked *guard* pass before the
> fix by design.

## Goal

An author can document an executable span in running prose by wrapping it in a code span:

````markdown
Write `` `{python} x` `` to show the value of `x`.
````

Today Jupyter and knitr evaluate the inner span, and the page shows `<code>2</code>`. Julia
already leaves it literal. After this plan all three engines leave it literal, and the page
shows the span exactly as written.

## Decisions

Decisions 1 to 6 were agreed with Gordon on 2026-11-16. Decisions 7 and 8 came out of the
design review the same day.

1. **Only the nested form is masked.** A code span whose parsed text contains an executable span
   (for example the text `` `{python} 1+1` ``). A plain span (the parsed text `{python} 1+1`)
   still runs.
2. **The no-inner-backtick forms stay evaluated, and the docs say so.** The qmd writer
   normalizes ``` ``{python} 1+1`` ``` and ```` `` {python} 1+1 `` ```` to a plain span before the
   engine scans, so q2 evaluates both. Quarto 1 with Jupyter leaves both literal (measured
   2026-11-16). This is a deliberate divergence, and the Phase 3 callout states it.
3. **No doubled-brace escape.** Quarto 1\'s `` `{{r}} x` `` is not implemented, for inline
   expressions or for cells. Wrapping the span in a code span is the q2 syntax.
4. **Same rewrite as blocks.** `MASK_INLINE_RE` inserts the `q2-nested-executable` marker after
   the span\'s opening backtick, and `unmask` deletes it. `unmask` does not change.
5. **Two predicates, one walker.** The body and the document\'s own metadata mask when the
   `Code` text contains a span (the unwrapped match, `NestedOnly`). Foreign metadata masks when
   the whole code span is a span (the existing wrapped match) or when its text contains a span
   (`WholeOrNested`). One walker built on `ast_walk.rs` covers every inline container,
   replacing the hand-written list in `mask_inlines` and the `Paragraph`/`Plain`-only match in
   `mask_foreign_meta`.
6. **Provenance.** A changed top-level block is stamped `Generated`, as for blocks, so every
   offset in it maps to "unknown", including a real span next to the example and its error
   line and column. Accepted. Metadata has no top-level block: that path stamps nothing, so
   offsets after a masked front-matter line shift by the marker length. Foreign metadata
   already does this. Error-line mapping is the only consumer.
7. **Own `!str` metadata is out of scope.** Untagged own values parse to `PandocInlines`, so
   `NestedOnly` covers them. An own `!str` value is a raw string, and `MASK_INLINE_RE` cannot
   tell a nested span from a plain one in a raw string. The docs do not promise it.
8. **Known limitations, documented, not fixed.**
   - An author who writes `q2-nested-executable` verbatim gets it rewritten by `unmask` (the
     block mask\'s existing limitation).
   - Two spans with no character between them leave the second unmasked. Jupyter and q2\'s
     `INLINE_R_PATTERN` also skip the second span. knitr\'s own pattern has no backtick guard
     and evaluates it, so under knitr a three-backtick outer span holding two adjacent classic
     `r` spans runs the second one (reproduced 2026-11-16). The module comment at
     `nested_cell_mask.rs:126-129` says the property is shared with Q1 and
     `INLINE_R_PATTERN`; reword it so it does not claim knitr.

## Phase 1: tests first

- [x] **1.1 Unit tests** in `crates/quarto-core/src/engine/nested_cell_mask.rs` (`mod tests`).
  Parse the single-space form `` `` `{python} x` `` ``; the writer normalizes extra padding.
  - [ ] a paragraph with the repro form is masked: the `Code` text gains the marker, and the
    block is stamped `Generated`. Assert the behavior on the writer\'s `SourceInfo`
    (`map_offset` is `None`, as T15 and T17 do). The index `mask` returns is an API assertion
    only: every caller discards it;
  - [ ] a plain `Code` span with text `{python} 1+1` is **not** masked (*guard*);
  - [ ] the classic `` `r x` `` nested form is masked, and a plain `r x` is not;
  - [ ] the nested form is masked at every position the engine evaluates today. Parse these
    from qmd: `Header`, `Emph`, `Link` text, `Image` alt text, a table cell, a table caption,
    a list item, a block quote, an inline `^[...]` note, a `[^1]:` footnote definition, a
    figure caption, and the title and body of a callout and a tabset heading (these are Custom
    nodes with inline slots);
  - [ ] AST-constructed cases, for positions the parser does not produce from qmd: `LineBlock`,
    `DefinitionList`, a `Cite` with a prefix and a suffix, CriticMarkup inlines, and
    `Caption.short` on a table and a figure;
  - [ ] `unmask(writer(masked))` equals `writer(original)` byte for byte for each of the above.
    The comparison is against the writer\'s normalized output, not the author\'s bytes;
  - [ ] own metadata: a nested form in an untagged `title:` is masked; a plain span in the
    document\'s own metadata is not (*guard*);
  - [ ] foreign metadata: build a `PandocInlines` leaf holding a nested `Code` and a
    `PandocBlocks` leaf holding it in a non-`Paragraph` block. A parsed string takes the
    scalar arm and passes today, so it does not exercise the gap. Confirm the new cases fail
    first; the existing whole-span case still masks (*guard*).
- [x] **1.2 Integration tests** in `crates/quarto-core/tests/integration/nested_cell_mask_render.rs`
  (`t25`/`t26` are the model: `render_html`). Each renders through `render_document_to_file`,
  not `render_qmd_to_html`. The file has only `knitr_render_available`; add a Jupyter helper
  modeled on `python3_kernel_available` and `engine_available` in
  `tests/integration/capture_splice_engines.rs:34-46`. CI installs a Jupyter venv.
  - [ ] Jupyter (`engine: jupyter`, `jupyter: python3`): the repro renders
    `<code>`{python} 1+1`</code>` with no marker anywhere, and a plain span in the same
    paragraph still renders its value;
  - [ ] knitr, brace spelling and classic `r` spelling, both nested;
  - [ ] an `include`d file holding a nested example renders it literal;
  - [ ] a nested example in the document\'s own `title:` renders literal in `<title>` and the
    title block;
  - [ ] the marker appears nowhere in the HTML, and not in a `--to markdown` render either.
    The capture\'s `input_qmd` keeps the marker on purpose (`engine_execution.rs:784`), so
    do not assert on it;
  - [ ] Julia (*guard*, passes before and after): the document needs a live `{julia}` cell and
    `execute: daemon: false`, as in `julia_engine_e2e.rs`; without them a plain span stays
    literal and the test shows nothing. The repro stays literal, and a plain span still runs.
    Add a second document whose only top-level block is a Div holding a nested example and
    the live cell, to cover the `sourceRanges` starvation (bd-quydz82t). `julia_available()`
    needs `QUARTO_JULIA_PROJECT`, and no CI workflow sets it, so this test skips in CI. Say so
    in the PR;
  - [ ] a document with only an example and no executable cell or other span still renders
    unchanged (*guard*, as `control_display_block_with_no_live_cell_renders_unchanged`);
  - [ ] an error in a live span in a paragraph **next to** a masked one still reports its
    line and column (*guard*).
- [x] **1.3 Invariant.** Keep `mask_inline_pattern_covers_every_scanned_span`
  (`crates/quarto-core/src/engine/units.rs:812`) as a pattern guard and add the nested
  spelling to it. It passes today and cannot go red, because it tests the regex and not the
  traversal. Add a second test in `nested_cell_mask.rs`: for every `Code.text` that contains a
  unit-scanner span, `mask` fires. That one fails if the walker misses a container.
- [x] **1.4** Ran each new test before the fix (2026-11-16). Results:
  - Unit tests (`nested_inline::*`): 9 red, 3 *guard* green. Every red test fails because `mask`
    does not fire (`mask` returns `[]`, `mask_foreign_meta` returns `false`), which is the
    stated reason: `repro_paragraph…`, `classic_r_nested…`, `nested_form_is_masked_at_every_position…`
    (first failure: `header`), `every_code_with_a_scanner_span_is_masked` (`header`),
    `ast_constructed_positions…` (`line block`), `caption_short…`,
    `code_block_inside_inline_note…`, `own_metadata_nested…`, `foreign_inlines_and_blocks…`.
    Green by design: `plain_span_is_not_masked`, `foreign_whole_span_still_masks`.
  - Integration tests: `jupyter_shows_the_span…`, `jupyter_include_and_title` (the title
    renders `T 2 x`), `markdown_output_has_no_marker` (the output is ``Write `2` here.``) and
    `knitr_brace_and_classic_spellings` (fails on the brace spelling first, evaluated) are red
    because the span is evaluated. Green by design: Julia, the no-live-cell control, and the
    error-location guard (`nested_err.qmd:9:19`, measured before the fix).
  - The `units.rs` pattern guard passes with the nested spelling added.

## Phase 2: implementation

- [x] **2.1** One inline walker in `nested_cell_mask.rs`, taking a `Mode` (`NestedOnly`,
  `WholeOrNested`), built on `crates/quarto-core/src/ast_walk.rs` (`block_children_mut`,
  `inline_children_mut`, `for_each_inline_list_mut`, `for_each_meta_inline_list_mut`). It
  exposes lists, not blocks, so stamp only the top-level ancestor; `mask()` already does that
  from the `changed` flag. It must reach what `mask_inlines` and `mask_block` miss today:
  `Cite` (content and `Citation` prefix and suffix), `Image`, `Note`, `NoteDefinitionPara`,
  `CaptionBlock`, `Caption.short`, the CriticMarkup inlines, and `Custom` inline slots.
  `mask_custom` currently skips `Inline` and `Inlines` slots; fix it and its comment, since
  callout titles and tabset headings live there. Decide whether recursing into `Inline::Note`
  newly masks `CodeBlock`s inside inline footnotes. The default is yes, matching block
  footnotes; assert it in a test.
- [x] **2.2** The body pass: `mask_block` runs the walker over every inline list it reaches and
  stamps `mark_generated` on the top-level ancestor.
- [x] **2.3** Metadata: `mask_foreign_meta` runs `WholeOrNested` on foreign leaves and
  `NestedOnly` on the parsed inlines of own leaves (it returns `false` for own leaves today).
  Own raw-string leaves stay untouched (decision 7). `PandocBlocks` goes through the same
  walker instead of the `Paragraph`/`Plain` match.
- [x] **2.4** Update the doc comments: the module doc, the `mask_inlines` comment, the
  `mask_custom` comment, the `unmask` note that a masked inline expression carries no
  `{.lang ...}` shape, and the adjacency limitation at `:126-129` (decision 8).

## Phase 3: docs

- [x] **3.1** `docs/guides/authoring/computations.qmd`, "Writing about an inline expression"
  (around lines 424-453): lead with the code span, using the single-space form, then the
  `markdown` block for a whole line. State the rule: a code span with two or more backtick
  delimiters, whose content contains a span, is displayed.
- [x] **3.2** The "Migrating from Quarto 1" callout in the same section (lines 455-463): the
  code span replaces the doubled-brace escape, as the `markdown` block does for cells. Add
  the divergence: Quarto 1 with Jupyter evaluates the nested form and leaves
  ``` ``{python} 1+1`` ``` and ```` `` {python} 1+1 `` ```` literal, while Quarto 2 displays the
  nested form and evaluates both of the others.
- [x] **3.3** Render the docs with `cargo run --bin q2 -- render docs/` (not `quarto`) and
  inspect the example\'s output. `migrating-markdown.qmd` is not edited: it covers differences
  from Pandoc, not from Quarto 1 execution.

## Phase 4: verification

- [x] **4.1** `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core`.
- [x] **4.2** `cargo xtask lint`.
- [x] **4.3** End to end through the `q2` binary for Jupyter, knitr (both spellings), and Julia,
  recording here the invocation, the output snippet, and that the output was inspected.
- [x] **4.4** `q2 render --strict claude-notes/plans/2026-11-16-nested-inline-span-mask.md`.
- [x] **4.5** (in the pre-push summary) Recommend, not run, a wider `nextest -p` on `quarto-core`\'s direct dependents
  (`cargo tree -i quarto-core`) in the pre-push summary. In the PR notes, say that preview
  records for a document with a nested example go stale once, because the capture\'s
  `input_qmd` keeps the masked bytes.
- [ ] **4.6** (pending Gordon\'s approval) Close bd-unt56ts1 with a comment naming the commits; reconcile this checklist
  against what landed first.

## Results

- **Implementation notes (deviations from the plan text).**
  - The walker is `InlineMasker` (a `ListVisitor`), with `Mode::{NestedOnly, WholeOrNested}`.
    `mask_block`, `mask_inlines`, `mask_custom` and the `Paragraph`/`Plain` match are gone.
  - An auto-generated `Header` id is recomputed after its text is masked. Without that the
    writer prints the stale id as `{#title-python-x}`, and `unmask` cannot remove it.
    Found by the Phase 1 round-trip test.
  - A masked pipe-table cell widens its column, so after `unmask` the padding and delimiter
    dashes differ from the writer's output for the original. The table cases compare with
    whitespace and dashes collapsed. Every consumer derives both sides of its comparison
    from the same masked serialization, so this is immaterial there (`record_splice.rs`).
  - Step 2.1's default holds: a `CodeBlock` inside an inline note is masked
    (`code_block_inside_inline_note_is_masked`).
- **Gate (2026-11-16).** `cargo clippy -p quarto-core --all-targets -- -D warnings` clean;
  `cargo nextest run -p quarto-core`: 8477 passed, 36 skipped (the Jupyter and Julia
  integration tests ran, not skipped); `cargo xtask lint` clean. No `.snap` file changed.
- **Windows cross-check could not run.** `cargo clippy ... --target x86_64-pc-windows-msvc`
  fails in a build script that compiles C for the target (`tree-sitter`, via `cc`). The new
  code uses no platform API.
- **Docs.** `cargo run --bin q2 -- render docs/` rendered 430 files (33 warnings, none from
  `computations.qmd`). Inspected `docs/_site/guides/authoring/computations.html`: the
  demonstration paragraph shows ``<code>`{r} release_version`</code>``, the callout shows both
  Quarto 1 forms as written, and no marker appears.
- **End to end (4.3).** `target/debug/q2 render <fixture>.qmd`, fixtures in the session
  scratchpad (`e2e/{j,kb,kc,ju}.qmd`), each reading ``Write `` `{...} ...` `` to show it, and
  a plain span runs.``. Output inspected, `grep -c q2-nested` is 0 for each:
  - Jupyter: `<p>Write <code>`{python} 1+1`</code> to show it, and 42 runs.</p>`
  - knitr brace: `<p>Write <code>`{r} v`</code> to show it, and 42 runs.</p>`
  - knitr classic: `<p>Write <code>`r v`</code> to show it, and 42 runs.</p>`
  - Julia (`QUARTO_JULIA_PROJECT=~/Library/Caches/quarto/julia`, `daemon: false`, live cell):
    `<p>Write <code>`{julia} 1+1`</code> to show it, and 42 runs.</p>`

## Details

- **Why the mask and not a scanner fix.** The classic `` `r x` `` spelling goes to knitr
  unrewritten (EXR-2 decision 2) and knitr scans the raw text. Only the mask changes the text
  before knitr sees it.
- **Why the writer needs no change.** The parser strips the padding space, and `write_code`
  (`crates/pampa/src/writers/qmd.rs`, `write_code`) re-adds one space whenever the text starts
  or ends with a backtick, and picks a delimiter longer than the longest backtick run. The
  single-space repro form round-trips byte for byte (checked 2026-11-16 with `pampa -t qmd`;
  13 shapes round-tripped through `unmask`); a form with two spaces of padding is normalized to
  one. The masked text still starts with a backtick, so the padding is unchanged.
- **Where an executable span is evaluated today** (measured 2026-11-16, Jupyter): body
  paragraph, `Header`, link text, `Image` alt text, table cell and caption, both footnote
  forms, figure caption, callout title and body, tabset heading, `Span`, `Strong`, `Quoted`,
  list item, block quote, an `include`d file, and the document\'s own `title:`. Shortcode
  arguments are not a path (a code span there is a parse error), and `LineBlock`,
  `DefinitionList` and CriticMarkup do not parse to their own nodes from qmd.
- **Measured 2026-11-16** with a uv-created venv and `target/debug/q2 render` (Jupyter, python3
  kernel): `` `{python} 1+1` `` and both double-backtick no-inner-backtick forms render `2`; the
  nested form renders `<code>2</code>`.
- **Marker leakage.** A path that skips `unmask` would show the marker in output. These call
  it: `engine_execution.rs:746` and `:893`, `record_splice.rs:279` and `:298-301`, and
  `quarto-hub-provider/src/execute.rs:763`. These keep the masked bytes on purpose:
  `compute_input_qmd` (`preview_record.rs:454`) and the capture `input_qmd`
  (`engine_execution.rs:784`). The Phase 1 tests assert no marker in the HTML.

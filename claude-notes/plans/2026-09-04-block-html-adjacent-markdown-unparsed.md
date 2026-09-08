# Fix: markdown adjacent to a raw HTML block is emitted verbatim

Strand: `bd-block-html-adjacent-markdown-unparsed-0qnjuwuy`
Exploration: `claude-notes/research/2026-09-03-block-html-adjacent-markdown-unparsed.md`
Regression from: PR #646 / `bdacd1122`

## Goal

A paragraph opening with a block-level HTML tag is currently lifted to a
single **verbatim** `RawBlock`, which switched off inline markdown parsing
inside it — pandoc's `markdown_in_html_blocks`. Keep the lift and its extent,
but split the paragraph so tag runs stay raw and the intervening text goes
back through the inline parser.

Target AST, matching `pandoc -f markdown-native_divs`:

```
<div class="x">          RawBlock html "<div class=\"x\">"
text with `code`    ->   Plain [ ... Code ... ]
</div>                   RawBlock html "</div>"
```

## Decisions (Gordon, 2026-09-04)

- **Match pandoc on round-trip.** A block-level html `RawBlock` writes
  **verbatim**, not as a ` ```{=html} ` fence. This reverses PR #646's
  deliberate normalisation to the documented spelling; the round-trip
  stability it buys is worth more. No adjacency rule needed.
- **Q-2-9 diagnostic change is fine.** *(Correction, after review: there is no
  diagnostic change. Q-2-9 is emitted per `html_element` node in
  `treesitter.rs`, which this branch does not touch, and the count is still 2
  for an open/close pair — verified against `main`. What changed is the
  `RawBlock.text` that `test_warnings` asserted on. The decision was sound but
  the thing being decided did not exist.)*
- **`Plain` always for split interiors** (recommended, not pandoc-exact).
  Pandoc's `Plain`/`Para` choice needs open-element tracking, which
  `dev-docs/syntax-notes.md` rejects. Every resulting difference is a `<p>`
  wrapper — the already-accepted `native_divs` gap. Documented, not chased.

## Phase 1 — Tests first

- [x] Reader: tight `<div>` splits to RawBlock/Plain/RawBlock with parsed `Code`/`Emph`
- [x] Reader: `<details>`/`<summary>` shape from the real bug report
- [x] Reader: leading-whitespace classifier — `<!-- a --> <!-- b --> <!-- c -->` stays one run
- [x] Reader: raw-text exemption — `pre`, `script`, `style`, `textarea` stay one verbatim RawBlock
- [x] Reader: tag-only paragraph still emits exactly one RawBlock (PR #646 unchanged)
- [x] Reader: each split part carries its own non-overlapping `SourceInfo`
- [x] Writer: block-html RawBlock writes verbatim; round-trip is byte-stable and reparses identically
- [x] Writer: round-trip inside a block quote (the incremental-writer failure)
- [x] Verify all the above fail before implementing

## Phase 2 — Reader

- [x] Raw-text-element guard (first-inline check) in `html_block.rs`
- [x] Split into `RawBlock`/`Plain` runs in `paragraph.rs`; return `IntermediateSection`
- [x] Classify on `text.trim_start()`
- [x] Per-part `SourceInfo` from child nodes, not cloned from the paragraph
- [x] Single-part result still returns one `IntermediateBlock`

## Phase 3 — Writer

- [x] `write_rawblock`: block-html html RawBlock verbatim (generalises the `is_html_comment` case)
- [x] `Plain` is tight — suppress the inter-block blank line at **every** block container
- [x] Retire `naked_block_round_trips_as_an_explicit_raw_fence` (decision above)

## Phase 4 — Reconcile existing expectations

- [x] `test_html_block_lift` — three tests asserting merged behaviour
- [x] `test_warnings::test_block_level_html_elements` — asserts the split tags,
      not a changed diagnostic (see the correction above)
- [x] `incremental_writer_tests::roundtrip_comment_in_blockquote`
- [x] Snapshot review (expect ~1); report count + summary per CLAUDE.md
- [x] Document the `Plain`/`Para` and `native_divs` divergences

## Phase 5 — Verify

- [x] `cargo clippy -p pampa --all-targets -- -D warnings`
- [x] `cargo nextest run --workspace` — 13684 passed / 199 skipped / 0 failed.
      Baseline 13676 passed; +8 is exactly the tests added here.
- [x] `cargo xtask lint`
- [x] `cargo xtask verify` — full run (not `--skip-hub-build`), all 14 steps pass,
      including the WASM/hub-client leg, since pampa is in that dependency chain
- [x] Both repros green simultaneously
- [x] End-to-end through the `q2` binary; inspect output (see below)

## Writer separator rule — measured, not assumed

The first attempt made `Plain` tight on both sides. That is *not* pandoc's
rule, and it removed a blank line between a `Table` and a following `Plain`,
reddening `quarto-core::llms_txt::llms_companion_rich_content_snapshot`. The
workspace run caught it; the per-crate pampa run did not.

Pandoc's markdown writer, measured over a 5x5 block matrix:

| prev | next | separator |
| --- | --- | --- |
| RawBlock | Plain | **tight** |
| Plain | RawBlock | **tight** |
| RawBlock | RawBlock | **tight** |
| everything else | | blank line |

We take the first two and deliberately **not** the third. Our reader's HTML
block extent is the paragraph, so two adjacent `RawBlock`s came from two
separate paragraphs; the blank line between them is what keeps them separate,
and writing them tight would merge them into one raw run on the next read.
The `RawBlock` side is further narrowed to blocks `write_rawblock` emits bare,
so a ```` ```{=html} ```` fence is never pulled tight against its neighbour.

## End-to-end verification

    $ q2 render index.qmd --to html

on the reported shape (`<details>`/`<summary>` then prose with no blank line):

    <details>
    <summary>
    Example custom instructions
    </summary>
    This example demonstrates how a <code>quarto.instructions.md</code> file shapes Positron
    Assistant behavior for anything ending with <code>.instructions.md</code>.

Output inspected: the code spans are parsed and no literal backticks remain
(`grep -c` for the backticked forms returns 0). Both repro fixtures are green
simultaneously, and q2 now matches Quarto 1 on all four rows of the regression
repro.

The trailing `</details></p>` in that same render is **pre-existing** — it is
byte-identical on 0.28.0, 0.29.0 and `main`, and is PR #646's documented
mid-paragraph gap. Filed as bd-495qnexy, linked `discovered-from` this strand.

## Snapshots

Two modified, both the same class: `html-comment-17-comment-at-line-start` and
`html-comment-44-comment-starts-at-block-boundary`. A comment followed by text
on the same line now parses as `RawBlock` + `Plain` instead of one verbatim
`RawBlock`. Rendering was checked before accepting: neither the old nor the new
output carries the `<p>` that Quarto 1 emits there, so this is not a
regression — the comment simply moves onto its own line, closer to Q1.

## Review round (2026-09-04)

A reviewer subagent over `914f20697..d7a6eb067` found three real defects. All
were reproduced through the binary before being fixed, and each has a
regression test.

**Critical — `Inline::LineBreak` was not treated as a separator.** Trailing
spaces on a tag line are invisible in the source but make the reader emit a
hard break rather than a `SoftBreak`. It fell into the content run, so
`<div class="x">··\ntext\n</div>` rendered `<br />text`, and
`<div>···\n</div>` invented a `Plain` holding the break alone (printing a
stray backslash). Both were new regressions on the default render path — `main`
renders them correctly. `LineBreak` now joins `Space`/`SoftBreak` in both the
separator arm and the trailing-pop. q2 is byte-identical to pandoc on both.

**Important — the bare-writing test was far too broad.** `writes_bare_html`
de-fenced *any* `RawBlock` starting with a block tag, including author-written
```` ```{=html} ```` fences. Their contents were then parsed on re-read: a
backticked word became `Code`, and a `#` line after a blank line became a real
`Header` — content corruption through any AST -> qmd -> read cycle. The comment
claiming "exactly the texts written bare are the ones the reader lifts back"
was false for anything but a pure tag run.

`round_trips_bare_html` now tests the whole text, not its first tag: bare is
allowed only for the two shapes the lift actually produces — a pure run of
block tags (`line_is_only_tags` on every line, quoted attribute values
skipped), or a raw-text element whose interior the reader leaves unparsed —
and never when a blank line would end the block. The writer and the blank-line
rule now share one predicate, `html_writes_bare`, so they cannot drift.

**Important — `write_orderedlist` was missed.** It has its own block loop,
separate from `write_bulletlist`'s, so an ordered item's split interior came
back as a `Paragraph` while a bullet item's stayed `Plain`. The plan's claim of
"every block container" was wrong. Fixed.

### Containers: the complete list, and what needs nothing

`IntermediateSection` is spliced at exactly six sites — `treesitter.rs:340`
(list item), `section.rs`, `document.rs`, `block_quote.rs`,
`fenced_div_block.rs`, `note_definition_fenced_block.rs` — so a split can only
land in: the document (`write_impl` and `write_impl_tracked`), a block quote, a
fenced div, a bullet-list item, an ordered-list item, and a fenced note
definition. All seven writer loops now apply the rule; sections flatten into
the document.

Two sites the reviewer flagged were checked and deliberately need no change:

- **`write_definitionlist`** — definition lists are not a splice site, so a
  lifted paragraph cannot reach one. (Separately, a definition's block content
  does not survive the reader at all today; that is pre-existing and identical
  on `main`, including for plain prose, so it is not this change's concern.)
- **the metadata block loop** — a front-matter value stays a YAML scalar and
  never becomes `PandocBlocks` holding a lifted paragraph; verified with an
  `abstract:` holding a tight `<div>`.

### Follow-ups taken from the review's Minor findings

- **`span_of`** no longer clones the run to compute a span (it takes the
  `SourceInfo`s directly), refuses to union offsets across different files, and
  `debug_assert!`s on the no-`Original` arm. That arm is unreachable — this runs
  in the reader, before any filter — but if a change made it reachable, two
  adjacent runs would fall back to the paragraph span and get the overlapping
  siblings the function exists to prevent. Better loud than quiet.
- **`tag_name`** is now factored out of `starts_block_html` and shared with
  `starts_raw_text_element`, which had been a second copy missing the
  tag-name-boundary check (`starts_raw_text_element("<pre-wrap>")` was true;
  unreachable behind the `starts_block_html` guard, but a live drift hazard).
  It also carries a closing-tag flag, so `</pre>` correctly does *not* start a
  raw-text exemption — CommonMark's type-1 start condition is the opening tag.
  Both behaviours now have tests.
- **Tests added** for the raw-text exemption inside a block quote, and for the
  closing-tag case above.

### Two source reflows worth naming

Neither changes the AST, but both change bytes on an AST -> qmd write:

- A blank line between a `Plain` and a following bare `RawBlock` is dropped:
  `<div>\ntext\n\n</div>` comes back as `<div>\ntext\n</div>`. The parse is
  identical either way.
- A `<pre>` whose content contains a blank line is exempt only for its *first*
  paragraph; the rest is parsed as markdown. Pre-existing — the extent has
  always been the paragraph — but it has a visible consequence now that the
  interior of everything else is parsed. Noted in `dev-docs/syntax-notes.md`.

## Second review, and the removal of the tight rule (2026-09-04)

A second review found the writer's tightness rule unsound. Three defects, all
reproduced through the binary:

- **A `Plain` was pulled tight against a following raw-text element from a
  *different* paragraph.** `<script>` then no longer *began* a paragraph, so
  the exemption did not fire and its JavaScript was parsed as markdown —
  `` const t = `hi`; `` became `const t = <code>hi</code>;` after one cycle.
- **A comment containing `>`** defeated `line_is_only_tags`, which forced a
  fence, which restored a blank line, which let `</div>` collapse back into a
  paragraph — the `<p><div>` shape PR #646 exists to prevent.
- **Filter-produced blocks were merged.** `[Plain, RawBlock, Plain, RawBlock]`
  written and re-read came back as a single `Paragraph` holding both tags as
  inlines.

### Why it was unsound, not merely incomplete

Pandoc's tight rule is safe because *pandoc's* `Plain` in this position means
"content inside an HTML block that is still open" — its reader guarantees it.
Ours does not: we emit `Plain` for every split interior, and a filter can emit
one anywhere. So the writer, seeing `Plain` next to a `RawBlock`, cannot tell
whether they came from one paragraph or two, and that question has no answer in
the AST. We had borrowed pandoc's writer rule without the reader invariant that
makes it sound.

That is a design fault rather than a missing clause, and the earlier plan text
claiming the rule was "measured, not guessed" was measuring the wrong thing: the
5×5 matrix recorded *what pandoc does*, not *why it may*.

### What replaced it

`needs_blank_line_between` and `writes_bare_html` are gone; every container loop
is back to separating blocks unconditionally. The fence-versus-bare decision
stays — it is what stops an authored ```` ```{=html} ```` block being stripped
and its contents parsed — and `line_is_only_tags` now scans a comment to `-->`
rather than to the first `>`.

**The cost, stated plainly:** a split interior comes back as a `Paragraph`
rather than a `Plain`, gaining a `<p>`. For `<div>` that matches Quarto 1. For a
phrasing-only element like `<summary>` it is invalid HTML — but only after a
write/read cycle, never on first render. Filed as bd-8md6k9dv.

Three alternatives were measured and rejected:

- **Keep the fence instead of writing bare** — identical `Paragraph` outcome,
  uglier source. Strictly worse.
- **Emit `Paragraph` from the reader instead of `Plain`** — round-trip becomes
  perfectly stable and `<div>` matches Q1 byte-for-byte, but every `<summary>`
  gets an invalid `<p>` on *first render*, which is worse than getting one only
  after an edit.
- **Recover provenance from source spans** — fragile for filter-produced and
  transformed blocks.

### Source reflows, in full

Writing the AST back out is parse-stable but not byte-stable. The reflows, so
the next reader is not surprised:

- a split interior returns as a `Paragraph`, gaining a `<p>` (see above);
- several block tags on one line are re-emitted one per line —
  `<div><section>` becomes two lines, from the `join("\n")` in `flush_raw`;
- text sharing a line with a tag moves to its own line — `<div>text` becomes
  `<div>` then `text`;
- leading indentation is dropped — `  <div>` becomes `<div>`;
- a blank line between a `Plain` and a following bare `RawBlock` is preserved
  now that nothing is written tight, so the earlier note about it being dropped
  no longer applies;
- a `<pre>` whose content spans a blank line is exempt only for its first
  paragraph — pre-existing, since the extent has always been the paragraph.

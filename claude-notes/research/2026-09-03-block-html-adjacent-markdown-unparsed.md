# bd-block-html-adjacent-markdown-unparsed-0qnjuwuy — exploration and recommendation

Investigation only. No fix implemented; the spikes described below were
measured and reverted.

## Summary

The strand's diagnosis is **correct**, and its suggested fix direction is
**sound but understated**. The reader-side change is small and I verified it
fixes the regression while keeping PR #646's guard green. Two costs the
strand did not identify make the total job medium, not small:

1. A **qmd-writer companion change** is mandatory, not optional. Without it
   the split destabilises round-tripping (`Plain` comes back as `Paragraph`)
   and breaks the incremental writer used by the hub editing path.
2. **Exact pandoc parity on the `Plain`/`Para` distinction is not reachable**
   with a local rule. It needs open-element tracking — the thing
   `dev-docs/syntax-notes.md` rejects. The recommendation is to *not* chase it.

Plus one measured exception class the strand did not have: `<pre>`,
`<script>`, `<style>`, `<textarea>` must keep the current verbatim
behaviour.

## 1. The diagnosis holds — verified, not taken on faith

Reproduced at HEAD (`914f20697`), not just at the 0.29.0 release build.

The strand's central measurement is exactly reproducible:

| input `-f …` | `<p>` wrapper | inline markdown |
| --- | --- | --- |
| `markdown` | yes | parsed |
| `markdown-native_divs` | no | parsed |
| `markdown-native_divs-markdown_in_html_blocks` | no | **literal** |
| q2 HEAD | no | **literal** |

q2's HTML is byte-identical to row 3. The conflation claim is confirmed.

I also confirmed it **at the AST level**, which the strand did not do and
which is the stronger evidence:

```
pandoc -f markdown-native_divs                          -> RawBlock, Plain, RawBlock
pandoc -f markdown-native_divs-markdown_in_html_blocks  -> RawBlock (one, verbatim)
q2 HEAD                                                 -> RawBlock (one, verbatim)
```

A fourth data point sharpens *why* the two extensions were easy to conflate:
`-f markdown-markdown_in_html_blocks` (dropping only that extension, keeping
`native_divs`) still parses the markdown and still emits `<p>`. The two
extensions interact — `markdown_in_html_blocks` only has an observable effect
once `native_divs` is off, which is precisely the configuration PR #646 put
q2 into. Dropping `native_divs` alone was safe; dropping it *and* lifting
verbatim was not.

`syntax-notes.md`'s objection is specifically to *parsing HTML into AST
structure* via backtracking parser combinators — i.e. `native_divs`. It does
not speak to `markdown_in_html_blocks`. The strand is right that the
constraint does not block this fix.

## 2. What it takes — spiked and measured

**Reader side (small).** The inline content is *already parsed* when the lift
happens: `process_paragraph` builds a full `Vec<Inline>` and then throws it
away in favour of re-reading the source bytes. The fix is to partition that
existing vector into runs — block-level-HTML `RawInline`s become `RawBlock`s,
everything else becomes `Plain` — and return
`IntermediateSection(Vec<Block>)`, which every block container already
splices (`treesitter.rs:340`, `document.rs`, `section.rs`, `block_quote.rs`,
`fenced_div_block.rs`, `note_definition_fenced_block.rs`).

This needs **no tag matching**: the predicate is per-inline and already
exists (`starts_block_html`). The paragraph boundary already fixes the
extent. Confirmed: the spike produced an AST identical to
`pandoc -f markdown-native_divs` for the tight `<div>` case.

Two reader-side details the spike surfaced:

- `RawInline.text` for a *non-first* inline carries **leading whitespace**
  (`" <!-- second -->"`), so `starts_block_html` must be applied to
  `text.trim_start()`. Without this, `<!-- a --> <!-- b --> <!-- c -->` on
  one line splits wrongly. The existing `paragraph_starts_block_html` never
  hit this because it only inspects the first inline.
- Each split block needs its **own** `SourceInfo` derived from the child
  nodes. Cloning the paragraph's `SourceInfo` onto every part (what the spike
  did) gives sibling blocks identical overlapping ranges and confuses the
  incremental writer.

**Writer side (the real cost, and mandatory).** `write_rawblock` emits a
non-markdown `RawBlock` as a ```` ```{=html} ```` fence, and every block
container unconditionally emits a blank line between blocks. So a split trio
round-trips to:

````
```{=html}
<div class="case-b">
```

Case B text with a `code span` and *emphasis*.

```{=html}
</div>
```
````

which re-parses as `RawBlock, **Para**, RawBlock` — a `Plain`→`Para` drift.
That is exactly the `incremental_writer_tests::roundtrip_comment_in_blockquote`
failure. Today's merged behaviour round-trips *structurally stable* (one
`RawBlock` → one fence → same `RawBlock`), so the split makes round-tripping
strictly worse unless the writer is changed with it.

Pandoc round-trips the same AST **byte-identically**. Its rule is entirely
AST-derivable: write an html `RawBlock` verbatim, and write `Plain` with no
blank line after it — which is what `Plain` *means*. I spiked both changes
and the q2 round-trip closed completely: byte-identical source, identical
reparse.

The cost is that the blank-line suppression must be applied at **every** block
container, not just `write_impl`. My spike patched only the top level, which
is why the blockquote test still failed.

**Conflict to decide.** The writer change contradicts a deliberate PR #646
choice: `test_html_block_lift::naked_block_round_trips_as_an_explicit_raw_fence`
asserts that naked HTML normalises to the documented ```` ```{=html} ````
spelling. Writing it verbatim reverses that. These can be reconciled — a
`RawBlock` adjacent to a `Plain` in a tight run writes bare, an isolated one
stays fenced, and that is decidable from the AST — but it is a design call,
not a mechanical edit.

## 3. Where exact parity stops being reachable

This is the part that changes the shape of the fix, and it argues *against*
the strand's implied ambition.

Pandoc's choice between `Plain` and `Para` for the interior is **not**
expressible as a local rule. Measured:

| input | pandoc blocks |
| --- | --- |
| `<div>\ntext\n</div>` | RawBlock, **Plain**, RawBlock |
| `<div>\nunclosed text` | RawBlock, **Para** |
| `<div class="x"> text after` | RawBlock, **Para** |
| `<div>\ntext\n</div>\nmore` | RawBlock, Plain, RawBlock, **Para** |
| `<div>\ntext\n</div>\nmore\n<div>\nagain\n</div>` | RawBlock, Plain, RawBlock, **Plain**, RawBlock, Plain, RawBlock |

The same text `more` is `Para` in one and `Plain` in the other, decided by
what follows it. And `<div>\ntext\n</div>\nmore` ends the block at the
balanced `</div>` — under plain CommonMark type-6 semantics it would run to
the blank line. Pandoc is tracking open elements.

So: **the strand's claim that the fix needs no balanced tag matching is right
for the split itself, but wrong if the goal is exact pandoc parity.** Chasing
the `Plain`/`Para` distinction means reimplementing pandoc's HTML block
parser, which is what `syntax-notes.md` rejects.

**This does not matter for the reported bug.** The bug is literal backticks;
`Plain` vs `Para` only decides a `<p>` wrapper. Against **Quarto 1** — the
actual porting baseline — the spike matches on *every* markdown-parsing
question:

```
                        Quarto 1                       q2 spike
<div class="s1">   inside <code>code</code>       inside <code>code</code>
<div class="s2">   text ... <code>code</code>     text ... <code>code</code>
<details>/<summary> Sum <code>code</code>          Sum <code>code</code>
nested divs        nested <code>code</code>       nested <code>code</code>
<pre>              raw `code` *em*  (literal)      raw `code` *em*  (literal)
```

Every residual difference is a `<p>` wrapper, i.e. the already-accepted
`native_divs` gap. **Recommendation: emit `Plain` unconditionally for split
interiors and document the divergence.** Do not track open elements.

## 4. Blast radius

**The `<pre>` exception is real and the strand did not have it.** Measured
across the whitelist: `pre`, `script`, `style`, `textarea` — CommonMark's
HTML block type 1 raw-text set — keep their content **verbatim** in pandoc.
All four are in q2's `BLOCK_TAGS`. A blanket split would newly break them by
parsing markdown inside a `<script>`. Every other tag probed (`title`,
`iframe`, `noscript`, `svg`, `canvas`, `object`, `section`, `table`/`tr`/`td`,
`div`, `details`) splits. So the ~60-tag whitelist partitions cleanly 4 / 56,
and the guard is a first-inline check — no state needed.

**Test blast radius, whole workspace, both spikes applied:**

```
baseline (branch HEAD, clean)  13676 tests run: 13676 passed, 0 failed, 199 skipped
with both spikes applied       13676 tests run: 13669 passed, 7 failed, 199 skipped
```

The run count is identical, so the spike added no tests and every failure is
an existing test changing behaviour — a clean delta of exactly 7.

All 7 in `pampa`; nothing downstream (`quarto-core`, hub, WASM untouched):

| test | nature |
| --- | --- |
| `test::unit_test_snapshots_json` | 1 snapshot, `html-comment-17` |
| `test_html_block_lift::details_summary_becomes_one_raw_block_verbatim` | asserts merged behaviour |
| `test_html_block_lift::details_renders_without_paragraph_wrappers` | asserts merged behaviour |
| `test_html_block_lift::container_prefixes_are_stripped_from_a_multi_line_block` | asserts merged behaviour |
| `test_html_block_lift::naked_block_round_trips_as_an_explicit_raw_fence` | **design conflict** (§2) |
| `test_warnings::test_block_level_html_elements` | Q-2-9 quotes `<div>` not `<div>content</div>` |
| `incremental_writer_tests::roundtrip_comment_in_blockquote` | **real breakage** — writer, per-container |

Only one snapshot moves, far less churn than PR #646's 27.

Note `test_warnings`: Q-2-9's diagnostic currently quotes the whole lifted
block. After a split it quotes just the tag. Worth deciding deliberately —
the tag alone is arguably the better diagnostic.

## 5. Recommendation

Do it, in this order, TDD per repo rules:

1. Reader: partition the already-parsed inlines into `RawBlock`/`Plain` runs;
   return `IntermediateSection`. Trim leading whitespace before
   `starts_block_html`. Give each part its own `SourceInfo`.
2. Guard the type-1 raw-text set (`pre`/`script`/`style`/`textarea`) with a
   first-inline check; those keep today's verbatim path.
3. Writer: make `Plain` tight and lifted block-html `RawBlock`s verbatim, at
   **every** block container. Resolve the fence-normalisation conflict by
   keeping the fence for an isolated `RawBlock` and writing bare only within
   a tight run.
4. Accept `Plain`-always for interiors; document the `Plain`/`Para` and
   `native_divs` divergences alongside the existing "no naked HTML" notes.
5. Regression tests from both repro fixtures; both must stay green together.

**Open design question for Gordon** (§2): should naked HTML still normalise
to a ```` ```{=html} ```` fence on round-trip, as PR #646 deliberately chose?
Preserving it needs the adjacency rule in step 3; dropping it is simpler and
matches pandoc, but reverses an intentional decision.

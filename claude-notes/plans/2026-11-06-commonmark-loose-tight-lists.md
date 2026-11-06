---
title: "CommonMark parity for loose vs tight lists"
date: 2026-11-06
description: "Spec study, a generated conformance corpus checked against a CommonMark oracle, and a rewrite of the loose/tight decision in the CST-to-AST traversal."
status: in-review
braid:
  strand: bd-az53ojvo
  priority: P2
---

Strand: `bd-az53ojvo`. PR: <https://github.com/quarto-dev/q2/pull/1098>. Related: `bd-25ezfgs8` (whitespace-only separator
lines make lists tight), `bd-i4xrcdqx` (whitespace-only blank line before
`:::` in a list item makes the list tight), `bd-2gc9` (closed: nested tight
lists marked loose), `bd-nrywksil` (preview parity for list items).

## Overview

**Goal.** pampa decides whether a list is loose or tight the way the
CommonMark spec does, for every input in the subset of CommonMark that qmd
accepts, plus qmd-only block constructs (fenced divs, display math) treated
as ordinary block-level elements.

**Where the behavior lives.** The tree-sitter grammar
(`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`) has no notion of
loose or tight. It produces `list`, `list_item`, `block_continuation` and
`_blank_line` nodes, and the decision is made while walking the concrete
tree in `crates/pampa/src/pandoc/treesitter.rs`:

- `process_list` (line 102) accumulates a `has_loose_item` flag over the
  items and, when the list is tight, rewrites the *first* `Paragraph` of
  each item to `Plain`.
- `process_list_item` (line 289) builds each item's blocks and asks
  `list_item_has_blank_line_between_blocks` (line 610) whether a blank
  line separates two of them. That helper looks for a
  `block_continuation` node *inside a `pandoc_paragraph`* that spans more
  than one row.

The qmd writer (`crates/pampa/src/writers/qmd.rs`, around line 785) goes
the other way: a list is written tight when every non-empty item starts
with `Plain`. So the reader's Plain/Para choice is also what the writer
relies on for round-trips.

**Method.** Study the spec (this document), build a generator that
produces list documents covering every blank-line placement the spec
distinguishes, compare pampa's list shape against a CommonMark oracle,
bucket the disagreements, then rewrite the decision in `process_list`
against the spec's two-clause definition and re-run the corpus until it is
clean. Keep the corpus as a regression test.

## What the spec says

File: `external-sources/commonmark-spec/spec.txt` (CommonMark 0.31.2). Line
numbers below are into that file; example numbers are the spec's sequential
numbering (the `spec_examples` parser in
`crates/comrak-to-pandoc/tests/integration/commonmark_spec.rs` numbers them
the same way).

### The definition (Lists, lines 5238-5267)

A list is **loose** if either

1. any of its constituent list items are separated by blank lines, or
2. any of its constituent list items *directly* contain two block-level
   elements with a blank line between them.

Otherwise the list is **tight**. The only output difference is that
paragraphs in a loose list are wrapped in `<p>`. In Pandoc AST terms (both
`pandoc -f commonmark` and the in-tree `comrak-to-pandoc` converter agree):
in a tight list, *every* `Para` that is a direct child of an item becomes
`Plain`, not just the first one. Nested lists decide for themselves:
the outer list and a sublist can differ (examples 323 and 327).

Consequences the definition spells out, each with a spec example:

| Example | Lines | Input shape | Verdict | Why |
|---|---|---|---|---|
| 316 | 5594-5614 | `- a\n- b\n\n- c` | loose | blank line between items |
| 317 | 5617-5634 | `* a\n*\n\n* c` | loose | the empty item still counts as an item; a blank after it separates items |
| 318 | 5637-5660 | `- a\n- b\n\n  c\n- d` | loose | one item directly contains two blocks with a blank between |
| 319 | 5664-5680 | `- b\n\n  [ref]: /url` | loose | the ref def is a block-level element even though it renders nothing |
| 320 | 5684-5704 | blank lines inside a fenced code block in an item | tight | the blanks are *inside* one block, not between two |
| 321 | 5708-5730 | `- a\n  - b\n\n    c\n- d` | outer tight, inner loose | the blank is between two blocks of the *sublist's* item |
| 322 | 5733-5750 | `* a\n  > b\n  >\n* c` | tight | the blank is inside the block quote |
| 323 | 5753-5775 | `- a\n  > b\n  ```\n  c\n  ```\n- d` | tight | consecutive blocks with no blank line between them |
| 324, 325 | 5777-5800 | `- a` and `- a\n  - b` | tight | single paragraph, or paragraph plus sublist with no blank |
| 326 | 5802-5820 | code fence, blank, `bar` in one item | loose | two blocks with a blank between (neither is a paragraph) |
| 327 | 5822-5840 | `* foo\n  * bar\n\n  baz` | outer loose, inner tight | blank between sublist and paragraph in the outer item |
| 328 | 5843-5868 | `- a\n  - b\n  - c\n\n- d\n  - e\n  - f` | outer loose, inner tight | blank between two outer items whose last block is a sublist |

### Blank lines (lines 3646-3650)

Blank lines between block-level elements are ignored *except* for the role
they play in deciding loose vs tight. A blank line is a line containing
only spaces and tabs (line 338), so a whitespace-only line is blank.
Container prefixes are stripped before this test: inside a block quote,
a line that is `>` followed by spaces is a blank line of the quote's
content (example 322).

### List items (lines 4119-5050): which lines belong to an item

The looseness clauses only talk about blank lines *between* items and
*between an item's direct children*, so the item rules decide what those
regions are:

- **Rule 1, basic case (4136-4157).** Content indented to the content
  column (marker width plus 1-4 spaces) belongs to the item; after a blank
  line, a line that is less indented ends the item and the list (examples
  256-262, 4180-4300). Blank lines after the *last* item therefore belong to
  no item and do not make the list loose: `- a\n- b\n\nfoo` is tight.
- **Rule 1, exception (4150).** A list that interrupts a paragraph may not
  start with a blank line.
- **Rule 2, indented code (4330).** An item starting with indented code may
  have exactly one blank line... out of scope here, qmd rejects indented
  code (Q-2-59).
- **Rule 3, item starting with a blank line (4601-4660).** `-\n  foo` is a
  one-item list whose content is `foo`; the blank line after the marker is
  part of how the item starts, not a separator, so examples 280-281 are
  tight. An item can begin with at most one blank line: in `-\n\n  foo`
  the item is empty and `foo` is a separate paragraph (example 282).
- **Lists, 5242 and 5404.** Items may be separated by *any number* of blank
  lines (example 307, 5404-5420, loose). The pre-0.26 "two blank lines end
  a list" rule is gone from the spec. The grammar still has a
  "closed after two consecutive blank lines" branch in `_list_item_content`
  (grammar.js line 950); whether that is observable needs a corpus case.
- **Lazy continuation and sublist indentation (4860-4990).** Decide which
  lines are inside an item; they affect looseness only indirectly. Several
  of these fail to parse in pampa today (see baseline).

### Terms the oracle must agree on

The spec defines the HTML. For an AST comparison we need the Pandoc
representation, which both oracles share: tight list means every direct
`Para` of every item becomes `Plain`; a loose list keeps `Para`. Empty
items are `[]` in both.

## Current implementation and known deviations

`process_list` uses four signals, in this order per item:

1. the previous item was a single paragraph whose end row differs from this
   item's start row;
2. this item's start row is greater than the previous item's end row
   (taken from the last block's `source_info` end offset);
3. this item has more than one `Paragraph` block;
4. this item has more than one block, at least one a `Paragraph`, and the
   `block_continuation`-based helper found a multi-row continuation inside
   a paragraph.

Deviations found so far:

- **Clause 3 is wrong.** Two paragraphs separated by a non-paragraph block
  with no blank lines are tight per the spec. Confirmed on 2026-11-06:
  `- a\n  # h\n  b\n- c` gives `Para, Header, Para` in pampa and
  `Plain, Header, Plain` in `pandoc -f commonmark` (and comrak).
- **Tight conversion rewrites only the first paragraph.** Both oracles
  rewrite every direct `Para`. Same repro as above.
- **Whitespace-only blank lines are missed** (bd-25ezfgs8, bd-i4xrcdqx).
  The helper requires the blank to be absorbed into a `block_continuation`
  inside the paragraph; a whitespace-only line splits that node and the
  blank lands in a sibling `block_continuation` between the blocks, which
  the helper never looks at. The writer-side mitigation in PR #899 only
  stopped the qmd writer from *producing* such lines.
- **Blank line between items whose last block is not a paragraph.**
  Example 328 (last block is a sublist) comes out tight; signal 2 should
  catch it, so the end row it computes for a sublist is probably inflated
  by the trailing blank line.
- **Empty item followed by a blank line.** Example 317 comes out tight.
- **Blank-line detection does not look inside sublists correctly in both
  directions**: `bd-2gc9` fixed the false-loose case; the false-tight case
  (example 328) remains.

Baseline survey on 2026-11-06, all 655 spec examples, comparing the
recursive list shape (block kinds, `Plain` vs `Para`) of
`target/debug/pampa -t json` against `pandoc -f commonmark -t json`
(pandoc 3.12.1), script kept in the session scratchpad and to be re-created
under `scripts/` in Phase 1:

| Bucket | Count | Examples |
|---|---|---|
| reference has a list, shapes agree | 54 | |
| looseness mismatch | 2 | 317, 328 |
| other shape mismatch (not looseness) | 6 | 4, 7 (tabs), 42 (precedence), 295 (lazy continuation in nested quote), 302 (setext underline), 319 (ref def) |
| pampa parse error | 18 | 5, 256, 259, 266, 272, 273, 275, 276, 280, 281, 282, 288, 289, 290, 292, 311, 314, 315 |
| no list in the reference | 575 | |

The parse errors are indented code (Q-2-59), tabs, one-space sublist
indentation (314), and items starting with a blank line (280-282). Those
are grammar-level gaps, not looseness bugs; they bound the subset the
generator may produce.

## Approach

### Oracle

Primary, in-process, runs in CI: comrak through `comrak_to_pandoc`
(`pampa` already depends on both; `pampa::readers::commonmark::convert`
wraps them). comrak passes the spec test suite and `convert_list_item` in
`crates/comrak-to-pandoc/src/block.rs` applies the all-paragraphs Plain
rule.

Secondary, dev-time only: `pandoc -f commonmark -t json`. Used once per
corpus revision to confirm that the comrak oracle agrees with pandoc on the
list shape of every generated document, so a comrak or converter bug cannot
silently become our target. Not a CI dependency.

Comparison is on a **list shape signature**, not AST equality: the block
kinds of the document recursively, with list items as nested sequences, so
`Plain` vs `Para` inside items is what is compared and the known inline and
attribute differences between pampa and comrak (heading ids, `uri` class,
smart typography, Figure wrapping) do not matter. Attributes, inlines and
source info are dropped from the signature.

### Generator

A small grammar of list documents, driven two ways: exhaustive enumeration
at small size, and proptest sampling with shrinking at larger size.

Dimensions, each a spec-motivated axis:

- **Marker**: `-`, `*`, `+`, `1.`, `1)`; start numbers other than 1 for
  ordered lists.
- **Marker spacing**: 1 to 3 spaces after the marker (4 is still basic
  case; 5 or more means indented code and is excluded).
- **Item count**: 1 to 3 at the top level.
- **Blocks per item**: 0 (empty item), 1, 2 or 3 direct children drawn
  from: paragraph (one or two lines), fenced code block (0 to 2 blank lines
  inside), block quote (optionally with a blank `>` line), ATX heading,
  nested list (depth up to 2 below the top), fenced div (qmd-only), display
  math block (qmd-only).
- **Separator between an item's blocks**: none, one blank line, two blank
  lines, a spaces-only line, a spaces-and-tab line, an indented blank line
  (whitespace to the content column).
- **Separator between items**: the same set.
- **Item start**: marker then content, or marker then blank line then
  content (rule 3), or marker alone (empty item).
- **After the list**: end of input; one or two blank lines then a
  paragraph; a non-indented line immediately after (ends the list).
- **Container**: top level, inside a block quote, inside a fenced div.

Exclusions, because qmd rejects or does not implement them and the oracle
would disagree for reasons unrelated to looseness: indented code blocks,
tabs for indentation (tabs only appear inside a whitespace-only line),
lazy continuation lines, setext headings, link reference definitions, HTML
blocks, CRLF (bd-qmtp61ms).

qmd-only blocks (div, display math) are not CommonMark, so comrak parses a
`:::` line as a paragraph. For documents containing them the check is
**metamorphic**: pampa's list shape for the document must equal pampa's
shape for the same document with each qmd-only block replaced by a block
quote of the same line count, and that substituted document is checked
against the oracle directly. This covers bd-i4xrcdqx's shape.

Size budget: exhaustive enumeration with at most 2 items, at most 2 blocks
per item, depth at most 2, one container choice, gives on the order of
tens of thousands of documents; each parse is sub-millisecond, so this
runs in CI under a few seconds. The proptest layer samples the full
grammar with a case count from an environment variable (default 256 in
CI, larger locally), and its shrinker gives minimal counterexamples.

### Harness layout

- `crates/pampa/tests/integration/list_looseness.rs`, registered in
  `main.rs` (alphabetized; no new top-level test file per
  `.claude/rules/integration-tests.md`). Contains the signature function,
  the generator, the enumerated and proptest tests, and the spec-example
  test (reusing the `spec_examples` parser from `commonmark_spec.rs`,
  moved to a shared helper or copied).
- A **known-divergence list** inside the test, keyed by a stable
  description of the generated case, so cases that fail for reasons outside
  looseness (a grammar gap filed as its own strand) do not block the
  looseness check. Each entry names the strand that owns it. The test
  fails if a known entry starts passing, so the list cannot go stale.
- `scripts/commonmark-list-survey.py`: the pandoc cross-check, taking a
  directory of generated `.md` files (the test can dump the corpus with an
  environment variable) and printing the bucketed comparison. Also runs the
  spec examples, reproducing the baseline table above.

### Fix strategy

Replace the four signals in `process_list` with the spec's two clauses,
computed from source rows rather than from `block_continuation` node
shapes:

1. **Separated items.** For consecutive items *i* and *i+1*, loose if any
   row strictly between the content end row of item *i* and the start row
   of item *i+1* is blank after stripping container prefixes.
2. **Internal blank.** For consecutive direct children *k* and *k+1* of an
   item, loose if any row strictly between the content end row of *k* and
   the start row of *k+1* is blank in the same sense.

"Content end row" is the last row that carries the block's own text: the
last line of a paragraph, the closing fence of a code block or div, the
last row of the last item of a nested list (recursively), the last row of
a block quote's content. The first implementation step is to establish
whether tree-sitter node ranges or the AST `source_info` ranges already
give this, or include trailing blank lines (the example 328 failure
suggests the latter for lists). If they include trailing blanks, compute
it from the node's last named descendant that is not a
`block_continuation` or `_blank_line`.

"Blank after stripping container prefixes": a row whose text, with every
leading run of spaces, tabs and `>` removed, is empty. Within a list, a
row strictly between two blocks of the same item or between two items can
only be a real blank line or a container-prefix-only line, so this test is
sound there. The rows come from the source text already held by the
`ASTContext` source context (the same text `map_offset` consults).

Then the tight rewrite applies to every direct `Paragraph` of every item.

Fixing in the traversal also resolves the whitespace-only line bugs
without a scanner change, provided the parse attaches the blocks to the
same item in both cases (bd-i4xrcdqx says it does for a paragraph and
needs checking for a div). Grammar and scanner changes are in scope when a
corpus case shows the grammar attaches a block to the wrong item and the
fix is proportionate to the failure (Decision 2); otherwise the case
becomes its own strand with the corpus case as its repro. Rule-3 items
(`-\n  foo`) and the two-blank-lines branch are the two known candidates.

### Round-trip and downstream

The qmd writer emits tight or loose from the Plain/Para shape, so the
corpus doubles as a round-trip property: `read(write(read(doc)))` has the
same signature as `read(doc)`. Add it as a second test over the same
generator. `test_indented_continuation::attributed_div_is_transparent_to_list_indentation`
pins the current whitespace-only behavior as "repro A" vs "control B"
(bd-25ezfgs8) and must be updated to expect loose in both.

Snapshot flips to expect and triage: `crates/pampa/tests/pandoc-match-corpus`
(`markdown/023.qmd`, `024.qmd` mention tightness), HTML writer snapshots,
hub-client preview fixtures (`Plain.tsx` vs `Para.tsx` in the preview
renderer render these differently), and the DOM-parity fixtures from
bd-nrywksil if the include case was a reader-side Plain/Para issue.

## Checklist

### Phase 0: spec study

- [x] Read the spec, locate the definition and every example that bears
      on loose vs tight (table above).
- [x] Baseline the spec examples against pandoc; bucket the results.
- [x] Confirm at least one deviation outside the spec examples (clause 3,
      first-paragraph-only rewrite).
- [x] User review of this plan; open questions settled (Decisions below).

### Phase 1: oracle and harness

- [x] `crates/pampa/tests/integration/list_looseness.rs`: shape signature,
      both readers, report with failure classes, spec-example test with
      known-divergence lists.
- [x] `scripts/commonmark-list-survey.py`: the pandoc cross-check (`spec`
      and `corpus` modes). comrak and pandoc agree on every spec example
      that both parse.
- [x] Harness green on `main` with the known lists in place (baseline
      below).

### Phase 2: generator

- [x] Document grammar with a deterministic enumerator (18,446 documents
      after normalization and dedupe) and the metamorphic div-to-quote
      substitution.
- [x] proptest sampler over the full grammar; `PROPTEST_CASES` sizes it.
- [x] Corpus dumped (`LIST_LOOSENESS_DUMP_DIR`) and cross-checked against
      pandoc (results below).
- [x] Baseline run against `main`, failures bucketed; non-looseness
      buckets filed as strands and recorded in `known_generated`.

### Phase 3: fix

- [x] Content-end row turned out unnecessary: the row *above* a block's or
      item's first row is always either the previous block's last content
      row, the marker row, or a blank line, so "is the row above blank?"
      is the whole test (see Results).
- [x] `process_list` rewritten with the two spec clauses and the
      all-paragraphs tight rewrite; `list_item_has_blank_line_between_blocks`
      rewritten on source rows; `enclosing_block_quote_depth`,
      `row_above_is_blank`, `is_blank_after_quote_prefixes` added with unit
      tests.
- [x] Spec examples 317 and 328 pass; the looseness failure class is
      empty on the enumerated corpus and on repeated 5,000-case samples.
- [x] bd-25ezfgs8 pinned expectations updated (repro A and control B now
      both loose, as pandoc says). bd-25ezfgs8 and bd-i4xrcdqx shapes are
      in the corpus and pass; close both when this lands.

### Phase 4: round-trip and verification

- [x] Round-trip tests (enumerated and sampled) over the same generator.
      Two writer gaps fixed: tightness is now "no item directly contains
      a Paragraph" rather than "every item starts with Plain", and a
      block pair the reader would merge (quote then quote, quote or
      sublist then paragraph) gets a blank line even in a tight item.
- [x] `qmd_writer_pandoc_shapes` definition-list settling test replaced:
      loose and tight single-paragraph definitions each round-trip as
      themselves now that the reader is CommonMark-correct.
- [x] `cargo xtask verify` green (all 14 steps); no snapshot flips outside
      the two pinned tests updated above.
- [x] Windows cross-check attempted: `cargo clippy -p pampa --all-targets
      --target x86_64-pc-windows-msvc` is blocked by `aws-lc-sys` building
      C for the target (the documented caveat); the change is plain byte
      scanning with no platform-specific code.
- [x] No authoring-docs change needed: `docs/guides/authoring/lmd.qmd`
      describes tight lists from `pandoc.Plain` fragments, which is
      unchanged. The rendering change is recorded in Results.

## Results (2026-11-06)

**The rule.** For a list node, `quote_depth` is the number of enclosing
`pandoc_block_quote` nodes. The list is loose if, for any item after the
first, or for any direct block child after the first within an item, the
source row immediately above its start row is blank once up to
`quote_depth` leading `>` markers (and surrounding whitespace) are
stripped. Why that is the whole spec: the row above a block's first row is
the previous block's last content row (a paragraph line, a closing fence,
a `:::`, a `>` line of an inner quote, which keeps its `>` because it is
beyond `quote_depth`), or the list marker row, or a blank line. Blank
lines inside fenced code, inside block quotes and inside sublists are
never directly above a sibling block or item, so they do not count
(examples 320-323). A whitespace-only line, with spaces or tabs, is blank
(bd-25ezfgs8, bd-i4xrcdqx). In a tight list every direct `Paragraph` of
every item becomes `Plain`.

**Baseline on `main` vs after the fix**, enumerated corpus of 18,446
documents, comrak oracle:

| Class | main | after |
|---|---:|---:|
| agree | 12,244 | 14,705 |
| looseness (Plain vs Para only) | 2,461 | 0 |
| other shape (block quotes merging, see below) | 3,345 | 3,345 (known) |
| parse error (items starting with a blank line) | 396 | 396 (known) |

`scripts/commonmark-list-survey.py corpus` on the dumped corpus: comrak and
pandoc 3.12.1 agree on the list shape of all 18,446 documents, and pandoc
vs the qmd reader reproduces the "after" column exactly.

Spec examples with a list: 54 agree, 2 looseness mismatches on `main`
(317, 328); 56 agree and 0 looseness mismatches after. The 6 other
mismatches (tabs, lazy continuation, setext, ref defs) and 18 parse
errors (indented code, tabs, rule-3 items) are out of scope and listed in
`SPEC_KNOWN_MISMATCH` / `SPEC_KNOWN_PARSE_ERROR`.

**What was wrong on `main`.** Three of the four signals: "two paragraphs
in an item" (wrong when a heading or fence sits between them without a
blank line), the first-paragraph-only tight rewrite, and the item-gap
check computed from AST end offsets, which include the trailing blank
lines of a sublist, code block or block quote, so a blank line after such
an item was invisible. Inside a block quote or div container no blank
line between items was ever seen. The `block_continuation`-based
within-item check missed whitespace-only lines.

**Strands filed from the corpus** (each with a repro and a
`known_generated` family in the harness; the family entry must go when
the strand is fixed, and the harness fails when a family has no failing
document left):

- bd-se0yhp3o: block quotes separated by an empty line inside a list item
  merge into one (scanner; an indentation-only separator is fine).
- bd-9xnwn3z7: a whitespace-only line containing a tab after a sublist is
  not treated as blank (scanner).
- bd-qgswtht1: a list item starting with a blank line (`-` alone, content
  on the next line; spec rule 3, examples 280-281) is a parse error
  (grammar + scanner).
- bd-6522kqyw: the qmd writer spells three nested empty bullet items as
  `* * *`, a thematic break (writer, P4).

The two-blank-lines branch in the grammar's `_list_item_content` is not
observable: `- a\n\n\n- b` reads as one loose list, as the spec says.

**Rendering change for users.** Lists in these shapes rendered tight (no
`<p>` in items) before and render loose now, matching CommonMark, pandoc
and Quarto 1: items separated by a blank line inside a block quote or a
fenced div; a blank line after an item whose last block is a sublist, a
code block or a block quote; an empty item followed by a blank line; a
whitespace-only separator line. One shape goes the other way: two
paragraphs in one item separated by a heading, fence or quote with no
blank lines is tight now (Plain, Header, Plain).

## Decisions (reviewed 2026-11-06)

1. **Oracle.** comrak in-process for CI; pandoc as a dev-time cross-check
   only. A regression test that does not run in CI will not stay green.
2. **Scope of grammar and scanner changes.** Not excluded. Whether a
   grammar gap the corpus finds is fixed here or filed as its own strand
   is a judgment call weighing the difficulty of the fix against the
   nature of the failure. When the call is unclear, stop and ask rather
   than guess. The two known candidates are rule-3 items (`-\n  foo`,
   example 280) and the two-blank-lines closing branch in
   `_list_item_content`.
3. **qmd-only blocks.** Divs and display math are ordinary block-level
   elements for looseness: a blank line before `:::` inside an item makes
   the list loose. Checked by the metamorphic substitution. This is what
   bd-i4xrcdqx already assumes.
4. **Quarto 1 differences.** The target is CommonMark. Where the corpus
   shows a Quarto 1 document rendering differently under pandoc-markdown
   rules, record the difference in the migrating-markdown guide; do not
   follow pandoc-markdown.
5. **Corpus size.** CI runs the exhaustive small-size enumeration plus 256
   sampled proptest cases. During development the sampled case count is
   turned up through the environment variable (thousands of cases, several
   seeds) to see what comes back; a larger sampled run can later become a
   nightly or a local command. The plan's "run against `main`" step in
   Phase 2 uses the turned-up setting.

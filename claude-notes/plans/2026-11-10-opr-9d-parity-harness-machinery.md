---
title: 'OPR-9d: parity harness machinery (normalizer, pinned entry, zip comparison, a real gate assertion)'
date: 2026-11-10
description: 'Test-infrastructure work for the plain-render gate: an HTML-aware document normalizer with exact document names and per-side fire counts, a pinned exact-difference entry, zip comparison for docx, pptx and epub, a loud failure for a format the harness cannot read, and an assertion that makes the gate report a pass or a fail. No production code.'
status: complete
depends-on: [opr-9a]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9d-parity-harness-machinery` · **Lane D** (`workspace-6`) · **Blocks:** 9e, 9f.
**Needs:** No R or Julia needed for the unit tests; the gate itself (9f) needs both.

> **Reading text copied from OPR-7d.** Passages copied from the old 7d plan say "this plan" or "7d". Read them as this
> piece of the series 9a to 9g (see the [tombstone](2026-11-08-opr-7d-switch-plain-render.md)). Marks: (read), (ran), (relied).

## Start condition

Start when 9a is `complete`. Check that `workspace-6` is clean and on no other branch (on 2026-11-10 it was on `knr/knitr-pieces-hook-memo` with an untracked research note); if not, stop and report. Read its S3 (the parser and the pin form, both settled: **`lol_html` in two passes, and the pin in a file beside the allowlist**) and S4 (the zip extractor spec). The sections below already carry those results; where the note and this plan disagree, the note wins and you say so.

## Decisions it implements

**4. Julia interleaving on the flip: accepted as an intended improvement (Gordon, 2026-11-09).** For a Julia cell with
several expressions the plain route runs the whole cell as one block; the recorded route splits it per expression,
with exact `prompt`, numeric `echo` and `warning: false` by kind (OPR-4d; Quarto 1 never interleaved Julia). The parity
gate\'s cases `julia/capture-facts`, `julia/presentation-attrs` and `julia/records-plot` are that behavior change, not
regressions. Record it in the gate report and the allowlist, with the forms in decision 5, and mention it in the
migration notes written by OPR-8.

**5. The whole-document allowlist form lives here (Gordon, 2026-11-10), and after 7j it has one normalizer and one
pinned entry.** Gordon chose to design one mechanism in this plan rather than sign a one-off rule in 7g. The six
unallowlisted cases at the end of 7g were: the three 4d interleaving cases above, `julia/fig-subcap`, and the Plots
script elements in `julia/docs-index-chunk`, `julia/positron-cache` and `julia/records-plot`. The Plots script cases are
OPR-7i\'s, and `julia/fig-subcap` is OPR-7j\'s plus interleaving (the group cell is split like any other). So 7d
allowlists four cases, all for interleaving: `julia/presentation-attrs`, `julia/records-plot` and `julia/fig-subcap` by a
document normalizer, and `julia/capture-facts` by a pinned exact-difference entry (Gordon, 2026-11-10: "2 sounds good").
The `julia/records-plot` case appears in both groups: it needs 7i for its script lines, and the normalizer must leave
those lines visible. The allowlist header calls the group references "not an allowlist candidate"; that sentence goes
with 7j, since after it there is nothing to allowlist for them.

## The whole-document form: design (settled by the 7d review; confirm it in tests first)

**Problem.** `RULES` in `tests/integration/opr_parity_harness.rs` are regex substitutions applied to both sides, so a
rule hides a line-level difference. The 4d interleaving is the same content arranged differently; a regex wide enough
to cover it would also hide real bugs in the same document. Evidence for what follows: the 7d review, items 11 to 20,
rendered by a sub-agent with prototype controls (the prototype stays outside the tree).

**Normalizer `julia-interleaved-cell` (a second kind of rule, a function from a side\'s HTML to a canonical HTML).**
Applied to both sides, scoped by an **exact document name** (not a prefix: a prefix would also cover a future
`julia/fig-subcap-tables`), named in `RULES`, and HTML-aware: use **`lol_html`** (already a normal native dependency of `quarto-core`; no Cargo change). 9a\'s S3 rejected `scraper`: re-serializing hides an attribute-order difference (trap 1). The form, as prototyped: pass 1 marks, with comments, every `div.cell`, the closing `</code>` of each cell\'s first code block and every later code block (`div.cell div.cell-code`, a descendant selector, since a `fig-subcap` group cell nests its blocks in `figure > div`); pass 2 is string splicing that cuts each later block\'s scaffold out and appends its `code` inner html to the first block. Every other byte is kept. The splice asserts the exact scaffold text (the opening tag, the copy button, the closing tag); a mismatch is a blocker, never a pass. The form:

- For each `div.cell`, take every `.cell-code` block that belongs to that cell (no nearer `.cell` or `.cell-output`
  ancestor). Append each later block\'s `code` children to the first block\'s `code`, joined with a newline (highlighting
  spans kept), and drop each later block\'s copy-button scaffold and the whitespace after it.
- **Do not join adjacent outputs.** The plain route already emits one output block per expression, so only code blocks
  differ, and joining outputs hides a real change (a control that merged two stdout outputs into one was reported by the
  narrow form and hidden by the join form).
- *Keeps:* every code line and its order, every output\'s content, count and order, cell attributes, the cell count,
  everything else in the cell and everything outside `.cell`. *Discards:* which code line each output follows, how many
  code blocks a cell has, and the per-block copy wrapper. Known gap: a recorded cell whose first unit hides its code but
  has output would put the output before the merged code; absent from this corpus; the test says so.

**Pinned exact-difference entry for `julia/capture-facts`.** Its diff has three kinds of difference besides
interleaving (the prompt marker and `&gt; ` prefix, the numeric `echo`, and `warning: false` stderr), each named in
decision 4 and each changing code text or outputs, so no normalizer can hide it without hiding real changes. The entry
stores the whole post-interleave diff; the document is allowlisted only if the actual diff equals it. Anything new in
that document is still a blocker. Gordon signs it.

**Traps the harness tests must cover** (all measured in the review):

1. *Serialization masks differences.* Re-serializing a parsed page sorts attributes and rewrites entities, so an
   attribute-order or `&quot;` difference becomes equal. Splice only the touched subtrees back into the original text,
   or round-trip both sides first and report a blocker when the raw texts differ but the round-tripped texts do not.
2. *The "rule used" bookkeeping.* `compare` counts a rule as used when the text changed; a re-serializing normalizer
   always changes it. Each normalizer returns a structural fire count per side; the report lists, per entry, the
   documents it touched and the plain and recorded counts.
3. *Stale entry.* For each allowlisted case, re-compare without each entry; an entry needed by no document in its scope
   fails the report test.
4. *Wrong document.* The interleaving normalizer fires only on the recorded side (plain count 0); a plain-side fire
   means the plain page has multi-block cells it was not written for.
5. *Order-only changes.* `describe_difference` is a multiset line diff, so swapping two outputs is a blocker with an
   empty diff; add an ordered diff or a "same lines, different order" note.
6. Run the regex rules on raw text before the normalizers.

**Negative controls (required, each a single mutation of a real recorded page that must still be reported):** drop one
output; swap two outputs; change one code line in a split-off block; fold two cells into one; change prose outside any
cell; merge two stdout outputs into one; and the serialization control of trap 1.

**Signing.** Each entry stays `proposed` until Gordon signs it; the agent writes the rule and reason, not the signature.
The interleaving entry is permanent (the behavior is the intended one).

## What the review added to the design

- **Schema (read).** `scope` is a `starts_with` prefix, and the signed `scope = "knitr"` relies on that to cover `knitr-corpus/...`,
  so exact-name scoping is a **new optional field** `document`, mutually exclusive with `scope`; the pin is a second optional
  field (`pin`, **the path of a file beside the allowlist**, valid only with `document`; 9a\'s S3 chose a file over a TOML literal because the 24 lines are long, span-laden html). Validate both in `Allowlist::parse`. `RULES` is a
  `&[&str]` and `apply_rule` returns no counts, so normalizers are a second table (a function from HTML to HTML plus a fire
  count), run after the regex rules (trap 6). `Verdict::Allowlisted { rules: Vec<String> }` becomes a list of applied entries with
  per-side fire counts; the tests that compare it by value change.
- **The pin compares an unclipped, ordered diff (read).** `describe_difference` is a multiset diff clipped to 8 lines per side and 240
  characters; a pin compared against it would hide everything past line 8. A changed pin is a `Blocker` that prints expected and
  actual; a pin on a document whose diff is empty is a stale entry. The `julia/capture-facts` diff has four kinds of difference
  (the `data-prompt="true"` attribute, the `&gt; ` prompt prefix, the numeric `echo`, the stderr blocks). **After the normalizer, S3 measured 24 lines (2, 6, 4 and 12 by kind; 41 before it, by the prototype\'s unclipped multiset count; the earlier 51 came from another count and was not reproduced).** The entry\'s `reason` names the four kinds with those counts; the report attaches the whole diff.
- **Serialization (ran, S3).** `scraper` fails trap 1 (an attribute-order swap round-trips to equal text) and keeps no source spans, so it is
  out. `lol_html` plus string splicing keeps every untouched byte, and the attribute-order control is reported. The seven negative controls were
  run (six on real pages; "merge two stdout outputs" has no site on them and ran on the synthetic page of the first test): each is reported.
  Nested `div.cell` is unsupported; a hidden first unit with output is the known gap.
- **The report needs a real assertion (ran).** `opr_parity_report` passed on the tip with four blockers because it asserts nothing. Add an
  assertion under `OPR_PARITY_REPORT` that fails when any case is an unallowlisted blocker, and the stale-entry check of trap 3 (for each
  allowlisted case, re-compare without each entry; an entry needed by no document in its scope fails). The plain
  `opr_parity_report` run with no environment variable must still do nothing.
- **Zip formats (ran; the extractor spec is S4\'s).** `COMPARABLE` is consulted only in the knitr loop, and `render_prepared` reads the
  output with `read_to_string`, so a docx, pptx or epub listed in the manifest fails on both sides and lands under "both routes failed".
  Make a manifest format with no extractor fail loudly (an error or "not compared", never "both failed"), and add the extractor: open the
  zip (`zip` is a dev-dependency already used by three integration tests; no new Cargo dependency) and canonicalize the text parts, never
  the zip bytes (`unzip -p` plus `xmllint --format` reproduces what S4 did by hand):

  | Format | Compare | Strip or normalize |
  |---|---|---|
  | docx | `word/document.xml`, `word/_rels/document.xml.rels`, `word/media/*` by content hash | ignore `docProps/core.xml` (timestamps); renumber `w:bookmarkStart w:id`, `wp:docPr id`, `pic:cNvPr id`, `rId*` and media names by first occurrence |
  | pptx | `ppt/slides/slide*.xml` in numeric order, `ppt/slides/_rels/*`, `ppt/media/*` by hash | ignore core.xml timestamps and the layout and master parts (the template) |
  | epub | `EPUB/text/*.xhtml`, `EPUB/nav.xhtml`, `EPUB/media/*` by hash | `content.opf` identifier, date and modified; `toc.ncx` uid |

  `Side.figures` for a zip is its media content hashes. **Two differences the normalizer must not hide** (S4), and 9b removes both: the recorded docx carried
  extra `w:bookmarkStart`/`End` pairs with hashed names that plain does not (9a\'s S2 traced them to the cell-wrapper ids the recorded route stamps on every target; Quarto 1 writes that id only for html-family, ipynb and JATS output, `jupyter.ts:1366-1376`, so **9b gates the id by target and the bookmarks disappear: 4 before, 0 after on the S2 fixture**; no extractor rule is needed), and the picture `descr` carried the figure directory (`figure-docx`
  versus `figure-html`; S2 confirmed the two now agree). Also strip Plots\' random SVG `clip<N>` ids before hashing SVG media (S2: the `pptx` and `odt` SVGs differ only by them). The epub `fig-subcap` group differs as the interleaving does (one code block versus two).

## Checklist (tests first; every box is `[unattended]`)

- [x] **Tests first for the normalizer and the pinned entry** (design above): the first test builds a plain page with
      one cell (one code block, two stdout outputs `a` and `b`) and a recorded page with the same cell split into two
      code blocks each followed by its output; assert `Allowlisted` with the rule named and fire counts 0 (plain) and 2
      (recorded). Its negative control, in the same test: the recorded page with the two outputs merged into one block
      `a\nb` must be a `Blocker`. Then one test per trap and per control listed above, including swapped outputs
      (a `Blocker` whose report says same lines, different order) and the serialization control. Rewrite
      `the_seeded_allowlist_parses_and_each_entry_is_proposed_or_signed` (in `opr_parity_runner.rs`; it asserts exactly three
      entries, the signed rule names and the "Signed: Gordon, 2026-11-09" text) for the new entries. 7e\'s note that it
      failed with 8 entries versus 6 is stale: it passes with 3 today. The normalizer and pinned-entry code goes in
      `opr_parity_harness.rs`; the report and manifest tests are in `opr_parity_runner.rs`.
- [x] **Tests first, the pin.** A pinned entry with the exact diff is `Allowlisted`; the same entry with one changed line is a `Blocker`
      showing both texts; a pin on an empty diff is a stale entry; a pin with `scope` (not `document`) is rejected by `Allowlist::parse`.
- [x] **Tests first, zip.** First red test: a `.docx` output becomes a `Side` (it fails today with a UTF-8 error). Then pptx and epub; the
      test zips are built from the draft document in the 9a note (S4) or a small committed fixture, and one test asserts that a bookmark present on one docx side only (a mutated copy of a real part, since the real pair has none after 9b) is
      reported, not normalized away. A manifest
      format with no extractor fails loudly. Build the test zips in memory with the `zip` writer if its features allow, else use a small
      committed fixture; say which.
- [x] **Tests first, the report assertion** and the stale-entry check, with a fake `Case` list (no rendering).
- [x] **Implement** `document` and `pin`, the normalizer table, per-side fire counts, the unclipped ordered diff and its "same lines,
      different order" note, the zip extractor, the loud failure, the assertion. Run the regex rules before the normalizers (trap 6).
- [x] **Update the existing tests** that this changes: `the_seeded_allowlist_parses_and_each_entry_is_proposed_or_signed` (it asserts
      exactly three entries, the signed rule names and the "Signed: Gordon, 2026-11-09" text; it passes today, and 7e\'s note that it failed
      with 8 entries versus 6 is stale), the test that compares `Verdict::Allowlisted { rules }` by value, the `allowlist_parse_rejects_*`
      pair (extend for `document` and `pin`), `the_report_names_every_class` (per-entry columns) and
      `the_plans_own_manifest_parses_and_names_real_documents`. The normalizer and pin code goes in `opr_parity_harness.rs`; the report and
      manifest tests are in `opr_parity_runner.rs`.
- [x] **Gates** (results in the notes; two are not clean for reasons outside this plan): `cargo clippy -p quarto-core --all-targets -- -D warnings` (clean on the tip today); `cargo nextest run -p quarto-core`; the
      Windows cross-clippy; `cargo xtask lint` (new fixtures must not add a rendered `X.html` beside a tracked `X.qmd`:
      `committed-render-output`).

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol's "When you cannot
finish") if any of these happens:

- a negative control is not reported as a blocker and you cannot fix the normalizer without hiding a real change: report it;
- the pin is too large or too unstable to review (S3 says how many lines): report it before writing the entry;
- `workspace-6` is dirty or on another branch;
- the harness needs a production-code change: stop and report;
- a test that passed on the tip fails after your change and is not one of the tests this plan names: report it, do not weaken it.

## Handoff

End the session with the protocol's handoff ("Ending your session"). Name 9e, the next plan in this lane, as startable if 9b (lane B) is complete; the entries themselves are written in 9f.

## Exit criteria

The harness unit tests pass, each trap and negative control is a test, the zip extractor and the loud failure are tested, the report test
fails on an unallowlisted blocker and on a stale entry, and clippy is clean. No allowlist entry is added here (9f does that).

## Notes from the run

**What landed** (branch `opr/9d-parity-harness-machinery`, all in `crates/quarto-core/tests/`; no production code):

- `opr_parity_harness.rs`: `Entry` gains optional `document` (exact name; exclusive with `scope`) and `pin`
  (path of a file beside the allowlist; `Allowlist::load` reads it into `pin_text`). A normalizer rule requires
  `document`. A pin needs a rule, so I added `PIN_RULE = "pinned-difference"`: `pin` goes with that rule and with
  nothing else (`Allowlist::parse` validates both ways). `Verdict::Allowlisted { applied: Vec<Applied> }` carries,
  per entry, `plain_fires`, `recorded_fires` and `needed` (re-comparing without the entry is a blocker). Regex rules
  report match counts; the normalizer reports merged later blocks. Order of work: regex rules on raw text, then the
  normalizers, then the pin (trap 6).
- Normalizer `julia-interleaved-cell`: `lol_html` marks in pass 1, string splicing in pass 2, exact scaffold asserted,
  nested `div.cell` refused; any failure is a blocker, never a pass. A plain-side fire is a blocker (trap 4).
- `difference()`: one function, clipped for blocker reports (8 lines per side, 240 characters) and unclipped for pins;
  it appends "same lines, different order" (or "the lines both sides share are in a different order") when the shared
  lines are reordered (trap 5).
- `opr_parity_zip.rs` (new file, registered in `main.rs`; the plan said to put it in the harness): docx, pptx, epub
  extractors per S4, built-in `zip` reader, `sha2` hashes. Media go in `Side::figures` as `media/<hash>.<ext>`.
  `side_from_output` is the single entry from an output file to a `Side`; an extension with no extractor is an error
  that says "no extractor" (`has_extractor`).
- `opr_parity_runner.rs`: `EXTRACTABLE` (text formats plus docx, pptx, epub) for the plan's manifest: a manifest format
  outside it goes to "formats not compared (... no extractor)", never to "both failed"; the knitr loop keeps its old
  `COMPARABLE` list (zip formats stay not compared there; changing that is not 9d's). `gate_failures` (blockers, routes
  that disagree, stale entries) is asserted under `OPR_PARITY_REPORT`; the report gained per-entry fire counts and the
  documents each entry touched, with "(NOT needed)" marks.
- Fixtures: `tests/fixtures/opr-parity-normalizer/`: the four real pages of 9a's S3 (plain and recorded html, copied from
  the 9a scratchpad, generator stamp stripped, named `*.html.txt` so `committed-render-output` does not see them) and
  `capture-facts.pin.txt`. No `.snap` files were added, changed or removed.

**Where the plan and the 9a note disagreed (the note won).** (1) Fire counts: the plan's first test says 0 and 2 for
a cell split into two blocks; the normalizer counts merged later blocks, so it is 0 and 1 (the note's S3 table agrees
on the real pages). (2) The pin: 24 `-`/`+` lines (the note's count) plus ONE order line, "the lines both sides share
are in a different order: first at shared line 68", because the recorded page puts the stderr blocks before the
display output that plain emits after them. That is a fifth kind of difference beside decision 4's four (prompt
marker, numeric echo, `warning: false` stderr, and the `data-prompt` attribute), and 9f must name it in the entry's
`reason`. Check it before signing.

**Evidence.** Ran: `cargo nextest run -p quarto-core -E 'binary(integration) & test(opr_parity)'` (52 tests, all
pass); each group was seen red first (stub implementation: 9 harness, 8 zip and 3 gate tests failed on assertions or on
"not implemented", not on compile errors). End to end: `OPR_PARITY_ONLY=julia/capture-facts OPR_PARITY_KNITR=0
OPR_PARITY_REPORT=<file> cargo nextest run -p quarto-core -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture`
rendered the real document twice through `render_document_to_file`, reported one unallowlisted difference (no entry yet:
9f) and the test failed with "the parity gate failed". Without the variable the test does nothing (it passed in the
52). Not done: a real docx, pptx or epub render through `render_prepared` (the zip tests build archives in memory with the
`zip` writer, `Stored`; real renders belong to 9e and 9f); the seeded-allowlist test
`the_seeded_allowlist_parses_and_each_entry_is_proposed_or_signed` is unchanged because this plan adds no entry (9f
does), and it still passes with 3 entries.

**Gates.** Host `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean. `cargo nextest run -p quarto-core
--no-fail-fast`: 8287 passed, 36 skipped, 1 failed: `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`
(a vendored reveal CSS file differs from the npm copy in property order; a `src` test, and this plan touches no `src`
file, so it is not caused by 9d; I did not check it on a clean tip checkout). Windows cross-clippy: NOT run to
completion: `aws-lc-sys` cannot build its C code for `x86_64-pc-windows-msvc` on this machine (`stdlib.h` not found), the
caveat in `.claude/rules/cross-platform.md`; a Windows user should run `cargo clippy -p quarto-core --all-targets -- -D
warnings`. The new code uses no platform APIs and builds paths only through `Path`. `cargo xtask lint`: one violation,
`crates/quarto-core/src/positron_cache.rs:360` (`metadata-as-str`), in a file this plan does not touch; no new
violation from the fixtures.

**Known gaps.** A recorded cell whose first unit hides its code but has output stays a false blocker (a test says so).
The stale-entry check ignores entries with no compared case in scope (a run without Julia or R cannot tell), and a
`pending` case is not exempt from the gate.


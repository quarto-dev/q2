---
title: 'OPR-7d plan review: the normalizer, the flip, the gate that goes vacuous'
date: 2026-11-10
description: 'A review of the OPR-7d plan against the code and real rendered pages: what the whole-document normalizers can and cannot hide, where the flag is set, why the parity gate stops measuring anything once the default flips, and the plan edits to make first.'
---

**Plan:** [OPR-7d](../plans/2026-11-08-opr-7d-switch-plain-render.md) at `76c93f12f`.
**Addendum (same day, after Gordon\'s answers):** items 15 and 16 are superseded. The pre-epic render shows the plain
route is unchanged by the epics, and the lost group caption plus the lost panel ids and references are a regression of
the flip that Gordon does not accept. [OPR-7j](../plans/2026-11-10-opr-7j-group-caption-and-panel-ids.md) fixes them for
every engine (prototype: Julia recorded matches plain; Jupyter gains the caption and references; knitr unchanged; 810
presenter, knitr and Julia end-to-end tests passed). The `julia-subfigure-group` normalizer is dropped, the interleaving
normalizer stays, `julia/capture-facts` gets a pinned exact-difference entry, and the gate runs before the flip with the
harness kept afterward as a regression test (Gordon: "2 sounds good", "3 yes keep harness"). The 7d plan, the 7j outline,
the subfloat draft and the epic table were edited to fit. Correction to item 15: `tbl-one` renders as a table with its
caption on all three engines, not as a literal pipe-table paragraph.
**Method:** I read the plan, the protocol, the 7g gate report, the harness and the 7i draft (from its branch), then ran two
sub-agents in parallel (A: normalizer design, opus; B: flip, gates and gaps, sonnet). I wrote this report myself.
Marks: (read), (ran), (relied) for a sub-agent claim I did not re-check. Review only: no code, plan edit, allowlist
signature or status flip.

## Verdict

**Not ready to flip as written.** Wait for 7f, 7g, 7h and 7i as planned, but make the plan edits in the last section
first. Three of them change what the plan can honestly claim: the gate is vacuous after the flip (finding 1),
`julia/capture-facts` cannot be allowlisted by an interleaving normalizer (finding 5), and `julia/fig-subcap` loses a
group caption that decision 3 did not accept for figures (finding 8).

## Findings: the gate and the flip

1. **The gate stops measuring anything once the default flips (read).** `write_project_config` in
   `opr_parity_harness.rs` builds the "recorded" side by adding `execute: {record: true}`; the "plain" side is the
   default render. Today the two sides take different routes. After the flip, with no opt-out (decision 2), both present
   from the record and the harness compares the recorded route with itself plus a written file. The checklist orders
   "flip the default" before "re-run the gate, zero unallowlisted differences", so that box would pass for any input.
   The gate must run before the flip, on the commit that has the normalizers and the signed entries, and its report must
   be the one attached. After the flip the harness needs either a retired role (the cases become regression tests of
   the recorded route) or a test-only seam on the pipeline builder; a test-only parameter is not an opt-out, but say so.
2. **Nothing in production sets the flag (read, re-ran).** `with_present_from_record` is called only from tests
   (`engine_stage_driver.rs:151`, `ts_record.rs:1825`, `ts_engine.rs:3709` is inside the engine and forwards the stage
   value). My grep (ran) confirms B's list. The field is `EngineExecutionStage.present_from_record`
   (`engine_execution.rs:116`, default false at `:129`), copied to `ExecutionContext` at `:683`; `builds_record()` is
   `want_record || present_from_record` (`context.rs:310`) (relied for the line numbers other than the ones my grep shows).
3. **Where the default flips is undecided, and one placement loses it (read, re-ran).**
   `EngineExecutionStage::new()` is called at `pipeline.rs:398` (shared head), `:986`
   (`insert_capture_splice_stage_in` rebuilds the stage with `new()`) and `:1921` (`parse_qmd_to_ast`). A builder call
   at `:398` is lost on every capture-splice pipeline (q2 preview, the browser pandoc path). Flipping the initial value
   inside `new()` reaches all three, and also every test that uses `new()` as "plain". The plan must name the placement
   and list the tests that relied on `new()` being plain.
4. **The quiet fallback is already done; the checklist item is stale (read, re-ran).** `plan_capture`
   (`engine/knitr/target_route.rs`) has one caller (`knitr/mod.rs:272`) and sets
   `diagnostic: want_record.then(...)`, so a fallback caused by `present_from_record` alone is quiet; the test
   `a_fallback_caused_only_by_present_from_record_is_quiet` covers pdf and pptx (relied for the test name and line).
   Two things the plan misses. (a) `want_record` is also true for the `--trace` observer, the preview recorder and the
   hub (`wants_execution_record`), so a knitr render to pdf under `--trace` prints the info line though nobody asked for
   a record. If "an explicit record request" is the intent, `ExecutionContext` needs a `record_requested` field; the
   plan should say whether the observer cases count. (b) Julia has no target gating: `offers_presentation`
   (`ts_record.rs:654`) checks only that a record is built and units exist. After the flip a Julia render to pdf, latex,
   beamer, pptx, docx or epub is presented by core, while the plan promises "LaTeX family stays on the engine route" for
   knitr only. The parity gate compares html, gfm, revealjs and typst only (`COMPARABLE`, `opr_parity_runner.rs:79`), so
   no Julia render to a LaTeX-family target is covered by any gate (relied for the manifest sampling).
5. **`.quarto`: no write sits on the engine route only (ran, read).** A plain render of an R document in a scratch
   directory leaves only `.quarto/render-manifest.json`; with `execute: record: true` it also leaves
   `.quarto/execution-records/doc.ipynb`. The HTML of the two renders was identical after splitting on `><`. The record
   file is gated on `record_requested` (`engine_execution.rs:913`). A non-brittle test: assert
   `.quarto/execution-records` does not exist after a plain render, and compare the sorted `.quarto` listing with a
   `--no-execute` render of the same document; do not pin the whole listing. Stage-level precedent:
   `present_from_record_builds_the_record_but_serializes_and_writes_nothing` in `engine_record_stage.rs` (relied).
6. **Preview and WASM: the flip changes nothing there (read, relied).** The preview recorder's observer already wants a
   record, so preview already presents from it for Julia and knitr. `wasm-quarto-hub-client` compiles
   `engine_execution.rs` and `context.rs`, but `ts_engine`, `knitr` and `jupyter` are `cfg(not(wasm32))`; in WASM the
   engine is replay or markdown. The preview-chain box is a compile check, not a behavior check. I did not run it.
7. **Native Jupyter does not take the flag (read, relied).** `text_execute.rs` gates only on `ctx.want_record` and
   already presents through `present_document` on every render. The plan's tests and end-to-end box name Jupyter as if
   the flip changed it; it does not. 7i decision 6 says 7d "carries" the native-Jupyter widget-warning difference as
   documented, and 7d's checklist has no item for it.
8. **Tests for the binary: no existing helper drives Julia or knitr (read, relied).** `no_execute_cli.rs` has `q2_render`
   and `Q2_BIN` but its fixture needs no R. `r_available()` and `julia_available()` live in `quarto-core` tests, so the
   `quarto` crate needs its own copy or a shared one. Missing engines skip by early return, not `#[ignore]`. CI installs
   a Jupyter venv only; a grep of `.github/workflows` for julia, setup-r and Rscript returns nothing, so the knitr and
   Julia binary tests are local evidence and the plan should say so. Machine here: Rscript with knitr 1.50, Julia 1.12.7.
9. **The `id="cell-N"` example is stale; the claim holds (read, relied).** OPR-7f made the presenter write the unit's
   wrapper id, so a test keyed on its absence would fail or be vacuous. B grepped Rust, TS, TSX, JS, CSS, SCSS, Lua, JSON,
   QMD and YAML under `crates/`, `hub-client/src`, `ts-packages/`, `resources/` and `docs/` and found no reader of a
   rendered `id="cell-N"`; the hits are figure file names (`cell-N-output-M.png`), an internal `UnitId`
   (`engine/units.rs`) and a class read in `dashboard/structure.rs`. Not checked: whether stale old-named figures in
   `<doc>_files` are cleaned on re-render; freeze, listings, search and the profile checkpoint as consumers of engine
   output (unverified, say so rather than claim "nothing reads it").
10. **Seeded-allowlist test (relied, not run).** 7e's notes say `the_seeded_allowlist_parses_and_every_entry_is_proposed`
    failed (8 entries vs 6). The file now has 3 signed entries. The test must be rewritten with the new entries; run it
    before building on it.

## Findings: the normalizers

Real pages were rendered by sub-agent A through the CLI; its narrowed harness run matched the 7g report (10 identical,
the same six unallowlisted documents), so I treat its renders as the harness's (relied).

11. **`julia/presentation-attrs` and `julia/records-plot` are pure interleaving once 7i lands (ran by A, relied).** The
    cell's code is split into two blocks, each with its own copy-button wrapper; every output is the same.
    `records-plot` also has the three Plots script lines, which are 7i's. Side finding: `presentation-attrs` renders no
    image on either route (`display(MIME("image/png"), bytes)` yields nothing), so its manifest note about figure
    names is checked against nothing.
12. **`julia/capture-facts` is not just interleaving, and no normalizer can hide the rest (ran by A, relied).** Three more
    kinds of difference: (a) prompt: plain has `data-prompt="true"` and no `> ` prefix, recorded has a `&gt; ` prefix on
    code lines; (b) numeric echo `[2]`: plain shows three code lines, recorded only `b = 2`; (c) `warning: false`: plain
    drops stderr, recorded adds two stderr outputs. Each changes code text or outputs, so hiding it hides real changes.
    Decision 4 names these as part of the accepted 4d behavior. Recommended form: a **pinned exact-difference entry**.
    Store the full post-interleave diff in the entry and allowlist the document only if the actual diff equals it, so
    anything new in that document is still a blocker.
13. **The sketch's "join adjacent `.cell-output` blocks" is unnecessary and too broad (ran by A; I re-ran control M6).**
    The plain route already emits one output block per expression (`println("a"); println("b")` gives two stdout blocks on
    both routes). Only code blocks differ. With the narrow normalizer, a mutation that merges two stdout outputs into
    `a\nb` is reported; with the sketch's join-outputs variant it is **hidden**. My re-run: `interleave` reports the
    difference (`+ recorded <pre class="code-with-copy"><code>a ... b</code></pre>`); `interleave,joinout` prints
    `VERDICT: equal`. This is the evidence that the cleverness is needed.
14. **Proposed canonical form for `julia-interleaved-cell` (ran by A, relied).** Per `div.cell`, take every `.cell-code`
    belonging to that cell (no nearer `.cell` or `.cell-output` ancestor); append each later block's `code` children to
    the first, joined with a newline (highlighting spans kept); drop each later block's copy-button scaffold and the
    whitespace after it; leave outputs alone. No per-unit markers exist and none are needed: merge by DOM position.
    *Keeps:* every code line and its order, each output's content, count and order, cell attributes, the cell count,
    everything else in the cell and everything outside `.cell`. *Discards:* which code line each output follows, how many
    code blocks a cell has, and the per-block copy wrapper. Order-safe because after the merge the recorded side reads
    code first and outputs after, the plain shape. Known gap: a recorded cell whose first unit hides its code but has
    output would put the output before the merged code; not implemented, absent from this corpus except in
    `capture-facts`.
15. **`julia/fig-subcap` loses a group caption, which decision 3 did not accept for figures (ran by A, relied).** Differences:
    (i) interleaving inside the group cell (the float div carries class `cell`), so the entry needs the interleaving
    normalizer too; (ii) panel wrapper: plain `div#fig-two-N.quarto-float > figure.quarto-float > div[aria-describedby]`
    plus a `figcaption` "Figure 2: First.", recorded `div.quarto-figure > figure > figcaption aria-hidden` "First.";
    (iii) group caption: plain "Figure 4: A group.", recorded "Figure 2" with class `quarto-uncaptioned`, so the text
    "A group." is **gone**; (iv) references: plain `Figure 2`, `Figure 3`, recorded `?fig-two-1?` with
    `quarto-unresolved-ref`. Unchanged: panel `img` src, alt and size, panel caption text, figure file names, `fig-one`.
    (ii), the numbers in (iii) and (iv), and the panel ids are the accepted wrong-to-unresolved change. The lost
    caption text is not numbering. Decision 3's probe established a dropped parent caption for tables only. **Gordon must
    decide** whether losing "A group." for figures is accepted; if so the entry's reason must say it. Side finding:
    `tbl-one` renders as a literal pipe-table paragraph on both routes, so the fixture exercises no table.
16. **`julia-subfigure-group` proposed form, and its blind spot (ran by A, relied).** Replace each panel with a token
    holding its image markup and its caption without `Figure N:`; map a reference to `#<group>-<n>` to one token when its
    text is `Figure N` or `?<group>-<n>?`; replace `Figure N` with `Figure #<id>` only where the caption and its
    references agree on N within that side. After it, the only remaining difference on the real pages is the group
    caption (4 lines, item 15). Controls caught: dropped panel image, changed panel caption, swapped panels, empty alt,
    a reference number inconsistent with its caption, an unrelated unresolved reference. **Blind spot:** a self-consistent
    renumbering of an unrelated figure (S5) is hidden. A too-broad regex that reduces every reference to its target hides
    the inconsistent-reference and unresolved-reference controls. The text uses a no-break space in `Figure N` on both
    routes, so patterns need `\s`; a literal space stops matching silently.
17. **Use the HTML parser already in the tree (read, ran by A).** `quarto-core` has `scraper` 0.26, `ego-tree` and
    `lol_html` as normal dependencies and `tests/integration/dashboard_format.rs` already calls `scraper::Html::parse_document`:
    no Cargo change. Both normalizers need balanced-tag handling (nested cell divs; panel wrappers three divs deep); regex
    is right only for the existing line-level rules.
18. **Trap: a parse-and-serialize normalizer hides more than it names (ran by A, relied).** Mutation M7 (reordered
    attributes, `&quot;` written as `"`) becomes equal after html5ever re-serialization. Either splice only the touched
    subtrees back into the original text, or round-trip both sides first and report a blocker when the raw texts differ
    but the round-tripped texts do not.
19. **Trap: the "rule used" bookkeeping (read, A).** `compare` marks a rule used when `p2 != p || r2 != r`. A
    re-serializing normalizer always changes the text, so it would always count as used and never look stale. Each
    normalizer must return a structural fire count per side.
20. **Failure modes and what to assert (A's list, my selection).**
    - *Stale entry:* for each allowlisted case, re-compare without each entry; an entry needed by no document in its scope
      fails the report test. This is also the `fig-subcap` removal test.
    - *Wrong document:* document normalizers use **exact names**, not prefixes (`julia/fig-subcap` would also cover a
      future `julia/fig-subcap-tables`). Interleaving must fire only on the recorded side (plain count 0); subfigure must
      fire equally on both.
    - *Differs for another reason:* run the regex rules on raw text before the normalizers; see items 18 and 19.
    - *Order-only changes:* `describe_difference` is a multiset line diff, so swapping two outputs (M2) is a Blocker with
      an empty diff. Add an ordered diff or a "same lines, different order" note.
    - The report lists, per entry, the documents it touched and the plain and recorded counts.

## Reconciling A and B: does the normalizer scope change what the gate must show?

Yes, in four ways. (1) The gate must run **before** the flip (item 1), after the normalizers and the 7i work. (2) The
"zero unallowlisted differences" box is satisfiable only if `capture-facts` gets the pinned-exact-diff entry (item 12) and
Gordon accepts the lost group caption (item 15); otherwise two of the four remaining cases stay blockers. (3) With 7i
landed, `docs-index-chunk` and `positron-cache` should become identical and `records-plot` needs only the interleaving
normalizer, and the normalizer must **not** hide the script lines. If 7i slips, 7d must not paper over it with a
whole-document rule: the plan should say "7d is not startable without 7i", which the frontmatter must also say. (4) Because
the harness then compares the recorded route with itself, the gate report is a before-flip artifact and the after-flip
evidence is the binary tests plus the end-to-end box.

## What the plan should change (proposed text; I edited nothing)

1. **Frontmatter and start condition.** Add `opr-7i` to `depends-on`. On the 7i branch this is done; it is correct there and
   absent here only because 7i has not landed. Add to "Start condition": "OPR-7i is `complete`."
2. **Reorder the checklist so the gate precedes the flip.** Replace boxes 3 and 4 with: "Run the OPR-0b/7c harness on the
   commit that has the normalizers, with no flip, and attach the report (zero unallowlisted differences). Then flip the
   default. After the flip the harness compares the recorded route with itself; keep it as a regression test of the
   recorded route and say so in the allowlist header." Reason: finding 1.
3. **Name the flip site.** "Flip the initial value of `present_from_record` in `EngineExecutionStage::new()` (reaches
   `pipeline.rs:398`, `:986` and `:1921`), and list the tests that used `new()` as plain." Reason: finding 3.
4. **Delete or reword "change the fallback diagnostic's condition".** Replace with: "Confirm `plan_capture` is quiet for
   `present_from_record` alone (done in 7a). Decide whether `--trace`, preview and hub observers count as an explicit
   record request; if not, add `record_requested` to `ExecutionContext`." Reason: finding 4(a).
5. **Julia on non-HTML targets.** Add a box: "Decide Julia's behavior on latex, beamer, pdf, pptx, docx and epub after
   the flip (presented by core today; `offers_presentation` has no target gate) and add one binary test per class."
   Reason: finding 4(b).
6. **Replace the whole-document design sketch's join-outputs step** with item 14's canonical form (code blocks only) and
   fix the table: normalizers use exact document names, are HTML-aware (`scraper`), and return fire counts. Add the
   controls of items 13 and 16 as the required negative controls, including M2 (order) and M7 (serialization).
7. **`julia/capture-facts` entry form.** "A pinned exact-difference entry: the entry stores the expected post-interleave
   diff; the document is allowlisted only if the actual diff equals it." Reason: item 12. Gordon signs it.
8. **`julia/fig-subcap` reason.** Add: "the group caption text is also lost (figures); Gordon accepted this on
   (date)." This needs Gordon's answer first (item 15). Update the allowlist header sentence that calls group references
   "not an allowlist candidate".
9. **Jupyter.** Move Jupyter out of the flip tests ("native Jupyter takes no flag; its HTML is identical before and after")
   and add the 7i decision 6 item: "native Jupyter and ipynb replay get the widget warning only; document it as a known
   difference."
10. **Replace the stale `id="cell-N"` example** with the id the presenter now writes (7f), and keep the claim "no reader of
    a rendered `id="cell-N"`" with B's grep scope; say freeze, listings, search and the profile checkpoint were not checked.
11. **Tests that run in CI.** State that the Julia and knitr binary tests skip in CI (no R or Julia installed there) and are
    local evidence; name the unit-level tests CI does run. Rewrite `the_seeded_allowlist_parses_and_every_entry_is_proposed`
    for the new entries.

## The first test I would write

In `opr_parity_harness.rs` tests, for `julia-interleaved-cell`: a hand-built plain page with one cell (one code block, two
stdout outputs `a` and `b`) against a recorded page with the same cell split into two code blocks, each followed by its
output. Assert `Allowlisted` with the rule named and a fire count of 0 on plain and 2 on recorded. **Negative control** in
the same test: the recorded page with the two outputs merged into one block `a\nb` must be a `Blocker`; this is the control
that the sketch's join-outputs variant would have hidden (M6). Second assertion pair: swap the two outputs, expect a
`Blocker` whose report says "same lines, different order".

## What I did not check

- I did not run the 7d flip, the harness after a flip, the preview or WASM chain, clippy, or any nextest run beyond what
  A ran (its narrowed gate run, relied). I did not run the seeded-allowlist test (item 10).
- Not examined by either sub-agent: knitr, Jupyter and marimo documents under the normalizers; other Julia documents
  (`positron-cache`, `docs-index-chunk`) under the normalizers; the repeat-render and cache path; the subfigure normalizer
  on Jupyter `fig-subcap`; whether the plain route ever emits more than one code block per cell (code-fold, `echo: fenced`);
  Windows.
- Line numbers from B (other than those in my re-run greps) and the `a_fallback_...` test name are (relied).
- I re-ran one claim from each sub-agent: from A, control M6 (narrow normalizer reports, join-outputs variant hides); from B,
  the `with_present_from_record` call sites, the `want_record.then` gate in `plan_capture`, and the three
  `EngineExecutionStage::new()` sites.
- Landing: the protocol describes landing only for plans and does not say review branches fast-forward into
  `feature/one-presenter`, so I left `opr/7d-review` unlanded.

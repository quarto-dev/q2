---
title: 'OPR-7j implementability review: could an unattended agent run the plan cold?'
date: 2026-11-10
description: 'Whether an implementer agent starting cold could execute OPR-7j to completion and get a correct result: the first test run against the unchanged tree, where the generic plot count has to live, the missing byte-identity recipe, the manifest formats that contradict the parity gate, the flip box, and the stop conditions the plan lacks.'
---

**Plan:** [OPR-7j](../plans/2026-11-10-opr-7j-group-caption-and-panel-ids.md) at the tip of `feature/one-presenter`
(`a2f989cec`). **Prior review:** [OPR-7j plan review](2026-11-10-opr-7j-plan-review.md).
**Method.** One agent, no sub-agents, in the lane C worktree. I read the protocol and the plan, then the presenter code
(`options.rs`, `cell.rs`, `knitr_inputs.rs`, `cell_step.rs`, `expansion.rs`, `document.rs`, `record_splice.rs`), and wrote
my findings down **before** opening the prior review. Then I read the review and marked each finding below as new or
already known. Marks: (read), (ran), (relied). Review only: I did not implement, did not flip a status, did not sign
anything and did not open a strand. Every prototype or scratch edit was reverted; the tree holds only the three
mechanical plan edits listed at the end.

## Verdict

**Ready after the listed edits.** The design is sound and the first test fails for the stated reason. Five things
would stop or mislead a cold implementer, in this order of cost: (1) the plan says the generic plot count is "filled for
every cell", but the obvious place to fill it, `CellInputs::from_cell`, is not on the path of an expanded unit, so the
count would be missing there (finding 4); (2) adding `gfm`, `typst` and `revealjs` to the `julia/fig-subcap` manifest
entry makes the parity gate fail on the figure directory name, which the plan says is 7d\'s, so the plan\'s own parity
box and THE FLIP box cannot both be satisfied (finding 8); (3) the byte-identity guard has no command, and its natural
form (a second build at the merge base) is a disk risk (findings 11 and 12); (4) the flip box needs two facts it does not
state (finding 14); (5) there are no stop conditions (finding 20). Three small mechanical corrections are already in
the plan. The rest is proposed text below, for Gordon to accept.

## Findings

Each carries a mark and "new" or "already in the review" (the review is the file linked above).

### Q1. Box by box

1. **Claim, branch and land (read; new).** The plan says lane D, `workspace-6`, branch `opr/7j-group-caption-and-panel-ids`.
   Startable by the protocol: status `ready` and `depends-on: [opr-7f]`, and 7f is `complete` (ran:
   `git grep` on `feature/one-presenter`). Nothing to ask.
2. **Box "Tests first" (read, ran; partly new).** An implementer can start it: the first test and its three negative
   controls are in the prior review, not in the plan, so a cold reader must open a second file. Proposed: inline them
   (text below). The plan also said none of the four knitr pins has a figure count of `None`; two of them
   (`each_figure_takes_its_own_subcaption`, `captions_count_figures_across_interleaved_runs`, both in `cell.rs`) build
   options with `from_knitr` and no figure count, so they are `None`; they have no label, so they never reach the
   relaxed gate. The other two live in `knitr_inputs.rs`, not in `options.rs` or `cell.rs`. Fixed in the plan (new).
3. **Box "Jupyter byte-identity guard" (read, ran; new).** The plan names a mechanism but no command, and it lists the
   13 shadow goldens "through `present_document`" without saying how. Finding 11 gives a recipe I ran. It cannot start
   as written without finding it.
4. **Box "A test per item 3 to 6" (read; new in part).** Six cases, each testable, but the "plot plus a widget" case needs
   a widget output fixture (`application/vnd.jupyter.widget-view+json`, which `has_widget_view` reads in `document.rs`);
   the plan does not say which. The knitr fallback chunk (item 6) is testable at the unit level only if the test builds a
   `RecordCell` with no resolved options: via `present_cell_step` it takes the `CellInputs` default path (read). Both are
   small; proposed one-line hints below.
5. **Box "A test through the binary" (read, ran; new).** `no_execute_cli.rs` is the named model but it renders a `.qmd`
   with failing cells, not a notebook. The fixture needs a `.ipynb` with stored outputs: write it inline as JSON in the
   test, or read `../quarto-core/tests/fixtures/opr-parity/jupyter/fig-subcap/doc.ipynb` (a cross-crate path;
   `include_str!` into a `crates/quarto` test is allowed, it is not `external-sources/`). Register the new file in
   `crates/quarto/tests/integration/main.rs` (alphabetical; `no_execute_cli` is registered there, read). I rendered that
   notebook with the unchanged binary and saw the three unresolved-crossref warnings and no `fig-two` id (ran), so the
   assertion "no unresolved-crossref warning" is the right negative.
6. **Box "A splice test at the `present_units` level" (read; new).** `document.rs` has a test helper at about line 1268
   that returns the reports of `present_units`; the stage test lives in `record_splice.rs`. The plan names neither.
   Say: put the test in `document.rs` tests next to that helper.
7. **Box "Implement" (read; new).** Startable, but see finding 4 for where the count goes; as written an implementer could
   put it in the wrong place and pass the unit tests.
8. **Box "Targets" (ran; new, and a contradiction).** On the unchanged tree I added `gfm`, `typst` and `revealjs` to the
   `julia/fig-subcap` entry and ran the harness (`OPR_PARITY_ONLY=julia/fig-subcap OPR_PARITY_KNITR=0`, 70 seconds, Julia
   and Plots present). All four formats are blockers today, as expected, but **every non-HTML format also differs in the
   figure file names**: plain writes `doc_files/figure-gfm/`, `figure-typst/`, `figure-revealjs/`; recorded writes
   `doc_files/figure-html/` (the "figure" lines of the report). The 7d plan says this is gfm only and presenter-wide;
   it is true of typst and revealjs as well (ran). After 7j these three formats would still differ by interleaving **and**
   the figure directory, so the "Parity gate" box ("differs from plain only by the interleaved code blocks") and THE FLIP
   condition 2 would both fail on the added formats, through no fault of 7j. Proposal in finding 8a below. I reverted
   the manifest change.
9. **Box "Parity gate" (read; new).** No command. Added to the plan (mechanical). Also: the report has a "not compared"
   section; a gate that skipped a case is not green. Added in the same edit.
10. **Boxes "Stale comments", "Snapshots", "Docs note", "Allowlist header" (read; new in detail).** All startable and
    checkable. Line ranges for the comments (164 to 175 and 1165 to 1181 of `crossref/codeblock_shorthand.rs`) still hold
    (read); `figures.qmd` "Subcaptions" is at about line 474 (read). The allowlist header sentence is at lines 14 to 16 of
    `opr-parity-allowlist.toml` (read) and the manifest note containing "blocker" is on the `julia/fig-subcap` entry (read).
    "Snapshots: expected zero" is checkable by `git status` after the gates.

### Q2. Code reality

11. **Every symbol the plan names exists and the described behavior holds (read).** `label_on_cell` (`options.rs`, about
    line 615) is gated on `figure_count.is_some()`; `plot_image_id` (about 645) returns `None` without it; `cell_caption`
    (about 635) gates on `label_on_cell`; `plot_image_attrs` (about 657) calls `plot_image_id`; `CellInputs::from_cell`
    fills `figure_count` only when resolved options exist and counts `is_plot_output && !is_included_graphic`;
    `interleave_group` declares `figures` once per cell group; the plot arm is in `format_outputs_with` (about
    `cell.rs:977`). `cell_div_id` has no caller (ran: `grep` over `crates`; the only hit is its definition) (already in
    the review).
12. **Where the plot count has to live (read; new, the main finding).** The plan says the count is "filled for every
    cell" and "consulted only when `figure_count` is `None`". Three places an implementer might fill it, and what each
    misses:
    - `CellInputs::from_cell`: **not on the expansion path.** `present_cell_step` returns early into `render_expanded_unit`
      (`expansion.rs`, `render_each`), which builds `CellOptions` per expansion and calls `render_cell_with` directly,
      never reading `CellInputs`. A Julia or Jupyter expanded unit would have no count and so no group. Also not on
      the `fallback` options path of `present_with_inputs`.
    - The `figures` counter in `interleave_group`: it is the **index** source (cumulative across runs), not a count; a
      per-run count would be wrong because `format_outputs_with` is called once per run of equal anchors.
    - `wrap_cell`: it sees only options and the finished body.
    The one place that sees the whole output list for every route is where options meet the result:
    `render_cell_whole` (called by `render_cell_with`, so by expansions and by the non-interleaved route) and
    `render_cell_interleaved` (after `select_by_kind`, which shadows `result` and `options` with filtered copies). So:
    add a field and a `with_plot_count` to `CellOptions` (`options.rs`), and set it on a clone at the top of
    `render_cell_whole` and in `render_cell_interleaved` after `select_by_kind` (`CellOptions` is `Clone`, read). Functions to
    change: `label_on_cell` and `plot_image_id` in `options.rs`; `render_cell_whole` and `render_cell_interleaved` in
    `cell.rs`. Not changed: `knitr_inputs.rs`, `cell_step.rs`, `expansion.rs`, `document.rs`. Proposed plan text below.
13. **Two small traps in the count (read; the second new).** (a) The count excludes included graphics but the `figures`
    counter includes them, so a plot plus an included graphic numbers the plot by position (the plan\'s item 5 says so).
    (b) The plot arm increments `figures` only when `write_image_at` succeeds; a plot whose image write fails falls to the
    text arm but still counted in the plot count, so a two-plot group with one failed write is a "group" with one
    panel. An image-write failure is already reported elsewhere (`image_failures`); say in the notes that the group
    markup follows the count, not the writes. Not worth a code path.
14. **`select_by_kind` and `echo` selection produce filtered options (read; new).** `render_cell_interleaved` can return
    through `render_cell_whole` when anchors are unusable. Setting the count in both places, after the filter, covers
    both exits; a single place would not.

### Q3. Test first

15. **The first test fails on the unchanged tree for the stated reason (ran; the test is the prior review\'s, the run is
    new).** In `presenter/cell.rs`, module `option_tests`, next to `each_figure_takes_its_own_subcaption`:
    `opts(json!({"label": "fig-two", "fig.cap": "A group", "fig.subcap": ["First", "Second"]}))`, two `plot()` outputs,
    `render_figs`; assert `::: {#fig-two .cell}`, `){#fig-two-1}`, `){#fig-two-2}`, and the output ends `\nA group\n:::`.
    `cargo nextest run -p quarto-core --lib -E 'test(scratch_group)'` built `quarto-core` and failed in 0.15 seconds
    (1 failed, 6223 skipped): the first assertion fails, and the printed markdown is `::: {.cell}` with no id and the
    panels `![First](...)` and `![Second](...)` with no `{#...}`. That is "no wrapper label, no panel ids", the reason the
    review gives. I reverted the test (`git checkout`). A cold build of this test binary took a few minutes on this
    machine. Negative control (a) (no `fig-subcap`) and (c) (single labeled figure) pass today and must still pass;
    control (b) (one plot, one subcap: no `fig-two-1`) also passes today and exists to catch the prototype\'s bug. So
    none of the three controls can fail before the change; their job is to fail **after a wrong change**. Say so in the
    plan, so an implementer does not think a passing control is a defect.

### Q4. The byte-identity guard

16. **The recipe, run on the unchanged tree (ran; new).** The plan has no recipe. This one works as written, and I ran
    its comparison with one binary on both sides (`target/debug/q2`, built 03:08 today; I did not confirm it equals the
    tip, and for a same-binary proof it need not):

    ```bash
    S=<scratchpad>; F=crates/quarto-core/tests/fixtures
    # copy the 8 stored notebooks into $S/base and $S/new: the six jupyter-markdown-goldens/ipynb-*.ipynb, and
    # opr-parity/jupyter/{ansi-output,fig-subcap}/doc.ipynb (renamed parity-<dir>.ipynb)
    (cd $S/base && for n in *.ipynb; do $S/q2-base render $n --to html 2> $n.err; done)
    (cd $S/new  && for n in *.ipynb; do $S/q2-new  render $n --to html 2> $n.err; done)
    diff -r -x render-manifest.json -x '*.err' $S/base $S/new
    ```

    Same binary both sides gave identical HTML and `_files` for all 8; the only differences were absolute paths in
    `.quarto/render-manifest.json` and in the stderr line "Rendering single file: ...", hence the two `-x`. Eight
    notebooks, no kernel needed. Today the `parity-fig-subcap` notebook renders with three unresolved-crossref warnings and
    only `fig-one` ids (ran), the baseline the changed binary must improve; every other pair must show no difference.
17. **What the recipe cannot cover (read, ran; new).** The 13 shadow goldens are `record.json` plus a qmd; rendering them
    through the binary needs `execute: record-sources` metadata and is not a one-liner. They are covered by existing
    kernel-free tests, `jupyter_markdown_goldens::stored_records_present_to_the_goldens` and
    `jupyter_markdown_matches_goldens` (read; I did not run them). Say in the plan that the 13 are guarded by those tests
    (run `-E 'test(jupyter_markdown_goldens) | test(presenter_pinning) | test(record_preview_splice)'`) and the 8 notebooks
    by the binary diff, rather than "render every fixture with two binaries".
18. **Build order and disk (read, ran; new).** The merge-base binary does not need a second worktree: tests-first edits
    touch only test code, so build the base `q2` right after the tests-first commit (or before any production edit), copy
    `target/debug/q2` to the scratchpad as `q2-base`, and build again after the implementation for `q2-new`; the second
    build is incremental. The volume holding the repositories is 94% full with 60 GB free (ran: `df`); a second
    full `target/` is about 12 GB (the integration-test rule\'s measurement), so a second worktree is the wrong
    default. Proposed text below.

### Q5. Re-runs

19. **Two claims re-run.** (a) The splice route: "`present_document` and the splice stage both call `present_units`, which
    reaches the cell step" (relied in the plan). **Holds (read):** `record_splice.rs` line 260 calls `present_units`;
    `document.rs` line 372 does the same for `present_document`; `present_units` calls `present_cell_step` at line
    254 for every `Unit::Cell`. One caveat the plan has right: a unit with stored markdown, or hoisted outputs removed by
    `without_outputs`, takes other branches first, so a group cell whose outputs were hoisted for page dependencies
    would be presented without those outputs; none is a plot, so no effect on groups. (b) "No Jupyter golden contains a
    real `fig-subcap` cell" (read in the plan). **Holds (ran):** `grep` finds `fig-subcap` in 13 `record.json` files, all
    at the line of an option-name list (`ceded-ojs.record.json:45`, `:87`, and so on) and none in a `.md` golden or in
    `jupyter-record-shadow/*.qmd`. One more, since it changed the verdict: the plan\'s "7i (in progress in lane D)" is
    **wrong**: 7i is `complete` on the integration branch (ran: `git grep`). Fixed in the plan.

### Q6. Contradictions

20. **With the `.cell` wrapper contract, the phases rule, D14 and 7i: none (read; already in the review).** The group id
    moves onto the one `Div.cell`; the rule in AGENTS.md about phase ranks concerns AST transforms, and the presenter runs
    before them; D14 says what 7j does; 7i\'s label concerns page dependencies and sets nothing here.
21. **With 7d (read; new in part).** (a) Decisions 3 to 5 and "Julia on non-HTML targets" fit, except the figure-directory
    scope: 7d says the directory difference is gfm\'s; I measured it on gfm, typst and revealjs (finding 8). 7d\'s
    per-class rule ("a class goes through the presenter only when a parity case differs from plain by the accepted
    interleaving alone") then excludes all three for Julia unless the directory is fixed, so the 7d item must be widened
    from "gfm" to "every non-HTML class". (b) 7d runs its own parity gate "on the commit that has the normalizer"; at
    7j\'s landing the four interleaving cases are therefore **unallowlisted blockers by construction** (ran: the report
    above is that shape for `julia/fig-subcap`), so THE FLIP box has to recognize them by name, not by absence of
    blockers (finding 14). (c) 7d\'s normalizer and its four interleaving cases do not conflict with the group markup:
    the group cell is split by interleaving and wrapped once, so the caption closes the div after the last block.
22. **The later-figure table against its definition (read, checked by hand; already in the review for the rows).** Rule: a
    group of `n` panels counts as `n + 1` figures. Row 1: panels 2 and 3, group 4, later 5. Row 2 (n = 3): panels 2 to 4,
    group 5, later 6. Row 3: group A has panels 2 and 3 and number 4, group B panels 5 and 6 and number 7, later 8. Row 4:
    panels 1 and 2, group 3. All four rows agree with the definition, and "`n` too high against Quarto 1" holds per group
    (row 3: Quarto 1 says 4, q2 says 8, two groups of two).
23. **Wording slips in the plan (read; new, harmless).** "Two cautions" introduces four bullets; the "Prototype
    evidence" section says 810 tests passed with a filter that includes `test(presenter)`, which would also match this
    plan\'s new tests (it was run before them). Neither misleads; I left both.

### Q7. Gaps an implementer will hit

24. **Test locations (read).** Unit tests: `cell.rs` `option_tests` (helpers `opts`, `render_figs`, `plot`) and
    `options.rs` tests; `present_units` test in `document.rs` tests; the binary test in
    `crates/quarto/tests/integration/<name>.rs` plus `main.rs`; no new top-level `tests/<name>.rs` (the integration-test
    rule). The parity manifest change is TOML only. No new snapshot is expected.
25. **Goldens and snapshots (read).** Expected zero changed files. If an implementer commits a Jupyter group golden
    (optional), it is three new files, and the `every_golden_file_is_consumed` test fails on any unread file in the
    goldens directory (read in the README), so a golden needs a test that reads it. Say so.
26. **The Windows cross-clippy (ran by the user\'s note; read in the plan).** The plan lists
    `cargo clippy ... --target x86_64-pc-windows-msvc` unconditionally. On macOS it fails in the `aws-lc-sys` build script,
    a known issue the plan should not make a gate. An unattended implementer would either loop on it or declare
    failure. Proposed: "run it; if it fails in a build script (aws-lc-sys), record that in the notes and go on; the
    change touches no `cfg` code".
27. **WASM (read, ran).** `npm run build:wasm` needs `npm install` from the repository root first; `hub-client/node_modules`
    is absent in this worktree (ran: `ls`), and a lane workspace may be the same. The `wasm32-unknown-unknown` target is
    installed here (ran: `rustup target list --installed`). The two WASM vitest files exist at
    `hub-client/src/services/recordPayload.wasm.test.ts` and `captureSpliceHtml.wasm.test.ts` (read); the script is
    `npm run test:wasm` with a vitest filter, which the plan does not give. Proposed text below. 7j changes no file under
    `hub-client/`, so no changelog entry.
28. **The docs note (read).** The OPR-8 note is a note in 7j\'s own notes ("Notes from the run"), since the plan says "for
    OPR-8" and does not edit the docs; an implementer might edit `figures.qmd`. Say "do not edit docs/; write the note in
    Notes from the run".
29. **Does it need Stop conditions like 7i? Yes (read).** The 7i plan has them; 7j has none. Proposed text below.

### Q8. Size

30. **One plan, about six commits (judgment).** (1) claim; (2) tests first (unit); (3) implementation (`options.rs`,
    `cell.rs`) with the tests green; (4) binary test, splice test and the edge-case tests; (5) manifest, stale comments,
    allowlist header, plan reconcile; (6) flip. Splitting would not help: the changes share one gate. **Riskiest step:**
    the plot count placement (finding 12), because a wrong place passes every unit test written against
    `render_cell_with` and fails only on expanded units and on the interleaved route. Second: the Jupyter later-figure
    numbers shift for real documents (accepted in D14), which the byte-identity guard cannot show because the one
    changed fixture is the expected change.

### Q1 again: THE FLIP box, checked separately

31. **Are its conditions checkable by an agent, and true today? (read, ran; new).**
    - "every plan in 7d\'s `depends-on` is `complete`": checkable by `git grep` on the integration branch (the protocol\'s
      command). Today 7a, 7b, 7c, 7f, 7g, 7h and 7i are `complete` and 7j is `ready` (ran). **The list includes 7j itself:**
      the condition can only hold after the 7j branch sets `status: complete`, which the protocol\'s landing step 3 does
      in the same branch. The box should say "after setting this plan complete". True once that is done.
    - "the parity gate shows no unallowlisted difference except the four interleaving cases": checkable by running the
      harness (command in finding 9; many minutes; R, Julia, Plots required). Today the allowlist has three entries
      (`knitr-empty-cell-wrapper`, `knitr-adjacent-blockquotes`, `julia-code-leading-newline`) and the four interleaving
      cases do not exist yet (7d writes them), so on the corpus the agent sees them as blockers by name: `julia/capture-facts`,
      `julia/presentation-attrs`, `julia/records-plot`, `julia/fig-subcap` (read from 7d decision 5). With
      the plan\'s added non-HTML formats the gate would show more (finding 8), so condition 2 fails unless the manifest edit
      changes. A `not compared` or `skipped` case must also count as not passed.
    - "every existing allowlist entry is `signed`": checkable; today all three are `signed` (ran: `grep status` on the file).
    - "no plan note says needs Gordon about anything still open": **not mechanically checkable as written.** I grepped 7d
      for the two literal forms (`needs Gordon`, the bracketed Gordon label) and found none (ran), but the notes use other
      wording. Proposed: define it as that grep over 7d and the epic, plus the status of every `depends-on` plan.
    - The protocol forbids agents flipping `blocked-on-gordon` plans (read); the box is an explicit, recorded exception
      (Gordon, 2026-11-10). Say so in the box so a cold agent does not stop at the protocol sentence.
    - It edits two places (7d frontmatter and the epic table row, which today reads `blocked-on-gordon`); the box says so.

## The prior review: new, known and disagreements

Marked per finding. **Already in the review:** `cell_div_id` has no caller; the Jupyter golden grep; the later-figure
numbers; the first test and its controls; the per-cell counter; the splice route claim (relied there, read by me);
contradictions with the wrapper, phases, D14 and 7i; the unreviewed fallback chunk case. **New here:** findings 2 (the
two pins with no figure count), 3, 5, 8 (figure directory on three formats, which breaks the parity box), 12 to 14 (count
placement; the expansion path), 15 (the run), 16 to 18 (recipe, run, disk), 19 (7i complete), 26 to 29, and the flip
analysis in 31. **Disagreements:** (1) the review says 7i is "in progress" or unread (its finding 16); it is complete, so
the review\'s "whichever lands second rebases" is moot. (2) The review recommends adding the three formats to the
manifest as a plain improvement; I measured that it adds a figure-directory blocker the plan assigns to 7d (finding 8).
(3) The review says "units in `present_units` are whole document cells", true, but an **expanded** unit is still one unit
with several presented cells; the count must be per presented cell, which is why `CellInputs` is the wrong place.
No disagreement on the verdict direction.

## Proposed plan text (not applied; each changes a design or a gate)

**Update, applied at Gordon\'s request:** 8a is now in the plan, in this form: 7j\'s parity gate is HTML only, and the
non-HTML `fig-subcap` parity check moved to 7d (its "Julia on non-HTML targets" item now has step 5, which adds the three
formats to the manifest after the figure directory is fixed for every non-HTML class). The 7j plan also says the harness
does not check those formats before 7d. Everything else below remains unapplied.

**8a. Targets box.** Replace "Add `gfm`, `typst` and `revealjs` to the `formats` of the `julia/fig-subcap` manifest entry"
with: "Do not add `gfm`, `typst` or `revealjs` to the manifest in this plan. On the unchanged tree each shows a
figure-directory difference (`figure-html` against `figure-<format>`) besides interleaving (ran, implementability review),
which is 7d\'s item and would make this plan\'s parity box and THE FLIP condition 2 unsatisfiable. Verify the group markup
on those formats by rendering the Jupyter notebook through the binary once per format and reading the caption, id and
reference lines, as the plan review did; write the lines in Notes from the run. 7d adds the formats when it fixes the
directory (widen its item from gfm to every non-HTML class)." *Alternative for Gordon:* add them anyway and have the flip
box tolerate the directory lines; I recommend against, because it weakens the one mechanical gate.

**12a. Implement box, the count.** Add: "The plot count lives on `CellOptions` (`plot_count: Option<usize>`,
`with_plot_count`), set on a clone at the top of `render_cell_whole` and in `render_cell_interleaved` after
`select_by_kind`, from `result.outputs` with `is_plot_output && !is_included_graphic`. Do not fill it in
`CellInputs::from_cell`: an expanded unit never reads `CellInputs` (`present_cell_step` returns into
`render_expanded_unit`), and not from the `figures` counter (that is the index). `label_on_cell` and `plot_image_id`
read `figure_count` first; with `None` they use `plot_count >= 2`. Add a test that an expanded unit (a unit with
`quarto_expansion` markers and two plots in one expansion) renders the group."

**16a. Byte-identity box.** Replace the mechanism with the recipe in finding 16, and: "build `q2-base` before the first
production edit (`cargo build --bin q2`, copy to the scratchpad), `q2-new` after the implementation; do not make a second
worktree (disk is tight). The 13 shadow goldens are guarded by `-E 'test(jupyter_markdown_goldens) | test(presenter_pinning)
| test(record_preview_splice)'`. Expected diff: only `parity-fig-subcap.html` and its warnings."

**Flip box.** Replace the conditions with: "After setting this plan\'s `status: complete`: (1) `git grep -h '^status:'
feature/one-presenter -- ` for each plan in 7d\'s `depends-on` shows `complete`, 7j counted; (2) the harness report (run it;
R, Julia and Plots must be present, and `not compared` and `skipped` must be empty for Julia and knitr) lists as blockers
exactly `julia/capture-facts`, `julia/presentation-attrs`, `julia/records-plot` and `julia/fig-subcap` for html, and none
else; (3) `grep status` on the allowlist shows only `signed`; (4) `grep -n 'needs Gordon'` and the `Gordon` label grep over 7d
and the epic print nothing open. This box is the recorded exception to the protocol\'s \'never flip\' rule. Change only 7d\'s
`status:` line and its epic-table row."

**Windows and WASM bullets.** "Windows cross-clippy: run it; a failure inside a build script (`aws-lc-sys`) is a known
macOS issue; record it in Notes from the run and continue. WASM: `npm install` from the repository root if
`hub-client/node_modules` is missing, then `cd hub-client && npm run build:wasm` and `npm run test:wasm -- recordPayload
captureSpliceHtml`."

**Stop conditions (new section, modeled on 7i).** Stop, commit progress, add "Notes from an earlier attempt", set `ready`,
land only the notes commit and report, if: (1) the first test passes on the unchanged tree; (2) any Jupyter golden or
snapshot changes, or the binary diff shows a changed notebook other than the `fig-subcap` one; (3) the one-plot group still
renders two figures after the change; (4) the count has to be filled in `CellInputs` to pass a test (that means an
expansion path was missed: stop and say which); (5) the parity report shows a new blocker other than the four interleaving
cases or the figure-directory lines of non-HTML formats; (6) the machine cannot build a second `q2` (disk) or the base
binary cannot be made before the production edit; (7) THE FLIP\'s conditions do not all hold: do not flip, list which.

**Inline the first test and controls** (from the prior review, with finding 15\'s note that the three controls pass today).

## What I did not check

- The implementation itself, clippy, the full `quarto-core` suite, the 810-test filter, the WASM build and the WASM
  vitest files: not run (review only; the machine built one test binary and the parity harness only).
- Julia plus knitr group renders through the changed code, expanded units with plots (I have no fixture that carries
  `quarto_expansion` plots), a plot-plus-widget group, a knitr fallback chunk reaching the cell step, and any real
  Quarto 1 render.
- The 13 shadow goldens through the binary (not possible as one-liner) and the existing golden tests (not run).
- That `target/debug/q2` equals the tip; it served only to prove the harness mechanics with one binary on both sides.
- The two non-HTML measurements beyond `julia/fig-subcap` (the figure directory on other Julia cases is read from 7d, not
  re-measured).
- Hub-client behavior in a browser; the docs site.

## Plan edits applied (mechanical only)

1. The 7i sentence: 7i is `complete`, nothing to rebase (a wrong status).
2. The "Tests first" box: which pins live in which file, and that two have no figure count but no label (a wrong claim and
   missing paths).
3. The "Parity gate" box: the harness command and the "not compared" rule (a missing command).

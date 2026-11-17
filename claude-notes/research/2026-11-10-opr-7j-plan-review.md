---
title: 'OPR-7j plan review: the group rules for every engine, the one-panel hole, the Jupyter numbers'
date: 2026-11-10
description: 'A review of the OPR-7j outline against the code, real rendered pages (Julia, Jupyter, knitr) and Quarto 1: which edge cases the prototype gets wrong, what the plan contradicts, which targets and tests it needs, and the plan edits made.'
---

**Plan:** [OPR-7j](../plans/2026-11-10-opr-7j-group-caption-and-panel-ids.md) (outline) at `b69101b85`.
**Method:** I read the protocol, the plan, epic D9 and D14, the 7d review, the subfloat draft, the 7d plan and the presenter
code, then ran two sonnet sub-agents in parallel (1: implementation and edge cases; 2: Quarto 1, contradictions, other
targets, tests, fit). Marks: (read), (ran), (relied) for a sub-agent claim I did not re-check. I re-ran one claim from
each (below) and wrote this report myself. Review only: no implementation, no status flip, no allowlist signature, no
strand; I edited the 7j plan and made the minimal consistent edits to the 7d plan and the subfloat draft.

## Verdict

**Not ready to implement as written; ready after the plan edits made here, plus one answer from Gordon.**
The idea holds: the markup 7j writes (cell id, closing group caption, `label-N` on each panel) is Quarto 1\'s own. Three
things were wrong or missing. (1) The prototype turns a one-plot, one-subcap group into two figures on Julia and Jupyter
where knitr and plain Julia give one, and the outline\'s fix (a generic `figure_count`) would break the byte-identical
rule for single-figure Jupyter cells. (2) 7j changes the number of **every later figure** in a Jupyter document (Quarto 1
3, today 2, with 7j 5), which D14 never covered. (3) The byte-identity guard named a mechanism the harness cannot supply.

## Findings

1. **The markup is Quarto 1\'s; the numbers are a q2 accident (ran by sub-agent 2 on Quarto 1, relied).** A Quarto 1 build
   runs here (`external-sources/quarto-cli/package/dist/bin/quarto`). On `jupyter/fig-subcap` with `--no-execute` it gives
   the group caption "Figure 2: A group.", panels `div#fig-two-1` and `div#fig-two-2` captioned "(a) First." and "(b)
   Second.", and references "Figure 2", "Figure 2 (a)", "Figure 2 (b)". The knitr fixture gives the same. Source (read):
   panel ids from `label + "-" + nextOutputSuffix++` (`to-markdown.ts`, here the `quarto-api` copy), the closing caption
   from `cellCaption`, the cell label gated by `shouldLabelCellContainer` (`labels.ts`). Q1 Julia failed in this
   environment (a Plots error), but the Julia engine calls the same `toMarkdown` (read), so I treat Jupyter as Julia.
   "Same as knitr and plain Julia today" is therefore true of the markup and false of the numbers.
2. **Jupyter later-figure numbers change, and the plan said they did not (ran, re-run by me).** A stored notebook with
   one figure, a `fig-subcap` group, then one more figure: Q1 numbers the last figure 3 (relied); the unpatched binary
   gives "Figure 2: After." (the group is not a figure); the prototype gives "Figure 5: After." (panels 2 and 3, group 4).
   I re-ran both q2 binaries on the sub-agent\'s notebook and saw exactly those lines. The plan\'s "wrongness predates
   this plan" is true for knitr and Julia only. **Needs Gordon** (question 1 below); I wrote the recommendation (accept)
   into the plan.
3. **A one-plot group is two figures on Julia and Jupyter (ran by sub-agent 1, re-run by me).** A cell with one plot and
   one `fig-subcap` entry: knitr gives "Figure 1: Grp A." (its `figure_count` is 1, so the image gets the bare `#label`);
   plain Julia the same; the prototype gives "Figure 1: Only." and "Figure 2: Grp A.", with `@fig-a-2` still unresolved
   (my re-run: ids `fig-a-1`, `fig-a`, one warning). Cause: the `None` branch of `plot_image_id` always writes `label-1`.
   Quarto 1 does not label the cell for one output (`shouldLabelCellContainer`).
4. **The outline\'s fix for item 3 would break D14 (read, sub-agent 2, confirmed against the code).** Open question 3 said
   to compute `figure_count` generically. OPR-1a\'s Deviations record that `from_cell` fills it only with resolved options,
   because "filling them for Jupyter would change `echo: fenced` and figure output", and `plot_image_id` with `Some(1)`
   writes a bare `#label` on the image (`options.rs`), which would change every single-figure labeled Jupyter cell. The
   plan now adds one separate generic plot count, consulted only when `figure_count` is `None`, and a one-plot group is
   not a group.
5. **The engine-id branch is not needed (read, not run).** It fires only when the presenter has no figure label but the
   engine wrote a `fig-` id; `build_head` lets the presenter id win over the engine id (`engine_attrs.rs`, read by
   sub-agent 1). Nobody ran the branch alone. The plan now says: write the Julia group test without it; add it only if
   that test fails; no `EngineAttrs::id()` getter.
6. **The counter is per cell and survives interleaving (read, ran).** "Units" in `present_units` are whole document cells
   (`document.rs`), `interleave_group` declares `figures` once per cell (`cell.rs`), and a real Julia cell with
   `println`, two `display` calls and an echo gave `fig-r`, `fig-r-1`, `fig-r-2`, no duplicates and no warnings, also with
   `echo: false` and `results: asis`. The outline\'s worry (label-1 twice) does not occur. The counter also counts
   included graphics, which carry no id; a plot-plus-graphic group and a plot-plus-widget group were not rendered.
7. **A user-written panel id duplicates (ran by sub-agent 1, relied).** A cell whose output markdown carries
   `{#fig-l-1}`, plus two plots, gives `id="fig-l-1"` twice and a broken caption; the unpatched binary does not. Plan:
   accept as author error and pin with a test.
8. **Edge cases that match knitr (ran by sub-agent 1, relied).** More subcaps than plots, `layout-ncol`/`layout-nrow`,
   `fig-cap` alone, no label, `echo: false`, `results: asis`, `include: false`, `output: false` match knitr or plain Julia
   as the plan now lists. Differences accepted because they are knitr\'s rule: fewer subcaps than plots recycles
   (C1, C2, C1; plain left the third panel uncaptioned); a list `fig-cap` uses the first entry (knitr itself errors on a
   list `fig-cap` in these renders, a separate engine bug). `fig-subcap: true` stays unresolved on every engine, head and
   knitr included, so the plan must not promise it. A non-`fig-` label keeps only `foo` on the recorded route where plain
   Julia wrote `foo-1`, `foo-2`; 7j does not change this, but the flip does; not figure-group related, noted for 7d.
9. **Contradictions (read, sub-agent 2).** One real one: finding 4. None with the `.cell` wrapper contract (a group is
   still one `Div.cell`, the id moves onto it; a hidden group emits no stray caption, ran), the phases rule and the
   no-DOM-postprocessor rule (presentation-time cell markdown, format-agnostic), OPR-1b, 4d, or 7f (the presenter\'s
   wrapper id already wins, test `a_presenter_wrapper_id_wins_over_the_engine_id`). One stale item outside the plan: the
   comments in `crossref/codeblock_shorthand.rs` say a `fig-subcap` cell is left for the engine to synthesize the panels;
   after 7j the presenter does for Jupyter and Julia. Added to the checklist.
10. **Other targets (ran by sub-agent 2 on Jupyter, relied).** Warnings: head 3, prototype 0 on every format. html and
    revealjs: the caption, reference and id lines of knitr plain, knitr recorded and Jupyter prototype are byte-identical.
    gfm: nested `div#fig-two` with the two panels. typst: the prototype writes `#quarto_super(... subcapnumbering: "(a)",
    label: <fig-two>)` with `<fig-two-1>` and `<fig-two-2>`, the shape knitr recorded already writes; head wrote an
    unlabeled grid. docx: the same paragraph sequence as knitr. epub: "(a)", "(b)" captions with in-text references 4, 2,
    3, as knitr already does. pptx: the group is on no slide before or after. So the new markup is safe on these classes;
    7j extends an existing inconsistency to Jupyter and Julia and creates none. Julia on non-HTML was not reproduced (below).
11. **The gfm `figure-html` directory is presenter-wide (read, ran by sub-agent 2).** `FsFigureSink` hardcodes
    `{files_dir}/figure-html/`; native Jupyter writes it on gfm today; knitr names its own `figure-gfm`; Quarto 1 writes
    `figure-commonmark`. `plot_image_id` has no format dependence (read). 7d\'s item (3) is rewritten.
12. **The harness does not cover the new markup on other formats (read).** `COMPARABLE` is html, gfm, revealjs, typst
    (`opr_parity_runner.rs`), but all three `fig-subcap` manifest entries list `formats = ["html"]`. Added to the checklist;
    7d\'s Julia per-class seed gets a group cell. The harness also cannot guard Jupyter byte-identity: both routes present
    through one function, so `jupyter/fig-subcap` compares a render with itself.
13. **Tests: what runs in CI, what changes (read, ran by sub-agent 2).** Unit tests and a `present_units`-level splice
    test run in CI. A stored `.ipynb` renders through `q2 render` with no kernel (ran), so the binary test for Jupyter
    runs in CI; Julia and knitr versions are local evidence. Expected changed goldens or snapshots: **zero** (read, grep: no
    Jupyter golden holds a real `fig-subcap` cell; the strings in `jupyter-markdown-goldens/*.record.json` are option-name
    lists). A new committed Jupyter group golden would be three files. WASM: yes, `presenter/` is in the protocol\'s list;
    add the two WASM vitest files OPR-1a ran.
14. **Splice (read, relied).** `present_document` and the splice stage both call `present_units`, which reaches the cell
    step, so one test at that level covers both. The splice writes the AST back as qmd with different attribute quoting,
    so "same AST and page", not "same bytes" (the plan said bytes). No hub-client or ts-packages code reads these ids or
    captions (grep, sub-agent 2); the preview renderer treats a nested Figure generically; I did not see it in a browser.
15. **Docs (read, relied).** `docs/guides/authoring/figures.qmd` (about line 476) promises "(a)" subcaptions; after 7j HTML
    shows "Figure 2: First." on every engine and later figures differ from Quarto 1. A note for OPR-8 was added.
16. **Fit (read).** The subfloat draft: 7j is consistent and eases Phase 3 (the markup is Q1\'s, option A is untouched, B
    gets cheaper); the later-figure numbers go into its Interaction paragraph. The 7d plan: gate, normalizer scope and
    decision 3 fit; its "nothing about figures changes" is true for Julia, not Jupyter, and 7d\'s line is fixed. 7j keeps
    `depends-on: [opr-7f]`; adding 7i is not needed (7i edits `presenter/cell.rs` but not the group rules; whichever lands
    second rebases; I did not diff 7i\'s branch). 4d, 1b, 7a and 7h are complete and transitive.
17. **The prototype patch is not persisted.** It lives in another session\'s scratchpad. Prototype code does not go in
    the tree, so the plan now spells out its two edits in words.

## The two re-runs

- Sub-agent 1, the one-subcap case (finding 3): I rendered `E/julia/one-subcap` with the prototype binary and
  `execute: record: true`: "Figure 1: Only.", "Figure 2: Grp A.", ids `fig-a`, `fig-a-1`, one unresolved warning for
  `@fig-a-2`. As reported.
- Sub-agent 2, the Jupyter later-figure numbers (finding 2): head "Figure 1", "Figure 2: After." (group absent);
  prototype "Figure 1: One figure.", "Figure 2: First.", "Figure 3: Second.", "Figure 4: A group.", "Figure 5: After.".
  As reported. Quarto 1\'s "Figure 3" remains (relied).

## Plan edits made

In the 7j plan: the numbering paragraph (Jupyter is not predated); the fit notes (Q1\'s markup, the 7i rebase); the
prototype paragraph (patch spelled out, engine-id branch dropped); the five open questions replaced by "Settled by the
review" (nine items, including the generic plot count and the one-plot rule); an "Open for Gordon" paragraph with a table
of the later-figure numbers; the checklist (byte-identity mechanism, binary test in CI, splice test, targets and manifest
formats, stale comments, docs note, exact snapshot expectation, WASM vitest files); exit criteria; a "Review findings"
section. In the 7d plan: decision 3\'s Jupyter line, the gfm directory finding, checklist item for Julia non-HTML (seed
document and item 3). In the subfloat draft: the Interaction paragraph (later-figure numbers; a later-figure case in the
corpus). No status flipped, no strand, no code.

## Questions for Gordon

1. **Jupyter later-figure numbers (finding 2).** Accept that after 7j every figure after a Jupyter `fig-subcap` group is
   numbered two too high instead of one too low (Quarto 1: Figure 3; today 2; with 7j 5), the same as knitr and Julia
   already are? **Recommendation: accept**, record it in D14; the fix is the subfloat plan\'s `parent` link. If not, 7j
   waits for that work.
2. **The plan\'s status.** It is still `blocked-on-gordon` (outline). With the edits it is startable once you flip it to
   `ready`.
3. **A user-written panel id (finding 7).** Accepted as author error with a pinning test; say if you want the presenter to
   skip clashing ids instead (a separate plan).

## The first test I would write

In `presenter/cell.rs` tests (next to `each_figure_takes_its_own_subcaption`): a cell with label `fig-two`, `fig-cap`
"A group.", `fig-subcap` `["First.", "Second."]`, no `figure_count`, and two PNG display outputs. Assert the wrapper head
starts `#fig-two .cell`, the first image carries `#fig-two-1`, the second `#fig-two-2`, and the cell closes with
"A group.". It fails today for that reason (no wrapper label, no panel ids). **Negative controls in the same test
module:** (a) the same cell with `fig-cap` and no `fig-subcap` must keep today\'s output; (b) the same cell with one PNG
and one subcap must render a single figure with no `fig-two-1` (the one-plot rule; this catches the prototype\'s bug);
(c) a single-figure labeled cell with `fig-cap` only is byte-identical (the D14 guard a generic `figure_count` would
break).

## What I did not check

- Neither sub-agent could finish a build, so the 810-test filter, clippy, the three-fixture parity harness, the Windows
  cross-clippy and the WASM build were not run by anyone in this review. (The earlier session\'s 810 stays (relied).)
- Julia on non-HTML targets and Julia group results by sub-agent 2 (its hardcoded-PNG fixtures produced no `<img>`, the
  known `presentation-attrs` side finding from the 7d review, item 11); sub-agent 1\'s Julia renders did work and I
  re-ran one. Q1 on Julia and on tables. A plot-plus-included-graphic group and a plot-plus-widget group.
- Any Jupyter byte-identity comparison (no one rendered the Jupyter fixtures with two binaries): that is the plan\'s
  guard, not evidence in this review.
- Whether a knitr fallback chunk with a label, `fig-cap` and `fig-subcap` reaches the cell step (plan item 6).
- The preview renderer on a nested figure, hub-client e2e, and 7i\'s branch.
- Landing: I ran the protocol\'s rebase and fast-forward for these note edits; I did not push.

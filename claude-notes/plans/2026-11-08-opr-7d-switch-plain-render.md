---
title: 'OPR-7d: switch plain render of Julia and knitr to the recorded route'
date: 2026-11-08
description: 'Turns the internal present_from_record control on by default, so a plain q2 render of a Julia or knitr document is presented by the Rust presenter from an in-memory record, writing nothing to .quarto. Keeps the LaTeX family and uncovered targets on the engine route, quietly. Applies Gordon''s decisions on the retained knitr fallback and on an opt-out control, and adds the one whole-document allowlist form (Julia interleaving) the gate needs.'
status: blocked-on-gordon
depends-on: [opr-7a, opr-7b, opr-7c, opr-7f, opr-7g, opr-7h, opr-7i, opr-7j]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/7d-switch-plain-render` · **Lane D** (`workspace-6`) · **Blocks:** OPR-8.
**Needs:** R 4.3 with knitr and Julia 1.12; `hub-client` tooling for the WASM gate and the preview chain.

## Start condition

`status` is `blocked-on-gordon` until Gordon has signed the allowlist (OPR-7c) and flipped this plan to `ready`; the
decisions below are recorded (2026-11-09). If the allowlist is unsigned or the status is not `ready`, stop and report.

**Allowlist state (2026-11-09).** Gordon signed the three non-Julia entries in
`crates/quarto-core/tests/fixtures/opr-parity/opr-parity-allowlist.toml` (`knitr-empty-cell-wrapper`,
`knitr-adjacent-blockquotes`, `julia-code-leading-newline`; each reason ends "Signed: Gordon, 2026-11-09"). The five
Julia markup entries are not signed: [OPR-7f](2026-11-09-opr-7f-presentation-attrs-and-julia-names.md) removes them.
Anything OPR-7h proposes is `proposed` until Gordon signs it. The flip to `ready` still waits on 7f, 7g and 7h, and on [OPR-7i](2026-11-10-opr-7i-jupyter-page-dependencies.md) (added
2026-11-10): the recorded route writes no requirejs or jQuery for a Julia or Jupyter page that needs them, which the
plain route does, so switching first would break those pages.

The flip also waits on [OPR-7j](2026-11-10-opr-7j-group-caption-and-panel-ids.md) (added 2026-11-10, from the 7d plan
review, [report](../research/2026-11-10-opr-7d-plan-review.md)): without it the recorded route drops the group caption
and the panel ids of a Julia `fig-subcap` group, which the plain route keeps. Start 7d only when 7a, 7b, 7c, 7f, 7g, 7h,
7i and 7j are all `complete`.

## Decisions by Gordon (2026-11-09)

**1. The retained knitr fallback (knitr epic decision 24) for plain render: keep, as today.** It stays for chunks Rust
cannot map and for fallback targets; OPR-7d changes nothing about it, and removing it remains the separate,
trigger-gated KNR-8. Reason: Gordon chose the recommended option; the corpus does not cover every use of knitr, so the
fallback is a safety net the switch should not remove.

**2. An opt-out control: none.** The `present_from_record` flag is flipped on and stays internal; there is no environment
variable, document key or other user-facing way back. Reason: Gordon chose the recommended option; the parity gate (zero
unallowlisted differences, OPR-7c) is the safety, and a control would be a user surface to document and later retire. A bad
case is fixed in q2, not worked around. OPR-7a\'s "behind a control" means this internal flag only, which agrees.

**3. Julia subfigure groups on the flip: the 2026-11-09 acceptance is replaced by OPR-7j (Gordon, 2026-11-10).** Gordon
accepted on 2026-11-09 that plain Julia resolves a `fig-subcap` group\'s references with wrong numbers while the recorded
route leaves them unresolved, on the reading that subfloat support was deferred by design. The 7d review then rendered
both routes (and the pre-epic commit `7016d595a`, which matches today\'s plain route) and found the flip is worse than
that reading: the group caption text is lost, the panel ids are gone, and the two panel references become `?fig-x-1?`
with two new warnings. Gordon\'s rule is that the epics must not make behavior worse, so the acceptance is withdrawn
for figures. [OPR-7j](2026-11-10-opr-7j-group-caption-and-panel-ids.md) applies knitr\'s group rules (caption, label on the
cell, `label-N` panel ids) to every engine; after it the Julia group renders as it does today, with today\'s (wrong)
panel numbers, and nothing about figures changes on the flip. Tables are unchanged (a 2026-11-09 probe: both routes
leave the panel references unresolved and drop the parent caption; the 7j session re-tested Julia, knitr and Jupyter on both
routes and the pre-epic commit, and all give the same page, so there is nothing for the flip to worsen and 7j leaves tables alone). Real
subfloats (numbering such as "Figure 2 (a)") stay out of scope: `bd-plcqhfcn` (the umbrella), `bd-gbaykhth` (the
Jupyter family) and `bd-au5gts6e` (knitr numbering), with the
[subfloat draft](2026-11-09-subfloat-parity-draft.md). Nothing in this plan fixes them.

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

## The whole-document form: design (settled by the 7d review; the implementer confirms it in tests first)

**Problem.** `RULES` in `tests/integration/opr_parity_harness.rs` are regex substitutions applied to both sides, so a
rule hides a line-level difference. The 4d interleaving is the same content arranged differently; a regex wide enough
to cover it would also hide real bugs in the same document. Evidence for what follows: the 7d review, items 11 to 20,
rendered by a sub-agent with prototype controls (the prototype stays outside the tree).

**Normalizer `julia-interleaved-cell` (a second kind of rule, a function from a side\'s HTML to a canonical HTML).**
Applied to both sides, scoped by an **exact document name** (not a prefix: a prefix would also cover a future
`julia/fig-subcap-tables`), named in `RULES`, and HTML-aware: use `scraper` (already a normal dependency of `quarto-core`;
no Cargo change). The form:

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

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-6 && pwd
git switch -c opr/7d-switch-plain-render feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7d-switch-plain-render
```

## Goal (decision D5)

A plain `q2 render` of a Julia or knitr document builds the record in memory and presents it through the Rust
presenter, as a recorded render does. Concretely:

- The internal `present_from_record` flag from OPR-7a is on by default. There is no opt-out (decision 2).
- **Nothing is written under `.quarto`** on a plain render except what is written today (`render-manifest.json`);
  `execute: record: true` still writes the record file (execution-records decision 3).
- The LaTeX family (`latex`, `beamer`, `pdf`), `pptx`, `context` and unknown targets stay on the engine route for
  knitr, **quietly**. This already holds (OPR-7a): `plan_capture` (`engine/knitr/target_route.rs`) gives the info
  diagnostic only when `want_record` is true (`want_record.then(...)`), so a fallback caused by `present_from_record`
  alone is quiet, and a unit test pins it. `want_record` is also true under `--trace` and for the preview and hub
  recorders, so a knitr render to a LaTeX target under `--trace` prints the line; Gordon accepted that (2026-11-10).
  The line reads, for `latex`, `beamer` and `pdf`: "knitr output for LaTeX is not presented from the execution record /
  the output format is `pdf`. / LaTeX targets keep knitr\'s finished markdown for every chunk."; for other uncovered
  targets: "knitr output for this target is not presented from the execution record / ... Targets outside HTML,
  revealjs, epub, Typst, docx and gfm keep knitr\'s finished markdown for every chunk." No code change.
- **Julia on non-HTML targets: decided (Gordon, 2026-11-10): present by evidence, gate the rest.** Julia has no target
  gate today: `offers_presentation` (`ts_record.rs`) checks only that a record is built and units exist, so after the
  flip core would present a Julia render to every target, while the gate compares only html, gfm, revealjs and typst
  (`COMPARABLE`, `opr_parity_runner.rs`; it skips zip formats, and every Julia manifest entry is `html`). One hand run of
  a Julia document (a `println`, a labeled figure, a labeled table, a bare expression; plain against recorded, 7d review
  session, ran) found: `docx` and `epub` differ only in where the `println` output sits relative to the figure (4d
  interleaving); `pptx` slide text is identical; `gfm` differs in a way the interleaving does not explain (the recorded
  route writes figures to `doc_files/figure-html/` where plain writes `figure-gfm/`; I did not check whether that is
  Julia-specific or presenter-wide); `typst` rendered on both routes and was not diffed; `latex` and `beamer` give
  "Unknown format" in this tree (LaTeX lives on `explore/pandoc-hybrid-latex`) and there is no TeX, so they are not
  measurable. The rule: a target class goes through the presenter for Julia only when a parity case for it differs from
  the plain route by the accepted interleaving alone. A class with no case, or a failing case, stays on the engine route
  for Julia, quietly, through a one-line target list in `offers_presentation` (the knitr gate\'s counterpart; do not
  reuse knitr\'s own list, which excludes pptx). `pdf`, `latex` and `beamer` stay gated until the LaTeX work exists. A
  class graduates by adding its case and removing it from the list.
- **Native Jupyter takes no flag.** `text_execute.rs` gates only on `ctx.want_record` and already presents through
  `present_document` on every render, so the flip changes nothing for it (a Jupyter `fig-subcap` group does change, but through 7j, which lands first: epic D14); the only difference from the plain route
  that [OPR-7i](2026-11-10-opr-7i-jupyter-page-dependencies.md) leaves (decision 6) is that native Jupyter and the ipynb
  replay path get the widget warning but not the page-dependency scan. 7d carries that as a known, documented
  difference, and OPR-8\'s notes say so.
- Cached knitr chunks fall back with no record (OPR-7b). If [OPR-7e](2026-11-09-opr-7e-cache-hits-as-records.md) is
  `complete`, a `cache=TRUE` chunk is a record cell on a hit as well as on a miss, and levels 1 and 2 still fall back;
  read its notes. If 7e is blocked or not done, proceed as in 7b: this plan does not wait for it (7e decision 5).
- Marimo is a documented exception and stays on its legacy path (decision D12).

## Checklist (tests first; every box is `[unattended]`)

The order matters: the gate runs **before** the flip. After the flip both sides of the harness present from the record
(it builds its "recorded" side by adding `execute: record: true` to a default render), so a post-flip run compares the
recorded route with itself and proves nothing.

- [ ] **Tests first for the normalizer and the pinned entry** (design above): the first test builds a plain page with
      one cell (one code block, two stdout outputs `a` and `b`) and a recorded page with the same cell split into two
      code blocks each followed by its output; assert `Allowlisted` with the rule named and fire counts 0 (plain) and 2
      (recorded). Its negative control, in the same test: the recorded page with the two outputs merged into one block
      `a\nb` must be a `Blocker`. Then one test per trap and per control listed above, including swapped outputs
      (a `Blocker` whose report says same lines, different order) and the serialization control. Rewrite
      `the_seeded_allowlist_parses_and_every_entry_is_proposed` for the new entries (run it first: 7e\'s notes say it
      failed with 8 entries versus 6).
- [ ] Entries `proposed` for `julia/presentation-attrs`, `julia/records-plot`, `julia/fig-subcap` (normalizer) and
      `julia/capture-facts` (pinned diff); Gordon signs; update the allowlist header sentence about group references.
- [ ] **Gate before the flip.** Run the OPR-0b/7c harness on the commit that has the normalizer, with 7i and 7j
      landed and no flip, and attach the report: zero unallowlisted differences, the per-entry fire counts, and the
      caption and reference lines of the three `fig-subcap` fixtures before and after.
- [ ] **Tests first, through the binary** (model `crates/quarto/tests/integration/no_execute_cli.rs`, which has
      `q2_render` and `Q2_BIN` but no Julia or knitr helper; `r_available()` and `julia_available()` live in
      `quarto-core` tests, so the `quarto` crate needs its own copy or a shared one): a plain render of a Julia and a
      knitr document is presented from a record (the output carries the recorded-route markup, for example the `.cell`
      wrapper with the id OPR-7f made the presenter write; do not key a test on a missing `id="cell-N"`); a Julia cell
      with a bare PNG gets the file names OPR-7c decided on; a marimo document\'s HTML is identical to before
      (`marimo_engine_e2e::` still passes); nothing new under `.quarto`, tested as: `.quarto/execution-records` does not
      exist after a plain render, and the sorted `.quarto` listing equals that of a `--no-execute` render of the same
      document (do not pin the whole listing); a knitr document to `pdf` or `latex` takes the engine route and prints
      nothing about it; a cached chunk falls back; `execute: record: true` still writes the file; native Jupyter\'s HTML
      is identical before and after; there is no opt-out to test (decision 2). The Julia and knitr binary tests skip
      cleanly when the engine is absent and **are local evidence only**: CI installs a Jupyter venv but neither R nor
      Julia (a grep of `.github/workflows` finds none), so say that, and name the unit tests CI does run.
- [ ] **Julia on non-HTML targets (decided above).** Tests first: (1) add one Julia parity case per class to
      `manifest.toml` (gfm, typst, docx, pptx, epub; the document of the review\'s hand run is a good seed) and teach the
      harness to compare zip formats by member (extract `word/document.xml`, the pptx slide XML, the epub xhtml; strip
      relationship ids; the review did this by hand); (2) a unit test that Julia\'s `offers_presentation` returns false for
      a gated target and true for the graduated ones, with the list as data; (3) the gfm figure directory: find out
      whether `figure-html` for gfm is a presenter-wide naming bug (check knitr and Jupyter) or Julia-specific, and fix it
      in the presenter or gate gfm; (4) run the cases, graduate each class whose difference is the interleaving alone, and
      leave the others in the list with a line in the notes saying why. Include the gate report\'s per-class table. A
      knitr target keeps its own list (decision D5). Add a binary test per graduated class that skips when Julia is absent.
- [ ] **Flip the default (no opt-out, decision 2) in `EngineExecutionStage::new()`.** `new()` is called at
      `pipeline.rs` in three places (the shared head, `insert_capture_splice_stage_in`, which rebuilds the stage with
      `new()` and would lose a builder-set flag, and `parse_qmd_to_ast`), so flipping its initial value reaches all three;
      list the tests that used `new()` as "plain" and say what each now means. Nothing in production calls
      `with_present_from_record` today.
- [ ] **Keep the harness as a regression test of the recorded route (Gordon, 2026-11-10).** After the flip it compares
      the recorded route with itself plus the file it writes; say so in the allowlist header and the runner doc, and keep
      the pre-flip report as the gate\'s evidence. A test-only seam on the pipeline builder that re-enables the old
      route is not an opt-out, but it is a new surface; add one only if Gordon asks.
- [ ] **End-to-end through the real binary,** plain `q2 render` on a Jupyter, a Julia and a knitr document and one
      LaTeX-target knitr document; read the HTML; record the invocations and what you saw. (Native Jupyter is unchanged;
      the check is that it is.)
- [ ] **Preview chain** (AGENTS.md, "Verifying Rust changes in `q2 preview`"): `cd hub-client && npm run build:wasm`,
      `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`; confirm `q2 preview` of the same documents still
      renders. This is a compile and smoke check, not a behavior check: the preview recorder already wants a record, so
      the flip is a no-op there, and `ts_engine`, `knitr` and `jupyter` are `cfg(not(wasm32))`. Say what was and was not
      seen by a person.
- [ ] Gates: `cargo clippy -p quarto -p quarto-core --all-targets -- -D warnings`; `cargo nextest run` for the same
      crates; the Windows cross-clippy; `cargo xtask lint`.

## Exit criteria

- A plain render of Julia and knitr is presented from an in-memory record; the pre-flip gate report shows zero
  unallowlisted differences; nothing new is written under `.quarto`.
- LaTeX-family and uncovered knitr targets stay on the engine route with no message (other than under `--trace`);
  marimo and native Jupyter unchanged.
- Gordon\'s decisions are implemented as recorded (fallback kept; no opt-out control; interleaving allowlisted by the
  normalizer and the pinned entry; subfigure groups unchanged by the flip, through 7j).

## Notes from the run

(Fill in at landing.)

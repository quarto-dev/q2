---
title: 'OPR-9f: run the plain-render gate (entries proposed, report attached)'
date: 2026-11-10
description: 'Writes the four interleaving entries as proposed, runs the whole parity harness before the flip, and attaches the report: zero unallowlisted differences, per-entry fire counts, and the caption and reference lines of the three fig-subcap fixtures. Ends with Gordon signing the entries; no flip happens here.'
status: ready
depends-on: [opr-7a, opr-7b, opr-7c, opr-7f, opr-7g, opr-7h, opr-7i, opr-7j, opr-9b, opr-9c, opr-9d, opr-9e]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9f-run-the-gate` · **Lane D** (`workspace-6`) · **Blocks:** 9g.
**Needs:** R 4.3 with knitr, Julia 1.12 with Plots, Python and uv, Node; about 5 minutes for the whole gate.

> **Reading text copied from OPR-7d.** Passages copied from the old 7d plan say "this plan" or "7d". Read them as this
> piece of the series 9a to 9g (see the [tombstone](2026-11-08-opr-7d-switch-plain-render.md)). Marks: (read), (ran), (relied).

## Start condition

Start when every plan in `depends-on` is `complete` on the integration branch (read the statuses from `feature/one-presenter`, not from your
checkout; 7j was `in-progress` on 2026-11-10). Check that `workspace-6` is clean and on no other branch. If R, Julia or Plots is absent,
stop: the gate cannot be measured without them and a skipped case is not a pass.

**Allowlist state (2026-11-09).** Gordon signed the three non-Julia entries in
`crates/quarto-core/tests/fixtures/opr-parity/opr-parity-allowlist.toml` (`knitr-empty-cell-wrapper`,
`knitr-adjacent-blockquotes`, `julia-code-leading-newline`; each reason ends "Signed: Gordon, 2026-11-09"). The five
Julia markup entries are not signed: [OPR-7f](2026-11-09-opr-7f-presentation-attrs-and-julia-names.md) removes them.
Anything OPR-7h proposes is `proposed` until Gordon signs it. The flip (9g) still waits on 7f, 7g and 7h (all complete), and on [OPR-7i](2026-11-10-opr-7i-jupyter-page-dependencies.md) (added
2026-11-10): the recorded route writes no requirejs or jQuery for a Julia or Jupyter page that needs them, which the
plain route does, so switching first would break those pages.

7j (group caption and panel ids of a `fig-subcap` group) must be `complete` too: without it the recorded route drops them and `julia/fig-subcap` cannot reach zero unallowlisted differences.

## Decisions it applies

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

## Checklist (every box is `[unattended]` except the last)

- [ ] Entries `proposed` for `julia/presentation-attrs`, `julia/records-plot`, `julia/fig-subcap` (normalizer) and
      `julia/capture-facts` (pinned diff); Gordon signs; update the allowlist header sentence about group references.
- [ ] **Gate before the flip.** Run the OPR-0b/7c harness on the commit that has the normalizer, with 7i and 7j
      landed and no flip (`OPR_PARITY_REPORT=<scratchpad>/report.md cargo nextest run -p quarto-core -E
      'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture`; it needs R, Julia and Plots; narrow
      with `OPR_PARITY_ONLY=julia/ OPR_PARITY_KNITR=0`, about 2 minutes; the test itself asserts nothing, so read the report
      counts), and attach the report: zero unallowlisted differences, the per-entry fire counts, and the
      caption and reference lines of the three `fig-subcap` fixtures before and after.
- [ ] **Record the result** in `claude-notes/research/2026-11-NN-opr-9f-gate-report.md`: the five counts, the per-entry fire counts (plain
      and recorded), the "not compared" list, the caption and reference lines of the three `fig-subcap` fixtures before and after, and
      every difference not covered by an entry. Zero unallowlisted differences is the exit; any other result is a stop (below).
- [ ] **Gates:** `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core` (the known
      `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package` failure is local `node_modules` drift, not yours).
- [ ] **[Gordon]** signs the four entries (`proposed` to `signed`, each reason ending "Signed: Gordon, 2026-11-NN") and sets OPR-9g to
      `ready`. The agent writes the rule and reason, never the signature, and does not flip a status.

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol's "When you cannot
finish") if any of these happens:

- the report shows an unallowlisted difference other than the four interleaving cases: re-run that case once, narrowed, before deciding
  (the Julia server has a known transient timeout); if it repeats, report it and do not add an entry;
- a case is skipped or "not compared" for a reason that is not an absent tool (a zip format with no extractor, a format missing from the
  manifest): that is a gap in 9d or 9e, not a pass;
- an entry is needed by no document in its scope (stale) or its pin changed;
- a dependency is not `complete`, or `workspace-6` is dirty.

## Handoff

End the session with the protocol's handoff ("Ending your session"). The handoff says the entries are `proposed` and that Gordon must sign them before 9g can start; write no plan handoff past that.

## Exit criteria

The report on the integration branch shows zero unallowlisted differences with every entry `proposed` and every fire count as designed
(plain 0, recorded greater than 0 for the interleaving normalizer); the three `fig-subcap` fixtures\' caption and reference lines are
attached; Gordon has the entries to sign.

## Notes from the run

(Fill in at landing.)

---
title: 'OPR-9e: Julia on non-HTML targets (fixture, parity classes, target allowlist)'
date: 2026-11-10
description: 'Measures Julia plain against recorded on gfm, typst, docx, pptx and epub, adds the three fig-subcap formats, and graduates each target class whose only difference is the accepted interleaving, through a target allowlist in offers_presentation. A class that fails stays on the engine route, quietly.'
status: ready
depends-on: [opr-9a, opr-9b, opr-9d]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9e-julia-non-html-targets` · **Lane D** (`workspace-6`) · **Blocks:** 9f, 9g.
**Needs:** Julia 1.12 with Plots, `pandoc`, `typst`. CI has no Julia, so the binary tests are local evidence only.

> **Reading text copied from OPR-7d.** Passages copied from the old 7d plan say "this plan" or "7d". Read them as this
> piece of the series 9a to 9g (see the [tombstone](2026-11-08-opr-7d-switch-plain-render.md)). Marks: (read), (ran), (relied).

## Start condition

Gordon read the [9a note](../research/2026-11-10-opr-9a-spike-results.md) and confirmed its two settled decisions for this plan (which formats graduate; the gate applies under `want_record`) on 2026-11-10, so the status is `ready`. Then start when 9a, 9b and 9d are `complete` (9d runs first in this same lane; 9b runs in lane B). Check that `workspace-6` is
clean and on no other branch (it was on `knr/knitr-pieces-hook-memo` with a dirty tree on 2026-11-10); if not, stop and report.

## The rule (Gordon, 2026-11-10)

- **Julia on non-HTML targets: decided (Gordon, 2026-11-10): present by evidence, gate the rest.** Julia has no target
  gate today: `offers_presentation` (`ts_record.rs`) checks only that a record is built and units exist, so after the
  flip core would present a Julia render to every target, while the gate compares only html, gfm, revealjs and typst
  (`COMPARABLE`, `opr_parity_runner.rs`; it skips zip formats, and every Julia manifest entry is `html`). 9a\'s S1 measured 22 formats
  on the unfixed presenter ([note](../research/2026-11-10-opr-9a-spike-results.md)): html, revealjs, dashboard and epub match (html by
  interleaving alone); docx and typst match except the figure directory name (9b fixes it); gfm, commonmark, markdown, djot, rst, org,
  native, textile and mediawiki inline a base64 image, and pptx, odt, rtf, icml, fb2, docbook and plain lose the figure (9b fixes
  both). `pdf` is "not yet supported" and `latex` and `beamer` give "Unknown format" in this tree (LaTeX lives on
  `explore/pandoc-hybrid-latex`), so none of the three is measurable. The rule: a target class goes through the presenter for Julia only
  when a parity case for it differs from the plain route by the accepted interleaving alone. A class with no case, or a failing case,
  stays on the engine route for Julia, quietly, through the allowlist below (the knitr gate\'s counterpart; do not reuse knitr\'s own
  list, which excludes pptx). **Settled by 9a\'s S2 (ran):** with 9b\'s fix, recorded matches plain on figures and directory for all 22 formats measured. Graduation follows the harness: **html, revealjs, dashboard, gfm and typst** have harness evidence (interleaving only; the three `fig-subcap` and `records-plot` cases); **epub, docx and pptx** join with 9d\'s zip extractor (the hand matrix already matches); the other text formats (commonmark, markdown, djot, rst, org, native, textile, mediawiki, plain, icml, fb2, docbook, rtf) have hand-matrix evidence only, so add them to the harness\'s comparable list and graduate each that shows interleaving alone; odt after the extractor handles it. Html, revealjs and dashboard must be on the list or they would stop presenting from the record. Everything else, and `pdf`, stays gated.

## What the review added

- **The list is an allowlist, not a deny list (read).** The flip-agent found `FormatIdentifier::as_str()` yields about 60 identifiers
  (`html`, `pdf`, `docx`, `pptx`, `epub`, `typst`, `revealjs`, `dashboard`, `gfm`, `commonmark`, `odt`, `rst`, `markdown`, ...), lowercase and
  canonical, with no `+extension`. "Remove it from the list to graduate" would present every unlisted identifier with no parity case. Write
  `JULIA_PRESENTED_TARGETS` (a data list in `engine/ts_record.rs`, read by `offers_presentation(ctx)` through `ctx.format`), and graduate a
  class by adding its identifier with its parity case. `latex` and `beamer` never reach the engine ("Unknown format", `format.rs`). `pdf`
  stays out until the LaTeX work exists. Do not reuse knitr\'s `route_for_target` list (it excludes pptx).
- **The gate and `want_record`.** `offers_presentation` is `builds_record() && !units.is_empty()` and `builds_record()` is
  `want_record || present_from_record`, so `record: true`, `--trace`, preview and hub make a Julia engine present from the record on a
  target the gate excludes: S1\'s "recorded" renders are exactly that. Gordon agreed (2026-11-10) to settle the rule after S2; S2 shows the recorded route is correct on every measured target, so the gate now protects only the unmeasured tail of formats and any target without a parity case. **Decided (9a note, 2026-11-10): the gate applies under `want_record` too**, so asking
  for a record never changes the output route (the record is still built and written); that means `offers_presentation` must test the
  allowlist, not only `builds_record()`. Tests: `record: true`, `--trace` and a plain render of a gated target give the same page.
- **The hand-run document is not in the tree (read).** It lived in an earlier session\'s scratch. 9a\'s S4 (done) put a corrected draft in the
  [results note](../research/2026-11-10-opr-9a-spike-results.md) (it needs only Julia\'s standard library, no Plots); commit it as `crates/quarto-core/tests/fixtures/opr-parity/julia/targets/doc.qmd` (a `println`, a labeled figure from a hardcoded PNG, a
  labeled table, a bare expression, and a `fig-subcap` group cell). The existing `julia/fig-subcap` table renders as literal pipe text on
  both routes, so it does not exercise a table.
- **gfm and pptx (ran):** the presenter bug behind them is fixed by 9b, so run them after 9b. docx and typst already match apart from the
  typst directory name, which 9b also fixes. A remaining difference in file names after 9b is a finding, not something to normalize in
  the harness.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Fixture and manifest.** Commit the fixture; add one Julia manifest entry per class (gfm, typst, docx, pptx, epub) with `name`, `dir`,
      `file`, `requires = ["julia", "plots"]` only if the document needs Plots, `formats`, and a non-empty `note`
      (`the_plans_own_manifest_parses_and_names_real_documents` asserts it). Use 9d\'s zip extractor for docx, pptx and epub.
- [ ] **Julia on non-HTML targets (decided above).** Tests first: (1) add one Julia parity case per class to
      `manifest.toml` (gfm, typst, docx, pptx, epub; the review\'s hand-run document plus a `fig-subcap` group cell, so that each class exercises 7j\'s markup; the three `fig-subcap` manifest entries list only `html`) and teach the
      harness to compare zip formats by member (extract `word/document.xml`, the pptx slide XML, the epub xhtml; strip
      relationship ids; the review did this by hand); (2) a unit test that Julia\'s `offers_presentation` returns false for
      a gated target and true for the graduated ones, with the list as data; (3) the figure directory and the display priority are fixed in the presenter by the box above (decision 6); after it, a remaining gfm, typst or revealjs difference in file names is a finding, not something to normalize in the harness; (4) run the cases, graduate each class whose difference is the interleaving alone, and
      leave the others in the list with a line in the notes saying why. (5) After (3), add `gfm`, `typst` and `revealjs`
      to the `formats` of the three `fig-subcap` manifest entries, which 7j leaves at `html` because their only
      blocker is the figure directory: the non-HTML parity check for 7j\'s group markup is this plan\'s, and a format
      missing from the manifest is "not compared", so the flip gate would pass without checking it. Include the gate report\'s per-class table. A
      knitr target keeps its own list (decision D5). Add a binary test per graduated class that skips when Julia is absent.
- [ ] **Target allowlist.** Tests first: `offers_presentation` returns false for a gated target and for an unknown identifier, true for each
      graduated one, with the list as data; the Julia target gate is quiet (nothing printed on a plain render) and a unit test pins that.
      Then implement `JULIA_PRESENTED_TARGETS`.
- [ ] **Run the cases** (`OPR_PARITY_ONLY=julia/ OPR_PARITY_KNITR=0 OPR_PARITY_REPORT=<scratchpad>/report.md cargo nextest run -p quarto-core
      -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture`; about 2 minutes plus the new classes), graduate the
      classes whose only difference is the interleaving, leave the others out of the list with a line in the notes saying why. Include the
      report\'s per-class table in the landing report. Gordon decides any class that fails (see Stop conditions).
- [ ] **Binary test per graduated class** (`crates/quarto/tests/integration`, a new file registered alphabetically in `main.rs`): a plain render of
      the fixture to that target on a Julia machine; skip cleanly when Julia is absent (copy `julia_available`, it lives in
      `quarto-core`\'s tests and the `quarto` crate cannot reach it). Say these are local evidence only.
- [ ] **Gates:** `cargo clippy -p quarto -p quarto-core --all-targets -- -D warnings`; `cargo nextest run` for the same crates; the Windows
      cross-clippy; `cargo xtask lint`.

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol's "When you cannot
finish") if any of these happens:

- a class fails parity by more than the interleaving, or its figure names differ in a way 9b did not fix: **stop and ask Gordon** whether to
  graduate it, gate it, or fix the cause first. Do not decide it yourself and do not normalize it away in the harness;
- Julia, Plots, `pandoc` or `typst` is missing: report which; do not skip the class silently;
- the fixture is unstable across two runs (not a transient): report it;
- `workspace-6` is dirty or on another branch.

## Handoff

End the session with the protocol's handoff ("Ending your session"). Name 9f as the lane's next piece when 9b, 9c, 9d and 9e are complete.

## Exit criteria

Each of gfm, typst, docx, pptx and epub is graduated (with its parity case and binary test) or listed as gated with a line saying why;
`offers_presentation` consults `JULIA_PRESENTED_TARGETS`; the three `fig-subcap` manifest entries list gfm, typst and revealjs and the gate
checks them; the per-class table is in the landing report.

## Notes from the run

(Fill in at landing.)

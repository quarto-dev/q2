---
title: 'OPR-0b: parity corpus and harness for the plain-render switch'
date: 2026-11-08
description: 'Builds the instrument that decides whether a plain render of Julia and knitr documents may move to the recorded route: knitr corpus additions, a live Julia corpus, a harness that renders each document both ways and classifies every difference, and a committed baseline report. The gate rule itself is applied in OPR-7c.'
status: in-progress
depends-on: []
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/0b-parity-corpus` · **Lane D** (`workspace-6`) · **Blocks:** OPR-7c.
**Needs:** R 4.3 with knitr; Julia 1.12 with the packages the Julia documents use (offline, see the knitr epic notes
and `claude-notes/plans/2026-11-02-records-in-use-epic.md` for the offline `Plots` recipe). Tests skip cleanly
when either tool is absent.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-6 && pwd
git switch -c opr/0b-parity-corpus feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/0b-parity-corpus
```

## Goal

Decision D10: the plain-render switch uses knitr\'s flip gate (epic decision 26: only Identical and SyntaxLevel
differences pass; anything else needs a signed-off allowlist entry) on a defined set of documents, and requires zero
unallowlisted differences. This plan builds the documents, the harness and the baseline. It does not decide
anything and does not touch product code beyond test support.

## What exists (read)

- Knitr: the corpus under `crates/quarto-core/tests/fixtures/knitr-corpus/` (a generated manifest with 104
  entries, handwritten sets `s1` to `s9`, a frozen `live/` oracle, `allowlist.toml` with 8 entries), the classifier
  `tests/integration/knitr_corpus_classify.rs` (identical, syntax-level, semantic, blocker), the gate
  `knitr_corpus_gate.rs`, the runner `knitr_corpus_runner.rs` (returns early unless `KNR_REPORT` is set and R is
  present), and `knitr_replay_equals_live.rs` (plain against recorded, small documents). KNR-6 recorded "280
  accepted, 8 allowlisted, 4 not compared".
- Harness caution (OPR-0c): when normalizing, treat `data:image` payloads and absolute paths in `data-quarto` as
  noise, and compare the **set of figure file names**, not only the HTML, or the file-name difference is invisible
  (Plots figures are inline base64 and hide it).
- Julia: `tests/integration/julia_presenter_parity.rs` over `tests/fixtures/julia-presenter-parity/` (12 stored
  notebooks with TypeScript goldens; structural, no live Julia).
- (relied) Only one knitr fixture sets a document `execute:`; none has a multi-figure subcaption case with crossrefs;
  no cached-chunk case exists.

## Checklist (tests first; every box is `[unattended]`)

- [ ] Read the existing knitr corpus support and decide how the harness reuses the classifier (do not fork it).
- [ ] **Tests for the harness first.** The harness takes a document and renders it twice through the real binary
      path (`render_document_to_file`, not the HTML-only helper): once plain, once with `execute: {record: true}`
      added; strips image data and generated ids; classifies the difference. Write tests with two tiny fixtures:
      one identical pair and one with a known single difference.
- [ ] **Knitr additions** (fixtures, in the existing manifest or `handwritten`): document-scope `execute:` keys
      (`echo`, `output`, `include`, `warning`, `message`); a labeled multi-plot cell with `fig-subcap` and
      `layout-ncol: 2` with crossrefs to the figure and each sub-figure (the 2026-11-08 finding: plain and recorded
      routes are byte-identical here and both number the sub-figures as separate figures, tracked as `bd-au5gts6e`);
      a `cache: true` chunk (mark it `pending OPR-7b`: its expected result changes there); the 12 documents under
      `crates/quarto/tests/smoke-all/typst/margin-layout/` that use R.
- [ ] **Cases the 0c follow-up measured (add as fixtures; 2026-11-09, [note](../research/2026-11-09-opr-measurements.md)):**
  (1) Julia, two cells that `display(MIME("image/png"), plot(...))`, the second with `#| label: fig-b` and `#| fig-cap`,
  after a prose block (shows the file-name and `cell-output` class differences); (2) Julia and a stored `.ipynb` with a
  `fig-subcap` group and `layout-ncol: 2` plus references to the group and each member (the unresolved-group case);
  (3) a knitr document with two live and two `cache: true` chunks, figures with captions in each, and references (mark
  `pending OPR-7b`; harness runs it twice so a miss and a hit are both covered); (4) a minimal marimo document with the
  extension from `tests/fixtures/extensions/marimo/` and `{python .marimo}` cells (needs `uv`; skip with a message
  without it). Julia plot fixtures need `Project.toml` and `Manifest.toml` copied beside the document (`Plots` is not
  in the runner\'s default environment).
- [ ] **Julia live corpus.** Documents: `crates/quarto-core/tests/fixtures/extensions/julia-website/plot.qmd`,
      `records-export/julia-plot.qmd`, `records-export/julia-error.qmd`, `positron-cache/julia.qmd`,
      `crates/quarto-preview/tests/fixtures/records-in-use/julia.qmd`, the Julia chunk of `docs/index.qmd` (as a
      fixture copy, not the live page), and the Julia engine\'s own documents under
      `resources/extension-subtrees/julia-engine/tests/docs/julia-engine/` (`error`, `keep-ipynb`, `source-ranges`,
      `engine-reordering`, `sleep`). Copy fixtures into a corpus directory of this plan\'s own; do not reference
      `external-sources/`. Skip a document that needs a package that is not installed, and say which.
- [ ] **The runner.** Gated by `OPR_PARITY_REPORT=1`, like `KNR_REPORT`; not part of default nextest runs; not wired
      into CI (R and Julia are absent there). Output: one report listing every document, its class and its diff.
- [ ] **The allowlist file**, `opr-parity-allowlist.toml`, in the format of the knitr `allowlist.toml`. Seed it
      with **proposed** entries only for differences you can name: Julia\'s `id="cell-N"`, `data-quarto` and `data-execution_count`
      attributes on the cell wrapper and output div (ran, OPR-0c), the dropped `cell-output` class on figure divs, and
      Julia\'s figure names (`cell-N-output-M.png`, different counters on the two routes; ran, OPR-0c). Do **not** seed an
      entry for the subfigure-group references: that is a blocker, not an allowlist candidate. Mark each entry `proposed`: Gordon signs them off in OPR-7c.
- [ ] **Baseline report.** Run both corpora on a machine with R and Julia; commit the report under
      `claude-notes/research/` with counts per class and the list of unallowlisted differences. Say which documents
      were skipped and why.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`, `cargo nextest run -p quarto-core` (the
      gated runner skips), the Windows cross-clippy for `quarto-core`, and the report render check
      (`target/debug/q2 render --strict <file>`).

## Exit criteria

- `OPR_PARITY_REPORT=1 cargo nextest run -p quarto-core -E 'test(opr_parity)'` (use the test name you chose; record
  the exact command here) runs both corpora end to end on a machine with R and Julia.
- The baseline report is committed with counts, the skipped list and the unallowlisted differences.
- The harness has its own passing tests; no default nextest run depends on R or Julia.

## Notes from the run

(Fill in at landing, including the exact run command.)

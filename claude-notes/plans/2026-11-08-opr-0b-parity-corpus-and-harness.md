---
title: 'OPR-0b: parity corpus and harness for the plain-render switch'
date: 2026-11-08
description: 'Builds the instrument that decides whether a plain render of Julia and knitr documents may move to the recorded route: knitr corpus additions, a live Julia corpus, a harness that renders each document both ways and classifies every difference, and a committed baseline report. The gate rule itself is applied in OPR-7c.'
status: complete
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

- [x] Read the existing knitr corpus support and decide how the harness reuses the classifier (do not fork it).
- [x] **Tests for the harness first.** The harness takes a document and renders it twice through the real binary
      path (`render_document_to_file`, not the HTML-only helper): once plain, once with `execute: {record: true}`
      added; strips image data and generated ids; classifies the difference. Write tests with two tiny fixtures:
      one identical pair and one with a known single difference.
- [x] **Knitr additions** (fixtures, in the existing manifest or `handwritten`): document-scope `execute:` keys
      (`echo`, `output`, `include`, `warning`, `message`); a labeled multi-plot cell with `fig-subcap` and
      `layout-ncol: 2` with crossrefs to the figure and each sub-figure (the 2026-11-08 finding: plain and recorded
      routes are byte-identical here and both number the sub-figures as separate figures, tracked as `bd-au5gts6e`);
      a `cache: true` chunk (mark it `pending OPR-7b`: its expected result changes there); the 12 documents under
      `crates/quarto/tests/smoke-all/typst/margin-layout/` that use R.
- [x] **Cases the 0c follow-up measured (add as fixtures; 2026-11-09, [note](../research/2026-11-09-opr-measurements.md)):**
  (1) Julia, two cells that `display(MIME("image/png"), plot(...))`, the second with `#| label: fig-b` and `#| fig-cap`,
  after a prose block (shows the file-name and `cell-output` class differences); (2) Julia and a stored `.ipynb` with a
  `fig-subcap` group and `layout-ncol: 2` plus references to the group and each member (the unresolved-group case);
  (3) a knitr document with two live and two `cache: true` chunks, figures with captions in each, and references (mark
  `pending OPR-7b`; harness runs it twice so a miss and a hit are both covered); (4) a minimal marimo document with the
  extension from `tests/fixtures/extensions/marimo/` and `{python .marimo}` cells (needs `uv`; skip with a message
  without it). Julia plot fixtures need `Project.toml` and `Manifest.toml` copied beside the document (`Plots` is not
  in the runner\'s default environment).
- [x] **Julia live corpus.** Documents: `crates/quarto-core/tests/fixtures/extensions/julia-website/plot.qmd`,
      `records-export/julia-plot.qmd`, `records-export/julia-error.qmd`, `positron-cache/julia.qmd`,
      `crates/quarto-preview/tests/fixtures/records-in-use/julia.qmd`, the Julia chunk of `docs/index.qmd` (as a
      fixture copy, not the live page), and the Julia engine\'s own documents under
      `resources/extension-subtrees/julia-engine/tests/docs/julia-engine/` (`error`, `keep-ipynb`, `source-ranges`,
      `engine-reordering`, `sleep`). Copy fixtures into a corpus directory of this plan\'s own; do not reference
      `external-sources/`. Skip a document that needs a package that is not installed, and say which.
- [x] **The runner.** Gated by `OPR_PARITY_REPORT=1`, like `KNR_REPORT`; not part of default nextest runs; not wired
      into CI (R and Julia are absent there). Output: one report listing every document, its class and its diff.
- [x] **The allowlist file**, `opr-parity-allowlist.toml`, in the format of the knitr `allowlist.toml`. Seed it
      with **proposed** entries only for differences you can name: Julia\'s `id="cell-N"`, `data-quarto` and `data-execution_count`
      attributes on the cell wrapper and output div (ran, OPR-0c), the dropped `cell-output` class on figure divs, and
      Julia\'s figure names (`cell-N-output-M.png`, different counters on the two routes; ran, OPR-0c). Do **not** seed an
      entry for the subfigure-group references: that is a blocker, not an allowlist candidate. Mark each entry `proposed`: Gordon signs them off in OPR-7c.
- [x] **Baseline report.** Run both corpora on a machine with R and Julia; commit the report under
      `claude-notes/research/` with counts per class and the list of unallowlisted differences. Say which documents
      were skipped and why.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`, `cargo nextest run -p quarto-core` (the
      gated runner skips), the report render check (`target/debug/q2 render --strict <file>`). **Not done: the Windows
      cross-clippy** (see "Notes from the run").

## Exit criteria

- `OPR_PARITY_REPORT=1 cargo nextest run -p quarto-core -E 'test(opr_parity)'` (use the test name you chose; record
  the exact command here) runs both corpora end to end on a machine with R and Julia.
- The baseline report is committed with counts, the skipped list and the unallowlisted differences.
- The harness has its own passing tests; no default nextest run depends on R or Julia.

## Notes from the run

**Exact run command** (about 7.5 minutes on this machine; needs R with knitr, Julia with Plots, Node, `uv`):

```bash
OPR_PARITY_REPORT=report.md cargo nextest run -p quarto-core \
  -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture
```

`OPR_PARITY_REPORT=1` prints the report to stderr instead. `OPR_PARITY_ONLY=<substring>` narrows the documents and
`OPR_PARITY_KNITR=0` leaves out the existing knitr corpus. A default `cargo nextest run -p quarto-core` skips the runner
(it returns at once without the variable).

**Where things are.** Harness and its 13 tests: `crates/quarto-core/tests/integration/opr_parity_harness.rs`. Runner and
its 5 tests: `opr_parity_runner.rs`. Corpus, manifest and the proposed allowlist:
`crates/quarto-core/tests/fixtures/opr-parity/` (`manifest.toml`, `opr-parity-allowlist.toml`). Baseline:
[the baseline note](../research/2026-11-09-opr-parity-baseline.md).

**Results** (details in the note): 56 identical, 12 allowlisted by proposed entries, 7 unallowlisted differences, 253
cases rejected by q2 before any engine runs, 2 execution failures (one expected), nothing skipped.

**Decisions I made.**

- The plan\'s knitr additions live in this plan\'s own manifest, not in `knitr-corpus/manifest.toml`: that manifest feeds
  the KNR gates (counts and a frozen oracle), and adding documents there would have changed them. The existing corpus is
  still run, through the same harness.
- The classifier is reused as vocabulary only (`Class`, `r_available`, `parallel`, `CorpusManifest`). The knitr
  classifier compares markdown at three levels, and two rendered pages have no level between identical and different, so
  I did not fork it. Differences are explained by named normalization **rules**, each an allowlist entry, applied to both
  sides, so an entry hides the difference it names and nothing else.
- `execute: {record: true}` is injected through the scratch directory\'s `_quarto.yml`, not front matter, so a stored
  `.ipynb` (no front matter) goes through the same path. `project.render` names the one document, because a default
  project excludes `.ipynb` from its render list.
- Allowlist: the five entries the plan asked for, plus `knitr-empty-cell-wrapper` (R\'s empty `.cell` div for a chunk that
  renders nothing). That divergence is already signed off for the knitr baseline presenter (decision 23); it is
  `proposed` here so Gordon carries it over in OPR-7c. The `julia-cell-output-class` rule is limited to figure divs. No
  entry for the subfigure-group references.

**Surprises and findings for Gordon / OPR-7c.**

- Seven differences nothing explains, found by this harness: the Julia subfigure group (known blocker, and the plain route
  names those files by label, so the proposed file-name rule does not cover them); Plots\' three requirejs/jquery
  `script` elements present only on the plain route (three documents); a leading newline in a Julia code block; **ANSI
  escape sequences in Julia output on the recorded route** (`@test` output, visibly wrong); `width`/`height` against
  `class="img-fluid"` on a bare PNG; and the empty blockquote shell of `chunk_in_blockquote`.
- 253 of the existing knitr corpus\'s cases cannot be exercised through `q2 render` at all (old-style chunk headers,
  Q-2-36, and parse errors), so that corpus\'s parity is measured at the engine level by KNR-6, not here.
- Generated ids (flextable, gt) changed on every render and had to be normalized as noise.
- The first Julia render after a server start once printed a stray stderr line on one side; the runner re-renders a
  difference once to tell that from a route difference.
- Process: the report-classification predicate (`is_rejection`) was changed before its test was written; the test was
  added right after and passes. Everything else was test-first.

**Gates.**

- `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean.
- `cargo nextest run -p quarto-core`: see the landing report for the counts after the rebase.
- Windows cross-clippy: **not done.** `cargo clippy -p quarto-core --all-targets --target x86_64-pc-windows-msvc` fails in
  dependency build scripts (`aws-lc-sys`, `wasmtime`) that compile C for the Windows target from macOS, the caveat in
  `.claude/rules/cross-platform.md`. The new code uses `Path`/`PathBuf`, `to_forward_slashes` for figure names, and no
  `std::os` APIs, but a Windows user should run the native check.
- `target/debug/q2 render --strict` on the baseline note: clean.
- WASM: no `presenter/`, `record/`, splice or replay file was touched, so no `build:wasm`.
- Snapshots: none added, changed or removed.

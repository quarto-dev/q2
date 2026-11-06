---
title: 'Run R via the Ark Jupyter kernel (bd-yt2o2ovt)'
date: 2026-11-04
description: 'Epic A, reconciled with the execution-records epic: what the epic already does for Jupyter, what this stack adds (lenient base64, a located image-failure warning, a corpus, docs), and the end-to-end record on real Ark.'
status: implemented  # 2026-11-06, four commits on braid/bd-yt2o2ovt-ark-r-kernel-on-epic, unpushed; open: Windows cross-check, owner-approved push and PR
braid:
  strand: bd-yt2o2ovt  # "Epic: run R via the Ark Jupyter kernel in q2"
  priority: P3
---

# Overview

Goal (from the epic): a `.qmd` with `{r}` cells renders in q2 using Ark
(posit-dev/ark) as the Jupyter kernel, with figures, and with clear
diagnostics when something goes wrong.

Done when a document with `jupyter: {kernel: ark, claims: {r: 1}}` and no
`engine:` key renders its `{r}` cells on Ark, plot included. The resolver
redesign that would make the `claims` line unnecessary is Epic B,
`2026-11-04-engine-interface-review.md`, and is not decided here.

**Restack (2026-11-05, decided 2026-11-06).** This epic was first built on
`main` as five commits (PR 1076). The execution-records epic
(`feature/execution-records-epic`, draft PR 1007) rewrote the Jupyter engine
around the same problems, so the work was re-planned against it. The findings,
the probe table (23 probes on a build of the epic, real Ark 0.1.252) and the
decision per behavior are in `2026-11-05-ark-on-execution-records-stack.md`.
In short:

- **Dropped.** Kernel resolution (the old Phase 3 and decision D1) and its
  integration test: the epic selects a kernel by name from `jupyter:` and the
  engine config, and by language (including Ark), and rejects an unknown name
  with the list of kernels. Only the precedence order differs (the epic reads
  `jupyter:` first), which is a style call; the epic wins.
- **Kept, re-ported.** Lenient base64 (the user-visible gap: on the epic a plot
  on Ark was silently dropped), the image-failure warning (with `Q-18-11`,
  because the epic took `Q-18-4`), the escaped placeholder, the corpus, and the
  docs.

Branch: local `braid/bd-yt2o2ovt-ark-r-kernel-on-epic` in
`.worktrees/workspace-7`, off `origin/feature/execution-records-epic`
(`baf2dcb9b` when cut), to be pushed as `feature/ark-r-kernel`. The old branch
`braid/bd-yt2o2ovt-ark-r-kernel` and PR 1076 are untouched.

# What landed

1. **Lenient base64** (`presenter/figures.rs`, `decode_image`): all ASCII
   whitespace is stripped and padding is optional. One decoder serves the
   filesystem sink, the WASM sink and `ipynb_replay`. Tests: unit tests, a
   fake-kernel end-to-end test that runs in CI, and a real-Ark integration test
   (`jupyter_ark_figures`) that skips without Ark.
2. **`Q-18-11` warning.** The figure sinks record failures; `present_document`
   drains them into `PresentNote::ImageUnusable { unit, mime, reason }`; the
   consumers map notes to warnings located at the cell (`text_execute`,
   `ts_record`, `ipynb_replay`, `replay`). The placeholder is escaped, so it is
   literal text and no longer raises `Q-2-49`. Catalog entry, docs page and
   sidebar entry are in the same commit.
3. **The corpus** (49 cases: 35 pass, 14 expected-fail pending Epic B's Phase
   B3) and the Epic B plan, with a note on what the epic changes.
4. **Docs:** "Running R cells on Ark" in `jupyter-kernels.qmd`, linked from
   `engines.qmd`.

# End-to-end record (2026-11-06)

Binary `target/debug/q2` built in `.worktrees/workspace-7` at the stack's
commit 3. Ark 0.1.252 (kernelspec `ark`); `jupyter` from a scratch venv on
`PATH`. Output files were opened and read; they were not inferred from the
absence of errors.

- `jupyter: {kernel: ark, claims: {r: 1}}`, `{r}` cell `plot(1:3)`,
  `q2 render a.qmd`: `a_files/figure-html/cell-1-output-1.png` is "PNG image
  data, 800 x 600"; `a.html` has
  `<img src="a_files/figure-html/cell-1-output-1.png" ...>`. No knitr
  `processing file` line (the cell ran on Ark).
- The same with `engine: {jupyter: {kernel: ark, claims: {r: 1}}}` and with
  `engine: [knitr, {jupyter: {kernel: ark, claims: {r: 2}}}]`: the same figure
  name and `<img>`. These are the two other spellings the docs show.
- `jupyter: ark` alone: knitr runs (`processing file: d.rmarkdown`, figure
  `unnamed-chunk-1-1.png`). This is why the docs keep the `claims` line.
- A Python cell that displays `{'image/png': '!!!not base64!!!'}`, default
  render: `Warning [Q-18-11]: Image Output Could Not Be Used`, located at
  `e.qmd:7:2`, `1 warning`, the page shows the literal `[Image output]`, and
  there is no `Q-2-49`.
- The same with `--strict`: `Error [Q-18-11] ...`, `1 error`, exit code 1.
  (The HTML file is still written, as for other strict failures.)

# Checklist

- [x] Owner decisions (2026-11-06): stack on the epic; code `Q-18-11`; drop
  the kernel commit; branch `feature/ark-r-kernel`.
- [x] Lenient base64, with a test that failed first on the epic, including the
  real-Ark test.
- [x] Image-failure warning `Q-18-11`, escaped placeholder, catalog entry,
  docs page, sidebar entry; epic tests and 3 presenter goldens updated.
- [x] Corpus and Epic B plan: 49 cases, 35 pass, 14 expected-fail.
- [x] Docs section in the kernel guide; `q2 render --strict` of the touched
  plan files.
- [x] End-to-end on the real binary with real Ark.
- [ ] Windows cross-check of `quarto-core`: could not run here (`aws-lc-sys`
  needs a C toolchain for the Windows target). The change uses no
  platform-specific API; a Windows user can run
  `cargo clippy -p quarto-core --all-targets -- -D warnings`.
- [ ] Owner: `cargo xtask verify --skip-hub-build` plus the WASM leg
  (`quarto-core` and the shared presenter are in the WASM build), then push.
- [ ] Owner: push, open the PR against the epic branch, close PR 1076.

# Known environment issue

`dashboard_goldens::dashboard_skeletons_match_goldens` fails on a machine with
R `bslib` older than 0.12 (here 0.9.0). It is independent of this work.

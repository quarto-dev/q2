---
title: 'Ark on the execution-records epic: reconcile and restack (bd-yt2o2ovt)'
date: 2026-11-05
description: 'What the execution-records epic already does for Jupyter versus the Ark branch (PR 1076), verified by probes on the epic build, and the ordered stack that keeps only what the epic lacks.'
status: planned  # nothing implemented; decided 2026-11-06: stack on the epic, Q-18-11, branch feature/ark-r-kernel; Epic B untouched
braid:
  strand: bd-yt2o2ovt  # stays open; bd-rorntup4 stays open (Epic B)
---

# Overview

Two lines of work overlap. The Ark branch
(`braid/bd-yt2o2ovt-ark-r-kernel`, PR 1076, five commits on `main`) makes `{r}`
cells render on Ark with plots. The epic
(`origin/feature/execution-records-epic`, tip `b1854e441`, draft PR 1007, 0
commits behind `main`) rewrote the Jupyter engine around kernel selection from
`jupyter:` and a format-agnostic presenter. A `rebase --onto` conflicted
heavily in `text_execute.rs` because the epic moved the code the Ark commits
edit.

Method: two independent reviewers. A read the epic by `git show` (no runs). B
built the epic in a scratch worktree (`.worktrees/probe-epic`, detached at
`b1854e441`) and ran 23 probes with real Ark 0.1.252 and a scratch Python
venv, each against both the epic build and the Ark branch build. I
spot-checked the claims that decide the plan directly against the epic ref.

**Verdict.** The epic already does the kernel work, so the kernel-resolution
commit is dropped. The epic does **none** of the image-output work, and that
is the failure a user sees: on the epic a plot on Ark is silently dropped.
So the new stack is small: lenient base64, an image-failure diagnostic (with a
new code, because the epic took Q-18-4), the corpus, and a short docs
section. The done-when does **not** change, and Epic B is not affected.

# Findings (probes)

"Epic" is the epic build, "Mine" is the Ark branch at `5cbb12d57`. Scratch
documents and outputs are under the session scratchpad (`probes/`,
`run-epic/`, `run-mine/`).

| Probe | Epic | Mine |
|---|---|---|
| P1 unpadded `image/png`, Python kernel | **Dropped.** `WARN ... invalid base64 ... Invalid padding` in the log only; no file, no `<img>` | file written, `<img>` |
| P1b same with `\r\n` and a space in the payload | **Dropped** (`Invalid symbol 13`) | written |
| P5c plot on real Ark, `jupyter: {kernel: ark, claims: {r: 1}}` | Ark runs, **figure dropped** (`Invalid padding`), spurious Q-2-49 | png written, `<img>` |
| P2 corrupt image | Q-2-49 present, context-free (no cell location); placeholder renders as `Image output` (brackets lost to a span) | `Q-18-4` located at the cell, no Q-2-49, literal `[Image output]` |
| P3 `jupyter: nosuchkernel`, `{python}` | exit 1, kernelspec not found, lists searched dirs and available kernels | same |
| P4a/b `jupyter: ark`, `jupyter: {kernel: ark}`, `{r}` | knitr | knitr |
| P4c `jupyter: {kernel: ark, claims: {r: 1}}`, `{r}` | Jupyter on Ark | same |
| P4d/e/f/g `engine: jupyter` alone, plus `jupyter: ark`, plus `{kernel: ark}`; `[knitr, jupyter]` | knitr in all | knitr in all |
| P8c `jupyter: {claims: {r: 1}}`, no kernel | Ark chosen by language, **plus** the epic\'s `Q-18-4` "Jupyter Kernel Not Named" warning | Ark, silent |
| P6 `jupyter: python3`, `{python}`; and no key | runs, prints 42 | same |
| P9 `jupyter: ark` with a `{python}` cell | Ark runs the Python code (`[1] 42`), no warning | same |
| P7 49-case corpus on the epic | compiles unchanged; **35 pass, 14 expected-fail** | same (it is the baseline) |

Reviewer A\'s reading agrees with every probe: the epic\'s `kernel_select.rs`
(`kernel_for_language`, case-insensitive, names Ark\'s `"R"` kernelspec) picks
Ark by language; `claims_language` still returns `Fallback(0)` for every
Jupyter language, so knitr keeps `{r}` unless Jupyter claims it. Image
decoding is `presenter/figures.rs:63`: strict `STANDARD` base64 after
stripping only `\n`, failure logged by `tracing::warn!` and returned as
`None`. One decoder, `decode_image`, serves both `FsFigureSink` and
`RuntimeFigureSink` (the WASM sink), and the presenter is also used by
`ipynb_replay`.

Differences worth knowing:

- **Kernel precedence is reversed.** The epic\'s `explicit_kernel`
  (`kernel_select.rs:140`) reads the `jupyter:` key first, then
  `engine_config.kernel`; the Ark branch did the reverse. The epic documents
  its order in `docs/guides/authoring/jupyter-kernels.qmd`. Nobody sets both
  to different values, and this is a style call, so the epic wins.
- **`Q-18-4` is taken on the epic** (catalog line 1948, plus a docs page) with
  a different meaning, "Jupyter Kernel Not Named". The epic also has `Q-18-5`
  and `Q-18-10`, and its comments say `Q-18-5` to `Q-18-9` are "claimed
  elsewhere". The next free code is `Q-18-11`.
- **Kernel/cell-language mismatch is silent on both** (P9). That is Epic B\'s
  R1; not decided here.
- An `{r}` cell `1+1` followed by `print(...)` in one Ark cell shows only the
  last value on both builds. Not a regression and not investigated; Jupyter
  kernels report the last expression of an execute request.
- Reviewer A said the epic\'s merge-base with `main` is `52063c43c`. That is
  stale: `git merge-base` now returns `95930bf97`, the current `main` tip.
  The epic is up to date with `main`, and the epic does not touch
  `tests/integration/main.rs`, so only the `rebase --onto` conflict in
  `text_execute.rs` (and the add/add on Q-18-4 files) matters.

# Decision table

Evidence column cites the probe rows above.

| Behavior | Decision | Evidence |
|---|---|---|
| Phase 1: unpadded base64 and all-ASCII-whitespace stripping | **Re-port** into `presenter::figures::decode_image` (one fix covers the filesystem and WASM sinks and `ipynb_replay`) | P1, P1b, P5c: plots lost on the epic; this is the user-visible gap |
| Phase 2a: escape the placeholder as `\[Image output\]` | **Re-port** in `presenter/cell.rs`; update the two epic tests that pin the unescaped text (`cell.rs:870`, `:885`) | P2: Q-2-49 present, brackets lost |
| Phase 2b: image-failure warning diagnostic located at the cell | **Re-port with a new code, `Q-18-11`** (not Q-18-4), through the presenter\'s sink, not `format_outputs` in `text_execute.rs`, which no longer decodes anything | P2: only a log line today |
| Phase 3: kernel resolution (D1) | **Drop** | Present on the epic: explicit name from `jupyter:` string and map forms and engine config, language match including Ark (P8c, `kernel_select.rs`), unknown name is a hard error that lists kernels (P3). Only difference is precedence order (style) and the epic\'s name-table removal (fallback is the Python kernelspec) |
| `jupyter_kernel_selection.rs` integration test | **Drop** | Passes on the epic (P3), so it cannot fail first; the epic covers it at `text_execute.rs:863` |
| Docs: "Running R cells on a Jupyter kernel" | **Re-port**, smaller, into the epic\'s `jupyter-kernels.qmd` (the kernel guide), with a one-line link from `engines.qmd` if the section stays there. Remove two claims that no longer hold: the `ir`-before-`ark` tie-break sentence (unverified on the epic\'s selector) and the "kernel must name installed kernel" bullet that the epic page now covers | The epic page already documents kernel order, "every cell Jupyter owns runs on it", and Q-18-4 |
| The `claims: {r: 1}` line in the docs | **Keep, unchanged** | P4a to P4g: still required on the epic; only `claims` moves `{r}` to Jupyter |
| 49-case resolution corpus (`cases.yaml`, harness, `render_table.rb`, research note) | **Keep**, as-is | P7: compiles with no edits and gives 35 pass, 14 expected-fail; the epic did not change resolution, so the 14 still fail with their pinned behavior |
| Epic B plan (`2026-11-04-engine-interface-review.md`) | **Keep**, add a short note (below) | Not decided here |
| Epic A plan (`2026-11-04-ark-r-kernel-epic.md`) | **Rewrite** to the reconciled scope, then commit it with the last code commit | It describes a Phase 3 and a Q-18-4 that no longer exist in this stack |

# Stack

New branch off the epic tip, with the friendly name **`feature/ark-r-kernel`**
(decided 2026-11-06; the old remote branch `feature/bd-yt2o2ovt-ark-r-kernel`
keeps its name). Locally the repo convention is a `braid/` branch pushed to a
`feature/` remote name, so: local `braid/bd-yt2o2ovt-ark-r-kernel-on-epic`,
pushed with `git push -u origin HEAD:feature/ark-r-kernel` (every later push
repeats the refspec, then check `git ls-remote`). Create it with
`cargo xtask switch-task bd-yt2o2ovt --from feature/execution-records-epic`
(or `git switch -c` from `origin/feature/execution-records-epic`). No history
rewrite, no force-push: the old branch and PR 1076 stay as they are until the
new PR exists. Snapshot refs already exist (`history/braid/bd-yt2o2ovt-ark-r-kernel`).

Each commit is one logical change, TDD, and gated on
`cargo clippy -p quarto-core --all-targets -- -D warnings` plus
`cargo nextest run -p quarto-core`.

1. **Accept unpadded base64 and any ASCII whitespace in Jupyter images.**
   `decode_image` only. Tests below (T1). Code and tests only.
2. **Warn when an image output cannot be used (Q-18-11).** Sink failure
   channel, escaped placeholder, catalog entry, `docs/errors/engine/Q-18-11.qmd`
   and the sidebar entry in this one commit (lint rules require it). Update the
   two epic tests that pin the unescaped placeholder.
3. **Add the engine-resolution corpus.** Harness, `cases.yaml`, research note,
   `main.rs` registration, plus the Epic B plan with its note. Test-only,
   passes on the epic as 35 plus 14 expected-fail.
4. **Document running R on Ark.** The docs section in the kernel guide, the
   reconciled Epic A plan, and the end-to-end record. Docs and plan only.

Order rationale: 1 and 2 are the behavior; 3 is test infrastructure that does
not depend on them; 4 documents the finished behavior. 1 and 2 touch the
presenter, which is active epic code, so keep them small.

# Tests (TDD)

Each test is written first and run on the epic tip to see the stated failure.

- **T1, commit 1 (fails first on the epic).**
  - `decode_image` given a valid PNG payload with the trailing `=` removed
    returns the bytes. Failure today: `None` (log line `Invalid padding`).
  - Same payload with `\r\n`, spaces and tabs inserted decodes. Failure today:
    `None` (`Invalid symbol 13`).
  - An end-to-end test through the epic\'s `fake_kernel.rs` (a kernel replying
    with an unpadded `image/png` display) rendering through the real entry that
    `q2 render` uses (`render_to_file`-style, as the epic\'s other integration
    tests do): asserts `<stem>_files/figure-html/cell-1-output-1.png` exists.
    Failure today: no file. This runs in CI, which has no Ark.
  - A skip-when-absent Ark test (`ark` kernelspec not registered, or no R):
    renders a `{r}` `plot(...)` with `jupyter: {kernel: ark, claims: {r: 1}}`
    and asserts the figure file. Failure today on a machine with Ark: no file.
    Skips in CI.
- **T2, commit 2 (fails first on the epic).**
  - A cell whose image cannot be decoded renders no Q-2-49 diagnostic. Failure
    today: one `[Image output] has no attribute block` warning.
  - The same render yields one `Q-18-11` warning that names the cell location
    and the decode error. Failure today: none, only a log line.
  - A figure write failure (unwritable directory) yields the same diagnostic.
  - The placeholder text is the literal `[Image output]` in the HTML. Failure
    today: `Image output` in a span.
  - `--strict` turns the warning into the one error.
  - `ipynb_replay` shares the presenter; add one case there so the new
    failure channel is not silently dropped by that consumer.
- **T3, commit 3.** The corpus harness is the test; it must report 49 cases,
  35 pass, 14 expected-fail, with no unexpected flip.
- **Commit 4.** `q2 render --strict` of the touched docs page, and the end-to-end
  record below.

# Design question to settle inside commit 2

`FigureSink::write_image` returns `Option<String>`, and `format_outputs`
(`presenter/cell.rs:173`) has no diagnostics output. Preferred shape: the sink
accumulates failures (cell index, mime, reason) and the trait gets a
`take_failures` method; `present_document` callers (`text_execute.rs`,
`ipynb_replay.rs`) turn them into `ExecuteResult.warnings` using
`describe_location` for the cell, which both already import. The alternative,
changing `write_image` to return a `Result`, touches more tests. Read
`present_document`\'s return type first and pick the smaller diff. If neither
fits without reshaping the presenter, stop and ask the epic owner.

# The new done-when

Unchanged: a `.qmd` with `{r}` cells renders on Ark, plot included, using
`jupyter: {kernel: ark, claims: {r: 1}}` and no `engine:` key. `jupyter: ark`
alone does **not** work on the epic (P4a/b: knitr runs). So:

- The docs keep the `claims` line and the "a later release may make it
  unnecessary" sentence.
- For Epic B: the epic does not change what D2b needs. All 14 pending cases
  still fail, `claims_language` is still `Fallback(0)`, and C10, C30 and C31
  still run knitr. This is a report, not a decision; D2b and R1 to R7 stay with
  the owner. Two observations for Epic B that the epic adds: the epic\'s new
  `Q-18-4` "kernel not named" warning and `Q-18-5` "no runnable kernel" now
  exist and R1\'s kernel-versus-ownership check should be designed alongside
  them, and the epic\'s `kernel_select` already carries a notion of "unrunnable
  owned language" that overlaps R1.

# PR logistics (nothing pushed; owner approval needed)

- **Base of the new PR:** `feature/execution-records-epic`, stacked on draft
  PR 1007, so the diff is the four commits only. When the epic merges, GitHub
  retargets the stack to `main`; if you prefer, retarget by hand.
- **PR 1076:** close it with a comment pointing at the new PR, after the new
  PR exists. Do not force-push over `feature/bd-yt2o2ovt-ark-r-kernel`; leave
  it as the record of the original work.
- **Merge order:** epic (1007) first, then this stack. The kernel commit is
  gone, so there is no semantic dependency on the epic beyond the presenter
  files, which only exist there.
- **Alternative if the epic stalls:** nothing here can land on `main` alone
  without redoing the work against `text_execute.rs`, which the epic deletes.
  Not recommended.
- **Q-18-11 is decided** (2026-11-06): no one else needs notifying. Commit 2
  touches `presenter/figures.rs`, `presenter/cell.rs` and the `FigureSink`
  trait; say so in the PR description for the epic reviewers.

# CI

Do not add Ark to CI in this stack. Commit 1\'s fake-kernel test runs in the
epic\'s existing CI (it installs uv, ipykernel and a `python3` kernelspec), so
the base64 fix is covered without Ark. The Ark test skips without an `ark`
kernelspec. If you want a real-Ark check, add it as a separate, owner-approved
change: a `workflow_dispatch`-only job that installs R and Ark and runs only
the Ark test. The epic workflow already has `workflow_dispatch`. I have not
drafted it.

# Checklist

- [x] Owner decisions (2026-11-06): stack on the epic; code `Q-18-11`; drop the
  kernel commit; branch named `feature/ark-r-kernel`.
- [ ] Create local `braid/bd-yt2o2ovt-ark-r-kernel-on-epic` off
  `origin/feature/execution-records-epic` (remote name `feature/ark-r-kernel`).
- [ ] Commit 1: write T1, see them fail on the epic, fix `decode_image`, gate.
- [ ] Commit 2: write T2, see them fail, resolve the sink design question, add
  the catalog entry, docs page and sidebar entry, fix, gate.
- [ ] Commit 3: corpus, Epic B plan note, `main.rs` registration; confirm
  35 plus 14.
- [ ] Commit 4: docs section in the kernel guide, reconciled Epic A plan,
  end-to-end record with the real binary and real Ark (plot included).
- [ ] Render the touched plan files with `q2 render --strict <files>`.
- [ ] Recommend `cargo xtask verify --skip-hub-build` and the WASM leg
  (quarto-core and the shared presenter are in the WASM build); owner decides.
- [ ] Reconcile this checklist against what landed before handing off.
- [ ] Remove the scratch worktree `.worktrees/probe-epic` (17 GB target) once
  the owner agrees.

# Notes on the probe environment

- Disk: free space went from 76 to 41 GiB for the probe build. Nothing was
  deleted.
- Ark was confirmed by behavior (no knitr `processing file` line, Jupyter-style
  figure names, and Python code printing `[1] 42`), not by a direct
  `kernel_info` request.
- Nothing was run on Windows or WASM.

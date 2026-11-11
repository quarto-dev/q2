---
title: 'Jupyter cell whose source contains three backticks mid-line is silently not executed (bd-f63cqoke)'
date: 2026-11-11
description: 'The qmd writer lengthens a cell fence to four backticks when the cell source contains a run of three backticks anywhere, and the Jupyter executor only runs three-backtick cells, so the cell silently renders as inert code; Quarto 1 runs any fence of three or more backticks.'
status: in-progress  # Phases 0-1 implemented on braid/bd-f63cqoke-jupyter-cell-whose-source; PR pending
braid:
  strand: bd-f63cqoke
---

**Branch:** `braid/bd-f63cqoke-jupyter-cell-whose-source` (off `main` @ `2511c70a8`). Go-ahead given 2026-11-11.

## Triage verdict

**Ready to design.** The bug reproduces on q2 0.42.0, the cause is confirmed and confined to
one Jupyter-only predicate, and Quarto 1's behavior (run any fence of three or more backticks)
gives a clear target. The open questions are about scope: whether to relax the executor rule,
tighten the writer, or both, and whether to add a warning.

## Issue context

Filed 2026-11-11 by Carlos while fixing GH #1156 (`output: asis` under Jupyter). Type bug, P2.

````python
```{python}
print("a ``` b")
```
````

renders as an inert code block with class `{python}` and no kernel output. One or two
backticks in the string work. Three or more anywhere in a line break it. Real-world impact:
an `output: asis` cell that prints fenced code (a tabset of install commands, which was the #1156
reporter's use case) never runs. No warning is emitted.

## Dependency graph

- **discovered-from**: bd-2evi7xlp (closed; GH #1156, fixed in PR #1174 / `d9febe2c3`). The
  `asis` fix made printing markdown from Python work, which made "print a fenced block" an
  obvious next use, and that exposed this bug. The repro sits on `main` at
  `claude-notes/issue-reports/1156/repro-triple-backtick-in-cell.qmd`.
- **related**: bd-jupyter-parse-code-blocks-regex-z1ehfla6 (open, child of the self-documenting
  cells epic bd-98m98wg8). It describes the old nesting-blind regex in `text_execute.rs`. **That
  description is now stale.** EXR-2 replaced the regex with the unit scanner (`parse_code_blocks`
  is `#[cfg(test)]` only now), and the executor consumes scanner units filtered by
  `runs_in_legacy_executor`. Its P3 case (indented cell never runs) is now the same predicate's
  `indent == 0` clause. This strand is the predicate's `fence_len == 3` clause.
- **Implicit siblings** (no edge yet). The EXR epic's decision 40 says `runs_in_legacy_executor`
  retires only when the strands deciding the executor's skipped cells are each decided:
  bd-9j6z311f (list-nested cells; q1 runs them), bd-rh4cce9e (`{python .cls}` attribute tail;
  q1 runs it) and bd-7gvl455a (`{Python}` capitalization). EXR-2 decision 12 lists "fence
  length of the opener" among the strand-owned skips, but **no strand owned it**. This strand
  is the natural owner.

## What the code looks like today

The chain the strand describes still holds at `2511c70a8`:

1. The AST is serialized back to qmd before execution. `pampa/src/writers/qmd.rs:1022-1024`
   picks the fence with `determine_backticks` (`:1830`), which counts the longest backtick run
   **anywhere** in the text, plus one, minimum 3. A string like `a`, three backticks, `b` therefore gets a four-backtick fence.
   That is valid CommonMark, but more conservative than needed: only a line made of backticks
   (and up to 3 spaces of indent) can close a fence.
2. `crates/quarto-core/src/engine/units.rs:240` `runs_in_legacy_executor` requires
   `fence_len == 3` (plus `indent == 0`, no container prefix, no attribute tail). Callers:
   `jupyter/text_execute.rs:362` (`owned_cells`) and `ipynb_replay.rs:131,176`.
3. The cell is dropped and nothing is reported.

Quarto 1's `startCodeCellRegEx` is an opener pattern that takes a run of three or more backticks (jupyter.ts:318-321), so q1 runs
fences of any length ≥3. knitr's chunk regex also accepts three or more.

### Reproduced (q2 0.42.0 from PATH, Jupyter venv; repros in `jupyter-backtick-fence-cells-investigation/`)

| File | Result |
|---|---|
| `mid-line-triple.qmd` | **Bug.** The cell with three backticks in its string is an inert `<pre class="{python}">`. The control cell runs. No warning. |
| `author-four-tick.qmd` | Runs. **This corrects the strand's guess:** the writer normalizes an author-written four-backtick fence back to 3 ticks when the source has no backticks. |
| `asis-tabset.qmd` | Not executed (same mechanism). This is the #1156 use case. |
| `knitr-mid-line.qmd` | Runs (`{r}` with three backticks in a string). **knitr is unaffected.** |

The only trigger is a backtick run of 3+ in the cell source. Where the run sits in the line does
not matter. An author-written 4-tick fence fails only when its source also contains backticks.

**Second likely trigger (unverified):** the `.ipynb` converter's `code_fence_ticks`
(`content_processors/ipynb.rs:618`) lengthens a code cell fence when a source *line starts*
with three backticks. A notebook cell like that would then be skipped by `ipynb_replay` (`:176`) and by
execution. Phase 0 should pin this.

## Proposed phases (draft)

- **Phase 0 — Failing tests.** Add a units-level test that a `fence_len == 4` bare `{python}`
  cell runs (this flips the existing `assert!(!runs_in_legacy_executor(&four[0]))` at
  `units.rs:739`). Add a Jupyter end-to-end test for the mid-line three-backtick cell and the asis tabset
  case. Add an ipynb test for a code cell with a line starting with three backticks.
- **Phase 1 — Executor rule.** Change `runs_in_legacy_executor` to `fence_len >= 3` (q1 parity).
  Update the EXR-2 plan (decision 12) and the epic (decision 40) to record that the
  opener-length skip is decided.
- ~~**Phase 2 — Writer.**~~ Dropped; see bd-4wvmsld9. Make `determine_backticks` count only runs that
  could close a fence (line-start runs), which matches the ipynb converter's Q1 rule.
- ~~**Phase 3 — Diagnostic.**~~ Dropped (decision 3). Warn when a braced `{lang}` cell owned by the
  Jupyter engine is skipped by the legacy rule.
- **Phase 4 — Changelog entry.**

## Decisions (Carlos, 2026-11-11)

1. **Narrow fix only.** Phase 1 (`fence_len >= 3` in `runs_in_legacy_executor`) is the fix.
2. **The writer change is out of scope.** It is filed as bd-4wvmsld9 (`discovered-from` this strand).
   Phase 2 is dropped from this plan.
3. **No new warning.** Skipped-cell behavior for the remaining legacy skips stays as it is.
   Phase 3 is dropped.
4. **Strand cleanup is done.** Comment `c-jjwerv7f` on bd-jupyter-parse-code-blocks-regex-z1ehfla6
   points at `runs_in_legacy_executor`. This strand is linked `related` to bd-9j6z311f,
   bd-rh4cce9e and bd-7gvl455a. **Follow-up when the PR lands:** add a second comment on
   bd-jupyter-parse-code-blocks-regex-z1ehfla6 saying the `fence_len` clause changed (PR number,
   merge commit).

Pre-flight note: `cargo xtask verify --skip-hub-build` at `2511c70a8` failed one unrelated knitr
test (`capture_classifies_pieces`, `rlang_error` vs `simpleError`; this looks like the local R
setup). It is filed as bd-yibbojew. Fail-fast cancelled the rest of the run, so re-run verify
before starting Phase 1.

## Open design questions for the user (answered above)

1. **Fix site.** Relaxing `runs_in_legacy_executor` to `fence_len >= 3` alone fixes every
   trigger (writer bump, ipynb conversion, author-written fences) and matches q1. Is that the
   fix, or do you also want the writer change?
2. **Writer tightening (Phase 2): in or out?** It would make re-serialized qmd closer to what
   authors wrote, but `pampa::writers::qmd` is shared by every qmd round-trip (hub, `-t qmd`,
   snapshots). That means snapshot churn and a separate risk surface for a fix Phase 1 already
   delivers. My lean is to file it as its own strand.
3. **Silent skip.** Should a skipped braced cell produce a warning now, for the remaining
   legacy skips (indented, list-nested, attribute tail)? Or is that left to bd-9j6z311f and
   bd-rh4cce9e, which may make those cells run anyway?
4. **Strand hygiene.** bd-jupyter-parse-code-blocks-regex-z1ehfla6's description refers to a
   regex that no longer exists in the executor. Should I add a comment on it that points to
   `runs_in_legacy_executor`, and link this strand plus the three EXR-2 divergence strands as
   `related`?

## Risks / tradeoffs (draft)

- Relaxing the predicate changes which cells the self-documenting cells epic (bd-98m98wg8)
  treats as executable. A 4-tick bare `{python}` fence would now run. q1 also runs it, and the
  display convention is a nested or doubled-brace cell, which the scanner classifies as
  opaque, so I don't expect a conflict. Check with Gordon, since that epic's plan 1
  (engine hand-off masking) is his.
- `ipynb_replay` uses the same predicate, so stored outputs for 4-tick cells will start to be
  replayed. That is the desired outcome, but replay tests may need new expectations.
- The fix makes q2 run cells it previously skipped. Any document that relied on this,
  knowingly or not, will start executing code. Mention it in the changelog.

## Implementation log

- **Phase 0 (tests, red first).**
  - `units.rs`: a fence of four or five backticks runs, including one whose source holds three backticks.
  - `text_execute.rs`: `bd_f63cqoke_cell_with_a_backtick_run_survives_the_qmd_round_trip`
    (parse → qmd writer → plan) and `bd_f63cqoke_longer_opener_runs`. The `cells_nested`
    corpus fixture is now a named exception to the old-finder comparison.
  - `ipynb_stored_replay.rs`: `cell_whose_source_holds_a_backtick_run_replays_its_output`
    covers both triggers. **It confirmed the ipynb trigger:** before the fix, both cells
    replayed as inert `{python}` blocks.
  - smoke-all `jupyter/backtick-run-in-cell.qmd` uses a live kernel (`requires: jupyter`)
    and covers the mid-line, `output: asis` tabset and control cells. Checked red with
    `== 3` put back, then green.
- **Phase 1.** `runs_in_legacy_executor` now uses `fence_len >= 3`. The EXR-2 decision 12 and
  epic decision 40 notes record that the opener-length skip is decided.
- **Phase 4.** No changelog: this repo only keeps `hub-client/changelog.md`, and hub-client
  is not touched.
- **Still to do after merge:** a follow-up comment on bd-jupyter-parse-code-blocks-regex-z1ehfla6
  (decision 4).

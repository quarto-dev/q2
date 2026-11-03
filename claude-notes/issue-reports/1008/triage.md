# Issue #1008 — `q2 render --no-execute` is rejected although the help text mentions it

- **GitHub**: https://github.com/quarto-dev/q2/issues/1008
- **Reporter**: @mcanouil (Mickaël Canouil), 2026-11-02
- **Triage date**: 2026-11-03
- **Worktree**: `.worktrees/issue-1008` (branch `issue-1008`, based on `main` @ `c00fd454c`)
- **Braid strand**: bd-2luq2ath
- **Scope**: the `--execute` / `--no-execute` pair on `q2 render`. The other
  `q2 render` flags that are declared but never read (see "Incidental
  finding") are recorded here but are out of scope for the fix.

## Summary

`q2 render --help` documents `--execute` as "Execute code (--no-execute to
skip execution)", but clap declares only `--execute` as a plain `bool`, so
`--no-execute` is rejected (exit 2). `--execute` is also a no-op: the `Render`
match arm ignores the field and hardcodes `ExecutionPolicy::All`. Reproduced on
`main`. **This is a real bug, and the fix is small.** The engine-level knob
already exists: `ExecutionPolicy::None` (bd-sl79jjiq) skips engine resolution
entirely and passes code cells through inert. `q2 preview --static` and
`preview.engine: off` already use it, and it is plumbed through
`RenderArgs.execution_policy` into every render path, books included.

## Reproduction

Fixture: `claude-notes/issue-reports/1008/repro.qmd` (one `{r}` cell, `1 + 1`).

```console
$ q2 render repro.qmd --no-execute
error: unexpected argument '--no-execute' found

  tip: a similar argument exists: '--execute'
  ...
exit=2
```

Expected (Quarto 1 parity): the document renders, the R cell does not run,
and `repro.html` contains no `[1] 2`.

The fixture has to be rendered from outside this repo. Under
`claude-notes/` it is excluded by that directory's `_quarto.yml` project.

## Localization

- `crates/quarto/src/main.rs:81-83` declares the flag as `#[arg(long)] execute: bool`,
  with no negation.
- `crates/quarto/src/main.rs:1697-1735` is the `Render` arm. It destructures
  with `..`, which drops `execute`, and passes
  `execution_policy: ExecutionPolicy::All`. The comment there ("`q2 render`
  always executes") is now wrong.
- `crates/quarto-core/src/engine/execution_policy.rs` defines `ExecutionPolicy { All, None, Only }`.
- `crates/quarto-core/src/stage/stages/engine_execution.rs:376-392` is where the
  policy is consulted. A skipped document never loads an engine, so it works
  without R or Python installed, which is the reporter's use case. It never
  warns, and it sets `execution_skipped`. Stored `.ipynb` replay is unaffected
  (it is checked before the policy), which matches Quarto 1, where
  `--no-execute` on a notebook still shows its stored outputs.
- Model to copy: `crates/quarto/src/commands/preview_static.rs:233-240`
  (`policy_for`, `preview.engine: off` → `ExecutionPolicy::None`).

## Open questions resolved during triage

1. **Does the execution machinery need new work?** No.
   `RenderArgs.execution_policy` (`commands/render.rs:113`) already flows to
   `render_to_file`, the pipeline, and the book paths (`grep -rn execution_policy crates/`).
   The fix is CLI-only.
2. **Is there a workaround today?** Partly. `-M 'execute:{enabled: false}'`
   suppresses the output (`[1] 2` count goes from 1 to 0), but knitr still
   launches, so it does not help on a machine without R. The colon form
   `-M execute:enabled:false` does **not** work, because dotted keys are not
   expanded (documented in `-M`'s help).
3. **Is there an existing strand?** No. `braid search no-execute` returned
   nothing. bd-nn2fou8h (execute-visibility options) is adjacent but separate.

## Outcome / recommended next step

Filed bd-2luq2ath. Fix scope:

- Make `--execute` / `--no-execute` a clap negatable pair, for example two
  flags with `overrides_with`, so the last one given wins. Map them to an
  `Option<bool>`.
- `--no-execute` → `ExecutionPolicy::None`.
- No flag → `ExecutionPolicy::All` (current behavior).
- `--execute`: in Quarto 1 it forces execution (it overrides
  `execute: enabled: false`). Either implement that or, at minimum, keep it as
  `All` and make the help text say what it actually does. Decide this at fix
  time.
- Update the stale comment in the `Render` arm.
- Tests: a clap-parse unit test (`--no-execute` parses and maps to `None`),
  plus an integration test showing that a `{r}` / `{python}` document renders
  under `--no-execute` with no engine output. Check that the inert cell still
  renders as an echoed code block, like Quarto 1's `--no-execute` output.
- Docs: mention `--no-execute` in the render CLI docs if they list flags.

## Incidental finding (not in scope)

The `Render` arm drops many declared flags through `..`. These are accepted
and silently ignored: `--site-url`, `-P/--execute-param`, `--execute-params`,
`--execute-dir`, `--execute-daemon`, `--execute-daemon-restart`,
`--execute-debug`, `--use-freezer`, `--cache` (whose help also advertises an
undeclared `--no-cache`), `--cache-refresh`, `--no-clean`, `--log`,
`--log-level`, `--log-format`, and the trailing `-- <PANDOC_ARGS>`. Filed
separately as bd-pyxh7mit (related).

## Verification commands used

```sh
gh issue view 1008 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
target/debug/q2 render repro.qmd --no-execute          # exit 2
target/debug/q2 render --help | grep -i execute
# from a scratch copy outside the repo:
q2 render repro.qmd                                    # [1] 2 present
q2 render repro.qmd -M 'execute:{enabled: false}'      # [1] 2 absent
q2 render repro.qmd -M 'execute:enabled:false'         # [1] 2 present (not expanded)
grep -rn execution_policy crates/quarto/src crates/quarto-core/src
braid search no-execute
```

## Cross-references

- bd-sl79jjiq: introduced `ExecutionPolicy` (plan `claude-notes/plans/2026-09-22-q2-preview-static.md`)
- bd-nn2fou8h: adjacent execute-option handling bug

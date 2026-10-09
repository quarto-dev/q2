# Render error/warning summary line (bd-ooleh)

## Overview

After a `q2 render` — especially a large project render whose per-file
diagnostics have long since scrolled off the top of the terminal — the
user has no quick way to know "did this run produce problems, and how
many?". We want to print **one** short, formatted line at the end of a
render summarizing the **total** number of errors and warnings the
process generated.

Example (exact wording TBD in design iteration):

```
Rendered 8 of 10 files to _site — 3 errors, 5 warnings
```

This issue is **design-first**: this document captures what the code
currently does and the open decisions. Decisions D1–D4 are **resolved**
(2026-06-02, see below); D5/D6 are formatting/plumbing details settled
at implementation time. Next step is TDD implementation.

## Resolved design (2026-06-02)

- **Counting is diagnostic-level, not file-level.** The numbers are an
  estimate of *total work to get a clean build*, so we count every
  reported problem, not "how many files failed". (D2)
- **Every reported diagnostic is counted by its `DiagnosticKind`,
  across all four sources.** A failure (`pass1` or `pass2`) with **no**
  structured diagnostics contributes **1 error** (its top-level error
  string is the one problem). A failure **with** structured diagnostics
  contributes those diagnostics counted by kind. `Info` / `Note` are
  ignored. (D1 + D2)
- **`pass1_failures` count as errors** — a file with no structured
  diagnostics falls to the "+1 error" rule, so the summary agrees with
  the non-zero exit code rather than the legacy `warning:` display
  prefix. (D1 = option (a))
- **Augment the existing project line**, don't add a second line:
  `Rendered 8 of 10 files to _site — 3 errors, 5 warnings`. (D4)
- **The counts clause is conditional on there being something to
  report.** Zero errors and zero warnings → no clause at all (the
  `Rendered N of M …` line is unchanged), exactly as a single-file
  render omits the "total files" line. For a single-file render (which
  has no "Rendered…" line today) we print a standalone counts line
  **only when** errors + warnings > 0; clean single-file runs stay
  silent. (D3)

## What the code does today (findings)

### Entry point and existing summary line

- CLI dispatch: `crates/quarto/src/main.rs:564` → `commands::render::execute`.
- `execute` (`crates/quarto/src/commands/render.rs:531`) classifies the
  input into a `RenderTarget` and dispatches to:
  - `execute_single_doc` (`render.rs:604`) for a single `.qmd`, or
  - `execute_project` (`render.rs:649`) for a project / subset.
- Both run the pipeline and get back a
  `ProjectRenderSummary`, then call `print_render_diagnostics`
  (`render.rs:744`).
- Only the **project** path prints a trailing summary line today, via
  `render_summary_line` (`render.rs:470`):
  ```
  Rendered {rendered} of {total_files} files to {output_dir}
  ```
  `render_summary_line` returns `None` for single-file renders
  (`render.rs:476`), so single files get no trailing line.
- The summary line is emitted with `quarto_util::user_status!`
  (`render.rs:711`) — **stderr**, silenced by `--quiet`, bypasses
  `EnvFilter`. This is the channel our new line should use too.

### Where the error/warning data lives

`ProjectRenderSummary` (`crates/quarto-core/src/project/orchestrator.rs:482`):

```rust
pub struct ProjectRenderSummary<O = RenderToFileResult> {
    pub outputs: Vec<O>,                         // successful per-file renders
    pub pass1_failures: Vec<FileFailure>,        // parse/metadata failures
    pub pass2_failures: Vec<FileFailure>,        // render failures
    pub project_diagnostics: Vec<DiagnosticMessage>, // project-level diags
    pub stopped_early: bool,                      // --fail-fast cut it short
}
```

There are **four** distinct sources of errors/warnings:

1. **`pass1_failures`** — `Vec<FileFailure>`. Each is a file that failed
   the profile/metadata pass. *Presented today with a `warning:` prefix*
   (`render.rs:771-777`) **but** forces a non-zero exit
   (`should_exit_nonzero`, `render.rs:734`). Each `FileFailure` may also
   carry structured `diagnostics: Vec<DiagnosticMessage>`.
2. **`pass2_failures`** — `Vec<FileFailure>`. Render failures. Presented
   with an `error:` prefix; "legacy" failures have no structured
   diagnostics, others carry one or more `DiagnosticMessage`s that get
   coalesced by source location for display (`render.rs:791-817`).
3. **`project_diagnostics`** — `Vec<DiagnosticMessage>`. Project-level,
   a mix of severities by `kind` (`render.rs:818-820`).
4. **`outputs[*].render_output.diagnostics`** — per-file warnings from
   *successful* renders (`render.rs:822-830`); mostly `Warning`.

### Severity type

`DiagnosticKind` (`crates/quarto-error-reporting/src/diagnostic.rs:9`):

```rust
pub enum DiagnosticKind { Error, Warning, Info, Note }
```

Ariadne color mapping already in the codebase
(`diagnostic.rs:722-725`): Error→Red, Warning→Yellow, Info→Cyan,
Note→Blue. We can reuse Red/Yellow for the summary line.

### Single vs. project

Render already handles both single-file and whole-project/subset
(`RenderTarget` in `execute`). The "total" must aggregate across **all**
files in a project run — the data already is aggregated in the single
`ProjectRenderSummary`, so no new plumbing through the pipeline is
needed.

## Design decisions (D1–D4 RESOLVED — see "Resolved design" above)

### D1 — Counting semantics (the crux) — RESOLVED: option (a)

How do we turn the four sources above into an `(errors, warnings)` pair?
The user asked for "total number of **errors and warnings** the process
generated" — i.e. severity-based counts, not file counts. Proposed
default model:

- **errors** =
  - each `pass2_failure`: count its `Error`-kind diagnostics, or `1` if
    it has no structured diagnostics (legacy single-line failure), **+**
  - `project_diagnostics` with `kind == Error`, **+**
  - `outputs[*].render_output.diagnostics` with `kind == Error`.
- **warnings** =
  - each `pass1_failure`: today it's *displayed* as a warning → count
    as `1` warning (or its `Warning`-kind diagnostics), **+**
  - `Warning`-kind diagnostics inside `pass2_failures`, **+**
  - `project_diagnostics` with `kind == Warning`, **+**
  - `outputs[*].render_output.diagnostics` with `kind == Warning`.
- `Info` / `Note` are **excluded** from the summary.

**Tension to resolve:** `pass1_failures` are *shown* as warnings but
*force a non-zero exit*. Counting them as warnings means the line could
read "0 errors, 2 warnings" on a run that exits non-zero — confusing.
Options:
- (a) count pass1_failures as **errors** (matches exit behavior), or
- (b) count as **warnings** (matches current display), or
- (c) count them in a third bucket / separate phrasing.

Recommendation: **(a)** — the summary should agree with the exit code.
But this is exactly the kind of thing to confirm.

### D2 — File-count vs diagnostic-count — RESOLVED: diagnostic-count

The user wants **total warnings and errors** as an estimate of the
total amount of work to reach a clean build — i.e. count every reported
problem (diagnostic-level), *not* "how many files failed". The existing
`rendered of total` clause already conveys the file split.

### D3 — When to print the line — RESOLVED: only when non-zero

- Project render: augment the existing summary line **only when** there
  is ≥1 error or warning; clean runs leave `Rendered N of M …`
  unchanged.
- Single-file render: print a standalone counts line **only when**
  ≥1 error or warning; clean single-file runs stay silent (consistent
  with today's no-trailing-line behavior).

### D4 — One line vs. augmenting the existing line — RESOLVED: augment

- Project: **augment** → `Rendered 8 of 10 files to _site — 3 errors, 5 warnings`.
- Single file: standalone line (no "Rendered…" line exists to augment),
  printed only when non-zero.

### D5 — Formatting details

- Pluralization: `1 error` / `2 errors`, `1 warning` / `2 warnings`.
- Zero handling: drop a zero clause (`3 errors` alone if 0 warnings) vs
  always show both.
- Color: errors in **red**, warnings in **yellow**, respecting
  `NO_COLOR` / non-tty (and matching how diagnostics already colorize).
  Need a small tty/`NO_COLOR` check since `user_status!` is plain
  `eprintln!`.

### D6 — Channel & machine-readable path

- Emit on **stderr** via `user_status!` (silenced by `--quiet`),
  consistent with the existing summary line and all diagnostics.
- Under `--json-errors`, **do not** print the human summary line — that
  path (`print_render_diagnostics_json`) is for machine consumers that
  count for themselves. (Confirm: or do we add a structured count object
  there too?)

## Proposed implementation shape (subject to design above)

1. **quarto-core** — add a counting method/struct on
   `ProjectRenderSummary`, e.g.
   ```rust
   pub struct DiagnosticCounts { pub errors: usize, pub warnings: usize }
   impl<O> ProjectRenderSummary<O> {
       pub fn diagnostic_counts(&self) -> DiagnosticCounts { … }
   }
   ```
   Pure, unit-testable, no I/O. Encodes the resolved counting rule:
   every diagnostic counted by `kind` across all four sources; a
   failure with no structured diagnostics adds 1 error; Info/Note
   ignored.
2. **quarto/render.rs** — a formatting function
   `fn error_warning_clause(counts: &DiagnosticCounts) -> Option<String>`
   (pluralization, zero-handling, color), plus wiring into
   `execute_project` (augment `render_summary_line`) and
   `execute_single_doc` (standalone line). Honor `--quiet` and
   `--json-errors`.

## Test plan (TDD — write first)

- **quarto-core** unit tests for `diagnostic_counts`:
  - empty summary → `0, 0`.
  - pass2 legacy failure (no diagnostics) → counted per D1.
  - pass2 failure with N Error diagnostics → N errors.
  - project_diagnostics mix Error/Warning/Info/Note → only Error/Warning
    counted.
  - output (successful-render) warnings counted.
  - pass1_failure with no structured diagnostics → 1 error (D1=a).
  - a combined fixture exercising all four sources at once.
- **quarto/render.rs** unit tests for the formatting function:
  - pluralization (0/1/2 each), zero-clause omission, both-zero → `None`.
  - color on/off (NO_COLOR / non-tty) if we colorize.
- **End-to-end** (per CLAUDE.md): build a small fixture project that
  deliberately emits ≥1 warning and ≥1 error, run
  `cargo run --bin q2 -- render <fixture>` and inspect the actual final
  stderr line. Record the exact invocation + observed line in this plan
  before declaring done. Also verify `--quiet` suppresses it and
  `--json-errors` omits the human line.

## End-to-end verification (2026-06-02)

Built `cargo run --bin q2 -- render …` against a scratch project
(`.scratch-ooleh/`, since deleted). Output inspected directly; stderr
shown. All cases below were observed, not inferred.

- **Project, errors + warnings** — a 3-file `default` project: two pages
  with unresolved `@fig-*` crossrefs (→ warnings) + one page with
  `theme: this-theme-does-not-exist` (→ a Pass-2 error that fails just
  that file):
  ```
  $ cargo run --bin q2 -- render .scratch-ooleh/proj
  …
  Rendered 2 of 3 files to …/proj — 1 error, 3 warnings
  $ echo $?   →   1
  ```
  The clause is appended to the existing "Rendered N of M" line (D4),
  counts are diagnostic-level (D2), and exit is non-zero.
- **No ANSI when stderr is piped** — `grep -c $'\x1b'` on the
  `Rendered …` line returned 0; `stderr_use_color()` correctly
  suppresses color off-tty. (Interactive color path is covered by the
  `counts_clause_color_wraps_counts_in_ansi` unit test; not re-checked
  in a live PTY.)
- **`--quiet`** — entire summary suppressed (`grep -c` for the line → 0).
- **`--json-errors`** — line prints as bare
  `Rendered 2 of 3 files to …/proj` with **no** counts clause (D6).
- **Standalone single file with a warning** — a lone
  `@fig-ghost` file (not in a project) → its own line `1 warning`,
  exit 0 (warnings don't fail). Confirms the `execute_single_doc`
  standalone-line path and singular pluralization.
- **Clean single file** — no diagnostics → no clause line at all (D3).

## Work items

- [x] Iterate on design decisions D1–D4 with the user (resolved 2026-06-02)
- [x] (TDD) Write `diagnostic_counts` unit tests in quarto-core — verify they fail (7 tests, red confirmed via missing-type compile error)
- [x] Implement `DiagnosticCounts` + `OutputDiagnostics` + `diagnostic_counts` in quarto-core (all 7 green)
- [x] (TDD) Write formatting-function unit tests in quarto/render.rs — verify they fail (8 tests, red confirmed)
- [x] Implement `format_counts_clause` / `count_phrase` / `colorize` / `stderr_use_color` + wiring in execute_project / execute_single_doc (all green)
- [x] Honor `--quiet` (via `user_status!`) and `--json-errors` (clause gated on `!args.json_errors`); color via `IsTerminal` + `NO_COLOR`
- [x] End-to-end fixture render; record invocation + observed line here (see "End-to-end verification" above)
- [x] `cargo nextest run --workspace` — my crates green (quarto-core + quarto, 2287/2287). Initial run showed failures in `quarto-citeproc locale::tests` and later `quarto-error-reporting::schema_drift` — **diagnosed as a polluted build cache, NOT flakes and NOT this change** (see note below). Re-verifying after `cargo clean`.
- [x] `cargo xtask verify` (full, exercises WASM leg) — **clean `target/`, VERIFY_EXIT=0: 9515/9515 Rust tests passed, hub-client suites green (79+10+181+180+62), fresh WASM build + vite bundle.** Confirms the trait's wasm32 cfg impl compiles and the citeproc/schema_drift failures were the stale-cache artifacts.

### Build-cache pollution diagnosis (2026-06-02)

Spurious `--workspace`-only test failures (`quarto-citeproc
locale::tests`, then `quarto-error-reporting::schema_drift`) were traced
to **stale test binaries in the shared `target/`** carrying a baked-in
`env!("CARGO_MANIFEST_DIR")` pointing at a **deleted worktree**:
`.worktrees/bd-3klmk-flaky-test-passoneusesmultiplethreads-asserts/…`.
\~26k objects under `target/debug/deps` referenced that dead path; cargo
reused them on fingerprint match. The schema-drift test reported the
checked-in schema as "no file on disk" because it looked under the
dead worktree; the locale tests fell back to English because
rust-embed's dynamic (debug) loader read from the dead path. Both
**pass the moment the crate is actually recompiled** and are unrelated
to this feature (quarto-citeproc / quarto-error-reporting don't depend
on quarto-core or quarto, and the failures reproduce with this change
stashed). Fix: `cargo clean` + rebuild.
- [ ] Update beads bd-ooleh, sync, commit (await push approval)

## References

- bd-ooleh
- `crates/quarto/src/commands/render.rs` (470, 531, 604, 649, 708-718, 734, 744-848)
- `crates/quarto-core/src/project/orchestrator.rs:482` (`ProjectRenderSummary`)
- `crates/quarto-error-reporting/src/diagnostic.rs:9` (`DiagnosticKind`)
- `crates/quarto-util/src/user_status.rs` (`user_status!`)

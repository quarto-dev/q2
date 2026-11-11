---
title: "One terminal output-style policy for q2 (bd-be8ve52f, GH #1135)"
date: 2026-11-11
---

## Overview

q2 decides whether to emit ANSI color and OSC 8 hyperlinks in at least four
independent, disagreeing places. The visible bug is GH #1135 / bd-be8ve52f:
render diagnostics hard-code color on and ignore `NO_COLOR` and non-tty
stderr. The audit and the conventions (`NO_COLOR`, `CLICOLOR`,
`CLICOLOR_FORCE` / `FORCE_COLOR`, `--color`) are in
`claude-notes/issue-reports/1135/triage.md`. The maintainer approved this
shape on 2026-11-11, with two notes:

- **The style must be resolved on the main thread, early**, before the tokio
  runtime, worker threads, or any output.
- **Before ANSI stops being baked into stored strings, prove nothing loses
  color it correctly has today** (item 3 of the triage).

The upstream default flip is filed separately as
posit-dev/quarto-error-reporting#12 and is out of scope here.

## Design

### Resolver

`quarto_util::output_style` holds a pure function and a once-set global:

```rust
pub enum ColorFlag { Auto, Always, Never }
pub struct StreamStyle { pub color: bool, pub hyperlinks: bool }
pub struct OutputStyle { pub stdout: StreamStyle, pub stderr: StreamStyle }

pub struct StyleInputs<'a> {   // everything the decision reads, injectable
    pub flag: ColorFlag,
    pub env: &'a dyn Fn(&str) -> Option<OsString>,
    pub stdout_is_tty: bool,
    pub stderr_is_tty: bool,
}
pub fn resolve(inputs: &StyleInputs) -> OutputStyle;      // pure, unit-tested
pub fn init(flag: ColorFlag) -> OutputStyle;              // reads process; sets OnceLock
pub fn get() -> OutputStyle;                              // plain if never init'd
```

Color is decided per stream. The first matching rule wins:

1. `--color=never` → off. `--color=always` → on.
2. `NO_COLOR` is non-empty → off.
3. `CLICOLOR_FORCE` or `FORCE_COLOR` is non-empty and not `0` → on.
4. `CLICOLOR=0` → off.
5. The stream is a tty and (`TERM` supports color, `CLICOLOR` is set, or
   `CI` is set) → on.
6. Otherwise → off.

Rules 2–6 match `anstream` 1.0.0 `auto::choice`. We use `anstyle-query` for
the env predicates. `FORCE_COLOR=0` is our only deliberate extension: it
falls through instead of forcing color.

Hyperlinks follow these rules:

- `--color=never` → off.
- Otherwise, `FORCE_HYPERLINK` set and not `0` → on. `FORCE_HYPERLINK=0` →
  off.
- Otherwise, on only when color is on, the stream is a tty, and the terminal
  supports OSC 8. Use the `supports-hyperlinks` heuristic, fed the injected
  tty bit.

So forcing color into a pipe or a CI log never adds hyperlinks.

`get()` returns plain when `init` never ran. That way a library, test or
WASM caller that never initialized fails safe.

### Where it runs

The very first statements of each binary's `main` resolve the style, right
after argument parsing:

- `q2` (`crates/quarto/src/main.rs`);
- `hub` (`crates/quarto-hub/src/main.rs`; `#[tokio::main]` is still the main
  thread at that point);
- `pampa`;
- `qmd-syntax-helper`.

On Windows, `init` calls `anstyle_query::windows::enable_ansi_colors()` when
it turns color on for a tty.

### Threading

This follows the `user_status!` convention: formatting functions take an
explicit `StreamStyle` or `TextRenderOptions`, so they stay testable without
global state. Only command entry points call `output_style::get()`.

## Checklist

### Phase 0: tests first (all must fail before Phase 1)

- [x] Unit: `resolve` precedence table. Cover every rule above, per stream,
      with an injected env and tty. Include empty-value cases (`NO_COLOR=`,
      `CLICOLOR_FORCE=`), `FORCE_COLOR=0`, `--color` beating `NO_COLOR`,
      `NO_COLOR` beating `CLICOLOR_FORCE`, and the `FORCE_HYPERLINK` cases.
- [x] Unit: `get()` before `init` is plain.
- [x] Integration (`crates/quarto/tests/integration/terminal_output_style.rs`),
      driving the `q2` binary on the #1135 fixture (missing include) with
      stderr captured, which makes it non-tty:
  - [x] default → 0 SGR, 0 OSC 8 (the #1135 non-tty case);
  - [x] `NO_COLOR=1` with `CLICOLOR_FORCE=1` → 0 SGR, 0 OSC 8;
  - [x] `CLICOLOR_FORCE=1` → SGR > 0, 0 OSC 8;
  - [x] `--color=always` → SGR > 0, 0 OSC 8;
  - [x] `--color=never` with `CLICOLOR_FORCE=1` → 0 SGR;
  - [x] `--color=always` with `FORCE_HYPERLINK=1` → OSC 8 > 0;
  - [x] `-vv` tracing into a pipe → 0 SGR; `-vv --color=always` → SGR > 0.
- [x] Integration (pampa, `test_terminal_output_style.rs`): a parse error (unclosed fenced div; the newline warning has no snippet, hence no color) with stderr
      captured → 0 SGR, 0 OSC 8.

### Phase 1: resolver

- [x] `crates/quarto-util/src/output_style.rs` plus deps (`anstyle-query`,
      `supports-hyperlinks`). Confirm `quarto-util` still builds for
      `wasm32-unknown-unknown` (pampa depends on it).
- [x] Phase 0 unit tests pass.

### Phase 2: binaries resolve early

- [x] `q2`: global `--color <auto|always|never>` on `Cli`. Call `init` right
      after `Cli::parse()` and before the tracing setup. Pass
      `.with_ansi(style.stderr.color)` to the fmt layer.
- [x] clap's own `--help` coloring: leave it on `anstream` auto (it already
      honors `NO_COLOR`). Document that `--color` does not affect help.
      Decide during implementation whether a pre-scan is worth it.
- [x] `hub`: same `--color` flag, `init` before tracing in both setup
      branches, `.with_ansi`.
- [x] `pampa`: `init(ColorFlag::Auto)` (a flag is optional). Every
      `to_text(None)` print site renders with `get().stderr`.
- [x] `qmd-syntax-helper`: `colored::control::set_override(get().stdout.color)`.

### Phase 3: thread to every q2 sink

- [x] `render.rs`: `print_render_diagnostics` passes the stderr style. This
      replaces the hard-coded `true` at `render.rs:1447`.
      `stderr_use_color()` stays as a one-line reader of `get().stderr.color`
      for the counts clause.
- [x] `format_render_diagnostics_text` takes `StreamStyle` (color and
      hyperlinks separately) in place of one `color: bool`.
      `RenderReport::diagnostics_text(false)` callers (preview overlay) pass
      plain.
- [x] `emit_cli_diagnostic` (`render.rs:2156`).
- [x] `create/mod.rs:94,145`, `add/mod.rs:149`, `use_cmd/mod.rs:134,143`.
- [x] `quarto-preview/src/lib.rs:602`.
- [x] Re-grep for `to_text(None)` and `eprintln!` of diagnostics in non-test
      code. Every terminal sink must take the style.
      Done 2026-11-11. The terminal sinks are all converted. What remains
      either builds a string rather than printing it (moved to Phase 4) or
      is the `pampa` dev binary `ast_reconcile` (left as is).
- [x] Shared helper `crates/quarto/src/commands/terminal_style.rs`
      (`text_render_options`, `stderr_text_options`).

### Phase 4: stop baking styling into strings (regression-guarded)

The maintainer chose the plain `Display` with rendering at the sinks
(2026-11-11), not a `Display` that reads the global style.

- [x] Audit every path where a `ParseError` / `QuartoError` `Display` (styled
      before) reaches the terminal:
  - `exit_with_abort`: `DiscoverParse` (`eprintln!("{pe}")`),
    `RenderAbort::Parse`, and `exit_with_parse_error` (`Scripts`);
  - `RenderAbort::user_message`, used by `q2 preview --static`;
  - `quarto-preview` pre-render script failure (`eprintln!("{parse_error}")`);
  - `main`, for any `QuartoError::Parse` / `ParseError` returned through
    anyhow.

  The string producers that called `to_text(None)` are
  `engine/ts_record.rs`, `pampa` `DiagnosticCollector::to_text`, the `pampa`
  WASM entry points, and `wasm-quarto-hub-client` (3 sites).
- [x] Guard tests written first. In `terminal_output_style.rs`:
      `config_abort_*` (Dispatch, project and single-file) and
      `script_abort_*` (Scripts). Each has a pipe test (plain; it failed
      before) and a `--color=always` guard (SGR > 0; it passed before and
      still passes). There are also unit tests for
      `format_error_report` and `ParseError::{Display, render_with}`.
- [x] `ParseError::render_with(&TextRenderOptions)`. `render()` and `Display`
      are now plain. Every terminal sink above renders with the stderr
      style.
- [x] `main` became a wrapper around `run()`. When it holds a parse error,
      `commands::terminal_style::format_error_report` prints it, either bare
      or with anyhow's `Caused by:` layout. Every other error prints exactly
      as before (`Error: {err:?}`).
- [x] The string producers now pass `TextRenderOptions::plain()` explicitly.
- [x] `strip_ansi_escapes` is **kept** in two places: the opaque-message
      fallback of `failure_error_text` and the preview abort message. They
      no longer clean up our own output. They guard against external tool
      text (pandoc, engines) that carries its own escapes.
- [x] End to end, from a file and from a terminal simulated with `script`
      (SGR/OSC 8 counts in the table below). A non-parse error
      (`q2 render does-not-exist.qmd`) prints unchanged.

| case | pipe (before → after) | tty (before = after) |
|------|------|------|
| broken `_quarto.yml` | 62/2 → 0/0 | 62/2 |
| unknown project type | 90/2 → 0/0 | 90/2 |
| missing pre-render script | 102/2 → 0/0 | 102/2 |

### Phase 5: docs

- [x] New user-facing page `docs/tools/terminal-output.qmd`. Cover
      `--color`, `NO_COLOR`, `CLICOLOR` / `CLICOLOR_FORCE` / `FORCE_COLOR`,
      `FORCE_HYPERLINK`, and the precedence. Explain that hyperlinks appear
      only in a capable terminal, and that `--help` follows the env vars, not
      `--color`. Add it to the `tools` sidebar in `docs/_quarto.yml`.
- [x] Render it with `cargo run --bin q2 -- render docs/` (Quarto 2).

### Phase 6: verification

- [x] `cargo nextest run -p quarto-util -p quarto -p pampa`, plus clippy
      `-D warnings` for every touched crate.
- [x] Windows cross-check:
      `cargo clippy -p quarto-util -p quarto --all-targets --target x86_64-pc-windows-msvc -- -D warnings`.
- [x] End to end: rerun `claude-notes/issue-reports/1135/repro/count-escapes.sh`
      against the new binary and record the table below. Also check
      interactively in a real terminal that color and links still appear.
- [ ] Full `cargo xtask verify`, including the WASM leg, because
      `quarto-util` is a pampa/WASM dependency. Known unrelated local
      failure: bd-54odm3rz (knitr `rlang_error`).
- [ ] Close bd-be8ve52f and reply on #1135 (wording to be confirmed with the
      maintainer).

## End-to-end record

Recorded 2026-11-11 with `target/debug/q2` built on branch `issue-1135`
(Phases 0–3 and 5). All output below was inspected.

Before and after, `claude-notes/issue-reports/1135/repro/count-escapes.sh`
with stderr redirected to a file:

| mode                          | before (main) | after |
|-------------------------------|---------------|-------|
| diagnostics                   | 162 SGR, 2 OSC 8 | 0, 0 |
| diagnostics, `NO_COLOR=1`     | 162, 2        | 0, 0  |
| `--json-errors`               | 0, 0          | 0, 0  |
| tracing `-vv`                 | 980, 0        | 0, 0  |
| tracing `-vv`, `NO_COLOR=1`   | 0, 0          | 0, 0  |
| `render --help` piped         | 0, 0          | 0, 0  |

A real terminal, simulated with `script -q tty.out q2 render index.qmd` and
`TERM=xterm-256color`:

| case                                   | SGR | OSC 8 |
|----------------------------------------|----:|------:|
| `TERM_PROGRAM=ghostty`                 | 164 | 2 (unchanged from before) |
| `TERM_PROGRAM=Apple_Terminal`          | 164 | 0 (unrecognized terminal) |
| `TERM_PROGRAM=Apple_Terminal FORCE_HYPERLINK=1` | 164 | 2 |
| `NO_COLOR=1` / `CLICOLOR=0` / `TERM=dumb` | 0 | 0 |

`--help` under the same terminal: 93 SGR by default, 0 with `NO_COLOR=1`,
93 with `--color=never` (help follows the environment only, as the docs
page says).

Test runs: `cargo nextest run` for quarto, pampa, quarto-util,
quarto-preview, quarto-hub and qmd-syntax-helper gives 7371 tests, all
passing after one test was updated. That test,
`json_errors::ipynb_diagnostic_hyperlinks_real_notebook`, pinned "hyperlinks
even when stderr is not a terminal". It now asks for links with
`FORCE_HYPERLINK=1`. Clippy `-D warnings` is clean on all six crates. The
Windows clippy of `quarto-util` is clean.

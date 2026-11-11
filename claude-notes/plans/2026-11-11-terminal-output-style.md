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

- [ ] Unit: `resolve` precedence table. Cover every rule above, per stream,
      with an injected env and tty. Include empty-value cases (`NO_COLOR=`,
      `CLICOLOR_FORCE=`), `FORCE_COLOR=0`, `--color` beating `NO_COLOR`,
      `NO_COLOR` beating `CLICOLOR_FORCE`, and the `FORCE_HYPERLINK` cases.
- [ ] Unit: `get()` before `init` is plain.
- [ ] Integration (`crates/quarto/tests/integration/terminal_output_style.rs`),
      driving the `q2` binary on the #1135 fixture (missing include) with
      stderr captured, which makes it non-tty:
  - [ ] default → 0 SGR, 0 OSC 8 (the #1135 non-tty case);
  - [ ] `NO_COLOR=1` with `CLICOLOR_FORCE=1` → 0 SGR, 0 OSC 8;
  - [ ] `CLICOLOR_FORCE=1` → SGR > 0, 0 OSC 8;
  - [ ] `--color=always` → SGR > 0, 0 OSC 8;
  - [ ] `--color=never` with `CLICOLOR_FORCE=1` → 0 SGR;
  - [ ] `--color=always` with `FORCE_HYPERLINK=1` → OSC 8 > 0;
  - [ ] `-vv` tracing into a pipe → 0 SGR; `-vv --color=always` → SGR > 0.
- [ ] Integration (pampa): the "no trailing newline" warning with stderr
      captured → 0 SGR, 0 OSC 8.

### Phase 1: resolver

- [ ] `crates/quarto-util/src/output_style.rs` plus deps (`anstyle-query`,
      `supports-hyperlinks`). Confirm `quarto-util` still builds for
      `wasm32-unknown-unknown` (pampa depends on it).
- [ ] Phase 0 unit tests pass.

### Phase 2: binaries resolve early

- [ ] `q2`: global `--color <auto|always|never>` on `Cli`. Call `init` right
      after `Cli::parse()` and before the tracing setup. Pass
      `.with_ansi(style.stderr.color)` to the fmt layer.
- [ ] clap's own `--help` coloring: leave it on `anstream` auto (it already
      honors `NO_COLOR`). Document that `--color` does not affect help.
      Decide during implementation whether a pre-scan is worth it.
- [ ] `hub`: same `--color` flag, `init` before tracing in both setup
      branches, `.with_ansi`.
- [ ] `pampa`: `init(ColorFlag::Auto)` (a flag is optional). Every
      `to_text(None)` print site renders with `get().stderr`.
- [ ] `qmd-syntax-helper`: `colored::control::set_override(get().stdout.color)`.

### Phase 3: thread to every q2 sink

- [ ] `render.rs`: `print_render_diagnostics` passes the stderr style. This
      replaces the hard-coded `true` at `render.rs:1447`. Delete
      `stderr_use_color()` and use the same style for the counts clause.
- [ ] `format_render_diagnostics_text` takes `StreamStyle` (color and
      hyperlinks separately) in place of one `color: bool`.
      `RenderReport::diagnostics_text(false)` callers (preview overlay) pass
      plain.
- [ ] `emit_cli_diagnostic` (`render.rs:2156`).
- [ ] `create/mod.rs:94,145`, `add/mod.rs:149`, `use_cmd/mod.rs:134,143`.
- [ ] `quarto-preview/src/lib.rs:602`.
- [ ] Re-grep for `to_text(None)` and `eprintln!` of diagnostics in non-test
      code. Every terminal sink must take the style.

### Phase 4: stop baking styling into strings (regression-guarded)

- [ ] Audit every path where a `ParseError` / `QuartoError` `Display` (styled
      today) reaches the terminal. Known so far:
  - `RenderAbort::user_message` (`render.rs:809`);
  - `exit_with_abort` → `main`\'s anyhow `Error:` print;
  - `preview_static.rs:870` (abort `eprintln!`, then `strip_ansi_escapes`
    for the browser).

  Record the full list here.
- [ ] Before any change, add a `--color=always` test per terminal path that asserts SGR > 0. A plain `Display` that loses color must fail these tests.
- [ ] Convert each path to explicit rendering (for example `ParseError::render_with(&TextRenderOptions)`), using the stderr style.
- [ ] Make `ParseError`\'s `Display` plain (`TextRenderOptions::plain()`).
      Then remove the `strip_ansi_escapes` calls that become dead (preview
      abort; the opaque-message fallback in `failure_error_text`).
- [ ] Check that the WASM / hub-client paths that stringify `QuartoError`
      (`wasm-quarto-hub-client/src/lib.rs`) now get plain text.
- [ ] If any path can't be converted cleanly, **stop and ask**. Do not keep a
      strip-after-the-fact hack.

### Phase 5: docs

- [ ] New user-facing page `docs/tools/terminal-output.qmd`. Cover
      `--color`, `NO_COLOR`, `CLICOLOR` / `CLICOLOR_FORCE` / `FORCE_COLOR`,
      `FORCE_HYPERLINK`, and the precedence. Explain that hyperlinks appear
      only in a capable terminal, and that `--help` follows the env vars, not
      `--color`. Add it to the `tools` sidebar in `docs/_quarto.yml`.
- [ ] Render it with `cargo run --bin q2 -- render docs/` (Quarto 2).

### Phase 6: verification

- [ ] `cargo nextest run -p quarto-util -p quarto -p pampa`, plus clippy
      `-D warnings` for every touched crate.
- [ ] Windows cross-check:
      `cargo clippy -p quarto-util -p quarto --all-targets --target x86_64-pc-windows-msvc -- -D warnings`.
- [ ] End to end: rerun `claude-notes/issue-reports/1135/repro/count-escapes.sh`
      against the new binary and record the table below. Also check
      interactively in a real terminal that color and links still appear.
- [ ] Full `cargo xtask verify`, including the WASM leg, because
      `quarto-util` is a pampa/WASM dependency. Known unrelated local
      failure: bd-54odm3rz (knitr `rlang_error`).
- [ ] Close bd-be8ve52f and reply on #1135 (wording to be confirmed with the
      maintainer).

## End-to-end record

_(fill in at Phase 6)_

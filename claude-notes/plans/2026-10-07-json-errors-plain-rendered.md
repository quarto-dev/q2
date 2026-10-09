---
title: '`q2 render --json-errors`: escape-free `rendered` and pass-1 `error` (R1)'
date: 2026-10-07
---

**Strand:** bd-ckbqmupi (item 2). Upstream: qe-hal9cc7b, released as `quarto-error-reporting` 0.4.0.
**Branch:** `braid/bd-ckbqmupi-q2-render-json-errors`, off `main` @ `5c61810b1` (after PR #795).
Predecessor plan: `2026-10-06-json-errors-output-hygiene.md` (R1 in its broader review).

## Overview

Under `--json-errors`, every human-readable string on the wire is full of terminal escapes:

- `JsonDiagnostic.rendered`: ariadne SGR color runs around single characters, plus OSC-8 hyperlinks;
- `JsonPass1Failure.error`: `ParseError::render()` = the same colored text for all of the failure's
  diagnostics, joined.

That is unreadable to a jq pipeline or an agent without an ANSI stripper. `quarto-error-reporting`
0.4.0 adds `TextRenderOptions::plain()` and `diagnostic_to_json_with_options`. Its default
(`diagnostic_to_json`) is unchanged.

## Design

- **Scope: the `--json-errors` stream only.** Text mode keeps color and hyperlinks.
  `diagnostic_to_json` callers outside the CLI (hub-client, preview) keep the default; they strip
  escapes on the JS side, as documented.
- **`rendered`:** `diagnostic_json` (the single JSON conversion point added in bd-gnw9asuo) calls
  `diagnostic_to_json_with_options(.., &TextRenderOptions::plain())`.
- **pass-1 `error`:** when the failure carries structured diagnostics and a source context, the CLI
  re-renders `error` plain from them (same content and order as `ParseError::render`). A failure
  without structured diagnostics already has a plain message (`QuartoError`\'s non-parse variants
  are plain strings), so it passes through unchanged. The field stays (the schema requires it);
  dropping it would be a wire break for no gain.
- **Plain means no OSC-8 either.** `ipynb_diagnostic_hyperlinks_real_notebook` asserted the
  notebook hyperlink inside JSON `rendered`. Hyperlinks remain the documented *text-mode* behavior,
  and JSON carries the target structurally (`origin.notebook_path`, pinned by
  `ipynb_parse_error_json_carries_cell_origin`). So the hyperlink assertions move to a text-mode
  run, and the JSON side asserts the plain label. Verified: text mode emits OSC-8 even when stderr
  is piped.
- **Text mode's no-color form (folds in bd-6d9ew2up).** `format_render_diagnostics_text(color: false)`
  (the `q2 preview --static` overlay) rendered with color and then ran `strip_ansi_escapes` over
  the whole output, because 0.2.2 had no color switch. It now renders with
  `TextRenderOptions::default().color(color).hyperlinks(color)`, and the post-hoc strip is gone.
  The pass-1 / legacy failure lines that strip also cleaned go through the same helper,
  `failure_error_text`, as JSON's `error`.
  `strip_ansi_escapes` itself stays for genuinely opaque text: preview's abort messages, and a
  failure's message when it has no structured diagnostics to re-render.
- **Dependency bump:** 0.3.2 → 0.4.0, in the workspace *and* in the excluded
  `wasm-quarto-hub-client` (own manifest and lockfile; it shares `quarto-core`\'s types). Breaking only for `TextRenderOptions { .. }` struct literals →
  `TextRenderOptions::default().hyperlinks(false)`.

## Checklist

### Phase 0: tests first
- [x] `json_errors_strings_carry_no_terminal_escapes`: a mixed project (page warning + pass-1
      failure); walk every string value of every record (nested included, after JSON decoding,
      so `\u001b` escapes are caught) and assert no `\x1b`.
- [x] Retarget `ipynb_diagnostic_hyperlinks_real_notebook` to text mode; add a JSON-side
      assertion that `rendered` shows the plain pseudo-path label with no escapes.
- [x] Confirm the new or changed assertions fail on the current code for the expected reason.

### Phase 1: bump and migrate
- [x] `quarto-error-reporting` 0.4.0 in the workspace `Cargo.toml`; `cargo update -p`.
- [x] Migrate `TextRenderOptions` struct literals.

### Phase 2: plain rendering under `--json-errors`
- [x] `diagnostic_json` → `diagnostic_to_json_with_options(.., &TextRenderOptions::plain())`.
- [x] Plain pass-1 `error`.
- [x] Update the comments that describe `rendered` as ANSI.
- [x] Text no-color form uses real options; post-hoc strip removed (bd-6d9ew2up).

### Phase 3: verify
- [x] `cargo nextest run --workspace`; `cargo xtask verify` (full: the bump touches crates the
      WASM client depends on).
- [x] End-to-end on claude-notes: no `\x1b` in any decoded string; size before and after.

## Results (2026-10-07)

- **Tests first:** `json_errors_strings_carry_no_terminal_escapes` failed on exactly the three
  targets (top-level `rendered`, nested `diagnostics[].rendered`, pass-1 `error`). The retargeted
  ipynb test failed on its JSON-side plain-label assertion. Both pass now.
  `page_error_is_reported_not_fatal` (no-color text has no escapes) passes without the strip.
- **Tests:** `cargo nextest run --workspace`: 15989/15990. `smoke_all` fails for the same
  environmental reasons as before (14 knitr/R cairo, 5 missing `great_tables`).
- **Full `cargo xtask verify --skip-rust-tests --skip-css-lint`:** all steps passed, including the
  WASM hub-client build and hub-client tests. The CSS lint is skipped for bd-0jtxndiy.
  - The worktree needed a fresh `npm install` (main added `@bjorn3/browser_wasi_shim`), and its
    `package-lock.json` churn was reverted (bd-mmi6cizz).
  - One run hit the known `doc-inventory` flake (bd-fuw5gcni and siblings); it passed 3/3 in
    isolation and on the rerun.
- **End-to-end** (`target/debug/q2 render --json-errors` in `claude-notes/`, output inspected):
  - 713 lines, 0 non-JSON, **0 decoded strings containing ESC**;
  - stderr went from 6,484,754 bytes to **1,450,385** (about 4.5× smaller);
  - all 1,202 `rendered`/`error` texts equal the #795-era text with escapes stripped, so the change
    only removes escapes;
  - apart from those fields, the records differ from the #795 capture only in R5's new Q-13-4
    wording.

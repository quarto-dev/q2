# Plan: CSS inlining for typst raw HTML tables (pandoc-request R8)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-request-epic.md`](2026-10-01-pandoc-request-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D9)
**Depends on:** R2 (ungated prefix builder), R4 (typst request); R0's rebase (PR #766). **Unblocks:** hub typst output with gt/pandas tables matching native.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** R (workspace-6). **Start gate:** R2 and R4 are fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** R7 (its Start gate decides whether it can begin).

## Overview

Raw HTML tables (gt, pandas) carry their styling in a `<style>` block, which typst cannot interpret, so for typst output the styling must be moved onto the cells before pandoc converts the table. PR #766 does this inside the pandoc Lua filter by piping the HTML through `q2 inline-css` (`normalize/astpipeline.lua`, `inline_css`, via `quarto.config.cli_path()`). That cannot work in the hub: the Lua runs inside pandoc.wasm, which has no `pandoc.pipe` and no q2 binary, so `inline_css` returns the HTML unchanged. The inliner itself is the pure-Rust `css-inline` crate (default features off), which does run on `wasm32`. This phase runs it on the Rust side, where the Rust wasm already holds the AST, as a stage ahead of the pandoc request.

## Decisions

- The inlining runs as an ungated Rust stage in the typst prefix, on the AST before `PandocPrepareStage`, so native and wasm share one implementation and the host needs nothing.
- The stage mirrors the Lua filter's selection rules (typst output only; the raw-HTML-table pattern; the `html_disable_table_processing_comment` opt-out) so the Lua step finds no `<style>` left and becomes a no-op.
- No stylesheet is ever fetched, as in `q2 inline-css` today.

## Checklist

### Tests first
- [x] A typst fixture with a raw HTML table whose `<style>` rules set `text-align` and `color`: the request's input JSON carries the rules on the cells and no `<style>` block (the existing native test `crates/quarto/tests/integration/typst_html_table_css.rs` keeps passing).
- [x] The opt-out comment leaves the table untouched; a non-typst format is untouched; a long data URI survives intact (the case PR #766's tests cover).
- [x] `cargo check --target wasm32-unknown-unknown` passes with the crate in `quarto-core`.

### Tasks
- [x] **Move the inliner into `quarto-core`** (a small `inline_css` module; `css-inline` with `default-features = false`, as `crates/quarto` has it) and confirm it compiles for `wasm32` in `crates/wasm-quarto-hub-client` (a trial `cargo check` with `css-inline` 0.21 and default features off passed with no extra flags or features); the `q2 inline-css` subcommand calls the same function.
- [x] **The stage:** walk raw HTML blocks the way the Lua filter selects them (read `handle_raw_html_as_table` and `should_handle_raw_html_as_table` in `astpipeline.lua` and cover each condition: both `inline_css` call sites, the table `RawBlock` and the `html-pre-tag-processing: parse` Div, and every opt-out: the comment, the `html-table-processing="none"` Div attribute and the same-named param), inline, and write the result back into the AST; register it as the last stage of the typst prefix, after the capture splice and `user-filters` post (the splice runs before `engine-execution`, `pipeline.rs:~694-712`), so spliced and filter-generated HTML is inlined too, and test with a captured table.
- [x] **Open (decided: stage everywhere, Lua pipe and subcommand kept; Gordon, 2026-10-02) (decide at the start of this phase):** whether native `q2 render` also switches to the stage, which would make the Lua `pandoc.pipe` call and the `inline-css` subcommand dead code (recommended, for one implementation), or keeps the pipe and uses the stage only for the hub.
- [x] Update R4's recorded limitation, D9's note and the epic's Known limitations bullet to say the limitation is gone.

## Verification

Crate-scoped clippy/nextest for `quarto-core` and `quarto`; the wasm compile check; a typst fixture with a raw HTML table through the hub request and the host's parity net once H3 exists.

## Exit

A typst request produced in the hub carries inlined table styling equal to native's.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/r8-css-inlining` in `.worktrees/workspace-6`.
- Last commit; tasks ticked: all tasks ticked (see `git log pandoc-wasm/r8-css-inlining`); Close-out ticked (workspace run below).
- State and gotchas:
  - Decision (Gordon, 2026-10-02): the stage runs everywhere (it is the last stage of `build_pandoc_prefix_stages`, a no-op unless the format is typst); the Lua `pandoc.pipe` call and the `q2 inline-css` subcommand are **kept** (no-ops once the stage has run, and deleting them would change the vendored share tree and so every pandoc recording's hash). Removing them is a later cleanup.
  - `inline_css` now lives in `quarto-core` (`src/inline_css.rs`, `css-inline` 0.21 default features off, moved from `crates/quarto`); the subcommand calls it. The stage is `inline-table-css` (`stage/stages/inline_table_css.rs`). It mirrors `astpipeline.lua`: html `RawBlock` with `<table>..</table>`, not carrying the `<!--| quarto-html-table-processing: none -->` comment; `<pre>` only as the first child `RawBlock` of a `Div` with `html-pre-tag-processing="parse"`; a `Div` with `html-table-processing="none"` is skipped with its contents. The `html-table-processing` *param* is not modelled: q2 never puts it in the filter params. Blocks with no `<style>` are left byte-for-byte. A table inside a footnote (`Inline::Note`) is not visited.
  - `Q-20-10` is retired (nothing left to warn about): `typst_limits.rs`, its call in `pandoc_write.rs`, the catalog entry, `docs/errors/pandoc/Q-20-10.qmd` and its `docs/_quarto.yml` line are gone; `Q-20-9` is emitted by `PrefetchRemoteImagesStage`. H5, R4, the epic and design D9 text updated.
  - Gates: clippy `-D warnings` and nextest for `quarto-core`/`quarto`; wasm `cargo check`; `npm run build:wasm` + `test:wasm` 240 passed, 28 skipped (unchanged from R6). Workspace nextest 15615 run, 15615 passed, 202 skipped; R6 ended at 15605, so +10 = +9 stage unit tests, +2 integration (opt-out comment, non-typst) +1 captured-table test, -2 `typst_limits` tests (the 4 `inline_css` tests moved from `quarto` to `quarto-core`, net 0; two old Q-20-10 integration tests rewritten, net 0).
- Next step: none in R8. Lane order continues with R7 (check its Start gate).

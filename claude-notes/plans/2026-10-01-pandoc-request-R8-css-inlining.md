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
- [ ] A typst fixture with a raw HTML table whose `<style>` rules set `text-align` and `color`: the request's input JSON carries the rules on the cells and no `<style>` block (the existing native test `crates/quarto/tests/integration/typst_html_table_css.rs` keeps passing).
- [ ] The opt-out comment leaves the table untouched; a non-typst format is untouched; a long data URI survives intact (the case PR #766's tests cover).
- [ ] `cargo check --target wasm32-unknown-unknown` passes with the crate in `quarto-core`.

### Tasks
- [ ] **Move the inliner into `quarto-core`** (a small `inline_css` module; `css-inline` with `default-features = false`, as `crates/quarto` has it) and confirm it compiles for `wasm32` in `crates/wasm-quarto-hub-client` (a trial `cargo check` with `css-inline` 0.21 and default features off passed with no extra flags or features); the `q2 inline-css` subcommand calls the same function.
- [ ] **The stage:** walk raw HTML blocks the way the Lua filter selects them (read `handle_raw_html_as_table` and `should_handle_raw_html_as_table` in `astpipeline.lua` and cover each condition: both `inline_css` call sites, the table `RawBlock` and the `html-pre-tag-processing: parse` Div, and every opt-out: the comment, the `html-table-processing="none"` Div attribute and the same-named param), inline, and write the result back into the AST; register it as the last stage of the typst prefix, after the capture splice and `user-filters` post (the splice runs before `engine-execution`, `pipeline.rs:~694-712`), so spliced and filter-generated HTML is inlined too, and test with a captured table.
- [ ] **Open (decide at the start of this phase):** whether native `q2 render` also switches to the stage, which would make the Lua `pandoc.pipe` call and the `inline-css` subcommand dead code (recommended, for one implementation), or keeps the pipe and uses the stage only for the hub.
- [ ] Update R4's recorded limitation, D9's note and the epic's Known limitations bullet to say the limitation is gone.

## Verification

Crate-scoped clippy/nextest for `quarto-core` and `quarto`; the wasm compile check; a typst fixture with a raw HTML table through the hub request and the host's parity net once H3 exists.

## Exit

A typst request produced in the hub carries inlined table styling equal to native's.

## Close-out

- [ ] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [ ] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [ ] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: _none yet_
- Last commit; tasks ticked: _none_
- State and gotchas: _none_
- Next step: _the first unticked task_

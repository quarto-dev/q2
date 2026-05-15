# Fix pipe-table × caption-start collision (bd-expy, issue #206)

## Overview

Pipe-table rows ending immediately before a fenced-div close `:::` produce
a parse error: the parser shifts the first `:` of `:::` as the start of a
`caption` rule (the only production matching `:` after `_pipe_table_newline`),
then errors on the second `:`. See `claude-notes/issue-reports/206/triage.md`
for the full triage record.

**Fix approach.** Replace the literal `":"` in the `caption` grammar rule with
an external scanner token `_caption_start`. The scanner emits it only when
`:` is followed by an inline-whitespace character (space, tab, newline, EOF) —
specifically NOT when followed by another `:`. This kills the ambiguity at
tokenization time: `:::` no longer matches caption start, so the parser
never commits to caption and the table can close cleanly, letting the
surrounding context (fenced div or top level) close as well.

Why this approach over alternatives:
- **Greedy `token(seq(':', /[ \t\n]/))` in grammar.js.** Tempting but it
  would eat the whitespace too, change AST node boundaries, and tangle with
  the soft-line-break machinery. Cleaner to keep `:` and whitespace as
  separate tokens and discriminate at scanner time.
- **Refuse `_pipe_table_line_ending` when next line starts with `:::`.**
  Too broad — affects every block successor after a table, not just `:::`,
  and would not help the symmetric case of a bare `:::` after a table with
  no fenced-div context.
- **Surgical scanner change in `parse_fenced_div_marker`** for `level == 1`
  is the cleanest: the scanner already enters `case ':'` at line-start, so
  we add the caption-start emission there without affecting any other
  parsing path.

## Test plan (TDD — write FIRST, verify failing)

### Tree-sitter corpus tests (run via `tree-sitter test` in `tree-sitter-markdown/`)

- [ ] `pipe-table-then-fenced-div-close.txt` — `::: foo / table / :::` parses into a `pandoc_div` containing a `pipe_table` (the issue's repro)
- [ ] `pipe-table-then-bare-triple-colon.txt` — `table / :::` parses (the bare case — also fails currently)
- [ ] `pipe-table-then-caption-still-works.txt` — `table / : caption text` continues to parse as `pipe_table` with `caption` child (regression guard for the fix)
- [ ] `caption-at-top-level-still-works.txt` — `: free-floating caption` at top level continues to parse (regression guard)

### Pampa-level round-trip test

- [ ] Native-output assertion on `repro.qmd` exact text produces `[Div([Table...])]`
  - Located in: pick the closest existing test file for parser round-trips. Likely `crates/pampa/tests/...`. Decide once we look at the existing test shape.

### Verification commands (TDD step 2 — confirm tests fail before fix)

```bash
cd crates/tree-sitter-qmd/tree-sitter-markdown
tree-sitter generate
tree-sitter test                                 # new caption×table tests fail; existing pass
cd ../../..
cargo nextest run -p pampa <new test name>      # round-trip test fails
```

## Implementation

### Phase 1 — write failing tests

- [x] T1.1 Add `tree-sitter-markdown/test/corpus/` test file(s) for the four scenarios above — appended two tests to `pipe_table.txt`: the issue's repro (fenced div + table + `:::`) and a regression check for `: caption` after a row. The existing "soft-break caption" test at line 383 already covers the basic `: caption` regression, so I didn't duplicate it.
- [ ] T1.2 Add `pampa` integration test for the repro fixture (use `claude-notes/issue-reports/206/repro.qmd` as the input, expected native string)
- [x] T1.3 Run `tree-sitter test` — confirmed the new test fails (ERROR node) with the expected shape, existing tests pass

### Phase 2 — implement the fix

- [x] T2.1 Added `CAPTION_START` to the externals enum in `scanner.c` (at the END of the enum, not in the middle — keeps preceding token IDs stable)
- [x] T2.2 Added `"CAPTION_START"` to the token_names debug array
- [x] T2.3 Added `$._caption_start` to the externals array in `grammar.js`
- [x] T2.4 Added the emission in `parse_fenced_div_marker`'s `level < 3` branch
- [x] T2.5 Replaced `":"` with `$._caption_start` in the `caption` rule body
- [x] T2.6 `tree-sitter generate` succeeded
- [x] T2.7 (n/a — `cargo build` regenerates the C library via build.rs)
- [x] T2.8 `tree-sitter test` — 485/485 pass (was 484/485 pre-fix with the new failing test)
- [x] **T2.X (added during work):** Discovered that the caption disambiguation alone left `:::` absorbed as a pipe_table_row (1 cell, 3 × pandoc_str). Added a second scanner change in the PIPE_TABLE_LINE_ENDING dispatch (line ~2350): peek-without-mark_end for `:::` followed by inline-whitespace / newline / EOF, and route to `LINE_ENDING` instead of `PIPE_TABLE_LINE_ENDING` when seen. This terminates the table so the next scan-call can handle `:::` as `FENCED_DIV_END` (or surface a clean error for bare `:::` with no open div).
- [ ] T2.9 `cargo nextest run -p pampa` — pampa tests pass
- [x] T2.10 Repro end-to-end:
  - Input: `::: foo / | | | / |:-:|:-:| / | a | b | / :::`
  - Output: `[ Div ( "" , ["foo"] , [] ) [Table ...] ]` — matches expected
  - Bonus: `table / ::: foo / bar / :::` (new div opener after table, no blank line) also parses now as `[Table, Div [Para [Str "bar"]]]`
  - Bare `:::` after table with no enclosing div → error on the `:::` line (was: error too, but at the second `:`). Acceptable — unmatched div opener is a real error.
  - Caption regression (`: caption text`) → still parses as `Caption`.

### Phase 3 — full verification

- [x] T3.1 `cargo nextest run --workspace` — 8942 tests passed, 195 skipped, 0 failures
- [x] T3.2 `cargo xtask verify --skip-hub-build --skip-hub-tests` failed only on the trace-viewer build step (`tsc: command not found`), which is environmental (`node_modules` not installed in this worktree). All preceding Rust legs were green.
- [x] T3.X `cargo check --workspace` — clean
- [x] T3.X `cargo xtask lint` — clean
- [ ] T3.3 `cargo xtask verify` (full hub-client/WASM build). **Blocked on env** — `npm install` not run from repo root, so `vitest` / `tsc` are missing for trace-viewer and hub-client. Hub-client tests not exercised. Pre-existing on this worktree, not introduced by this fix. Recommend running `npm install` from repo root before pushing.
- [x] T3.4 WASM `cargo check` from `crates/wasm-qmd-parser` errored with a workspace-boundary issue (`current package believes it's in a workspace when it's not`) — pre-existing structural issue with running `cargo` directly inside a worktree on the WASM crate, not caused by this fix. The WASM build picks up the regenerated `parser.c` via `tree-sitter-qmd`'s `build.rs`, which is the same path as the native build; no WASM-specific changes were made.

### Phase 4 — real-world verification

- [x] T4.1 Fetched `quarto-dev/quarto-web` (main branch) `docs/websites/website-navigation.qmd` (~34KB). Section L155-L159 is exactly the bug pattern (`::: column-screen-inset-shaded` opening a div, a 2-row pipe table inside, then `:::` directly closing it).
- [x] T4.2 End-to-end verification record:
  - Invocation: `cargo run --bin pampa -- < /tmp/website-navigation.qmd 2>&1 | grep -ci "Error: Parse error"`
  - Result: **0** parse errors on the whole file
  - Also verified the bug repro: `cargo run --bin pampa -- < claude-notes/issue-reports/206/repro.qmd` → `[ Div ( "" , ["foo"] , [] ) [Table ...] ]`
  - Bonus regressions verified:
    - Nested 5-colon outer / 3-colon inner div with pipe table inside: parses correctly into `[ Div ["outer"] [Div ["inner"] [Table...], Para [...]] ]`
    - Table immediately followed by `::: foo / bar / :::` (no blank line): parses correctly into `[Table, Div ["foo"] [Para [Str "bar"]]]`
    - Single `:` caption after a row still produces `Caption` (regression guard for caption parsing).

### Phase 5 — commit + housekeeping

- [ ] T5.1 Commit on the `issue-206` worktree branch with a message referencing bd-expy and issue #206. Single logical commit covering grammar + scanner + regenerated parser.c + tests.
- [ ] T5.2 `br update bd-expy --status in_progress` then on completion `br close bd-expy --reason "Fixed via issue-206 branch, PR #..."` (the close happens after the PR is merged; for now just update status when the fix lands locally).
- [ ] T5.3 Sync beads JSONL on main and commit (per CLAUDE.md beads workflow).
- [ ] T5.4 Update the issue-206 triage doc with the resolution note (link to the fix commit).

## Risk / open questions

- **parser.c is large and regenerated.** The commit will include a big regenerated `parser.c` diff. That's expected — same pattern as past tree-sitter changes in this repo.
- **Snapshot tests.** Some snapshot tests may need updating if any existing fixture happens to be a parse-error-recovery case that the fix now parses cleanly. Per CLAUDE.md snapshot-test rules, count + summarize any changes in the commit message.
- **WASM/hub-client.** wasm-qmd-parser uses pampa via cargo, so it picks up parser.c via build.rs. No separate change should be needed, but verify in T3.3.
- **`level == 0` defensive case.** The fenced-div-marker function's while-loop is entered only after `case ':'` in the caller, so `level >= 1` is guaranteed; no defensive `level == 0` branch needed.

## Cross-references

- Beads: bd-expy
- GitHub: https://github.com/quarto-dev/q2/issues/206
- Triage: `claude-notes/issue-reports/206/triage.md`
- Grammar: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
- Scanner: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
- CLAUDE.md TDD rule (test FIRST, never implement before verifying test fails)

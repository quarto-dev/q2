# SQL chunk options under knitr strict mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `knitr_replay_equals_live` pass under knitr 1.52 by testing the `--|` option syntax that knitr and q2 both read in a `sql` chunk, and pin that a `#|` `sql` chunk behaves the same in recorded and live runs on any knitr version.

**Architecture:** Test and comment changes only. No product code changes. `insert_unit_ids` keeps its `#|` fallback, which still serves knitr older than 1.51.

**Tech Stack:** Rust (quarto-core integration tests), R/knitr through the rig project library.

**Spec:** this document (Design section). Strand bd-lnu81d4a, GH #1345.

## Design

### Why

The test `a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live` fails in its **live** run, which inserts no `quarto-unit-id` line. Nothing in q2's id-line code is involved.

What comes from knitr (verified against the locked knitr 1.52 and xfun 0.62):

- `knitr:::partition_chunk` calls `xfun::divide_chunk(engine, code, strict = is_quarto())`. Options are read under the engine's prefix from `xfun:::comment_chars` (`sql` uses `--`).
- In strict mode, a `#|` first line in a chunk whose engine prefix is not `#` stops with `The chunk options should start with '--| ' instead of '#| '.` Outside strict mode, knitr falls back to reading `#|` lines only.
- `is_quarto()` is true when `opts_knit$get("quarto.version")` is set, or when the input is `.qmd`.
- Strict mode arrived in knitr 1.51 (yihui/knitr#2225, merged 2025-04-03).

What comes from q2:

- `resources/rmd/execute.R` sets `quarto.version = 1` (as Quarto 1 does), so knitr runs in strict mode.
- q2's own option scanner (`cell_options::comment_syntax_for`, `partition_cell_options`) reads only the language prefix. It never read `#|` lines in a `sql` chunk.
- `insert_unit_ids` writes the id line in the language prefix, and uses `#|` only when the author's first option line is `#|`. That fallback keeps the options together on knitr older than 1.51.

Probe with knitr 1.52, `knit()` directly (Quarto mode is `opts_knit$set(quarto.version = 1)`):

| Chunk option lines | plain | Quarto mode |
|---|---|---|
| `#| connection` | OK | strict error |
| `--| connection` | OK | OK |
| `--| quarto-unit-id` then `--| connection` | OK | OK |
| `#| quarto-unit-id` then `--| connection` | `connection` missing | strict error |
| `#| quarto-unit-id` then `#| connection` | not run | not run; strict error by source (first line is `#|`) |

### Approaches

- **A (picked):** fix the fixture to `--|`, correct the stale comments, add a version-independent parity test for `#|`. No product change. `--|` is the syntax q2 supports (maintainer comment on GH #1345).
- B: also drop the `#|` fallback in `insert_unit_ids`. Rejected: breaks recorded runs on knitr 1.44 to 1.50 unless q2 declares a knitr 1.51 floor.
- C: a q2 diagnostic for `#|` lines in a non-`#` chunk. Deferred to a follow-up strand.

### Non-goals

- Rewriting `#|` to `--|` before knitr sees it.
- Declaring a knitr version floor.
- Editing dated plan logs that mention the old test name (`2026-11-23-ci-engine-test-env-r-jupyter.md` and older).

## Global Constraints

- Never skip, `#[ignore]` or cfg-gate a test to make it pass.
- Do not run `cargo build --workspace`, `cargo nextest run --workspace`, `cargo xtask verify` or `npm run build:all` on this Windows machine. Focused runs only.
- R for tests comes from the rig project: `rig proj sync --locked --group fixtures` (already synced), then put `.rvenv/bin` first on PATH and unset `QUARTO_R` (`claude-notes/instructions/engine-test-env.md`).
- Integration tests stay in `crates/quarto-core/tests/integration/knitr_replay_equals_live.rs` (no new top-level `tests/*.rs`).
- Comments describe the code as it is. History goes in the commit message.
- Commit messages: no em-dashes, state only what was checked. Never push.

## Review Focus

- Under knitr older than 1.51 a `#|` `sql` chunk succeeds in both runs. The parity test must then compare outputs, not just pass. Covered by the `(Ok, Ok)` arm in Task 2, which the locked knitr 1.52 does not exercise. On that side, two identical failures for another reason must not pass: the `(Err, Err)` arm requires the strict-mode message.
- A test that returns early when DBI or RSQLite is missing passes silently. Each run in this plan must show that R executed (Task 1 Step 2 checks the RED message, Task 1 Step 4 the `knitsql-table` needle).
- The error messages of the two runs could differ only in temp paths. Task 2 Step 2 shows the real messages before the assertion is trusted.

---

### Task 1: Test the `--|` syntax in the SQL fixture

**Files:**

- Modify: `crates/quarto-core/tests/integration/knitr_replay_equals_live.rs:280-297`

**Interfaces:**

- Consumes: `assert_recorded_run_equals_live(doc, extra_files, must_contain)`, `r_has(packages)` (same file).
- Produces: test `a_sql_chunk_with_dash_pipe_options_records_as_it_runs_live`.

- [x] **Step 1: Confirm RED on the current fixture**

PowerShell, from the worktree root:

```powershell
$env:PATH = "$PWD\.rvenv\bin;$env:PATH"; Remove-Item Env:QUARTO_R -ErrorAction SilentlyContinue
cargo nextest run -p quarto-core -E 'test(a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live)'
```

Expected: FAIL, panic at `.expect("live run")` with `The chunk options should start with '--| ' instead of '#| '.`

- [x] **Step 2: Change the fixture and rename the test**

Rename to `a_sql_chunk_with_dash_pipe_options_records_as_it_runs_live`. In the `sql` chunk, replace `#| connection: con` with `--| connection: con`. Keep the setup `r` chunk (`#| include: false` is correct there) and the `knitsql-table` needle. Replace the doc comment with one that states what the test checks: a `sql` chunk with `--|` options records as it runs live, and the id line `insert_unit_ids` adds uses the same `--|` prefix. Drop the claim that knitr reads `#|` in a `sql` chunk.

- [x] **Step 3: Run to GREEN**

```powershell
cargo nextest run -p quarto-core -E 'test(a_sql_chunk_with_dash_pipe_options_records_as_it_runs_live)'
```

Expected: 1 passed.

- [x] **Step 4: Prove R ran**

Temporarily change the needle `knitsql-table` to `knitsql-table-NOPE`, rerun Step 3, expect the `the live run lacks` assertion, then restore the needle. A test that returns early on missing packages cannot fail this way.

- [x] **Step 5: Commit**

```bash
git add crates/quarto-core/tests/integration/knitr_replay_equals_live.rs
git commit -F <msgfile>
```

Message subject: `test: write SQL chunk options with --| in the knitr replay test (bd-lnu81d4a)`. Body: knitr 1.51 and later refuse `#|` in a `sql` chunk when `quarto.version` is set, which q2's execute.R does; the live run failed before any unit-id line was involved; `--|` is the syntax q2's scanner reads. Refs GH #1345.

Committed as `55ce8b6e0`.

### Task 2: Pin `#|` parity between live and recorded runs, correct the fallback comments

**Files:**

- Modify: `crates/quarto-core/tests/integration/knitr_replay_equals_live.rs` (helper extraction near line 140, new test after the Task 1 test)
- Modify: `crates/quarto-core/src/engine/knitr/chunk_records.rs:91-93` (comment), `:445-448` (test doc comment)

**Interfaces:**

- Consumes: `context(doc, dir, want_record)`, `classify`, `normalize`, `r_has` (same file); `quarto_core::engine::ExecutionError` (re-exported in `engine/mod.rs:215`, derives `Debug` + `thiserror::Error`).
- Produces: `fn execute_live_and_recorded(doc: &str, extra_files: &[(&str, &str)]) -> (Result<ExecuteResult, ExecutionError>, Result<ExecuteResult, ExecutionError>, tempfile::TempDir, tempfile::TempDir)` returning (live, recorded, live\_dir, rec\_dir); test `a_sql_chunk_with_hash_pipe_options_fails_or_renders_alike_live_and_recorded`.

This is a characterization test, not RED/GREEN: it pins current behavior that is already correct on every knitr version.

- [x] **Step 1: Extract the helper and add the parity test**

Move the directory setup and the two `execute` calls of `assert_recorded_run_equals_live` into `execute_live_and_recorded`. `assert_recorded_run_equals_live` then calls it and keeps its `.expect("live run")` and `.expect("recorded run")` (Task 1's test still panics there if knitr refuses).

New test, gated like Task 1's on `r_has(&["DBI", "RSQLite"])`, with the original `#|` fixture (setup `r` chunk plus `sql` chunk with `#| connection: con`). Match on the pair:

```rust
match (live, recorded) {
    (Ok(live), Ok(recorded)) => {
        // knitr below 1.51 reads #| here: both runs render the table alike.
        assert!(live.markdown.contains("knitsql-table"), "{}", live.markdown);
        let class = classify(
            &normalize(&live.markdown, live_dir.path()),
            &normalize(&recorded.markdown, rec_dir.path()),
        );
        assert!(class <= Class::SyntaxLevel, "{class:?}");
    }
    // knitr 1.51 and later refuse #| in a sql chunk in Quarto mode: the id line adds no other failure.
    (Err(live), Err(recorded)) => {
        let live = live.to_string();
        assert!(live.contains("should start with '--| '"), "{live}");
        assert_eq!(live, recorded.to_string());
    }
    (live, recorded) => panic!("live and recorded runs disagree:\nlive: {live:?}\nrecorded: {recorded:?}"),
}
```

The doc comment states the claim: a `#|` `sql` chunk fails or renders the same way in both runs, whichever knitr is installed.

- [x] **Step 2: Run both tests and read the error messages**

```powershell
cargo nextest run -p quarto-core --no-capture -E 'test(a_sql_chunk_with_hash_pipe_options_fails_or_renders_alike_live_and_recorded) | test(a_sql_chunk_with_dash_pipe_options_records_as_it_runs_live)'
```

Expected: 2 passed. Under knitr 1.52 the parity test takes the `(Err, Err)` arm, and the strict-message assertion proves it failed for the expected reason rather than any shared failure. If the two messages differ only by a temp path, stop and report rather than loosening the assertion.

Prove the `(Err, Err)` arm ran, as in Task 1 Step 4: temporarily change the expected text to `should start with '--| 'NOPE`, rerun, expect the assertion to fail with the strict-mode message shown, then restore it. A test that returns early on missing packages, or takes another arm, cannot fail this way.

The `(Ok, Ok)` arm is not exercised by the locked library (knitr 1.52), locally or in CI. Running it needs knitr below 1.51 in a separate library, which is outside the lock and needs Chris's approval; say so in the commit message.

- [x] **Step 3: Correct the comments in `chunk_records.rs`**

At `:91-93`, replace "It accepts `#|` in any chunk" with what holds: knitr below 1.51, or outside Quarto mode, reads `#|` in any chunk, so an author's `#|` lines in a `sql` chunk must not be split from the options by a `--|` line; knitr 1.51 and later refuse those lines under Quarto, with or without the id line. Same correction in the doc comment at `:445-448` ("knitr accepts them" becomes "knitr below 1.51 accepts them"). Comment-only; the code is unchanged.

- [x] **Step 4: Unit tests and clippy**

```powershell
cargo nextest run -p quarto-core -E 'test(chunk_records::) | test(knitr_replay_equals_live::)'
cargo clippy -p quarto-core --all-targets -- -D warnings
```

Expected: all pass, no warnings. The `knitr_replay_equals_live` tests other than the two SQL ones run R too; report any failure they show without fixing it here.

- [x] **Step 5: Commit**

```bash
git add crates/quarto-core/tests/integration/knitr_replay_equals_live.rs crates/quarto-core/src/engine/knitr/chunk_records.rs
git commit -F <msgfile>
```

Subject: `test: pin that a #| SQL chunk fails or renders alike live and recorded (bd-lnu81d4a)`. Body: the `#|` fallback in `insert_unit_ids` still serves knitr below 1.51; the test holds on either side of 1.51 and says which arm ran; comments corrected to the 1.51 behavior. Note that this is a characterization test.

Committed as `d67e93a44`.

### Task 3: Bookkeeping

- [x] File the follow-up strand for approach C (q2 diagnostic for `#|` option lines in a chunk whose prefix is not `#`), `discovered-from` bd-lnu81d4a. Filed as bd-46dq7sgo.
- [x] `braid comment bd-lnu81d4a` with the two commit hashes and the knitr/q2 findings above. Close the strand only after Chris reviews.
- [x] Check the boxes in this plan, then check it renders: `cargo run --bin q2 -- render --strict claude-notes/plans/2026-11-25-sql-chunk-dash-pipe-options.md` (no warnings). Commit the plan on its own, after the steps above, since it records the Task 1 and Task 2 hashes.

## Dev workflow

- Branch `sql-chunk-options` in this worktree, created from the PR #1292 head (`feature/bd-9kbjjeb6-windows-ci-r-and-jupyter`, `d1d417481`). Only CI with R exercises this test, so the fix lands on #1292's branch or in a PR based on it. Chris decides which. No push, no PR, no CI dispatch without approval.
- `.gitignore` shows as modified after `rig proj sync`: line endings only, no content change. Do not stage it.
- Before pushing (when approved): the push refspec rules in `.claude/rules/worktrees.md` § Pushing for PR.

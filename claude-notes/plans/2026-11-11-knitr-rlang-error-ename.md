---
title: 'knitr capture reports rlang_error instead of simpleError (bd-54odm3rz)'
date: 2026-11-11
description: 'knitr 1.51+ entraces every chunk error through rlang, so capture.R records ename rlang_error; decide which class ename should carry, fix capture.R, and stop the local verify leg from failing on it.'
status: in-progress  # design agreed 2026-11-11; implementation on braid/bd-54odm3rz-knitr-rlang-error-ename
braid:
  strand: bd-54odm3rz
---

**Checkout:** main checkout, branch `main` @ `3743b5d6a`
**Pre-flight verify (2026-11-11):** 8813 passed, 1 failed (`capture_classifies_pieces`, this strand); nextest fail-fast stopped the run at 8814 of 19251.
**Implementation:** branch `braid/bd-54odm3rz-knitr-rlang-error-ename`. Follow-up for the test cadence: bd-qbkwun87.

## Triage verdict

**Ready to design.** The root cause is known and reproduces in a few lines of R; what remains
is a choice about what `ename` should mean, plus whether CI should start running the R tests.

## Issue context

bd-54odm3rz (bug, P2, open, filed 2026-11-07 by Carlos during the bd-mumobar8 pre-flight
verify). `cargo nextest run -p quarto-core capture_classifies_pieces` fails on macOS:
`crates/quarto-core/src/engine/knitr/capture_tests.rs:170` expects `ename == "simpleError"`
and gets `"rlang_error"`. The 2026-11-10 comment reproduces it again (knitr 1.52, evaluate
1.0.5, rlang 1.3.0) and notes that the failure aborts the workspace leg of
`cargo xtask verify` before about 10k tests run, because nextest stops at the first failure.

## Dependency graph

No children; two outgoing edges.

- **discovered-from bd-mumobar8** (closed; citeproc ordering, PR #1122). Unrelated subject.
  This strand surfaced only because that session ran the pre-flight verify. No context to
  inherit beyond "this blocks verify on a dev machine".
- **related bd-qwscag8b** (open, P3, filed 2026-11-09 by shikokuchuo). The same assertion,
  filed independently: **a duplicate**. Its comment adds a second R-version skew failure,
  `knitr_replay_equals_live::a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live`
  (newer knitr refuses `#|` options in SQL chunks). One claim in it is wrong: it says CI
  is green because CI pins an older knitr. CI installs no R at all (see below).

The project memory note `project_local_knitr_rlang_error` describes the workaround
(`--no-fail-fast`, then `verify --skip-rust-tests`). That note should be retired once this
is fixed.

## What the code looks like today

Still the same shape as when filed.

- `crates/quarto-core/src/engine/knitr/resources/rmd/capture.R:146-155` turns an error
  into a piece with `ename = class(p)[[1]]`, `evalue = conditionMessage(p)`,
  `text = as.character(p)`.
- `capture_tests.rs:139` (`capture_classifies_pieces`) asserts the `simpleError` name.
  `pieces_map.rs` and `types.rs` only carry `ename` through; their unit tests use
  hand-written JSON with `"simpleError"` and do not run R. No Rust code branches on
  `ename`.

### Root cause (reproduced at HEAD)

knitr 1.51 (yihui/knitr#2388, "Improve error traceback when rlang is available and
evaluate > 1.0.3") adds `rlang_entrace_handler` to the calling handlers that
`knitr:::knit_handlers` passes to evaluate. Its body is `function(e) rlang::entrace(e)`.
`rlang::entrace()` re-signals **every** error as a new condition of class
`c("rlang_error", "error", "condition")` and keeps the original condition in `$error`.
So with knitr >= 1.51 and rlang installed:

| `stop(...)` argument | `class(p)` seen by capture.R | `class(p$error)` |
|---|---|---|
| `"boom"` | `rlang_error error condition` | `simpleError error condition` |
| custom `myError` condition | `rlang_error error condition` | `myError error condition` |

and `as.character(p)` changes from `Error in ...: boom` to the rlang format, which
puts the message on its own line after `!`. The original description guessed rmarkdown; it is knitr.
Plain `evaluate::evaluate()` does not entrace, which is why the bare-evaluate probe in the
description still returned `simpleError`.

Repro: `claude-notes/plans/knitr-rlang-error-ename-investigation/repro.R`.

### What Quarto 1 does (checked 2026-11-11)

Quarto 1 (`external-sources/quarto-cli` @ `ac310a424`) has no structured error record and no
`ename`. `src/resources/rmd/patch.R` sends an error through `knitr:::sew.error`, and
`hooks.R:607` renders it with `delegating_output_hook("error", c("error"))`: the user sees
whatever text knitr produced. Quarto 1 sets no rlang options and does not unwrap or
re-format conditions. So with knitr >= 1.51, Quarto 1 already shows rlang-format errors.
Rendering a two-chunk `gfm` document with the local Quarto 1 dev build (99.9.9) gives:

```
Error:
! boom

Error in `f()`:
! inner
```

Consequence for this plan: on the user-visible `text`, Q2 matches Quarto 1 if it keeps
knitr's text. `ename` is new in Q2 (it exists for the structured execution records), so
Quarto 1 does not constrain what it should be.

### Why CI never sees it

`.github/workflows/test-suite.yml` installs no R. `r_available()` in `capture_tests.rs`
returns false, prints `SKIP`, and the test passes in about 10 ms. Every knitr test
that needs R is a no-op in CI; these tests run only on developer machines, so they drift
with whatever R packages each person has installed.

### Inventory: R tests that run locally but not in CI (2026-11-11)

Measured, not grepped: a logging `Rscript`/`R` shim on `PATH`
(`knitr-rlang-error-ename-investigation/rscript-logging-shim.sh`) recorded the nextest test
name of every process that started R during `cargo nextest run --workspace --no-fail-fast`.
Full list: `knitr-rlang-error-ename-investigation/r-tests-2026-11-11.txt`.

- **137 distinct tests start R** (out of 19251). 39 are `quarto-core` unit tests (all in
  `engine::knitr`), 93 are `quarto-core` integration tests, 4 are `quarto` integration
  tests, 1 is in `quarto-test`. The count includes tests that only probe for R or a package
  and then skip (for example the SQL test, which needs DBI and RSQLite).
- They take **about 480 test-seconds** locally (wall time is lower; nextest runs them in
  parallel). The largest: `smoke_all` 123 s (one test over the smoke-all corpus, about 25
  documents of which use R), the two `knitr_corpus_control` tests at 70 s each,
  `dashboard_goldens` 24 s.
- CI: the latest green `test-suite.yml` run on `main` (43086661780) reports 19249 tests
  passed on ubuntu and macOS, with no R setup step. All of these gates need knitr and
  rmarkdown, which no runner image ships, so they skip there.

### Exposure to future knitr changes

This failure is one instance of a general coupling. Three layers:

1. **Test oracles are live knitr.** About 137 tests compare Q2 against whatever knitr,
   evaluate and rlang are installed locally. A CRAN release changes the expected output
   with no change to Q2. Because `cargo xtask verify` runs nextest fail-fast, one such test
   stops verify for every agent on that machine, and it looks like a branch regression.
   That happened twice in four days here (bd-54odm3rz, bd-qwscag8b), plus a memory-note
   workaround.
2. **Product code uses knitr private API.** The vendored R resources call 45 distinct
   `knitr:::` functions, mostly the `sew.*`/`wrap.*` family in `patch.R`, plus
   `parse_block`, `parse_params`, `opts_chunk_attr`. These can change without a NEWS
   entry. Quarto 1 shares `patch.R`/`hooks.R`, so it has the same exposure.
3. **Structured records reconstruct knitr formatting.** `capture.R` re-derives some text
   ("as knitr formats it before comment_out (sew.warning + msg_wrap)") and names
   conditions. Quarto 1 only passes knitr's text through, so Q2 is more coupled than
   Quarto 1 here. The `ename` fix in this plan removes one such coupling; the warning
   formatting is another.

What CI never running R implies: drift is caught only on developer machines, and only
when someone upgrades packages. Possible mitigations (for discussion, not in this strand):

- Default test runs use **recorded** knitr output (the execution-record / replay
  machinery), and live-R tests become opt-in (an env var or a nextest profile), in the
  same spirit as `cargo xtask capture-pandoc-recordings`.
- Each recording stores the knitr/evaluate/rlang/rmarkdown versions it came from, and a
  periodic local job (weekly, or on demand) re-runs the live tests against current CRAN and
  reports differences as a drift report, not as a failing verify.
- A cheap version-check test that warns when the installed packages differ from the
  versions the recordings were made with, so a failure points at the cause.

## Proposed phases (draft)

- Phase 0 (done): Tests first. Extend `capture_classifies_pieces` (or add a sibling) with a custom
  condition class, so the test pins the class the author raised, not just `simpleError`.
  Confirm it fails locally for the reason above.
- Phase 1 (done; `call` too, see below): capture.R. Unwrap the entraced condition before naming it, for example
  `orig <- if (inherits(p, "rlang_error") && inherits(p$error, "error")) p$error else p`,
  then take `ename` (and possibly `evalue`) from `orig`. Decide what `text` carries
  (question 2).
- Phase 2: done 2026-11-11. bd-qwscag8b closed as a duplicate; the SQL `#|` skew is
  bd-lnu81d4a.
- Phase 3: no R in CI (decided 2026-11-11). Whether to move live-R tests to an opt-in or
  periodic local cadence is a separate question (see "Exposure to future knitr changes").
- Implementation note: entrace also replaces the condition call (a custom condition
  built with `call = sys.call()` lost it entirely), so `ename`, `evalue` and `call` all
  come from the unwrapped condition; `text` stays knitr's. `rlang::abort()` errors have
  no `$error` and keep their own class (covered by the new test).
- Phase 4: Retire the `project_local_knitr_rlang_error` memory note.

## Open design questions for the user

1. **What should `ename` name?** Options: (a) the class the author raised, unwrapped from
   rlang's `$error` (`simpleError`, `myError`), so the name does not depend on the knitr
   version; (b) the first class as-is (`rlang_error`), relaxing the test; (c) the test
   accepts either. Resolved 2026-11-11: (a).
2. **What should `text` carry?** Resolved 2026-11-11: keep knitr's own text
   (`as.character(p)`), which matches what Quarto 1 shows.
3. **Do we want R in CI?** Resolved 2026-11-11: no, CI never runs R. Open follow-up:
   should live-R tests also leave the default local run (opt-in or periodic), using
   recorded executions instead? Not part of this strand.
4. **Duplicate handling.** Resolved 2026-11-11: yes. bd-qwscag8b closed, SQL failure
   split into bd-lnu81d4a.

## Risks / tradeoffs (draft)

- If any recorded execution artifacts (freeze or replay records) have already stored
  `rlang_error` from a local run, changing the name makes old and new records differ.
  No Rust code reads `ename`, so the impact is cosmetic, but replay-equality tests could
  notice.
- `p$error` is an rlang implementation detail. A guard (`inherits(p$error, "condition")`)
  plus a fallback to `class(p)[[1]]` keeps capture.R working if rlang changes it.
- Without an R leg in CI, any fix here is verified only on the machine that makes it.

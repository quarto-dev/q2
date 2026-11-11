---
title: 'knitr capture reports rlang_error instead of simpleError (bd-54odm3rz)'
date: 2026-11-11
description: 'knitr 1.51+ entraces every chunk error through rlang, so capture.R records ename rlang_error; decide which class ename should carry, fix capture.R, and stop the local verify leg from failing on it.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-54odm3rz
---

**Checkout:** main checkout, branch `main` @ `3743b5d6a`
**Pre-flight verify (2026-11-11):** 8813 passed, 1 failed (`capture_classifies_pieces`, this strand); nextest fail-fast stopped the run at 8814 of 19251.
**Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

- Phase 0: Tests first. Extend `capture_classifies_pieces` (or add a sibling) with a custom
  condition class, so the test pins the class the author raised, not just `simpleError`.
  Confirm it fails locally for the reason above.
- Phase 1: capture.R. Unwrap the entraced condition before naming it, for example
  `orig <- if (inherits(p, "rlang_error") && inherits(p$error, "error")) p$error else p`,
  then take `ename` (and possibly `evalue`) from `orig`. Decide what `text` carries
  (question 2).
- Phase 2: done 2026-11-11. bd-qwscag8b closed as a duplicate; the SQL `#|` skew is
  bd-lnu81d4a.
- Phase 3 (optional): CI. Add an R leg, or a nightly job, that installs a pinned knitr,
  rmarkdown, jsonlite, rlang, DBI and RSQLite, so these tests run somewhere.
- Phase 4: Retire the `project_local_knitr_rlang_error` memory note.

## Open design questions for the user

1. **What should `ename` name?** Options: (a) the class the author raised, unwrapped from
   rlang's `$error` (`simpleError`, `myError`), so the name does not depend on the knitr
   version; (b) the first class as-is (`rlang_error`), relaxing the test; (c) the test
   accepts either. Recommendation: (a), since a knitr upgrade should not change the
   records Q2 writes.
2. **What should `text` carry?** Keep knitr's own rendering (`as.character(p)`, now in
   rlang's format, which is what the user sees in the knitted output), or rebuild the base
   R form from the unwrapped condition? Recommendation: keep knitr's text, since `text` is
   "as knitr formats it" elsewhere in capture.R, the replay tests compare against live
   knitr output, and Quarto 1 shows exactly that text (see "What Quarto 1 does").
3. **Do we want R in CI?** The knitr capture and replay tests currently never run in CI.
   Should this strand add an R leg (cost: R + packages install time per run, and pinning
   versions to keep it stable), file a separate strand for it, or leave R tests as
   local-only?
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

---
title: 'Auto-file a GitHub issue for every test that fails on main'
date: 2026-11-23
description: 'A workflow_run-triggered filer that turns each red main run into one deduplicated GitHub issue per failing test (or per failed job when no test is identifiable), so repeated failures accumulate in one place instead of being re-run away as one-off flakes.'
status: in-progress
---

**Braid:** bd-red-main-issues-fs4r6cp0 (discovered from bd-classifier-perf-s7ay426r)
**Related:** bd-test-perf-tracking-5d8drwh0 (duration history: Phase 3 here is its first step)

## Why

We never merge to `main` without a green suite, so **every red run on `main` is a signal**:
a flake, an environment drift, a cross-PR interaction, or a regression that only
crosses a timeout under load. At the moment we re-run these and move on.

The motivating case (2026-11-23): `quarto-hub-mcp`'s `exit-drain.test.ts` failed on
main **8 times in 5 days**, on both OSes. Each failure looked like a one-off timeout.
It was actually a deterministic ~2x slowdown introduced by PR #1210
(fixed in #1293, bd-classifier-perf-s7ay426r). Had the failures piled up in one issue,
with durations attached, the pattern would have been obvious on day two.

### What a month of red main looks like (2026-10-24 → 2026-11-23)

`gh run list --branch main --status failure`: **64 failed runs**. By workflow:
Test Suite 28, Hub-Client E2E 18, TS Test Suite 10, Nightly 2, others 6.

A prototype log extractor (see *Extraction* below) found a failing test ID in
**52/64**. The other 12 were build breaks (unresolvable crate version, 3 runs),
infrastructure (Pages 502, apt mirror), release gates (Nightly), and one empty log.

Repeat offenders (count = runs in which the test failed):

| n | runner | test |
|---|---|---|
| 8 | vitest | hub-mcp `exit-drain.test.ts › create_project then immediate stdin EOF…` (fixed by #1293) |
| 6 | playwright | `[webkit] pandoc-pdf-viewer.harness.spec.ts › the preview pane…` |
| 6 | nextest | `quarto-core::integration jupyter_markdown_goldens::legacy_capture_payload_is_byte_identical_after_flip` |
| 5 | playwright | `[webkit] pandoc-warm.harness.spec.ts › warm path: timing › whole refresh…` |
| 5 | nextest | `quarto-core::integration jupyter_markdown_goldens::jupyter_markdown_matches_goldens` |
| 5 | vitest | hub-mcp `upload-local-file.test.ts` (two tests; fixed by #1293) |
| 3 | playwright | `[chromium] import-dialog.harness.spec.ts › the button is keyboard-reachable…` |

Some of these already have hand-filed braid strands (`bd-c4le84pn`, `bd-fxx9bzzl`,
`bd-mc451ftq`) or a GH issue (#1266, also pinned on #1210). Several do not. None
record *how often* the test fails, which is the information that separates
"flake, low priority" from "regression, fix now".

## Design

### Shape

```
.github/workflows/red-main.yml      workflow_run trigger, permissions, concurrency
scripts/red-main/file-issues.mjs    zero-dependency Node ESM: fetch → extract → dedupe → file
scripts/red-main/extract.mjs        pure log → failures parser (unit-tested)
scripts/red-main/extract.test.mjs   node --test, against fixture logs
scripts/red-main/fixtures/*.log     trimmed real logs (one per runner + fallbacks)
```

Node with no dependencies, because the runner already has it, no `npm ci` is needed,
and the parser stays unit-testable. `scripts/check-sw-precache.{mjs,test.mjs}` is
the precedent. All GitHub I/O goes through `gh` (preinstalled on runners), which
keeps the script small and makes dry runs from a laptop work out of the box.

### Trigger and safety

```yaml
on:
  workflow_run:
    workflows: ["Test Suite", "TS Test Suite", "Hub-Client E2E Tests"]
    types: [completed]
permissions: { issues: write, actions: read, contents: read }
jobs:
  file:
    if: >-
      github.event.workflow_run.conclusion == 'failure'
      && github.event.workflow_run.head_branch == 'main'
      && contains(fromJSON('["push","schedule","workflow_dispatch"]'), github.event.workflow_run.event)
      && github.event.workflow_run.head_repository.full_name == github.repository
```

- `workflow_run` runs the **default-branch** copy of the workflow with a write token.
  It never checks out or executes the triggering run's code, only reads its logs.
- `head_branch == 'main'` alone is **spoofable**: a fork PR from a branch named `main`
  matches it. The `event` and `head_repository` guards close that hole.
- Each re-run attempt triggers a new `workflow_run` event. The filer records the
  attempt number, so "failed on attempt 1, passed on attempt 2" still produces a
  comment. That is exactly the flake signal we want to keep.

### Extraction (phase 1: logs)

`gh run view <id> --log-failed`, with ANSI stripped, then per-line patterns:

| runner | pattern (after the `job\tstep\ttimestamp` prefix) |
|---|---|
| nextest | `FAIL [ 1.23s] (n/m) <crate::binary> <test::path>` |
| vitest | `FAIL  <file.test.ts> > <describe> > <name>` |
| vitest teardown | `EnvironmentTeardownError` / `Unhandled Errors` → key on the file named in "This error originated in" |
| playwright | `N) [<project>] › <spec>:L:C › <title path>` |

Prototype coverage on the month above: 52/64 runs. The OS is **not** part of the
key. The job name carries it, and the issue body tallies it, so a test failing on
ubuntu and on macOS lands in one issue with both platforms listed.

**Fallback (no test identified):** key on `workflow › job › failed step`. Build
breaks and infra failures still get an issue. They just aren't per-test.

**Mass-failure cap:** if one job reports more than 5 distinct failing tests (the
smoke-all runs reported 12+), file **one** issue for the job, listing the tests,
rather than N issues. A mass failure almost always has a single cause.

### Deduplication and lifecycle

- Key = `sha1(runner + normalized test id)`, truncated to 12 hex. It is embedded
  in the issue body as `<!-- red-main-key: … -->` and found with
  a single `gh issue list --label red-main --state all` (newest 300), matched locally.
  This avoids search, whose index lags just after an issue is created and would let
  two quick failures open duplicates.
- **Open issue exists** → add a comment with the run link, SHA, job/OS, attempt,
  duration if known, and the first ~30 lines of the failure.
- **Closed less than 30 days ago** → reopen and comment "recurred after close". A
  recurrence after a claimed fix is the most important case to surface.
- **Closed longer ago, or never filed** → open a new issue that links to any older
  one.
- Title: `[red main] <runner>: <short test id>`. Labels: `red-main`, `testing`.
- The body keeps a small table of occurrences (date, SHA, job, attempt), which the
  filer rewrites on every hit. The table is meant to be read at a glance; the
  comments hold the detail.

### Relationship to braid

Strands stay the place where work is planned. GH issues are only the intake.
The filer does **not** write to braid, because that would put the skein's bearer
token into Actions secrets. The human loop is the existing `/triage` skill: triage
a `red-main` issue → a braid strand (or link an existing flake strand like
`bd-c4le84pn`) → note the strand id in the issue. The filer leaves issues that
mention a `bd-` id alone apart from adding comments.

## Phases

1. **Parser + dry run (this strand).** `extract.mjs`, with fixture tests built from
   the 64 logs already collected. `file-issues.mjs --dry-run --run <id>` prints the
   issues it would open or update. Acceptance: replaying the last 30 days in dry
   run produces a sensible issue list. On the data above the prototype yields
   about 36 issues: 17 nextest, 8 playwright, 4 vitest, 1 capped mass failure
   (smoke-all), and 6 or more fallbacks. Exit-drain should get one issue with
   8 occurrences.
2. **Workflow, in dry-run mode.** Land `red-main.yml` with `DRY_RUN=1`, which
   writes the would-file list to the job summary. Watch a week of real red runs.
3. **Go live.** Flip `DRY_RUN` off and create the `red-main` label. Optionally do
   a one-time backfill of the last 30 days.
4. **Durations (feeds bd-test-perf-tracking-5d8drwh0).** Switch extraction from
   logs to structured reports: nextest JUnit (`[profile.ci.junit]`), vitest
   `--reporter=json`, playwright JSON, uploaded as artifacts. That brings exact
   IDs, retried-then-passed flakes, and per-test durations, so issue comments can
   say "took 44 s, 30-day median 28 s". It's out of scope for the first cut.
   Wiring reporters into ~30 separate test invocations is its own chunk of work.

## Decisions (2026-11-23)

- **Workflows:** only the test-running ones: Test Suite, TS Test Suite and
  Hub-Client E2E Tests. Nightly, deploy and publish are excluded; Nightly's red
  runs in the sample were release gates, not tests.
- **Hidden flakes** (non-gating legs, `--retry`-then-pass): not filed. We run the
  filer for a while first and see whether main gets greener.
- **Notifications:** none beyond the `red-main` label. The repo is watched
  closely already.
- **No concurrency group** on the filer job. GitHub keeps only one *pending* run
  per group, so a third red run arriving while one files and one waits would be
  cancelled and never filed. A duplicate issue needs the same test failing in two
  workflows within seconds of each other, and the script is idempotent per run
  attempt.

## Status

Phases 1 and 2 are implemented on `braid/bd-red-main-issues-fs4r6cp0-ci-auto-file-github`:

- `scripts/red-main/extract.mjs`, `file-issues.mjs`, and 31 `node --test` tests,
  with 5 trimmed real logs as fixtures.
- `.github/workflows/red-main.yml`, triggered by `workflow_run`. Automatic runs
  are dry runs that write the job summary. `workflow_dispatch` takes `run_id` or
  `since` plus a `dry_run` toggle. PRs that touch the filer run its unit tests.
- **Replay** of 2026-10-24 → 2026-11-23 (read-only, `--dry-run --since`):
  71 occurrences → **35 issues**. The top ones are exit-drain (8), `jupyter_markdown_goldens`
  legacy capture (6), webkit `pandoc-pdf-viewer` (6), webkit `pandoc-warm` timing (5)
  and `jupyter_markdown_matches_goldens` (5). Flaky-but-passed Playwright tests are
  correctly left out. One run had no log on GitHub, so it fell back to a job/step issue.

**Remaining:** merge, watch a week of dry-run summaries, then set `DRY_RUN` to
`'0'` for automatic runs (Phase 3). Optionally backfill with
`workflow_dispatch since=2026-10-24 dry_run=false`.

# Experiment: consolidate integration tests into single binary per crate

**Beads:** [bd-xvdop](../../.beads/issues.jsonl) — `br show bd-xvdop`
**Branch:** `beads/bd-xvdop-integration-test-consolidation` (off `main`)
**Status:** proposed (not yet started)

## Overview

Rust's default integration-test layout creates one test binary per
`tests/*.rs` file. Each binary is fully linked against the host crate
and all transitive dependencies. We have **164 integration test files
across 20 crates** and `target/debug/` on this machine is currently
**251 GB** while `target/release/` is only 2.7 GB — strongly
suggesting the per-file debug test binaries are the dominant bloat,
matching exactly the pattern the ark project diagnosed.

The ark project hit the same problem on Linux CI (out-of-disk
failures) and resolved it by moving `tests/*.rs` into
`tests/integration/*.rs` with a single `tests/integration/main.rs`
declaring each former file as a `pub mod`. Reported wins:

- Fresh `cargo clean` size: **8.1 GiB → 3.5 GiB** (~57% reduction)
- Test-suite compile time on macOS: **88s → 52s** (~40% faster)
- Linux CI runner footprint: **15 GB → ~2 GB**

This experiment measures the same change on Q2 from a macOS dev
machine. We cannot directly measure Linux/Windows CI from here, but
the ark numbers suggest the macOS delta will be a representative
proxy for the platform delta.

### Goals

- Establish a clean baseline for debug + release test-build footprint.
- Pilot the migration on `pampa` (57 files — the largest single signal).
- If the pilot delta justifies it, roll out to the other 12 multi-file
  crates and remeasure.
- Capture all numbers in a research note so the Linux/Windows CI win
  can be predicted before pushing.

### Non-goals

- Migrating crates that already have only one integration test file
  (no payoff — each is already 1 binary).
- WASM / hub-client changes (this is a Rust-only refactor).
- Changing test execution semantics — nextest handles single-binary
  test discovery fine.

## References

- ark PR: <https://github.com/posit-dev/ark/pull/1240>
- matklad post: <https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html>
- Cargo issue cited by matklad: <https://github.com/rust-lang/cargo/pull/5022#issuecomment-364691154>

## Crates in scope

13 crates have >1 integration test file (sorted by file count, since
the payoff scales with file count):

| Crate                  | tests/*.rs files |
| ---------------------- | ---------------: |
| pampa                  |               57 |
| quarto-core            |               33 |
| qmd-syntax-helper      |               20 |
| quarto-preview         |                7 |
| quarto-sass            |                7 |
| quarto                 |                7 |
| quarto-highlight       |                6 |
| comrak-to-pandoc       |                5 |
| quarto-yaml-validation |                5 |
| quarto-brand           |                4 |
| quarto-citeproc        |                2 |
| quarto-csl             |                2 |
| quarto-doctemplate     |                2 |

Out of scope (single-file integration test crates, no benefit):
`quarto-error-reporting`, `quarto-hub`, `quarto-lsp`,
`quarto-lsp-core`, `quarto-publish`, `quarto-trace`,
`wasm-qmd-parser`.

## Work Items

### Phase 0 — Setup

- [x] Create worktree
      `.worktrees/bd-xvdop-experiment-consolidate-integration-tests/`
      on branch `beads/bd-xvdop-experiment-consolidate-integration-tests`
      (off `main`); worktree starts with empty `target/`, so baseline
      measurements aren't polluted by the 259 GB in the main checkout
- [x] Write a measurement helper script `scripts/measure-test-build.sh`
      that:
  - times `cargo build --workspace --tests` (debug or release per arg)
  - records `target/<profile>/` size via `du -sh`
  - lists the largest 25 binaries under `target/<profile>/deps/`
  - prints a paste-able summary block
- [x] Create research note skeleton
      `claude-notes/research/2026-05-28-integration-test-bloat.md`
      and `claude-notes/research/measurements/` directory

### Phase 1 — Baseline measurement

- [x] `cargo clean` (no-op — fresh worktree)
- [x] `scripts/measure-test-build.sh debug` →
      **21 GB / 114 s / 220 deps executables / 10.5 GiB exec-bytes**
- [x] `cargo clean` (freed 22.3 GiB / 36 689 files)
- [x] `scripts/measure-test-build.sh release` →
      **11 GB / 133 s / 220 deps executables / 9.0 GiB exec-bytes**
- [x] Write baseline numbers into
      `claude-notes/research/2026-05-28-integration-test-bloat.md`

### Phase 2 — Pilot migration: pampa

- [x] Create `crates/pampa/tests/integration/main.rs` with `pub mod
      <name>;` lines for each of the 57 current `tests/*.rs` files
- [x] `git mv` each `tests/*.rs` → `tests/integration/<same-name>.rs`
- [x] Audit for collisions:
  - `test_location_health.rs` has one inline `mod tests {}` — verified
    inline only, no file-resolution risk
  - Zero `crate::` references in pampa tests (would change meaning
    in consolidated layout)
  - Exactly one `super::` reference in `test_location_health.rs`,
    inside the inline `mod tests {}` — parent semantics preserved
    (`super::*` still refers to the enclosing file)
  - No `include_str!` / `include_bytes!` (source-file-relative
    compile-time paths)
- [x] Adjacent data dirs (`fixtures/`, `snapshots/`, `*.qmd`) stay
      where they are — they're referenced by fixed paths from test
      code; moving the .rs files doesn't change `CARGO_MANIFEST_DIR`
- [x] **Discovered:** insta `set_snapshot_path("../snapshots/…")`
      in `test.rs` and `test_error_corpus.rs` is resolved relative
      to the test file's directory. After the move, all 3 occurrences
      needed an extra `../` to keep pointing at `crates/pampa/snapshots/`.
      Fixed in this pilot; cleaned up 3 stale `.snap.new` litter
      files generated by the first broken run.
- [x] `cargo nextest run -p pampa --test integration` → **941 passed,
      2 skipped, 0 failed**
- [ ] `cargo xtask verify --skip-hub-build` → expect green (in progress)

### Phase 3 — Pilot measurement

- [x] `cargo clean` between each measurement
- [x] `scripts/measure-test-build.sh debug` (pampa-pilot, first run):
      **18 GB / 173 s / 164 exes / 7.8 GiB exec-bytes**
- [x] `scripts/measure-test-build.sh release` (pampa-pilot, first run):
      **9.2 GB / 255 s / 164 exes / 7.2 GiB exec-bytes**
- [x] Controlled back-to-back **debug** re-measurement to validate
      the surprising wall-time delta: baseline **114 s** vs. pilot
      **130 s** (+14 %, far smaller than the first run's apparent
      +52 %). Disk numbers reproduced identically.
- [x] Controlled **release** re-measurement: baseline **138 s** vs.
      pilot **136 s** (−2 s, statistically a wash). The first pilot
      release's 255 s was the same kind of system-noise artifact as
      the first pilot debug.
- [x] Compute pampa-pilot delta vs. baseline in research note

### Phase 4 — Decision point

- [ ] Review pilot numbers with user (awaiting input)
- [ ] **If** the pilot delta is meaningful (e.g. >20% size drop) →
      Phase 5
- [ ] **If not** → revert pilot, close beads issue with findings,
      stop

**Pilot numbers (controlled, ready for decision):**

|                            | Debug  Δ            | Release Δ           |
| -------------------------- | -------------------:| -------------------:|
| `target/<profile>` size    | −3 GB  (−14 %)      | −1.9 GB (−17 %)     |
| Executables in `deps/`     | −56  (−25 %)        | −56  (−25 %)        |
| Sum of executable bytes    | −2.7 GiB (−26 %)    | −2.3 GiB (−26 %)    |
| Build wall time            | +16 s  (+14 %)      | −2 s   (−1 %)       |

This is from pampa *alone* (57/164 ≈ 35 % of all integration test
files). If the per-binary savings amortize roughly linearly across
the remaining 12 candidate crates (107 more files → 12 binaries,
i.e. saving 95 more binaries), Phase 6 should land near a ~50 %
reduction in `target/debug` and `target/release` from the baseline.
The wall-time cost stays small.

Recommendation: proceed to Phase 5.

### Phase 5 — Full rollout (conditional)

For each crate below, migrate in its own commit (one commit per
crate makes any individual revert cheap). After each migration, run
`cargo nextest run -p <crate>` before moving on.

**Preflight checklist** (run on each crate before moving files —
the pampa pilot showed how easy it is to miss one and then watch
several seemingly-unrelated tests fail):

```bash
crate=<crate-name>
# 1. File-root fn main / file-based mod / inline mod summary
grep -nE "^(mod [a-zA-Z_]+;|mod [a-zA-Z_]+ \{|fn main)" \
  crates/$crate/tests/*.rs

# 2. Source-file-relative compile-time and runtime paths
grep -nE 'include_str!|include_bytes!|include_dir!|#\[path' \
  crates/$crate/tests/*.rs
grep -nE 'set_snapshot_path|"\.\./' \
  crates/$crate/tests/*.rs

# 3. crate:: and super:: usage (would change meaning when each file
#    becomes a sub-module rather than the binary's crate root)
grep -nE 'crate::|super::' crates/$crate/tests/*.rs
```

Each non-zero match needs to be evaluated against the
"`tests/integration/` is one level deeper" rule. The two known
surgical edits below were caught by this checklist on the audit
pass; new ones may surface per crate.

Audit findings (pre-cached during Phase 0 / Phase 2 to make rollout
fast): all remaining crates are **clean pure-rename migrations**
*except* the specific files called out below.

- [x] quarto-core (33 files, commit `0ad8a40d`) — 4 `include_str!`
      `fixtures/...` paths got a `../` prefix
- [x] qmd-syntax-helper (20 files, commit `0d345c7c`) — clean rename
- [x] quarto-preview (7 files, commit `86dcd0c1`) — clean rename
- [x] quarto-sass (7 files, commit `1ad8b116`) — clean rename
- [x] quarto (7 files, commit `6651b1da`) — `#[path]` edit
- [x] quarto-highlight (6 files, commit `8dc679ef`) — `include_str!`
      prefix edit
- [x] comrak-to-pandoc (5 files, commit `6f8b1507`) — surgical edits:
      `proptest_roundtrip.rs` had `mod generators;` referring to the
      sibling top-level file; replaced with `use super::generators::*;`
      since `generators` is now a sibling module of
      `proptest_roundtrip` in the same binary. Also dropped the
      vestigial `fn main() {}` lines from `debug.rs` and `debug_comrak.rs`.
- [x] quarto-yaml-validation (5 files, commit `ce214672`) — 7
      `include_str!("../test-fixtures/...")` paths got an extra `../`
- [x] quarto-brand (4 files, commit `91cd5b71`) — clean rename
- [x] quarto-citeproc (2 files, commit `e8a9d075`) — clean rename
- [x] quarto-csl (2 files, commit `1d03f3f9`) — clean rename
- [x] quarto-doctemplate (2 files, commit `136a5c87`) — clean rename
- [x] `cargo nextest run --workspace` → 9444 passed, 196 skipped
      (same count as pre-migration baseline)
- [x] `cargo xtask verify --skip-hub-build` → Rust steps 1-7 green
      (warnings denied build pass, 9444/9444 tests). JS/TS legs
      remain environmentally blocked in this fresh worktree.
- [x] Snapshot relocations: 20 `.snap` files in quarto-core and
      quarto-highlight needed to move from `tests/snapshots/` to
      `tests/integration/snapshots/` and gain an `integration__`
      prefix to match insta's new file resolution
- [x] `.config/nextest.toml`: bd-u3ze override filter updated from
      `binary(staleness) | binary(eager_capture) | binary(boot)`
      to `package(quarto-preview) & binary(integration) &
      test(/^(staleness|eager_capture|boot)::/)`

### Phase 6 — Final measurement

- [x] Controlled back-to-back comparison via `git checkout`
      alternation between baseline (`8733ed67`) and branch tip:

|                            | Baseline (debug) | Rollout (debug) |       Δ debug | Baseline (release) | Rollout (release) |     Δ release |
| -------------------------- | ---------------: | --------------: | ------------: | -----------------: | ----------------: | ------------: |
| `target/<profile>` size    |            21 GB |           12 GB |  **−9 GB (−43 %)** |              11 GB |           4.5 GB |  **−6.5 GB (−59 %)** |
| Executables in `deps/`     |              220 |              76 | **−144 (−65 %)** |                220 |               76 | **−144 (−65 %)** |
| Sum of executable bytes    |         10.5 GiB |          2.5 GiB | **−8.0 GiB (−77 %)** |           9.0 GiB |           2.1 GiB | **−6.9 GiB (−77 %)** |
| Build wall time            |            118 s |           122 s |  **+4 s (+3 %)** |              158 s |             120 s | **−38 s (−24 %)** |

- [x] Computed final delta vs. baseline; recorded in research note

### Phase 7 — Report and decide

- [x] Update research note with all numbers + extrapolated CI impact
- [ ] Discuss findings with user; ask for push permission
- [ ] If pushed: update `CLAUDE.md` if any developer-facing test
      invocation conventions change (e.g. references to per-file
      test binaries)

## Risks & open questions

- **nextest binary filtering.** Per-binary names change from
  `<test-file>` to `integration`. Current CI runs
  `cargo nextest run --tests --cargo-profile ci` (no per-binary
  filters), so CI is unaffected. Confirmed by grepping `.github/`.
  Still worth a quick check during pilot that no `xtask` or
  `scripts/` invocation depends on the old per-file binary names.

- **Module name collisions inside a single binary.** Each former
  `tests/foo.rs` becomes module `integration::foo`. If two former
  files both had e.g. `mod helpers;` referring to sibling files,
  they would now refer to the same `integration/helpers.rs`. None
  of the pampa files declare a file-based `mod` at the top level
  (only `test_location_health.rs` has any `mod` keyword — needs
  audit). Risk is small; the audit step in Phase 2 covers it.

- **`fn main()` collisions.** A `#[test]` integration file can
  optionally declare its own `fn main()`. Generated `main.rs` will
  declare one. Verified: zero pampa test files declare `fn main()`.

- **macOS-only measurement.** We cannot directly measure the
  Linux/Windows CI delta from this machine. The ark PR shows the
  size ratio is roughly stable across platforms (linker bloat is
  proportional to dependency closure, which is the same on all
  platforms). The research note should call out that the
  Linux/Windows wins are *predicted*, not measured.

- **Existing 259 GB target/.** Not affected by this change — it
  accumulates across all branches the user has built locally. The
  experiment uses a fresh build for clean numbers and `cargo clean`
  between each phase. Worth offering to clean it up at the end of
  the experiment regardless of outcome (would free ~256 GB).

## Decision log

- 2026-05-28: Use `tests/integration/` (matches ark PR) over
  `tests/it/` (matklad blog). Same mechanic; ark name is more
  discoverable to new contributors.
- 2026-05-28: Pilot pampa first rather than migrate all 13 crates
  in one pass. pampa is 57/164 ≈ 35% of integration files; if the
  per-file bloat hypothesis holds, the pilot should already produce
  a measurable size drop in `target/debug/` and give us a confident
  decision point.

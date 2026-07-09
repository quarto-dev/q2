# CSL manifest lockfile false-positive on Windows incremental builds

Strand: bd-2w80 ("Investigate CSL manifest test failure after rebase") — **closed**.

## Resolution (final)

Neither research agent's mechanism theory panned out. Empirical repro (add
then remove a fixture file, no `cargo clean`) showed directory-level
`rerun-if-changed` correctly triggers a rebuild both ways on cargo
1.97.0-nightly — the "directory watch misses changes" theories (both the
pre-1.50-citation one and the mtime-granularity one) were never confirmed as
the actual cause. Bigger finding: the baked `expected` string only ever
depended on file names/counts, never content — so the per-file
`rerun-if-changed` fix originally planned below **would not have prevented
this failure even if applied**, since it only helps with content-edit
detection.

Actual fix shipped: stopped baking the validation state at build time
entirely. `csl_validate_manifest` is now a hand-written runtime test in
`tests/integration/csl_conformance.rs` that reads `test-data/csl-suite/` +
`tests/enabled_tests.txt` live at test-run time (via
`env!("CARGO_MANIFEST_DIR")`, a compile-time *path* constant, not a baked
*computed value*) and compares against `tests/csl_conformance.lock` in the
same instant. No time-of-check/time-of-use gap, no dependency on
build-script rerun timing for this check at all. `build.rs` now only handles
per-fixture test codegen (unaffected — verified byte-identical by an
independent review pass). Full crate suite green: 863 passed, 0 failed.

The per-file `rerun-if-changed` hardening for `quarto-citeproc` +
`quarto-sass` (checklist items below) was **not applied** — no reproducible
defect was ever found for that angle, so it would have been unproven
defensive code.

---

## Original investigation (kept for history)

## Overview

`quarto-citeproc::csl_conformance::csl_validate_manifest` failed on a Windows
incremental build with "Lockfile mismatch: tests/csl_conformance.lock", even
though `git status` showed `tests/enabled_tests.txt` and `test-data/csl-suite/`
clean, and regenerating the lockfile (`UPDATE_CSL_LOCKFILE=1`) produced a
byte-identical file. `cargo clean -p quarto-citeproc` + fresh build made the
test pass with zero lockfile changes. Conclusion: `build.rs` served a stale
baked `expected` value from an old `generated_csl_tests.rs`, not a real data
divergence.

Two research passes (Opus, repo-specific; Sonnet, ecosystem-wide) looked at
*why* `build.rs`'s `cargo:rerun-if-changed=test-data/csl-suite` (a directory,
not per-file) failed to trigger a rebuild, and disagree on mechanism:

- **Opus**: cited [rust-lang/cargo#2599](https://github.com/rust-lang/cargo/issues/2599)
  — directory `rerun-if-changed` only tracks the directory's own mtime, misses
  in-place file edits.
- **Sonnet**: found #2599 was *fixed* by
  [rust-lang/cargo#8973](https://github.com/rust-lang/cargo/pull/8973), merged
  2020-12-14, shipped in **Cargo 1.50**. Since then, directory
  `rerun-if-changed` recursively scans and takes the max mtime of every file
  inside. Current repo toolchain is `nightly-2026-04-28` → `cargo
  1.97.0-nightly` — Opus's cited mechanism is stale for the cargo version we
  actually run.

So the "directory watch is shallow" story is **not confirmed** for our cargo
version. Sonnet's alternate candidates: mtime-granularity/timestamp-truncation
bugs Cargo has open elsewhere (e.g.
[#13119](https://github.com/rust-lang/cargo/issues/13119), WSL/9p nanosecond
truncation; [#9445](https://github.com/rust-lang/cargo/issues/9445), relative
path normalization in dep-info) are more plausible, but neither is a confirmed
match either — they're precedent for "Cargo's rerun-if-changed freshness check
is fragile on Windows-adjacent filesystems in general," not a proven cause
here.

**Before writing any public rationale (tracker comment, commit message), we
need to settle the actual mechanism empirically on this machine** — not ship a
fix with a plausible-sounding but unverified "why."

The concrete fix both agents converge on regardless of mechanism — enumerate
every fixture file explicitly for `rerun-if-changed` instead of relying solely
on the directory line — is correct either way: it's strictly more precise than
Cargo's own directory scan, and matches an existing in-repo pattern
(`watch_recursive()` in `quarto-trace-server`, `quarto-preview`,
`quarto-mcp-launcher` build.rs files). `quarto-sass/build.rs` has the same
directory-only gap (`resources/scss`, `src`) and has not yet manifested a
failure — worth fixing in the same pass since it's the same category of bug.

## Checklist

- [x] **Reproduce mechanism empirically.** Confirmed on cargo 1.97.0-nightly:
      add/remove of a fixture file under `test-data/csl-suite/` correctly
      triggers a build.rs rerun with no `cargo clean` needed — the
      shallow-directory-scan theory is stale for this toolchain.
- [x] **Identify actual trigger** for the original failure if possible —
      not reconstructed; documented as "not reproduced from a clean
      baseline, only observed once," and the fix removes the dependency on
      build-script rerun timing entirely rather than chasing the trigger.
- [x] **Apply per-file `rerun-if-changed` fix** — superseded. Root cause
      wasn't rerun-if-changed at all (see Resolution above): fixed instead by
      moving `csl_validate_manifest` to a runtime test with no baked value.
- [x] **Apply the same fix to `crates/quarto-sass/build.rs`** — decided not
      pursued (see Resolution above): no reproducible defect found for that
      angle, so the defensive change wasn't justified.
- [x] **Verify per project TDD/bug-fix rules**: added `manifest_logic_tests`
      unit tests for the extracted pure logic (red before, green after); the
      original staleness bug isn't regression-testable (can't force Cargo's
      build-script rerun timing deterministically) — fixed architecturally
      instead.
- [x] **Full workspace verify**: full crate suite green, 863 passed, 0
      failed, 142 skipped.
- [x] **Correct the existing bd-2w80 comment.** Posted a follow-up comment
      retracting the NTFS-vs-ext4 framing and stating the confirmed
      mechanism, before closing.
- [x] **Decide bd-2w80 disposition**: closed with the confirmed root cause +
      fix; `quarto-sass` sibling fix not split off (not pursued, see above).
- [x] **Commit.** Shipped in
      [`9f96e2c1`](https://github.com/quarto-dev/q2/commit/9f96e2c16b95abbd0171d634315b48857138488f)
      (PR [#380](https://github.com/quarto-dev/q2/pull/380), merged), staging
      only `crates/quarto-citeproc/build.rs` + the new runtime test file
      (`quarto-sass/build.rs` untouched, per the decision above).

## Notes / open questions

- Both research agents agree on the *fix* even while disagreeing on the
  *mechanism* — low risk either way, but the tracker comment and commit
  message should state only what we've actually verified on this cargo
  version, not repeat either agent's unverified citation.
- Dispatch-prompt gap noticed mid-investigation: non-fork subagents (Explore,
  general-purpose) inherit zero context from `~/.claude/rules/*.md` or project
  `CLAUDE.md` — the sonnet research agent used `curl` for GitHub source
  reads instead of `gh repo read-file`/`read-dir` because the dispatch prompt
  never mentioned that preference. Not fixed as part of this plan; noted for
  future dispatch-prompt hygiene.

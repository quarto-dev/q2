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

- [ ] **Reproduce mechanism empirically.** With current `generated_csl_tests.rs`
      freshly built (no dirty cache), touch *only the mtime and content* of one
      existing file in `test-data/csl-suite/` (no add/remove), then run
      `cargo nextest run -p quarto-citeproc csl_validate_manifest` without
      `cargo clean` first. If build.rs re-runs and the test still passes (no
      new fixture added, so `expected` shouldn't change anyway) — need a
      variant that actually changes what's baked, e.g. add a fixture to
      `test-data/csl-suite/` *and* to `enabled_tests.txt`, then only touch
      mtime-adjacent files to see if a plain incremental build (no clean)
      picks up the addition. Confirm whether Cargo actually re-invokes
      build.rs (check via `cargo build -vv -p quarto-citeproc | grep
      "Running.*build-script"` or by adding a temporary `eprintln!` in
      build.rs) or serves stale output.
- [ ] **Identify actual trigger** for the original failure if possible — was it
      a `git rebase`/checkout that touched files at/before Cargo's cached
      build-script timestamp? Check `git reflog` / recent branch history if
      still reconstructable; otherwise document as "not reproduced from a
      clean baseline, only observed once" and rely on the mechanism test above.
- [ ] **Apply per-file `rerun-if-changed` fix** to `crates/quarto-citeproc/build.rs`:
      after `test_files` is collected and sorted (existing code, ~line 36),
      loop over it and emit `cargo:rerun-if-changed=<path>` per fixture. Keep
      the existing directory line (still needed/harmless for catching
      additions/removals). No new `walkdir` dependency needed — `csl-suite/`
      is flat, and the file list is already collected.
- [ ] **Apply the same fix to `crates/quarto-sass/build.rs`** (`resources/scss`
      and `src` directory watches) — same latent gap, not yet triggered.
      Confirm whether that build.rs already collects a file list to reuse, or
      needs its own walk (check if `walkdir` is already a build-dependency
      there for the sibling crates' pattern).
- [ ] **Verify per project TDD/bug-fix rules**: this is build-script infra, not
      app logic, so the "test" is the reproduction procedure itself (already
      have one: `cargo clean -p quarto-citeproc` fixes it; need the
      no-clean-incremental repro from step 1 to prove the *fix*, not just the
      symptom, is addressed). Confirm fixed build.rs reruns correctly on a
      fixture add without requiring `cargo clean`.
- [ ] **Full workspace verify**: `cargo build --workspace`, `cargo nextest run
      --workspace`, `cargo xtask verify --skip-hub-build` (Rust-only change).
- [ ] **Correct the existing bd-2w80 comment.** The comment already posted
      claims "cargo's directory-mtime dependency tracking is known to be
      unreliable on Windows (NTFS mtime semantics differ from Linux ext4)" —
      that framing is Opus's pre-1.50 citation, not confirmed against our
      actual cargo version. Post a follow-up comment with the corrected
      mechanism once step 1 settles it, before/alongside closing.
- [ ] **Decide bd-2w80 disposition**: close with the confirmed root cause +
      fix, or split the `quarto-sass` sibling fix into its own strand
      (`discovered-from` link) if it's not bundled into the same PR.
- [ ] **Commit.** Stage `crates/quarto-citeproc/build.rs`,
      `crates/quarto-sass/build.rs` (if touched). Do not push without explicit
      approval per project git policy.

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

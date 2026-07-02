# Pampa pandoc-oracle tests hard-fail on local pandoc newer than allowlist (bd-i9i5ad2t)

**Date:** 2026-07-02
**Braid:** bd-i9i5ad2t
**Worktree:** `.worktrees/bd-i9i5ad2t-pampa-pandoc-oracle-tests` (branch `braid/bd-i9i5ad2t-pampa-pandoc-oracle-tests`, based on `main` @ `51cf3707`)
**Status:** Design aligned 2026-07-02 — ready to implement (TDD). One prerequisite: bd-nj9nnkn1 (Windows clippy blocker) must be green for the final `cargo xtask verify`.

## Triage verdict

**Ready to implement.** Root cause confirmed at HEAD, repro trivial. Four design decisions resolved with the user (below). Skeleton is now a real plan.

## Issue context

Substring version gate in `crates/pampa/tests/integration/test.rs`:

```rust
fn has_good_pandoc_version() -> bool {
    // ...
    version_str.contains("3.6")
        || version_str.contains("3.7")
        || version_str.contains("3.8")
        || version_str.contains("3.9")
}
```

Two failure axes:
1. **Fragile matching.** `contains("3.6")` matches anywhere in the string — brittle. `pandoc 3.10` matches none of the four literals → returns `false`.
2. **Inconsistent handling at call sites.** Internal helpers (`matches_pandoc_*_reader`) treat `false` as *skip* (`return true`). But four `#[test]` functions (`test_html_writer`, `test_json_writer`, `unit_test_corpus_matches_pandoc_markdown`, `unit_test_corpus_matches_pandoc_commonmark`) `assert!(has_good_pandoc_version(), ...)` → **hard-fail** with unhelpful `"Pandoc version is not suitable for testing"`.

CI unaffected — `.github/workflows/test-suite.yml` pins `PANDOC_VERSION=3.8.3` exactly. Only local dev with off-allowlist pandoc bites. Priority 3, bug, filed + `in_progress` by cderv 2026-07-02.

**Design intent (from strand):** keep the hard-FAIL. The allowlist is a *manual verification ledger* — each minor version was added by a human confirming the 4 oracle tests still pass against it (commit `12bca3b5`). A graceful skip would rot silently (nextest shows early-return tests as PASS, not SKIP). So the goal is not "stop failing" — it's "fail with an actionable message, and make the ledger bump a one-command chore."

## Dependency graph

**Empty.** `braid dep list` returns no edges — no `discovered-from`, no `blocks`, no `related`. No incoming urgency, no upstream context beyond the strand's own (rich) description. The design ledger substitutes for what a dep graph would normally supply.

## What the code looks like today

Both paths in the strand still exist with the described shape:

- `crates/pampa/tests/integration/test.rs:91-101` — `has_good_pandoc_version()` substring allowlist (confirmed).
- `crates/pampa/tests/integration/test.rs` call sites — 3 skip-style (`:126, :141, :171`), 4 hard-fail `assert!` (`:227, :257, :438, :528`) (confirmed).
- `crates/xtask/src/dev_setup.rs:239` — `pandoc_version_at_least(version_output, min_major, min_minor)`. Floor check only (`>= (min_major, min_minor)`), no upper bound. Already has unit tests (`:253-263`) covering `3.6`, `3.10`-style… actually only tests up to `4.0`; parses `major.minor` off the first line after `"pandoc "`. Used once, for a dev-setup warning.

**Repro at HEAD:** local `pandoc 3.10` installed. `contains("3.6".."3.9")` all `false` → the 4 oracle tests `assert!`-fail. This is exactly the reported symptom. (No fixture needed; the repro is "run the 4 tests with pandoc 3.10 on PATH.") Repro is confirmed *logically* — see the pre-flight note below; a harness-level run of the 4 tests was blocked by an unrelated clippy error before nextest ran.

**Pre-flight verify did NOT reach the pampa tests.** `cargo xtask verify --skip-hub-build` at HEAD (`51cf3707`) fails at the clippy stage on a **pre-existing, Windows-only, unrelated** dead-code error: `highest_version_node` in `crates/quarto-mcp-launcher/src/node.rs:231` is called only from the non-Windows branch of `node_search_paths` (lines 207-213); the `#[cfg(windows)]` branch doesn't call it and the fn itself is not cfg-gated, so on Windows it is dead code → `-D warnings`. CI (Linux/Mac) uses the fn, so it's green there; only Windows dev (Chris) hits it. Filed as a discovered strand linked to this one. It blocks running the pampa test leg locally until fixed or `#[allow]`/`#[cfg]`-gated.

The two version-check sites **disagree on philosophy**: dev_setup wants a *floor* (`>=3.6`, forward-open), test.rs wants a *calibrated set* (3.6–3.9, closed). A shared parser body serves both, but each site keeps its own *policy* (floor in dev_setup, closed range in test.rs) — see decisions 1 and 3.

## Resolved design decisions (2026-07-02)

1. **Parser home — duplicate, don't cross-crate-share.** xtask is bin-only (no `[lib]`, "not part of the library API"), so pampa's test binary can't import from it without converting xtask to lib+bin and pulling `syn`/`clap`/`walkdir`/`serde_yaml` into pampa's test build. Instead: keep a small `parse_pandoc_version(&str) -> (u32, u32)` in **both** `crates/pampa/tests/integration/test.rs` and `crates/xtask/src/dev_setup.rs`, pinned to identical behavior by matching unit tests. No `Cargo.toml` dep changes, no cycle.
2. **`pandoc-check` is print-only.** It does **not** auto-edit `test.rs` (avoids fighting the `cargo fmt` post-edit hook and needing a stable source anchor). On green it prints the exact ceiling to bump to; the human makes the one-line edit — matching the "manual verification ledger" spirit.
3. **Closed range, not an explicit set.** Calibrated window is a closed range `(3,6)..=(3,9)`; bumping = raise the ceiling.
4. **`pandoc-check` scope is narrow.** Runs just the 4 oracle tests against local pandoc + reports calibration. It does **not** also drive the dev-setup floor warning.
5. **Gate bypass is an env var.** `has_good_pandoc_version()` returns `true` when `PAMPA_PANDOC_ORACLE_BYPASS_VERSION_GATE` is set to **exactly `1`** (checked before parsing); any other value, or unset, leaves the gate active. This is the single seam `pandoc-check` uses to run the 4 tests against an off-range pandoc without weakening the assertion or duplicating test bodies. Normal local/CI runs never set it, so the hard-fail ledger signal is untouched. (Added from design review — this seam was previously implicit.)

### Out of scope

- **No-pandoc behavior is unchanged.** `has_good_pandoc_version()` currently `Command::new("pandoc")…expect(...)` → panics when pandoc is absent. That is pre-existing and not addressed here; this strand is about the version *range* gate, not pandoc discovery.

## Phases

### Phase 0 — Test plan (TDD, RED first)

Two pure functions to test (both spawn nothing → unblocked by bd-nj9nnkn1). The pampa copies run via `cargo nextest run -p pampa`; the mirrored xtask parser tests run via `cargo nextest run -p xtask` — run **both** to complete the RED/GREEN loop across the two copies.

**a. `parse_pandoc_version(&str) -> (u32, u32)` + range check.** Cases:
- `pandoc 3.10` → `(3,10)`, **out of range** (this is the bug: substring `contains` false-rejects; numeric `(3,10) > (3,9)` correctly out-of-range, not falsely-in).
- `3.6` / `3.9` boundaries → in range; `3.5` → below floor; `4.0` → above ceiling.
- Malformed / empty → `(0,0)`, out of range.

**b. Diagnostic formatter** — a pure `fn format_gate_failure(raw_version: &str, range: …) -> String` that the `assert!` message uses. Assert it contains the raw detected version line, the calibrated range, and the `cargo xtask pandoc-check` command. This guards the *user-visible* improvement (M3 from design review) without depending on assertion-message brittleness.

**Duplicated case table (M5).** Consistent with decision 1 (duplicate, don't cross-crate-share): put an **identical** named `const` slice of parse test vectors in *each* location — `test.rs` and xtask's `test_pandoc_version_at_least` module — not a single shared slice (no new crate dep). Each copy carries a comment requiring lockstep updates. Both parser copies are validated against their (identical) table.

Confirm RED before implementing.

### Phase 1 — Replace the gate + bypass seam

In `test.rs`, rewrite `has_good_pandoc_version()`:
1. First, if `PAMPA_PANDOC_ORACLE_BYPASS_VERSION_GATE=1`, return `true` (decision 5 — the calibration seam).
2. Otherwise replace the `contains("3.x")` chain with `parse_pandoc_version()` + closed-range check `(3,6)..=(3,9)`.

Keep the hard-`assert!` at the 4 test call sites (ledger signal preserved); leave the 3 internal helpers' skip behavior unchanged. The bypass lands **here**, not in Phase 3, so the xtask command layers cleanly on a finished seam (H4 — avoids Phase 3 rewriting Phase 2's assertions).

### Phase 2 — Actionable failure message

Wire the Phase 0b `format_gate_failure` into the 4 `assert!` sites so on out-of-range they print: the raw detected version line, the calibrated range, and `cargo xtask pandoc-check`.

### Phase 3 — `cargo xtask pandoc-check`

New xtask subcommand (steps in `.claude/rules/xtask.md`). Sets `PAMPA_PANDOC_ORACLE_BYPASS_VERSION_GATE=1` and runs the 4 oracle tests against local pandoc, narrow scope. Print-only — no source edit. **Contract:**
- **Exit 0** iff all 4 oracle tests pass under bypass; **nonzero** otherwise.
- **Output** always: detected pandoc version, current calibrated range, and — on green with detected > ceiling — the proposed new ceiling plus the exact constant/line in `test.rs` to edit.
- **On failure:** report which of the 4 tests broke (so an incompatible pandoc is diagnosable, not just "red").

### Phase 4 — Docs

Ledger comment next to the range constant in `test.rs` describing the bump workflow; add `pandoc-check` to the xtask command list in `.claude/rules/xtask.md`.

### Phase 5 — Final verification

After **bd-nj9nnkn1** is green, run `cargo xtask verify --skip-hub-build` and confirm the full Rust leg passes (this is the step the pre-flight couldn't reach). Rust-only change → `--skip-hub-build` is sufficient.

## Prerequisite

- **bd-nj9nnkn1** — `highest_version_node` Windows dead-code clippy failure blocks `cargo xtask verify` locally on Windows. One-line `#[cfg(not(windows))]` gate. Must be green for Phase 5; does not block Phase 0–4 iteration via `cargo nextest run -p pampa`.

## Risks / tradeoffs

- **Two parser copies** (decision 1): drift risk, mitigated by the shared unit-test contract in Phase 0.
- Low blast radius: test-harness + xtask only, no product code. CI pins `PANDOC_VERSION=3.8.3` and is unaffected either way.

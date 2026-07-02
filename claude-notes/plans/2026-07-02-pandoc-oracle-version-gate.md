# Pampa pandoc-oracle tests hard-fail on local pandoc newer than allowlist (bd-i9i5ad2t)

**Date:** 2026-07-02
**Braid:** bd-i9i5ad2t
**Worktree:** `.worktrees/bd-i9i5ad2t-pampa-pandoc-oracle-tests` (branch `braid/bd-i9i5ad2t-pampa-pandoc-oracle-tests`, based on `main` @ `51cf3707`)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Root cause confirmed at HEAD, repro trivial, and Chris's strand already carries a detailed design ledger. Three real design decisions remain (cross-crate parser sharing, auto-edit-vs-print for the bump command, range-vs-allowlist representation) before the skeleton becomes a finished plan.

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

The two version-check sites **disagree on philosophy**: dev_setup wants a *floor* (`>=3.6`, forward-open), test.rs wants a *calibrated set* (3.6–3.9, closed). A shared parser can serve both, but the *policy* differs — see design questions.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Unit-test the new parser/gate for: `3.10` (numeric > 3.9, must be out-of-range not falsely-in), `3.6`/`3.9` boundaries, `4.0`, malformed/empty. Test lives where the shared parser lands (see Q1). RED first.
- **Phase 1 — Shared parser.** Replace substring matching with `major.minor` parsing; unify with `dev_setup.rs`'s `pandoc_version_at_least`. Represent the calibrated range (see Q3).
- **Phase 2 — Actionable failure message.** On out-of-range: print detected version, calibrated range, and the exact command to verify+bump.
- **Phase 3 — `cargo xtask pandoc-check`.** Run the 4 oracle tests against local pandoc bypassing the gate; on green, self-heal the allowlist (or print the line to add) — see Q2.
- **Phase 4 — Docs.** Note the bump workflow (dev-docs or the ledger comment near the allowlist).

## Open design questions for the user

1. **Where does the shared parser live?** `dev_setup.rs` is in the `xtask` crate; `test.rs` is pampa's integration-test binary. xtask is not (and shouldn't become) a dependency of pampa's tests. Options: (a) a tiny shared crate / `quarto-util` helper both depend on; (b) accept a small duplicated `parse_pandoc_version()` in each place with a shared unit-test contract; (c) something else. The strand says "reuse/share logic" — but a cross-crate share may cost more than the ~8-line parser is worth. **Recommendation: (b) duplicate the parser, share the contract via matching unit tests** unless you want a util home for it.

2. **`pandoc-check`: auto-edit source, or print-only?** The ledger says "edits the allowlist in test.rs … (or at minimum prints the exact line to add)." Auto-editing a source file from an xtask is a codegen-touches-tracked-source pattern (needs a stable anchor in test.rs, risks fighting rustfmt/the fmt hook). Print-only is simpler and keeps the human in the ledger loop (matches the "manual verification ledger" spirit). **Recommendation: print-only for v1**, add auto-edit later if the chore is still annoying.

3. **How is the calibrated range represented?** Once parsing is numeric, is it (a) a floor+ceiling `(3,6)..=(3,9)`, or (b) an explicit set of known-good minors `{6,7,8,9}`? A closed range is one bump-the-ceiling edit; an explicit set matches "each version individually verified" but is more to maintain. Note the ledger semantics: a *gap* (e.g. 3.7 verified, 3.8 skipped, 3.9 verified) is only expressible with a set. Has any minor ever been skipped, or is it always contiguous? **Recommendation: closed range** unless gaps are real.

4. **Scope of `pandoc-check` vs. the gate.** Should `pandoc-check` also drive the dev-setup floor warning (one command reporting both), or stay narrowly "run the 4 oracle tests + report calibration"? Keeps blast radius small if narrow.

## Risks / tradeoffs (draft)

- **Auto-editing test.rs** (Q2 option) is the highest-risk piece: fmt hook runs `cargo fmt` on any edited Rust file, so the xtask's edit and the hook could race/conflict; a print-only design sidesteps this entirely.
- **Cross-crate sharing** (Q1) could pull a new dependency edge into pampa's test build for an 8-line function — likely not worth it.
- Low blast radius overall: changes are test-harness + xtask only, no product code, CI already pinned and unaffected.

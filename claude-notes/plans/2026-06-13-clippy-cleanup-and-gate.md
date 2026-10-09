---
title: 'Clean up clippy debt and gate clippy in CI (bd-3zst4hwy)'
date: 2026-06-13
description: 'Clears the accumulated clippy violations across the workspace and adds a `-D warnings` clippy gate to CI and `cargo xtask verify` so the workspace stays clean.'
braid:
  strand: bd-3zst4hwy
  priority: P2
---

## Overview

CI enforces `RUSTFLAGS="-D warnings"` (rustc warnings) but **never runs
`cargo clippy`**, so clippy lint violations accumulated unchecked. This
plan clears them and adds a CI gate so the workspace stays clean.

### Scope (measured, not guessed)

The workspace already has a curated `[workspace.lints.clippy]` table
(root `Cargo.toml`) that **allows** the architectural/stylistic lints
(`result_large_err`, `large_enum_variant`, `too_many_arguments`,
`type_complexity`, `ptr_arg`, `wrong_self_convention`, …) and sets a set
of specific lints to `warn`. All 41 workspace crates opt in via
`[lints] workspace = true`.

Respecting that allow-policy (`cargo clippy --workspace --all-targets --
-D warnings`), the **true** burn-down is **69 violations across 11
crates**:

| crate | n |
|---|---|
| quarto-hub | 42 |
| xtask | 11 |
| quarto-system-runtime | 7 |
| quarto-highlight-encoding | 2 |
| quarto-trace-server, quarto-source-map, quarto-sass, quarto-preview, quarto-mcp-launcher, quarto-brand, lua-src-wasm | 1 each |

By lint:

| lint | n | auto-fix? |
|---|---|---|
| collapsible_if | 32 | yes |
| map_unwrap_or | 13 | yes |
| needless_borrows_for_generic_args | 5 | yes |
| implicit_clone | 4 | yes |
| get_unwrap | 3 | usually |
| io_other_error | 2 | yes |
| derivable_impls | 2 | yes |
| collapsible_match | 2 | yes |
| unreadable_literal, unnecessary_map_or, single_component_path_imports, should_implement_trait, print_literal, naive_bytecount | 1 each | mixed |

> ⚠️ Earlier raw counts of \~560 / \~1811 were an artifact of running
> clippy with `-W clippy::all`, which **re-enables the deliberately
> allowed** architectural lints. Those stay allowed; do not "fix" them.

### Judgment-call lints (per the agreed policy: `#[allow]` + note, file
follow-ups only if a real refactor is warranted)

- `should_implement_trait` \@ `quarto-source-map/src/source_info.rs:163`
  — an inherent method shadowing a trait method name; likely a
  deliberate API. `#[allow]` with a note unless trivially renameable.
- `naive_bytecount` \@ `xtask/src/braid_snapshot.rs:60`,
  `xtask/src/create_worktree.rs:1132` — would pull in the `bytecount`
  crate for a dev-tool line count; `#[allow]` (not worth a dep).

## Gate mechanism (gate-and-grow)

The opt-in lever (`[lints] workspace = true`) is already fully consumed —
all crates are in. So the gate is simply: **fix all 69, then enforce
`-D warnings` for clippy in CI + xtask.** No per-crate baseline-allow
scaffolding is needed at this size. (Had the count been in the hundreds,
we'd have baselined per-crate allows and burned them down; documented
here in case the policy widens later.)

Enforcement points to add once green:
1. `.github/workflows/test-suite.yml` — a clippy step:
   `cargo clippy --workspace --all-targets -- -D warnings`.
2. `cargo xtask verify` / `lint` — same invocation, so local matches CI
   (mirrors the existing `-D warnings` RUSTFLAGS pattern).

## Work items

- [x] Measure true scope; strand + plan (bd-3zst4hwy)
- [x] Phase 1 — `cargo clippy --fix` auto-fixed the mechanical bulk.
      Checkpoint commit `25c1e187` (revert point).
- [x] Phase 2 — hand-fixed / `#[allow]`-ed the remainder (28 + a later
      cascade-revealed layer in quarto-publish/quarto leaf crates).
- [x] Phase 3 — `cargo clippy --workspace --all-targets -- -D warnings`
      reports **zero** (exit 0). Renamed the stale `empty_enum` table
      entry to `empty_enums` so the run is notice-free.
- [x] Phase 4 — `cargo nextest run --workspace` → **10036 passed**,
      twice (post-auto-fix and post-hand-fix). `cargo check --workspace
      --all-targets` green; `cargo fmt --check` clean.
- [x] Phase 5 — CI gate added (`.github/workflows/test-suite.yml`
      "Clippy (deny warnings)" step) + `cargo xtask verify` Step 1 now
      runs `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] Phase 6 — `cargo xtask verify --skip-hub-build`: Steps 1–6 green
      (clippy gate fired in Step 1 ✓, build ✓, 10036 tests ✓, ts-packages
      ✓). Step 8 hub-client `test:ci` fails on `vitest.config.ts` load —
      the pre-existing missing-WASM-build environmental issue, unrelated
      to this Rust-only change (`--skip-hub-build` skips the build but
      still runs the tests).

## Verification record

- **True scope**: the deny-cascade (each crate aborts on its first lint,
  masking the rest and all downstream crates) hid the real count.
  Successive fix-and-remeasure passes peeled back layers:
  69 → 250 → 28 → +9 (quarto-publish/quarto leaf crates). Net cleanup
  ≈ 525 violations.
- **Auto-fix friction**: `quarto-core` fixes wouldn't apply via normal
  `cargo fix` (it reverts a whole crate when any fix breaks compilation).
  `--broken-code` forced them and exposed a clippy bug that expanded a
  `matches!()` macro into its raw `$expression`/`$pattern` template in
  `compile_theme_css.rs`, cascading into a dropped `SidebarStyle` import.
  Both hand-corrected; a workspace `cargo check` then confirmed compile.
- **Lessons**: (1) measure clippy scope WITHOUT `-D warnings` (use the
  table levels) so the cascade doesn't mask crates; (2) avoid
  `--broken-code` — prefer per-crate `cargo clippy --fix` after the crate
  compiles clean; (3) a statement-level `#[allow]` on an `assert!`/macro
  invocation is **ignored** (and trips `unused_attributes` under
  `-D warnings`) — put the allow on the enclosing fn/`let` instead.
- **Behavior-sensitive fixes validated by the full suite**: shortcode
  let-chain, render `filter_map`→`filter`, orchestrator cache `if let`,
  github always-https collapse, capture_splice `never_loop` allow,
  revealjs `while let`.
- **Final gate**: `cargo clippy --workspace --all-targets -- -D warnings`
  → exit 0; `cargo nextest run --workspace` → 10036 passed.

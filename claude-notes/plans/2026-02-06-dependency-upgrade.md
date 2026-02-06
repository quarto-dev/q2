# Dependency Upgrade — 2026-02-06

First run of the dependency upgrade workflow.

## Phase 0: Preparation
- [x] Clean working tree
- [x] Run cargo audit
- [x] Note any security advisories:
  - VULN: `bytes` 1.11.0 — integer overflow in BytesMut::reserve (RUSTSEC-2026-0007), fix: >=1.11.1
  - WARN: `bincode` 1.3.3 — unmaintained (transitive via deno_core, not actionable)
  - WARN: `paste` 1.0.15 — unmaintained (transitive via v8/deno_core, not actionable)

## Phase 1: Lock File Update
- [x] cargo update — 90 packages updated
- [x] cargo build --workspace — clean
- [x] cargo nextest run --workspace — 6180 passed, 193 skipped
- [x] Committed as `022e02dc`

## Phase 2: Bump Declared Minimums
- [x] cargo upgrade (compatible only) — 48 packages bumped
- [x] cargo build --workspace — clean
- [x] cargo nextest run --workspace — 6180 passed, 193 skipped
- [x] cargo xtask verify — all passed (272 hub-client tests + 11 WASM tests)
- [x] Committed as `3bfa0e58`

## Phase 3: Breaking Upgrades

All breaking upgrades attempted except the Deno cluster.

### Completed (12 upgrades)

| Upgrade | Code changes | Commit |
|---------|-------------|--------|
| schemars 0.8 -> 1.2 | None | `fdd5bbd1` |
| toml 0.8 -> 0.9 | None | `2c543d96` |
| thiserror 1 -> 2 | None | `ee71a0b2` |
| colored 2 -> 3 | None | `1de343ef` |
| yaml-rust2 0.10 -> 0.11 + hashlink 0.10 -> 0.11 | None (coupled versions) | `b627113d` |
| which 7 -> 8 | None | `90cec998` |
| crossterm 0.28 -> 0.29 | None | `7b2aea51` |
| comrak 0.49 -> 0.50 | None | `4529df0b` |
| ariadne 0.4 -> 0.6 | `Report::build` API change (3 args -> 2 with tuple span) | `b7df4af5` |
| mlua 0.10 -> 0.11 | `inspect_stack` now takes closure; fixed in diagnostics.rs + types.rs | `86b7942d` |
| samod 0.6 -> 0.7 | Already upgraded by recursive dep resolution | `e72e2ebf` |
| jupyter-protocol 0.11 -> 1.0 + runtimelib 0.30 -> 1.0 | MediaType variants hold Value directly, not Map | `3c11efd9` |

### Reverted (1 upgrade)

| Upgrade | Reason | Commits |
|---------|--------|---------|
| tree-sitter 0.25 -> 0.26 | WASM build fails for wasm32-unknown-unknown target | `31034e40` (upgrade), `91b8dbf1` (revert) |

### Post-Phase 3 Fix

- [x] wasm-quarto-hub-client yaml-rust2 0.10 -> 0.11 (excluded crate, needed manual update) — `d403c726`
- [x] cargo xtask verify — all passed

### Deferred

- Deno cluster (`deno_core`, `deno_web`, `deno_webidl`, `serde_v8`) — must upgrade atomically, complex
- tree-sitter 0.25 -> 0.26 — blocked on WASM target support

## Summary

First run of the dependency upgrade workflow (2026-02-06).

- **Phase 0**: cargo audit found 1 vulnerability (bytes integer overflow) and 2 warnings (unmaintained transitive deps in Deno tree)
- **Phase 1**: Updated Cargo.lock — 90 packages bumped within existing constraints. Fixes RUSTSEC-2026-0007.
- **Phase 2**: Bumped Cargo.toml declared minimums — 48 compatible version bumps across 16 files.
- **Phase 3**: 12 of 13 breaking upgrades completed successfully. tree-sitter 0.26 reverted due to WASM incompatibility. Fixed WASM crate yaml-rust2 version mismatch.
- **Final state**: `cargo xtask verify` passes (all Rust tests + WASM build + hub-client tests).

Tools installed: `cargo-edit` v0.13.8, `cargo-outdated` v0.17.0, `cargo-audit` v0.22.1

# Migrate samod: `quarto-dev/samod@q2` → `shikokuchuo/samod@access-policy`

## Overview

Our vendored samod fork is moving from **`quarto-dev/samod` branch `q2`**
(samod `0.9.0`, automerge `0.8.0`) to **`shikokuchuo/samod` branch
`access-policy`** (samod `0.12.1`, automerge `0.10.0`).

The new fork = upstream samod `0.12.1` + a fresh "Implement access policy"
commit. The *only* thing the old `q2` branch carried on top of its base was
its own "Implement access policy" commit; every other commit under it was
already upstream. So switching forks **gains** the upstream 0.10/0.11/0.12
work and **loses nothing q2-specific** except the *shape* of the access-policy
API, which is deliberately replaced.

Two direct consumers, both native-only:

- `crates/quarto-hub` — the collaborative editing server (features `tokio`,
  `axum`, `tungstenite`). Owns `AuditAccessPolicy`.
- `crates/quarto-preview` — reads back binary docs (feature `tokio`).

The WASM crate (`crates/wasm-quarto-hub-client`) has **no** Rust samod/automerge
dependency, so the WASM build is unaffected. The browser/Node side uses JS
`@automerge/automerge ^3.2.6` + `@automerge/automerge-repo ^2.5.x`.

**Scope is exactly two `Cargo.toml`s.** Reverse-dep check: only `quarto-preview`
(deps `quarto-hub`) and the `quarto` binary (deps both) sit above these crates —
the WASM crate is *not* above them, so the hub-client/WASM leg is out of scope
(unless the JS-bump question pulls it in; see Phase 5 / open Q2). `quarto` and
`quarto-util` mention `samod` only in comments and tracing-filter *strings*
(`samod=info`), not as a dependency, so they need no change. A plain
`cargo build --workspace` covers the transitive rebuild of `quarto-preview` /
`quarto`.

### What actually changed in the API (verified against both trees)

**1. `AccessPolicy` trait: async → synchronous (the headline break).**

Old (`q2`, samod 0.9.0):
```rust
pub trait AccessPolicy: Clone + Send + 'static {
    fn should_allow(&self, doc_id: DocumentId, peer_id: PeerId)
        -> impl Future<Output = bool> + Send + 'static;
}
// also exported: LocalAccessPolicy (unused by us)
```

New (`access-policy`, samod 0.12.1):
```rust
pub trait AccessPolicy: Send + Sync + 'static {
    fn is_allowed(&self, doc_id: &DocumentId, peer_id: &PeerId) -> bool;
}
// blanket impl for Fn(&DocumentId, &PeerId) -> bool; struct AllowAll
```

Changes: method renamed `should_allow` → `is_allowed`; args now **borrowed**
(`&DocumentId, &PeerId`); returns **`bool` synchronously** (no future); bound
is `Send + Sync + 'static` (was `Clone + Send + 'static`); no `LocalAccessPolicy`.
The policy is now consulted *synchronously* at the hub's actor↔peer boundary
and **cannot perform async work** (DB / remote-auth lookups). Our
`AuditAccessPolicy` only locks a `Mutex`, reads a `HashMap`, and logs — all
synchronous already — so the port is mechanical.

**2. automerge `0.8.0` → `0.10.0` (spans two minor releases).** samod 0.12.x
pins `automerge = "0.10.0"`; our two crates declare a **direct** dep on
`automerge = "0.8.0"` (feature `utf16-indexing`) to share samod's types, so the
direct dep must move to `0.10.0` in lockstep. Direct automerge API touched (8
files): `AutomergeError`, `ReadDoc`, `Transactable`, `Value::Scalar`,
`ScalarValue::{Str,Bytes}`, `ROOT`, `ObjType`, `ObjId`, `ChangeHash`,
`Automerge::merge`, and `TextEncoding::{platform_default, Utf16CodeUnit}`
(tests only). The `TextEncoding` variants we use survive in 0.10 (the enum only
*gained* variants). The remaining 0.8→0.10 breaks are unknown until a compile
pass — let the compiler drive them.

### What is NOT breaking (verified present in the new fork)

`RepoBuilder` shape — `Repo::build_tokio()`, `.with_storage()`,
`.with_announce_policy(NeverAnnounce)`, `.with_access_policy(..)`, `.load()`;
`AnnouncePolicy` / `NeverAnnounce` / `AlwaysAnnounce` (still async, unchanged);
`Repo::make_acceptor` + `AcceptorEvent`; `Stopped`; `PeerInfo { storage_id:
Option<StorageId> }`; `StorageId::from`, `PeerId::from`, `DocumentId` +
`from_str`; `InMemoryStorage`, `storage::TokioFilesystemStorage`; `DocHandle`
methods `with_document`, `document_id`, `create`, `find`, `peers`, `changes`,
`broadcast`. The `with_access_policy(audit_policy)` **call site** in
`context.rs` needs no change — only the trait impl does.

### Highest-risk item: JS ↔ Rust interop & on-disk format

samod exists for JS `automerge-repo` interoperability. Bumping the Rust
automerge 0.8→0.10 must remain compatible with (a) documents already on disk in
the hub's `automerge_dir`, and (b) the JS automerge / automerge-repo versions
hub-client speaks over the wire. automerge's storage + sync formats are designed
to be stable, but this is a **must-verify**, not an assume. This is the one part
the Rust test suite cannot cover on its own.

## Work Items

> **Compile-unit note.** Phases 1–3 do not build in isolation: rewriting the
> tests to the new trait (Phase 1) breaks `quarto-hub` until the dep (Phase 2)
> and source (Phase 3) land. That is the intended migration "red" — it is
> compile-level, not a running-but-failing test. Land 1–3 together; the first
> green test run is Phase 4.

### Phase 0 — Isolated worktree + baseline

- [x] `cargo xtask create-worktree bd-qp353u2b` (base `main`). Worktree:
      `.worktrees/bd-qp353u2b-migrate-samod-quarto-devsamodq2/`. No `npm install`.
- [x] Green baseline on branch HEAD (old fork): `cargo build -p quarto-hub -p quarto-preview`
      clean; `cargo nextest run -p quarto-hub -p quarto-preview` → 360 passed, 1 skipped.
- [x] Interop baseline captured with the *current* (samod 0.9 / automerge 0.8) hub
      binary in standalone mode: `automerge_dir` with index doc
      `2J1DWLvMjrSn3KdfZe3VQosnQ3Sw` (incremental sha256
      `dc51f19e…a4fda2bc9`). JS versions: `@automerge/automerge ^3.2.6`,
      `@automerge/automerge-repo ^2.5.6` (hub-client) / `^2.5.1` (ts-packages).
      Pristine copy + README in the session scratchpad.

### Phase 1 — Tests first (TDD)

- [x] Rewrote the three `AuditAccessPolicy` tests to the sync trait:
      `policy.is_allowed(&doc_id, &peer_id)` (no `.await`), `#[tokio::test]` →
      `#[test]`, renamed `should_allow_*` → `is_allowed_*`.
- [x] Confirmed TDD "red": `cargo build -p quarto-hub --tests` failed with 3×
      `error[E0599]: no method named is_allowed` against the old dep.
- [x] Left `automerge_api_tests.rs` untouched as the compiler-driven checklist
      (it turned out to compile & pass unchanged — no automerge break surfaced).

### Phase 2 — Dependency bump

- [x] `crates/quarto-hub/Cargo.toml`: samod → `git = shikokuchuo/samod.git,
      branch = "access-policy", version = "0.12.1"`; `automerge` `0.8.0` → `0.10.0`
      (kept `utf16-indexing`). Pinned by **branch** (user decision). Added a
      one-line "vendored fork (long-term home)" comment.
- [x] `crates/quarto-preview/Cargo.toml`: same samod + automerge bump.
- [x] axum comment left as-is — new samod still uses axum `0.8.x`, so `axum 0.8`
      stays correct (workspace built clean; no axum resolution change).
- [x] `cargo update -p samod` re-resolved: automerge 0.8→0.10, samod 0.9→0.12.1,
      samod-core 0.9→0.12.0 (both at `c5a06c39`), added transitive `chacha20`,
      `rand 0.10.1`, `rand_core 0.10.1`.
- [x] Version unification confirmed: exactly **one** `automerge` entry in
      `Cargo.lock` (0.10.0); `cargo tree -p quarto-hub -i automerge` shows the
      direct dep and samod/samod-core share it. No dual-automerge red flag.

### Phase 3 — Migrate the source

- [x] `access_policy.rs`: `impl AccessPolicy` now `fn is_allowed(&self, doc_id:
      &DocumentId, peer_id: &PeerId) -> bool`; reads `.get(peer_id)`, logs on hit,
      returns `true`. Dropped `use std::future::Future;` and the `async { true }`.
      Kept `#[derive(Clone)]` (harmless; no longer required by the bound).
- [x] `context.rs`: `.with_access_policy(audit_policy)` type-checks unchanged
      (`quarto-hub` compiled clean) — no source change.
- [x] **No automerge 0.8→0.10 breaks materialized.** `quarto-hub` (lib + tests)
      and `quarto-preview` compiled clean against automerge 0.10 with zero edits
      to the 8 anticipated files. The automerge API surface these crates use
      (`ReadDoc`, `Transactable`, `ScalarValue`, `ROOT`, `ObjType`, `ChangeHash`,
      `Automerge::merge`, `TextEncoding::{platform_default,Utf16CodeUnit}`, …) is
      stable across 0.8→0.10 for our usage. The single hand-edit was the trait impl.

### Phase 4 — Rust test suite

- [x] `cargo nextest run -p quarto-hub -p quarto-preview` → 360 passed, 1 skipped
      (matches baseline). Targeted run: 3 access-policy + 24 automerge-API tests
      all pass.
- [x] `cargo nextest run --workspace` → **9855 passed, 197 skipped** (exit 0).
      No downstream regression.

### Phase 5 — End-to-end interop verification (the part tests can't do)

- [x] **On-disk backward-compat verified.** Started the NEW (automerge 0.10 /
      samod 0.12) `target/debug/hub` in standalone mode pointed at a copy of the
      Phase-0 `automerge_dir` written by the OLD binary:
      `QUARTO_HUB_DATA_DIR=<copy> ./target/debug/hub -P 3988 -H 127.0.0.1 -v`.
      Log: `Loaded existing index document doc_id=2J1DWLvMjrSn3KdfZe3VQosnQ3Sw`
      (the exact Phase-0 doc), and `hub.json`'s `index_document_id` unchanged. No
      re-init, no corruption.
- [x] **Live JS↔Rust wire interop verified (automated).** Ran the sync-client
      Rust-hub interop suite against the new binary from the worktree:
      `vitest run offline-creation-rust-hub restart-window-creation exit-drain`
      (ts-packages/quarto-sync-client) → **3 files / 5 tests passed**. These
      spawn the worktree's new `hub` and drive the real JS client
      (`@automerge/automerge 3.2.x` / `automerge-repo 2.5.x`): a JS-created
      project + file syncs over the wire and lands in the new hub's on-disk
      storage (`automerge/<shard>/<rest>`), proving both the wire and on-disk
      formats interoperate across automerge 0.8→0.10 / samod 0.9→0.12.
- [x] **Audit log verified (deterministic, in-repo).** Added a `tracing`-capture
      test `access_policy::tests::logs_document_accessed_with_email_for_known_peer`
      asserting the `"Document accessed"` line + email are emitted from the now
      synchronous `is_allowed`, and strengthened `no_log_when_peer_unknown` to
      assert the line is absent. The peer→email wiring (`server.rs:974`, on
      `AcceptorEvent::ClientConnected`) compiles clean against samod 0.12 and the
      accept path is exercised by the interop tests above.
      **Honest limitation:** the *live-auth* grep of a running hub's stdout was
      NOT performed — it needs either the embedded MCP bundle (a `PLACEHOLDER` in
      this fresh worktree) or the mock-OIDC bearer harness (`auth_bearer.rs`);
      both are unrelated to and out of scope for the samod/automerge migration.
      The `tracing::info!(… "Document accessed")` call itself is byte-identical
      pre/post migration (only the enclosing fn signature changed).

### Phase 6 — Full verification & handoff

- [x] `cargo xtask verify` → **✓ All verification steps passed!** The in-scope
      Rust legs are green at CI strictness: clippy `--workspace --all-targets
      -D warnings`, `cargo fmt --check`, `cargo build --workspace -D warnings`,
      tree-sitter grammar tests, and `cargo nextest run --workspace -D warnings`
      (**9856 passed, 197 skipped**). The JS/browser legs (hub-client build+tests,
      ts-packages build, shared preview-renderer/runtime, trace-viewer,
      q2-preview-spa, hub-mcp) were skipped — they are environmentally blocked in
      a fresh worktree (`wasm-quarto-hub-client` is not built, so every JS test
      leg fails with `Cannot find package 'wasm-quarto-hub-client'`) and are out
      of scope (the WASM crate does not depend on `quarto-hub`/`quarto-preview`).
      Verified this by a first run with only `--skip-hub-build`: the sole non-
      environmental failure was `intelligenceService.test.ts` (an LSP
      semantic-tokens parsing test, disjoint from samod/automerge) — pre-existing
      / unrelated to this Rust change.
- [x] `cargo xtask lint` → All checks passed (812 files).
- [x] Staged + committed as `9967abf9` on branch
      `braid/bd-qp353u2b-migrate-samod-quarto-devsamodq2` (worktree). Commit
      message calls out the automerge 0.8→0.10 bump + the sync AccessPolicy trait
      change. **Not pushed** — awaiting explicit approval (GIT PUSH POLICY).
- [x] Reported E2E interop result + `Cargo.lock` diff summary; push approval
      requested.

## Open questions — RESOLVED

1. **Pin to rev vs branch?** → **Branch** (`branch = "access-policy"`), per user
   decision. Cargo froze rev `c5a06c39…` in `Cargo.lock`. Mirrors the old `q2`
   branch-pin. (A force-push to `access-policy` moves the build on next
   `cargo update`; acceptable — it is the user's own fork.)
2. **JS package bump?** → **No.** Phase 5's live JS↔Rust round-trip passed with
   the *existing* JS versions (`@automerge/automerge ^3.2.6`,
   `@automerge/automerge-repo ^2.5.x`) against the automerge-0.10 / samod-0.12
   hub. Wire + on-disk formats interoperate; hub-client/ts-packages stay out of
   scope.
3. **Upstreaming intent** → **Long-term fork** (user decision). Dep comment reads
   "Vendored samod fork carrying the synchronous AccessPolicy trait (long-term
   home)."

## References

- New fork: `shikokuchuo/samod` branch `access-policy`, samod `0.12.1` /
  samod-core `0.12.0` / automerge `0.10.0`. Access-policy commit `c5a06c3`.
- Old fork: `quarto-dev/samod` branch `q2`, rev `0b50c16` ("Implement access
  policy" on samod `0.9.0` / automerge `0.8.0`).
- samod CHANGELOG (0.9→0.12): automerge 0.8→0.10; added `Repo::search` /
  `SearchState`; `Repo::find` hang-after-unavailable fix; exponential sync-time
  fix; `DocHandle::{changes,ephemeral}` now `'static`.
- Consumers: `crates/quarto-hub/src/{access_policy,context,server,sync,index,
  sync_state,resource,automerge_api_tests}.rs`,
  `crates/quarto-preview/src/capture_driver.rs`.

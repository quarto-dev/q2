# Ephemeral hub secrets for `q2 preview`

## Overview

`q2 preview` boots an in-process hub whose `data_dir` is a fresh `TempDir`
([preview.rs:93](../../crates/quarto/src/commands/preview.rs)). On every
invocation, `StorageManager::init`
([storage.rs:447](../../crates/quarto-hub/src/storage.rs)) finds no
`hub.json` secrets, so `resolve_server_secret` / `resolve_session_secret`
auto-generate, persist, and emit two `WARN` lines aimed at multi-instance
deployments:

```
WARN quarto_hub::storage: generated a new server secret and persisted it to hub.json
WARN quarto_hub::storage: generated a new session secret and persisted it to hub.json
```

The warning's premise ("now pinned to *this* data directory") never holds for
preview: the directory is deleted on exit, the server binds loopback only, and
`auth_config` is hardcoded `None`.

**Approach:** make ephemeralness explicit in the `quarto-hub` storage API
(new `*_ephemeral` constructors backed by a crate-private `SecretPolicy`),
and switch `quarto-preview` to it. The real hub server path is untouched.

## Work Items

### Phase 1 — Tests first (TDD)

Write the new tests against the not-yet-existing API and confirm they fail
(expected failure mode: compile error — the constructors don't exist yet).

- [x] Add tests in `crates/quarto-hub/src/storage.rs` test module (reuse
  `capture_logs`, `fresh_hub_dir`, `ENV_MUTEX` helpers):
  - `ephemeral_secrets_are_not_persisted_and_do_not_warn`: fresh dir →
    `new_standalone_ephemeral` → no WARN in captured logs; `hub.json` on
    disk has neither `server_secret` nor `session_secret`; both
    `manager.server_secret()` and `session_secret()` return 32 bytes.
  - `ephemeral_secrets_are_distinct`: server ≠ session (mirrors the
    existing persistent-path distinctness test).
  - `ephemeral_respects_env_override`: set `QUARTO_HUB_SERVER_SECRET` under
    `ENV_MUTEX` (existing `unsafe` + SAFETY-comment pattern) → manager uses
    it, no WARN, nothing persisted.
  - `ephemeral_generates_fresh_secrets_each_boot`: two sequential managers
    on the same dir (drop the first to release the lock) get different
    secrets; `hub.json` still clean.
- [x] `cargo nextest run -p quarto-hub storage` — confirm the new tests
  fail to compile (expected TDD failure for a new API). Confirmed:
  `error[E0599]: no associated function or constant named
  new_standalone_ephemeral found for struct storage::StorageManager`.

### Phase 2 — `quarto-hub` implementation (`crates/quarto-hub/src/storage.rs`)

- [x] Add private `fn generate_secret() -> [u8; 32]` (dedupes the
  `rand::rng().fill_bytes` boilerplate currently at ~lines 218-221 and
  259-262).
- [x] Add crate-private `enum SecretPolicy { Persist, Ephemeral }`.
- [x] Add `fn resolve_ephemeral_secret(env_var: &str) -> Result<[u8; 32]>`:
  env var via `decode_secret_hex` if set, else `generate_secret()`; emit a
  value-free `debug!` noting non-persistence.
- [x] Change `init` to take `secret_policy: SecretPolicy`; replace the two
  resolve calls (~lines 481-484) with a `match`: `Persist` → existing
  functions; `Ephemeral` → `resolve_ephemeral_secret(
  "QUARTO_HUB_SERVER_SECRET")` / `("QUARTO_HUB_SESSION_SECRET")`.
- [x] Existing constructors (`new`, `new_standalone`, `new_with_data_dir`)
  pass `SecretPolicy::Persist` — signatures unchanged, no caller churn.
- [x] Add public `new_standalone_ephemeral(data_dir)` and
  `new_with_data_dir_ephemeral(project_root, data_dir)` with rustdoc:
  secrets live only in memory, env vars still honored, intended for
  short-lived embedded hubs (preview); contrast with the persistent
  constructors' multi-instance warning.
- [x] Refactor `resolve_server_secret` / `resolve_session_secret` branch 3
  to call `generate_secret()` (no behavior change).
- [x] `cargo nextest run -p quarto-hub` — new and existing tests pass.
  (452 passed, including the 4 new ephemeral tests.)

### Phase 3 — Preview switch (`crates/quarto-preview/src/lib.rs`)

- [x] `build_storage` (lines 351-358): switch to
  `new_with_data_dir_ephemeral` / `new_standalone_ephemeral`; update the
  doc comment to note secrets are per-session and never persisted.
- [x] `cargo nextest run -p quarto-preview` — the `boot.rs` integration
  test exercises the switched path end to end. (86 passed, 1 pre-existing
  skip.)

### Phase 4 — Docs

- [x] Short note in `dev-docs/quarto-hub/session-auth-operations.md`
  (documents the multi-instance warning at ~line 146): embedded/short-lived
  hubs use the `*_ephemeral` constructors; secrets are per-process, env
  vars still honored, nothing persisted or warned.

### Phase 5 — Verification

- [x] `cargo build --workspace` — clean.
- [x] `cargo nextest run --workspace` (monorepo rule — do not stop at the
  modified crates) — **10881 passed, 0 failed**, 197 skipped.
- [x] `cargo xtask verify --skip-hub-build` (Rust-only change;
  `quarto-hub`/`quarto-preview` are not in the WASM/hub-client dependency
  chain, so the hub-build leg is not required) — all 14 steps passed.
- [x] End-to-end through the real binary (per CLAUDE.md, tests alone are
  not sufficient) — evidence below. (Ran `target/debug/q2` directly
  instead of via `cargo run`: the server must run in the background with
  a direct PID for signal delivery, and `cargo run` wraps the child.)

#### End-to-end evidence

**1. Preview, default ephemeral tempdir:**

```
$ target/debug/q2 preview examples/websites/01-minimal --no-browser

  q2 preview
  → http://127.0.0.1:52724/?page=index.qmd
```

Full boot log inspected: `grep -c 'generated a new'` → **0**; no WARN
lines at all.

**2. Preview with a persistent `--data-dir`:**

```
$ target/debug/q2 preview examples/websites/01-minimal --no-browser \
    --data-dir /tmp/q2-secret-check
```

WARN count **0**; `/tmp/q2-secret-check/hub.json` after shutdown
(inspected) contains no secret fields:

```json
{
  "version": 1,
  "created_at": "1786099964",
  "index_document_id": "3KAAwqQu4HBzML1RENRWboNCQ4Sf"
}
```

**3. Real hub server (control case — warning must survive):**

```
$ target/debug/q2 hub --no-project --data-dir /tmp/q2-hub-check
```

Both warnings fire and both secrets persist — `hub.json` keys:
`created_at, index_document_id, server_secret, session_secret, version`.
The multi-instance warning remains intact for actual hub servers.

## Details

### Design decisions

1. **Two new public constructors, existing ones unchanged.** All three
   existing constructors keep their signatures and delegate with
   `SecretPolicy::Persist`, so `hub.rs`, `main.rs`, and all hub tests are
   untouched. The policy is crate-private; the public surface is just the
   two constructors, which read clearly at call sites.
2. **Ephemeral semantics:** honor the env var if set
   (`QUARTO_HUB_SERVER_SECRET` / `QUARTO_HUB_SESSION_SECRET` — preserves
   the "env is always highest priority" invariant, still no I/O, no warn);
   otherwise generate fresh random 32 bytes. Never mutates
   `HubStorageConfig`, never saves `hub.json`, never warns.
3. **Preview always uses ephemeral** — no `PreviewConfig` field, no CLI
   change. Preview has no auth, binds loopback, and its data dir is
   per-session by default; even with `--data-dir`, pinned secrets buy
   nothing (crash resilience is about the automerge store; actor IDs are
   opaque to automerge).
4. **The two resolve functions keep their signatures** — their ~10 test
   call sites don't churn. The policy branch lives in `init`.

### Explicitly out of scope

- The `.gitignore` and `hub.lock` writes in `init` still happen in
  ephemeral mode (harmless in a tempdir; the lock is needed for
  `HubAlreadyRunning`).
- `new_with_data_dir` (persistent variant) becomes unused by the workspace
  after Phase 3 — kept as public API; removal is a separate decision.
- The two test-only `new_standalone` calls in
  `crates/quarto-preview/src/capture_driver.rs` (lines 519, 893) may
  optionally switch to ephemeral to keep test logs clean; not required.
- No changes to `q2 hub` / `main.rs` — real hub servers keep the current
  persist-and-warn behavior.

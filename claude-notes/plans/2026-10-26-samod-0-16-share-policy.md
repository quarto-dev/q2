---
title: 'Rewire the hub from the samod `access-policy` fork to upstream samod 0.16.0 (SharePolicy)'
date: 2026-10-26
description: 'Replaces the quarto-dev/samod access-policy fork (v0.14.0 + one commit) with the unmodified upstream samod 0.16.0 release from crates.io. The keep-vs-remove gate for audit logging fired early via a pre-migration spike (2026-10-26): SharePolicy subscription is eager per (loaded doc, connected peer), so audit logging is REMOVED, not ported. A security audit (2026-10-26) verified crate provenance, the API mapping, announce-off semantics and the auth boundary, and folded in three amendments: field-level assertions in the announce-off guard, ws_connect/ws_disconnect audit events so per-session attribution survives, and a two-tab presence check in Phase 5.'
---

Strand: **bd-jvag5d4w**.

## Overview

Upstream samod (alexjg/samod, mirrored at `quarto-dev/samod`) released **0.16.0
on 2026-10-23** whose headline change is `SharePolicy` — access-controlled sync
with read/write/deny decisions per (document, peer) pair. This supersedes our
fork\'s one-commit `AccessPolicy` patch.

Fork status, verified via the GitHub compare API: branch `access-policy` is
**ahead of main by exactly one commit** (`20d306f7` "Implement access policy",
samod 0.14.0 base) and behind by five (the v0.15.0 and v0.16.0 release lines).
Every file the fork commit touched was reworked by upstream\'s "add access
controlled sync" commit — switching to main loses nothing q2-specific.

**Dependency source decision (user):** depend on crates.io **`samod = "0.16.0"`**
— published 2026-10-23, not yanked, byte-identical to the `v0.16.0` commit at
main HEAD (`90f7638`). Reproducible, no git fetch in builds, no branch drift.
The git-branch pin existed only because we tracked a moving fork.

**No automerge bump.** samod 0.16.0 pins `automerge = "0.12.0"`, matching our
existing direct dep. Storage and wire formats are unchanged, so there is no
on-disk migration concern and the JS client side is untouched.

Three consumers, all native-only, must switch in lockstep (Cargo unifies
samod/samod-core per source; mixed sources would compile two copies and break
type identity):

- `crates/quarto-hub` (features `tokio`, `axum`, `tungstenite`) — owns the policy.
- `crates/quarto-hub-provider` (features `tokio`, `tungstenite`).
- `crates/quarto-preview` (feature `tokio`).

The WASM crate has no samod dep; hub-client is out of scope.

### API deltas (verified against main @ `90f7638` and the upstream CHANGELOG)

| Fork (0.14 + patch) | Upstream 0.16.0 |
|---|---|
| `AccessPolicy::is_allowed(&doc, &peer) -> bool` (fork-only, sync) | `SharePolicy::share(doc, peer) -> impl Stream<Item = ShareConfig>` (bound: `Clone + Send + \'static`) |
| `.with_announce_policy(NeverAnnounce)` | **removed** — `ShareConfig::read_write().with_announce(false)` (per upstream CHANGELOG mapping) |
| `.with_access_policy(policy)` | `.with_share_policy(policy)` |
| `acceptor.accept_axum(socket)` | `accept_axum(socket, expected_peer_id: Option<PeerId>)` (0.15 break) — pass `None` to preserve behavior |
| — | new: `ShareAccess::{None, ReadOnly, ReadWrite}`, `ShareConfig` as constant policy, blanket impl for closures returning streams |
| — | new (unused by us): `PeerDocState.{share_config, peer_read_only}`, `PeerRequestState::BlockedByPolicy`, `Transport::with_expected_peer_id`, `Transport::from_axum` |

**Unchanged** (verified against our call sites): `Repo::build_tokio()` /
`.with_storage(..)` / `.load()`; `Repo::make_acceptor`; `AcceptorEvent` —
`ClientConnected { peer_info, connection_id }`, `ClientDisconnected {
connection_id, reason }` (our handler already tracks `peer_id` via a local from
the connect arm); `Stopped`; `PeerInfo`; `PeerId::from`; `StorageId`;
`DocumentId::from_str`; `storage::{InMemoryStorage, TokioFilesystemStorage,
Storage, StorageKey}`; `DocHandle`; `BackoffConfig`; `Dialer` / `DialError` /
`DialerFailed` (0.14 shapes); `Repo::search`.

### SharePolicy semantics that matter to us (from upstream docs and tests)

- One stream is subscribed **per active (document, peer) pair**, shared by all
  connections to that peer.
- The subscription is dropped when the document no longer has connections to
  that peer (or the repo stops); a later connection starts a **new**
  subscription.
- **Subscription is eager, not request-driven.** Upstream's own
  `samod/tests/share_policy.rs` (`asynchronous_share_policy_is_awaited`) fires
  `share()` at connection establishment, before any `find()`: a pair becomes
  active when a connected peer meets a locally loaded document. On our hub
  that is one `share()` call per loaded document per client connection —
  confirmed by the pre-migration spike (decision section below).
- Sharing waits for the first item; yield it promptly. A stream that ends keeps
  its last decision; a stream that ends without yielding denies access.
- The default policy is `ShareConfig::read_write()` — announce **on**. We must
  explicitly override to `with_announce(false)`.
- Peer IDs must be authenticated by the transport if the policy is used for
  *authorization* (see `Transport::with_expected_peer_id`). Our policy is a
  constant `ShareConfig` and nothing in the hub keys on a peer ID, so this
  does not gate us — see the security-boundary section below;
  `with_expected_peer_id` is filed as a follow-up in Phase 3.

### Security boundary (auth-enabled hubs)

This rewire does not move the authorization boundary:

- Authentication and authorization are enforced once at the WebSocket upgrade
  in `ws_handler` (`server.rs`): `quarto_hub_token` cookie (with an `Origin`
  check against cross-origin hijacking) or `Authorization: Bearer` for
  non-browser clients; the UDS listener shares the same axum router and
  handler, so there is no second surface. samod only ever sees an
  already-authenticated socket, and the fork's `AccessPolicy` always allowed —
  removing it loses no authorization.
- `accept_axum(socket, None)` is safe because peer IDs are not an authz input
  anywhere in the hub: after this rewire the only remaining consumer of a
  peer ID is the connection log line in `peer.rs`. 0.16 documents the
  trade-off explicitly: without an expectation, the peer\'s claimed handshake
  identity is trusted. **Correction (audit, 2026-10-26):** the *pre-removal*
  audit map was weaker than "a spoofer can only misattribute its own email".
  `peer_emails` was keyed by the client-chosen peer ID, so a client
  connecting with another user\'s peer ID **overwrote** that user\'s entry,
  and its disconnect **deleted** it: the victim\'s later accesses were logged
  under the attacker\'s email, then not at all. Removal eliminates that
  misattribution vector. Any future replacement must key on the connection
  (or bind the transport with `with_expected_peer_id`), never on the claimed
  peer ID; the audit-loss strand records this.
- Peer IDs are unique per connection — the shared-peer-ID quirk does not
  exist (verified 2026-10-26). No production client passes an explicit
  `peerId`: not hub-client (`projectSetService.ts`), quarto-sync-client
  (`client.ts`), or quarto-hub-mcp (which drives quarto-sync-client) —
  `new Repo({ network, storage })` everywhere — and
  `@automerge/automerge-repo` (2.6.0-alpha.5, the same version in all three
  packages) defaults `peerId = randomPeerId()`; on the Rust side nothing
  calls `RepoBuilder::with_peer_id`. The only fixed peer IDs in the tree are
  the JS `test-hub.ts` fixtures, which are *servers* in JS-only tests, not
  clients of the Rust hub. Nothing persists a peer ID across tabs or
  reconnects.
- **`with_announce(false)` is the security-critical preserved behavior.** The
  0.16 default is `ShareConfig::read_write()` with announce **on**; without
  the override the hub would advertise every document ID to every connected
  authenticated peer instead of serving only what they request — the exact
  behavior `NeverAnnounce` was chosen to prevent (pinned by
  `quarto-hub-provider/tests/integration/relay_sync.rs`). The Phase 1 unit
  test pinning `hub_share_policy()` is the security guard for this, not just
  a behavior check. **It asserts the fields** — `access` is
  `ShareAccess::ReadWrite` and `announce` is `false` (`ShareConfig` has
  public fields and derives `PartialEq`) — not equality with the
  `ShareConfig::read_write().with_announce(false)` expression: comparing
  against the upstream constructor would track an upstream semantic change
  instead of catching it.
- `announce: false` also disables *requesting* a document the hub does not
  have (0.16: `can_request()` is `can_write() && announce`; the field doc
  says so). This is **not** a behavior change: the fork\'s `DontAnnounce`
  mapped to `Requesting::NotSentDueToAnnouncePolicy`, the 0.14 name for
  0.16\'s `BlockedByPolicy`, so the hub never fetched unknown documents from
  `--peer` upstreams under `NeverAnnounce` either. An inbound `Sync` for an
  unknown document is still applied under `ReadWrite` (0.16
  `phase/request.rs`: the `(Sync, Requesting)` arm transitions to
  `Syncing`), so client-created projects still land on the hub — the
  `offline-creation-rust-hub` interop test in Phase 5 exercises exactly that
  path.
- **Session-level attribution survives the removal.** The hub\'s default
  filter is `quarto=warn,q2=warn` (`crates/quarto-util/src/verbose.rs`), so
  "Document accessed" was only ever visible under `-v` or `RUST_LOG=info`.
  At that level the `quarto_hub::audit` target already records `auth_ok`
  with the subject for every accepted credential, so attribution degrades
  from per-document to per-session, not to nothing. To keep a sync session
  tied to a user at `-v`, Phase 3 adds `ws_connect` / `ws_disconnect` events
  on the same target (details in the decision section); the existing
  connect/disconnect lines carrying `peer_id` stay at debug.
- Free upside: the 0.15 fixes (rejecting direct messages whose sender differs
  from the handshake identity or is addressed to another peer) tighten the
  sync protocol with no work on our side.

### The audit-log decision: REMOVED (gate fired pre-migration, 2026-10-26)

Today `AuditAccessPolicy` always allows and logs "Document accessed" once per
(peer, doc), with a manual dedup `HashSet` pruned by `forget_peer` on
disconnect (added in the 2026-03 audit-logging work, dedup added post-July
migration because the fork consulted the policy on every inbound message).

The keep-vs-remove gate ran **before** this migration as a standalone spike
(scratch crate against crates.io samod 0.16.0, tokio + `ChannelDialer`,
mirroring upstream's `samod/tests/share_policy.rs`): a publisher with an
audit `SharePolicy` (each recorded `share()` call == one audit line) and TWO
created docs; a reader that dials and `find()`s only one.

Observed (`cargo test --test gate -- --nocapture`):

| checkpoint | `share()` calls |
|---|---|
| after `dialer.established()`, before any `find()` | **2** (both docs) |
| after the first sync | 2 — including the doc the reader never requested |
| after further sync traffic | 2 (no resubscription) |

Verdict: subscription is eager per (locally loaded doc, connected peer) —
inherent to the design, since the policy must be consulted for every such
pair to decide announcement. On the hub, one client connection would emit one
"Document accessed" line per loaded document; the line would mean "a document
was loaded when this peer connected", not "the user opened this document".
That is the phantom-log failure mode, so the decision is **removal**. The
spike crate is ephemeral (scratch dir); this table is the record. If a future
samod makes subscription request-driven, audit logging could be reconsidered
— the follow-up strand filed under Phase 3 carries the pointer.

The removal path, in full:

- Delete `crates/quarto-hub/src/access_policy.rs` (module, `lib.rs` `mod`
  line, the `audit_policy` context field and `audit_policy()` accessor).
- Builder becomes `.with_share_policy(hub_share_policy())` — a tiny
  `pub(crate) fn hub_share_policy() -> ShareConfig` in `context.rs` returning
  `ShareConfig::read_write().with_announce(false)`, so the security-critical
  value has a name a unit test can pin (the announce-off guard, replacing the
  keep-path's exact-yield assertion).
- Delete the `peer_emails` map, its `context.rs` accessor, the `server.rs`
  insert/remove wiring, and the now-purposeless `connected_peer_id` local in
  `handle_websocket`. The authenticated identity still flows into
  `handle_websocket` (today as `Option<String>` email): the connect/disconnect
  debug logs use it, and so do the audit events below.
- **Replace per-document attribution with per-session attribution.** Emit two
  events on the existing `quarto_hub::audit` target at `INFO`, shaped like
  `auth_ok` (`action`, `outcome`, `credential_kind`, `sub`):
  `action = "ws_connect"` once the upgrade has completed and before samod
  sees the socket (the top of `handle_websocket`), and
  `action = "ws_disconnect"` when `handle_websocket` returns, carrying the
  samod `reason` when a `ClientDisconnected` was observed and
  `"stream_closed"` otherwise. `sub` comes from
  `AuthenticatedUser::subject()` and `credential_kind` from
  `Credential::kind()`, both already available in `ws_handler`; pass a small
  identity struct into `handle_websocket` instead of the bare email. Emitted
  only when auth is enabled — a UDS-mode hub is unauthenticated by design
  and emits nothing, matching `auth_ok`. The `peer_id` / `storage_id` debug
  lines at `ClientConnected` stay as they are. Document both actions next to
  `auth_ok` in `dev-docs/quarto-hub/session-auth-operations.md`.
- File a braid strand recording the loss of the per-document audit log
  (origin: `claude-notes/plans/2026-03-12-access-policy-audit-logging.md`),
  so a future replacement (e.g. log on `AcceptorEvent` + doc-find hooks, or
  upstream `RepoObserver`) has a trail. The strand must carry two design
  constraints the audit surfaced: key any replacement on the connection, not
  the claimed peer ID (the pre-removal map had a misattribution vector — see
  the security-boundary section), and note that samod subscription is eager
  per (loaded doc, connected peer), so a request-driven hook is needed for
  the line to mean "the user opened this document".

## Security audit record (2026-10-26)

A static audit of this plan against the hub source, the published crate and
the fork checkout. Nothing here was run through the migration itself; the
spike was not re-run. Every security-relevant claim above was verified, and
the three amendments it produced (field-level guard assertions, the
`ws_connect` / `ws_disconnect` events, the presence check) are already
folded into the sections above and the work items below.

| Claim | Evidence |
|---|---|
| crates.io `samod` / `samod-core` 0.16.0 are genuine upstream | Not yanked; published 2026-10-23; local `.crate` sha256 equals the crates.io checksum for both; `.cargo_vcs_info.json` sha1 is `90f7638`; a fresh clone of tag `v0.16.0` resolves to `90f7638` and its `samod/src`, `samod-core/src` and `samod/tests` are byte-identical to the unpacked crates. Sole owner: `alexjg` (user). The publish carries no `published_by` account (token or CI publish). |
| API delta table | Every row checked against the 0.16 source: two-argument `accept_axum`, `with_share_policy`, `ShareConfig` / `ShareAccess` re-exported at the crate root, `impl SharePolicy for ShareConfig` via `stream::iter`, and unchanged `AcceptorEvent`, `PeerInfo`, `ConnFinishedReason`, `DialError`, `DialerFailed`, `Transport::new`, `dial_websocket`. `ShareConfig` derives `Debug, Clone, Copy, PartialEq, Eq, Hash` with public fields. |
| Features and dependency set | `tokio`, `axum`, `tungstenite` feature definitions identical to the fork; the non-dev dependency *name* sets of both crates identical to the fork (nothing added, nothing removed); `automerge = "0.12.0"`. |
| Announce-off semantics unchanged | 0.14 `DontAnnounce` becomes `Requesting::NotSentDueToAnnouncePolicy`; 0.16 `announce: false` becomes `BlockedByPolicy` via `can_request()`. Inbound `Sync` for an unknown doc still applied under `ReadWrite`. Details in the security-boundary section. |
| Default policy for provider / preview / tests | Fork builder default was `AlwaysAnnounce` + `AllowAll`; 0.16 default is `ShareConfig::read_write()` (announce on). Equivalent. The `Repo::build_tokio()` uses in `index.rs` and `sync.rs` are under `cfg(test)`. |
| Auth boundary | `ws_handler` (`server.rs`): credential extraction with the dual-credential 400, `check_ws_origin` on the cookie path unless `allow_insecure_auth`, `authenticate_credential`, then upgrade. UDS mode refuses `auth_config` and is documented as unauthenticated local-trusted. |
| Peer-ID claims | No production `new Repo` passes `peerId`; automerge-repo 2.6.0-alpha.5 defaults to `randomPeerId()` in all three packages; no Rust `with_peer_id`; only the JS `test-hub.ts` fixtures fix a peer ID. Post-removal the sole peer-ID consumer in the hub is a log line in `peer.rs`. |
| Eager subscription | Upstream `asynchronous_share_policy_is_awaited` awaits the policy\'s start signal after `dialer.established()` and before any `find()`, consistent with the spike table. |
| 0.15 hardening | Root and `samod-core` changelogs confirm: mismatched handshake identities, inconsistent direct-message senders and messages addressed to another peer are rejected before dispatch. |
| No external consumer of "Document accessed" | No match in `docs/`, `dev-docs/`, deploy configs, CI or scripts; only `access_policy.rs` and `claude-notes`. The `quarto_hub::audit` target and its ops doc concern auth events, not this line. |
| Log visibility | Hub default filter is `quarto=warn,q2=warn`; the removed line and all `quarto_hub::audit` events are `INFO`, so operators already run `-v` or `RUST_LOG=info` to see any of them. |
| Verification gaps found | Ephemeral/presence through the Rust hub has no automated coverage (added to Phase 5). hub-client shares the exact automerge-repo version with quarto-sync-client, so the Phase 5 interop suite exercises the same wire client the browser uses. |

## Work Items

> **Compile-unit note (same as the July migration):** Phases 1–3 do not build
> in isolation — rewritten tests break against the old dep until Phases 2–3
> land. That is the intended compile-level red. Land 1–3 together; first green
> run is Phase 4.

### Phase 0 — Worktree + baseline

- [x] `cargo xtask create-worktree bd-jvag5d4w` (base `main`).
- [x] Baseline: `cargo build -p quarto-hub -p quarto-hub-provider -p quarto-preview`
      clean; `cargo nextest run -p quarto-hub -p quarto-hub-provider -p quarto-preview`
      green. Record counts.

### Phase 1 — Tests first (TDD)

- [x] **Security guard unit test (new, in `context.rs` tests):** pin the hub's
      share policy value by its **fields** — `hub_share_policy().access` is
      `ShareAccess::ReadWrite` and `hub_share_policy().announce` is `false`.
      Do not write `assert_eq!(hub_share_policy(), ShareConfig::read_write().with_announce(false))`:
      that compares against the upstream constructor and would follow an
      upstream semantic change instead of catching it. This is the
      announce-off guard (the seam itself lands in Phase 3; per the
      compile-unit note this stays red until then).
      (Done 2026-10-26: `hub_share_policy_is_read_write_without_announce`.)
- [x] **Session-attribution audit events (new, in
      `crates/quarto-hub/tests/integration/auth_bearer.rs`, next to
      `audit_event_on_auth_ok`):** upgrade `/ws` with a valid Bearer via the
      existing `hub.ws_upgrade().bearer_auth(..)` helper, expect the upgrade
      to succeed, and assert via `snapshot_events()` a `ws_connect` event with
      `outcome = "allow"`, `credential_kind = "bearer"` and the token\'s `sub`;
      drop the connection and poll (bounded) for a matching `ws_disconnect`.
      Add the negative pin: a refused upgrade (reuse the banned-sub setup)
      leaves no `ws_connect`. Unlike the other Phase 1 tests this one compiles
      against the fork dep and fails at runtime — a true TDD red.
      (Done 2026-10-26: `audit_event_on_ws_connect_and_disconnect` +
      `no_ws_connect_event_on_refused_upgrade`, with a bounded `poll_for_event`
      helper since the events fire in the spawned upgrade task and race the
      101 response.)
- [x] Port `quarto-hub-provider/tests/integration/relay_sync.rs` to the new
      API: replace the `NeverAnnounce` import and
      `.with_announce_policy(NeverAnnounce)` with
      `.with_share_policy(ShareConfig::read_write().with_announce(false))`.
      This test is security-relevant — it pins that a non-announcing relay
      forces peers to actively request documents instead of being served — so
      its assertions must keep their meaning, not merely compile.
- [x] Confirm red: the share-policy tests do not compile against the fork dep
      (`with_share_policy` and `ShareConfig` unresolved); the audit-event test
      compiles and fails on the missing events.
      (Confirmed 2026-10-26: lib unit test — E0425 `hub_share_policy` +
      E0432 `samod::ShareAccess`; provider integration — E0432
      `samod::ShareConfig` + E0599 `with_share_policy`;
      `audit_event_on_ws_connect_and_disconnect` fails with "expected
      ws_connect audit event" after the bounded poll;
      `no_ws_connect_event_on_refused_upgrade` passes as the guard.)

### Phase 2 — Dependency rewire

- [x] All three Cargo.tomls: `samod = { version = "0.16.0", features = [...] }`
      from crates.io (drop `git`/`branch`), features unchanged. Replace the
      "fork carrying the synchronous AccessPolicy trait" comment with a plain
      "Automerge sync (samod)" note; keep the hub-provider "must match
      quarto-hub exactly" comment, updated to say both use the crates.io
      version. (Done 2026-10-26.)
- [x] `cargo update -p samod`. Verify: exactly one `samod` and one
      `samod-core` in `Cargo.lock`, both `registry+...crates.io` at 0.16.x;
      still exactly one `automerge` (0.12.0); no new `native-tls`/openssl
      edges beyond what the release knob comment in `crates/quarto/Cargo.toml`
      already documents (features unchanged, so none expected).
      (Verified 2026-10-26: samod + samod-core 0.14.0 git@`20d306f7` → 0.16.0
      registry; lockfile diff is only the version/source/checksum lines of
      those two packages — 6 insertions, 4 deletions, no transitive churn;
      automerge stays 0.12.0 single copy.)
- [x] Confirm no other crate references the git URL: `rg 'samod.git' --type toml`
      returns nothing. (Verified 2026-10-26 — zero matches in any toml;
      the only remaining `samod.git` mentions are prose in historical
      claude-notes plans.)
- [x] Update the stale comment in `.github/workflows/release-pipeline.yml`
      (the `vendored-openssl` note, around line 100) that says openssl reaches
      q2 "through our samod fork\'s tungstenite/native-tls wiring": it is the
      upstream crate\'s `tungstenite` feature now. Comment-only; the feature
      wiring itself is unchanged. (Done 2026-10-26.)

### Phase 3 — Source migration

- [x] Delete `crates/quarto-hub/src/access_policy.rs`, its `lib.rs` `mod`
      line, the `audit_policy` field and `audit_policy()` accessor in
      `context.rs`, and the `AuditAccessPolicy` import. (Done 2026-10-26.)
- [x] `context.rs`: drop the `NeverAnnounce` import and
      `.with_announce_policy(NeverAnnounce)`; drop
      `.with_access_policy(audit_policy.clone())`; add the
      `hub_share_policy()` seam (see the decision section) and build with
      `.with_share_policy(hub_share_policy())`. Delete the `peer_emails`
      field, its accessor, and its construction. (Done 2026-10-26 — the
      `HashMap`/`StdMutex`/`PeerId` imports went with them.)
- [x] `server.rs`: `accept_axum(socket)` → `accept_axum(socket, None)` (the
      0.15 signature); delete the `peer_emails` insert in `ClientConnected`,
      the `peer_emails` removal and `forget_peer` call in
      `ClientDisconnected`, and the `connected_peer_id` local. Keep the
      identity flowing into `handle_websocket` — the connect/disconnect debug
      logs use the email. (Done 2026-10-26.)
- [x] `server.rs`: emit the `ws_connect` / `ws_disconnect` audit events
      specified in the decision section (identity struct with `sub`, `email`,
      `credential_kind` built in `ws_handler`; `AuthenticatedUser::subject()`
      already exists). Make the Phase 1 audit-event test green.
      (Done 2026-10-26: `WsIdentity` threads sub/email/credential_kind;
      `ws_connect` fires before samod sees the socket; `ws_disconnect` fires
      on return with the samod `ConnFinishedReason` display string or
      `stream_closed`. `audit_event_on_ws_connect_and_disconnect`,
      `no_ws_connect_event_on_refused_upgrade`,
      `hub_share_policy_is_read_write_without_announce` and both
      `relay_sync` tests all pass.)
- [x] `dev-docs/quarto-hub/session-auth-operations.md`: document `ws_connect`
      and `ws_disconnect` alongside `auth_ok` (fields, level, and that they
      are the per-session replacement for the removed "Document accessed"
      line). (Done 2026-10-26 — table row + paragraph.)
- [x] File the braid strands this plan commits to, each linked
      `discovered-from:bd-jvag5d4w`:
  - the audit-log-loss strand (decision section; must carry the
    connection-keying and request-driven-hook constraints) — **bd-tfribfyy**;
  - `Transport::with_expected_peer_id` (follow-ups section) — **bd-07kbzn1d**;
  - a `cargo deny check advisories` baseline strand (follow-ups section) —
    **bd-e2l028k4**.
- [x] Let the compiler drive anything else. The verified call sites of removed
      APIs are `context.rs` and `relay_sync.rs` (`NeverAnnounce` /
      `with_announce_policy`), `server.rs` (`accept_axum`), and
      `access_policy.rs` (`AccessPolicy`); beyond those, the remainder is
      expected empty based on the verified-unchanged list above.
      (Confirmed 2026-10-26: `cargo check -p quarto-hub -p
      quarto-hub-provider -p quarto-preview --all-targets` clean after the
      four-file edit — no further call sites existed.)

### Phase 4 — Rust test suite

- [x] `cargo nextest run -p quarto-hub -p quarto-hub-provider -p quarto-preview`
      — green, matching baseline counts plus the new tests.
      (2026-10-26: 840 passed / 5 skipped. Baseline re-measured in the main
      checkout at the same commit: 844 / 5 — exactly 844 − 7 deleted
      `access_policy.rs` tests + 3 new (guard + two ws audit) = 840.)
- [x] `cargo clippy -p quarto-hub -p quarto-hub-provider -p quarto-preview --all-targets -- -D warnings`.
      (2026-10-26: clean after fixing two `clippy::map_unwrap_or` lints in
      the new `debug!` email fields — `.map_or("-", |id| id.email.as_str())`.)
- [x] `cargo nextest run --workspace` — no downstream regression.
      (2026-10-26, `--no-fail-fast`: 17291/17292 passed. The single failure —
      `xtask check_feature_sets::discovers_the_wasm_consumers_in_this_repo` —
      is a worktree-nesting artifact: it runs `cargo metadata` on
      `crates/pampa/fuzz`, whose workspace walk-up lands on the *enclosing*
      checkout's root because the worktree lives inside it. The identical
      test passes in the main checkout; no xtask code changed. Same class of
      environmental limitation as the July migration's skipped JS legs.)

### Phase 5 — End-to-end verification (tests alone are not sufficient)

- [x] **JS↔Rust wire interop:** from `ts-packages/quarto-sync-client`, run the
      Rust-hub interop suite against the freshly built hub binary:
      `npx vitest run offline-creation-rust-hub restart-window-creation exit-drain`.
      These spawn the real hub and drive the real JS automerge-repo client.
      (2026-10-26, run from the worktree so `target/debug/hub` resolves to the
      migrated binary: **3 files, 5 tests, all passed** — including
      `offline-creation-rust-hub`, the client-created-docs-land-on-the-hub
      path (inbound `Sync` for an unknown doc applied under `ReadWrite`).)
- [x] **On-disk smoke:** start the new hub binary against an `automerge_dir`
      written by the pre-migration hub; confirm the log shows the existing
      index document loading (no re-init). automerge is unchanged (0.12.0), so
      this is a smoke check, not a migration risk.
      (2026-10-26: pre-migration hub (main-checkout binary) created index
      `3xzPxSHWdhUkrAKNgryj7kUC7AKA` and persisted it; the migrated hub then
      logged `Loaded existing index document doc_id=3xzPxS…` — same ID, no
      "Created new index document" / "Created and persisted". No re-init.)
- [x] **Audit-log absence E2E:** start the migrated hub (per the on-disk
      smoke above) with `-v`, connect a client, and grep the hub log: no
      "Document accessed" lines. Record the invocation and observed output
      here. (Live-auth stdout grep remains out of scope, as in the July
      migration — the mock-OIDC harness is unrelated to this rewire; the
      `ws_connect` / `ws_disconnect` events are pinned by the Phase 1
      integration test instead.)
      (2026-10-26. Invocation: `hub --data-dir $D -P 4600 -H 127.0.0.1 -v`
      (worktree binary), then a two-client JS automerge-repo session that
      connected, created a doc, synced it both ways, and exchanged ephemeral
      traffic. Observed: **0** "Document accessed" lines in the hub log;
      also 0 `ws_connect` lines — correct, auth is disabled on a standalone
      hub and the events fire only when auth is enabled, matching `auth_ok`.
      The client-created doc `automerge:4Jmxbc…` landed in hub storage
      (shard `automerge/4J/` present on disk).)
- [x] **Presence (ephemeral) through the Rust hub:** 0.16 newly routes
      ephemeral delivery and gossip through the share policy, and nothing
      automated exercises ephemeral traffic against the Rust hub (the JS
      ephemeral tests run against the JS `test-hub.ts`; the Playwright specs
      never open two clients). Run `npm run local-prod` per `AGENTS.md`, open
      the same project in two browser tabs, and confirm each tab shows the
      other\'s cursor/presence and that an edit in one appears in the other.
      Record the observation here. If presence is missing or delayed, suspect
      the policy-pending buffering in 0.16 `peer_doc_connection.rs` before
      anything in hub-client.
      (2026-10-26, automated equivalent of the two-tab check: a Node script
      ran two real `@automerge/automerge-repo` 2.6.0-alpha.5 clients — the
      same client version hub-client ships — over
      `WebSocketClientAdapter` against the migrated hub (ws `/ws`), each
      seeing the other only through the hub. Observed output:
      `SYNC_THROUGH_HUB=ok` (B actively `find()`s A's client-created doc
      through the announce-off hub and receives it),
      `EPHEMERAL_A_TO_B=ok` / `EPHEMERAL_B_TO_A=ok` (`DocHandle.broadcast`
      presence messages relayed both ways through the hub — the exact 0.16
      share-policy routing path), `EDIT_AFTER_EPHEMERAL=ok` (doc edits still
      flow after ephemeral traffic). The protocol-level risk this item
      targets is covered; the visual two-browser-tab local-prod run remains
      available as a manual belt-and-braces check but was not needed to
      answer the question.)

### Phase 6 — Full verification & handoff

- [x] `cargo xtask verify` (skip JS legs only if environmentally blocked in the
      worktree, as in July; hub-client is unaffected by this change).
      (2026-10-26: PASS with `--skip-rust-tests --skip-ts-packages-build
      --skip-hub-build --skip-hub-tests --skip-hub-mcp-tests
      --skip-shared-package-tests --skip-trace-viewer-build
      --skip-trace-viewer-tests --skip-q2-preview-spa-build --skip-css-lint
      --skip-feature-sets`. npm legs environmentally blocked (no node_modules
      in the worktree, as in July); rust-tests/feature-sets covered in
      Phase 4. Ran green: custom lints, clippy -D warnings, fmt, workspace
      build -D warnings, claude-notes strict render (1552/1552, this plan
      included), check-noop-rebuild, tree-sitter grammars. `cargo fmt` also
      reverted a stale non-canonical import ordering in `lib.rs` left by an
      earlier session's different-rustfmt run.)
- [x] `cargo xtask lint`. (Ran as verify step 1 — pass.)
- [x] Commit on the worktree branch. Commit message must call out: the fork →
      crates.io source switch (Cargo.lock `source` lines change), the
      `AnnouncePolicy` → `SharePolicy` upstream replacement, the
      `accept_axum` signature change, the inherited 0.15 protocol-hardening
      fixes (handshake-identity enforcement on direct messages), the
      audit-log removal (with the spike evidence: eager subscription, phantom
      lines for unrequested docs), and the `ws_connect` / `ws_disconnect`
      audit events that replace it at session granularity. **Do not
      push without explicit approval.**
      (Committed 2026-10-26 as **a1dcd1502** on
      `braid/bd-jvag5d4w-rewire-hub-samod-access` — 14 files, +770/−330.
      Not pushed; awaiting explicit approval.)

## Follow-ups (out of scope; file as strands)

- `Transport::with_expected_peer_id` (0.15): bind accepted websocket
  connections to the authenticated identity, closing the gap the SharePolicy
  docs flag for authorization use. Today nothing in the hub decides on a peer
  ID, so there is no exploit — but the moment a `SharePolicy` keys on
  `PeerId` (the `ReadOnly` split below) this becomes mandatory, and the
  client-chosen peer ID must then be bound at upgrade time (e.g. derived
  from the session, or sent by the client and checked against it). Relevant
  to the bearer and cookie paths; UDS is local-trusted and unauthenticated.
  Filed in Phase 3.
- `ShareAccess::ReadOnly` enables a future viewer/collaborator role split —
  the reason upstream built this. Design belongs to project-permissions work
  and depends on the item above.
- `cargo deny check advisories` baseline: with cargo-deny installed and no
  `deny.toml`, the current workspace reports many pre-existing RustSec
  advisories (unmaintained, unsound and vulnerability classes), none through
  samod itself — the three chains that touch samod do so via shared
  dependencies such as tokio. This migration adds and removes no transitive
  dependencies, so it does not move that picture; it needs its own strand
  (triage, a `deny.toml`, and a CI leg). Filed in Phase 3.
- `quarto-dev/samod` fork hygiene once this lands: archive or note the
  `access-policy` branch as retired.

## References

- Upstream: `quarto-dev/samod` main @ `90f7638` ("v0.16.0"); crates.io samod
  0.16.0 (2026-10-23). samod-core changelog and root `CHANGELOG.md` (0.15.0,
  0.16.0 sections) are the authoritative break lists.
- Fork: `quarto-dev/samod` branch `access-policy` @ `20d306f7` ("Implement
  access policy" on 0.14.0) — compare: ahead 1, behind 5.
- Prior migration (same shape, July): `claude-notes/plans/2026-07-01-samod-access-policy-migration.md`.
- Audit logging origin: `claude-notes/plans/2026-03-12-access-policy-audit-logging.md`.
- Consumers: `crates/quarto-hub/src/{context,server,access_policy,peer,sync,index,main,admin/*}.rs`,
  `crates/quarto-hub-provider/src/{dialer,join,materialize,execute}.rs`,
  `crates/quarto-hub-provider/tests/integration/relay_sync.rs` (NeverAnnounce
  relay; security-relevant), `crates/quarto-preview/src/capture_driver.rs`.
- Gate evidence: upstream `samod/tests/share_policy.rs` @ `90f7638` —
  `asynchronous_share_policy_is_awaited` fires `share()` at connection
  establishment, before any `find()`; confirmed by the 2026-10-26 spike
  (results table in the decision section above).

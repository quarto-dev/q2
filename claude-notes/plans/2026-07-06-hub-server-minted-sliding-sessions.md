# Hub server-minted sliding sessions

**Status:** proposed
**Date:** 2026-07-06
**Strand:** `bd-ey6jg70f` (use as the epic)
**Part of:** the auth-reshape path — see the umbrella index `claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md`. Independent of the other two plans (connection-gated local-first; Part B PKCE unification), except that Part B's "retire One-Tap" phase soft-depends on this one.

## Overview

Today the hub is a **stateless OIDC resource server**: the `quarto_hub_token`
HttpOnly cookie's value *is* the Google ID token (1 h `exp`), re-validated
against Google's JWKS on every request. The hub mints/signs nothing. This has
two independent failure modes:

1. **Renewal depends on Google One-Tap**, which is unavailable in many
   environments (FedCM / third-party-cookie policies / unregistered origins →
   `gsi/status 403`). When the 1 h token expires and One-Tap can't renew, every
   WS upgrade 401s and the SPA presents as permanently offline. This *is* the
   root cause of the closed bug `bd-3o8zmz46`, which shipped only a **tactical**
   client-side mitigation (surface `exp`, schedule refresh, two-strike probe)
   and explicitly deferred the structural fix to this strand.
2. **Large IdP tokens can be silently dropped.** `build_auth_cookie` warns that
   a token >3800 bytes may exceed the ~4096-byte cookie limit and be silently
   dropped by the browser (`server.rs:292-298`) — presenting, again, as "not
   logged in."

**Goal:** validate the Google token **once** at login, then mint a **hub-signed,
compact, HttpOnly session cookie with sliding expiry**. The hub re-issues the
cookie on activity, so sessions outlive Google's 1 h token without One-Tap, and
the cookie is small. Bearer/JWKS validation (the MCP path) is untouched.

### Why this is a separate plan (not a Part-B phase)
- **Orthogonal to the SPA→PKCE migration.** PKCE changes how the browser
  *acquires* a Google ID token; sliding sessions change what the hub *does with*
  it. Neither requires the other (2026-07-06 coupling investigation).
- **Standalone production value.** It fixes `bd-3o8zmz46`'s root cause and the
  >3800-byte drop regardless of PKCE or local-first.
- **New security-critical surface.** It adds the hub's **first token-minting
  capability** and (in the revocable variant) its **first session store**, on
  the credential path — it merits a focused design + security review.

## Current state (map for implementers)

### The hub signs nothing today, but the primitive + secret pattern exist
- Validation-only: `jsonwebtoken = "10"` + `axum-jwt-auth = "0.6"` used only for `RemoteJwksDecoder` *decode* (`auth.rs:344`, `:612-618`). `EncodingKey` (the mint primitive) appears **only** in `tests/auth_bearer.rs` (mock IdP), never in production. No `client_secret`, no `oauth2`/`openidconnect` client.
- **HMAC-SHA256 is already in use and shipped:** `sub_to_actor_id_for_project` (`auth.rs:729-737`); deps `hmac`, `sha2`, `rand` already present.
- **Secret lifecycle precedent:** `resolve_server_secret` (`storage.rs:179-197`) resolves a 32-byte secret via env `QUARTO_HUB_SERVER_SECRET` → `hub.json` `server_secret` → auto-generate + persist (mode `0o600`, `storage.rs:115-135`). Cached in `StorageManager.server_secret` (`storage.rs:232,317,339`), surfaced via `ctx.server_secret_bytes()` (`context.rs:388`). **Survives restarts; no rotation mechanism (no `kid`/versioning).**

### No session store today
- No DB dep (no sqlite/redis/sled/sqlx). Only persistence: `hub.json` (single config blob), `hub.lock` (fs2 lock), and samod `TokioFilesystemStorage` at `<hub_dir>/automerge` (document blobs — not queryable, not a session table). Revocation → the hub's **first** session store.

### Credential validation is centralized (change is additive)
- One extractor: `extract_credential(&HeaderMap)` → `Credential::{Cookie|Bearer}(String)` (`server.rs:212`); both present → **400 conflicting** (`:241-246`).
- One core validator: `authenticate_claims_for_kind(token, kind)` (`context.rs:538`); wrappers `authenticate_claims` (`:526`), `authenticate` (`:511`) delegate here.
- ~6 call sites: `Authenticated` extractor → `:440`; `ws_handler` → `:938`; `auth_me` → `:770` (cookie-only); `auth_actor` → `:801` (cookie-only); `auth_callback` → `:717` (validates *incoming* Google token before cookie-ing); `auth_refresh` → `:868` (validates *new* Google token before re-cookie-ing).
- `CredentialKind` enum (`server.rs:149`) already threaded through audit + CSRF/Origin gating (cookie-only CSRF `:569`, logout CSRF `:820`, WS-Origin `:930`). Bearer is the documented non-browser MCP path, exempt from CSRF/Origin (`:144-147`).
- `AUTH_COOKIE_MAX_AGE = 3600` (`server.rs:133`) hard-binds cookie lifetime to Google's 1 h — sliding sessions decouple this.
- **`ws_handler` validates once at upgrade and never re-checks** (`server.rs:894-900`) — expiry/revocation only takes effect on reconnect. Sliding sessions inherit this trade-off; live-socket revocation is out of scope (would need periodic re-check).

### Client refresh plumbing partly exists (from `bd-3o8zmz46`)
- `/auth/me` already returns token `exp` (`auth.rs:212-217`, `server.rs:737-739,777`); the SPA reads it and schedules refresh, with a two-strike WS-down `/auth/me` probe and offline-safe error handling (`useAuth.ts`, `useAuthProbe.ts`). Under sliding sessions, only *what mints the cookie* and *what `exp` means* (now sliding) change; the client can rely on **server-side re-issue on activity** and drop the One-Tap dependency.

## Design decisions

1. **Token format — hub-signed compact token.** Mint an HS256 JWT (reuse
   `jsonwebtoken`'s `EncodingKey::from_secret`) or an equivalent compact
   `base64url(payload).hmac` token, signed with a dedicated **session** secret.
   Payload: `sub`, the display claims `/auth/me` needs (`email`, `name`,
   `picture`), `iat`, sliding `exp`, an optional `kid` (reserved for future
   rotation — **absent in v1**, which uses a single secret), and an optional
   `sid` (only if/when revocation lands). Distinguish it from Google tokens by
   `iss = "quarto-hub"` (route to HMAC verify, not JWKS). Size ≈ 200–400 bytes —
   immune to the >3800-byte drop.
2. **Sliding expiry.** Define an **idle timeout** (e.g. re-issue window) and an
   **absolute max lifetime**. The `Authenticated` extractor / a response layer
   re-issues `Set-Cookie` when the token is within the refresh threshold, so any
   authenticated request renews the session. No client JWT round-trip needed —
   this is the One-Tap-free win.
3. **Stateless first; revocable is a deferred fork (the true size driver).**
   - **v1 — stateless sliding-window** (no server store): global invalidation is
     via secret rotation; per-user logout clears the cookie client-side (exists
     today). **Size S–M.**
   - **v2 — revocable** (explicit "log out everywhere" / server revocation):
     adds the hub's first session store (schema, restart-durability, GC sweep,
     concurrency). **Size M–L. Defer** to its own sub-plan unless a concrete
     requirement (e.g. security/compliance) forces it.
4. **Dedicated session secret (single secret in v1; rotation deferred).** Do
   **not** reuse the actor-id `server_secret` (different blast radius). Add a
   `session_secret` via the same `resolve_server_secret` pattern (env →
   `hub.json` → autogen `0o600`). **v1 signs/verifies with a single secret and
   ships no rotation machinery** — the hub has no rotation mechanism or schedule
   today, so a (rare, manual) secret change forcing re-login is no worse than a
   deploy. **`kid`/versioned two-secret overlap is deferred** (C5b) until a real
   rotation requirement appears; the retrofit is cheap because a **missing `kid`
   deterministically maps to the single secret**, so v1 cookies keep verifying
   once `kid` is later introduced.
5. **Additive coexistence with MCP Bearer.** At the central validator, branch on
   `credential.kind()`: `Cookie` → hub-session HMAC verify (+ sliding re-issue);
   `Bearer` → existing JWKS decode, **unchanged** (MCP clients keep sending
   `Authorization: Bearer <google_id_token>`). Preserve the dual-credential 400
   (`server.rs:241-246`), cookie-only CSRF (`:569,820`) and WS-Origin (`:930`).
6. **Migration.** Existing cookies hold Google JWTs. On cutover either (a) accept
   both during a window (cookie with `iss=accounts.google.com` → JWKS; `iss=
   quarto-hub` → HMAC), then drop the Google-cookie path; or (b) force one
   re-login. Prefer (a) for a seamless deploy.

## Phases (TDD-first)

- [ ] **C0 — Test scaffolding + token-format spec.** Failing tests for mint/verify/expiry/sliding-re-issue/tamper-rejection; a fixture hub with a known session secret. (Rotation/`kid` tests belong to the deferred C5b.)
- [ ] **C1 — Session secret (single).** `resolve_session_secret` (own field, `resolve_server_secret` pattern), a **single** secret — no `kid`/overlap. Reserve an optional `kid` slot in the token but leave it absent. Tests: secret resolves via env → `hub.json` → autogen `0o600` and survives restart.
- [ ] **C2 — Mint + verify + central routing.** HS256 mint/verify; add a `Session` path at `authenticate_claims_for_kind` (Cookie → session verify; Bearer → JWKS unchanged). Tests: valid/expired/tampered/wrong-`kid`; Bearer path untouched; dual-credential still 400.
- [ ] **C3 — Wire minting + sliding re-issue.** `auth_callback` + `auth_refresh` validate the Google token once, then mint the session cookie; `auth_me`/`auth_actor` switch to session verify; re-issue `Set-Cookie` on activity near expiry. Tests: session survives past 1 h with no One-Tap; `/auth/me` returns sliding `exp`; large Google token no longer cookie-dropped.
- [ ] **C4 — Invariants + migration + security review.** Preserve dual-credential 400, cookie-only CSRF/Origin; accept legacy Google-JWT cookies during the overlap window; review the token format against the auth-confusion protections that drove device-flow Phase 2. Tests: legacy cookie still works during window; CSRF/Origin gates intact.
- [ ] **C5 — (Deferred) revocable session store.** Only if required. Own sub-plan: store schema, durability, GC, "log out everywhere". Not in v1.
- [ ] **C5b — (Deferred) secret rotation via `kid`/overlap.** Keep current + previous secret, sign with current, verify against both during an overlap window; new cookies carry the new `kid`. Only when a concrete rotation requirement appears. Retrofits without a mass logout — missing-`kid` (v1) cookies map to the single v1 secret. Tests: rotate → old cookies still verify during overlap, new cookies use new `kid`, expired-overlap old cookies rejected.
- [ ] **C6 — Client alignment.** Rely on server sliding re-issue; retire the One-Tap renewal dependency (coordinate with Part B's B2); confirm `/auth/me` `exp` semantics. Tests: renewal works where One-Tap is blocked (FedCM/3p-cookie).
- [ ] **C7 — End-to-end verification + docs.** Real browser against a running hub: log in, idle past 1 h, keep working (no re-login, no One-Tap); confirm cookie size; confirm `q2 mcp` (Bearer) unaffected. Record invocation + observed output per the end-to-end policy.

## Risks
- **Secret rotation forces re-login in v1** (single secret, no overlap) — acceptable because rotation is rare/manual and has no mechanism today. `kid`/overlap is deferred (C5b) and retrofits without a mass logout: missing-`kid` cookies map to the v1 secret.
- **Revocation store** is the size fork; keep v1 stateless to avoid the hub's first DB. Revisit only on a concrete requirement.
- **Security-review surface:** a new credential type must not weaken the dual-credential-confusion protections, CSRF, or WS-Origin gating.
- **WS validate-once:** expiry/revocation still only bite on reconnect (unchanged from today) — call out explicitly; live-socket revocation is out of scope.
- **Migration window:** mis-handling the legacy Google-JWT cookie during cutover would log users out; the dual-accept window (decision 6a) avoids this.

## Braid strand structure
- **Epic `bd-ey6jg70f`** (epic, p2, open). Sub-strands **C0–C4, C6, C7** (parent-child); **C5 (revocable store)** and **C5b (`kid`/rotation)** as separate `related` strands, opened only when their requirement appears.
- Related links in place: `bd-3o8zmz46` (structural fix), `bd-qxgoti2b` (Part B; its B2 soft-depends on this). `bd-3g0aijb3` (Bearer fix) is a standalone strand, not part of this epic.

## References
### Plans
- `claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md` — umbrella index for all three plans.
- `claude-notes/plans/2026-07-06-hub-client-auth-unification-pkce.md` — Part B (its B2 phase soft-depends on this plan).
- `claude-notes/plans/2026-06-10-ws-auth-expiry-handling.md` — the tactical `bd-3o8zmz46` mitigation; sliding sessions are the structural follow-up.
- `claude-notes/plans/2026-02-26-httponly-cookie-auth.md` — the current cookie model.

### Strands
- `bd-ey6jg70f` — this epic.
- `bd-3o8zmz46` — *closed* root-cause bug (expired session → permanent offline).
- `bd-3g0aijb3` — Bearer on `/auth/actor` + `/auth/me` (standalone; not part of this epic).

### Key files
- Secret lifecycle: `crates/quarto-hub/src/storage.rs` (115-135, 179-197, 232, 317, 339, 391), `crates/quarto-hub/src/context.rs:388`.
- Validation: `crates/quarto-hub/src/context.rs` (511-542), `crates/quarto-hub/src/server.rs` (149, 165-181, 212-247, 415, 440, 907, 938).
- Mint points + cookie: `crates/quarto-hub/src/server.rs` (291-309 `build_auth_cookie`, 701-729 callback, 844-879 refresh, 768-807 me/actor, 133 max-age, 292-298 size warning), `crates/quarto-hub/src/auth.rs` (212-217 exp, 644-664 OidcState CSRF stub, 729-737 HMAC precedent).
- WS once-at-upgrade: `crates/quarto-hub/src/server.rs:894-950`.
- Client refresh plumbing: `hub-client/src/hooks/useAuth.ts`, `hub-client/src/hooks/useAuthProbe.ts`, `hub-client/src/services/authService.ts`.

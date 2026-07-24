# Hub server-minted sliding sessions

**Status:** in progress (kicked off 2026-07-24)
**Strand:** `bd-ey6jg70f` (epic)
**Phase strands:** C0 `bd-j1241nof` · C1 `bd-sekcpmv1` · C2 `bd-jyzz8o97` ·
C3 `bd-nh5pt1pd` · C4 `bd-74iiwb3l` · C5 `bd-3dq0x6ut` · C5b `bd-6kll0jr6` ·
C6 `bd-exk3hfxk` · C7 `bd-6s83nc38`
**Branches:** integration `feature/hub-sliding-sessions`; topic branches
`braid/<id>-<slug>` per phase, merged `--no-ff` into the integration line.
**Part of:** the auth-reshape path — umbrella index:
`claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md`
(Part B's "retire One-Tap" phase B2 soft-depends on this plan).

## Overview

Today the hub is a **stateless OIDC resource server**: the `quarto_hub_token`
HttpOnly cookie's value *is* the Google ID token (1 h `exp`), re-validated
against Google's JWKS on every request. The hub mints/signs nothing. Two
independent failure modes follow:

1. **Renewal depends on Google One-Tap**, which is unavailable in many
   environments (FedCM / third-party-cookie policies / unregistered origins →
   `gsi/status 403`). When the 1 h token expires and One-Tap can't renew, every
   WS upgrade 401s and the SPA presents as permanently offline — the root cause
   of `bd-3o8zmz46`, which shipped only a tactical client-side mitigation.
2. **Large IdP tokens can be silently dropped.** A token >3800 bytes may exceed
   the ~4096-byte cookie limit and be dropped by the browser
   (`server.rs:292-298`) — presenting, again, as "not logged in."

**Goal:** validate the Google token **once** at login, then mint a
**hub-signed, compact, HttpOnly session cookie with sliding expiry**. The hub
re-issues the cookie on activity, so sessions outlive Google's 1 h token
without One-Tap, and the cookie is small. Bearer/JWKS validation (the MCP
path) is untouched.

**Scope:** the target deployment profile includes a hub with **many public
users**, so v1 ships the full stack — minting, sliding expiry, a
revocation-event store (`revocations.json`), and secret rotation via `kid`
overlap. This adds the hub's **first token-minting capability** and **first
revocation store**, both on the credential path: review accordingly.

## Current state (map for implementers)

### The hub signs nothing today, but the primitive + secret pattern exist
- Validation-only: `jsonwebtoken = "10"` + `axum-jwt-auth = "0.6"` used only for `RemoteJwksDecoder` *decode* (`auth.rs:344`, `:612-618`). `EncodingKey` (the mint primitive) appears **only** in `tests/auth_bearer.rs` (mock IdP), never in production. No `client_secret`, no `oauth2`/`openidconnect` client.
- **HMAC-SHA256 is already in use and shipped:** `sub_to_actor_id_for_project` (`auth.rs:729-737`); deps `hmac`, `sha2`, `rand` already present.
- **Secret lifecycle precedent:** `resolve_server_secret` (`storage.rs:179-197`) resolves a 32-byte secret via env `QUARTO_HUB_SERVER_SECRET` → `hub.json` `server_secret` → auto-generate + persist (mode `0o600`, `storage.rs:115-135`). Cached in `StorageManager.server_secret` (`storage.rs:232,317,339`), surfaced via `ctx.server_secret_bytes()` (`context.rs:388`). Survives restarts; no rotation mechanism (no `kid`/versioning).

### No store today
- No DB dep (no sqlite/redis/sled/sqlx). Only persistence: `hub.json` (single config blob), `hub.lock` (fs2 lock), and samod `TokioFilesystemStorage` at `<hub_dir>/automerge` (document blobs — not queryable). C5 adds the hub's first revocation store (`revocations.json`: per-`sub` `not_before` map + ban entries, not a session table — §3).

### Credential validation is centralized (change is additive)
- One extractor: `extract_credential(&HeaderMap)` → `Credential::{Cookie|Bearer}(String)` (`server.rs:212`); both present → **400 conflicting** (`:241-246`).
- One core validator: `authenticate_claims_for_kind(token, kind)` (`context.rs:538`); wrappers `authenticate_claims` (`:526`), `authenticate` (`:511`) delegate here.
- ~6 call sites: `Authenticated` extractor → `:440`; `ws_handler` → `:938`; `auth_me` → `:770` (cookie-only); `auth_actor` → `:801` (cookie-only); `auth_callback` → `:717` (validates *incoming* Google token before cookie-ing); `auth_refresh` → `:868` (validates *new* Google token before re-cookie-ing).
- `CredentialKind` enum (`server.rs:149`) already threaded through audit + CSRF/Origin gating (cookie-only CSRF `:569`, logout CSRF `:820`, WS-Origin `:930`). Bearer is the documented non-browser MCP path, exempt from CSRF/Origin (`:144-147`).
- `AUTH_COOKIE_MAX_AGE = 3600` (`server.rs:133`) hard-binds cookie lifetime to Google's 1 h — sliding sessions decouple this.
- **`ws_handler` validates once at upgrade and never re-checks** (`server.rs:894-900`) — expiry/revocation only take effect on reconnect. Sliding sessions inherit this trade-off; live-socket revocation is out of scope (would need periodic re-check).

### Client refresh plumbing partly exists (from `bd-3o8zmz46`)
- `/auth/me` already returns token `exp` (`auth.rs:212-217`, `server.rs:737-739,777`); the SPA reads it and schedules refresh, with a two-strike WS-down `/auth/me` probe and offline-safe error handling (`useAuth.ts`, `useAuthProbe.ts`). Under sliding sessions, only *what mints the cookie* and *what `exp` means* (now sliding) change; the client relies on server-side re-issue and drops the One-Tap dependency.

## Design

1. **Token format — hub-signed compact token.** HS256 JWT (reuse
   `jsonwebtoken`'s `EncodingKey::from_secret`), signed with a dedicated
   **session** secret (§4). Payload:
   - `sub`; `email`, `email_verified`, `name`, `picture` — stamped from the
     Google claims validated at mint; consumed by `/auth/me` and the
     per-request allowlist re-check (§5);
   - `iat` — time of the most recent (re-)issue;
   - **`auth_time`** — the original Google-validation instant, carried
     **unchanged across every re-issue**: the anchor for the absolute
     lifetime cap (§2), without which a stolen cookie could be renewed
     indefinitely by attacker activity;
   - sliding `exp`;
   - **`sid`** — random, minted at login, carried unchanged across re-issues
     (identifies the session family; per-`sub` revocation doesn't need it,
     but emitting it from day one lets per-device revocation retrofit with
     no token-format change);
   - `iss = "quarto-hub"` — required by the session validator (defensive
     claim; with the §6 hard break there is no cookie-content routing).

   The JOSE *header* carries a **static `kid`** derived with domain
   separation: first 8 hex of
   `HMAC-SHA256(session_secret, "quarto-hub-session-kid-v1")` — never a
   truncated plain `SHA-256(session_secret)`, which would publish bits of
   the secret's bare hash in every token (cross-protocol-reuse hazard, and
   an offline dictionary oracle if an operator supplies a low-entropy env
   secret). Size ≈ 250–450 bytes — immune to the >3800-byte drop.
2. **Sliding expiry — pinned lifetimes, capped by `auth_time`.** Named
   constants, deployment-configurable via env/config with these defaults
   (public deployments should prefer tighter caps — sliding sessions mean
   more standing credentials): **idle timeout 7 days**, **absolute max
   lifetime 30 days**. At every (re-)issue:
   `exp = min(now + idle, auth_time + absolute)`. Verification enforces `exp`
   **and, independently, `now < auth_time + absolute`** (defense in depth — a
   re-issue bug can never extend a session past the cap). A response layer
   re-issues `Set-Cookie` on authenticated activity, subject to **all** of:
   - (a) credential kind is **Cookie** — a Bearer/MCP response must never
     set a session cookie;
   - (b) the request passed **full validation including the allowlist
     re-check** (§5);
   - (c) the token is ≥ 1 h old (`now − iat ≥ 1 h`, bounding Set-Cookie
     churn to ~1/h per session) **or was signed under a non-current `kid`**
     (migrates sessions promptly during graceful rotation, §4);
   - (d) re-issue **never advances `auth_time`**.

   No client JWT round-trip needed — this is the One-Tap-free win. Today a
   stolen cookie dies within 1 h; under sliding expiry the idle/absolute
   caps are what bound it — hard requirements, not tuning knobs. A WS-only
   client never slides the window (validate-once at upgrade; `Set-Cookie` on
   a 101 is unreliable) — the SPA's periodic `/auth/me` probe is the
   keep-alive (C6).
3. **Stateless sessions + a revocation-event store.** At public scale the
   hub likely runs with **no allowlist** (any verified Google account passes
   `check_allowlists`), so the allowlist is not a per-user deny lever, and
   secret rotation as incident response would be a mass logout — revocation
   is an operational necessity. Sessions stay **stateless** (tokens are
   self-contained; **no session table**); the store records only
   **revocation events**:
   - **Shape:** a per-`sub` `not_before` map plus operator **ban** entries
     (a ban is `not_before = ∞` — the only per-user deny that works without
     an allowlist). Verify rejects when `auth_time < not_before[sub]` or the
     `sub` is banned; **bans gate mint too** (`auth_callback`/`auth_refresh`
     refuse a banned `sub` — otherwise a banned user just re-logs-in via
     Google). Plain logout stays a client-side cookie-clear; the revocation
     action is self-service `POST /auth/logout-everywhere` (CSRF-gated,
     cookie-kind only).
   - **Storage: a dedicated `revocations.json`** in the hub dir — *not*
     `hub.json`, which holds startup config + two signing secrets at `0o600`
     and must not gain a user-triggerable write path. In-memory map behind a
     tokio mutex; persist via atomic temp-file + rename. Single-writer is
     guaranteed by construction (`hub.lock` holds an exclusive fs2 lock for
     the process lifetime), so no DB is needed for concurrency. Revisit
     sqlite only if crash-durability of individual events or event-rate
     growth demands it.
   - **GC:** logout-everywhere entries self-expire once
     `not_before + absolute_max < now`; **ban entries persist until
     explicitly lifted**. Size stays trivial — entries exist only for users
     who revoke or are banned.
   - **Out of scope (not security-critical):** per-`sid` (single-device)
     revocation and an admin ban endpoint/UI — v1 ships the self-service
     endpoint plus operator-managed ban entries; `sid` (§1) makes
     per-session revocation a pure retrofit. **Ban procedure:** edit
     `revocations.json` only with the hub stopped, or restart immediately
     after — never hand-edit while the hub runs: the hub's own atomic
     persist can overwrite a live edit (the single-writer guarantee covers
     hub processes, not concurrent operator edits), silently losing the
     ban. The restart doubles as a feature — it severs the banned user's
     live WS (see Risks).
4. **Dedicated session secret; rotation via `kid` overlap.** Do **not**
   reuse the actor-id `server_secret` (different blast radius). Add a
   `session_secret` via the `resolve_server_secret` pattern (env →
   `hub.json` → autogen `0o600`), plus an optional **previous** secret
   (`previous_session_secret` + `session_secret_rotated_at` in `hub.json`;
   env `QUARTO_HUB_SESSION_SECRET_PREVIOUS`). Signing always uses the
   current secret; verification resolves the token's `kid` (§1) by
   exact-match lookup in the `kid → secret` map (**size ≤ 2**: current +
   optional previous), **failing closed** on an unknown *or missing* `kid`.
   The `kid` buys (a) **observability** — a verify failure logs as `kid`
   mismatch ("minted under a different secret": rotated, or config drift
   such as a wrong secret env var after a restore) vs expired vs tampered,
   instead of one generic failure — and (b) rotation as a **pure map
   operation** with no verifier-logic change. **Two rotation modes — the
   distinction is security-critical:**
   - **Graceful (scheduled hygiene):** supply a new current secret, keep
     the old as previous; both verify during an **overlap window = one idle
     timeout** (long enough that sliding re-issue re-mints every active
     session under the new `kid`, §2c), after which the previous entry is
     dropped automatically (`session_secret_rotated_at + idle ≤ now`). No
     user disruption.
   - **Emergency (secret compromise):** supply only the new secret, **no
     previous** — every outstanding token dies immediately. Mass logout is
     the *point*: a compromised secret can forge tokens, so an overlap
     window would keep accepting attacker-minted cookies. **Never respond
     to a compromise with a graceful rotation.**

   Scope note: sessions are bound by the secret, not the instance — hub
   processes sharing `QUARTO_HUB_SESSION_SECRET` via env accept each other's
   session cookies (intended for multi-instance deployments; per-hub
   autogenerated secrets give per-instance isolation; no per-instance `aud`
   binding).
5. **Additive coexistence with MCP Bearer — pinned per-branch algorithms +
   per-request allowlist.** At the central validator, branch on
   `credential.kind()`: `Cookie` → hub-session HMAC verify (+ sliding
   re-issue); `Bearer` → existing JWKS decode, **unchanged** (MCP clients
   keep sending `Authorization: Bearer <google_id_token>`). Each branch pins
   its own algorithm set and key type — the token header must never select
   either: session branch `Validation { algorithms: [HS256] }` +
   `DecodingKey::from_secret` (+ required `iss`/`exp` claims, 60 s leeway
   matching `validate_azp_and_iat`); Bearer branch keeps its JWKS-declared,
   asymmetric-only algorithms (`signing_algorithm` maps no HMAC alg).
   Cross-path rejection is a tested invariant (C2): a hub session token
   presented as `Authorization: Bearer` fails JWKS verify; a Google RS256
   token in the cookie fails session verify (= the §6 hard break). **The
   session path re-runs `check_allowlists` on every request** using the
   session claims (`email`, `email_verified` carried from mint): removal
   bites on the user's next request (`context.rs:613`), and skipping the
   re-check would silently defer it to absolute expiry. Together with §3's
   revocation/ban entries this forms the per-user deny toolkit (allowlist
   for closed deployments; bans when no allowlist is configured). Preserve
   the dual-credential 400 (`server.rs:241-246`), cookie-only CSRF
   (`:569,820`) and WS-Origin (`:930`).
6. **Cutover — hard break.** Existing cookies hold Google JWTs. After
   cutover, session verify (HS256-only) rejects them → 401 → the SPA's
   normal logged-out flow → one re-login. No dual-accept window, no
   cookie-content routing, no legacy-accept code. The cost is bounded:
   legacy cookies ship `Max-Age=3600` with a ≤ 1 h Google `exp`, so affected
   users would have re-authenticated within the hour anyway. C4 verifies the
   failure path is clean (401 + logged-out UX, no redirect loop). A
   `__Host-` cookie-name upgrade stays out of scope: `__Host-` requires
   `Secure`, which the `--allow-insecure-auth` HTTP dev mode cannot satisfy
   — revisit if we adopt env-conditional cookie naming. Until it lands,
   public deployments must serve the hub from an origin with **no untrusted
   sibling subdomains**: subdomains are *same-site*, so a compromised
   sibling can plant a session cookie past the `SameSite`/CSRF gates
   (login-fixation — victim unknowingly works in an attacker's account);
   cookie planting is exactly the residual `__Host-` closes.

## Phases (TDD-first)

- [x] **C0 — Test scaffolding + token-format spec.** *(done 2026-07-24, `bd-j1241nof`.
  `crates/quarto-hub/src/session.rs` pins the spec (constants, `SessionClaims`,
  kid scheme, lifetimes; defaults confirmed at idle 7 d / absolute 30 d, env
  overrides `QUARTO_HUB_SESSION_IDLE_SECS`/`QUARTO_HUB_SESSION_ABSOLUTE_SECS`).
  26 unit + 13 session integration tests written first and observed failing.
  Test scaffolding consolidated per `.claude/rules/integration-tests.md`:
  `tests/auth_bearer.rs` moved to `tests/integration/`, shared fixtures in
  `tests/integration/support.rs` (`TestHubBuilder` with known session secret
  `TEST_SESSION_SECRET`). Deviation from the C0 bullet: revocation-and-ban
  failing tests land at C5 kickoff and sliding-re-issue HTTP tests at C3
  kickoff — same deferral pattern the bullet itself uses for rotation/kid
  tests — so every committed tree stays green.)* Failing tests for mint/verify/expiry/sliding-re-issue/absolute-cap/allowlist-re-check/revocation-and-ban/tamper-rejection; a fixture hub with a known session secret. The token-format spec pins: the static-`kid` scheme (JOSE *header* parameter, domain-separated derivation per §1), the immutable-`auth_time` anchor semantics, the per-login `sid` (§1), and the lifetime constants (idle 7 d / absolute 30 d defaults from §2, deployment-configurable — confirm or adjust here, as named constants). (Rotation/overlap tests land with C5b; `kid` emission + fail-closed rejection tests with C2.)
- [x] **C1 — Session secret + derived `kid` + secret-file hygiene.** *(done
  2026-07-24, `bd-sekcpmv1`. `resolve_session_secret` in `storage.rs` (env
  `QUARTO_HUB_SESSION_SECRET` → `hub.json` `session_secret` → autogen 0o600),
  cached on `StorageManager`; `derive_session_kid` (domain-separated HMAC,
  first 8 hex); catch-all `.gitignore` written into the hub dir at
  `StorageManager::init` (created fresh, added to existing dirs, never
  overwrites an operator-modified one). Tests in `storage.rs` +
  `session.rs`.)* `resolve_session_secret` (own field, `resolve_server_secret` pattern), a **single** secret at this phase — the previous-secret overlap config lands in C5b. Derive the static `kid` with domain separation (first 8 hex of `HMAC-SHA256(session_secret, "quarto-hub-session-kid-v1")`, §1). **Write a `.gitignore` containing `*` into `.quarto/hub/` at `StorageManager::init`**: in project mode `hub.json` sits inside the user's project tree, and a session-signing secret there escalates the leak blast radius from actor-id correlation (`server_secret`) to full session forgery — q2's own repo gitignores `**/.quarto/hub/hub.json`, but user projects rely on their own hygiene. Tests: secret resolves via env → `hub.json` → autogen `0o600` and survives restart; derived `kid` is deterministic and stable across restarts; `.gitignore` is created on fresh init and added to an existing `.quarto/hub/` that lacks it.
- [x] **C2 — Mint + verify + central routing.** *(done 2026-07-24,
  `bd-jyzz8o97`. `mint_session`/`reissue_session`/`verify_session` in
  `session.rs` (pinned HS256, kid exact-match fail-closed, manual `exp` with
  60 s leeway against a caller-supplied clock, absolute cap independent of
  `exp`, future-`iat`/`auth_time` rejected as tampered).
  `HubContext::authenticate_session` (session verify + per-request
  `check_allowlists_for` re-check + distinguishable audit details
  `session_kid_mismatch`/`session_expired`/`session_absolute_cap`/
  `session_tampered`) and `authenticate_credential` central dispatch
  (Cookie → session, Bearer → JWKS unchanged); `Authenticated` extractor +
  `ws_handler` switched over. Cookie-side CSRF regression test now uses a
  session cookie (Google-JWT cookies 401 before the CSRF check — the §6
  hard break at work). All 327 quarto-hub tests green.)* HS256 mint/verify with pinned per-branch algorithms (§5): session branch `Validation { algorithms: [HS256] }` + `DecodingKey::from_secret`, required `iss`/`exp`, 60 s leeway. Mint stamps the static `kid` (JOSE header); verify resolves it by exact-match lookup in the `kid → secret` map (size ≤ 2 per §4; size 1 until C5b lands), **failing closed** on an unknown or missing `kid`, and logs failures distinguishably (`kid` mismatch vs expired vs tampered; never log token contents). Verify enforces the absolute cap independently of `exp` (`now < auth_time + absolute`, §2) and re-runs `check_allowlists` on the session claims (§5). Add a `Session` path at `authenticate_claims_for_kind` (Cookie → session verify; Bearer → JWKS unchanged). Tests: valid/expired/tampered; unknown-`kid` and missing-`kid` both rejected; failure logging distinguishes the classes; **absolute-cap** (valid signature, future `exp`, `auth_time` past the cap → rejected); **allowlist-removal** (valid unexpired session token, user removed from allowlist → 403 on next request); **cross-path** (hub session token as `Authorization: Bearer` → rejected; Google RS256 token in the cookie → rejected); Bearer path untouched; dual-credential still 400.
- [ ] **C3 — Wire minting + sliding re-issue.** `auth_callback` + `auth_refresh` validate the Google token once, then mint the session cookie (`auth_time = now`, fresh random `sid`, `email_verified` stamped from the validated Google claims); `auth_me`/`auth_actor` switch to session verify — and, while rewiring them, route them through the shared credential extraction rather than entrenching today's `cookie_token`-only bypass of the dual-credential rule (coordinate with the standalone `bd-3g0aijb3`); re-issue `Set-Cookie` per the §2 constraints (Cookie-kind only, post-full-validation, ≥ 1 h token age or non-current `kid`, `auth_time` immutable, `exp` capped). Tests: session survives past 1 h with no One-Tap; `/auth/me` returns sliding `exp`; large Google token no longer cookie-dropped; re-issue never fires on Bearer responses; re-issued cookie preserves attributes (`HttpOnly`, `SameSite=Lax`, `Secure`, `Path=/`); re-issued token keeps the original `auth_time`.
- [ ] **C4 — Invariants + hard-break cutover + security review.** Preserve dual-credential 400, cookie-only CSRF/Origin; **hard break for legacy Google-JWT cookies (§6)** — no dual-accept window, no legacy-accept code; review the token format against the auth-confusion protections that drove device-flow Phase 2. Tests: legacy Google-JWT cookie → 401 and a clean logged-out flow (no redirect loop); CSRF/Origin gates intact.
- [ ] **C5 — Revocation-event store.** Dedicated `revocations.json` per §3: in-memory per-`sub` `not_before` map + ban entries behind a tokio mutex, atomic temp+rename persist, loaded at startup; verify rejects `auth_time < not_before[sub]` or banned `sub`; **mint refuses a banned `sub`** (`auth_callback`/`auth_refresh`); self-service `POST /auth/logout-everywhere` (CSRF-gated, cookie-kind only); GC on load/write (logout entries expire after the absolute cap; **bans never GC'd**). Own PR sequenced after C4 — keeps the minting release reviewable. Tests: logout-everywhere → prior tokens (including re-issued siblings) rejected, immediate re-login works (`auth_time ≥ not_before`); ban → verify rejected AND mint refused; revocations + bans survive restart; expired logout entries GC'd, bans retained; writes are atomic and never touch `hub.json`; endpoint requires CSRF header and rejects Bearer-kind callers; a ban entry hand-added to `revocations.json` while the hub is stopped is enforced after restart (the documented ban procedure, §3).
- [ ] **C5b — Secret rotation via `kid` overlap.** Keep current + previous secret in the `kid → secret` map (config per §4: `previous_session_secret` + `session_secret_rotated_at`, env equivalents), sign with the current one, verify against both during the overlap window (= one idle timeout), auto-drop the previous entry when the window lapses. Purely additive over C2's verifier — every cookie already carries its `kid`. Sliding re-issue re-mints any old-`kid` token on its next qualifying request (§2c), so active sessions migrate well inside the window. Ship the **emergency mode** alongside (new secret with no previous → immediate global invalidation) and document it as the compromise response (§4). Own PR after C5. Tests: graceful rotate → old-`kid` cookies verify during overlap and are re-minted under the new `kid` on next request; new logins carry the new `kid`; post-overlap old-`kid` cookies rejected (fail closed, logged as `kid` mismatch); emergency rotation rejects all prior cookies immediately; the map never exceeds two entries; both derived `kid`s are deterministic and distinct.
- [ ] **C6 — Client alignment.** Rely on server sliding re-issue; retire the One-Tap renewal dependency (coordinate with Part B's B2); confirm `/auth/me` `exp` semantics (now sliding); **own the keep-alive explicitly** — a WS-only client never slides the window (§2), so keep the periodic `/auth/me` probe running while a WS is open, at a cadence comfortably inside the idle timeout. Tests: renewal works where One-Tap is blocked (FedCM/3p-cookie); WS-open + probe keeps the session sliding.
- [ ] **C7 — End-to-end verification + docs.** Real browser against a running hub: log in, idle past 1 h, keep working (no re-login, no One-Tap); confirm cookie size; exercise `logout-everywhere` across two browser sessions (revoke on device A → device B's next request 401s into the logged-out flow); confirm `q2 mcp` (Bearer) unaffected. Record invocation + observed output per the end-to-end policy.

## Risks
- **Rotation-mode confusion:** a graceful rotation (overlap window) after a *compromise* would keep accepting attacker-forgeable old-`kid` cookies for up to one idle window — the emergency procedure (new secret, no previous) is the documented compromise response (§4). Post-overlap old-`kid` cookies fail closed and log as `kid` mismatch, so a mis-timed rotation surfaces in logs rather than as a mysterious mass logout.
- **Plain logout is not revocation:** logout clears the cookie only; the revocation action is `logout-everywhere` (C5), which kills the whole token family via `not_before[sub]` (re-issue yields parallel-valid tokens within the caps — that family is exactly what `not_before` revokes). Stolen-cookie response: `logout-everywhere` (user) or a ban entry (operator). Operator trap: allowlist remove-then-re-add does not kill an unexpired token — use a revocation/ban entry, not allowlist churn.
- **Revocation store write path:** user-triggerable persistence is new — mitigated by the dedicated `revocations.json` (never `hub.json`, which holds the signing secrets), single-writer by construction (`hub.lock`), tokio-mutex + atomic temp+rename persist. GC must never drop ban entries (they don't self-expire). Sqlite is the recorded fallback if crash-durability of individual events or event rate ever demands it.
- **Security-review surface:** a new credential type must not weaken the dual-credential-confusion protections, CSRF, or WS-Origin gating.
- **WS validate-once:** expiry/revocation only bite on reconnect (unchanged from today); a banned user's open sync socket survives until reconnect — accepted; if public-scale abuse response needs immediacy, the follow-up is a periodic re-check or killing the peer's connections on ban (the peer→email map, `context.rs:498`, already identifies them).
- **Hard-break cutover:** every logged-in user re-authenticates once at deploy — legacy cookies self-expire within 1 h anyway, and skipping a dual-accept window removes cookie-content routing and legacy-accept code entirely. The risk to watch is UX, not security: C4 verifies the 401 → logged-out flow is clean.

## Braid strand structure
- **Epic `bd-ey6jg70f`** (epic, p2, open). Sub-strands **C0–C5, C5b, C6, C7** (parent-child); no deferred phases.
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

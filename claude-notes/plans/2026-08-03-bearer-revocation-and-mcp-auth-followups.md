# Auth review follow-ups: Bearer revocation parity, MCP reconnect auth classification, /auth/me exp semantics

**Status:** proposed — not started. **Date:** 2026-08-03.
**Epic:** `bd-rk55baiz`. **Child strands:** F1 `bd-jkih1ql7` · F2 `bd-l3b1brn8` ·
F3 `bd-aw8f3sp8`.
**Branches (planned):** integration `feature/auth-review-followups`; topic
branches `braid/<id>-<slug>` per finding, merged `--no-ff` per the worktrees
convention.

## Overview

An auth review surfaced three real gaps. This plan fixes them:

1. **F1 (security, hub):** the Bearer path never consults the revocation
   ledger. Bans and `logout-everywhere` only affect session cookies — in a
   no-allowlist public deployment, a banned user keeps full MCP access
   indefinitely, and a stolen Google ID token survives `logout-everywhere`
   for up to ~1 h. Already noted as future work ("`sub_denylist`") in
   `2026-05-28-hub-mcp-loopback-pkce.md`; the ledger shipped since (C5), so
   the fix is now a wiring job, not a new store.
2. **F2 (robustness, MCP client):** a 401/403 on the WS upgrade — or a
   terminal refresh failure (`invalid_grant` → `ReauthRequired`) — is
   indistinguishable from a network blip:
   `NodeWebSocketClientAdapter.openSocket` swallows `getBearer` failures and
   the retry loop spins silently forever. The SPA got an evidence-based probe
   for exactly this shape (`bd-3o8zmz46`, `useAuthProbe`); MCP has no
   equivalent, so a revoked grant mid-session presents as an immortal, silent
   "offline".
3. **F3 (semantics, hub + SPA):** `AuthMeResponse.exp` means "sliding session
   expiry" on the cookie path but "fixed Google token expiry" on the Bearer
   path, with nothing in the response distinguishing them
   (`server.rs:1268-1288`). Latent trap for any future Bearer caller; also
   the SPA still carries the dead pre-sliding `DEFAULT_SESSION_MS = 1 h`
   fallback (`useAuth.ts:42`), which would mis-schedule (~168× too often) if
   it ever fired.

Sequencing: **F1 → F2** (F2's end-to-end "banned mid-session" case and its
403 handling exercise F1's new hub behavior). **F3 is independent** and can
land any time. F1 is Rust-only; F2 is TS-only (plus e2e); F3 touches both,
lightly.

## F1 — enforce the revocation ledger on the Bearer path (`bd-jkih1ql7`)

### Design

- **Where:** the Bearer *credential* path only — i.e. the path reached from
  `authenticate_credential`'s `Credential::Bearer` arm
  (`context.rs:823-840`). **Not** the mint-time validation path:
  `auth_callback`/`auth_session` validate an incoming Google token through
  the same `authenticate_claims` machinery, and they must keep their existing
  semantics (bans already gate mint explicitly; the `not_before` floor is
  handled there by the `min_auth_time` clamp so same-second re-login works —
  a raw `iat < not_before` check at mint would break exactly that).
  Pick the seam at implementation under one hard constraint: **`auth_ok`
  must not be emitted before the ledger check passes** (today it is emitted
  at `context.rs:663-670`; a check bolted on after the call would leave an
  `auth_ok` + deny pair in the audit log). Note a bearer-specific *wrapper*
  cannot satisfy this as-is — `auth_ok` is emitted inside
  `authenticate_claims_for_kind`, which the mint callers share — so the
  practical seam is an explicit enforce/skip parameter on
  `authenticate_claims_for_kind` (the ledger is already on `self` via
  `revocations()`), with the check inserted between the allowlist check
  (`:641-661`) and the `auth_ok` event. Mint callers
  (`authenticate_claims`) pass skip.
- **Checks**, mirroring `authenticate_session` (`context.rs:749-781`), using
  `RevocationLedger::check(sub, anchor)` (`revocation.rs:136-143`) with the
  Google token's `iat` as the anchor:
  - banned `sub` → **403**, audit `detail = "user_banned"`,
    `credential_kind = "bearer"`;
  - `iat < not_before[sub]` → **401**, audit `detail = "bearer_revoked"`
    (deliberately not `session_revoked` — it isn't a session);
  - **missing `iat` fails closed**: `OidcClaims.iat` is `Option<i64>`
    (`auth.rs:210` — required by OIDC and always sent by Google, but the
    type admits absence). Anchor with `claims.iat.unwrap_or(0)` so any
    `not_before` entry kills an `iat`-less token (the ban check is
    `iat`-independent anyway).
- **Honest scope (document it, don't oversell):** the `not_before` check
  kills outstanding *ID tokens* issued before a `logout-everywhere` — closing
  the documented ≤1 h stolen-token window. It does **not** kill a stolen
  *refresh token*: a refresh grant mints a fresh `iat` that passes. The
  hub-side lever for a hostile identity is the **ban**; Google-side
  revocation (`authenticate_clear` / RFC 7009) remains the refresh-token
  lever. A legitimate MCP client caught by `logout-everywhere` self-heals on
  its next refresh (fresh `iat` ≥ `not_before`) — same "immediate re-login
  works" semantics as the browser.
- **Unchanged:** WS validate-once (a ban still doesn't sever a live socket;
  restart remains the operator remedy — `server.rs:1584-1590`), allowlist
  checks, azp/iat validation, the dual-credential 400, and all mint paths.
  `q2 provide-hub` (`quarto-hub-provider`) inherits enforcement automatically
  since it presents the same Bearer.

### Work items (TDD)

- [x] Tests first (extended `auth_bearer.rs` with a `revocation_setup()`
      fixture — pre-written `revocations.json`, per-sub `not_before` floors
      anchored to the fixture instant; `support.rs` gained
      `TestHubBuilder::not_before_subs` and `ClaimsBuilder::no_iat`;
      observed 5/6 failing pre-fix, the self-heal 200 already passing as
      expected):
      - banned `sub` + otherwise-valid Google Bearer → 403 on `/health` **and**
        on the WS upgrade; audit shows `user_banned` / `credential_kind=bearer`
        (`bearer_banned_sub_returns_403`, `ws_upgrade_with_banned_bearer_returns_403`);
      - `not_before` floor then a Bearer whose `iat` predates it → 401, audit
        `bearer_revoked` (`bearer_with_iat_before_not_before_returns_401`);
      - a Bearer with **no `iat` claim** while a `not_before` entry exists →
        401 (`bearer_without_iat_fails_closed_when_not_before_exists`);
      - a Bearer minted *after* the revocation instant → 200
        (`bearer_minted_after_revocation_authenticates`);
      - mint regression (`bearer_revocation_does_not_leak_into_mint_path`):
        the same credential that 401s as a Bearer still mints via
        `POST /auth/session` — the shared-machinery mint path; `/auth/callback`
        is Google-provider-only (sealed login-state cookie) and uses the same
        `authenticate_claims` + `min_auth_time` clamp. Both deny-tests also
        pin the audit ordering (no `auth_ok` for a denied sub);
      - session-path regression: all 448 quarto-hub tests green, including
        `ban_gates_verify_and_mint` and
        `logout_everywhere_kills_prior_tokens_and_relogin_works`.
- [x] Implemented: `RevocationEnforcement { Enforce, Skip }` parameter on
      `authenticate_claims_for_kind`; ledger check inserted between the
      allowlist check and the `auth_ok` emission, anchored at
      `claims.iat.unwrap_or(0)`; the Bearer dispatch arm passes `Enforce`,
      `authenticate_claims` (both mint callers) passes `Skip`.
- [x] Docs: `ts-packages/quarto-hub-mcp/README.md` residual-window
      paragraph rewritten (window closed for hub-side events;
      refresh-token caveat stated); `dev-docs/quarto-hub/session-auth-operations.md`
      updated in three spots (model paragraph, revocation section,
      audit-detail list gains `bearer_revoked`); all three `sub_denylist`
      notes in `2026-05-28-hub-mcp-loopback-pkce.md` annotated.
- [x] `cargo nextest run --workspace`: 10863 passed, 0 failed.
      `cargo xtask verify --skip-hub-build`: pass (see session log).
- [x] E2E per policy: `scripts/hub-bearer-revocation-e2e.mjs` (committed,
      sibling of `hub-sliding-sessions-e2e.mjs`) — mock IdP + real
      `target/debug/hub`; baseline 200/101 for three subs; stopped-hub
      write of `revocations.json` (ban sub A, `not_before` floor for
      sub B); restart; observed: banned A → 403 on `/health` **and** the
      WS upgrade (fresh token too), B pre-floor token → 401 on both,
      B fresh-iat token → 200 (self-heal), untouched C → 200/101.
      Invocation: `cargo build --bin hub && node scripts/hub-bearer-revocation-e2e.mjs`
      → `ALL CHECKS PASSED` (12/12, 2026-08-03). Additionally the
      full-stack MCP e2e (`ts-packages/quarto-hub-mcp/src/e2e-auth.test.ts`:
      real hub binary + real keyring + loopback PKCE + Bearer WS) passes
      against the F1-patched hub.

## F2 — MCP-side auth classification on reconnect (`bd-l3b1brn8`)

### Design

Split evidence from policy, mirroring the SPA's `bd-3o8zmz46` invariant
(*only a reachable server's definitive 401/403 changes auth state; network
errors never do*):

- **Adapter reports evidence** (`quarto-sync-client`):
  - Widen the factory/`WebSocketLike` seam (`NodeWebSocketClientAdapter.ts:50-73`)
    so the default `ws` factory can surface a non-101 upgrade status. Note:
    `ws` exposes this via the **EventEmitter-only `'unexpected-response'`
    event** — it is not reachable through `addEventListener`, so the default
    factory must attach it natively and translate it into the seam (an
    optional capability; test fakes without it keep working). With no
    `'unexpected-response'` listener, `ws` folds the status into a generic
    `'error'` — which is exactly the current information loss.
    **Verified in ws@8 source** (`websocket.js`: `!websocket.emit('unexpected-response', …) && abortHandshake(…)`):
    when a listener **is** attached, ws skips `abortHandshake` entirely —
    no `'error'` or `'close'` fires for that socket and the underlying
    HTTP request is left open. The factory's handler must therefore abort
    the handshake itself (destroy the request, drain the response) after
    capturing the status, or every failed attempt leaks a connection.
    Retry continuity then rests on the adapter's `retryIntervalId`
    interval — cleared only in `onOpen`, re-created by `connect()` after a
    live-socket close — not on `'close'` from the failed socket. Pin both
    with tests: no request leak per failed attempt, and retry survives the
    mid-session sequence open → hub closes socket → reconnect gets 403
    via `'unexpected-response'`.
  - New optional `onAuthRejected(evidence)` on the adapter options /
    `SyncClientAuthOptions` (`types.ts:204-212`), fired on definitive
    evidence only: `{ kind: 'upgrade-status', status: 401 | 403 }` or
    `{ kind: 'token-refresh-terminal' }` (a `ReauthRequired` thrown by
    `getBearer` — today swallowed at `openSocket`, `:184-190`). Debounced to
    one report per failure episode — an episode ends at the next successful
    open (`peer-candidate`), which resets the debounce; plain network
    close/error never fires it. **Classification is by
    `error.name === 'ReauthRequired'`**: sync-client cannot import the
    class (the dependency direction is hub-mcp → sync-client), and
    `refresh-manager.ts:81-88` already stamps
    `override readonly name = 'ReauthRequired'`. Document that as the
    cross-package contract. `TokenRefreshError` (structured non-`invalid_grant`
    IdP failures — transient or config, per `buildTokenRefreshMessage`) must
    **not** be treated as terminal. Plumbing: `buildWsAdapter`
    (`client.ts:130-152`) currently forwards only `getBearer` +
    `retryInterval` to the adapter — forward the new callback alongside.
  - On `token-refresh-terminal`, stop the retry loop (today it spins forever
    calling a `getBearer` that will throw every time). On upgrade-status
    evidence, keep retrying — policy below may fix the token and the next
    attempt succeeds.
- **Connection manager owns policy** (`quarto-hub-mcp`,
  `connection-manager.ts`): coalesce concurrent reports — multiple project
  adapters share one manager and will all fire after a hub-wide event, so at
  most one forceRefresh+reprobe cycle runs at a time. On
  `upgrade-status: 401` → one `forceRefresh()` +
  `probeAuth` (reusing the `:484-501` pattern; that code is pre-connect —
  the mid-session handler is a new method reusing the same pieces); if the
  probe then passes, do nothing (the
  adapter's retry picks up the fresh token via `getBearer`). If it still
  401s → `rm.invalidate()` + disconnect the project handle + set a
  `reauth-required` state so the **next tool call returns the existing
  `ReauthRequired` message immediately** instead of hanging into the 15 s
  peer timeout. On `403` → terminal "your account is not allowed on this hub
  (banned or not allowlisted)" state; **keyring kept** (credentials are
  valid; identity is denied — re-auth won't help, so don't wipe). Also map a
  403 from the *initial connect probe* (today: `Unexpected status 403`,
  `:511-513`) to the same clear message.
- Surface through the existing `SyncClientCallbacks.onError` seam + stderr;
  no new MCP protocol surface.
- **Bundle-safety constraints:** `ws` must stay out of browser bundles (the
  lazy import in `client.ts:130-152` is the guard — don't disturb it);
  hub-client bundles `quarto-sync-client` from source, so
  `npm run build:all` must pass.

### Work items (TDD)

- [x] Tests first, sync-client (7 new specs in
      `NodeWebSocketClientAdapter.test.ts`, 4 observed failing pre-fix, 3
      pinning must-stay behavior): upgrade-401 fires `onAuthRejected` exactly
      once per episode with reset on peer handshake; mid-session 403 with no
      close event (the ws unexpected-response shape) keeps the interval retry
      alive; network close/error fires nothing and keeps retrying;
      `ReauthRequired`-named `getBearer` failure fires `token-refresh-terminal`
      and stops the retry loop; `TokenRefreshError`-named failures stay
      transient; a factory without the status capability degrades to today's
      behavior; plus a REAL-`ws` spec against a raw net server that answers
      403 and keeps the connection open — pins both the status surfacing and
      the no-connection-leak invariant.
- [x] Tests first, hub-mcp (10 new specs in `connection-manager.test.ts`,
      all observed failing pre-fix): wiring; 401 → one forceRefresh+reprobe
      → silent recovery; persistent 401 → invalidate + reauth-required +
      next call rejects `ReauthRequired` with zero network (scripted fetch
      exhausted); re-auth self-heal; 403 evidence → keyring intact +
      `HubAccessDeniedError` on the re-probe; recheck-403 maps to the same
      denial; token-refresh-terminal skips the pointless refresh; concurrent
      reports coalesce to one cycle; transient refresh failure is
      state-neutral; initial-probe 403 gets the clear message.
- [x] Implemented. Adapter: factory seam gains optional `onUpgradeStatus`
      capability; the default `ws` factory attaches the EventEmitter-only
      `'unexpected-response'` natively and aborts the handshake itself
      (drain + destroy request + destroy the captured TCP socket — verified
      empirically that a no-op handler leaks and that ws skips
      `abortHandshake` when a listener exists); `AuthRejectionEvidence`
      reported via `onAuthRejected`, episode-debounced, reset at
      peer-candidate; ReauthRequired-by-name classification; `authTerminal`
      stops the loop. Found+fixed adjacent hazard: `disconnect()` during a
      CONNECTING real socket removed listeners then `close()`d, turning
      ws's "closed before established" error event into an
      uncaughtException — a swallow-only error listener now guards it.
      Manager: `handleAuthRejected` (coalesced), `enterReauthRequired` /
      `enterDenied` / `dropDeadProjects`, `gateAuthState` fail-fast with
      store-presence self-heal, `HubAccessDeniedError` replacing
      `Unexpected status 403`, wiring via `buildAuthOptions`.
- [x] `e2e-auth.test.ts` extended (real hub binary + mock IdP + real
      keyring): grant revoked mid-session (IdP `invalid_grant`, 45 s TTL
      forces refresh) → next tool call returns the ReauthRequired message in
      <10 s and the keyring is wiped; ban `test-subject-1` mid-session
      (stopped-hub `revocations.json` write + restart, F1's enforcement) →
      WS reconnect 403 → adapter evidence → stderr terminal message → next
      tool call answers with the banned/allowlist message in <10 s, keyring
      kept. `TEST_REFRESH_TOKEN` exported from `test-idp.ts` for the
      revocation hook.
- [x] Verification: sync-client vitest 137/137; hub-mcp vitest 246 passed /
      3 platform-skipped; `cd hub-client && npm run build:all` ✓ and
      `npm run test:ci` 130/130 (sync-client bundles from source; the lazy
      `ws` import untouched); `cargo xtask build-hub-mcp-bundle && cargo
      build --bin q2` → `q2 mcp --launcher-info` shows the fresh embed
      (bundle-hash 0fa70f4f3a9bbdd4, gitCommit = HEAD).
- [x] E2E through the real binaries: `npx vitest run src/e2e-auth.test.ts`
      (ts-packages/quarto-hub-mcp) → 3/3 passed (2026-08-03), with channel B
      driving the real `q2 mcp` launcher (embed fresh; no fallback notice).
      Observed outputs: `create_project` after revocation → "…credentials
      have expired or were revoked. Ask me to authenticate again." ;
      post-ban stderr → "[hub-mcp] Your account is not allowed on this
      Quarto Hub…" ; post-ban `read_file` → the same denial message.

## F3 — discriminate `/auth/me` `exp` (`bd-aw8f3sp8`)

### Design

- Add a discriminator to `AuthMeResponse` (`server.rs`, struct at `:1208`,
  handler at `:1268-1288`): `credential: "session" | "bearer"` (naming: match the
  `AuthenticatedUser` variants; final name at implementation). Document
  `exp` as *the expiry of the presented credential* — sliding for sessions,
  fixed for Bearer. Additive; no field removed, Bearer keeps returning `exp`.
- SPA: extend `authService.AuthState` with the field; **remove the dead
  `DEFAULT_SESSION_MS = 1 h` fallback** in `useAuth.ts:42` — when `exp` is
  absent, schedule *no* expiry re-check (the mount check, visibility-change
  re-check, hourly `useSessionKeepAlive`, and disconnected `useAuthProbe`
  all remain; a sliding-session hub always reports `exp` — the server field
  is a non-optional `i64` — so the fallback is unreachable today and
  168×-too-frequent if it ever weren't). The tests currently pinning the
  1 h fallback are `useAuth.test.tsx:186` and `:204`
  (`vi.advanceTimersByTime(3600 * 1000 + 2000)`).

### Work items (TDD)

- [x] Tests first, hub integration (extended the two existing `/auth/me`
      tests in `session_auth.rs`, both observed failing pre-fix):
      `auth_me_returns_sliding_exp_from_session` now asserts
      `credential == "session"`; `auth_me_supports_bearer` asserts
      `credential == "bearer"` alongside the token's own `exp`.
- [x] Tests first, hub-client (both observed failing pre-fix):
      `authService.test.ts` gains a `fetchAuthMe` mapping test
      (`exp` → `expiresAt` ms + `credential` passthrough);
      `useAuth.test.tsx`'s two 1 h-fallback-pinned tests rewritten to
      schedule from an explicit server `expiresAt` (behavior coverage
      kept), plus a new spec: absent `exp` → zero re-checks across a
      simulated week (the retired fallback would have fired ~168×).
- [x] Implemented: `AuthMeResponse.credential: &'static str`
      (`"session"`/`"bearer"`, mirroring the `AuthenticatedUser`
      variants) with `exp` re-documented as the presented credential's
      expiry; SPA `AuthState`/`AuthMeResponse` gain
      `credential?: AuthCredentialKind`, `DEFAULT_SESSION_MS` deleted,
      and the expiry-re-check effect skips scheduling when `expiresAt`
      is absent (mount/visibility/keep-alive/probe checks unchanged).
- [x] Full `cargo xtask verify`: pass (see session log). E2E through the
      real hub binary (scratchpad `auth-me-credential-e2e.mjs`, mock IdP):
      Bearer `/auth/me` → 200, `credential:"bearer"`, `exp ≈ now+600 s`
      (the token's own); `/auth/session`-minted cookie → 200,
      `credential:"session"`, `exp ≈ now+7 d` (sliding). All checks passed
      (2026-08-03). hub-client changelog committed via the two-commit
      workflow.

## Verification (whole epic)

Per CLAUDE.md: TDD per item (fail → implement → pass), full
`cargo nextest run --workspace` after each finding lands, `cargo xtask
verify` at the epic tip (full, not `--skip-hub-build` — F2/F3 touch
ts-packages/hub-client), and end-to-end through the real binaries with
invocation + observed output recorded in this plan before any strand closes.

## Risks

- **F1 audit-ordering trap:** the ledger check must precede the `auth_ok`
  emission inside the claims path — a post-hoc check in the dispatch arm
  would log allow-then-deny for the same request.
- **F1 mint-path leakage:** `authenticate_claims` is shared by the mint
  endpoints; an unconditional in-place check would break same-second
  re-login after `logout-everywhere` (the `min_auth_time` clamp exists
  precisely for that). The mint regression test pins this.
- **F2 upstream variance:** `'unexpected-response'` is a `ws`-specific
  EventEmitter event; keep it an optional capability of the factory seam so
  fakes and any future transport swap degrade to today's behavior, never
  crash.
- **F2 false terminals:** wiping the keyring or stopping retries on
  non-definitive evidence would strand offline users — the evidence rule
  (definitive 401/403 or terminal refresh error only) is the guard; network
  errors must remain state-neutral.
- **F1×F2 clock-granularity race (accepted, not mitigated):** the Bearer
  anchor is IdP-clock `iat` against hub-clock `not_before = now + 1` —
  the session path's mint clamp has no Bearer equivalent (the hub doesn't
  mint the token). A refresh landing in the same second as the revocation
  (or with the IdP clock behind the hub's by skew) still 401s, and F2's
  single forceRefresh+reprobe would then surface a spurious
  `ReauthRequired`. Window is ≤1 s plus NTP-level skew; consequence is one
  re-run of `authenticate`. Documented here so nobody "fixes" it with a
  leeway that re-opens the revocation window.
- **F3 back-compat:** additive field only; do not change `exp`'s presence on
  either path.

## References

- Review context: `2026-07-06-hub-server-minted-sliding-sessions.md` (the
  sliding-session design + "Bearer unchanged" coexistence),
  `2026-05-28-hub-mcp-loopback-pkce.md` (MCP auth; `sub_denylist`
  future-work note F1 discharges), `2026-06-10-ws-auth-expiry-handling.md`
  (the SPA evidence-based probe F2 mirrors),
  `2026-06-11-q2-mcp-hub-auth.md` (q2 mcp launcher / bundle rebuild chain).
- Key files: `crates/quarto-hub/src/context.rs` (566-672 Bearer claims path,
  749-781 session revocation handling, 823-840 dispatch),
  `crates/quarto-hub/src/revocation.rs` (130-143 ledger API),
  `crates/quarto-hub/src/server.rs` (1268-1288 auth_me, 1584-1590 WS
  validate-once),
  `ts-packages/quarto-sync-client/src/NodeWebSocketClientAdapter.ts`
  (50-73 seam, 181-221 openSocket/onClose),
  `ts-packages/quarto-hub-mcp/src/connection-manager.ts` (448-538 probe +
  policy), `hub-client/src/hooks/useAuth.ts`,
  `hub-client/src/services/authService.ts`.

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

- [ ] Tests first (extend `crates/quarto-hub/tests/integration/` —
      `auth_bearer.rs` has the mock-IdP fixtures, `support.rs` the hub
      builder; observe all failing):
      - banned `sub` + otherwise-valid Google Bearer → 403 on `/health` **and**
        on the WS upgrade; audit shows `user_banned` / `credential_kind=bearer`;
      - `logout-everywhere` (or a direct ledger `not_before` write) then a
        Bearer whose `iat` predates it → 401, audit `bearer_revoked`;
      - a Bearer with **no `iat` claim** while a `not_before` entry exists →
        401 (the fail-closed anchor);
      - a Bearer minted *after* the revocation instant → 200 (the self-heal
        path);
      - mint regression: login via `/auth/callback` immediately after
        `logout-everywhere` still succeeds (the `min_auth_time` clamp — pins
        that F1 didn't leak into the mint path);
      - session-path regression: existing revocation/ban session tests
        untouched and green.
- [ ] Implement the ledger check on the Bearer credential path per the design
      constraints above.
- [ ] Docs: update `ts-packages/quarto-hub-mcp/README.md:243-249` (the
      residual-window paragraph — the window is now closed for bans and
      post-revocation ID tokens; refresh-token caveat stated),
      `dev-docs/quarto-hub/session-auth-operations.md` (bans now also deny
      Bearer/MCP; live-socket caveat unchanged), and strike the
      `sub_denylist` future-work note in
      `2026-05-28-hub-mcp-loopback-pkce.md` with a pointer here (it appears
      **three times**: `:650`, `:994`, `:1189` — the future-work list entry
      at `:1189` is the main one; annotate all three).
- [ ] `cargo nextest run --workspace`; `cargo xtask verify --skip-hub-build`
      (Rust-only change).
- [ ] E2E per policy: drive the real `hub` binary + real `q2 mcp` (or the
      hub-mcp dist bundle) with a mock IdP; ban the sub in
      `revocations.json` (stopped-hub procedure), restart, observe the MCP
      probe/WS get 403 while a non-banned identity still works. Record
      invocation + output here.

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

- [ ] Tests first, sync-client (vitest, fake factory): scripted upgrade-401
      fires `onAuthRejected` exactly once per episode; scripted network
      close/error fires nothing and keeps retrying; `getBearer` throwing
      `ReauthRequired`-shaped errors fires `token-refresh-terminal` and stops
      the retry loop; a factory without the status capability behaves as
      today (no report, no crash).
- [ ] Tests first, hub-mcp (vitest): 401 evidence → forceRefresh + reprobe →
      recovery (no user-visible state change); persistent 401 → invalidate +
      reauth-required + next tool call returns `ReauthRequired` promptly;
      403 evidence and initial-probe 403 → terminal message, keyring intact;
      network-only failures never change auth state.
- [ ] Implement adapter evidence reporting, then manager policy.
- [ ] Extend `e2e-auth.test.ts` (real hub + mock IdP + real keyring):
      mid-session grant revocation (mock IdP returns `invalid_grant`, short
      TTL forces the refresh) → next tool call reports `ReauthRequired`
      instead of hanging; with F1 landed: ban the sub mid-session → reconnect
      → 403 → terminal message.
- [ ] `cd hub-client && npm run build:all && npm run test:ci` (sync-client is
      bundled from source); sync-client + hub-mcp vitest suites;
      `cargo xtask build-hub-mcp-bundle && cargo build --bin q2` so the q2
      embed picks the change up (verify with `q2 mcp --launcher-info`).
- [ ] E2E through the real binary per policy; record invocation + output.

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

- [ ] Tests first, hub integration: `/auth/me` with a session cookie →
      `credential: "session"` + sliding `exp`; with a Google Bearer →
      `credential: "bearer"` + the token's own `exp`.
- [ ] Tests first, hub-client (vitest): `useAuth` schedules from a present
      `exp`; absent `exp` → no scheduled expiry re-check (replaces any test
      pinning the 1 h fallback).
- [ ] Implement server field + doc comment; SPA type + fallback removal.
- [ ] Full `cargo xtask verify` (touches hub + hub-client); hub-client
      changelog two-commit workflow applies to the SPA half.

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

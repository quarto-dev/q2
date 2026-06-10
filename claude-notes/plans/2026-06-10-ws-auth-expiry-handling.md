# WS auth-expiry handling (bd-3o8zmz46)

## Overview

Diagnosed 2026-06-10: when the Google ID token (1 h `exp`) expires and GIS One
Tap renewal is unavailable (observed `accounts.google.com/gsi/status` 403 in
dev), the auth cookie is evicted by the browser and every Automerge WS upgrade
gets 401 from the hub. The SPA never notices:

- Browsers hide the HTTP status of a failed WS upgrade from JS, and nothing
  probes `/auth/me` out-of-band when sync drops.
- `useAuth`'s visibilitychange handler resets `cookieSetAt = Date.now()` on
  every refocus where `/auth/me` still returns 200, drifting the assumed
  expiry up to an hour past the real one, so the `cookieExpired()` guards and
  the App's auth-lost effect never fire.

Net effect: UI stays "logged in", shows "Connection lost — working offline",
and the WS adapter retries forever.

Evidence record (end-to-end): probed `ws://localhost:5173/ws` (Vite proxy)
and `http://127.0.0.1:3000/ws` directly → both 401; user's DevTools showed
the failing upgrades carry **no Cookie header** (cookie evicted at expiry);
`fetch('/auth/me')` → 401 while the editor remained open; page reload landed
on the login screen.

**Offline-mode invariant (drives all fixes):** only a definitive 401/403 from
a reachable hub may clear auth. Network errors, timeouts, and 5xx must never
log the user out — offline editing is a feature. Logout on evidence, not on
schedule.

Related: bd-ey6jg70f (hub-minted sliding sessions, out of scope here).

## Phase 1 — server: expose token expiry on /auth/me

- [x] Failing test: `/auth/me` response includes `exp` (epoch seconds) equal
      to the token's `exp` claim
- [x] Surface `exp` in `OidcClaims` (if not already deserialized) and add it
      to `AuthMeResponse` in `crates/quarto-hub/src/server.rs`
- [x] Targeted tests pass (`cargo nextest run -p quarto-hub`)

## Phase 2 — client: schedule from real expiry; offline-safe catches

- [x] Failing tests (vitest, fake timers) for `useAuth`:
  - schedules silent refresh at `exp − 15 min` and hard re-check at `exp`,
    from the server-reported `exp` (not a local 1 h assumption)
  - refocus `/auth/me` 200 does NOT extend assumed expiry beyond server `exp`
    (the drift bug)
  - refocus `/auth/me` network error keeps auth (today: logs out)
  - expiry-time `/auth/me` network error keeps auth and reschedules a
    re-check (today: logs out)
  - expiry-time `/auth/me` 401 with failed renewal → auth cleared with
    `sessionExpired` flag
- [x] Implement: `AuthState.expiresAt` (from `exp`, fallback +1 h if absent),
      remove `cookieSetAt` drift resets, offline-safe catch branches,
      `sessionExpired` state exposed from the hook
- [x] Targeted vitest run green

## Phase 3 — client: WS-failure auth probe + session-expired UX

- [x] Failing tests for new `useAuthProbe` hook:
  - while sync is disconnected (and authed, project open): probes
    `/auth/me` immediately and then on an interval
  - probe 200 → no action; probe network error → no action (offline mode)
  - first 401/403 → `triggerRefresh()` only (renewal gets a chance)
  - second consecutive 401/403 → `onAuthRejected()` (clears auth)
  - reconnect / 200 resets the strike counter; probing stops when online
- [x] Implement `useAuthProbe`, wire in `App.tsx` off `isOnline`
- [x] LoginScreen shows "Session expired — please sign in again" when auth
      was cleared by evidence (distinct from the generic offline banner and
      from ordinary logout)
- [x] Targeted vitest run green

## Phase 4 — verification & bookkeeping

- [x] `npm run build:all` from hub-client (production build is stricter)
- [x] `npm run test:ci` from hub-client
- [x] `cargo xtask verify` (full: quarto-hub Rust change + hub-client change)
- [x] Commits (hub-client two-commit changelog dance), strand comment + close
- [x] Honest E2E note: real-expiry flow needs a browser with a live Google
      session; record what was and wasn't exercised end-to-end

## Design details

- Probe three-way split maps directly onto `fetchAuthMe()`'s contract:
  `AuthState` = valid, `null` = definitive 401/403, throw = network/5xx.
- Strike-2 semantics: first 401 triggers renewal; only a second 401 on the
  next probe cycle (~30 s later) clears auth. Avoids racing One Tap and
  avoids relying on One Tap callbacks ever firing (they may not when GIS is
  blocked — the coalesced `isRefreshing` flag would otherwise wedge).
- Cold-starting the app while fully offline still lands on login (mount-time
  probe can't succeed; HttpOnly cookie unreadable client-side). Known
  limitation, unchanged by this strand; belongs to bd-ey6jg70f territory.

## Verification record (2026-06-10)

- `cargo xtask verify` (full): 9648 Rust tests passed; hub-client 574 unit +
  66 integration + 81 wasm tests passed; WASM + production builds green.
- New tests: `auth_me_returns_token_exp` (Rust); 5 expiry/offline tests in
  `useAuth.test.tsx`; 6 tests in `useAuthProbe.test.tsx`; 2 LoginScreen tests.
- Commits: `465de01f` (fix), `329e3702` (changelog).
- **E2E honesty note**: the real expiry flow (Google token aging past 1 h with
  One Tap blocked) was NOT exercised end-to-end in a browser — it needs a live
  Google session. Manual recipe: sign in, delete the `quarto_hub_token` cookie
  in DevTools → Application, restart the hub (drops the established WS); the
  client should attempt renewal within ~30 s and land on the login screen with
  the "session expired" message within ~60 s, instead of retrying forever.

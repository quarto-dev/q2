# Retire GIS / One-Tap silent renewal (renewal-only scope)

**Epic:** `bd-qxgoti2b` — "Unify hub-client and hub-mcp auth on Authorization Code
+ PKCE." This plan is the renewal-retirement slice, referred to as **B2** below;
the deferred public-client (PKCE) login replacement is referred to as **B1**. This
plan is self-contained and does not depend on any other plan document.

## Overview

Server-minted sliding sessions (`bd-ey6jg70f`, merged in #414) made the browser's
GIS **One-Tap silent renewal** redundant: the hub now re-issues the session cookie
server-side on authenticated HTTP activity (idle 7 d / absolute 30 d), driven by
`useSessionKeepAlive`'s periodic `/auth/me` probe. One-Tap had already been demoted
to a documented *fallback*.

This plan **removes the One-Tap renewal path entirely** and deletes the *client*
code that existed only to serve it. After this lands, session renewal is
**exclusively** server-side sliding re-issue; a session that hits the absolute cap,
goes idle past the window, or is revoked ends definitively and the user re-logs-in
through the existing GIS button.

On the server, the endpoint One-Tap posted to (`/auth/refresh`) is **kept but
renamed to `/auth/session`** — see the decision section. It is not a One-Tap
artifact; it is the direct-JSON credential-submission **login** path for non-Google
(Generic) OIDC frontends, and B1 will build on it. "Refresh" was only ever its name
because One-Tap used it to refresh; with One-Tap gone, the name is corrected to
describe what it actually does (mint a session from a submitted OIDC ID token).

### Scope: **renewal-only**

The GIS coupling splits into two independent halves:

| Half | Client | Server endpoint | In scope? |
|------|--------|-----------------|-----------|
| **Login** | `<GoogleLogin ux_mode="redirect">` button | `/auth/callback` (form_post + `g_csrf_token`) | **KEEP** |
| **Renewal** | `useGoogleOneTapLogin` (One-Tap) → `refreshToken()` | `/auth/refresh` (JSON ID-token resubmit) | client: **REMOVE**; server endpoint: **KEEP + RENAME** |

The GIS **login** button and `/auth/callback` stay untouched — removing them would
leave no way to sign in until B1 (a replacement public-client login) is built, and
B1 is deferred. This plan therefore does **not** touch: JWKS validation, the audience
allowlist, `--oidc-*` flags, `/auth/callback`, `g_csrf_token`, the
`GoogleDoubleSubmit` CSRF mode, or the GIS CSP entries.

The **renewal** half is removed *on the client* (the One-Tap hook, the `refreshToken()`
service helper, and all the `useAuth` renewal machinery). The server endpoint the
client posted to is **not** a renewal artifact — it is retained and renamed (below).

### Behavior change (intended, not a regression)

Before: on a definitive session end (revocation / absolute cap), One-Tap could
silently restore the session without a click. After: the SPA shows the login screen
and the user clicks "Sign in with Google." This is the intended sliding-sessions
model — day-to-day renewal is invisible (server-side slide); re-authentication is
required only at the rarely-reached hard boundaries.

## Decision: **keep** `/auth/refresh`, rename it to `/auth/session`

`/auth/refresh` (`server.rs:996`) is **generic in shape** — it accepts any OIDC ID
token as JSON, validates it through the full `authenticate_claims` path, and mints a
session cookie (`X-Requested-With` CSRF, dual-credential 400 rule). Its only *client*
caller today is GIS One-Tap (verified: the sole call chain is `useAuth` →
`provider.useSilentRenewal` = `useGoogleOneTapLogin` → `refreshToken()` →
`POST /auth/refresh`; `noopAuthProvider.useSilentRenewal` is a no-op, and hub-mcp
never calls it — its `refreshToken` symbols are the OAuth2 refresh-token *grant*, an
unrelated concept). But the **endpoint itself is not One-Tap-specific.**

### Why keep it

Two reasons make retaining the endpoint the right call:

- **`/auth/refresh` is the only login/mint endpoint a Generic OIDC deployment has.**
  `/auth/callback` is registered **only** for providers where
  `AuthConfig::uses_form_post_callback()` is true — i.e. Google alone
  (`server.rs:1329-1334`, `auth.rs:138-143`). The Generic provider does **not** get
  `/auth/callback`, and its `callback_csrf_mode` is `OidcState` = "not yet
  implemented; fails safe" (`auth.rs:152-156`). So for a non-Google OIDC frontend,
  the JSON-submission endpoint is the *only* way to mint a session. Deleting it would
  remove Generic-provider login entirely. The handler's own doc already says as much:
  "the recommended credential submission endpoint for non-Google OIDC frontends
  (instead of the Google-specific `/auth/callback`)" (`server.rs:992-993`).
- **The deferred public-client login work (B1) wants exactly this shape.** It will
  use this endpoint as the sink for the browser's OIDC ID token; keeping it means B1
  builds *on* it rather than re-adding it.

### Why rename it

With One-Tap gone, "refresh" is a **misnomer** — the endpoint never refreshed
anything server-side (sliding re-issue does that); it minted a fresh session from a
resubmitted credential, which is *login*. Its true remaining role is the direct-JSON
login counterpart to the redirect/form-post `/auth/callback`. The chosen name is
**`/auth/session`**: `POST /auth/session` = "create a session," which
reads cleanly next to `GET /auth/me` (read the session) and `POST /auth/logout`
(destroy it), and covers both first-login and re-login without implying "first time."

**Renaming now is low-risk:** the same change removes the only live client caller
(`authService.refreshToken()`), so after B2 *nothing* depends on the old name until
B1 adds a purpose-named client helper (e.g. `createSession()`) pointing at
`/auth/session`.

**Test migration is mechanical.** The five `session_auth.rs` tests that post to
`/auth/refresh` all run on **Generic-provider** hubs (which do *not* register
`/auth/callback`), so they cannot be re-pointed to `/auth/callback` — but they *can*
follow the rename to `/auth/session` with a mechanical URL swap, staying on their
existing Generic fixtures (which is exactly correct, since `/auth/session` is the
Generic-provider login path).

## Current surface (file:line)

### Client — renewal machinery to remove
- `hub-client/src/auth/AuthProvider.tsx` — `useSilentRenewal(opts)` on the interface (`:48`), `SilentRenewalOpts` (`:68`), the `noopAuthProvider.useSilentRenewal` no-op (`:80`), and GIS-referencing doc (`:35-53`, `:60-61`).
- `hub-client/src/auth/GoogleAuthProvider.tsx` — `useGoogleOneTapLogin` import (`:15`), `useSilentRenewal` impl (`:37-54`), its wiring into `googleAuthProvider` (`:58`). **Keep** `SignInButton` (`:24-35`) and `signOut` (`:59`).
- `hub-client/src/auth/MockAuthProvider.tsx` — `lastSilentRenewalOpts` field + capture (`:31`, `:42`, `:47`, `:72-73`) and its doc (`:8-11`).
- `hub-client/src/hooks/useAuth.ts` — `REFRESH_BUFFER_MS` (`:45`), `REFRESH_VERDICT_TIMEOUT_MS` (`:54`), `refreshEnabled` state (`:60`), `isRefreshing`/`refreshDeadline` refs (`:62-63`), `settleRefresh` (`:114`), `abandonRenewal` (`:121`), `triggerRefresh` (`:128`), the `provider.useSilentRenewal({...})` block (`:141-164`), the pre-expiry `refreshTimer` (`:216-219`), the `isRefreshing` branch of the expiry re-check (`:230-236`), the visibility `triggerRefresh()` call (`:189`), and `triggerRefresh` in the return (`:260`).
- `hub-client/src/services/authService.ts` — `refreshToken()` (`:120-135`, posts to `/auth/refresh`); `resolveActorId`'s `onSessionExpired` semantics/doc (`:75-105`, esp. `:87`).
- `hub-client/src/hooks/useAuthProbe.ts` — `triggerRefresh` opt (`:27`, `:32`, `:35-38`, `:58`).
- `hub-client/src/hooks/useSessionKeepAlive.ts` — `triggerRefresh` opt (`:43`, `:46`, `:49-52`, `:67`) and doc (`:18`).
- `hub-client/src/App.tsx` — passes `triggerRefresh` into `useAuthProbe`/`useSessionKeepAlive`/`resolveActorId` (`:95`, `:134`, `:145`, `:160-161`).
- `hub-client/src/main.tsx` — **no change**: GIS `<GoogleOAuthProvider>` wrap stays (login button needs it).

> **Note:** removing `authService.refreshToken()` deletes the *only* live caller of
> the server endpoint. The endpoint is retained for B1 / Generic OIDC frontends;
> B1 will add a fresh, purpose-named client helper pointing at `/auth/session`.

### Client — tests to rewrite/remove
- `hub-client/src/auth/GoogleAuthProvider.test.tsx` — the `useSilentRenewal` describe block (`:59-61+`, plus the `useGoogleOneTapLogin` mock at `:33` and `SilentRenewalOpts` import at `:11`) → remove; keep any `SignInButton`/`signOut` tests.
- `hub-client/src/auth/AuthProvider.test.tsx:23` — drop `useSilentRenewal` from the inline stub.
- `hub-client/src/hooks/useAuth.test.tsx` — the entire renewal suite (`lastSilentRenewalOpts`, `triggerRefresh`, `REFRESH_BUFFER_MS` timing tests: `:156`–`:638` in large part) → replace with definitive-expiry tests (below).
- `hub-client/src/hooks/useAuthProbe.test.tsx` / `useSessionKeepAlive.test.tsx` — the `triggerRefresh` expectations (`:65`–`:109`, `:83`–`:105`) → assert the definitive-reject path instead.
- `hub-client/src/services/authService.test.ts` — the `refreshToken` describe block (`:115`–`:169`) → remove; keep/adjust `resolveActorId` tests (`:261`+) for the new `onSessionExpired` meaning.

### Server — to rename (`/auth/refresh` → `/auth/session`)
- `crates/quarto-hub/src/server.rs` — `RefreshRequest` struct → `SessionRequest` (`:802-806`), `auth_refresh` handler → `auth_session` (`:996`–`:1057`), the route `.route("/auth/refresh", post(auth_refresh))` → `.route("/auth/session", post(auth_session))` (`:1313`). Rewrite the handler doc (`:985-995`): drop the "Google One Tap silent refresh" framing; describe it as the direct-JSON credential-submission **login** endpoint for OIDC frontends (the counterpart to form-post `/auth/callback`). Keep the CSRF (`X-Requested-With`) and dual-credential-400 behavior verbatim — only names/docs change, not logic.
- `crates/quarto-hub/src/context.rs:553` — comment `auth_callback`/`auth_refresh` → `auth_callback`/`auth_session`.
- `crates/quarto-hub/src/revocation.rs:148` — comment `auth_refresh` → `auth_session`.
- `crates/quarto-hub/tests/integration/session_auth.rs` — `/auth/refresh` uses at `:502`, `:614`, `:1061`, `:1133`, `:1294` → swap the URL to `/auth/session`; rename `auth_refresh_mints_session_cookie` (`:491`) → `auth_session_mints_session_cookie`. **No fixture change** — all five stay on their Generic hubs (`session_setup`, the inline ban `SETUP`, `rotated_session_setup`), which is correct because `/auth/session` is the Generic-provider login path.

## Phases (TDD-first)

### B2.0 — Tests first (write/adjust to fail before touching impl)
- [x] **Server:** `POST /auth/refresh` returns **404** (old name gone); `POST /auth/session` mints a session cookie (renamed route, XRW-CSRF, dual-credential-400 preserved); `/auth/callback` still mints for the Google hub; `/auth/me` + `/auth/actor` still accept the cookie and Bearer. Revocation / absolute-cap / ban-at-mint / rotated-kid behavior stays proven **through `/auth/session`** — the same tests, re-pointed by URL, on their existing Generic fixtures.
- [x] **`useAuth`:** (a) a definitive 401 from the expiry-time re-check → `sessionExpired === true`, `auth === null`, and **no** provider renewal is ever attempted; (b) a definitive 401 on refocus → same; (c) a **network error** on refocus / re-check → session **preserved** (evidence-based logout invariant, `bd-3o8zmz46`); (d) the hook's returned API **no longer** exposes `triggerRefresh`; (e) a sliding `/auth/me` (later `exp`) reschedules without logout.
- [x] **`AuthProvider` interface:** type-level — `AuthProvider` has no `useSilentRenewal`; `MockAuthProvider` has no `lastSilentRenewalOpts`.
- [x] **`useAuthProbe` / `useSessionKeepAlive`:** a definitive 401 invokes the reject/`onAuthState`-expired path (→ `expireSession`), never a renewal trigger; a network error is a no-op. **`useAuthProbe` keeps its two-strike debounce:** strike 1 (first 401) becomes a no-op that just records the strike, strike 2 (second *consecutive* 401, ~30 s later) calls `onAuthRejected` → `expireSession`; any intervening 200 resets `strikes = 0`. Rationale: the strike count is client-side UX, not a security boundary — the server rejects every request the instant a session ends, and the probe only runs while the WS is *already* disconnected (no new data flows, no writes persist during the window), so two-strike does not widen access to anything protected. It does buy robustness: a single transient 401 (multi-instance deploy / key-rotation race) followed by a 200 no longer flaps the user to the login screen, and this stays within the evidence-based-logout invariant (two 401s is stronger evidence, still never a network-error logout). The prompt-logout case is unaffected: `useAuth`'s expiry-time re-check already logs out on the *first* 401 past the token `exp`, preempting the probe; two-strike only governs *unexpected* mid-session 401s while offline (revocation/ban), where a brief debounce is exactly right. Update the hook doc to drop the "first strike triggers silent renewal" line and state the no-op-first-strike debounce reason. Assert both strikes in tests.
- [x] **`authService`:** `refreshToken` is gone; `resolveActorId` on 401 calls `onSessionExpired` (now "session ended") and returns `null`; auth-disabled and success paths unchanged.

### B2.1 — Server: rename `/auth/refresh` → `/auth/session`
- [x] Rename the route (`server.rs:1313`), the `auth_refresh` handler → `auth_session`, and `RefreshRequest` → `SessionRequest`. Rewrite the handler doc to the generic-OIDC-login framing (drop One-Tap language). **Logic unchanged.**
- [x] Update the comments in `context.rs:553` and `revocation.rs:148`.
- [x] Re-point `session_auth.rs`: swap `/auth/refresh` → `/auth/session` at the five call sites; rename `auth_refresh_mints_session_cookie` → `auth_session_mints_session_cookie`. No fixture migration.
- [x] Grep for stragglers: `rg 'auth/refresh|auth_refresh|RefreshRequest'` across `crates/`, `hub-client/`, `ts-packages/`, `docs/` must come back clean (except intentional 404-assertion test strings and hub-mcp's unrelated OAuth `refresh_token` grant).
- [x] `cargo nextest run -p quarto-hub` green.

### B2.2 — Client: trim the `AuthProvider` seam
- [x] Remove `useSilentRenewal` + `SilentRenewalOpts` from `AuthProvider.tsx`; drop the `noopAuthProvider` no-op member.
- [x] `GoogleAuthProvider.tsx`: drop the `useGoogleOneTapLogin` import and the `useSilentRenewal` impl; keep `SignInButton` + `signOut`.
- [x] `MockAuthProvider.tsx`: drop `lastSilentRenewalOpts` and its capture.

### B2.3 — Client: strip renewal from `useAuth`
- [x] Delete `triggerRefresh`, `refreshEnabled`, `isRefreshing`, `refreshDeadline`, `settleRefresh`, `abandonRenewal`, the `provider.useSilentRenewal({...})` block, `REFRESH_BUFFER_MS`, `REFRESH_VERDICT_TIMEOUT_MS`, and the pre-expiry `refreshTimer`.
- [x] Route every **definitive** 401 (mount, refocus, expiry re-check) to `expireSession`; keep network-error branches as no-ops (invariant preserved). Collapse the `isRefreshing` ambiguity out of the expiry re-check.
- [x] Remove `triggerRefresh` from the returned object; keep `expireSession`/`applyAuth`/`sessionExpired`.

### B2.4 — Client: update consumers
- [x] `useAuthProbe.ts` / `useSessionKeepAlive.ts`: drop the `triggerRefresh` opt; on definitive 401 call the reject/expire path (per the B2.0 strike-count decision).
- [x] `App.tsx`: stop passing `triggerRefresh`; wire the reject path to `expireSession`; update the `resolveActorId` call site (`:160`) to pass `expireSession` as `onSessionExpired`.

### B2.5 — Client: `authService` cleanup
- [x] Remove `refreshToken` (and its `/auth/refresh` fetch). Update `resolveActorId` doc: `onSessionExpired` now means "the session ended; show login," not "start a silent refresh." (No client code should reference `/auth/session` yet — B1 adds that helper.)

### B2.6 — Verification & docs
- [x] `cd hub-client && npm run build:all` (stricter than tsc/vitest — required for hub-client) **and** `npm run test:ci`.
- [x] `cargo nextest run --workspace` and `cargo xtask verify --skip-hub-build` (server-only crate; hub-client covered by build:all above).
- [x] **E2E** (per project policy — tests are necessary but not sufficient): run the hub (`--allow-insecure-auth` won't exercise Google; use a real OIDC-configured hub or `local-prod`), sign in via the GIS button, confirm the session slides via keep-alive across an `/auth/me` cycle, force a definitive 401 (revoke via `/auth/logout-everywhere`) and confirm the SPA lands on the login screen (no silent renewal, no hang). Confirm the rename: `curl -X POST /auth/refresh` → **404**, while `curl -X POST /auth/session` (with `X-Requested-With: XMLHttpRequest`) is *registered* (400/401 on a bad/absent credential, **not** 404). Record the invocations + observed output here.
- [x] Docs: update the One-Tap-as-fallback language in the `useAuth.ts` header and `useSessionKeepAlive.ts` doc (note the `/auth/refresh` → `/auth/session` rename + its clarified generic-OIDC-login role). Consider dropping `@react-oauth/google`'s One-Tap surface from any dev docs.
- [x] **`hub-client/changelog.md`** — two-commit workflow: (1) commit the hub-client changes; (2) commit the changelog entry with the hash under the commit day's `### YYYY-MM-DD` header.

## Verification record (2026-07-27)

Implemented on branch `braid/bd-s042qcxj-retire-onetap-renewal`; commits
`e73786ed` (code) + `bccddf93` (changelog).

**Automated:**
- `quarto-hub`: `cargo nextest run -p quarto-hub` → 371 passed (incl. renamed
  `auth_session_mints_session_cookie` and new `auth_refresh_old_route_is_gone`).
- Workspace: `cargo xtask verify --skip-hub-build` → "All verification steps
  passed!" (build + `nextest --workspace` + ts-packages, CI `-D warnings`).
- hub-client: `npm run build:all` (tsc project-refs + vite) → clean;
  `npm run test:ci` → 740 unit + 109 integration + 129 wasm passed
  (`changelogRender.wasm.test.ts` green for the new changelog entry).

**E2E — rename confirmed through the real `hub` binary** (standalone,
`hub --port 3999`; auth disabled, so `/auth/session` validates-and-rejects
rather than minting — the point here is *route registration*):

```
$ curl -s -o /dev/null -w '%{http_code}' -X POST :3999/auth/refresh \
    -H 'X-Requested-With: XMLHttpRequest' -d '{"credential":"x"}'
404                       # old route gone
$ curl ... -X POST :3999/auth/session -H 'X-Requested-With: XMLHttpRequest' ...
401                       # registered; auth-disabled hub rejects the credential
$ curl ... -X POST :3999/auth/session          # no CSRF header
403                       # route exists, CSRF guard fires — not 404
```

**Not exercised (stated honestly):** the full browser flow — GIS sign-in,
sliding keep-alive across an `/auth/me` cycle, and a `logout-everywhere`
revocation landing the SPA on the login screen — needs a real Google-OIDC hub
and a browser, unavailable in this environment. The client behavior is covered
by the rewritten `useAuth` / `useAuthProbe` / `useSessionKeepAlive` unit tests
(definitive-401 → `expireSession`; network error → session preserved; no
`triggerRefresh` in the returned API).

## Non-goals
- Not touching the GIS **login** button, `/auth/callback`, `g_csrf_token`, or the `GoogleDoubleSubmit` CSRF mode (that's B1 territory, deferred).
- Not touching Google-as-IdP config (JWKS, audiences, `--oidc-*`).
- Not implementing the generic `OidcState` CSRF path or a pre-flight endpoint (B1).
- **Not removing** the generic-OIDC credential-submission endpoint — it is **retained** (renamed `/auth/refresh` → `/auth/session`) and its logic is unchanged. Only its name, doc, and the (now-removed) client caller change.
- `@react-oauth/google` stays as a dependency (the login button still uses `GoogleLogin`/`GoogleOAuthProvider`/`googleLogout`); only its One-Tap surface stops being used.

## Risks
- **Evidence-based-logout regression.** The renewal machinery is interwoven with the "only log out on a definitive 401 from a reachable server" invariant (`bd-3o8zmz46`). The B2.0 network-error tests exist to prevent a regression where a refocus/re-check network error now logs the user out. Treat those as gating.
- **Rename blast radius.** The rename touches the route, handler, struct, five `session_auth.rs` tests, two comments, and the handler doc. Because the same change removes the only live client caller, there is **no live consumer** of the old name after B2 — so this is a safe rename, but the B2.1 grep step is mandatory to catch stragglers (skip hub-mcp's unrelated OAuth `refresh_token` grant symbols).
- **Tests stay on Generic fixtures.** The five `session_auth.rs` tests cannot be re-pointed to `/auth/callback` — that route is unregistered (404) on the Generic hubs they run against. Following the `/auth/refresh` → `/auth/session` rename is a mechanical URL swap on the same fixtures; no callback/Google-fixture migration is needed.

## Braid
- File **B2** as a child strand of epic `bd-qxgoti2b` (type `task`, p2). Reference this plan file in the strand. A `discovered-from` link to `bd-ey6jg70f` (the sliding-sessions work that made this possible) is optional but accurate.

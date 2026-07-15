# hub-client ↔ hub-mcp auth unification (SPA → Authorization Code + PKCE)

**Status:** proposed
**Date:** 2026-07-06
**Part of:** the auth-reshape path — see the umbrella index `claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md`. Independent of the other two plans (connection-gated local-first; server-minted sliding sessions).

## Overview

Migrate the SPA from the Google Identity Services (GIS) button to **Authorization
Code + PKCE** as a **public client**. The browser obtains a Google ID token and
hands it to the *existing* server callback — **no hub token-exchange, no new
server minting/session capability**. The OAuth-config + PKCE primitives *may
later* be extracted into a shared module with hub-mcp, but **v1 builds the SPA
provider standalone** (inlining the small helpers); the shared package is
deferred (see B0) until duplication proves real.

### Key reality that shapes this plan
**A browser is a public OAuth client.** It cannot bind a `127.0.0.1` loopback
listener, cannot use an OS keychain, and must not hold a `client_secret`. The
hub-mcp flow's loopback + `@napi-rs/keyring` + confidential-client refresh are
**inherently native-only** (RFC 8252). So "both use PKCE + loopback + keychain"
is not literally achievable. What genuinely unifies (and *already* does, at the
hub) is the **IdP (Google) + shared JWKS validation + an audience allowlist
explicitly built "to share one hub instance between the SPA and quarto-hub-mcp"**
(`crates/quarto-hub/src/auth.rs:129-131`). This plan unifies at the IdP /
auth-code+PKCE / shared-primitives level; the native-only transport/storage
stays divergent by necessity — that is correct, not accidental drift.

## Current architecture (map for implementers)

### The SPA auth seam (the extension point)
- `hub-client/src/auth/AuthProvider.tsx` — IdP-agnostic interface consumed via `useAuthProvider`. The seam comment (`:58-66`) already anticipates "a future Code+PKCE provider" whose `loginUri` is "the redirect URI the SPA registers." A `noopAuthProvider` handles auth-disabled builds.
- `hub-client/src/auth/GoogleAuthProvider.tsx` — the only file naming GIS APIs (`<GoogleLogin ux_mode="redirect">`, `useGoogleOneTapLogin`, `googleLogout`). This is one implementation behind the interface; a PKCE provider is a sibling.
- Credential sink already exists: silent renewal → `onCredential(jwt)` (`GoogleAuthProvider.tsx:37-54`, `useAuth.ts:119`) → `refreshToken()` → `POST /auth/refresh` (`hub-client/src/services/authService.ts:109-120`). A PKCE provider drops into the same seam and feeds the same id_token to the same endpoint. (Design rationale: `claude-notes/plans/2026-05-20-auth-provider-interface.md:221-224`.)

### Server connection auth (what the browser talks to)
- Stateless OIDC resource server: validates a Google ID-token JWT locally against cached JWKS. **No session store, no hub-minted JWT** — the cookie value *is* Google's token (1 h `exp`, `AUTH_COOKIE_MAX_AGE=3600`). The hub has **zero OAuth-client capability**: no `client_secret`, no `oauth2`/`openidconnect` crate, no token-endpoint call (`Cargo.toml:29-42`; grep for `client_secret|token_endpoint|authorization_code` in `crates/quarto-hub/src/` is empty).
- Credential accepted as `quarto_hub_token` cookie (browser) **or** `Authorization: Bearer` (MCP); both present → 400; neither → 401 (`server.rs:901-950`, `extract_credential` `:212-247`).
- Endpoints: `/auth/callback` (Google form_post → validate → cookie, `:701-729`), `/auth/refresh` (accepts a fresh JWT → validate → re-set cookie, `:844-879`), `/auth/me`, `/auth/actor`, `/auth/logout`, `/ws`, `/health`, `/api/*` (`:1048-1081`). Both `/auth/callback` and `/auth/refresh` are the *receiving* half of a BFF — "receive an already-minted token → validate → cookie it."
- A **stateless** `CallbackCsrfMode::OidcState` variant exists but is a fail-safe stub (`auth.rs:644-664`, `validate_callback_csrf` returns `false` for it at `server.rs:684-688`) — the standard `state`-cookie CSRF path a redirect flow would use.
- Security invariants to preserve: CSRF via `X-Requested-With` (`server.rs:328-338`), WS-origin check (`:345-367`).
- Browser = Web-app Google client (`VITE_GOOGLE_CLIENT_ID` == server `--oidc-client-id`); MCP = separate Desktop-app Google client; hub bridges the two `aud` values via `--additional-audiences` (`auth.rs:129-131`, applied `:599-603`).

### hub-mcp reference (the flow we align to)
- hub-mcp uses Authorization Code + PKCE (S256) + loopback (RFC 8252), but as a **confidential** Desktop-app client: it retains a `client_secret` for the token exchange + refresh grant, and stores tokens in the OS keyring. Its `exchangeCode` (`ts-packages/quarto-hub-mcp/src/auth/auth-tools.ts:440-478`) uses `ClientSecretPost(clientSecret)` — a browser public client can replicate **none** of this. The reusable parts are `oauth-config.ts` (issuer resolution, OIDC discovery) and `pkce.ts` (S256 verifier/challenge/state via `oauth4webapi`) — pure IdP plumbing, portable to the browser.

## Design

### What unifies vs what stays divergent
- **Unifies:** Google as IdP; Authorization Code + PKCE (S256); the hub's shared JWKS validation + audience allowlist (already present); and — optionally, later — shared OAuth-config + PKCE primitive modules (see B0, deferred).
- **Stays divergent (by necessity):** the SPA is a **public** client → origin `redirect_uri` (not loopback), token lands in the server-set HttpOnly cookie (not an OS keychain), no `client_secret`, no locally-stored refresh grant. hub-mcp keeps loopback + keyring + confidential Desktop-app client.

### Flow shape — pattern (i), public-client / ID-token (recommended)
The browser runs authorize + PKCE + `state`/nonce and obtains a **Google ID
token**, which it hands to the *existing* `/auth/refresh` (or `/auth/callback`)
that validates + cookies it — today's model, different acquisition. This keeps
this plan **independent of any server minting** — no dependency on the
sliding-sessions plan. If the redirect is routed through a hub HTTP callback
(rather than landing in the SPA), implement the existing **stateless**
`OidcState` CSRF stub — this is *not* a session store.

**Rejected — pattern (ii), BFF** (hub exchanges the auth code server-side): gives
the hub a brand-new OAuth-*client* role it entirely lacks today (an `oauth2`
client, the Web-app `client_secret`, a token-endpoint call) and pulls
refresh-token + session handling in. Not entailed by "the browser uses PKCE";
adds surface. Do not adopt without a specific driver. **Decide up front** — it
dictates the Google client registration.

## Phases (TDD-first)

- [ ] **B1 — SPA Authorization Code + PKCE provider (pattern i).** New `AuthProvider` implementation: public client, origin `redirect_uri`, PKCE, `state`/nonce; obtains a Google ID token and feeds it to the existing `/auth/refresh` (or `/auth/callback`). **Built standalone — inline the small PKCE / oauth-config helpers (browser Web Crypto); do not block on a shared package (B0, deferred).** Register the Google Web-application `redirect_uri`. Optionally implement the `OidcState` CSRF stub if routing through a hub callback. Tests: full redirect round-trip against a mock IdP; `state`/PKCE verification; `GoogleAuthProvider` still selectable behind a flag during migration.
- [ ] **B2 — Retire GIS/One-Tap renewal.** Replace `useSilentRenewal` (One-Tap) with the chosen renewal path; make `GoogleAuthProvider` legacy/removable. **Sequencing:** the durable renewal path is the sliding-sessions plan — sequence B2 after that plan lands, or keep One-Tap as the interim. Tests: refresh works where One-Tap is blocked (FedCM/3p-cookie).
- [ ] **B4 — E2E verification + docs.** Both `q2 mcp` and a real browser authenticate against one hub via the unified path; both obtain the correct per-project actor. Record invocations + observed output. Docs.

### Deferred / de-scoped

- [ ] **B0 (deferred) — Extract shared OAuth primitives.** Only once B1 proves the browser needs the same `pkce.ts` (30 lines) / `oauth-config.ts` (155 lines) as the hub-mcp Node bundle *and* the duplication is real. The two runtimes use different crypto backends (browser Web Crypto vs Node), so a cross-runtime package boundary is not obviously worth it up front — extract if duplication proves painful, otherwise leave inlined.
- **B3 — out of scope (standalone strand).** The Bearer-on-`/auth/actor`+`/auth/me` fix (`bd-3g0aijb3`) is a small, independent server-side bug that gates nothing; it is tracked as its **own standalone strand**, not a phase of this epic. (It fixes MCP sessions silently falling back to random actors.)

## Non-goals
- The browser will **not** use a loopback listener, an OS keychain, or a `client_secret`. That is a property of public clients, not a gap to close.
- Does **not** introduce hub token-minting or sessions (that's the sliding-sessions plan). Pattern (i) keeps the hub a pure validator.

## Relationship to the other plans
- **Independent of the connection-gated local-first plan.** That plan's "Connect to a hub" action triggers whatever browser flow exists — GIS today, this plan's PKCE flow after it lands.
- **Soft dependency on server-minted sliding sessions:** B2 (retiring One-Tap) needs a durable renewal path. Either sequence B2 after the sessions plan, or keep One-Tap as the interim. B1 has no such dependency.

## Risks & open questions
- **B1 flow-shape decision (pattern i vs BFF):** adopt pattern (i) to keep this plan independent of server minting; BFF re-couples everything. Decide before registering the Google client.
- **Client-id / redirect-uri registration (B1):** requires a Google Web-app client `redirect_uri` registration change; coordinate with whoever owns the quarto-hub.com OAuth clients (see `bd-ra5ypj3s`).
- **Migration overlap:** keep `GoogleAuthProvider` selectable behind a flag until the PKCE provider is proven in production.

## Braid strand structure
- **Epic `bd-qxgoti2b`** (epic, p2, open). Sub-strands **B1, B2, B4** (parent-child). **B0 deferred** (own strand, opened only if duplication proves real); **B3 removed** — `bd-3g0aijb3` is tracked standalone.
- Related links in place: `bd-3g0aijb3` (standalone Bearer fix — related, no longer a phase), `bd-cmp48` / `bd-81cfshmw` (hub-mcp reference), `bd-ra5ypj3s` (client registration), `bd-ey6jg70f` (B2 soft dependency). No hard `blocks` to the other two plans.

## References
### Plans
- `claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md` — umbrella / path.
- `claude-notes/plans/2026-07-06-hub-server-minted-sliding-sessions.md` — the durable renewal path B2 depends on.
- `claude-notes/plans/2026-05-20-auth-provider-interface.md` — the `AuthProvider` seam (done).
- `claude-notes/plans/2026-02-26-httponly-cookie-auth.md` — cookie storage (done).
- `claude-notes/plans/2026-05-28-hub-mcp-loopback-pkce.md` — hub-mcp current auth (loopback + PKCE, confidential Desktop-app client).
- `claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md` — `q2 mcp` launcher (inherits TS auth).
- `claude-notes/plans/2026-05-05-hub-mcp-device-flow-implementation.md` — superseded device flow.

### Strands
- `bd-3g0aijb3` — Bearer on `/auth/actor` and `/auth/me` (standalone; not a phase of this epic).
- `bd-cmp48`, `bd-81cfshmw` — hub-mcp auth (reference).
- `bd-ra5ypj3s` — Google client registration issue (B1).
- `bd-ey6jg70f` — server-minted sliding sessions (B2 soft dependency; own plan).

### Key files
- SPA auth seam: `hub-client/src/auth/{AuthProvider,GoogleAuthProvider,MockAuthProvider}.tsx`, `hub-client/src/hooks/useAuth.ts`, `hub-client/src/services/authService.ts`, `hub-client/src/main.tsx` (provider selection).
- Server auth: `crates/quarto-hub/src/server.rs` (212-247, 328-338, 345-367, 662-729, 844-879, 901-950, 1048-1081), `crates/quarto-hub/src/auth.rs` (129-131, 599-603, 644-664).
- hub-mcp auth (reference): `ts-packages/quarto-hub-mcp/src/auth/{oauth-config,pkce,loopback,browser,auth-tools,refresh-manager,credential-store}.ts`, `crates/quarto-mcp-launcher/src/{lib,defaults}.rs`.

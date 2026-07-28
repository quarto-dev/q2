# Auth hardening — current-flow easy wins (pre-pattern-(ii))

**Status:** proposed — intended to land **before** Epic 2's B1 (the pattern-(ii)
redirect login), as independent hardening of the flow that exists today.
**Epic:** `bd-uv8xynxk` (open; discovered-from `bd-qxgoti2b`). **Date:** 2026-07-27.
**Part of:** the auth-reshape path — umbrella index
`claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md`.

## Overview

The 2026-07-27 security audit that re-scoped Epic 2 to pattern (ii) (hub-side
code exchange — see `2026-07-06-hub-client-auth-unification-pkce.md`
§Decision record) also identified improvements to the **current** GIS →
`/auth/callback` flow that do **not** require the hub to become an OAuth
client. This plan collects them as easy wins to ship first. Every item stands
on its own: if pattern (ii) is later rejected, nothing here is wasted; if it
proceeds, H2's sealed-cookie machinery is a direct stepping stone to its
`OidcState` callback and the rest simply persists.

The four items, by payoff-per-effort:

| Item | What | Size | Closes |
|------|------|------|--------|
| H1 | Deregister `POST /auth/session` for Google deployments | S | Second token-replay mint sink (zero callers today) |
| H2 | Server-verified `nonce` in the GIS login | M | The ~1 h ID-token replay-to-mint window — the audit's main in-place gap |
| H3 | `__Host-` prefix on the session cookie | S | Subdomain cookie-tossing / session fixation |
| H5 | Distinct login-mint audit event | S | Forensics gap: mints indistinguishable from per-request auth |

## Work items

### Phase 1 — independent quick wins (H1, H3, H5; any order, TDD each)

- [ ] **H1 — register `/auth/session` only for Generic OIDC providers.**
  Tests first: with a Google-shaped `AuthConfig`, `POST /auth/session` → 404;
  with a Generic config it still mints; with auth disabled it is absent.
  Then: change the unconditional `.route("/auth/session", post(auth_session))`
  (`server.rs:1316`) to mirror the `uses_form_post_callback()` conditional used
  for `/auth/callback` (`server.rs:1332`) — inverted: register only when auth is
  enabled **and** the provider is Generic (it is documented as the Generic
  provider's only mint endpoint; Google deployments log in via the form-post
  callback and nothing else calls it — grep of `hub-client/src` +
  `ts-packages/` verified empty, 2026-07-27). Migrate any existing integration
  tests that exercise `/auth/session` under a Google-shaped config to a Generic
  config.
- [ ] **H3 — `__Host-` prefix on the session cookie.**
  Tests first: secure-mode mint sets `__Host-quarto_hub_token` with
  `Secure; Path=/; HttpOnly` and no `Domain`; `extract_credential` accepts the
  prefixed name; the legacy name is ignored as a credential and cleared on
  login; insecure-auth mode keeps the unprefixed name (the `__Host-` prefix
  *requires* `Secure`, which requires TLS).
  Then: replace the `AUTH_COOKIE_NAME` constant (`server.rs:130`; 4 use sites,
  all in `server.rs` — the client never reads the name, the cookie is HttpOnly)
  with a mode-aware name helper used by mint, extract, clear, and the
  `session_reissue_layer`. On login, additionally emit a clear-cookie for the
  legacy name for one release so stale cookies don't linger. Existing sessions
  are invalidated once at rollout (users re-log-in); call this out in the PR.
- [ ] **H5 — distinct login-mint audit event.**
  Tests first (follow the existing audit-event test pattern if one exists;
  otherwise assert at the mint-helper level). Emit
  `target: "quarto_hub::audit", action = "login_mint"` with `sub`, the new
  session's `sid`, and `endpoint = "callback" | "session"` on successful mint in
  `auth_callback` and `auth_session` — `mint_session_cookie` (`server.rs:316`)
  currently returns only the `Set-Cookie` string, so it must be widened to
  surface the `sid`. Note the `sid` is generated one layer deeper, in
  `session::mint_session_at`, so it has to be threaded out of there (or
  generated in `mint_session_cookie` and passed in) — not simply read off the
  returned cookie. The `session_reissue_layer` stays silent by design
  (documented at `server.rs:1067` — don't change it).
  Vocabulary should sit beside the existing `auth_fail` /
  `revoke_all_sessions` actions.

### Phase 2 — the substantive win (H2)

- [x] **H2 — server-verified `nonce` in the GIS login.**
  Today the hub validates signature/`iss`/`aud`/`exp` but cannot bind an ID
  token to a login attempt: **any captured Google ID token can be replayed to a
  mint endpoint for its ~1 h validity.** GIS supports a `nonce` at
  `google.accounts.id.initialize`; a hub pre-flight makes it verifiable
  server-side with no OAuth-client role.
  *Design:*
  - `GET /auth/nonce` (new, unauthenticated): generate a 32-byte random nonce;
    return `{ "nonce": … }`; set a **sealed** (HMAC-signed) cookie carrying
    `{nonce, exp}` with `Max-Age` ≈ 600 s. Sign with the existing session keys
    but with **domain separation** (a distinct `typ`/purpose so a sealed
    login blob can never verify as a session token, and vice versa).
  - Cookie attributes: `__Secure-` prefixed name, `HttpOnly; Secure;
    SameSite=None; Path=/auth`. `SameSite=None` is load-bearing: Google's
    credential delivery is a **cross-site form POST**, and `Lax` cookies are
    not attached to those (the `g_csrf` cookie only survives via Chrome's
    2-minute Lax-by-default grace). `__Secure-` rather than `__Host-` because
    we want `Path=/auth`; the HMAC covers the cookie-tossing risk `__Host-`
    would otherwise address.
  - SPA: the nonce must reach `<GoogleLogin nonce={…}>`, but `LoginScreen`
    does **not** render `GoogleLogin` directly — it renders
    `<provider.SignInButton>` (`LoginScreen.tsx:38`) and the Google provider
    renders `GoogleLogin` behind the `SignInButtonProps` boundary
    (`GoogleAuthProvider.tsx:16-27`), an abstraction introduced deliberately
    (see `2026-05-20-auth-provider-interface.md`). Do **not** widen the generic
    `SignInButtonProps` with a Google-specific `nonce`; instead **fetch
    `/auth/nonce` inside `GoogleAuthProvider`** (the nonce is a GIS concern and
    belongs with the GIS provider) and pass it to `<GoogleLogin>` there. Verify
    the `@react-oauth/google` wrapper forwards `nonce` to `initialize` in
    `ux_mode="redirect"` — current versions accept the prop; confirm at
    implementation.
  - Callback: add an optional `nonce` field to `OidcClaims`; verify the sealed
    cookie (signature, expiry) and require `claims.nonce` to match; any
    missing/mismatch/expired/tampered case → `/?auth_error`; clear the sealed
    cookie on both success and failure.
  - **Enforcement is unconditional in secure mode.** Under
    `--allow-insecure-auth` (no TLS), `SameSite=None; Secure` cookies cannot
    function over plain http — the callback logs a warning and skips the check
    there, consistent with that flag's existing "never in production" contract.
  - **Scope: Google/callback flow only.** H2 binds the nonce for the GIS →
    `/auth/callback` path (the audit's target — the ~1 h GIS ID-token replay
    window). It deliberately does **not** touch `/auth/session` (the Generic
    provider's JSON mint), which stays replay-able within the submitted token's
    validity. That is an accepted scope boundary — the audit targeted the GIS
    flow — but note the asymmetry so it is not mistaken for full coverage.
    (H1 still narrows `/auth/session` to Generic deployments only.)
  - *Rollout note:* a user holding a stale SPA bundle (no nonce) fails login to
    `/?auth_error` and recovers on reload. Accept this — login is a full-page
    flow and hub + client deploy together. No compatibility flag unless it
    bites in practice.
  *Tests first:* mock-JWKS helper gains a nonce-claim knob; happy-path round
  trip; missing cookie; nonce mismatch; expired blob; tampered signature;
  sealed blob presented as a session cookie is rejected (domain separation);
  insecure-mode skip logs the warning; cookie cleared after use. Client test:
  `LoginScreen` passes the fetched nonce. **E2E:** real browser login via
  `local-prod:nginx`… noting that insecure mode skips enforcement, the full
  enforcement path is exercised by the integration tests + a TLS-fronted
  staging login before sign-off.
  *Pattern-(ii) synergy:* the sealed short-lived signed-state helper (mint /
  verify / expiry / domain separation) is exactly what the `OidcState`
  callback needs — build it as a reusable unit in `session.rs` or a sibling
  module, not inline in the handler.

## Sequencing & scope

- Proposed order: Phase 1 as one or more small PRs (items are independent),
  then H2. All four before Epic 2's B1 kicks off.
- Out of scope, deliberately: anything giving the hub an OAuth-client role
  (that is B1, `bd-qxgoti2b`, with its own decision record);
  `SameSite=Strict` on the session cookie (negligible gain over Lax + the
  `X-Requested-With` gate); rate-limiting mint endpoints (signature forgery is
  not brute-forceable).
- What this plan **cannot** deliver (only pattern (ii) can): removing the GIS
  third-party script, and a single-use PKCE-bound login exchange — though H2
  captures most of the latter's replay protection.

## Verification record (as implemented)

Branch `feature/auth-hardening-current-flow`. Strands filed at kickoff:
H1 `bd-zbep24xd`, H2 `bd-uqjiac5a`, H3 `bd-gt2hhrcg`, H5 `bd-k2xvvh9f`
(all children of `bd-uv8xynxk`).

### E2E actually performed (and what could not be)

`npm run local-prod` (Node-proxy mode) against a freshly built
`target/debug/hub`: H1 confirmed **through the real binary** — with auth
disabled, `POST /auth/session` → 404 and `POST /auth/callback` → 404
while `GET /health` → 200.

**Not done, explicitly:** no real GIS browser login. It needs a Google
client ID whose authorized origin includes the local-prod host, which
this session does not have, and no browser-automation tooling was
available. H2's nonce round-trip is therefore covered only by the
integration and client tests, not by a browser. This gap should close
with a TLS-fronted staging login before sign-off, tracked as
`bd-fcv3q5kl` — an **open child of the epic**, so `bd-uv8xynxk` cannot
be closed until that verification happens.

Full `cargo xtask verify` (all 14 steps, including the WASM rebuild and
the hub-client build + tests) passes at the branch tip, and the working
tree is clean afterwards — the WASM rebuild produced no tracked diffs.

### H2 — implementation notes

The sealed-state helper landed as its **own module**,
`crates/quarto-hub/src/login_state.rs`, rather than inside `session.rs`
(already 1100+ lines) — the plan allowed either, and a sibling module
makes the pattern-(ii) reuse obvious. It needed two crate-private
accessors on `SessionKeys` (`current_secret`, `secret_for_kid`), since
the raw secrets are otherwise private to `session.rs`.

**Domain separation is doubled, not single.** The plan called for a
distinct `typ`; the implementation also uses a distinct `iss`
(`quarto-hub-login` vs `quarto-hub`), because both verifiers pin their
issuer via `set_issuer`. Either barrier alone suffices; both are asserted,
**in both directions**, at the unit level (a session token must not open
as login state, and a login blob must not verify as a session) and again
across the HTTP surface (`a_sealed_login_blob_is_not_a_session_cookie`,
`a_session_token_is_not_a_login_state_cookie`).

Two deviations from the plan's letter, both deliberate:

- **`GET /auth/nonce` is registered whenever auth is enabled**, not only
  for form-post providers. The SPA then has one unconditional way to get
  a nonce and never branches on the deployment's provider. *Enforcement*
  stays exactly as scoped — only `/auth/callback` checks it.
- **The `login_mint` audit event lives in `mint_session_cookie`**, not
  duplicated in the two handlers as the plan described. Same events, but
  a future third mint call site cannot forget to log.

`@react-oauth/google@0.13.4` was verified to forward `nonce` to
`google.accounts.id.initialize` — it is part of `IdConfiguration`, which
`GoogleLoginProps` spreads through. **But its effect's dependency array
does not include `nonce`**, so a nonce arriving after the first render
would never reach GIS and every login would fail the server check.
`SignInButton` therefore renders nothing until the fetch resolves, and
renders an error rather than a nonce-less button if it fails. A test pins
that ordering explicitly (`does not render GIS before the nonce arrives`)
because it would otherwise look like a removable loading state.

Note: `GoogleOAuthProvider`'s own `nonce` prop is unrelated — it sets the
CSP nonce on the injected `<script>` tag. Do not pass the login nonce
there.

#### Why a dedicated `GET /auth/nonce` and not something cheaper

Asked and settled 2026-07-28. Three alternatives were considered:

- **Embed it in the SPA document.** Impossible: the SPA HTML is served by
  nginx / the static proxy, and the hub never sees that request.
- **Let the client generate the nonce and have the hub seal it.** Breaks
  the scheme outright. An attacker holding a captured ID token can read
  its `nonce` claim (plaintext JWT payload), ask the hub to seal that
  value, and replay. The nonce **must** be server-generated and
  uninfluenced by the caller — which forces a round trip before the IdP
  hop.
- **Fold it into `GET /auth/me`,** the one pre-login request the SPA
  already makes. Rejected: `/auth/me` doubles as the sliding-session
  keep-alive probe (`useSessionKeepAlive`), so it would re-mint the
  login-state cookie repeatedly — clobbering a second tab's in-flight
  login, since the cookie is a single named slot — and it is
  Bearer-reachable from hub-mcp, where browser login state is
  meaningless. Two different lifetimes on one endpoint.

Also rejected: carrying the sealed blob in GIS's `state` parameter
(`GsiButtonConfiguration.state` does return with the ID token) instead of
a cookie. Tempting, because it needs no endpoint *and* would work under
`--allow-insecure-auth`. But `state` comes back as a **form field in the
same POST as the credential**, making both halves of the pair
attacker-suppliable, with no single-use clear. The cookie is not
attacker-chosen, is HttpOnly, and is cleared after one use.

The residual cost is one same-origin GET, serialized after `/auth/me`
because `LoginScreen` renders only once auth state resolves. Not
optimised: the GIS script must load from `accounts.google.com` before the
button can render at all, so a local GET is very unlikely to be on the
critical path. If it ever measures as such, the fix is a module-level
promise cache in `GoogleAuthProvider` (no interface change), not moving
the endpoint.

### Divergences found in passing (filed, not fixed here)

- `scripts/hub-sliding-sessions-e2e.mjs` still logs in via the removed
  `POST /auth/refresh` route, so the session-auth E2E script the
  operator guide points at is already broken (`bd-gppva1ee`).
- The local-prod Node proxy only forwards `/auth` and `/ws`, so
  `/health` and `/api` are served as the SPA fallback instead of being
  proxied to the hub — nginx forwards `^/(auth|api|health)`. Pre-existing
  parity gap (`bd-r28nr1fc`).
- `scripts/local-prod-port.test.mjs` is not wired into any test runner
  (`bd-h2jdelwm`).

## Verification

- Per item: TDD (test fails → fix → passes), then
  `cargo nextest run --workspace`; `cargo xtask verify --skip-hub-build` for
  the Rust-only items (H1, H3, H5).
- H2 touches hub-client: `cd hub-client && npm run build:all` +
  `npm run test:ci`.
- End-to-end (per CLAUDE.md policy — tests alone are insufficient): a real
  browser GIS login through `npm run local-prod:nginx` after H2 (login
  succeeds; then a doctored replay attempt with a mismatched nonce fails to
  `/?auth_error`). Record invocations + observed output in this plan or the
  closing strand comments.

## Braid

- **Epic `bd-uv8xynxk`** (open) — this plan. Child strands (H1, H2, H3, H5) filed at
  kickoff, one per item; H2 depends on nothing but is Phase 2 by size, not by
  blocking.
- **Related:** `bd-qxgoti2b` (Epic 2 — this epic was discovered from its
  2026-07-27 security-audit re-scope; H2's sealed-cookie helper is reusable by
  its B1), `bd-ey6jg70f` (closed — sliding sessions; H3/H5 touch its mint and
  audit surfaces).

## References

- `claude-notes/plans/2026-07-06-hub-client-auth-unification-pkce.md` — the
  pattern-(ii) re-scope + §Decision record; §References carries the audit
  sources (Google's secret-required token endpoint; Browser-Based Apps BCP).
- `claude-notes/plans/2026-07-06-hub-server-minted-sliding-sessions.md` —
  session mint/verify + revocation this plan hardens around.
- Key files: `crates/quarto-hub/src/server.rs` (`build_csp` 108,
  `AUTH_COOKIE_NAME` 130, `mint_session_cookie` 316, `auth_callback` 728,
  `auth_session` 999, routes ~1316), `crates/quarto-hub/src/auth.rs`
  (`OidcClaims` ~190, `CallbackCsrfMode` ~656), `crates/quarto-hub/src/session.rs`
  (mint/verify — home for the sealed-state helper),
  `config/local-nginx.conf` (:8080 `server {}` :28, SPA `location /` :86),
  `scripts/local-prod.sh` (+ its
  Node proxy), `hub-client/src/components/auth/LoginScreen.tsx`,
  `hub-client/src/auth/GoogleAuthProvider.tsx`. *(Line numbers approximate.)*

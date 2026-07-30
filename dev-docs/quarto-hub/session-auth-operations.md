# Hub session auth — operations guide

Operational reference for the hub's server-minted sliding sessions
(epic `bd-ey6jg70f`; design in
`claude-notes/plans/2026-07-06-hub-server-minted-sliding-sessions.md`).

## The session model in one paragraph

The hub validates a user's Google ID token **once** at login, then
mints its own compact HS256 session token into an HttpOnly session
cookie (~400 bytes) — named `__Host-quarto_hub_token` under TLS, or
`quarto_hub_token` in insecure dev mode. The session **slides**: authenticated
HTTP activity re-issues the cookie (at most ~1/hour), up to an **idle
timeout** (default 7 days) and an **absolute lifetime cap** (default
30 days, anchored at login — re-issue can never extend past it). The
MCP Bearer path (`Authorization: Bearer <google_id_token>`) is
unchanged. Legacy Google-JWT cookies are rejected (one-time re-login
at the cutover deploy).

## Configuration

| Setting | Source | Default |
|---|---|---|
| Session signing secret | `QUARTO_HUB_SESSION_SECRET` (64-char hex) → `hub.json` `session_secret` → auto-generated | auto |
| Previous secret (graceful rotation) | `QUARTO_HUB_SESSION_SECRET_PREVIOUS` **with** `QUARTO_HUB_SESSION_SECRET_ROTATED_AT` (epoch s) → `hub.json` `previous_session_secret` **with** `session_secret_rotated_at` | none |
| Idle timeout | `QUARTO_HUB_SESSION_IDLE_SECS` | 604800 (7 d) |
| Absolute cap | `QUARTO_HUB_SESSION_ABSOLUTE_SECS` | 2592000 (30 d) |

Public deployments should prefer tighter caps — sliding sessions mean
more standing credentials.

Notes:

- `hub.json` (mode `0o600`) holds the signing secrets; the hub writes a
  catch-all `.gitignore` into its data dir so project-mode deployments
  never commit it. Never share the session secret with
  `server_secret` (actor-id derivation) — different blast radius.
- **Multi-instance:** hubs sharing `QUARTO_HUB_SESSION_SECRET` via env
  accept each other's session cookies. Per-hub auto-generated secrets
  give per-instance isolation.
- **Cookie name is TLS-mode dependent** (H3, `bd-gt2hhrcg`): secure
  deployments use `__Host-quarto_hub_token`; only
  `--allow-insecure-auth` uses the bare `quarto_hub_token` (the
  `__Host-` prefix requires `Secure`, which requires TLS). The prefix
  is browser-enforced — a sibling subdomain cannot plant one — which
  closes the cookie-planting residual previously called out here. In
  secure mode the bare name is **not** accepted as a credential, so the
  H3 rollout invalidates outstanding sessions once (users re-log-in);
  a login also emits a clear for the bare name so it doesn't linger.

## Login nonce (Google flow)

The Google login is a two-request flow. `GET /auth/nonce` mints a random
nonce, returns it to the SPA (which hands it to Google Identity
Services), and seals a copy into a short-lived HMAC-signed cookie —
`__Secure-quarto_hub_login`, `SameSite=None; HttpOnly; Path=/auth`, 10
minute lifetime. `POST /auth/callback` then requires the ID token's
`nonce` claim to match that cookie.

This is what stops a **captured ID token from being replayed** to mint a
session: signature, `iss`, `aud`, and `exp` all still validate for a
stolen token, so before the nonce the hub had no way to tell whether a
token belonged to a login *it* started. The blob is single-use — the
callback clears the cookie on every exit path.

Operational notes:

- **`SameSite=None` is required, not lax.** Google delivers the
  credential by cross-site form POST; a `Lax` cookie is not attached to
  it. That in turn requires `Secure`, hence TLS.
- **Enforcement is unconditional in secure mode**, and **skipped under
  `--allow-insecure-auth`** (that cookie cannot work over plain HTTP).
  The skip logs at WARN on every login — if you see
  `nonce verification skipped` in a deployment that is meant to be
  production, the hub is running with the dev flag.
- **Rejections** appear as `auth_fail` with `detail=login_state_<class>`,
  where `<class>` is one of `stale_client`, `missing`, `nonce_mismatch`,
  `token_nonce_missing` (cookie present, client too old to send a
  nonce), `expired`, `tampered`, `kid_mismatch`. The last four all mean
  a cookie arrived and failed on its merits. The two **cookie-absent**
  classes are the ones to read carefully, because they want opposite
  remedies:
  - `login_state_stale_client` — no cookie **and** no `nonce` claim.
    A current SPA cannot produce this (`GoogleAuthProvider` renders no
    button until it holds a nonce), so it is an out-of-date bundle **or**
    a login attempt made outside the app: the hub's Google client ID
    ships in the SPA, so anyone driving GIS directly can mint a
    signature-valid nonce-less token for our `aud`. Enforcement is
    unaffected either way — the heuristic describes honest clients, it
    does not certify benignity.
  - `login_state_missing` — no cookie, but the token *does* carry a
    nonce, so a pre-flight really ran. Either the cookie did not survive
    delivery (`SameSite=None`, `Path=/auth`, or the reverse proxy — the
    fix is configuration), **or** it is the replay shape: a captured
    token presented from a browser that never did a pre-flight. A single
    event cannot tell those apart; correlation can — volume, distinct
    `sub`s, and source IPs look very different for a misconfiguration
    than for a replay campaign.

  **Log-tooling note.** Until 2026-07-30 the cookie-absent class was
  emitted double-prefixed, as `login_state_login_state_missing`, and it
  covered both readings above. Anything exact-matching that string needs
  updating; substring greps for `login_state_missing` matched both forms
  and keep working.
- **Rollout:** a user holding a stale SPA bundle fails login once to
  `/?auth_error=stale_client` — which the client renders as "this version
  of the app is out of date, please reload" — and recovers on reload. Hub
  and client deploy together.
- **Scope:** the Google callback only. `/auth/session` (the Generic
  provider's JSON mint) is not nonce-bound and remains replay-able
  within the submitted token's validity.
- Sealed blobs verify against the **previous** secret during a graceful
  rotation overlap, so a login started just before a rotation completes.

## Auto-generated secrets and the multi-instance hazard

With neither `QUARTO_HUB_SESSION_SECRET` nor a `session_secret` in
`hub.json`, the hub generates one on startup, persists it, and **warns**:

```
WARN generated a new session secret and persisted it to hub.json — it is
     now pinned to this data directory. Multi-instance deployments must set
     QUARTO_HUB_SESSION_SECRET to the same value on every instance;
     otherwise instances reject each other's session cookies. hub_dir=…
```

Expect this exactly once per data directory, on first start. Seeing it on
**every** restart means the hub cannot persist `hub.json` (check
permissions on the data dir) — and a hub that regenerates its secret on
each restart logs every user out on each restart.

If two instances each generate their own, each rejects the other's
session cookies and sealed login blobs. The symptom is maddening:
sign-in fails intermittently and appears to heal itself on retry,
depending on which instance the load balancer picked. The signature in
the log is a run of `session_kid_mismatch` (and
`login_state_kid_mismatch` on the login path) with no rotation to explain
it. The fix is to set `QUARTO_HUB_SESSION_SECRET` to the same value on
every instance. `QUARTO_HUB_SERVER_SECRET` warns the same way; divergence
there means each instance derives different actor IDs for the same user.
The secret **values** are never logged.

## Rotating the session secret

Two modes. **The distinction is security-critical.**

### Graceful (scheduled hygiene)

1. Generate a new 64-char-hex secret.
2. Move the current secret to the *previous* slot and record the time:
   - env: set `QUARTO_HUB_SESSION_SECRET=<new>`,
     `QUARTO_HUB_SESSION_SECRET_PREVIOUS=<old>`,
     `QUARTO_HUB_SESSION_SECRET_ROTATED_AT=$(date +%s)`; or
   - `hub.json` (hub stopped): set `session_secret` to the new value,
     `previous_session_secret` to the old one,
     `session_secret_rotated_at` to the current epoch seconds.
3. Restart. Both secrets verify during an overlap window of **one idle
   timeout**; active sessions re-mint under the new key on their next
   request; after the window the previous entry is ignored
   automatically (remove it at your leisure). No user disruption.

A previous secret **without** its rotated-at timestamp is a startup
error by design — an unbounded overlap window would silently defeat
the rotation.

### Emergency (secret compromise)

Set **only** the new `session_secret` / `QUARTO_HUB_SESSION_SECRET`,
**no previous**, and restart. Every outstanding cookie dies
immediately (mass logout is the point: a compromised secret can forge
tokens, so any overlap window keeps accepting attacker-minted
cookies). Rejections appear in the audit log as
`detail=session_kid_mismatch`. **Never respond to a compromise with a
graceful rotation.**

## Revoking users (`revocations.json`)

Sessions are stateless; `revocations.json` (next to `hub.json`)
records only revocation events:

- **Self-service:** `POST /auth/logout-everywhere` (browser session +
  CSRF header) kills the calling user's entire token family across
  devices. Immediate re-login works.
- **Operator ban:** with the **hub stopped** (or restarting right
  after), add the user's Google `sub` to the `banned` array:

  ```json
  { "version": 1, "not_before": {}, "banned": ["1234567890"] }
  ```

  A ban rejects every session **and refuses new logins** for that
  `sub`; it never expires until removed. Never hand-edit while the hub
  runs — the hub's own atomic persist can overwrite a live edit. The
  restart also severs the banned user's live WebSocket (expiry and
  revocation otherwise bite on reconnect, not on open sockets).
- Allowlist removal (`--allowed-emails`/`--allowed-domains`) also bites
  on the user's next request — but remove-then-re-add is **not** a
  revocation: unexpired tokens resume working. Use
  `logout-everywhere`/a ban to kill tokens.
- Logout-everywhere entries self-expire after the absolute cap; the
  file stays small. A malformed `revocations.json` is a startup error
  (never silently ignored — that would un-ban users).

## Verifying a deployment shape locally

`scripts/hub-sliding-sessions-e2e.mjs` (Node built-ins only) spins up a
mock OIDC IdP, runs the real `hub` binary against it, and exercises the
whole session lifecycle — login mint, sliding expiry, the hard break
for legacy cookies, cross-path rejection, WS upgrade, session
outliving the Google token (real 120 s wait), and logout-everywhere
across two sessions:

```
cargo build --bin hub
node scripts/hub-sliding-sessions-e2e.mjs
```

`scripts/hub-auth-error-reasons-e2e.mjs` covers the callback's *failure*
side — the `/?auth_error=<reason>` codes and the `callback_csrf` audit
event — against the real binary:

```
cargo build --bin hub
node scripts/hub-auth-error-reasons-e2e.mjs
```

It needs **outbound network**, and that is not incidental: the
`POST /auth/callback` route is registered only for a Google provider, and
the provider is derived from the issuer being exactly
`https://accounts.google.com`. A mock IdP on localhost yields a Generic
provider and the route 404s, so the sliding-sessions script above
structurally cannot reach any callback failure path. Using Google's real
public discovery + JWKS is what makes the route exist. The trade-off:
only the pre-credential rejections (CSRF, undecodable token) are
reachable, because everything past them needs a credential Google
actually signed for our audience. `stale_client`, `denied` and `server`
are covered by `crates/quarto-hub/tests/integration/login_nonce.rs`
instead.

## Audit log quick reference

Auth events on target `quarto_hub::audit` carry
`credential_kind=cookie|bearer` and a `detail` discriminator:
`session_kid_mismatch` (different secret: rotated away, config drift, a
legacy Google-JWT cookie, or **two instances that each auto-generated
their own secret** — see below), `session_expired`,
`session_absolute_cap`, `session_tampered`, `session_revoked`,
`user_banned`, `user_not_allowlisted`, `conflicting_credentials`,
`login_state_stale_client` and `login_state_missing` (the two
cookie-absent login classes — see "Login nonce" above for their opposite
remedies), `callback_csrf`. Token contents are never logged.

**Mind the level.** The login-state classes and `callback_csrf` are
**WARN**, so they appear at the hub's default verbosity. The
credential-path details — `jwt_decode:*`, `azp_or_iat_rejected`,
`email_not_verified`, `user_not_allowlisted` — are **INFO**, and are
invisible unless the hub runs with `-v` or `RUST_LOG=info`. Chasing "why
is this user refused?" without one of those set will show you nothing.

`callback_csrf` means the Google double-submit pair on
`POST /auth/callback` did not match: the form's `g_csrf_token` and the
cookie GIS set on the hub origin disagreed, or one was missing. A
*deployment-wide* run of these usually means the reverse proxy is
dropping that cookie. It carries no `sub` — the check runs before the
token is parsed, and an unvalidated subject in the audit log would be
worse than an absent one. It is also emittable by unauthenticated
callers (any garbage POST to `/auth/callback`), so WARN volume here is
attacker-influenceable; that is already true of the login-nonce classes
and is not a new exposure.

Actions, by what they mean:

| `action` | Meaning |
|---|---|
| `auth_ok` / `auth_fail` | Per-request authentication decision. |
| `login_mint` | A **new session family** was created. Carries `sub`, the new `sid`, and `endpoint=callback\|session` (which login path). |
| `revoke_all_sessions` | Logout-everywhere: every prior session for `sub` is dead. |

`login_mint` is the one to join sessions against: `sid` is immutable
across sliding re-issues, so it identifies a login for as long as that
session lives. **Sliding re-issue is deliberately silent** — it extends
a session rather than opening one, so a keep-alive never looks like a
fresh login.

## What the user saw: `/?auth_error=<reason>`

A failed `POST /auth/callback` redirects to `/?auth_error=<reason>`, and
the SPA turns that into one of four sentences. Use this table to go from
a user's report back to the `detail` worth grepping for. The mapping is
**many-to-one on purpose** — the reason is in a URL the user can read and
anyone can craft, so the precise cause stays in the audit log.

| `reason` | What the user is told | Audit `detail` to look for |
|---|---|---|
| `stale_client` | This version of the app is out of date. Please reload the page and try again. | `login_state_stale_client` |
| `restart` | Sign-in didn't complete. Please try again. | `login_state_missing`, `login_state_nonce_mismatch`, `login_state_expired`, `login_state_tampered`, `login_state_kid_mismatch`, `login_state_token_nonce_missing`, `callback_csrf`, `jwt_decode:*`, `azp_or_iat_rejected`, `email_not_verified` |
| `denied` | Sign-in failed. Your account is not authorized to access this hub. | `user_banned`, `user_not_allowlisted` |
| `server` | Something went wrong on the hub. Please try again shortly. | (no audit event — a mint failure logs at ERROR, not `auth_fail`) |

Two properties worth knowing when reading reports:

- **`denied` is exactly the two real denials** — a ban and an allowlist
  miss, the cases where an identity *was* established and then refused.
  Everything else is a retry, so a user reporting "not authorized" really
  does need an administrator, not a reload.
- **Unknown and empty reasons render as `restart`.** A pre-2026-07-30 hub
  emits a bare `/?auth_error`, and the parameter is craftable, so the
  client falls back to the retry copy rather than the alarming one. If a
  user reports the retry sentence and you find no matching `auth_fail`,
  suspect a stale or cached redirect.
- `email_not_verified` is a known-coarse mapping: it lands in `restart`
  even though retrying will not verify an email. `denied` is no better —
  no administrator can fix it either. Revisit if it shows up in practice.

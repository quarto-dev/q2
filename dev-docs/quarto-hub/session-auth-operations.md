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

## Audit log quick reference

Auth events on target `quarto_hub::audit` carry
`credential_kind=cookie|bearer` and a `detail` discriminator:
`session_kid_mismatch` (different secret: rotated away, config drift,
or a legacy Google-JWT cookie), `session_expired`,
`session_absolute_cap`, `session_tampered`, `session_revoked`,
`user_banned`, `user_not_allowlisted`, `conflicting_credentials`.
Token contents are never logged.

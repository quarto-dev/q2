# Hub-mcp operator runbook

One-time setup for hub operators who want to expose `quarto-hub-mcp` to
end users. Sits alongside the SPA OAuth registration in
[`claude-notes/plans/2026-02-24-oauth2-middleware-design.md`](../plans/2026-02-24-oauth2-middleware-design.md);
both clients live in the same Google Cloud project.

Design context:
[`claude-notes/plans/2026-05-28-hub-mcp-loopback-pkce.md`](../plans/2026-05-28-hub-mcp-loopback-pkce.md).

## What you're registering

`quarto-hub-mcp` authenticates to the hub via Google's OAuth 2.0
Authorization Code grant with PKCE and a loopback redirect (RFC 8252).
That requires a second Google OAuth client of type **"Desktop app"**
in the same Google Cloud project as the SPA's existing "Web
application" client. The hub accepts ID tokens from either audience.

## Step 1 — register the OAuth client

1. Open <https://console.cloud.google.com/apis/credentials> on the
   project that already hosts the SPA OAuth client.
2. **Create Credentials → OAuth client ID**.
3. **Application type → "Desktop app"**.
4. Name it something the audit log will read clearly, e.g.
   `quarto-hub-mcp`.
5. Click **Create**. Copy the **client_id** and **client_secret** off
   the confirmation dialog.

Notes:

- The OAuth consent screen, brand assets, and verified scopes are
  shared with the SPA client. No extra consent-screen submission.
- The Desktop-app client also issues a **client_secret**, and Google
  requires it on the token exchange and the refresh-token grant; PKCE
  is layered on top of the confidential-client flow, not a replacement
  for the secret. Google documents the installed-app secret as "not
  treated as a secret," but it must still be distributed and set in the
  env. No bundled default ships in the npm package for v1 — operators
  (including the canonical-hub operator) publish both values to end
  users.

## Step 2 — configure the hub

Add the new client_id to the hub's audience allowlist. The SPA's
client_id stays as the primary `--oidc-client-id`; the hub-mcp
client_id goes into `--additional-audiences`.

```bash
hub \
  --oidc-client-id        "<spa-client-id>.apps.googleusercontent.com" \
  --additional-audiences  "<mcp-client-id>.apps.googleusercontent.com" \
  ...
```

Equivalent env vars (e.g. for `docker compose` or systemd):

```
OIDC_CLIENT_ID=<spa-client-id>.apps.googleusercontent.com
QUARTO_HUB_ADDITIONAL_AUDIENCES=<mcp-client-id>.apps.googleusercontent.com
```

`--additional-audiences` accepts a comma-separated list — multiple
hub-mcp clients (for staging vs production, etc.) are supported.
Exact matches only; no wildcards. The hub validates `azp` against the
same allowlist whenever the claim is present (OIDC §3.1.3.7).

Restart the hub. The startup log will show one `Discovered JWKS URL
from OIDC issuer` line and one signing-algorithm-lock line; the
audience allowlist itself is not logged.

## Step 3 — publish the values to end users

Each end user needs **both** values to run `quarto-hub-mcp`:

- `QUARTO_HUB_MCP_CLIENT_ID` — the hub-mcp client_id from step 1.
- `QUARTO_HUB_MCP_CLIENT_SECRET` — the matching secret.

`hub-mcp` reads them only from `process.env`. It does not look in
`.mcp.json`, the OS keyring, source literals, or any well-known file
path. Partial config (one set, one unset) is a startup error naming
both vars literally.

The recommended path is to publish them in your deployment's
end-user docs (the same place you publish the hub's WebSocket URL),
e.g. an internal handbook page or onboarding README. Each developer
then pastes them into their per-user MCP-client config — most
commonly `~/.config/claude/mcp.json` on Linux/macOS or
`%APPDATA%\Claude\mcp.json` on Windows. The package's README
(`ts-packages/quarto-hub-mcp/README.md`) carries a copy-pasteable
example.

The end-user `.mcp.json` is **not** intended to be checked into a
shared repo — the secret would leak. If you need to ship MCP config
to many users, distribute the secret through your normal
secret-management channel (1Password Connect, Kubernetes Secret +
init script, AWS Secrets Manager, etc.) and have each user's
`.mcp.json` reference an env var rather than the literal.

## Step 4 — verify

Once a user has set both env vars and configured the MCP client,
their first agent action triggers the documented flow:

1. The MCP server probes the hub. Hub returns 401 (no creds).
2. hub-mcp surfaces `AuthRequired`, prompting the agent to call the
   `authenticate` MCP tool.
3. The tool binds a `127.0.0.1` listener and opens the user's browser
   to Google's sign-in page (also printing the URL for headless/SSH
   users).
4. User signs in and approves consent; the redirect lands on the local
   listener.
5. The tool exchanges the authorization code (PKCE verifier +
   client_secret) and persists the bundle to the user's OS keyring
   under service `dev.quarto.hub-mcp`, account
   `https://accounts.google.com:<mcp-client-id>`. On success the agent
   sees `"Authenticated as <email>."` and retries the original action.

Users on headless or remote machines forward the loopback port over
SSH — see the package README's "Headless / SSH sessions" section
(`quarto-hub-mcp --redirect-port N` + `ssh -L N:127.0.0.1:N`).

Subsequent connects refresh the ID token automatically and reuse the
keyring entry until the user revokes the grant.

## Auditing and observability

The hub emits one `tracing::event!` per auth decision on the
`quarto_hub::audit` target. Set `RUST_LOG=quarto_hub::audit=info` to
include them in stdout. Each event carries:

- `action` — `auth_ok` / `auth_fail`
- `outcome` — `allow` / `deny`
- `credential_kind` — `cookie` / `bearer` (so MCP traffic is
  distinguishable from SPA traffic)
- `sub` — the Google subject identifier on accepted requests
- `detail` — failure reason on `auth_fail` (e.g.
  `user_not_allowlisted`, `conflicting_credentials`)

Tokens themselves are never logged; `tower-http`'s `MakeSpan` is
overridden to drop the `Authorization` / `Cookie` headers from the
request span (`crates/quarto-hub/src/server.rs::RedactedMakeSpan`).

## Secret rotation

If the `client_secret` leaks (committed to a public repo, exposed in
CI logs, etc.):

1. **Reset the secret in Google Cloud Console** — same Credentials
   page, click the client, **Reset Secret**. The old value stops
   working immediately.
2. **Update the secret in your secret-management system.** Every
   end user picks up the new value the next time their MCP client
   restarts (env vars are read at process start).
3. Existing user keyring entries are **not** invalidated — they hold
   ID + refresh tokens that were issued by the old secret but remain
   redeemable against the new one (the refresh grant authenticates with
   whatever secret is currently configured). No user-side action
   required after rotation unless an existing refresh token itself is
   also compromised, in which case ask affected users to revoke the
   grant (see below).

If a **user's** refresh token leaks (rather than the operator's
secret), the quickest fix is `authenticate_clear`, which best-effort
revokes the refresh token at Google before clearing the local copy.
The manual equivalent is <https://myaccount.google.com/permissions> →
"Third-party apps with account access" → the hub-mcp client → "Remove
Access". Their next agent action surfaces `ReauthRequired` and runs
through the loopback sign-in again with a new bundle.

## Residual risk to communicate to operators

- **Stolen ID tokens are valid for up to ≤1 h** regardless of grant
  revocation at Google. JWTs are self-contained; the hub does not
  consult Google on each request. Closing this window requires a
  hub-side `sub_denylist` — not in v1; tracked in the plan's
  "Future work" section.
- **A stolen refresh token is an indefinite foothold** until the user
  revokes the grant (via `authenticate_clear` or
  myaccount.google.com). hub-mcp persists a `refresh_token` only when
  Google returns one and keeps the prior value otherwise, so rotation
  behaviour does not change the residual; whichever value is current is
  the one to revoke. (The Desktop-app client's exact rotation behaviour
  is pending confirmation by the loopback+PKCE plan's Spike A.)
- **The loopback redirect closes the remote no-malware phishing class**
  that device flow enabled: tokens are delivered only to the user's own
  `127.0.0.1` listener, and PKCE binds the code to a verifier held in
  the hub-mcp process. An attacker needs local code execution to
  capture tokens — at which point the keyring is already exposed.
- **Headless Linux without Secret Service / libsecret cannot persist
  credentials.** The credential store refuses silent fallback to a
  plaintext file. Users on those hosts use the SPA cookie path.

See the implementation plan's *Threat model* and *Residual risks
accepted for v1* sections for the full enumeration.

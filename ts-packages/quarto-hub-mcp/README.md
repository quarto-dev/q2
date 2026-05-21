# @quarto/hub-mcp

MCP server that lets an AI agent (Claude Code, Claude Desktop, Cursor,
Continue, …) read and write Quarto Hub projects through Automerge.

Authentication uses Google's OAuth 2.0 device-authorization grant
(RFC 8628). The agent calls one MCP tool to start the flow, you
approve in a browser, and the agent calls a second tool to finish.
The resulting credentials are persisted in your OS keyring; the agent
can use them indefinitely until you revoke them.

The design lives in
[`claude-notes/plans/2026-05-05-hub-mcp-device-flow-implementation.md`](../../claude-notes/plans/2026-05-05-hub-mcp-device-flow-implementation.md);
operators registering a hub for end-user use should consult
[`claude-notes/instructions/hub-mcp-operator-runbook.md`](../../claude-notes/instructions/hub-mcp-operator-runbook.md).

## Setup

You need two values from your hub operator:

| Env var                          | Source                                    |
|----------------------------------|-------------------------------------------|
| `QUARTO_HUB_MCP_CLIENT_ID`       | Google OAuth client_id (hub-mcp)          |
| `QUARTO_HUB_MCP_CLIENT_SECRET`   | Matching client_secret                    |

Both are mandatory. Partial config (one set, one unset) fails at
startup with an error message naming both variables literally.

Add hub-mcp to your MCP client config. For Claude Code the file is
`~/.config/claude/mcp.json` (macOS/Linux) or `%APPDATA%\Claude\mcp.json`
(Windows):

```json
{
  "mcpServers": {
    "quarto-hub": {
      "command": "npx",
      "args": [
        "@quarto/hub-mcp",
        "--server",
        "wss://hub.example.com/ws"
      ],
      "env": {
        "QUARTO_HUB_MCP_CLIENT_ID":     "<operator-supplied>.apps.googleusercontent.com",
        "QUARTO_HUB_MCP_CLIENT_SECRET": "<operator-supplied>"
      }
    }
  }
}
```

> ⚠️ This file holds a long-lived secret. Do not check it into a
> shared repo. If you need to share MCP config across a team, point
> the env entries at a secret-manager-fed env file instead of
> hard-coding the value.

Restart the MCP client. The first agent action against the hub
triggers the auth flow:

1. Agent calls the `authenticate_start` MCP tool.
2. Tool response shows `https://www.google.com/device` and a short
   `user_code`. Google's response carries its own `verification_uri`
   that is also shown — both URLs are valid; the canonical
   `https://www.google.com/device` is hard-coded as a
   phishing-resistance check.
3. You open the URL, type the code, approve consent in your browser.
4. Agent calls `authenticate_finish`. On success the response is
   `"Authenticated as <your-email>."` and the agent retries the
   original action.

If the hub does **not** require authentication (operator-disabled
auth) the agent just talks to the hub directly — no device flow.
Asking the agent to authenticate against a known no-auth hub returns
`"The configured hub does not require authentication; no action
needed."`.

## Credential storage

The bundle (`id_token`, `refresh_token`, `id_token_expires_at`,
`scopes`) is stored as a single opaque JSON value in your OS keyring:

| Platform | Backend                       | Service / target                                         |
|----------|-------------------------------|----------------------------------------------------------|
| macOS    | login Keychain                | service `dev.quarto.hub-mcp`, account `<issuer>:<client_id>` |
| Linux    | Secret Service (libsecret)    | schema `dev.quarto.hub-mcp`, attribute `<issuer>:<client_id>` |
| Windows  | Credential Manager (DPAPI)    | target `dev.quarto.hub-mcp:<issuer>:<client_id>`         |

`<issuer>` is `https://accounts.google.com`; `<client_id>` is the
value you set in `QUARTO_HUB_MCP_CLIENT_ID`. The entry is bound to
your OS user account; another user on the same machine cannot read
it through normal APIs.

There is **no plaintext file on disk** on any supported platform.
hub-mcp refuses to fall back silently — if the keyring is
unreachable, `write` fails with a typed error and you'll need to fix
the keyring before re-running the flow. (`read` failures fold to
"no credentials" so try-without-creds-first still works on
no-auth hubs.)

### Inspecting the entry

| Platform | Command                                                                                  |
|----------|------------------------------------------------------------------------------------------|
| macOS    | `security find-generic-password -s dev.quarto.hub-mcp -a "<issuer>:<client_id>" -w`      |
| Linux    | `secret-tool lookup service dev.quarto.hub-mcp account "<issuer>:<client_id>"`           |
| Windows  | `cmdkey /list:dev.quarto.hub-mcp:<issuer>:<client_id>`                                   |

### Clearing the entry

| Platform | Command                                                                       |
|----------|-------------------------------------------------------------------------------|
| macOS    | `security delete-generic-password -s dev.quarto.hub-mcp -a "<issuer>:<client_id>"` |
| Linux    | `secret-tool clear service dev.quarto.hub-mcp account "<issuer>:<client_id>"` |
| Windows  | `cmdkey /delete:dev.quarto.hub-mcp:<issuer>:<client_id>`                      |

Clearing the entry forces the next agent action to start a fresh
device flow.

### Headless Linux

Headless Linux machines without a running Secret Service / libsecret
(`gnome-keyring-daemon`, `kwallet5`, or equivalent) cannot run
hub-mcp. Either install one of those daemons or use the Hub SPA
cookie path from a graphical session on the same hub.

## Revoking access

To revoke the agent's access entirely:

1. Visit <https://myaccount.google.com/permissions>.
2. **Third-party apps with account access** → the hub-mcp client →
   **Remove Access**.

The agent's next action surfaces `ReauthRequired` with a message
asking you to re-authenticate. Clear your local keyring entry too
(see the table above) if you don't intend to authenticate again
soon.

> **ID-token residual validity.** A stolen ID token authenticates to
> the hub for up to **≤1 hour** after revocation, because JWTs are
> self-contained and the hub does not consult Google on each
> request. Closing this window requires a hub-side denylist — not in
> v1. If you have evidence of an active compromise (e.g. a leaked
> machine), ask your hub operator to roll the audience allowlist or
> rotate the OAuth client.

## Why both env vars must come from the operator

Symmetric with the SPA: the operator owns the Google project, the
consent screen, the quota, the audit trail, and the revocation key.
The Quarto team does **not** ship a baked-in client_id or
client_secret — there is no shared default to compromise.

Defence in depth: a leaked `client_secret` alone cannot redeem any
user's approval. Each device flow is bound to a `device_code` that
Google issues per-flow and that hub-mcp never persists; without a
fresh approval against that `device_code` the secret is inert.

## Insecure transport (dev only)

By default hub-mcp refuses to attach `Authorization: Bearer …` to a
non-loopback `ws://` / `http://` URL. Loopback (`localhost`,
`127.0.0.1`, `::1`, `*.localhost`) is always permitted. To override
for local development against a non-loopback hub without TLS:

```
QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1
```

With the override set, every connect emits a loud `console.warn`.
Never set this in production.

## What gets logged

- No log line — at any level — contains an ID token, refresh token,
  client secret, or any string matching the shape `ya29.*`, `1//*`,
  or a JWT. A centralised `redactTokens` utility scrubs every log
  call site; `uncaughtException` / `unhandledRejection` handlers
  scrub stack traces before printing.
- Hub-side audit events on the `quarto_hub::audit` tracing target
  carry the Google `sub`, the credential kind (`bearer` for hub-mcp,
  `cookie` for the SPA), and an outcome — never the token bytes.

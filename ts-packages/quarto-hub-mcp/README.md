# @quarto/hub-mcp

MCP server that lets an AI agent (Claude Code, Claude Desktop, Cursor,
Continue, …) read and write Quarto Hub projects through Automerge.

Authentication uses Google's OAuth 2.0 Authorization Code grant with
PKCE and a loopback redirect (RFC 8252) — the same pattern as `gcloud
auth login`, `gh auth login`, and `aws sso login`. The agent calls a
single `authenticate` MCP tool, your browser opens to Google's sign-in
page, and the redirect lands back on a short-lived `127.0.0.1` listener
the tool started. The resulting credentials are persisted in your OS
keyring; the agent can use them indefinitely until you revoke them.

The design lives in
[`claude-notes/plans/2026-05-28-hub-mcp-loopback-pkce.md`](../../claude-notes/plans/2026-05-28-hub-mcp-loopback-pkce.md);
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
        "wss://quarto-hub.com/ws"
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

1. Agent calls the `authenticate` MCP tool. The tool binds a local
   `127.0.0.1` listener and opens your browser to Google's sign-in
   page. The authorization URL is also printed (to the tool's progress
   output and to stderr) so you can open it manually if the browser
   doesn't launch — useful on headless or SSH sessions.
2. You sign in and approve consent in your browser. The redirect lands
   on the local listener, which closes immediately after.
3. The tool exchanges the authorization code (bound to a PKCE verifier
   held only in this process) and stores the credentials. On success
   the response is `"Authenticated as <your-email>."` and the agent
   retries the original action.

If the hub does **not** require authentication (operator-disabled
auth) the agent just talks to the hub directly — no sign-in flow.
Asking the agent to authenticate against a known no-auth hub returns
`"The configured hub does not require authentication; no action
needed."`.

## Upgrading from the device-flow version

Earlier hub-mcp used Google's device-authorization flow with a "TV and
Limited Input devices" OAuth client. The loopback+PKCE version uses a
**Desktop app** client, so your operator will issue new `client_id` /
`client_secret` values. After you switch them, the first agent action
prompts you to sign in once: the credential store keys entries by
`(issuer, client_id)`, so the old entry is simply invisible to the new
lookup and a fresh `authenticate` runs automatically. The stranded old
entry under the previous `client_id` is harmless and can be cleared at
your leisure with the platform command in the **Clearing the entry**
table (substituting the old `client_id`).

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
sign-in. The `authenticate_clear` MCP tool does the same from inside
the agent, and additionally best-effort revokes the stored refresh
token at Google before deleting the local copy (see **Revoking
access** below).

### Headless / SSH sessions

The sign-in redirect lands on a `127.0.0.1` listener on the machine
running hub-mcp. On a headless or remote machine there is no local
browser to receive it, so use one of:

- **SSH port-forward.** Pick a free non-privileged port `N`, start
  hub-mcp with a fixed redirect port, and forward it from your
  workstation:

  ```bash
  # on the remote host (via your MCP client's args)
  quarto-hub-mcp --server wss://quarto-hub.com/ws --redirect-port N
  # on your local workstation
  ssh -L N:127.0.0.1:N <remote>
  ```

  Call `authenticate`; the printed authorization URL opens in your
  local browser, and the redirect to `http://127.0.0.1:N/callback`
  travels back through the tunnel. Omitting `--redirect-port` lets the
  OS pick a port (logged to stderr on bind), which is fine for a local
  desktop but awkward to forward.

- **Hub SPA cookie path** from a graphical session on the same hub.

### Headless Linux keyring

Headless Linux machines without a running Secret Service / libsecret
(`gnome-keyring-daemon`, `kwallet5`, or equivalent) cannot persist
credentials. Either install one of those daemons or use the Hub SPA
cookie path from a graphical session on the same hub.

## Revoking access

The quickest path is to ask the agent to call `authenticate_clear`.
It best-effort revokes the stored refresh token at Google's revocation
endpoint (which also invalidates any access tokens derived from it),
then deletes the local keyring entry. If the revoke fails (offline, or
the token was already invalid) the local delete still proceeds and the
response tells you to finish the job manually.

To revoke manually, or to be sure when the revoke step failed:

1. Visit <https://myaccount.google.com/permissions>.
2. **Third-party apps with account access** → the hub-mcp client →
   **Remove Access**.

The agent's next action surfaces `ReauthRequired` with a message
asking you to re-authenticate.

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
client_secret — there is no shared default to compromise. The
operator registers a **Desktop app** OAuth client (Google requires the
`client_secret` on the token exchange even for installed-app clients,
so both values are distributed to end users).

Defence in depth: the loopback redirect collapses the remote-attack
surface to a local one. After consent, tokens are delivered only to
`http://127.0.0.1:<port>` on your own machine — unreachable from any
remote network — and the authorization code is bound by PKCE to a
`code_verifier` that never leaves this process. An attacker holding the
`client_id`/`client_secret` cannot mint tokens under your identity
without first achieving code execution on your machine (at which point
the keyring is already exposed and OAuth is moot). This is the key
improvement over the previous device-flow design, which an attacker
could phish remotely with only the client credentials and a plausible
cover story.

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

# Auth verification — hub-client (SPA cookie) + hub-mcp (device flow)

End-to-end verification for the two auth paths into Quarto Hub:
SPA Google sign-in → HttpOnly cookie → WS upgrade, and hub-mcp's
RFC 8628 device flow → OS-keyring bundle → Bearer-token WS upgrade.

Use this when re-running the user-driven half of Phase 9 verification
described in
[`claude-notes/plans/2026-05-05-hub-mcp-device-flow-implementation.md`](../plans/2026-05-05-hub-mcp-device-flow-implementation.md)
§ "Deferred to user-driven verification". The autonomous half is
already recorded in that plan; what's below needs a real browser, a
real Google consent, a real MCP client, or grant revocation.

Targets macOS. Linux/Windows keyring equivalents are in
[`ts-packages/quarto-hub-mcp/README.md`](../../ts-packages/quarto-hub-mcp/README.md).

## One-time setup

### 1. Google Cloud Console — SPA OAuth client

The SPA's "Web application" OAuth client (matching `OIDC_CLIENT_ID`)
must list the local dev URLs, or Google rejects sign-in with
`Error 400: unsupported_response_type`:

1. Open <https://console.cloud.google.com/apis/credentials>.
2. Click the SPA "Web application" OAuth client.
3. **Authorized JavaScript origins** → add `http://localhost:5173`.
4. **Authorized redirect URIs** → add `http://localhost:5173/auth/callback`.
5. Save.

The hub-mcp "TV and Limited Input devices" client has no redirect URIs
— leave it untouched.

### 2. Register hub-mcp with Claude Code CLI

User-scope so it overrides any project-level `.mcp.json` while
verifying against the local hub. Run from the repo root so `$(pwd)`
resolves to the absolute path Claude Code stores in the registration:

```bash
cd <repo-root>

claude mcp remove quarto-hub -s user 2>/dev/null || true

claude mcp add quarto-hub -s user \
  -- node "$(pwd)/ts-packages/quarto-hub-mcp/dist/index.js" \
        --server ws://localhost:3000/

claude mcp get quarto-hub   # confirm
```

## Per-session setup (every new terminal)

### Env vars

```bash
export OIDC_CLIENT_ID=$(grep ^OIDC_CLIENT_ID=                             ~/.Renviron | cut -d= -f2-)
export QUARTO_HUB_MCP_CLIENT_ID=$(grep ^QUARTO_HUB_MCP_CLIENT_ID=         ~/.Renviron | cut -d= -f2-)
export QUARTO_HUB_MCP_CLIENT_SECRET=$(grep ^QUARTO_HUB_MCP_CLIENT_SECRET= ~/.Renviron | cut -d= -f2-)
```

### Rebuild after code changes

```bash
cd <repo-root>
cargo build -p quarto-hub --bin hub
(cd ts-packages/quarto-hub-mcp && npm run build)
```

### Terminal A — authenticated hub

Stays running for items 3, 4, 5, 6, 4b. Audit log on stdout.

```bash
DATA_DIR=$(mktemp -d -t phase9-hub-auth)
trap 'rm -rf "$DATA_DIR"' EXIT
RUST_LOG="quarto_hub=info,quarto_hub::audit=info,info" \
  target/debug/hub \
    --data-dir "$DATA_DIR" \
    --port 3000 --host 127.0.0.1 \
    --oidc-client-id "$OIDC_CLIENT_ID" \
    --additional-audiences "$QUARTO_HUB_MCP_CLIENT_ID" \
    --allowed-domains posit.co \
    --allow-insecure-auth
```

## Optional: keyring helpers

Quality-of-life shell functions used by the items below. Skip if
you'd rather run the underlying `security` commands directly — the
helpers do nothing the macOS Keychain CLI can't.

```bash
inspect-kr() {
  local account="https://accounts.google.com:$QUARTO_HUB_MCP_CLIENT_ID"
  security find-generic-password -s dev.quarto.hub-mcp -a "$account" -w 2>/dev/null \
    | jq '{schema_version, issuer, client_id, scopes, id_token_expires_at,
           id_token_segments: (.id_token|split(".")|length),
           refresh_token_len: (.refresh_token|length)}' \
    || echo "(no keyring entry)"
}
clear-kr() {
  local account="https://accounts.google.com:$QUARTO_HUB_MCP_CLIENT_ID"
  security delete-generic-password -s dev.quarto.hub-mcp -a "$account" 2>/dev/null \
    && echo "cleared" || echo "(no entry to clear)"
}
expire-kr() {
  local account="https://accounts.google.com:$QUARTO_HUB_MCP_CLIENT_ID"
  local blob; blob=$(security find-generic-password -s dev.quarto.hub-mcp -a "$account" -w 2>/dev/null) \
    || { echo "(no entry — run the auth flow first)"; return 1; }
  local patched; patched=$(echo "$blob" | jq -c '.id_token_expires_at = "1970-01-01T00:00:00Z"')
  security add-generic-password -s dev.quarto.hub-mcp -a "$account" -w "$patched" -U
  echo "expired"
}
```

## Verification matrix

| Item | Path     | Goal                                                         |
|------|----------|--------------------------------------------------------------|
| 3    | Cookie   | SPA Google sign-in works; cookie path unaffected by Phase 2. |
| 4    | Bearer   | hub-mcp full device flow + keyring round-trip.               |
| 5    | Bearer   | Force a proactive refresh; observe one `/token` POST.        |
| 6    | Bearer   | Grant revocation surfaces typed `ReauthRequired`.            |
| 4b   | Both     | Allowlist parity — byte-identical 403 audit lines.           |
| 4a   | Bearer   | No-auth hub doesn't trigger device flow; short-circuit works.|

Order below minimises hub restarts.

---

## Item 3 — SPA cookie path (browser)

In a second terminal:

```bash
cd <repo-root>/hub-client
VITE_GOOGLE_CLIENT_ID="$OIDC_CLIENT_ID" npm run dev
```

Open <http://localhost:5173/>, click "Sign in with Google", complete
consent with your `@posit.co` account.

**Record** from terminal A: one event
`action="auth_ok" outcome="allow" credential_kind="cookie" sub=<your subject>`.
Keep the `sub` — item 4 should produce the same one on Bearer.

Close the tab when done.

---

## Item 4 — hub-mcp full device-flow E2E

```bash
clear-kr     # fresh slate
```

In a fresh terminal: `cd /tmp && claude`.

**4.1 — trigger AuthRequired.** Ask the agent:

> Use the `quarto-hub` MCP server. Call `connect_project` with
> project_id `phase9-test-project`. (If the project does not exist,
> that's fine — I just need to see the error path.)

Expect: typed `AuthRequired` naming `authenticate_start`.

**4.2 — start the device flow.**

> Yes, call `authenticate_start`.

Expect tool response with `https://www.google.com/device` (the
hard-coded canonical URL), a `user_code`, expiry seconds.

**4.3 — finish.** Complete the flow in your browser, then:

> Now call `authenticate_finish`.

Expect: `Authenticated as <your email>.`

**4.4 — retry.**

> Retry the `connect_project` call from earlier.

Expect: succeeds (or `project not found` — auth path is what matters).

**Record:**

- Terminal A:
  `action="auth_ok" outcome="allow" credential_kind="bearer" sub=<your subject>`.
  Subject should match item 3.
- `inspect-kr` shows `schema_version: 1`, scopes
  `["openid","email","profile"]`, expiry ≈ 1 h from now.
- `ls ~/Library/Application\ Support/quarto/ 2>/dev/null` → no such
  directory (no plaintext fallback).

---

## Item 5 — Force refresh

```bash
expire-kr    # rewrites id_token_expires_at to 1970
inspect-kr   # confirms 1970
```

In Claude Code:

> Use the `quarto-hub` MCP tool to call `list_files` against
> project_id `phase9-test-project`.

Expect:

- Terminal A: one new `auth_ok credential_kind="bearer"` event.
- hub-mcp's MCP-server stderr (`claude --debug`) shows exactly one
  request to `https://oauth2.googleapis.com/token`.

```bash
inspect-kr   # expiry is now ~1h in the future
```

---

## Item 6 — Force re-auth

1. Open <https://myaccount.google.com/permissions>.
2. Third-party apps → hub-mcp client → **Remove Access**.

In Claude Code:

> Call `list_files` against the same project again.

Expect typed `ReauthRequired`:

> Your Quarto Hub credentials have expired or were revoked. Ask me to
> authenticate again.

`RefreshManager` catches `invalid_grant` from `/token`, clears the
store, throws. Verify:

```bash
inspect-kr   # "(no keyring entry)" — store.clear() ran
```

Decline if the agent proposes `authenticate_start` again — verification
is complete.

---

## Item 4b — Allowlist parity (two accounts)

Hub already runs with `--allowed-domains posit.co`. Needs an
allowlisted (`@posit.co`) and a non-allowlisted (e.g. `@gmail.com`)
Google account.

**Cookie path.** In the SPA, sign out if needed, then sign in with the
non-allowlisted account. Expect 403 on `/auth/callback`.

Terminal A:
`action="auth_fail" outcome="deny" credential_kind="cookie" detail="user_not_allowlisted"`.

**Bearer path.**

```bash
clear-kr
```

In Claude Code:

> Run `authenticate_start`, complete with my @gmail.com account, then
> `authenticate_finish` and `connect_project`.

Expect: device flow succeeds (Google has no allowlist), then any
tool that opens the WS gets 403 surfaced as typed error.

Terminal A:
`action="auth_fail" outcome="deny" credential_kind="bearer" detail="user_not_allowlisted"`.

The `detail` field must be byte-identical between the two paths.

**Happy-path retest:** `clear-kr`, re-run with `@posit.co`. Bearer
path back to `auth_ok`.

---

## Item 4a — No-auth hub + short-circuit

```bash
clear-kr
# Stop terminal A (Ctrl-C). Relaunch hub with no auth:
DATA_DIR=$(mktemp -d -t phase9-hub-noauth)
trap 'rm -rf "$DATA_DIR"' EXIT
RUST_LOG="quarto_hub=info,info" \
  target/debug/hub --data-dir "$DATA_DIR" --port 3000 --host 127.0.0.1
```

Same `ws://localhost:3000/` registration; no re-register needed.

**Restart Claude Code** (`/exit`, `claude`) so the MCP subprocess
respawns with fresh `lastObservedAuthMode = 'unknown'`.

**4a.1 — no-auth probe.**

> Call `connect_project` against project_id `phase9-test-project`.

Expect: succeeds — no `AuthRequired`, no device flow. `inspect-kr`
stays empty.

**4a.2 — explicit-authenticate short-circuit.**

> Now run `authenticate_start`.

Expect:
"The configured hub does not require authentication; no action needed."
No request to `oauth2.googleapis.com` (visible in `claude --debug`).

**4a.3 — unknown-state baseline.** `/exit` and relaunch `claude` so
`lastObservedAuthMode` resets to `'unknown'`. **Before** any other hub
call:

> Run `authenticate_start`.

Expect: device flow **does** initiate — confirms the short-circuit is
gated on positive observation, not absence of evidence. Abort or
complete the flow.

---

## Teardown

```bash
# Stop terminal A.
clear-kr
claude mcp remove quarto-hub -s user
```

## Logging results

After each item, append observed output to the plan under a new
`#### Phase 9 user-driven verification — <date> @ <commit-hash>`
sub-heading. The verification log is append-only.

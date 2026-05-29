# hub-mcp: replace device flow with Authorization Code + PKCE + loopback

## Amendment 2026-05-28

Two changes to the plan as originally drafted, both settled by the user:

1. **Phase 0 spikes are no longer a hard gate.** Implementation may
   begin before the spikes are run. The spikes are retained below as
   *recommended validation* to perform during Phase 1 / before merge,
   not as a blocker for starting work. In particular, Spike B's
   host-RPC-lifetime risk is real — if a host turns out to enforce a
   short `tools/call` deadline, the two-tool fallback still applies —
   but we proceed with the single-tool design and the 5-minute
   deadline as the working assumption.
2. **Both `CLIENT_ID` and `CLIENT_SECRET` are kept.** Google's
   "Desktop app" client type issues a `client_secret` and requires it
   on the token exchange and refresh-token grant; PKCE is layered *on
   top* of the confidential-client exchange, it does not replace the
   secret. The original draft's premise that loopback+PKCE makes this
   a secret-free public-client flow does not hold for Google as the
   IdP. Consequently: `QUARTO_HUB_MCP_CLIENT_SECRET` is **retained**,
   `oauth.ClientSecretPost(...)` stays in `refresh-manager.ts`, and no
   `oauth.None()` public-client refresh construct is introduced.

   The justification for the device-flow → loopback switch now rests
   entirely on the **threat-model improvement** (loopback structurally
   defeats the no-malware remote-phishing class that device flow
   enables — see Phase 4), not on secret elimination. The
   secret-distribution operational friction is unchanged from today;
   that is no longer claimed as a benefit.

Sections below have been edited to reflect both changes. Where the
original "no secret" framing survives in prose, read it through this
amendment.

## Overview

Replace the device-flow auth path in `quarto-hub-mcp` with
Authorization Code + PKCE using a loopback redirect (RFC 8252).
Single new `authenticate` MCP tool replaces the existing
`authenticate_start` / `authenticate_finish` pair; the existing
`authenticate_clear` tool is retained on the MCP surface as the
escape hatch when the hub rejects cached credentials, with one
behavioural addition — best-effort revocation of the stored refresh
token at Google's revocation endpoint before the local keyring
delete. Today's `handleClear` is documented as not touching
Google-side grants; this plan closes that gap so "clear" actually
means "the credential cannot be used anywhere," which is the
contract a user clearing-on-suspected-compromise needs.

Both `QUARTO_HUB_MCP_CLIENT_ID` and `QUARTO_HUB_MCP_CLIENT_SECRET`
are retained (see Amendment 2026-05-28): Google's Desktop-app client
type requires the secret on the token exchange and refresh grant, so
PKCE is layered on top of the confidential-client flow rather than
replacing it. The change from device flow to loopback is justified by
the threat-model improvement alone (Phase 4), not by secret
elimination.

Device flow is removed, not kept as a fallback. Headless users
without a browser fall back to either SSH port-forwarding the
loopback URL (`ssh -L`) or the existing SPA-cookie path from a
graphical session.

For v1 the `client_id` is **not** bundled as a default in the npm
package — operators (including the canonical-hub operator) publish
it to end users the same way they publish the hub WebSocket URL.
Bundling is deferred as future work.

## Why

The current device-flow design (see
[`2026-05-05-hub-mcp-device-flow-implementation.md`](2026-05-05-hub-mcp-device-flow-implementation.md))
assumes an operator-to-end-user relationship: the operator registers
an OAuth client at Google and distributes `client_id` + `client_secret`
to each user through a secret-management channel. That model is
correct for internal-developer hubs (the case the original plan
solved for) and is workable — but device flow has a structural
phishing weakness that loopback does not. That weakness, not the
secret, is the reason to switch.

Two design alternatives were considered:

1. **Keep device flow** (bundled or operator-distributed
   `client_id` + `client_secret`). Works, but device flow uniquely
   enables the no-malware remote-phishing class documented in Phase 4
   (Storm-2372-style attacks). The secret-distribution requirement is
   the same under loopback, so it is not a differentiator either way.
2. **Authorization Code + PKCE + loopback** (this plan). Standard
   pattern for installed native apps per RFC 8252. Used by `gcloud
   auth login`, `aws sso login`, `gh auth login`, `firebase login`,
   `terraform login`. With Google's Desktop-app client type the
   `client_secret` is still sent on the token exchange and refresh
   (Google requires it; PKCE is layered on top), so this is not a
   secret-free public-client flow — but the loopback `redirect_uri`
   collapses the remote-attack surface to a local-attack surface,
   which is the win. See Phase 4 threat-model section.

(2) is the smallest delta that gives a strictly stronger threat model
than today.

**What this plan does not solve:** the original public-hub motivation
("end users with no operator relationship") is unchanged — end users
still need two values (`client_id` + `client_secret`) from the
canonical-hub operator. Loopback does not reduce that config burden;
it improves the threat model. Closing the config-burden gap requires
bundled defaults (deferred).

### PKCE-on-device-flow was considered and rejected

RFC 9700 §4.13 recommends PKCE for device flow; Google does not
support it on `/device/code`. Even if Google added support, PKCE
on device flow protects against `device_code` leakage *between
clients on the same device*, not against the no-malware remote
phishing threat that loopback structurally prevents (see Phase 4).
Loopback is the actually-stronger position.

## Scope decisions

These are settled. Reopen only with explicit user discussion.

1. **Stay on Google as the IdP.** Same identity provider as today,
   same hub audit story, smaller blast radius.
2. **Do NOT bundle the `client_id` / `client_secret` in the npm
   package for v1.** Operators — including the canonical-hub operator
   — publish both values to end users alongside the hub WebSocket
   URL. End users set `QUARTO_HUB_MCP_CLIENT_ID` and
   `QUARTO_HUB_MCP_CLIENT_SECRET`. Bundling is future work pending
   operational signal on end-user friction.
3. **Replace device flow entirely.** Do not keep as a fallback.
   Removes one auth path, one MCP tool pair, and one OAuth client
   type (the "TV and Limited Input devices" type is swapped for the
   "Desktop app" type). Both env vars (`QUARTO_HUB_MCP_CLIENT_ID` and
   `QUARTO_HUB_MCP_CLIENT_SECRET`) are retained — Google's Desktop-app
   client requires the secret on token exchange. Headless users
   without a browser use SSH loopback port-forwarding or the SPA
   cookie path; documented in Phase 3.
4. **Single `authenticate` MCP tool.** Replaces `authenticate_start`
   + `authenticate_finish`. Returns when the loopback listener
   fires (success), the user cancels (error), or the timeout
   expires (error). `authenticate_clear` stays on the MCP surface
   — it is independent of the device-flow / loopback choice and
   remains the documented recovery action for stale cached
   credentials — but its implementation gains best-effort Google-
   side refresh-token revocation (see Phase 1 "authenticate_clear
   revocation" item). The public contract widens from "delete the
   local copy" to "render the credential unusable, locally and at
   Google."
5. **Concurrency: rely on the MCP host's request serialisation,
   do not add a single-flight guard.** MCP stdio servers are
   single-client by design (the host launches `quarto-hub-mcp`
   as a subprocess and is the sole peer), and host agent loops
   issue `tools/call` requests serially — the next call doesn't
   leave the host until the previous `CallToolResult` arrives.
   Two `authenticate` invocations in flight at the same time is
   therefore a host-bug case, not a normal-operation case. The
   same logic applies to `authenticate_clear` arriving while
   `authenticate` is in flight: under a well-behaved host it
   cannot happen. We accept the narrow misbehaving-host failure
   mode (two listeners on different kernel-picked ports, confusing
   browser UX) rather than carry single-flight bookkeeping for an
   event that shouldn't occur. This is *not* a security
   concession — `state` and PKCE bind each flow independently, so
   tokens from one flow cannot be redirected into another. See
   the Phase 1 single-`authenticate` tool item for the documented
   assumption and the residual UX note.

## Phase 0 — recommended validation spikes (no longer blocking)

**Amended 2026-05-28: these spikes no longer gate Phase 1.**
Implementation may proceed against the working assumptions baked into
Phase 1 (5-minute deadline, `ClientSecretPost` refresh, `prompt=consent`
on by default). The spikes remain valuable validation to run during
Phase 1 or before merge; if one surfaces a problem, the fallback shape
is spelled out below and the affected Phase 1 items are revised. They
are documented here as the two structural risks worth confirming
against real systems, not as a precondition for starting work.

- [ ] **Spike A — Google "Desktop app" client type + loopback + PKCE
      against a real hub audience allowlist.** Register a throwaway
      Desktop-app client in a test-tier Google project, drive a full
      auth-code+PKCE round-trip from a Node script against the
      production discovery endpoint, exchange the code, decode the
      ID token, and verify:
  - the `aud` claim equals the new `client_id`,
  - the hub (with the test `client_id` added to
    `--additional-audiences`) accepts the resulting bearer token on
    a real WebSocket connect,
  - refresh-token rotation behaviour is unchanged from the Limited-
    Input-Devices client (the persistence rule documented in
    `refresh-manager.ts`'s top-of-file comment still holds),
  - **second-run refresh-token return.** Run the full flow twice
    with the **same** Google account and **without**
    `prompt=consent` on the second authorization request. Record
    whether the second `/token` response body contains a non-empty
    `refresh_token`. This decides whether we can drop the
    `prompt=consent` default for returning users (see Phase 1
    Authorization URL construction). If the second run returns no
    `refresh_token`, the default stays on; do not drop it on a hunch.
  - **refresh construct under the Desktop-app client.** Confirm the
    existing `oauth.ClientSecretPost(clientSecret)` construct in
    `refresh-manager.ts` still works for the Desktop-app client's
    refresh-token grant (it should — Desktop-app is a confidential
    client to Google, same as the LID client). Run an end-to-end
    refresh against Google and confirm the wire payload **includes**
    `client_secret`. (Superseded by Amendment 2026-05-28: the secret
    is kept, so there is no `oauth.None()` public-client construct to
    pin. This bullet is now a regression check that the existing
    construct still applies, not a new-symbol spike.)
  - **Fail condition:** Google rejects loopback for Desktop-app
    clients, or `aud` does not match `client_id`, or the hub
    rejects the token despite allowlisting. → fall back to bundled-
    defaults (alternative 1 in the "Why" section) and re-open the
    secret-distribution discussion. The threat model in Phase 4
    still applies; the operational story changes.

- [ ] **Spike B — MCP host tool-call lifetime.** The whole design
      assumes the host (Claude Code, Cursor) keeps a `tools/call`
      RPC alive for up to 5 minutes while the user signs in. This
      is **not** something we control — if the host enforces a
      shorter deadline (e.g. 60 s) the listener and browser are
      torn down mid-flow and the single-tool surface is unviable.
      Test both hosts against a stub MCP server whose tool handler
      sleeps for N seconds before responding, sweep N ∈ {30, 60,
      90, 120, 300}, and record the timeout floor each host
      enforces. Run on:
  - Claude Code (latest stable),
  - Cursor (latest stable),
  - the project's own `mcp-test-client.ts` for a sanity baseline.
  - **Success condition:** every host under test holds the RPC for
    ≥300 s without cancelling. Then set the `authenticate`
    deadline to 5 min as planned.
  - **Partial success (some host floor < 300 s but ≥ 60 s):**
    shorten the `authenticate` deadline to fit the lowest floor
    minus a 10 s safety margin, document the floor in the README,
    and proceed with the single-tool design.
  - **Fail condition (any host floor < 60 s):** the single-tool
    design does not survive. Fall back to a two-tool shape —
    `authenticate_begin` returns the listener URL and starts the
    listener; `authenticate_await` polls the listener state.
    Loopback + PKCE still applies; only the MCP surface changes.
    Add this as a separate plan and pause this one.

Record spike outcomes in this plan's verification log as they are
run. Per Amendment 2026-05-28 they no longer block the Phase 1 PR,
but a merged Phase 1 should still carry the Spike B host-deadline
result (it determines the final `authenticate` timeout constant) and
the manual end-to-end check.

## Phase 1 — loopback+PKCE implementation

### Pre-work: module reshuffle

`auth/device-flow.ts` exports 16 symbols; before Phase 3 deletes the
file, every non-device-flow-specific symbol needs a new home so the
deletion really is a clean `git rm`. Full inventory:

| symbol | disposition | new home |
| --- | --- | --- |
| `MissingCredentialsConfigError` | **rename + move** to `MissingOAuthConfigError` | `src/auth/oauth-config.ts` |
| `redactTokens` | move | `src/auth/redact.ts` |
| `DeviceFlowEnvConfig` (interface) | **rename + move** to `OAuthEnvConfig` | `src/auth/oauth-config.ts` |
| `loadDeviceFlowConfigFromEnv` | **rename + move** to `loadOAuthConfigFromEnv`; reads **both** `QUARTO_HUB_MCP_CLIENT_ID` and `QUARTO_HUB_MCP_CLIENT_SECRET` (unchanged behaviour — secret retained per Amendment 2026-05-28) | `src/auth/oauth-config.ts` |
| `discoverAuthorizationServer` | move | `src/auth/oauth-config.ts` |
| `_resetDiscoveryCache` (test hook) | move — must live with the cache it resets | `src/auth/oauth-config.ts` |
| `AuthorizationServerSpec` (interface) | **delete** with device-flow | — |
| `buildAuthorizationServer` | **delete** with device-flow (only `device-flow.test.ts` uses it) | — |
| `DeviceFlowError`, `DeviceFlowDeniedError`, `DeviceFlowExpiredError` | **delete** with device-flow | — |
| `DeviceFlowClientConfig`, `DeviceFlowRequestOptions` (interfaces) | **delete** with device-flow | — |
| `PollResult` (type) | **delete** with device-flow | — |
| `initiateDeviceFlow`, `pollDeviceFlowOnce` | **delete** with device-flow | — |

Verified (`grep` over `src/`) that `buildAuthorizationServer` /
`AuthorizationServerSpec` have no callers outside
`device-flow.test.ts`, which itself is deleted in Phase 3 — so they
go with the file, not into `oauth-config.ts`.

- [x] Create `src/auth/oauth-config.ts` containing
      `loadOAuthConfigFromEnv`, `OAuthEnvConfig`,
      `MissingOAuthConfigError`, `discoverAuthorizationServer`,
      and `_resetDiscoveryCache`.
- [x] Create `src/auth/redact.ts` with `redactTokens` and its
      `TOKEN_PATTERN` regex (small, no deps).
- [x] Update imports in `src/index.ts`, `src/tools.ts`,
      `src/auth/auth-tools.ts`, and any other in-tree caller so they
      point at the new modules. After this pre-work,
      `device-flow.ts` exports *only* device-flow-specific symbols;
      Phase 3 then `rm`s the file with no dangling references.

### Implementation

Per Amendment 2026-05-28 there is no spike pre-condition. Implement
against the working assumptions noted inline (5-minute deadline,
`ClientSecretPost` refresh, `prompt=consent` on). Run the Phase 0
spikes as validation during this phase or before merge; revisit the
relevant item only if a spike contradicts an assumption.

- [x] PKCE primitives. Reuse `oauth4webapi` (already a project dep):
      `generateRandomCodeVerifier()`, `calculatePKCECodeChallenge()`,
      `generateRandomState()`. Rationale: the rest of the auth code
      already routes through `oauth4webapi`; rolling our own ~20-line
      version with Node `crypto` would diverge from the established
      pattern for no gain. Wrap in a thin `src/auth/pkce.ts` only if
      a stable surface is needed for tests.
- [x] Local HTTP listener in `src/auth/loopback.ts`:
  - Bind to the **literal** `127.0.0.1`, not `localhost`. `localhost`
    resolves via DNS / `/etc/hosts` and has historically been
    hijackable on some installed-app stacks; the IP literal is the
    fix RFC 8252 §7.3 recommends. v1 is IPv4 only; if we ever see a
    user on a v6-only loopback we'll add `::1` then.
  - Port selection: default to `0` (kernel-picks) for the normal
    desktop case; honour an explicit override from the
    `--redirect-port <N>` CLI flag (see below) for SSH-tunnel
    users. The chosen port flows into `redirect_uri` verbatim and
    must match byte-for-byte at the token-exchange step.
  - Single route `/callback` accepting `?code=&state=` or
    `?error=&state=`; any other path → 404 (do not echo path).
  - **Host-header allowlist (DNS-rebinding defence).** Before
    parsing query params, reject (`400`, empty body, no listener
    teardown) any request whose `Host` header is not exactly
    `127.0.0.1:<port>` for our bound port. The bind to the IP
    literal closes outbound rebinding, but a malicious tab open in
    the user's browser during the auth window can still POST/GET to
    `http://attacker-controlled-name/callback?code=…` and — if that
    name resolves (via cached A-record rebinding) to `127.0.0.1`
    after the initial fetch — land on our listener. The fixed-host
    check is the standard RFC 8252 §8.4 mitigation. Reject **before**
    state validation so attackers can't probe for active flows by
    timing the response. Add a Phase 1 test that hits the listener
    with a forged `Host: evil.example` header and asserts a 400 with
    the listener still bound.
  - On hit: validate `state` in constant time (CSRF defence), serve
    the success HTML response (see next item), then resolve the
    promise and close the listener.
  - 5-minute timeout, 1-shot listener (close on first valid hit,
    error response from Google, or timeout).
  - Clean shutdown on SIGINT/SIGTERM — the listener registers and
    unregisters its own handlers so they don't outlive the flow.
- [x] Callback response page — security-spec'd, not free-form HTML:
  - Status: `200 OK`; respond fully **before** closing the listener
    so the browser doesn't show a connection-reset error.
  - Body: single self-contained HTML doc with inline CSS, **no
    JavaScript**, no external resources (no fonts, images, fetches).
    Eliminates exfiltration and referer-leak vectors.
  - Headers: `Content-Type: text/html; charset=utf-8`,
    `Cache-Control: no-store`, `Referrer-Policy: no-referrer`,
    `X-Content-Type-Options: nosniff`,
    `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'`.
  - Copy: "Authenticated. You can close this tab." On error,
    surface the error code from `?error=` (already attacker-known)
    plus the same "you can close" guidance.
- [x] Browser opener — cross-platform, with manual-URL fallback:
  - macOS: `open <url>` (pass the URL as a single argv element to
    `spawn`, no shell).
  - Linux: `xdg-open <url>` (same — single argv element, no shell).
  - Windows: **`cmd.exe /c start "" "<url>"`** — invoked via
    `spawn('cmd.exe', ['/c', 'start', '', url], { windowsVerbatimArguments: false })`.
    Two gotchas the literal `start <url>` form gets wrong:
    1. `cmd.exe` treats `&` as a statement separator, and OAuth
       authorization URLs are dense with `&` (`response_type=code&client_id=…&redirect_uri=…&state=…`).
       Quoting the URL prevents that interpretation.
    2. `start`'s first quoted argument is the *window title*, not the
       URL. The empty `""` is a placeholder title so the quoted URL
       is parsed as the target. Without it, `start "https://accounts.google.com/…"`
       opens an empty CMD window titled with the URL and never
       launches the browser.
    A test fixture asserting the constructed argv vector matches the
    expected `['/c', 'start', '', '<url-with-&-and-=>']` form is the
    cheapest way to keep this from regressing.
  - Default to in-tree ~15-line helper using `child_process.spawn`;
    revisit bundling the `open` npm package if corner cases warrant.
  - **Failure handling does not change the control flow** — the
    listener is bound *before* the browser is launched and the
    `authenticate` tool blocks on it regardless of whether the
    launch succeeded. On non-zero exit / spawn error the only
    observable difference is the tool's eventual response text:
    on success-after-fallback it appends a one-line note
    ("Browser launch failed; you signed in manually."); on
    timeout-after-fallback the error text includes the manual-paste
    URL so the user can retry. We never return early on launch
    failure — the user may still complete the flow by pasting the
    URL into a browser themselves, and the listener has to be live
    when that callback lands.
  - **Always surface the URL up-front, not only on launcher
    error.** Immediately after the listener binds and *before*
    invoking the browser opener, surface the full authorization URL
    via **two channels simultaneously** (not redacted — the URL is
    public, and the listener port + PKCE verifier already gate
    misuse):
    1. **MCP `notifications/progress` against the `tools/call`
       progress token.** This is the primary, host-UI-visible
       channel: hosts that implement the MCP progress UI (Claude
       Code, Cursor) surface progress notifications inline with the
       in-flight tool call, exactly where the user is already
       looking. Construction:
       - **Only fire if** the incoming `CallToolRequest` carried a
         `_meta.progressToken` (per MCP spec, progress notifications
         are *only* valid when the caller requested progress). If the
         token is absent, skip silently — do not synthesise a token,
         do not error. The MCP SDK exposes this on the request-handler
         context; verify the accessor name against the SDK version in
         `package.json` during Phase 1 (same accessor-audit note as
         the `AbortSignal` wiring above — do them together).
       - Send one notification at bind-time with the URL in the
         `message` field, `progress: 0`, `total: 1`. Progress
         notifications carry no semantic state beyond
         message-of-the-moment for this flow; do not emit a stream of
         them.
       - On any settle (success / error / abort / timeout) do not
         emit a closing progress notification — the `CallToolResult`
         response is itself the terminal signal per MCP semantics.
    2. **stderr at INFO level.** Belt-and-braces for hosts that
       don't surface MCP progress, for SSH-tunnel users tailing the
       remote process, and for `mcp-test-client.ts`. Writing to
       stderr at bind-time means any host whose UI tails the
       subprocess's stderr always has an actionable link without
       waiting out the deadline.

    Rationale for both: on many headless Linux / SSH-only machines
    `xdg-open` exits 0 successfully but opens nothing visible to the
    user, who would otherwise see a spinner for the full deadline
    (up to 5 min) before the timeout-path response surfaces a URL.
    MCP progress is the right channel by spec; stderr is the
    universal fallback. The launcher-failure response-text fallback
    above stays as-is — it is the third line of defence for users
    who see neither.

    **Test additions for this dual-channel surfacing (added to the
    Phase 1 test list):**
    - Invoke `handleAuthenticate` with a `progressToken` in
      `_meta`; assert exactly one `notifications/progress` is sent
      at bind-time, with `progress: 0`, `total: 1`, and `message`
      containing the full authorization URL.
    - Invoke `handleAuthenticate` *without* a `progressToken`;
      assert zero progress notifications are sent and the flow
      otherwise proceeds normally (no synthesised token, no error).
    - On settle (success, timeout, abort) assert no further
      progress notifications fire — the `CallToolResult` is the
      terminal signal.
- [x] Authorization URL construction:
  - Endpoint: `https://accounts.google.com/o/oauth2/v2/auth`
  - Params: `response_type=code`, `client_id`,
    `redirect_uri=http://127.0.0.1:<port>/callback`,
    `scope=openid email profile`, `code_challenge`,
    `code_challenge_method=S256`,
    `state` from `oauth4webapi.generateRandomState()` (≥128 bits of
    entropy, base64url-encoded; per `oauth4webapi`'s implementation
    this draws from `crypto.getRandomValues`).
  - **`access_type=offline` — required.** Google only issues a
    `refresh_token` when this is set; the default (`online`) returns
    an `id_token` but no `refresh_token`, which would silently break
    every code path in `refresh-manager.ts` and force the user to
    re-authenticate each time the ID token expires (~1 h). Add a
    Phase 1 token-exchange test that asserts the response body
    contains a non-empty `refresh_token` — this is the regression
    guard for accidentally dropping the param.
  - **`include_granted_scopes=true` — set.** Lets a future scope
    addition piggy-back on the existing grant rather than forcing a
    fresh consent screen. No cost today; pays off if we ever add a
    scope.
  - **`prompt=consent` — set by default.** Not a UX preference: for
    Google's Desktop-app client, a *returning* user who has already
    consented to the app is frequently issued a new `id_token`
    *without* a `refresh_token` on subsequent authorizations even
    with `access_type=offline`. The Phase 1 regression test only
    catches dropped params on a fresh consent; without
    `prompt=consent` we would silently break every returning user's
    refresh path in production while every test passes. Spike A
    measures this directly (see Phase 0 Spike A's "second-run
    refresh-token return" item); if the spike confirms returning
    users *do* receive a refresh_token without `prompt=consent`, we
    may drop it for the UX win, but the default until then is on.
- [x] Token exchange at `https://oauth2.googleapis.com/token`:
  - POST `grant_type=authorization_code`, `code`, `redirect_uri`
    (**byte-exact** match of the value sent in the auth request),
    `client_id`, `client_secret`, `code_verifier`. (Per Amendment
    2026-05-28 the `client_secret` is sent — Google's Desktop-app
    client requires it on the token exchange; PKCE's `code_verifier`
    is sent in addition, not instead.)
  - Use `oauth.ClientSecretPost(clientSecret)` as the client-auth
    construct, matching the existing refresh path, so the secret
    travels in the request body the same way it does today.
  - Returns the same JSON shape as the old device-flow token
    response → existing keyring storage code reused unchanged.
- [x] Refresh-token handling: **unchanged** (Amendment 2026-05-28).
      `src/auth/refresh-manager.ts` keeps calling
      `oauth.ClientSecretPost(this.deps.config.clientSecret)` at
      line ~127 — the Desktop-app client is confidential to Google
      and the secret is required on the refresh grant, same as today.
      `clientSecret` stays in `RefreshClientConfig`
      (refresh-manager.ts:49) and in `AuthFlowConfig`
      (auth-tools.ts:60). The refresh request stays
      `grant_type=refresh_token` + `client_id` + `client_secret` +
      `refresh_token`. (The original draft swapped this to a
      public-client `oauth.None()` construct; that change is dropped.)
- [x] Update the `refresh-manager.ts` top-of-file doc-comment
      (currently lines 19–26, the **"Refresh-token persistence rule"**
      paragraph). It documents an empirical 2026-05-19 finding that
      Google does not rotate refresh tokens for the **Limited-Input-
      Devices** client type. That observation is no longer load-
      bearing once we switch to the Desktop-app client. Replace it
      with the rotation behaviour Spike A actually records (verbatim
      from the Verification log) — one of:
  - "Spike A 2026-MM-DD confirmed Desktop-app client does not rotate
     refresh tokens; the defensive 'keep prior on missing field' rule
     below still applies and is the correct fallback for any future
     IdP change.", or
  - "Spike A 2026-MM-DD confirmed Desktop-app client **does** rotate
     refresh tokens on every grant; the defensive rule below is now
     also the steady-state path and persists each new value."

      The defensive rule itself (persist when present, keep prior
      when absent) stays as-is — it's correct under both behaviours.
      Only the empirical paragraph rotates. Do not leave the LID-
      client reference in place; a future reader will assume the
      observation is current and reason about the wrong IdP client
      type.
- [x] Single `authenticate` MCP tool:
  - **Pre-flight short-circuits — match today's `handleStart`
    behaviour at `auth/auth-tools.ts:196–220`, do not regress
    either:**
    1. **Already-authenticated.** Call
       `refreshManager.getValidIdToken()` first. If it returns,
       respond `"Already authenticated as <email>. No action
       needed."` without binding a listener or opening a browser.
       Only `ReauthRequired` falls through to the loopback path;
       every other error propagates (network blip, malformed JWT,
       etc. — same rationale as today).
    2. **Hub does not require auth.** If
       `connectionManager.lastObservedAuthMode() === 'no-auth'`,
       respond `"The configured hub does not require
       authentication; no action needed."`. Only the positive
       `'no-auth'` observation triggers — `'requires-auth'` and
       `'unknown'` both fall through to the loopback flow, same as
       today.
    These run before any network or listener I/O.
  - Opens browser, runs listener, exchanges code, stores tokens in
    keyring on success.
  - Returns `"Authenticated as <email>."` on success.
  - Returns typed error on timeout, user cancel, listener failure,
    or token-exchange failure.
  - **Concurrency — none.** No `inflight` slot, no single-flight
    guard, no `finishTail`-style serialisation. The MCP host
    serialises its own `tools/call` requests (it awaits each
    `CallToolResult` before issuing the next), so two concurrent
    `authenticate` invocations cannot arise under a well-behaved
    host. Code a single handler that binds a listener, opens a
    browser, and returns — no shared state to protect. If a
    misbehaving host *does* double-fire, both flows run
    independently on different kernel-picked ports; PKCE and
    `state` bind each flow's tokens to its own callback so this
    is a UX defect, not a security one. Do not borrow the
    `RefreshManager.forceRefresh` or `finishTail` patterns — they
    were correct for their problems (genuine token-cache mutation
    races; device-flow finish-call coalescing) and are not needed
    here. Document the assumption in a single comment at the
    top of `handleAuthenticate`: "MCP stdio hosts serialise
    `tools/call`; this handler intentionally has no concurrency
    guard. Two simultaneous calls is undefined-but-non-corrupting
    behaviour."
  - **MCP-host cancellation mid-flow.** Independent of
    concurrency — a single in-flight `authenticate` can be
    cancelled by the host (e.g. when the user clicks cancel on
    the host's tool-progress UI) via `notifications/cancelled`.
    Thread the `AbortSignal` exposed by the MCP SDK's
    `CallToolRequest` handler context (`extra.signal` on
    `@modelcontextprotocol/sdk` ≥1.x — verify the exact accessor
    against the version actually in `package.json` during Phase
    1; do this at the same time as the progress-token accessor
    audit) into both the loopback listener (which closes its
    `http.Server` on abort) and the browser-opener subprocess
    (which is killed on abort). Without this wiring, a host
    cancel leaves the listener bound and the browser subprocess
    alive until the deadline fires. Phase 1 test: start
    `handleAuthenticate`, fire `abort()` on the signal, assert
    the listener is closed, the browser-opener subprocess (if
    still alive) is killed, the promise rejects with a typed
    cancellation error, and a follow-up `handleAuthenticate` is
    accepted normally (no stale state from the cancelled flow).
  - **Timeout:** governed by Spike B's outcome. If every host holds
    RPCs ≥300 s the deadline is 5 min (matches the device-flow
    plan); otherwise it is the lowest host floor minus a 10 s
    safety margin, per Spike B's "partial success" branch. The
    chosen value lives in a single named constant in
    `auth/loopback.ts` so README + tests reference one source.
- [x] **Logging policy for the new flow** — explicit because the
      existing code is consistent and we shouldn't drift:
  - Every log call site reachable from the loopback path
    (`auth/loopback.ts`, `auth/oauth-config.ts`, the new
    `authenticate` handler) funnels through `redactTokens` (now at
    `auth/redact.ts` per the module reshuffle), same rule as the
    five existing sites: `connection-manager.ts:185, 366`,
    `tools.ts:405`, `index.ts:79, 86, 184`,
    `auth/credential-store.ts:158, 174, 186`,
    `auth/auth-tools.ts:283, 331, 335`.
  - The authorization URL is **explicitly not** routed through
    `redactTokens`. It is public by construction (browser address
    bar, server access logs, referer headers absent only because
    we set `Referrer-Policy: no-referrer` on *our* response).
    Redacting it would defeat the stderr-at-bind-time mitigation
    in the Browser opener item, which is the actionable URL
    surface for headless machines.
  - The kernel-picked port (when `--redirect-port` is omitted) is
    logged to stderr at INFO level on bind so SSH-tunnel users
    aren't forced to set `--redirect-port` just to learn the port.
  - The `code_verifier`, the auth `code`, the `state` value, and
    every token field are never logged in any branch, redacted or
    otherwise — `redactTokens` handles the token shapes by
    pattern, but `code_verifier` / `state` / `code` do not match
    those patterns and must be filtered at the call site (i.e.
    don't pass them in). Add a Phase 1 review-checklist note for
    this; mechanical enforcement is out of scope.
- [x] Remove `authenticate_start` and `authenticate_finish` MCP tools.
      Keep `authenticate_clear` on the MCP surface (see next item
      for its one behavioural change).
- [x] **`authenticate_clear` revocation.** Today's `handleClear`
      (`ts-packages/quarto-hub-mcp/src/auth/auth-tools.ts:272`)
      clears the in-memory cache and the keyring entry; its tool
      description (line 120) explicitly disclaims any Google-side
      effect. The stored refresh token is long-lived at Google and
      remains usable until the user goes
      to `myaccount.google.com` and revokes the grant by hand.
      That's the wrong default for an "escape hatch when the hub
      rejects cached credentials" tool — if a user is clearing
      because they suspect compromise, leaving a working refresh
      token at Google is the worst possible outcome. Implementation:
  - **Order: read → revoke → delete.** Read the refresh token from
    the keyring first; if present, POST to the revocation endpoint
    *before* the local delete. Reading first means a revoke-then-
    delete crash leaves the next `authenticate_clear` call able to
    retry the revoke. Deleting first would orphan the token.
  - **Revocation endpoint.** Discover via the existing
    `discoverAuthorizationServer` (after the Phase 1 module reshuffle
    it lives in `auth/oauth-config.ts`) and use the
    `revocation_endpoint` field from the OIDC discovery doc. For
    Google this is `https://oauth2.googleapis.com/revoke` today; do
    not hardcode — discovery is already a sunk cost in the
    authenticate path. POST `token=<refresh_token>` +
    `token_type_hint=refresh_token` as `application/x-www-form-
    urlencoded`. Google's revocation endpoint does not require client
    authentication — POST the `token` alone (the token itself is the
    capability being burned). Do not send `client_id` /
    `client_secret` on the revoke even though they are configured for
    the token-exchange path; the revoke endpoint neither needs nor
    expects them.
  - **Revoking the refresh token also invalidates any access tokens
    derived from it** (per RFC 7009 §2.1 and Google's documented
    behaviour). One revoke call suffices; do not separately revoke
    the ID token — ID tokens are JWTs and can't be revoked at the
    IdP anyway (they expire on their own ≤1 h timer; the hub-side
    `sub_denylist` deferred to future work is the right closure for
    that window, same as it is for stolen-token-without-clear).
  - **Best-effort: revocation failure does NOT block local cleanup.**
    Network errors, 5xx, expired-tokens-returning-200/400-with-
    `invalid_token` — the local delete proceeds regardless. The
    revoke is an extra layer; if it fails, the user is no worse off
    than today's behaviour. The response text distinguishes the
    cases:
    - All clean: `"Quarto Hub credentials cleared and revoked at
      Google. Call authenticate to sign in again."`
    - Local delete OK, revoke failed: `"Quarto Hub credentials
      cleared locally. Google-side revocation failed (<short
      reason>); revoke the grant at myaccount.google.com if you
      need it gone server-side."` (Short reason = HTTP status +
      `error` field if RFC-6749-shaped, or a redacted exception
      message otherwise — `redactTokens` from `auth/redact.ts`.)
    - Local delete failed: existing error path (line 282) is
      already correct; mention whether revoke ran in the message so
      the user knows the Google-side state.
  - **No revoke when no refresh token present.** `authenticate_clear`
    is idempotent today (`safe to call when no credentials are
    present`) and that contract is preserved. If the keyring read
    returns no entry, skip the revoke entirely and return the
    existing "nothing to clear" response shape.
  - **Logging.** The revoke POST and its response go through the
    same `redactTokens` policy as every other log site (the refresh
    token itself must never appear in logs, including in error
    messages — the response-text contract above is the only place
    a short reason surfaces, and that path is already filtered).
  - **Tool-description text.** Rewrite
    `AUTH_TOOL_DEFINITIONS[authenticate_clear].description` (lines
    115–122) to reflect the new contract: drop the "Does not touch
    Google-side grants" disclaimer, replace with one sentence
    naming the best-effort revoke and pointing at
    `myaccount.google.com` only as the manual-recovery path if
    revocation fails. This is in addition to the "discard any in-
    progress device-flow state" → "discard any in-progress sign-in"
    rewording already listed in the in-tree sweep item below.
  - **Success-message text.** The current `handleClear` returns
    `"Quarto Hub credentials cleared. Call authenticate_start to
    authenticate again."` (line 287). Update to reference
    `authenticate` and use one of the two response strings above.
    Add to the in-tree sweep checklist for completeness.
- [x] Sweep in-tree references to the old tool names. Known sites
      (verified by `grep` 2026-05-28):
  - `src/tools.ts:46` — `connect_project`'s description text
    embeds `"call authenticate_start to begin the device-flow"`;
    rewrite to name `authenticate` and drop "device-flow".
  - `src/tools.ts:386–393` — the dispatcher branch matches
    `'authenticate_start' | 'authenticate_finish' | 'authenticate_clear'`;
    collapse to `'authenticate' | 'authenticate_clear'`.
  - `src/auth/auth-tools.ts` `AUTH_TOOL_DEFINITIONS` (lines ~80–130)
    — replace the two old `Tool` entries with a single
    `authenticate` entry; the `authenticate_clear` entry stays but
    its description needs two changes (covered in detail by the
    "`authenticate_clear` revocation" item above): "discard any
    in-progress device-flow state" → "discard any in-progress
    sign-in"; "Does not touch Google-side grants; revoke those at
    myaccount.google.com if needed" → one sentence naming the
    best-effort revoke and the manual-recovery URL only as a
    fallback when revocation fails.
  - `src/mcp-test-client.ts` — audit for hard-coded tool names used
    by integration tests; update call sites.
  - `src/connection-manager.ts` error messages — audit for any
    "call `authenticate_start`" hints surfaced on `AuthRequired` /
    `ReauthRequired` paths, rewrite to `authenticate`.
  - `README.md` and `claude-notes/instructions/hub-mcp-operator-runbook.md`
    are covered separately under Phase 2.
- [x] **Keep** `QUARTO_HUB_MCP_CLIENT_SECRET` (Amendment 2026-05-28).
      `loadOAuthConfigFromEnv` still requires both `CLIENT_ID` and
      `CLIENT_SECRET`; a partial config (one set, the other not) fails
      with the existing `MissingOAuthConfigError` naming the missing
      var. No removal, no migration-pointer error for the secret.
- [x] Leave `hasAuthEnv` in `src/index.ts` (currently
      `CLIENT_ID || CLIENT_SECRET` at line ~99) **as-is** — both vars
      still participate. The "either var present → attempt auth
      bootstrap; neither present → run no-auth" behaviour is
      unchanged, and `loadOAuthConfigFromEnv` remains the single point
      that rejects a partial config. Preserve the "no-auth hubs still
      work" path explicitly.
- [x] Add `--redirect-port <N>` CLI flag in `src/index.ts`
      alongside `--server` / `--read-only`. Validates as a TCP
      port (`1..=65535`), defaults to `0` (kernel-picks) when
      absent, threads through to the loopback listener and into
      `redirect_uri`. This is the bridge that makes the SSH
      port-forwarding story in Phase 3 actually work — without a
      known port, `ssh -L <port>:127.0.0.1:<port>` has nothing to
      point at. Implementing it in Phase 1 keeps the Phase 3
      headless documentation accurate when it ships.
- [x] Existing tests that need to be torn down or rewritten before
      new tests land — this is **not** a touch-up, the device-flow
      assumptions are baked in deeply:
  - `src/auth/auth-tools.test.ts` (841 lines) — the entire test
    surface is `handleStart` / `handleFinish` / the coalesce window
    / the `finishTail` mutex chain / `authorization_pending` /
    `slow_down` polling. None of these concepts survive. Rewrite
    the file end-to-end against the new `handleAuthenticate`
    surface; keep only `handleClear` tests, which carry over with
    one extension — the four new revocation cases listed in the
    test additions further down.
  - `src/auth/refresh-manager.test.ts` (524 lines) — **no change**
    required (Amendment 2026-05-28). It asserts the refresh request
    includes `client_secret` via `ClientSecretPost`, which remains
    correct. The `FAKE_CLIENT_SECRET` fixture (line ~31) stays.
  - `src/auth/device-flow.test.ts` (535 lines) — deleted wholesale
    in Phase 3 alongside `device-flow.ts`. Anything still useful
    (e.g. the `redactTokens` known-answer cases) gets lifted into a
    new `src/auth/redact.test.ts` first.
  - `src/hub-mcp.test.ts` (379 lines) and
    `src/connection-manager.test.ts` (543 lines) — audit for
    references to the old tool names and for any fixtures that set
    both `CLIENT_ID` and `CLIENT_SECRET`. Adjust as needed; these
    do not require a full rewrite.
- [x] New unit tests (match the project's existing mocking pattern
      — `nock` is not in use here, do not introduce it):
  - PKCE: known-answer tests against RFC 7636 §4.6 example values
    (whether using the `oauth4webapi` helpers directly or through
    our thin wrapper).
  - Listener: spin up listener, fire a `fetch` at it, verify the
    returned promise resolves with `code`; verify state mismatch
    rejects with a typed error; verify the response includes the
    spec'd security headers.
  - Sequential reuse: call `handleAuthenticate` once and let it
    settle (success, rejection, or abort); a follow-up call must
    be accepted normally. This is the only concurrency-shaped
    test — there is no single-flight guard to exercise, but we
    do want to confirm the handler leaves no stale state behind
    (listener closed, subprocess reaped, no resident timers) that
    would interfere with the next call. The host-serialised model
    means this is the realistic worst case.
  - SIGINT mid-flight: listener tears down cleanly and the in-flight
    promise rejects with a typed cancellation error.
  - MCP-host cancellation mid-flight: invoke `handleAuthenticate`
    with an `AbortSignal`, fire `abort()` while the listener is
    waiting, assert the listener is closed, the browser-opener
    subprocess (if still alive) is killed, the promise rejects
    with a typed cancellation error, and a follow-up
    `handleAuthenticate` is accepted normally.
  - Token exchange: mock token endpoint with the existing test
    pattern; verify the request body contains **both** `code_verifier`
    and `client_secret` (Amendment 2026-05-28 — PKCE is layered on top
    of the secret, not a replacement).
  - Refresh path regression: `refresh-manager.test.ts` continues to
    assert the refresh request **includes** `client_secret`. No
    rewrite — the existing assertions stay correct.
  - `--redirect-port` validation: parser accepts `1..=65535`,
    rejects `0` (the kernel-pick default is reached by *omitting*
    the flag, not by `--redirect-port 0` — keeps the "stable port"
    intent unambiguous), rejects non-numeric, rejects out-of-range
    (`-1`, `65536`, `99999`), rejects values inside the privileged
    range (`<1024`) with an error message naming the typical free
    range to use over SSH. Bonus assertion: the validated port
    flows verbatim into the URL the listener logs to stderr at
    bind-time (per the Logging policy item) and into
    `redirect_uri`.
  - DNS-rebinding `Host` check: send a request with `Host:
    evil.example` to the bound listener and assert a 400 with no
    listener teardown and no `state` validation having run
    (rejection must precede state parsing so the response is
    timing-uniform with respect to flow state).
  - `authenticate_clear` revocation, four cases:
    1. **Refresh token present + revoke succeeds.** Mock the
       discovery doc to point `revocation_endpoint` at a stub that
       returns 200. Pre-seed the keyring with a refresh token. Call
       `handleClear`. Assert: revoke POST hit the stub with
       `token=<refresh>` + `token_type_hint=refresh_token` in a
       `application/x-www-form-urlencoded` body, *no* `client_id`
       or `client_secret` on the wire, keyring entry is gone after
       the call, response text matches the "cleared and revoked"
       string.
    2. **Refresh token present + revoke fails (network/5xx).** Stub
       returns 500 or rejects the connection. Assert: keyring entry
       is still gone (best-effort revoke does not block local
       delete), response text matches the "cleared locally,
       revocation failed" string with a redacted short reason, no
       refresh token leaks into the message.
    3. **No refresh token in keyring.** Empty keyring, call
       `handleClear`. Assert: revoke endpoint is *not* hit (no POST
       fired), response is the existing idempotent "nothing to
       clear" shape, no error.
    4. **Local delete fails.** Force the keyring delete to throw.
       Assert: response uses the existing failure path (line 282)
       and the message names whether the revoke ran, so the user
       knows the Google-side state.
- [ ] Manual end-to-end:
  - Real Google "Desktop app" client (test-tier project, not the
    canonical-hub client)
  - Real browser
  - Verify token lands in keyring with same schema as before
  - Verify hub accepts the token (add test `client_id` to a dev
    hub's audience allowlist)
  - Verify the refresh path works after the initial sign-in (force
    an ID-token expiry and confirm refresh succeeds using
    `client_secret` via `ClientSecretPost`, unchanged from today).
  - Document the invocation and observed output in this plan per
    the end-to-end-verification policy in `CLAUDE.md`

## Phase 2 — canonical Quarto Hub client + documentation

- [ ] Quarto team registers the canonical "Quarto Hub MCP" Desktop-app
      OAuth client at Google:
  - Verified-publisher consent screen
  - App name: "Quarto Hub MCP" (or similar)
  - Icon + support URL pointing at canonical Quarto documentation
- [ ] Add the canonical `client_id` to the canonical hub's audience
      allowlist (`--additional-audiences`).
- [ ] Document the canonical `client_id` **and** `client_secret` in
      the canonical hub's end-user onboarding (quarto.org / handbook /
      wherever the hub WebSocket URL is published). Per Amendment
      2026-05-28 the Desktop-app `client_secret` is required;
      Google's own docs note it is "not treated as a secret" for
      installed apps, but it must still be distributed and set in the
      env — keep the existing secret-handling guidance.
- [x] README updates:
  - **Keep** the `QUARTO_HUB_MCP_CLIENT_SECRET` row in Setup
    (Amendment 2026-05-28).
  - Replace the device-flow walkthrough with the loopback
    walkthrough: agent calls `authenticate`, browser opens, user
    signs in, returns to terminal with `"Authenticated as X."`
  - Update credential-storage section (still keyring, same schema)
  - Keep "Why both env vars must come from the operator." Update only
    the client *type* (Desktop app, not TV/Limited-Input) and the
    auth *flow* (loopback, not device flow); the audience-allowlist
    and consent-screen-ownership rationale, and the two-value
    distribution, are unchanged.
  - Update the `authenticate_clear` description in the tools
    reference to match the new contract: it now performs a
    best-effort revoke at Google before the local delete, with the
    manual `myaccount.google.com` step framed as the fallback when
    revocation fails (network down, token already invalid) rather
    than the only Google-side step the user is responsible for.
- [x] Operator-runbook updates
      (`claude-notes/instructions/hub-mcp-operator-runbook.md`):
  - Replace "TV and Limited Input devices" client-registration
    steps with "Desktop app" steps
  - **Keep** the secret-distribution section (Amendment 2026-05-28);
    update it to reflect the Desktop-app client and the
    "publish both `client_id` and `client_secret` to end users"
    framing. Google's installed-app secret is low-sensitivity but
    still distributed; the section stays.
  - Note: the canonical-hub operator follows the same steps as
    private operators (no bundled-default special case for v1)
- [x] Migration for existing users: their refresh tokens were issued
      against the device-flow client. After upgrading and switching
      `QUARTO_HUB_MCP_CLIENT_ID` (and `QUARTO_HUB_MCP_CLIENT_SECRET`)
      to the new Desktop-app values, the credential
      store keys by `(issuer, clientId)`, so the old entry is simply
      invisible to the new lookup — `getValidIdToken` throws
      `ReauthRequired` on first call and the normal sign-in prompt
      fires. No detection code, no migration warning; the old
      keyring entry becomes unreachable garbage that can sit until
      the user runs the documented keyring-clear command for
      housekeeping. Document this in the README's upgrade section
      ("on first run after upgrading, you'll be prompted to sign in
      once; the old credentials are stranded under the previous
      `client_id` and can be cleared at leisure") and move on.

## Phase 3 — device-flow removal

- [x] Delete device-flow code paths:
  - `auth/device.ts` (or equivalent)
  - `authenticate_start` / `authenticate_finish` tool registrations
  - Device-flow-specific tests
- [x] Delete or archive the device-flow section of the operator
      runbook. Done by rewriting the runbook in place — the
      device-flow / "TV and Limited Input devices" content is replaced
      with the Desktop-app + loopback flow (no separate archived
      appendix; the device-flow plan remains the historical record).
- [ ] After a migration grace period (typically one release):
  - Decommission the device-flow Google OAuth client (canonical
    hub) or document the decommissioning step for private operators
  - Remove the device-flow `client_id` from the hub's
    `--additional-audiences` once migration is confirmed complete
- [x] Headless story documentation:
  - README: explicit section covering the two options for headless
    users —
    (a) SSH port-forward — pick a free port `N`, start
    `quarto-hub-mcp --redirect-port N` on the remote host, run
    `ssh -L N:127.0.0.1:N <remote>` from the local machine, call
    the `authenticate` MCP tool; the manual-paste URL surfaces in
    the response and resolves to the local browser via the tunnel.
    (b) SPA cookie path from a graphical session on the same hub.
  - Operator runbook: matching coverage from the operator side.

## Phase 4 — threat-model documentation

- [x] New threat-model section in this plan (or update of the
      existing device-flow plan's threat-model) covering the loopback
      analysis. The load-bearing argument:

      **Device flow uniquely enables no-malware remote phishing.**
      The user-facing leg ("enter `WXYZ-ABCD` at google.com/device,
      sign in, approve consent") is decoupled from the device
      process. An attacker on the other side of the internet, holding
      only the `client_id` (and historically the `client_secret`),
      can:
      1. Initiate a device flow from their own server.
      2. Email the victim with a plausible cover story asking them to
         enter the `user_code`.
      3. Victim signs in to Google with their own credentials and
         approves a genuine, verified-publisher consent screen
         showing the legitimate app name. There is nothing on the
         consent screen identifying the *initiator* of the flow.
      4. Attacker polls `/token` and receives tokens minted under the
         victim's identity. Hub accepts them — correct audience, real
         Google signature, real `sub`.

      The trust model assumed by RFC 8628 ("the user trusts the
      device they are standing in front of") collapses when the
      device is an attacker's server and the only binding is an
      eight-character code. This attack class is well documented in
      the wild against Microsoft 365 device-flow clients (Storm-2372
      and related campaigns).

      **Loopback structurally cannot be phished the same way.** The
      user-facing leg embeds `redirect_uri=http://127.0.0.1:<port>`.
      The redirect after consent lands on the victim's own loopback
      interface, unreachable from any remote network. An attacker
      who only has the `client_id` cannot construct a flow that
      delivers tokens to themselves without first achieving code
      execution on the victim's machine — at which point the OAuth
      design is moot (keyring is already accessible). The remote
      no-malware attack mode is closed.

      **Other deltas vs device flow:**
      - **Unchanged:** secret-in-distribution-channel threat. Per
        Amendment 2026-05-28 the `client_secret` is retained
        (Google's Desktop-app client requires it), so the operator
        still distributes it and the same channel-exposure
        consideration as device flow applies. This is *not* a benefit
        of the switch; the switch is justified by the
        no-malware-remote-phishing closure below. Note Google
        documents the installed-app secret as not truly confidential,
        so its exposure is low-severity — but it is not eliminated.
      - **Reduced:** auth-code interception. PKCE binds the code to
        the originating process's `code_verifier`; even if the code
        leaked (browser history, referer, local log scraping), it is
        useless without the verifier in process memory.
      - **Strengthened:** CSRF defence. The `state` parameter binds
        the callback to the originating flow; mismatched `state`
        rejects.
      - **Unchanged:** stolen ID/refresh tokens authenticate to the
        hub for up to ≤1 h (ID) / indefinitely (refresh, until user
        revokes grant). Closing the ID-token window still requires
        the hub-side `sub_denylist` deferred from v1.
      - **Unchanged:** brand-confusion residual. If an attacker
        already has code execution on the victim's machine, they can
        drive a real loopback flow under our `client_id` and capture
        tokens via their own local listener. Same residual as bundled
        defaults; documented as accepted per RFC 8252. Dynamic client
        registration (RFC 7591) is the deeper fix; out of scope.

      **Loopback-specific threat: DNS-rebinding via the user's
      browser.** Binding to the IP literal `127.0.0.1` closes
      *outbound* rebinding (we never resolve `localhost`), but the
      listener is still reachable from any page open in the user's
      browser during the auth window — including via a hostname that
      a malicious DNS server has rebound to `127.0.0.1` after its
      initial resolution. Without a defence, an attacker page could
      forge a `/callback?code=…&state=…` request to our port. The
      Phase 1 listener mitigates this by rejecting any request whose
      `Host` header is not exactly `127.0.0.1:<port>` (RFC 8252 §8.4),
      with the rejection happening *before* `state` validation so
      timing cannot leak whether a flow is in progress. Residual:
      attacker still needs to learn the listener port — a non-issue
      for `port=0` (entropy from kernel choice) but worth flagging
      when `--redirect-port` is set to a stable value (the port name
      itself is operationally public, e.g. for SSH-tunnel users).
      The `Host`-check, the CSRF `state`, and PKCE all have to fail
      simultaneously for token capture; that's acceptable defence in
      depth for the residual.

## Open questions

- **Bundled `open` package vs in-tree browser launcher?** Default to
  in-tree (~15 lines, no new dep). Revisit if a corner case argues
  otherwise.

Resolved (recorded for posterity):

- **`prompt=consent` on every flow?** **Default on**, until Spike A
  measures that a returning user receives a non-empty
  `refresh_token` without it. This is a correctness gate (refresh
  path silently breaks without a refresh_token on the second
  authorization), not a UX preference. See Phase 1 Authorization
  URL construction + Spike A's "second-run refresh-token return"
  item.
- **`--redirect-port <N>` flag for SSH-tunnel scenarios?** Yes —
  committed as a Phase 1 implementation item. The headless-via-SSH
  story documented in Phase 3 leans on it; deferring would leave
  that doc unactionable. Trivial to implement (one flag, one
  validator, one thread-through).
- **PKCE: roll our own vs. `oauth4webapi`?** Use `oauth4webapi`;
  it's already a project dep and used by every other auth path.
  See Phase 1.
- **What happens to `authenticate_clear`?** Kept on the MCP
  surface — flow-independent, documented recovery action. Gains
  best-effort Google-side refresh-token revocation: the public
  contract widens from "delete the local copy" to "render the
  credential unusable, locally and at Google." Revocation failure
  does not block local cleanup. See Phase 1
  "`authenticate_clear` revocation" item for the full
  read-→-revoke-→-delete order, endpoint discovery, response-text
  contract, and the four test cases.
- **Concurrent `authenticate` calls?** Not guarded against. MCP
  stdio hosts serialise `tools/call` requests (each
  `CallToolResult` is awaited before the next call leaves the
  host), so two concurrent invocations cannot arise under a
  well-behaved host. The cost of a single-flight guard
  (`inflight` slot, detached-cleanup pattern, two extra tests)
  outweighs the benefit for an event that shouldn't happen, and
  the misbehaving-host failure mode is a UX defect (two listeners
  on different ports) not a security one — PKCE and `state` bind
  each flow's tokens to its own callback. Same logic retires the
  `authenticate_clear`-while-`authenticate`-in-flight question:
  under host serialisation it can't happen, and we don't carry
  bookkeeping for it. See Phase 1 single-`authenticate` tool
  "Concurrency — none" sub-item.
- **MCP-host-issued cancellation mid-flow?** Independent of the
  concurrency decision — a single in-flight `authenticate` can
  still be cancelled by the host. Observed via the `AbortSignal`
  from the MCP SDK's `CallToolRequest` handler context; listener
  and browser-opener subprocess tear down on abort, and the next
  `handleAuthenticate` call is accepted normally. See Phase 1
  single-`authenticate` tool "MCP-host cancellation mid-flow"
  sub-item.
- **Headless-Linux silent `xdg-open` exit-0 lock-out?** Mitigated
  by surfacing the authorization URL via *both* an MCP
  `notifications/progress` against the `tools/call` progress token
  (when the caller supplied one) *and* stderr at INFO level, both
  fired at listener-bind time before invoking the browser opener
  and regardless of its outcome. MCP progress is the host-UI
  primary; stderr is the universal fallback. See Phase 1 Browser
  opener item for the full construction (including the
  no-progress-token-no-op rule and the test additions).

## Verification log

Spike outcomes (Phase 0) and end-to-end checks (Phase 1, manual
end-to-end item) land here. Per Amendment 2026-05-28, spikes no longer
gate the Phase 1 PR; fill these in as the spikes are run (Spike B's
host-deadline result and the manual end-to-end check should still
accompany a merged Phase 1). Each entry: date,
operator, command/invocation, observed result, pass/fail/partial
verdict, link to any captured artefacts (screenshots, raw token
responses with secrets redacted).

### Spike A — Desktop-app client + loopback + PKCE against real hub

- **Date:**
- **Operator:**
- **Test-tier Google project / `client_id`:**
- **Hub used (with `--additional-audiences` including the test
  `client_id`):**
- **Round-trip outcome (`aud` claim matches `client_id`? hub
  accepts the bearer on WebSocket connect?):**
- **Refresh-token behaviour (rotated / not rotated, response body
  shape vs the Limited-Input-Devices client):**
- **Second-run refresh-token return (same account, no
  `prompt=consent` on the second auth request — was a
  `refresh_token` returned?):** ☐ yes (safe to drop default)  ☐ no
  (default `prompt=consent` stays on)
- **Refresh construct (confirm `oauth.ClientSecretPost` still applies
  to the Desktop-app client; secret retained per Amendment
  2026-05-28 — no `oauth.None()` public-client construct):**
- **Verdict:** ☐ pass  ☐ fail (→ revisit; the device-flow path
  remains available as the fallback while the issue is diagnosed)

### Spike B — MCP host tool-call lifetime

| Host                        | Floor observed (s) | Notes |
| --------------------------- | ------------------ | ----- |
| Claude Code (vX.Y.Z)        |                    |       |
| Cursor (vX.Y.Z)             |                    |       |
| in-tree `mcp-test-client.ts`|                    |       |

- **Verdict:** ☐ all hosts ≥300 s (5-min deadline as planned)
  ☐ partial (lowest floor = ___ s, deadline set to ___ s)
  ☐ any host < 60 s (→ two-tool fallback per Phase 0)
- **Chosen `authenticate` deadline constant value:** ___ s

### Phase 1 implementation verification (2026-05-28)

**Implemented in this session** (code + tests; `npm run build` and
`npm test` both green — 141 passed / 2 pre-existing skips across 11
files, including new `pkce`, `redact`, `browser`, `oauth-config`,
`loopback`, `index` suites and the rewritten `auth-tools` suite):

CLI end-to-end checks against the **real built binary** (`node
dist/index.js`), inspected output:

- `--help` lists `--redirect-port <N>` with the SSH-tunnel guidance.
- `--redirect-port 80` → exits 1 with
  `--redirect-port must be a non-privileged port (1024-65535); for SSH
  tunnels pick one in the ephemeral range (e.g. 49152-65535). Got 80.`
- `--redirect-port abc` → exits 1 with `must be an integer`.
- No-auth env, `tools/list` → `connect_project, list_files, read_file,
  write_file, patch_file, create_file, delete_file, rename_file,
  create_project` (no auth tools, as expected).
- With `QUARTO_HUB_MCP_CLIENT_ID` + `QUARTO_HUB_MCP_CLIENT_SECRET` set
  (dummy values; live Google discovery succeeded), `tools/list` →
  `authenticate, authenticate_clear, …` — the device-flow tool pair is
  gone and the single `authenticate` tool plus `authenticate_clear`
  are registered.

**NOT run in this session** (require external resources, left
unchecked above):

- Spike A / Spike B (real test-tier Google project; live MCP-host
  deadline sweep).
- Manual end-to-end with a real Google "Desktop app" client + real
  browser + dev hub allowlist (keyring round-trip, hub WebSocket
  accept, forced-expiry refresh). The token-exchange and revocation
  wire contracts are covered by unit tests against stubbed endpoints
  (`code_verifier` + `client_secret` on the exchange; `token` +
  `token_type_hint` with no client auth on the revoke), but the live
  Google round-trip is unverified here.

### Phase 1 manual end-to-end check (per `CLAUDE.md` policy) — PENDING

- **Date:**
- **Invocation:**
- **Observed output (snippet):**
- **Keyring entry verified present, schema matches pre-change:** ☐
- **Hub WebSocket connect with new token succeeded:** ☐
- **Forced ID-token expiry → refresh succeeds using `client_secret`
  (via `ClientSecretPost`, unchanged):** ☐

## Future work (out of scope)

- **Bundled `client_id` defaults** for the canonical Quarto Hub.
  Removes the last per-user config step for canonical-hub users.
  Deferred to gather operational signal on end-user friction first.
  When revisited: extend `src/config.ts` to export a
  `DEFAULT_CLIENT_ID`, used when `QUARTO_HUB_MCP_CLIENT_ID` is unset.
- **Dynamic client registration (RFC 7591)** so each hub-mcp install
  registers its own `client_id` and there is no shared brand to
  phish under. Requires IdP support; Google does not currently offer
  it. Revisit if we move to self-hosted OIDC.
- **`sub_denylist` on the hub side** to close the ≤1 h stolen-ID-token
  window. Already noted as future work in the existing device-flow
  plan; cross-listed here.

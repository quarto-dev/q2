# CAP-16 design: hub-served MCP over Streamable HTTP

Status: **design for review** — gates any CAP-16 implementation. Written as
the Phase 6 "design doc first" item of
[2026-10-05-elevate-quarto-hub-mcp.md](2026-10-05-elevate-quarto-hub-mcp.md)
(bd-8iv9jty5). Resolves Q-5. Time-boxed per the plan's risk table; if any §
expands, it becomes its own document.

## 1. Goal and non-goals

Let browser-hosted agents (Claude.ai connectors, ChatGPT connectors, MCP
hosts without a stdio channel) use the full Quarto Hub MCP surface by
connecting to an MCP endpoint served by the hub itself, over the
2026-07-28 **stateless** Streamable HTTP model. Today `q2 mcp` and
`npx @quarto/hub-mcp` are stdio-only, which confines them to hosts that
spawn local processes.

Non-goals: no 2025-era session support (`Mcp-Session-Id`, GET streams,
resumability); no hub-side rendering farm (the `render` tool stays
opt-in and local to wherever the server code runs — see §8); no
authorization-server build-out beyond what the resource-server role
requires (the spec puts AS implementation out of scope, and our IdP
stays Google/other OIDC providers).

## 2. What Phase 6 already proved

The Phase 6 loopback listener
(`ts-packages/quarto-hub-mcp/src/http-loopback.ts`) is the rehearsal for
this design: the SDK v2.3 `createMcpHandler` serves our unmodified
`createServer` per-request (one fresh server instance per HTTP request
around a shared `ConnectionManager`), and the official conformance suite
passes the frozen 2026-07-28 server requirements against it with an
empty baseline — including `server-stateless`, MRTR (`input_required`)
round-trips across per-request instances, and DNS-rebinding protection.
The two gaps the loopback deliberately does not cover are **auth** and
**multi-tenant connection lifecycle** — the subject of this document.

Transport facts the design inherits:

- Per-request serving is the SDK's shape: no session state may live on a
  server instance; anything long-lived keys off an external identity
  (here: the token subject).
- `responseMode: 'auto'` is load-bearing: terminal errors get
  status-mapped JSON responses (`-32021` → HTTP 400); SSE upgrade is
  reserved for exchanges whose handlers emit related messages and for
  `subscriptions/listen` streams.
- The resource bridge unhooks per-instance listeners on close (Phase 6
  fix), so per-request instances are cheap and leak-free.

## 3. Endpoint shape

- `POST /mcp` on the hub origin, alongside `/ws` (sync) and `/auth/*`.
  JSON-RPC over the 2026-07-28 per-request envelope. One endpoint for
  all projects; project selection stays a tool call (`connect_project`)
  as on stdio — the alternative (per-project URL path) would multiply
  auth/resource-indicator surface for no ergonomic gain, and the
  `resource=` indicator (§4) then names the endpoint, not a project.
- **Legacy policy: `reject`.** The SDK handler can stateless-fallback
  2025-era clients, but every client we court (Claude.ai, ChatGPT,
  current hosts) negotiates 2026-07-28, and the fallback exists for
  deployments with a legacy client base. Rejecting keeps one wire
  contract; revisit if a mainstream connector turns out legacy-only.
- `GET`/`DELETE` answer 405 (stateless: no streams to resume, no
  sessions to close) — the SDK handler's built-in behavior.

## 4. Auth: the hub as OAuth resource server

The governing rules (MCP Security Best Practices + 2026-07-28 auth):

1. **No token passthrough.** The MCP endpoint MUST NOT accept raw IdP
   tokens (Google ID tokens) — tokens it accepts must be issued *to the
   MCP endpoint* (audience = the endpoint's canonical URL).
2. Today's stdio server sends the user's Google ID token as the sync
   Bearer. That is defensible for a user-spawned local process speaking
   to one hub; it is exactly the confused-deputy shape the remote
   endpoint must not normalize.

### Decision D-1: hub-minted access tokens

The hub already runs an OIDC callback (`/auth/callback`) and mints
session state for the SPA; CAP-16 extends that role: after any sign-in
flow completes, the hub issues its **own short-lived access token**
(JWT signed by a hub key; audience = `https://<hub>/mcp`; subject = the
hub user; expiry ~1 h) plus a refresh grant. The MCP endpoint validates
only hub-minted tokens. This is the same direction the auth-unification
epic (bd-qxgoti2b, pattern (ii) hub-side exchange/BFF) took for the SPA —
**coordinate, don't duplicate**: the token-minting and JWKS surface
belongs to that epic's hub auth service; CAP-16 consumes it. The
alternative (RFC 8707 resource indicators against Google as AS) is a
dead end: Google's audience model binds `aud` to client IDs, not
arbitrary resource URLs.

The stdio server's token (`q2 mcp` / npm package) migrates to the same
hub-minted shape when pointed at a CAP-16 hub — one token model across
transports. `bd-ra5ypj3s` (the Google-consent 400) is resolved in the
same console work.

### Discovery and negotiation (RFC 9728 + SEP-985)

- Unauthenticated requests → `401` with
  `WWW-Authenticate: Bearer resource_metadata="https://<hub>/.well-known/oauth-protected-resource/mcp"`.
- The hub serves Protected Resource Metadata at that `.well-known` URL
  (and at the bare `/.well-known/oauth-protected-resource` fallback per
  SEP-985): `resource` = the canonical endpoint URL,
  `authorization_servers` = the hub's own issuer URL (it is the AS for
  these tokens), `scopes_supported` (§5), `bearer_methods_supported:
  ["header"]`.
- The hub's issuer metadata (`/.well-known/oauth-authorization-server`)
  advertises authorization/token endpoints, PKCE methods, and **CIMD**
  support (SEP-991). **DCR is deprecated — do not implement it.**
  Browser-hosted connectors register out-of-band or via CIMD (the
  client hosts a metadata document; the hub fetches it — see §6 SSRF).
- The 2026-07-28 **Auth extension** (client-credentials / enterprise
  JWT flows) is noted as compatible with this shape but not built in the
  first implementation; the conformance suite marks its scenarios
  `extension`/not-scored.

### Consent (confused-deputy)

First time a *client* (a connector, identified by `client_id`) asks a
*user* for hub access, the hub shows its own consent screen naming the
client, the requested scopes, and the consequence (agent can read/write
the user's hub projects). Consent is recorded per (user, client,
scopes); step-up re-consents on scope widening (§5). This is the hub's
screen, not the IdP's — the IdP consent the user already gave covers
sign-in, not MCP delegation.

## 5. Scopes and step-up (SEP-835)

Minimal, capability-shaped scopes, enforced at the endpoint and
surfaced in `WWW-Authenticate` challenges:

| Scope | Gates |
|-------|-------|
| `mcp:read` | read tools, resources/read, prompts, list_* |
| `mcp:write` | create/patch/write/delete/rename/restore tools |
| `mcp:listen` | `subscriptions/listen` streams, wait_for_change |

- `401` challenges carry `scope` hints; `403` on authenticated-but-
  insufficient tokens carries `error="insufficient_scope"` + the
  required scope so the client can step up.
- Project-level authorization rides the hub's existing ACL: the token's
  subject must have project access, exactly as sync connections enforce
  today. Per-project OAuth scopes are explicitly rejected (scope
  explosion; ACL is the right layer).

## 6. Security Best Practices, applied

- **Token passthrough:** prohibited by D-1; the endpoint rejects any
  token whose `aud` is not the endpoint URL — in particular Google
  `aud=<client-id>` tokens get a clear 401, not acceptance.
- **Origin validation → 403:** the endpoint validates `Origin` (when
  present) against the hub's own origins and rejects cross-origin
  browser posts (DNS-rebinding class). The loopback already implements
  this with the SDK helpers; the hub mount does the same, with the
  connector user-agents' no-Origin behavior noted (non-browser clients
  send no Origin — allowed).
- **SSRF on metadata URLs:** the hub fetches client metadata (CIMD) and
  its own AS/PRM documents only over https, rejecting private/loopback
  ranges and link-local hosts after DNS resolution — the same rule set
  as hub-mcp's BP-18 `assertSafeAuthorizationEndpoint`, implemented
  server-side. Redirects are followed with the same checks per hop.
- **Rate limiting / fair use:** per-token request-rate buckets on
  `/mcp`, tighter on `subscriptions/listen` concurrency (§7); 429 with
  `Retry-After`.
- **Stdio parity:** the `render` tool (arbitrary code execution) is
  **not offered** on the hub-served endpoint in the first
  implementation (§8).

## 7. Connection lifecycle — Q-5 resolved

The problem (Q-5): stateless serving constructs a fresh MCP server per
request, but `ConnectionManager` holds per-project automerge websocket
state that must outlive any single request — a `connect_project` in
request *n* must still be connected in request *n+1*, and we cannot pay
a fresh automerge handshake per request.

**Resolution: a per-user `ConnectionManager` pool on the hub.**

- **Key:** the token subject (hub user id). One `ConnectionManager` per
  active user, holding that user's per-project sync connections (the
  manager's existing `(serverUrl, indexDocId)` keying is unchanged).
- **Creation:** lazily on the user's first request; the per-request
  `McpServer` is constructed around the pool entry (exactly the
  loopback's factory shape).
- **Warm-keep and eviction:** entries idle-evict after **15 minutes**
  without a request *or an open listen stream* (listen streams touch
  the entry on every notification, so an agent parked on
  `wait_for_change`/subscriptions keeps its pool entry). Eviction calls
  `disconnectAll` (bounded drain, same as stdio shutdown).
- **Sizing (the Q-5 numbers):** pool cap **500 entries** with LRU
  eviction; per-entry cap **20 project connections** (matching the
  manager's existing per-process reality — an agent working set is
  1–3 projects). Memory per entry is dominated by synced documents
  (~1–10 MB for text projects); 500 hot entries ≈ 1–5 GB worst case —
  inside the hub's single-node envelope, and LRU eviction keeps the
  *working* set far below the cap. Automerge websocket fan-out: at most
  500 × 20 = 10k theoretical, realistically hundreds — same order as
  the browser clients the hub already serves; if the two pools (browser
  sync + MCP) must share a global budget, the MCP pool takes a
  configurable fraction.
- **Cold-start:** first request after eviction pays one sync handshake
  per reopened project (~100–300 ms measured on the stdio path) —
  acceptable; subsequent requests ride the warm entry.
- **Fairness:** per-user concurrency cap (8 in-flight requests) with
  429 + `Retry-After` beyond; listen streams capped at 4 per user.
  These bind the "one automerge connection per user × N users" risk
  row: an abusive user can hold at most 20 sync connections and 4
  streams, and eviction reclaims them.

## 8. The tool surface over HTTP

Identical to stdio (the handlers are transport-agnostic by design),
with two deliberate exceptions in the first implementation:

- **`render` is not registered.** Rendering shells out to `q2` on the
  host — fine on a user's machine, a compute-amplification and
  sandboxing problem on the hub. Remote render is its own design
  (sandboxed workers, quotas) and its own strand.
- **`authenticate` / `authenticate_clear` are not registered.** Auth is
  the connector↔hub OAuth flow; a tool that opens a browser makes no
  sense server-side. Credential state errors surface as 401 challenges
  instead.

Everything else — including resources and `subscriptions/listen` —
serves unchanged. `wait_for_change` long-polls ride the request's SSE
upgrade (per-request responseMode 'auto'), no special casing.

## 9. CAP-3 full discovery — does the design need it?

Yes, minimally. Success criterion §9.1 ("enumerate the caller's project
collections without a collection id") has no server-side answer today:
`list_projects` needs a collection share URL. For a remote agent that
starts from nothing but OAuth, the hub must answer "my projects". The
implementation plan should include a **hub-side per-user project
registry** (auth-keyed index over the projects the user owns or was
shared into — the hub already tracks this for the SPA's project list),
exposed as a zero-argument form of `list_projects`. Small, additive,
and unblocked by this design. (MVP shipped in Phase 2; this is the
full form.)

## 10. Decisions requested before implementation

| # | Decision | Recommendation |
|---|----------|----------------|
| D-1 | Token model | Hub-minted JWT access tokens, audience = endpoint URL (§4) |
| D-2 | Legacy policy | `reject` — modern-only endpoint (§3) |
| D-3 | Scope granularity | Capability scopes only; project authz stays in the hub ACL (§5) |
| D-4 | Pool parameters | 500-entry LRU, 20 projects/user, 15 min idle TTL, 8 req/user (§7) — to be validated against production traffic before launch |
| D-5 | Deployment | In-hub mount (`/mcp` next to `/ws`), not a separate gateway service — one auth surface, one deployment |
| D-6 | `render` + `authenticate` | Omitted from the HTTP surface in v1 (§8) |

## 11. Risks

| Risk | Mitigation |
|------|-----------|
| Scope creep into the auth-unification epic (bd-qxgoti2b) | CAP-16 consumes that epic's token mint; the implementation plan sequences after its B1 (hub-side exchange) lands |
| Listen-stream eviction kills a live agent watch | Streams touch the pool entry (§7); alert on eviction-of-active-entry in metrics |
| Connector client-registration churn (CIMD is young) | Support CIMD + out-of-band registration; log and revisit when Claude.ai/ChatGPT document their connector registration |
| Pool memory blowup from many small users | LRU cap + per-entry project cap + per-user concurrency caps (§7) |

## 12. Explicitly out of scope

- 2025-era sessions / SSE resumability (§3)
- Remote `render` (§8) — separate design
- The Auth extension's enterprise flows (§4) — noted for later
- MCP Apps in-host preview — see the feasibility note
  (`claude-notes/research/2026-10-07-mcp-apps-feasibility.md`)

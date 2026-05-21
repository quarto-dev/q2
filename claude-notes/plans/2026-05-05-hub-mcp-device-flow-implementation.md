# 2026-05-05 — Hub MCP auth: Design C′ (Google device flow) implementation

## Overview

Implements **Design C′ (Google as device-flow AS)** to let
`ts-packages/quarto-hub-mcp` authenticate to `crates/quarto-hub` over
WebSocket. Uses Google's OAuth 2.0 device-authorization endpoint
(RFC 8628); hub-mcp persists Google ID + refresh tokens locally. The
hub keeps its existing JWKS validator — no new credential type, no new
persistence, no issuance UI.

Design context: `claude-notes/plans/2026-04-27-hub-mcp-auth-design.md`.

Work spans:

- `crates/quarto-hub` — accept a Bearer Google ID token; allow a list
  of audiences (SPA's client_id + hub-mcp's client_id); dual-credential
  400 rule; audit log distinguishes credential identity from user identity.
- `ts-packages/quarto-hub-mcp` — device-flow primitives, MCP-tool
  exposure (`authenticate_start` / `authenticate_finish`),
  refresh-on-401, OS-keyring credential store, redact-everywhere logging.
- `ts-packages/quarto-sync-client` — header pass-through on the WS
  upgrade via a Node-only adapter.
- Per-operator admin step: register a second Google OAuth client of
  type "TV and Limited Input devices"; publish client_id +
  client_secret to end users (consumed by hub-mcp via
  `QUARTO_HUB_MCP_CLIENT_ID` / `QUARTO_HUB_MCP_CLIENT_SECRET`).

### Out of scope / deferred

- Hub-side `tokens` table, credential-issuance API, SPA token-mgmt UI.
- Browser subprotocol-auth fallback (hub-mcp is Node-only; SPA uses cookie).
- Per-request transport-gate middleware, per-IP rate limiter,
  schema-locked audit module + `jwt_jti_or_hash` field — deferred to
  a follow-up plan covering both Bearer and cookie surfaces symmetrically.
- Hub-side `sub_denylist` (the only mechanism that closes the ≤1 h
  ID-token residual-validity window) — deferred; requires the hub to
  gain persistent storage it doesn't have today.
- Hub `/auth/info` auto-discovery for hub-mcp credentials — deferred;
  v1 requires manual paste of operator-supplied env vars.

## Threat model

In priority order:

1. **Refresh-token theft from local disk.** Mitigated by OS-native
   keyring storage (`@napi-rs/keyring`): DPAPI on Windows, Keychain on
   macOS, Secret Service on Linux. Bound to current user account on
   each platform. No plaintext file on disk.
2. **MCP-config commit / screenshot leak.** No long secret in
   `.mcp.json`; credentials live in OS keyring.
3. **Auth-confusion via cookie + Authorization.** Reject with 400.
4. **Audience confusion / stolen Google ID token from another client.**
   Strict audience allowlist; no wildcards.
5. **Verification-URI phishing.** Hard-coded canonical URL
   (`https://www.google.com/device`) in tool response alongside
   Google's `verification_uri`; user told to compare. Canonical URL
   is a constant, not a value from Google's response.
6. **Refresh-token replay after revocation.** Empirically confirmed
   2026-05-19: Google does **not** rotate refresh tokens for
   Limited-Input-Devices clients. Stolen ID tokens authenticate for
   up to ≤1 h regardless of revocation (JWTs are self-contained).
   v1 mitigation: at-rest protection + audit visibility + documented
   revocation runbook. Closing the ≤1 h window requires hub-side
   `sub_denylist` — deferred.
7. **PII in credential** (Google ID token carries `email`, `name`,
   `picture`). Unavoidable with Google as AS; documented.
8. **Polling-endpoint DoS** — N/A; polling targets Google, not hub.
9. **Lateral movement after compromise.** Per-scope enforcement
   deferred.
10. **OAuth `client_secret` leakage from an operator deployment.**
    Empirically (2026-05-19) Google requires `client_secret` for this
    client type. Mitigated by env-var-only sourcing (no plaintext file,
    no `.mcp.json` value, no baked-in default); operator handles via
    normal secret-management. Structural defence in depth: the
    `device_code` is the per-flow authentication binding — a leaked
    `client_secret` alone cannot redeem any user's approval.

### Revocation

User revokes at myaccount.google.com → "Third-party apps with account
access". Hub does not surface a "your sessions" list.

## Cross-cutting security requirements

These apply to every phase:

- **Strict cookie-vs-Authorization precedence.** Both presented → 400
  Bad Request, body `{"error":"conflicting_credentials"}`.
- **Strict audience allowlist.** Configured list (SPA client_id +
  hub-mcp client_id). No wildcards. No issuer-only mode.
- **Email/domain allowlist parity across credential kinds.** Bearer
  and cookie paths both gated by the shared `check_allowlists()` call
  from `authenticate_claims()` (`auth.rs:131-165`, `context.rs:362-386`).
  401-vs-403 distinction preserved on Bearer path. Hub-mcp client_id
  being an allowlisted audience does NOT bypass user-identity allowlist.
- **TLS at the application layer.**
  - Hub: existing startup-time `validate_tls_config` (`auth.rs:396-409`).
  - hub-mcp: refuses to send Bearer over plain HTTP/WS to non-loopback
    without `QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1`. Loopback always
    permitted. Loud warning on every insecure connect when the env var
    is set.
- **Audit log distinguishes credential identity from user identity.**
  Every authenticated event records `(sub, credential_kind, action,
  outcome)` where `credential_kind ∈ {cookie, bearer}`. v1 emits via
  inline `tracing::event!`; schema-lock + dedicated module deferred.
- **Token never logged.** `Authorization` header redacted in
  `tower-http` `TraceLayer`; hub-mcp redacts via centralised utility
  on every log call site.
- **Bearer is the only non-cookie auth path.** `Basic`, custom schemes,
  query-param tokens rejected with 401.
- **Refresh tokens never appear in logs/errors.** hub-mcp installs
  `uncaughtException` / `unhandledRejection` handlers that scrub
  Google-token-shaped substrings (`ya29.*`, `1//*`, JWT-shaped)
  before logging.
- **hub-mcp follows the hub's auth policy, not its own.** Try-without-
  creds-first against unknown hubs; only trigger device flow when the
  hub returns 401. Lets hub-mcp work against `auth_config: None` hubs
  without forcing device flow.

## Phase 1 — Design lock-in

Empirical verification (2026-05-19, see Verification log) answered
discovery questions. The following are **immutable** for v1:

- **hub-mcp client-authentication wire:** `oauth.ClientSecretPost(secret)`
  (Option B). Google requires `client_secret` for the
  Limited-Input-Devices client type at `/token`.
- **Credential sourcing for hub-mcp:** operator-supplied via env vars,
  symmetric with hub-client.
  - `QUARTO_HUB_MCP_CLIENT_ID` — operator's Google OAuth client_id.
  - `QUARTO_HUB_MCP_CLIENT_SECRET` — operator's matching secret.
  - Read **only from `process.env`**, never from `.mcp.json`,
    keyring, or source literals.
  - **Both mandatory** when the device flow may run. Fail loud with
    typed `MissingCredentialsConfigError` naming both vars literally.
  - Rationale: symmetry with SPA's existing `VITE_GOOGLE_CLIENT_ID` /
    `OIDC_CLIENT_ID` model; operator sovereignty (consent screen,
    quota, audit, revocation key off operator's project); no
    Quarto-team-owned default.
  - Each end-user pastes both values into the env block of their
    `.mcp.json` alongside `--server <URL>`.
- **Hub validator audience policy:** allowlist of N strings; token
  accepted iff `aud` ∈ allowlist. Built at startup as
  `iter::once(&config.client_id).chain(config.additional_audiences.iter())`.
  Per OIDC Core §3.1.3.7:
  - if `aud.len() > 1`: require `azp` present and in allowlist;
  - if `azp` present (any `aud` shape): require `azp` in allowlist
    — this is the live rule for every real Google token today;
  - if `aud` single-valued and `azp` absent: accept on `aud` alone.

  `jsonwebtoken`'s `set_audience` only enforces `aud`; `azp` is a
  custom post-decode check.
- **Hub validator issuer/algorithm policy:** unchanged. Single issuer
  (`https://accounts.google.com`); algorithms from JWKS.
- **Bearer extraction:** the value is a Google ID token (a JWT);
  shares the cookie's JWT validator branch.
- **Middleware order:** extract credential **before** CSRF / WS-Origin.
  Bearer-authenticated requests skip both checks; cookie-authenticated
  requests still enforce them. Dual-credential 400 wins.
- **Audit log fields (v1):** `sub`, `credential_kind`, `action`,
  `outcome`, plus `detail` on failure. Inline `tracing::event!`; no
  dedicated module, no locked schema. Schema-lock + `jwt_jti_or_hash`
  + OpenTelemetry naming + `audit-log.md` doc deferred.
- **hub-mcp credential storage:** OS-native keyring on every platform
  via `@napi-rs/keyring`. No plaintext file on disk on any platform.
- **hub-mcp credential blob shape** (single opaque JSON in keyring):
  ```json
  {
    "schema_version": 1,
    "issuer": "https://accounts.google.com",
    "client_id": "<hub-mcp-client-id>",
    "id_token": "<jwt>",
    "refresh_token": "<opaque>",
    "id_token_expires_at": "<iso8601>",
    "scopes": ["openid", "email", "profile"]
  }
  ```
- **In-memory bundle shape:**
  ```ts
  type CredentialBundle = {
    idToken: string;
    refreshToken: string;
    idTokenExpiresAt: Date;
    scopes: readonly string[];
  };
  ```
- **Per-platform service / account identifiers** (uniform
  `Entry(service, account)`):
  - macOS: service `dev.quarto.hub-mcp`, account `<issuer>:<client_id>`.
  - Linux: schema `dev.quarto.hub-mcp`, account attribute
    `<issuer>:<client_id>` in default collection.
  - Windows: target name `dev.quarto.hub-mcp:<issuer>:<client_id>`.
- **Headless-Linux:** Secret Service unreachable → typed
  `KeyringUnavailableError` on `write`. No silent plaintext fallback.
- **Re-auth trigger:** second consecutive 401 with a freshly-refreshed
  ID token, OR `invalid_grant` from Google's `/token`. Re-auth = full
  device-flow restart via `authenticate_start`.
- **MCP auth tool surface:**
  - `authenticate_start({}) -> { verification_uri, canonical_url,
    user_code, expires_in_seconds }` — initiates device flow. Short
    circuits to text when (a) valid creds already cached, or
    (b) connection-manager observed `lastObservedAuthMode === 'no-auth'`.
  - `authenticate_finish({}) -> string` — **one** poll against
    Google's `/token`. Returns `pending`/`slow_down` text on those
    responses; persists bundle + returns "Authenticated as <email>"
    on success; typed error on terminal failure.

  Single-poll-per-call is deliberate: MCP tool calls have client
  timeouts; blocking on a user-driven flow is fragile.
- **`device_code` is process-local**, never persisted. Cached state
  carries `nextPollAllowedAt` for RFC 8628 §3.5 rate-limiting,
  initialised to `start_time + interval`, bumped by 5 s per `slow_down`.
  Second `authenticate_start` within ~5 s returns the cached
  device_code; outside that window overwrites.
- **First-run trigger.** When connect attempt with no creds hits the
  hub's 401, the typed error names `authenticate_start`.

## Phase 2 — Hub middleware integration (TDD)

Touch points: `crates/quarto-hub/src/auth.rs:361` (audience config),
`crates/quarto-hub/src/server.rs:144-160` (cookie extraction),
`:262-285` (`Authenticated` extractor), `:399,:617,:644` (CSRF), `:691`
(WS-Origin).

### Tests first

Tests in `crates/quarto-hub/tests/auth_bearer.rs`. Axum test server
with new audience allowlist; test JWKS via a `MockOidcProvider` helper.

- [x] `bearer_with_spa_audience_authenticates`
- [x] `bearer_with_mcp_audience_authenticates`
- [x] `bearer_with_unknown_audience_returns_401`
- [x] `bearer_with_no_audience_returns_401` — via
  `validation.set_required_spec_claims(&["exp", "aud"])`. Without
  that, `jsonwebtoken@10`'s default `validate_aud=true` is silently
  skipped for no-aud tokens (`validation.rs:325-350`).
- [x] `bearer_with_aud_array_and_matching_azp_authenticates`
- [x] `bearer_with_aud_array_and_missing_azp_returns_401` — OIDC
  §3.1.3.7 conformance.
- [x] `bearer_with_aud_array_and_mismatched_azp_returns_401` —
  confused-deputy prevention.
- [x] `bearer_with_single_aud_and_present_azp_validates_azp` — `azp`
  validated whenever present, not only when `aud` is array.
- [x] `bearer_with_single_aud_and_absent_azp_authenticates` —
  regression on common Google case.
- [x] `bearer_with_wrong_issuer_returns_401`
- [x] `bearer_with_expired_token_returns_401`
- [x] `bearer_with_future_iat_returns_401` — `iat > now + leeway`,
  `nbf` absent (so rejection is unambiguously the `iat` check).
- [x] `bearer_with_future_iat_within_skew_authenticates` — `iat =
  now + 30 s` with default 60 s leeway → 200.
- [x] `bearer_with_invalid_signature_returns_401`
- [x] `bearer_with_unverified_email_returns_401` — Bearer path runs
  the existing `email_verified` gate.
- [x] `bearer_with_unallowlisted_email_returns_403` — allowlist
  parity; 403-vs-401 distinction load-bearing.
- [x] `bearer_with_allowed_domain_authenticates`
- [x] `bearer_with_mcp_audience_but_unverified_email_returns_401` —
  confused-deputy test on user-identity check.
- [x] `ws_upgrade_with_bearer_outside_allowlist_returns_403`
- [x] `cookie_still_authenticates` — regression.
- [x] `cookie_and_bearer_returns_400` — body
  `{"error":"conflicting_credentials"}`. **CVE-prevention test.**
- [x] `bearer_wrong_scheme_returns_401` — `Basic` / `Token` → 401,
  never 400.
- [x] `audit_event_on_auth_ok` — `action="auth_ok"`,
  `credential_kind="bearer"`, `outcome="allow"`, `sub=<expected>`.
- [x] `audit_event_on_auth_fail` — three failure shapes, each with
  distinct status code:
  - bad credentials → 401, `detail` carries JWT validation error;
  - good creds, not allowlisted → 403,
    `detail = "user_not_allowlisted"`;
  - dual credentials → 400, `detail = "conflicting_credentials"`.
- [x] `tracing_redacts_authorization_header`
- [x] `ws_upgrade_with_bearer_works`
- [x] `ws_upgrade_rejects_dual_credentials`
- [x] `ws_upgrade_with_bearer_skips_origin_check` — Bearer + no
  `Origin` (or cross-origin) → 101. **CVE-prevention test.**
- [x] `ws_upgrade_with_cookie_still_requires_origin` — regression.
- [x] `mutating_endpoint_with_bearer_skips_csrf_check`
- [x] `mutating_endpoint_with_cookie_still_requires_csrf` — regression.
- [x] `dual_credential_400_wins_over_csrf_and_origin`
- [x] `unauthenticated_endpoint_unaffected` — regression.

### Implementation

- Extend `AuthConfig` with `additional_audiences: Vec<String>`
  (defaults empty). `client_id` stays primary; hub-mcp client_id lands
  in `additional_audiences`.
- **Validation pipeline** — library checks then custom post-decode.

  Step 1 — `jsonwebtoken@10` (at `auth.rs:366-369`):
  ```rust
  let mut validation = Validation::default();
  validation.algorithms = algorithms;
  validation.set_issuer(&[&config.issuer]);
  validation.validate_nbf = true;

  let allowed: Vec<&String> = std::iter::once(&config.client_id)
      .chain(config.additional_audiences.iter())
      .collect();
  validation.set_audience(&allowed);

  // Without this, no-aud tokens silently bypass the audience check.
  validation.set_required_spec_claims(&["exp", "aud"]);
  ```

  Step 2 — post-decode helper (called from the same path as
  `check_allowlists` at `auth.rs:131-153`):
  ```rust
  // jsonwebtoken does not validate iat against a future bound.
  let leeway = i64::try_from(validation.leeway).unwrap_or(60);
  let now = chrono::Utc::now().timestamp();
  if let Some(iat) = claims.iat {
      if iat > now + leeway {
          return Err(StatusCode::UNAUTHORIZED);
      }
  }

  // OIDC §3.1.3.7 azp rule.
  let aud_is_multi = claims.aud.len() > 1;
  match (claims.azp.as_deref(), aud_is_multi) {
      (None, true) => return Err(StatusCode::UNAUTHORIZED),
      (Some(azp), _) if !allowed.iter().any(|a| a.as_str() == azp)
          => return Err(StatusCode::UNAUTHORIZED),
      _ => {}
  }
  ```

- **`OidcClaims` struct migration** (`auth.rs:109-116`). Add three fields:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct OidcClaims {
      pub sub: String,
      pub email: String,
      #[serde(default)]
      pub email_verified: bool,
      pub name: Option<String>,
      pub picture: Option<String>,

      #[serde(deserialize_with = "deserialize_aud", default)]
      pub aud: Vec<String>,
      #[serde(default)]
      pub azp: Option<String>,
      #[serde(default)]
      pub iat: Option<i64>,
  }

  fn deserialize_aud<'de, D>(d: D) -> Result<Vec<String>, D::Error>
  where D: serde::Deserializer<'de>,
  {
      #[derive(serde::Deserialize)]
      #[serde(untagged)]
      enum AudClaim { Single(String), Multi(Vec<String>) }
      match AudClaim::deserialize(d)? {
          AudClaim::Single(s) => Ok(vec![s]),
          AudClaim::Multi(v)  => Ok(v),
      }
  }
  ```
  Migrate fixtures `make_claims` (`auth.rs:468-471`) and the three
  `serde_json::from_str` tests (`:616-658`) with defaults.

- **New extractor** `extract_credential(headers) -> Result<Credential,
  StatusCode>` returning `Cookie(jwt)`, `Bearer(jwt)`, or
  `Err(BadRequest)` for dual.
- **Routing invariant:** both branches feed
  `HubContext::authenticate_claims()` (the only decode site,
  unconditionally calls `check_allowlists`). No per-credential-kind
  bypass.
- `Authenticated` struct gains `credential_kind: CredentialKind`.
- **CSRF / WS-Origin gating respects `credential_kind`.** Rework
  state-mutating handlers (`server.rs:399,:617,:644`) and the WS
  handler (`:691`):
  ```rust
  if matches!(auth.credential_kind, CredentialKind::Cookie) {
      check_csrf(&headers)?;   // or check_ws_origin
  }
  ```
- Audit emission via inline `tracing::event!` on success and failure.
- `tower-http` `TraceLayer.on_request(...)` redacts `Authorization`
  and `Cookie` from spans.

### Phase 2 — completion notes (2026-05-20)

Landed on `feature/hub-mcp-device-flow`:

- `AuthConfig` now carries `additional_audiences: Vec<String>` with
  an `audiences()` iterator that lists primary `client_id` first,
  then additional. `set_audience` + `set_required_spec_claims(&["exp",
  "aud"])` plumbed in [`build_auth_state_from_parts`] —
  `build_auth_state` retains its discovery wrapper and now delegates
  to it. The test-only `build_auth_state_from_parts` skips OIDC
  discovery so integration tests can drive a mock OIDC provider on
  `http://localhost`; `build_router_with_state` honours an
  externally-injected `AuthState` via the new
  `HubContext::auth_state_initialized()` check.
- `OidcClaims` gained `aud: Vec<String>` (with `deserialize_aud`
  accepting either a JSON string or array), `azp: Option<String>`,
  and `iat: Option<i64>`. `validate_azp_and_iat` implements the OIDC
  §3.1.3.7 azp rule plus the future-`iat` rejection at the call site
  inside `HubContext::authenticate_claims`.
- `CredentialKind`, `Credential`, `extract_credential`, and the
  conflicting-credentials 400 body are public in `server.rs`. The
  `Authenticated` extractor now carries the `credential_kind` so
  CSRF / WS-Origin checks gate on it. Mutating handlers
  (`update_document`, `auth_logout`, `auth_refresh`) skip CSRF for
  Bearer; the WS handler skips Origin for Bearer; the dual-credential
  400 wins over both. The status code from `authenticate_claims_for_kind`
  is preserved through `Authenticated` so 403 (user not allowlisted)
  is no longer collapsed to 401 — the regression that drove the
  allowlist-parity tests.
- `HubContext::authenticate_claims_for_kind` emits inline
  `tracing::event!` audit lines (`target = "quarto_hub::audit"`,
  `action`, `outcome`, `credential_kind`, `sub`, optional `detail`)
  on every allow/deny decision. The existing `RedactedMakeSpan` keeps
  `Authorization` / `Cookie` out of span fields — `tracing_redacts_authorization_header`
  scans every captured event field for the raw token and `Bearer ` to
  enforce that.
- `--additional-audiences` / `QUARTO_HUB_ADDITIONAL_AUDIENCES` plumbed
  through both `crates/quarto-hub/src/main.rs` (standalone `hub` bin)
  and `crates/quarto/src/main.rs` → `commands/hub.rs` (the
  `quarto hub` subcommand).

Verification: 33 new tests in
`crates/quarto-hub/tests/auth_bearer.rs` (Bearer aud/azp/iat,
allowlist parity, dual-credential 400, CSRF / WS-Origin gating,
audit-event capture, redaction); 7 new lib tests under
`crates/quarto-hub/src/auth.rs` (claims deserialization, audience
iteration, azp/iat helper). Full workspace `cargo nextest run`
(9248 tests) and `cargo xtask verify --skip-hub-build` clean.

## Phase 3 — Audit logging (minimal v1)

Gate is Phase 2's test set (`audit_event_on_auth_ok`,
`audit_event_on_auth_fail`, `tracing_redacts_authorization_header`).
Implementation emits inline `tracing::event!` with `sub`,
`credential_kind`, `action`, `outcome`, plus `detail` on failure.

Deferred to a follow-up plan that wires log aggregation: dedicated
`audit.rs` module, schema-lock tests, SHA-256 `jti`-or-hash
correlation field, `target: "quarto_hub::audit"` stable name,
OpenTelemetry semantic-conventions naming,
`claude-notes/instructions/audit-log.md` doc.

### Phase 3 — completion notes (2026-05-20)

Subsumed by Phase 2. The Phase 2 implementation already routes every
allow/deny decision through `HubContext::authenticate_claims_for_kind`
(`crates/quarto-hub/src/context.rs:440-545`) and the Bearer-credential
400 path (`crates/quarto-hub/src/server.rs:253-260`), each emitting
`tracing::event!(target: "quarto_hub::audit", action, outcome,
credential_kind, sub, detail?)`. The three gate tests
(`audit_event_on_auth_ok`, `audit_event_on_auth_fail`,
`tracing_redacts_authorization_header`) pass under
`cargo nextest run -p quarto-hub`. No additional Phase 3 changes
landed — the module/schema-lock work remains deferred.

## Phase 4 — hub-mcp device-flow primitives (TDD)

New module: `ts-packages/quarto-hub-mcp/src/auth/device-flow.ts`.
Sits on `oauth4webapi` + `jose`. Two primitives — `initiateDeviceFlow`
and `pollDeviceFlowOnce` — not a single blocking flow.

### Tests first

Tests in `ts-packages/quarto-hub-mcp/test/auth/device-flow.test.ts`.
Fixture HTTP server via msw or undici `MockAgent`; never live Google.

- [x] `initiate_request_has_correct_params` — POST to device-auth
  endpoint includes `client_id`, `scope=openid email profile`.
- [x] `client_id_and_secret_sourced_from_env` — values come from
  `process.env.QUARTO_HUB_MCP_CLIENT_ID` / `_CLIENT_SECRET`, not from
  literals / files / keyring.
- [x] `startup_fails_loud_when_env_missing` — typed
  `MissingCredentialsConfigError` naming both vars literally.
- [x] `no_baked_default_client_id_or_secret` — grep over `src/`
  asserts no `*.apps.googleusercontent.com` or `GOCSPX-…` literal.
- [x] `client_secret_sent_on_token_endpoint_only` — device-auth body
  has no `client_secret`; token-endpoint body does.
- [x] `initiate_returns_full_device_response` — pass-through of
  `verification_uri`, `user_code`, `device_code`, `interval`,
  `expires_in`.
- [x] `poll_once_returns_pending_on_authorization_pending` —
  `{ kind: 'pending' }`, never throws.
- [x] `poll_once_returns_slow_down_on_slow_down`.
- [x] `poll_once_resolves_with_tokens_on_success`.
- [x] `poll_once_surfaces_access_denied_as_typed_error` →
  `DeviceFlowDeniedError`.
- [x] `poll_once_surfaces_expired_token_as_typed_error` →
  `DeviceFlowExpiredError`.
- [x] `poll_once_honours_abort_signal`.
- [x] `does_not_log_user_code_in_debug_lines`.
- [x] `does_not_log_id_token_or_refresh_token_anywhere`.
- [x] `redact_util_handles_known_token_shapes` — `ya29.*`, `1//*`,
  JWT `xxx.yyy.zzz`.

### Phase 4 — completion notes (2026-05-20)

Landed on `feature/hub-mcp-device-flow`:

- New module `ts-packages/quarto-hub-mcp/src/auth/device-flow.ts`
  exposes: `initiateDeviceFlow`, `pollDeviceFlowOnce`,
  `loadDeviceFlowConfigFromEnv`, `discoverAuthorizationServer`,
  `buildAuthorizationServer`, plus typed errors
  (`MissingCredentialsConfigError`, `DeviceFlowError`,
  `DeviceFlowDeniedError`, `DeviceFlowExpiredError`) and the
  `redactTokens` utility.
- `initiateDeviceFlow` uses `oauth4webapi.None()` (not
  `ClientSecretPost`) for the device-authorization request — the
  device-auth endpoint doesn't require client authentication and
  withholding the secret here minimises its exposure surface. The
  token-endpoint poll uses `ClientSecretPost` per the Phase-1
  lock-in. The test `pollDeviceFlowOnce > resolves with tokens on
  success` asserts both halves of this contract.
- Single-poll-per-call: `pollDeviceFlowOnce` performs **one** request
  and returns a discriminated `PollResult` (`pending` / `slow_down` /
  `tokens`) without retrying. Higher-level cadence + RFC 8628 §3.5
  rate-limiting belongs in Phase 7.
- `redactTokens` strips `ya29.*`, `1//*`, and JWT-shaped substrings —
  every log call site in this module funnels through it. The
  `does_not_log_*` tests spy on every `console.*` sink to enforce it.
- `loadDeviceFlowConfigFromEnv` reads only `process.env`, never
  `.mcp.json` / files / keyring / source literals. The
  `no_baked_default_client_id_or_secret` test walks `src/` and rejects
  any `*.apps.googleusercontent.com` or `GOCSPX-` literal.
- Discovery cache is process-local (`cachedAS`), keyed on issuer; a
  `_resetDiscoveryCache()` hook is exported for Phase-5+ tests.

Tests live at `src/auth/device-flow.test.ts` (not `test/auth/…` as
the plan path suggested) to match the existing co-located
`src/hub-mcp.test.ts` convention and so a single `tsconfig.json` /
`vitest` config covers both. Behaviour is unchanged.

Verification: 23 new Vitest specs pass under `npm test` from
`ts-packages/quarto-hub-mcp/`; `npm run typecheck` clean. Pre-
existing `live:` tests in `src/hub-mcp.test.ts` continue to fail
identically before and after this change (they require an
`indexedDB` polyfill the test env doesn't carry) — confirmed by
stash + re-run.

Dependencies added to `ts-packages/quarto-hub-mcp/package.json`:
`oauth4webapi ^3.5.5`, `jose ^6.0.11`, and dev-dep `undici ^7.16.0`
(reserved for Phase-6+ refresh tests). `npm install` from repo root
hoisted them; root `package-lock.json` is the change record.

### Implementation

- Cache `AuthorizationServer` via
  `oauth4webapi.discoveryRequest(new URL(issuer))` +
  `processDiscoveryResponse(...)`.
- Client + auth:
  ```ts
  import * as oauth from 'oauth4webapi';

  function requireEnv(name: string): string {
    const v = process.env[name];
    if (!v || v.trim() === '') {
      throw new MissingCredentialsConfigError(
        `${name} is not set. Hub-mcp requires QUARTO_HUB_MCP_CLIENT_ID ` +
        `and QUARTO_HUB_MCP_CLIENT_SECRET in the MCP-client env. ` +
        `Ask your hub operator for the Google OAuth client credentials ` +
        `they registered for hub-mcp.`,
      );
    }
    return v;
  }

  const clientId     = requireEnv('QUARTO_HUB_MCP_CLIENT_ID');
  const clientSecret = requireEnv('QUARTO_HUB_MCP_CLIENT_SECRET');

  const client: oauth.Client = { client_id: clientId };
  const clientAuth: oauth.ClientAuth = oauth.ClientSecretPost(clientSecret);
  ```
- `initiateDeviceFlow`:
  ```ts
  const params = new URLSearchParams({ scope: 'openid email profile' });
  const resp = await oauth.deviceAuthorizationRequest(as, client, clientAuth, params);
  return await oauth.processDeviceAuthorizationResponse(as, client, resp);
  ```
- `pollDeviceFlowOnce`:
  ```ts
  type PollResult =
    | { kind: 'pending' }
    | { kind: 'slow_down' }
    | { kind: 'tokens'; bundle: oauth.TokenEndpointResponse };

  try {
    const resp = await oauth.deviceCodeGrantRequest(
      as, client, clientAuth, deviceCode, { signal }
    );
    const tokens = await oauth.processDeviceCodeResponse(as, client, resp);
    return { kind: 'tokens', bundle: tokens };
  } catch (err) {
    if (err instanceof oauth.ResponseBodyError) {
      switch (err.error) {
        case 'authorization_pending': return { kind: 'pending' };
        case 'slow_down':             return { kind: 'slow_down' };
        case 'access_denied':         throw new DeviceFlowDeniedError(err);
        case 'expired_token':         throw new DeviceFlowExpiredError(err);
        default:                      throw new DeviceFlowError(err);
      }
    }
    throw err;
  }
  ```
- Centralised `redact(s: string): string` installed at every log
  call site.
- `process.on('uncaughtException')` / `unhandledRejection` scrub
  `ya29.*`, `1//.*`, JWT-shaped substrings before logging.

## Phase 5 — hub-mcp credential storage (OS-native keyring; TDD)

New module: `ts-packages/quarto-hub-mcp/src/auth/credential-store.ts`.
Single opaque JSON blob per `@napi-rs/keyring`:

| Platform | Backend | Binding |
|---|---|---|
| Windows | Credential Manager (DPAPI) | Current user account |
| macOS | login Keychain, `kSecAttrAccessibleWhenUnlocked` | Current user account |
| Linux | Secret Service / libsecret (default collection) | Current user session |

No plaintext file on disk; no silent degradation.

### Tests first

Tests in `ts-packages/quarto-hub-mcp/test/auth/credential-store.test.ts`.
Mock backend for unit tests; real keyring gated on
`KEYRING_INTEGRATION=1` for per-platform CI lanes.

**Cross-platform:**

- [x] `read_returns_null_when_keyring_entry_absent`
- [x] `write_then_read_round_trips` — deep equality on every field.
- [x] `write_uses_locked_service_and_account_names` — `service =
  'dev.quarto.hub-mcp'`, `account = '<issuer>:<client_id>'`.
- [x] `entries_scoped_by_client_id_do_not_collide` — write under
  client_id_a, read under client_id_b → `null`.
- [x] `clear_removes_the_entry`
- [x] `concurrent_writes_serialise_via_mutex` — last-wins, never torn.
- [x] `corrupt_blob_yields_null_not_throw`
- [x] `schema_version_mismatch_yields_null_not_throw` — schema_version
  999 → re-auth, not crash.
- [x] `read_does_not_log_credential_values`
- [x] `write_does_not_log_credential_values`
- [x] `keyring_round_trip_completes_under_50ms_on_warm_path`

**Headless / no-keyring:**

- [x] `write_throws_typed_error_when_secret_service_unavailable` —
  `KeyringUnavailableError` naming Secret Service / libsecret.
- [x] `read_returns_null_when_secret_service_unavailable` — read is
  not fatal so try-without-creds-first still works.
- [x] `keyring_error_does_not_leak_blob_in_message`

**Platform-conditional** — opt-in integration lane gated on
`KEYRING_INTEGRATION=1` (not the `process.platform` switch the plan
sketched). The lane round-trips through the real `@napi-rs/keyring`
binding; per-platform CLI assertions (`security find-generic-password`,
`secret-tool lookup`, `cmdkey /list`) are deferred to the Phase 9 E2E
verification because they shell out to OS tools rather than exercise
the credential-store module:

- [x] `[integration]` round-trips through the real OS keyring
- [x] `[integration]` clear removes a previously-written entry

### Implementation

- `class CredentialStore` with `read(): Promise<CredentialBundle | null>`,
  `write(bundle): Promise<void>`, `clear(): Promise<void>`.
- Single `@napi-rs/keyring` `Entry`:
  ```ts
  const SERVICE_NAME = 'dev.quarto.hub-mcp';
  const accountName = `${cfg.issuer}:${cfg.clientId}`;
  const entry = new keyring.Entry(SERVICE_NAME, accountName);
  ```
- In-process mutex via single `Promise` chain.
- `JSON.stringify`/`parse`; read path validates `schema_version`.
- **Error mapping** (asymmetric on purpose):
  - `read`: not-found / Secret-Service-unavailable / D-Bus errors
    → `null` with redaction-aware warning.
  - `write` / `clear`: Secret-Service-unavailable → throw typed
    `KeyringUnavailableError`. Connection-manager (Phase 8) maps this
    to a tool-surface error directing user toward Secret Service
    install or SPA cookie path.
- All log sites go through `redact()`. Keyring errors re-wrapped via
  `redact(err.message)`.
- **No silent fallback.** Keyring or nothing.

### Phase 5 — completion notes (2026-05-21)

Landed on `feature/hub-mcp-device-flow`:

- New module `ts-packages/quarto-hub-mcp/src/auth/credential-store.ts`
  exposes `CredentialStore`, `KeyringUnavailableError`, the
  `SERVICE_NAME` constant (`'dev.quarto.hub-mcp'`), the
  `defaultKeyringBackend` factory, and the `CredentialBundle` /
  `CredentialStoreConfig` / `KeyringBackend` types.
- `CredentialStore` accepts an optional `KeyringBackend` parameter
  so unit tests inject in-memory or failing backends without
  touching the platform keyring. The default backend wraps
  `@napi-rs/keyring`'s `AsyncEntry(SERVICE_NAME, '<issuer>:<client_id>')`.
- On-disk blob is `schema_version: 1`, with `issuer`, `client_id`,
  `id_token`, `refresh_token`, `id_token_expires_at` (ISO 8601),
  and `scopes` exactly per the Phase 1 lock-in. `parseBundle`
  returns `null` on `JSON.parse` failure, schema mismatch, missing /
  malformed required fields, or unparseable date — never throws.
- In-process mutex is a tail-promise chain (`enqueue`): every
  operation chains onto the previous one with `prev.then(op, op)`
  so a rejected operation doesn't poison the chain, and the shared
  tail records only the "settled" signal.
- Asymmetric error handling per the Phase 1 contract: `read`
  catches every backend failure and folds it to `null` (logging a
  redacted warning via `redactTokens`) so try-without-creds-first
  still works on headless Linux. `write` / `clear` re-wrap every
  backend failure as `KeyringUnavailableError`, with the backend
  message run through `redactTokens` before embedding so a leaky
  backend message cannot propagate token bytes.
- `redactTokens` is re-exported from `device-flow.ts`; the
  credential-store module funnels every log + error message through
  it, so the cross-module redaction surface stays single-sourced.
- Phase 4's `no_baked_default_client_id_or_secret` walker now skips
  every `*.test.ts` file rather than only `device-flow.test.ts`;
  fixture-shaped `*.apps.googleusercontent.com` literals in test
  files are legitimate and the walker should not flag them. Non-
  test source files are still scanned.

Tests live at `src/auth/credential-store.test.ts` (co-located, same
convention Phase 4 established). The "Platform-conditional" tests
the plan sketched are wired as an opt-in `[integration]` describe
gated on `KEYRING_INTEGRATION=1`; they round-trip through the real
`@napi-rs/keyring` binding rather than shell out to per-platform
CLI tools (`security` / `secret-tool` / `cmdkey`). Those CLI
assertions are deferred to Phase 9's E2E verification, which
already calls them out explicitly. Each integration test scopes
itself to a per-run client_id so parallel runs don't collide.

Verification: 21 new Vitest specs pass under `npm test` (60 of the
68 specs in this package now pass; 6 failures remain in
`src/hub-mcp.test.ts` from the documented pre-existing `indexedDB
is not defined` issue and 2 are the opt-in integration lane).
Confirmed via stash + re-run that the failure count is unchanged
from the pre-Phase-5 baseline. `npm run typecheck` and `npm run
build` both clean; `dist/auth/credential-store.{js,d.ts}` emitted.

Dependencies added to `ts-packages/quarto-hub-mcp/package.json`:
`@napi-rs/keyring ^1.3.0`. Workspace `npm install` from repo root
picked up the binding plus the macOS-arm64 prebuilt; root
`package-lock.json` is the change record.

## Phase 6 — hub-mcp refresh-on-401 (TDD)

New module: `ts-packages/quarto-hub-mcp/src/auth/refresh-manager.ts`.

### Tests first

- [x] `refresh_called_on_401` — hub returns 401 once; refresh; retry;
  succeeds.
- [x] `refresh_failure_triggers_reauth` — Google returns
  `invalid_grant`; `CredentialStore.clear()`; caller receives
  `ReauthRequired`.
- [x] `concurrent_401s_share_single_refresh` — three in-flight requests
  → one POST to `/token`; all three retries see same new ID token.
- [x] `expired_id_token_proactively_refreshes` — within 60 s skew of
  expiry → refresh before sending.
- [x] `refresh_persists_new_id_token_and_expiry`
- [x] `refresh_keeps_original_refresh_token_when_google_omits_field`
  — **live Google behaviour** (empirically confirmed 2026-05-19,
  3/3 refreshes omitted the field).
- [x] `refresh_keeps_original_refresh_token_when_google_returns_same_value`
- [x] `refresh_persists_rotated_refresh_token_when_google_returns_new_value`
  — defensive in case Google or future IdP changes behaviour.
- [x] `refresh_does_not_log_tokens`
- [x] `refresh_does_not_persist_partial_state_on_failure`

### Implementation

- `class RefreshManager` wraps store + `oauth4webapi` server/client/auth:
  ```ts
  const resp = await oauth.refreshTokenGrantRequest(
    as, client, clientAuth, refreshToken
  );
  const tokens = await oauth.processRefreshTokenResponse(as, client, resp);
  ```
- **Refresh-token persistence rule:** if response has `refresh_token`,
  persist it; else keep the prior value. Discarding on missing field
  would force re-auth on every refresh.
- Public API:
  - `getValidIdToken(): Promise<string>` — refreshes within skew window.
  - `forceRefresh(): Promise<string>` — used by 401 retry path.
  - Both share in-flight-promise mutex; concurrent callers coalesce.
- On `ResponseBodyError` with `err.error === 'invalid_grant'`: call
  `CredentialStore.clear()`, throw typed `ReauthRequired`. User-visible
  message: "Your Quarto Hub credentials have expired or were revoked.
  Ask me to authenticate again."

### Phase 6 — completion notes (2026-05-21)

Landed on `feature/hub-mcp-device-flow`:

- New module `ts-packages/quarto-hub-mcp/src/auth/refresh-manager.ts`
  exposes `RefreshManager` (`getValidIdToken` + `forceRefresh`),
  the typed `ReauthRequired` error, and the `RefreshManagerDeps` /
  `RefreshManagerConfig` types.
- `forceRefresh` shares an in-flight `Promise<string>` mutex; both
  the direct call and the `getValidIdToken` proactive-refresh path
  coalesce onto it, so concurrent callers issue exactly one
  `/token` POST and observe the same new id_token. The mutex
  detaches its cleanup with `.finally(...).catch(() => undefined)`
  so the caller observes the underlying rejection (the chained
  promise would otherwise mask it) and a failed refresh leaves the
  next call free to start fresh.
- Refresh-token persistence rule per the Phase-1 lock-in: if the
  `/token` response carries a non-empty `refresh_token`, we
  persist it; otherwise we keep the prior value. Verified against
  empirical Google behaviour (omitted field on live refreshes) and
  the two defensive paths (same-value, rotated).
- `id_token_expires_at` is derived from the refreshed id_token's
  `exp` claim via `jose.decodeJwt` — `expires_in` from the response
  describes the access token, not the id token. A missing or
  malformed `exp` is a hard error (no partial-state write).
- `invalid_grant` is the one terminal failure the manager handles
  itself: clears the credential store (`store.clear()` failures
  are swallowed since the original problem is the rejected token,
  not the cleanup) and throws `ReauthRequired` with the documented
  user-visible message and `oauthError = 'invalid_grant'` for
  callers that want to discriminate. All other failures propagate
  untouched and leave the store byte-identical to its pre-call
  state — `refresh_does_not_persist_partial_state_on_failure`
  asserts this against both a network-throw and a 500-response
  fixture.
- Logging redaction is structurally guaranteed by routing every
  log site in `credential-store.ts` (which the manager owns
  writes through) via `redactTokens`; the manager itself does no
  direct logging. The `does_not_log_*` spec spies on every
  `console.*` sink and verifies no token bytes (old or new id /
  refresh) leak.

Tests live at `src/auth/refresh-manager.test.ts` (co-located, same
convention Phases 4–5 established). They stub `oauth4webapi`'s
`customFetch` symbol so no live Google call is ever made. The
fake-id_token helper fills in `iss`/`aud`/`azp`/`iat` defaults
because `oauth4webapi.processRefreshTokenResponse` validates those
claims even on the refreshed token.

Verification: 17 new Vitest specs pass under `npm test` from
`ts-packages/quarto-hub-mcp/` (77 of the 85 specs in this package
now pass; 6 failures remain in `src/hub-mcp.test.ts` from the
documented pre-existing `indexedDB is not defined` issue and 2 are
the opt-in `[integration]` lane). `npm run typecheck` and
`npm run build` both clean.

No new dependencies — the module re-uses `oauth4webapi ^3.5.5` and
`jose ^6.0.11` already added by Phase 4.

## Phase 7 — hub-mcp MCP-tool exposure for auth (TDD)

New module: `ts-packages/quarto-hub-mcp/src/auth/auth-tools.ts`.

stdio-transport MCP clients (Claude Code, Claude Desktop, Cursor,
Continue, etc.) capture stderr to log files — banners never reach the
user. The MCP tool response is the only agent-visible channel.
Uses only standard `CallToolResult.content` text — no
client-specific rendering hints.

### Tool surface

Registered alongside existing read/write tools in `tools.ts`. Agent
flow: call `authenticate_start` on `AuthRequired` (Phase 8) → show
URL+code → user completes browser flow → call `authenticate_finish` →
re-prompt on `pending`/`slow_down`.

```
authenticate_start({}) -> { verification_uri, canonical_url,
                            user_code, expires_in_seconds }
                         | "already authenticated as <email>"
                         | "the configured hub does not require
                            authentication; no action needed"
authenticate_finish({}) -> "authenticated as <email>"
                         | "still pending — complete the flow then
                            ask me to retry"
                         | <typed error>
```

### Tests first

Tests in `ts-packages/quarto-hub-mcp/test/auth/auth-tools.test.ts`.

- [x] `start_returns_verification_uri_user_code_and_canonical_url`
- [x] `start_response_includes_expires_in_seconds`
- [x] `start_canonical_url_is_a_constant_not_from_google_response` —
  even with malicious mock AS response, canonical URL unchanged.
- [x] `start_caches_device_code_in_process_memory_only` — no
  `CredentialStore.write` call.
- [x] `start_short_circuits_when_already_authenticated`
- [x] `start_short_circuits_when_hub_known_no_auth` — spy on HTTP
  client; no request issued.
- [x] `start_initiates_device_flow_when_hub_known_auth_required`
- [x] `start_initiates_device_flow_when_auth_mode_unknown` — only
  positive `'no-auth'` triggers short-circuit.
- [x] `start_overwrites_prior_unconsumed_device_code` — outside the
  ~5 s coalescing window.
- [x] `finish_without_prior_start_returns_typed_error`
- [x] `finish_pending_returns_user_actionable_text`
- [x] `finish_slow_down_returns_user_actionable_text_with_wait`
- [x] `finish_success_persists_bundle_via_credential_store`
- [x] `finish_success_clears_cached_device_code`
- [x] `finish_terminal_error_clears_cached_device_code` —
  `DeviceFlowDeniedError` / `DeviceFlowExpiredError` (split into one
  spec each).
- [x] `finish_returns_authenticated_as_email_from_id_token` — only
  `email` claim, nothing else.
- [x] `tool_responses_never_contain_id_token_or_refresh_token`
- [x] `expired_cached_device_code_is_cleared_on_next_start`
- [x] `concurrent_finish_calls_serialise_safely`
- [x] `finish_called_too_soon_returns_slow_down_advice_without_polling_google`
  — `nextPollAllowedAt > now`: return "still pending" text, **do not**
  call `oauth4webapi.deviceCodeGrantRequest`.
- [x] `finish_after_interval_elapsed_polls_google`
- [x] `start_called_repeatedly_within_window_short_circuits` — two
  calls within ~5 s return same `device_code` without calling Google.
- [x] `slow_down_response_increases_subsequent_interval` — bumps
  `nextPollAllowedAt` by 5 s per RFC 8628 §3.5.

### Phase 7 — completion notes (2026-05-21)

Landed on `feature/hub-mcp-device-flow`:

- New module `ts-packages/quarto-hub-mcp/src/auth/auth-tools.ts`
  exposes `AuthToolsState` (handler class), `AUTH_TOOL_DEFINITIONS`
  (the two `Tool` records), `CANONICAL_VERIFICATION_URL` constant,
  `registerAuthTools`, and the `AuthToolsDeps` /
  `LastObservedAuthModeSource` / `AuthFlowConfig` types.
- `AuthToolsState.handleStart` walks the documented 3-step short-
  circuit chain: (1) `RefreshManager.getValidIdToken()` succeeds →
  "Already authenticated as <email>." with no Google call; (2)
  `connectionManager.lastObservedAuthMode() === 'no-auth'` → "The
  configured hub does not require authentication; no action needed."
  with no Google call; (3) otherwise initiates the device flow and
  caches the result. `ReauthRequired` is the *only* error from
  `getValidIdToken` that falls through to the device-flow path —
  other failures propagate so an unexpected network blip doesn't
  silently start a new flow.
- `CANONICAL_VERIFICATION_URL` is a `const`-bound literal
  (`'https://www.google.com/device'`); the response text quotes it
  unconditionally and reproduces Google's `verification_uri` as a
  secondary "also valid" hint. The phishing test injects a malicious
  `verification_uri` and asserts the canonical URL is unchanged in
  the response.
- Cached device-flow state lives on a closure-scoped private field
  `{ deviceCode, userCode, verificationUri, expiresAt, startTime,
  interval, nextPollAllowedAt }` — never persisted. A repeat
  `authenticate_start` within `coalesceWindowMs` (default 5 s)
  returns the same cached values without a fresh Google call; outside
  that window the cache is overwritten. `clearCacheIfExpired` runs
  before every read so expired codes are GC'd implicitly.
- `handleFinish` rate-limits via `nextPollAllowedAt` per RFC 8628
  §3.5: if `now < nextPollAllowedAt`, return "still pending — wait
  N seconds" text *without* calling Google. On `pending` it bumps
  `nextPollAllowedAt` by the cached `interval`; on `slow_down` it
  bumps both `interval` and `nextPollAllowedAt` by an additional 5 s
  (the SLOW_DOWN_BUMP constant). On success it decodes the `email`
  claim from the id_token (and nothing else from the JWT body),
  writes a fresh `CredentialBundle` to the store, clears the cached
  device_code, and returns "Authenticated as <email>." Terminal
  errors (`DeviceFlowDeniedError` / `DeviceFlowExpiredError` /
  generic `DeviceFlowError`) clear the cache and surface a
  redacted error message — `redactTokens` runs on every error
  message before it reaches the tool response.
- Concurrent `handleFinish` calls serialise via a tail-promise mutex
  mirroring `CredentialStore`/`RefreshManager`. The second caller
  observes the first's cache mutation: on success that's a cleared
  cache → second call returns the "no flow in progress" tool error,
  which the `concurrent_finish_calls_serialise_safely` spec asserts
  (exactly one success, exactly one error).
- `ConnectionManager` gained a minimal `lastObservedAuthMode()`
  accessor that returns `'unknown'` for now (with the `ObservedAuthMode`
  type exported). Phase 8 will be the one to flip this to `'no-auth'`
  / `'requires-auth'` based on actual WS-upgrade outcomes; today's
  default means Phase 7's short-circuit-on-`'no-auth'` path is
  inert until Phase 8 lands.
- `registerAuthTools(server, deps)` returns the `AuthToolsState` so
  the caller (index.ts in Phase 8) can compose it with `registerTools`
  into a single `CallToolRequestSchema` dispatcher. Today it
  installs handlers that respond to only the auth tools — that's the
  correct behaviour when called in isolation, and is overridden by
  the combined-dispatch wiring Phase 8 will land.

Tests live at `src/auth/auth-tools.test.ts` (co-located, same
convention Phases 4–6 established). They drive `AuthToolsState`
directly with injected `now`/`fetch` — no MCP `Server` instance is
spun up. The `CredentialStore` is wired to an in-memory keyring
backend so writes are observable; the `RefreshManager` is the real
class with its `fetch` stubbed. The `LastObservedAuthModeSource`
type is a single-method interface so the stub fits in one line.

Verification: 26 new Vitest specs pass under `npm test` from
`ts-packages/quarto-hub-mcp/` (103 of the 111 specs in this package
now pass; 6 failures remain in `src/hub-mcp.test.ts` from the
documented pre-existing `indexedDB is not defined` issue and 2 are
the opt-in `[integration]` lane). Confirmed via stash + re-run that
the failure count is unchanged from the pre-Phase-7 baseline. `npm
run typecheck` and `npm run build` both clean; `dist/auth/auth-tools.{js,d.ts}`
emitted.

No new dependencies — the module re-uses `oauth4webapi`, `jose`, and
the `@modelcontextprotocol/sdk` already on the workspace
`package-lock.json`.

### Implementation

- Module exposes:
  ```ts
  registerAuthTools(server, deps: {
    credentialStore: CredentialStore;
    refreshManager: RefreshManager;
    connectionManager: ConnectionManager;
    flowConfig: { clientId: string; issuer: string };
  }): void
  ```
  Tool annotations: `readOnlyHint: false`, `destructiveHint: false`,
  `idempotentHint: false`.
- Cached state (closure-local, never persisted):
  `{ deviceCode, interval, expiresAt: Date, nextPollAllowedAt: Date }`.
  Accessor clears when `expiresAt < now`.
- **Rate limiting** (RFC 8628 §3.5):
  - `authenticate_start`: if non-expired `device_code` exists and was
    created within ~5 s, return cached values without re-initiating.
  - `authenticate_finish`: gate on `nextPollAllowedAt`; bump by 5 s on
    `slow_down`; if too soon, return "still pending" text without
    calling Google.
  - Clear cached state on success and on terminal errors.
- `authenticate_start`:
  1. `RefreshManager.getValidIdToken()` succeeds → decode email,
     return "Already authenticated as <email>".
  2. `connectionManager.lastObservedAuthMode() === 'no-auth'` → return
     "The configured hub does not require authentication; no action
     needed." (no Google call). Only positive observation triggers
     short-circuit; `'requires-auth'` and `'unknown'` fall through.
  3. Otherwise call `initiateDeviceFlow`, cache state, return:
     ```
     To authenticate Quarto Hub MCP:

     1. Open https://www.google.com/device in your browser
        (also valid: <verification_uri Google returned>)
     2. Enter this code: ABCD-EFGH
     3. Sign in and approve the consent screen.

     The code expires in <expires_in_seconds> seconds. Once
     you've completed those steps, ask me to finish
     authentication.
     ```
- `authenticate_finish`:
  1. No cached device_code or expired → tool error directing to
     `authenticate_start`.
  2. `pollDeviceFlowOnce(config, deviceCode)`.
  3. Dispatch:
     - `pending` → "Still waiting for browser approval…"
     - `slow_down` → similar with recommended brief wait.
     - `tokens` → `CredentialStore.write(bundle)`, decode email,
       clear cached device_code, return "Authenticated as <email>".
     - `DeviceFlowDeniedError` / `DeviceFlowExpiredError` → clear,
       return typed tool error.
- Canonical URL `https://www.google.com/device` is a hard-coded
  constant in this module — never from Google's response.

### Cross-tool wiring

`registerAuthTools` runs **before** `registerTools` so read/write tools
can detect "no credentials" and instruct the agent to call
`authenticate_start` in their own error text.

## Phase 8 — quarto-sync-client + connection-manager integration (TDD)

`@automerge/automerge-repo-network-websocket@2.5.1`'s
`BrowserWebSocketClientAdapter` does **not** accept custom headers
(constructor `(url, retryInterval = 5000)` → `new WebSocket(this.url)`,
`WebSocketClientAdapter.ts:53-59,82`). Ship a local
`NodeWebSocketClientAdapter` inside `quarto-sync-client` that constructs
`new WebSocket(url, [], { headers })` on Node. Selected when consumer
passes `auth.getBearer`; browser default unchanged.

Follow-up beads issue: submit upstream PR to thread `headers` through
`BrowserWebSocketClientAdapter`.

### Tests first

- [x] `client_passes_authorization_header_to_adapter` — `WebSocketFactory`
  test seam captures the constructor headers.
- [x] `client_does_not_log_authorization`
- [x] `client_redacts_token_in_error_messages` — `[redacted]`, not token.
- [x] `connection_manager_threads_token_through_when_creds_exist`
- [x] `connection_manager_omits_authorization_when_no_creds`
- [x] `connection_manager_succeeds_against_no_auth_hub_without_creds` —
  probe `/health` returns 200 with no header; connect succeeds with no
  device-flow trigger.
- [x] `connection_manager_succeeds_against_no_auth_hub_with_stale_creds`
- [x] `connection_manager_handles_401_via_refresh_then_retry`
- [x] `connection_manager_surfaces_reauth_required` — typed error
  named `authenticate_start`.
- [x] `connection_manager_surfaces_auth_required_when_hub_demands_auth_and_no_creds`
  — trigger is the hub's 401, not absence of creds.
- [x] `connection_manager_surfaces_reauth_after_post_refresh_401` —
  second consecutive 401 trigger.
- [x] `last_observed_auth_mode_starts_unknown`
- [x] `last_observed_auth_mode_becomes_no_auth_on_101_without_creds`
- [x] `last_observed_auth_mode_becomes_requires_auth_on_101_with_creds`
- [x] `last_observed_auth_mode_becomes_requires_auth_on_401`
- [x] `last_observed_auth_mode_unchanged_on_network_error`
- [x] `last_observed_auth_mode_not_persisted_across_restart` — trivially
  satisfied; the field lives on the class instance, no persistence layer.
- [x] `browser_path_unchanged_when_no_auth` — `buildWsAdapter` falls
  through to `BrowserWebSocketClientAdapter` when `auth` is absent.
- [x] `insecure_bearer_to_loopback_succeeds_without_env_flag` —
  `localhost` / `127.0.0.1` / `::1` / `*.localhost`.
- [x] `insecure_bearer_to_non_loopback_throws_without_env_flag` —
  `InsecureTransportError` naming the env var. No HTTP issued.
  **CVE-prevention test.**
- [x] `insecure_bearer_to_non_loopback_succeeds_with_env_flag` — loud
  warning on every connect (not just first).
- [x] `no_bearer_over_http_to_non_loopback_succeeds_without_env_flag`
  — no Bearer to leak, gate doesn't fire.
- [x] `wss_bearer_to_non_loopback_succeeds_without_env_flag` — baseline.

### Phase 8 — completion notes (2026-05-21)

Landed on `feature/hub-mcp-device-flow`:

- New `quarto-sync-client/src/NodeWebSocketClientAdapter.ts` — Node-only
  `NetworkAdapter` that constructs `new WebSocket(url, [], { headers })`
  via the `ws` package, threading `Authorization: Bearer <token>` into
  the WebSocket upgrade. The `getBearer()` getter is invoked on every
  `connect` (including upstream's retry loop) so the refreshed token
  surfaces on reconnect. A `WebSocketFactory` test seam lets unit tests
  drive the adapter without standing up a real socket.
- `quarto-sync-client/src/client.ts` — `connect()` and `createNewProject()`
  now accept `auth?: { getBearer: () => Promise<string> }`. When `auth`
  is set, the Node adapter is lazily imported via dynamic
  `import('./NodeWebSocketClientAdapter.js')` so browser bundles never
  pull in `ws`. When `auth` is absent, the existing
  `BrowserWebSocketClientAdapter` path runs unchanged.
- `redactAuthorization` is exported from `quarto-sync-client` — a small
  helper that swaps `Authorization: Bearer <token>` for
  `Authorization: [redacted]` in arbitrary strings. The adapter funnels
  every error-throw through it.
- `quarto-hub-mcp/src/connection-manager.ts` — rewritten around an
  HTTP `/health` probe + try-then-fallback policy. New constructor
  takes `ConnectionManagerDeps` (`serverUrl`, optional `credentialStore`,
  optional `refreshManager`, plus `fetch` / `env` / `syncClientFactory` /
  `probePath` test seams). The legacy `new ConnectionManager(url)`
  signature is still accepted for the no-auth path. `connect()` walks:
  1. Read bundle (`null` if missing or store absent).
  2. With bundle: insecure-transport gate, `getValidIdToken()`, probe
     `/health`. 401 → `forceRefresh()` + retry. Still 401 → throw
     `ReauthRequired`. 200 → open WS with a `getBearer` getter that
     pulls a freshly-refreshed token on each attach.
  3. Without bundle: probe with no header. 200 → no-auth hub, open WS
     without header. 401 → throw `AuthRequiredError` naming
     `authenticate_start`.
  4. Network error → typed connection error; auth state unchanged.
- `lastObservedAuthMode()` flips per the documented state machine —
  `'no-auth'` on 200-without-header, `'requires-auth'` on
  200-with-header (conservative) or 401, unchanged on network error.
  Process-local; resets to `'unknown'` on restart by construction.
- Insecure-transport gate fires **only** when a Bearer is about to be
  attached. Loopback (`localhost`, `127.0.0.1`, `::1`, `*.localhost`)
  is always permitted. Non-loopback `ws://` / `http://` is rejected
  unless `QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1` is set; with the env
  var set the gate emits a loud `console.warn` on every connect.
- `quarto-hub-mcp/src/index.ts` — bootstraps the auth chain when both
  `QUARTO_HUB_MCP_CLIENT_ID` and `QUARTO_HUB_MCP_CLIENT_SECRET` are
  set. Partial env config fails loud through `MissingCredentialsConfigError`
  with both var names named. When auth env is absent, hub-mcp runs
  unauthenticated and a no-auth hub still works (the connection-manager's
  no-creds branch covers it). `installRedactingErrorHandlers` wires
  `uncaughtException` / `unhandledRejection` scrubbers that run every
  message through `redactTokens` before logging.
- `quarto-hub-mcp/src/tools.ts` — `registerTools` gained an optional
  `AuthToolsState` parameter. When provided, the dispatcher composes
  the auth tools (`authenticate_start` / `authenticate_finish`) with
  the data tools under a single `CallToolRequestSchema` handler; the
  `ListTools` response surfaces both families. Error messages from
  data-tool handlers run through `redactTokens` defensively.

Tests live at `ts-packages/quarto-sync-client/src/NodeWebSocketClientAdapter.test.ts`
(5 specs) and `ts-packages/quarto-hub-mcp/src/connection-manager.test.ts`
(24 specs). Both files inject all external surfaces (fetch / WebSocket
factory / RefreshManager) so no live Google / hub call is ever made.

Verification: `npm test` from `ts-packages/quarto-sync-client/` passes
86/86 specs; from `ts-packages/quarto-hub-mcp/` 127 of 135 specs pass,
with the 6 pre-existing `indexedDB is not defined` failures in
`hub-mcp.test.ts` and 2 opt-in `[integration]` keyring specs unchanged
from the documented baseline. `npm run typecheck` and `npm run build`
clean in both packages. `cargo xtask verify --skip-hub-build
--skip-rust-tests` green (this phase touches only TypeScript).
`hub-client` (the SPA that consumes `@quarto/quarto-sync-client`)
typechecks clean — backwards-compatibility on the optional `auth`
parameter is intact.

Smoke tests: `node dist/index.js --help` prints usage; the no-args
path errors with the required-arg message; the partial-env path
(`QUARTO_HUB_MCP_CLIENT_ID` set, `_CLIENT_SECRET` unset) exits 1 with
`MissingCredentialsConfigError` naming both vars.

Dependencies added to `ts-packages/quarto-sync-client/package.json`:
`ws ^8.18.0` (Node-only WebSocket library that supports custom
upgrade headers) and dev-dep `@types/ws ^8.18.1`. `npm install` from
repo root hoisted them; root `package-lock.json` is the change record.

### Implementation

- Add `auth?: { getBearer: () => Promise<string> }` to `connect()`
  options. A getter so retry loop sees the refreshed token.
- New `quarto-sync-client/src/NodeWebSocketClientAdapter.ts`
  implementing the upstream `NetworkAdapter` contract but with
  `new WebSocket(url, [], { headers: { Authorization: \`Bearer ${token}\` } })`.
- `client.ts` selects adapter at `:336` and `:722` based on
  `auth.getBearer` presence.
- **`connection-manager.ts` try-then-fallback policy:**
  1. Read bundle from `CredentialStore` (may be absent).
  2. Attempt WS upgrade with `Authorization: Bearer <bundle.idToken>`
     if bundle exists; without header otherwise.
  3. Dispatch:
     - **101** → success.
     - **401 + creds attached** → `forceRefresh()`, retry once. Still
       401 → `ReauthRequired` naming `authenticate_start`.
     - **401 + no creds attached** → `AuthRequired` naming
       `authenticate_start`. Trigger is the hub's 401, not creds absence.
     - Other / network error → typed connection error; auth state
       unchanged.
- **`lastObservedAuthMode: 'no-auth' | 'requires-auth' | 'unknown'`**
  (process-local, initialised `'unknown'`):
  - 101 + no Authorization attached → `'no-auth'`.
  - 101 + Authorization attached → `'requires-auth'` (conservative).
  - Any 401 → `'requires-auth'`.
  - Other / network error → unchanged.

  Exposed via `lastObservedAuthMode()` for Phase 7's short-circuit.
- **Insecure-transport gate.** Before constructing socket:
  1. No Bearer being attached → no gate fires.
  2. `wss://` / `https://` → fine.
  3. `ws://` / `http://` + loopback host → fine.
  4. `ws://` / `http://` + non-loopback → require
     `QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1`. Unset → throw
     `InsecureTransportError` naming the env var. Set → loud warning
     on every connect, proceed.
- Centralised redaction shared with Phase 4.

## Phase 9 — End-to-end verification (CRITICAL — per CLAUDE.md)

Tests passing is necessary but not sufficient. Before declaring done:

1. `cargo xtask verify` clean.
2. Bring up real hub locally with audience allowlist (SPA client_id +
   hub-mcp client_id).
3. Sign in to SPA in real browser via Google OIDC; confirm cookie path
   unchanged (regression).
4. **Auth flow E2E.** Clean machine state (no keyring entry under
   `dev.quarto.hub-mcp:<issuer>:<client_id>`). Connect Claude Code as
   MCP client; ask agent to `connect_project`.
   - Agent receives typed `AuthRequired` naming `authenticate_start`.
   - Agent calls `authenticate_start`; tool response carries Google's
     `verification_uri`, the `user_code`, and canonical URL
     `https://www.google.com/device`.
   - Open canonical URL; type code; approve consent screen.
   - Agent calls `authenticate_finish` → "Authenticated as <email>".
   - Re-issue action; succeeds.
   - **Inspect credential store:**
     - macOS: `security find-generic-password -s dev.quarto.hub-mcp
       -a <issuer>:<client_id> -w` returns blob; parses to valid
       bundle. No plaintext file under `~/Library/Application Support/quarto/`.
     - Linux: `secret-tool lookup service dev.quarto.hub-mcp account
       <issuer>:<client_id>` returns blob. No plaintext under
       `~/.config/quarto/`.
     - Windows: `cmdkey /list:dev.quarto.hub-mcp:<issuer>:<client_id>`
       shows entry; round-trip via small Node REPL with
       `@napi-rs/keyring`. No plaintext under `%APPDATA%\quarto\`.
   - **No plaintext credential leaks:** `grep -r` token bytes against
     `$HOME` (or `%APPDATA%\..`) excluding OS keyring storage paths.
   - **Agent transcript:** no token value anywhere in tool responses
     or error messages.

4a. **No-auth hub regression.** Second hub instance with
    `auth_config: None`. Clean machine state, no keyring entry.
    - Agent action succeeds with no `AuthRequired`, no device flow.
    - No `authenticate_start` in MCP-client transcript.
    - No keyring entry created.
    - **Explicit-authenticate short-circuit.** After a hub call (flips
      `lastObservedAuthMode → 'no-auth'`), ask agent to authenticate.
      `authenticate_start` returns "The configured hub does not require
      authentication; no action needed.". No request to
      `oauth2.googleapis.com` (check hub-mcp logs).
    - **Short-circuit requires observation.** Restart hub-mcp (clears
      state). Before any hub call, ask agent to authenticate.
      Device flow *does* initiate (state is `'unknown'`). Complete or
      abort, then a hub call against the no-auth hub flips state, then
      the explicit-authenticate request short-circuits.

4b. **Allowlist parity E2E.** Restart authenticated hub with
    `--allowed-domains posit.co` (no `--allowed-emails`).
    - **SPA cookie path:** `@posit.co` Google account → 200. `@gmail.com`
      → 403 on `/auth/callback`; audit shows `credential_kind="cookie"`,
      `outcome="deny"`, `detail="user_not_allowlisted"`.
    - **MCP Bearer path:** clean keyring, hub-mcp through Claude Code
      with `@gmail.com` → device flow succeeds, WS upgrade returns 403.
      Audit shows `credential_kind="bearer"`, `outcome="deny"`,
      `detail="user_not_allowlisted"`. Agent surface reports 403 as
      typed error.
    - **MCP happy path:** `@posit.co` device flow → 200; `connect_project`
      succeeds.
    - Record both audit lines (cookie 403, bearer 403) in Verification
      log; `detail` byte-identical between them.

4c. **Insecure-Bearer dev-mode interaction.** Authenticated hub with
    `--allow-insecure-auth` (plain HTTP).
    - hub-mcp → `ws://localhost:3000/ws` without env var: connects,
      Bearer attached, auth succeeds (loopback exception).
    - Same on non-loopback bind (`ws://hub.local:3000/ws`) without env
      var: `InsecureTransportError`, no socket opened. With
      `QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1`: connects, loud warning
      in stderr/log.

5. **Force refresh.** Write modified bundle into keyring entry with
   `id_token_expires_at` in past (via `@napi-rs/keyring` or platform
   CLI). Re-run tool call; confirm single `/token` call to Google;
   keyring entry updated with fresh expiry.
6. **Force re-auth.** Revoke hub-mcp grant at myaccount.google.com.
   Re-run tool call; confirm typed `ReauthRequired` with documented
   message.
7. **Dual-credential CVE.** `curl -H "Authorization: Bearer <jwt>"
   --cookie "quarto_hub_token=…"` against protected endpoint → 400.
   Also: hub-mcp WS upgrade succeeds with **no** `Origin` header and
   **no** `X-Requested-With` (Node `ws` defaults). Regression here is
   the failure mode that drove this audit.
8. **Audit-log output.** Confirm `tracing` events for `auth_ok`
   (kind=cookie, kind=bearer) and `auth_fail` (conflicting_credentials)
   with correct `sub`, `credential_kind`, `action`, `outcome`.
9. **Record exact invocations + observed output** in Verification log
   below. Test-pass-only completion is **not acceptable**.

## Phase 10 — Operational hardening + documentation

- [x] **Operator runbook: hub-mcp Google OAuth registration.**
  One-time setup in Google Cloud Console, symmetric with existing SPA
  OAuth registration (covered in
  `claude-notes/plans/2026-02-24-oauth2-middleware-design.md`):
  - Create second OAuth client of type "TV and Limited Input devices"
    in same Google project as SPA client.
  - Copy client_id and client_secret.
  - Configure hub with `--additional-audiences <hub-mcp-client-id>`.
  - Publish both values in operator's deployment docs.
  - Secret handled in operator's normal secret-management (Kubernetes
    Secret, 1Password Connect, AWS Secrets Manager, etc.). Rotate on
    leak via Google Cloud Console.
- [x] **End-user `.mcp.json` example:**
  ```json
  {
    "mcpServers": {
      "quarto-hub": {
        "command": "npx",
        "args": ["@quarto/quarto-hub-mcp", "--server", "wss://hub.example.com/ws"],
        "env": {
          "QUARTO_HUB_MCP_CLIENT_ID":     "<operator-supplied>.apps.googleusercontent.com",
          "QUARTO_HUB_MCP_CLIENT_SECRET": "<operator-supplied>"
        }
      }
    }
  }
  ```
  Document: both vars mandatory; values from operator's docs;
  per-developer `~/.config/claude/mcp.json` is the intended home (not
  repo-checked-in MCP configs).
- [x] **README: credential-sourcing rationale** — cross-reference
  Phase 1 lock-in and threat-model #10. Cover symmetry with hub-client,
  operator sovereignty, no Quarto-team default, `device_code`
  defence-in-depth, rotation procedure.
- [x] **Credential storage docs.** Per-platform clear commands:
  - Windows: `cmdkey /delete:dev.quarto.hub-mcp:…`
  - macOS: `security delete-generic-password -s dev.quarto.hub-mcp`
  - Linux: `secret-tool clear service dev.quarto.hub-mcp …`

  Bundle is bound to current user account. Stolen ID tokens
  authenticate ≤1 h regardless of grant revocation. Headless Linux
  without Secret Service / libsecret cannot run hub-mcp → SPA cookie
  path or install `gnome-keyring-daemon` / `kwallet5`.
- [x] Document the revocation path (myaccount.google.com → Third-party
  apps).
- [x] Audit error-reporting and tracing config for token-leak paths.
  Add regression test scanning tracing output for `Bearer ` and
  Google-token-shaped substrings.
- [x] `hub-client/changelog.md` not updated by this work — SPA does
  not change.
- [ ] **Future work, not in v1:**
  - `authenticate_status` and `authenticate_clear` MCP tools.
  - `--login` CLI flag for direct interactive runs.
  - Hub-side `sub_denylist` — closes the ≤1 h residual-validity window.
    v1 accepts the risk; promote if leakage-detection telemetry shows
    exploitation.
  - Per-project / per-scope authorization with hub-issued wrapper JWT.
  - GitHub OIDC as second IdP.
  - Refresh-token expiry monitoring + proactive re-auth nudge.
  - PKCE on device-authorization grant (RFC 9700 §4.13) — Google
    doesn't support it on this endpoint today; revisit if/when it does.

### Phase 10 — completion notes (2026-05-21)

Landed on `feature/hub-mcp-device-flow`:

- New `claude-notes/instructions/hub-mcp-operator-runbook.md` — the
  operator-facing setup doc. Covers the four phases of operator
  responsibility: (1) registering the second "TV and Limited Input
  devices" Google OAuth client in the same Google Cloud project as
  the SPA client, (2) configuring the hub with
  `--oidc-client-id` (SPA primary) plus `--additional-audiences`
  (hub-mcp client_id), (3) publishing both `QUARTO_HUB_MCP_CLIENT_ID`
  / `QUARTO_HUB_MCP_CLIENT_SECRET` to end users via the operator's
  normal secret-management channel (1Password / K8s Secret / etc.),
  (4) verifying the first end-user flow against the hub. Includes
  auditing pointers (`RUST_LOG=quarto_hub::audit=info`), the secret
  rotation procedure (rotate in console → push to secret manager;
  existing user keyring entries remain valid because Google
  authenticates the per-flow `device_code`, not the secret), and the
  three residual risks operators should communicate to users.
- New `ts-packages/quarto-hub-mcp/README.md` — the end-user setup
  doc. Carries a copy-pasteable `.mcp.json` example with both env
  vars; the credential-storage table (service/account naming per
  platform); inspect / clear commands (`security
  find-generic-password`, `secret-tool lookup`/`clear`, `cmdkey
  /list`/`/delete`); the revocation procedure at
  myaccount.google.com; the headless-Linux caveat (Secret Service /
  libsecret required, no silent plaintext fallback); the
  credential-sourcing rationale (symmetry with hub-client, operator
  sovereignty, no Quarto-team default, `device_code` defence in
  depth from threat-model #10); and the
  `QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1` dev-mode escape hatch.
- New regression test `tracing_redacts_google_token_shapes` in
  `crates/quarto-hub/tests/auth_bearer.rs`. Drives a request bearing
  a synthetic `ya29.*` access-token shape in the `Authorization`
  header and a synthetic `1//*` refresh-token shape in the
  `quarto_hub_token` cookie (both shapes the centralised hub-mcp
  redact utility scrubs from logs); asserts that no field of any
  captured tracing event contains those substrings, the synthetic
  literals, or `Bearer `. Complements the existing
  `tracing_redacts_authorization_header` — that one proves the
  hub's accepted-Bearer path doesn't leak a valid JWT; the new one
  proves the rejected-token path doesn't leak the raw bytes through
  the `jwt_decode:{err}` audit-event `detail` field. Both pass
  under `cargo nextest run -p quarto-hub --test auth_bearer
  tracing_redacts` (run independently to avoid global-state
  cross-talk; the second run-mode that the existing dual-credential
  test exercises is unchanged).

Items intentionally **not** done in this phase:

- "Future work, not in v1" remains an unchecked notes list — those
  items (e.g. `authenticate_status` / `authenticate_clear` tools,
  hub-side `sub_denylist`, GitHub OIDC as second IdP, refresh-token
  expiry monitoring) are the v2 backlog and live there by design.
- `hub-client/changelog.md` — Phase 10 ships no SPA changes; the
  changelog convention only applies to commits that touch
  `hub-client/`.

Verification: `cargo nextest run -p quarto-hub --test auth_bearer
tracing_redacts` runs both redaction tests green
(`tracing_redacts_authorization_header`,
`tracing_redacts_google_token_shapes`). No code changes outside the
new test + two markdown docs; no TS surface touched.

## Residual risks accepted for v1

- Stolen ID tokens authenticate for up to ≤1 h regardless of grant
  revocation at Google (closed only by deferred `sub_denylist`).
- Leaked refresh tokens are an indefinite foothold until manual
  user-driven revocation (Google does not rotate refresh tokens for
  this client type — no rotation-based theft-detection signal).
- Headless Linux without Secret Service / libsecret cannot run hub-mcp
  — fall back to SPA cookie path.

## Beads issue plan

After review:

- One epic: "hub-mcp Google device-flow auth (Design C′)".
- One issue per phase (1–10), `parent-child`-linked to epic.
- Phase 2 dual-credential test gets its own `bug`-p0 issue: highest-
  impact CVE-shaped item.
- Phase 5 `keyring_error_does_not_leak_blob_in_message` gets `bug`-p1.
- Phase 7 `start_canonical_url_is_a_constant_not_from_google_response`
  gets `bug`-p1.

## References

- `claude-notes/plans/2026-04-27-hub-mcp-auth-design.md` — design doc.
- `claude-notes/plans/2026-03-13-hub-mcp-server-design.md` — hub-mcp design.
- `claude-notes/plans/2026-02-24-oauth2-middleware-design.md` — hub
  OAuth middleware.
- `claude-notes/plans/2026-02-26-httponly-cookie-auth.md` — HttpOnly
  cookie design.
- RFC 8628 — OAuth 2.0 Device Authorization Grant.
- Google "OAuth 2.0 for Limited-Input Devices":
  `https://developers.google.com/identity/protocols/oauth2/limited-input-device`
- `crates/quarto-hub/src/auth.rs` — OIDC/JWT validation; Phase 2 lands here.
- `crates/quarto-hub/src/server.rs:144-160,262-285` — cookie extraction
  + `Authenticated` extractor; Bearer extraction lands here.
- `ts-packages/quarto-sync-client/src/client.ts:336,722` — adapter
  call sites for Phase 8 swap.
- `ts-packages/quarto-hub-mcp/src/index.ts` — entry point;
  device-flow bootstrap lands here.
- `ts-packages/quarto-hub-mcp/src/connection-manager.ts` — Bearer
  plumbs into `client.connect()` here.
- Posit Assistant `MCPOAuthProvider.ts` — refresh-mutex pattern
  (Phase 6); skip the callback server (device flow doesn't need it)
  and `SingleFileStore` (we use OS keyring).
- `@napi-rs/keyring`: `https://github.com/Brooooooklyn/keyring-node`.

## Verification log

### Empirical verification — 2026-05-19

Scripts: `claude-notes/scripts/phase0-google-init.sh` and
`phase0-google-finish.sh`. Credentials from `~/.Renviron`.

**Option A vs Option B at `oauth2.googleapis.com/token`.** Two
independent runs polled `/token` pre-approval without `client_secret`.
Both returned:
```json
{ "error": "invalid_request",
  "error_description": "Missing required parameter: client_secret" }
```
→ **Option B locked in.**

**Device-authorization response** (Google's `/device/code`):
```json
{ "device_code": "AH-1Ng…<98 chars>",
  "user_code":   "FJZL-WTDR",
  "expires_in":  1800,
  "interval":    5,
  "verification_url": "https://www.google.com/device" }
```
Google uses `verification_url`; RFC 8628 says `verification_uri`.
`oauth4webapi` normalises. `verification_uri_complete` not returned.

**Token-endpoint response on successful grant** (redacted):
```json
{ "access_token": "<redacted>",
  "expires_in":   3599,
  "refresh_token":"<redacted>",
  "scope": "https://www.googleapis.com/auth/userinfo.email openid https://www.googleapis.com/auth/userinfo.profile",
  "token_type":   "Bearer",
  "id_token":     "<redacted>" }
```

**ID-token claim shape** (one fresh token, hub-mcp client):
```json
{ "iss": "https://accounts.google.com",
  "azp": "<hub-mcp client_id>",
  "aud": "<hub-mcp client_id>",
  "sub": "<redacted>",
  "hd":  "posit.co",
  "email": "<redacted>",
  "email_verified": true,
  "at_hash": "Av7uZsJPrzEgJVMPTK8YFA",
  "name": "<redacted>",
  "picture": "https://lh3.googleusercontent.com/...",
  "given_name": "<redacted>",
  "family_name": "<redacted>",
  "iat": 1779177814,
  "exp": 1779181414 }
```
- `aud` single string, not array
- `azp` present, equals `aud`
- `jti` absent
- `nbf` absent

**Refresh-token rotation** (three sequential
`grant_type=refresh_token` calls, then fourth using original):

| Call | `refresh_token` in response | `id_token` | Original still works? |
|---|---|---|---|
| 1 | **absent** | present | (n/a) |
| 2 | **absent** | present | (n/a) |
| 3 | **absent** | present | (n/a) |
| 4 (original) | — | present | **YES** |

→ **Google does NOT rotate refresh tokens for this client type.**

### Phase 9 end-to-end verification — 2026-05-21

This entry records the **autonomous half** of Phase 9: every check that
can be driven from a single terminal session without a real Google
consent screen, two Google accounts, or Claude Code as the MCP
client. The remaining sub-items (4 full flow, 4a no-auth E2E, 4b
allowlist-parity, 5 force-refresh, 6 force-reauth) are explicitly
**deferred to user-driven verification** — see § "Deferred to
user-driven verification" at the end of this section. Their
acceptance criteria are unchanged; only the operator is.

#### (1) `cargo xtask verify` — clean

Full WASM-leg verification (12/12 steps) on `feature/hub-mcp-device-flow`
at `eff8b2a0`:

```bash
cargo xtask verify
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# ✓ All verification steps passed!
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

12 steps including Rust workspace build + nextest, hub-client
build:all (WASM), hub-client test:ci, trace-viewer build, and the
q2-preview-spa build that bundles the freshly-rebuilt WASM. The
Playwright E2E lane was skipped (no `--e2e` flag) — that lane gates
on browser fixtures and is out of scope for this plan.

Confirms: Phase 2 (Rust hub middleware) and Phases 4–8 (TS
packages) all compile and test green together; the hub-client
SPA still typechecks against the updated `@quarto/quarto-sync-client`
optional `auth` parameter.

#### (7) Dual-credential CVE — observed 400 + body shape

Live hub bound to `127.0.0.1:13099` in production mode:

```bash
OIDC_CLIENT_ID=phase9-spa.apps.googleusercontent.com \
QUARTO_HUB_ADDITIONAL_AUDIENCES=phase9-mcp.apps.googleusercontent.com \
QUARTO_HUB_ALLOWED_DOMAINS=posit.co \
RUST_LOG="quarto_hub=info,quarto_hub::audit=info,info" \
  target/debug/hub --data-dir <tmp> --port 13099 \
                   --host 127.0.0.1 --behind-tls-proxy
```

At startup the hub discovered Google's JWKS at
`https://www.googleapis.com/oauth2/v3/certs` and locked the signing
algorithm to `[RS256]` (visible in log).

**Dual-credential on the WS upgrade endpoint** (the CVE-prevention
case that drove this audit) — `curl -H 'Cookie: …' -H 'Authorization:
Bearer …' http://127.0.0.1:13099/`:

```
HTTP/1.1 400 Bad Request
content-type: application/json
content-length: 35

{"error":"conflicting_credentials"}
```

**Dual-credential on `/health` (an `Authenticated`-extractor route)**:

```
HTTP/1.1 400 Bad Request
content-type: application/json

{"error":"conflicting_credentials"}
```

Both 400s emitted while the supplied JWTs were structurally
malformed — i.e. the 400 fires at credential extraction *before* any
token validation. ✓

#### Bearer-vs-Cookie asymmetric Origin gating

In strict mode (no `--allow-insecure-auth`), WebSocket upgrade with
**no Origin header**:

| Credential | Outcome | Reason |
|---|---|---|
| `Authorization: Bearer aaa.bbb.ccc` | 401 | Origin gate skipped; reached JWT validator (JWT-decode failure → 401) |
| `Cookie: quarto_hub_token=aaa.bbb.ccc` | 403 | Origin gate fired; never reached JWT validator |

The asymmetry is load-bearing for hub-mcp (Node `ws` client doesn't
default an `Origin` header) — verified with curl above. ✓

#### (8) Audit-log output — observed

`RUST_LOG=quarto_hub::audit=info` produced one event per auth
decision through `tracing::event!(target: "quarto_hub::audit", ...)`.
Captured from the live run (de-ANSI'd, tail of strict-mode log):

```
WARN quarto_hub::audit: action="auth_fail" outcome="deny"
  credential_kind="bearer" detail="conflicting_credentials"
INFO quarto_hub::audit: action="auth_fail" outcome="deny"
  credential_kind="bearer" detail=jwt_decode:JWT error: …
INFO request{method=GET path="/health"}: quarto_hub::audit:
  action="auth_fail" outcome="deny" credential_kind="bearer"
  detail=jwt_decode:JWT error: …
WARN request{method=GET path="/health"}: quarto_hub::audit:
  action="auth_fail" outcome="deny" credential_kind="bearer"
  detail="conflicting_credentials"
INFO request{method=POST path="/auth/logout"}: quarto_hub::audit:
  action="auth_fail" outcome="deny" credential_kind="cookie"
  detail=jwt_decode:JWT error: …
```

Confirms: `credential_kind` correctly distinguishes the two paths;
`detail="conflicting_credentials"` is byte-identical between the
WS-upgrade and `Authenticated`-extractor sites; failure events at
WARN (dual-cred CVE) and INFO (validation failure) are
appropriately differentiated; the tracing request span (`method`,
`path`) annotates extractor-side events and is absent for the WS
upgrade (which emits before any span enters scope). ✓

#### (4c partial) Insecure-Bearer gate — env-var smokes

Run from the assembled hub-mcp dist:

| Smoke | Env | Outcome |
|---|---|---|
| Missing `--server` | both vars set | exit 1, "--server `<url>` or QUARTO_HUB_SERVER is required" |
| Partial env (`CLIENT_ID` set, `CLIENT_SECRET` unset) | — | exit 1, `MissingCredentialsConfigError` naming both vars literally |
| Partial env (`CLIENT_SECRET` set, `CLIENT_ID` unset) | — | exit 1, `MissingCredentialsConfigError` naming both vars literally |

Verified message body:

```
[hub-mcp] QUARTO_HUB_MCP_CLIENT_SECRET is not set. Hub-mcp requires
QUARTO_HUB_MCP_CLIENT_ID and QUARTO_HUB_MCP_CLIENT_SECRET in the
MCP-client env. Ask your hub operator for the Google OAuth client
credentials they registered for hub-mcp.
```

The remainder of 4c (loopback / non-loopback ws:// behaviour, env-flag
override, loud warning on every connect) is covered by the
`connection-manager.test.ts` insecure-transport-gate specs that
`cargo xtask verify` runs green. The binary-level smoke confirms
those specs reflect what the shipping `dist/index.js` actually does
on startup. ✓

#### (4 partial) Plaintext-leak grep — clean

Non-test source files in `ts-packages/quarto-hub-mcp/src/` have **no**
matches for `*.apps.googleusercontent.com`, `GOCSPX-`, `ya29.`, or
`1//` (the four token-shape regexes the Phase 4
`no_baked_default_client_id_or_secret` walker enforces). Matches in
`*.test.ts` files are fixtures (literally `'GOCSPX-test-secret'`,
`'ya29.fake-access-token'`, `'1//original-refresh-token'`) and are
exempted by design. The Phase 4 walker spec passes under
`npx vitest run`.

Compiled `dist/` mirrors source — non-test `.js` files have no
literal matches; test compilation outputs (`*.test.js`) carry the
same fake fixtures. The package's `"private": true` in package.json
prevents these from ever reaching npm. ✓

The hub log inspection also confirmed redaction is structurally
intact: the bogus token strings I sent in `Authorization: Bearer …`
and `Cookie: quarto_hub_token=…` headers (`aaa.bbb.ccc`,
`zzz.yyy.xxx`) appear **zero** times in the captured hub log,
including the `tower-http` request span. ✓

#### Deferred to user-driven verification

The following sub-items require a real browser, real Google consent,
real Claude Code as the MCP client, real grant revocation in the
Google Account UI, or two distinct Google accounts on different
domains. They are intentionally deferred from this autonomous pass
and tracked under their original phase-9 acceptance criteria:

- **(3) SPA cookie path regression** — sign in to the SPA in a real
  browser via Google OIDC; confirm the cookie path still works.
- **(4) Full device-flow E2E through Claude Code** — clean keyring,
  `authenticate_start` → browser consent → `authenticate_finish` →
  re-issue action; inspect macOS Keychain entry with
  `security find-generic-password -s dev.quarto.hub-mcp -a
  <issuer>:<client_id> -w` and confirm no plaintext file under
  `~/Library/Application Support/quarto/`.
- **(4a) No-auth hub regression + explicit-authenticate short-circuit**.
- **(4b) Allowlist-parity E2E** — needs a `@posit.co` and a
  non-allowlisted account; capture the byte-identical
  `detail="user_not_allowlisted"` audit entry from cookie 403 and
  bearer 403 paths.
- **(5) Force refresh** — modify the keyring blob's
  `id_token_expires_at` to the past; confirm one `/token` call.
- **(6) Force re-auth** — revoke at myaccount.google.com → confirm
  typed `ReauthRequired` with documented message.

The autonomous half above does not block declaring Phases 2–8
implementation-complete; what remains is the operator-driven
verification matrix Phase 10 is supposed to ship alongside the
runbook. When the user runs each of the deferred sub-items, append
the observed output here under a "Phase 9 user-driven verification"
sub-heading (date + commit hash) so the log stays append-only.


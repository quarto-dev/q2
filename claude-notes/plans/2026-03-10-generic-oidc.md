# Generic OIDC Authentication for quarto-hub

## Overview

The hub server's authentication is currently hardcoded to Google as the OIDC provider. This plan makes the backend generic so that any OIDC-compliant identity provider (Auth0, Azure AD, Okta, Keycloak, etc.) can be used by changing CLI flags — no code changes required.

**Scope**: Backend (Rust) only. The frontend currently uses `@react-oauth/google` and will be updated separately when a new provider is needed. The backend should accept any valid JWT from any configured OIDC provider.

## Current State — Google-Specific Touchpoints

### `crates/quarto-hub/src/auth.rs`
- `GoogleClaims` struct with Google-specific fields (`picture`)
- `build_auth_state()` hardcodes Google JWKS URL (`googleapis.com/oauth2/v3/certs`)
- `build_auth_state()` hardcodes Google issuer (`https://accounts.google.com`)
- `build_auth_state()` hardcodes `Algorithm::RS256`
- `validate_tls_config()` parameter named `google_client_id`

### `crates/quarto-hub/src/server.rs`
- `CSP_WITH_AUTH` hardcodes Google domains (`accounts.google.com`, `fonts.googleapis.com`, `lh3.googleusercontent.com`)
- `AuthCallbackForm` has Google-specific `g_csrf_token` field
- `auth_callback()` validates `g_csrf_token` cookie (Google's redirect CSRF mechanism)
- `AuthMeResponse` includes `picture` (Google-specific, though common in OIDC)
- Error messages reference "Google JWKS decoder"

### `crates/quarto-hub/src/context.rs`
- `GoogleClaims` imported and used in `authenticate_claims()` return type
- Module doc references "Google"

### CLI flags (3 locations)
- `crates/quarto-hub/src/main.rs`: `--google-client-id` / `QUARTO_HUB_GOOGLE_CLIENT_ID`
- `crates/quarto/src/commands/hub.rs`: `google_client_id` field in `HubArgs`
- `crates/quarto/src/main.rs`: `--google-client-id` / `QUARTO_HUB_GOOGLE_CLIENT_ID` in `Commands::Hub`

### Frontend (out of scope for this plan, listed for awareness — except CSRF cookie)
- `hub-client/src/main.tsx`: `GoogleOAuthProvider`
- `hub-client/src/hooks/useAuth.ts`: `useGoogleOneTapLogin`
- `hub-client/src/components/auth/LoginScreen.tsx`: `GoogleLogin`
- `hub-client/src/services/authService.ts`: `googleLogout`

## Design Decisions

### 1. Standard OIDC Claims
Replace `GoogleClaims` with `OidcClaims` using standard OIDC claim names (`sub`, `email`, `email_verified`, `name`, `picture`). These are defined in the [OIDC Core spec](https://openid.net/specs/openid-connect-core-1_0.html#StandardClaims) and supported by all major providers. Use `#[serde(default)]` for optional claims so providers that omit them still work.

**Identity note**: The `sub` claim is only unique within a single issuer. The true user identity is the tuple `(issuer, sub)`. Currently only one provider is supported, so `sub` alone suffices for identity comparisons. If multi-provider support is ever added, all identity lookups must be scoped by issuer to prevent cross-provider `sub` collisions.

### 2. Issuer from CLI, JWKS URL from OIDC Discovery
Replace `--google-client-id` with:
- `--oidc-client-id` (required to enable auth)
- `--oidc-issuer` (optional; defaults to `https://accounts.google.com`)
- `--oidc-image-domains` (optional; comma-separated list of domains allowed in CSP `img-src` for profile pictures; defaults to `lh3.googleusercontent.com`)

Remove `--google-client-id` entirely. There is no `--oidc-jwks-url` flag — the JWKS URL is always discovered by fetching `{issuer}/.well-known/openid-configuration` at startup and reading the `jwks_uri` field. This guarantees the JWKS endpoint is cryptographically bound to the issuer (an operator cannot accidentally misconfigure them independently). The discovery document is fetched once at startup; if the fetch fails, the server refuses to start with a clear error.

This approach follows the [OIDC Discovery spec](https://openid.net/specs/openid-connect-discovery-1_0.html#ProviderConfig). All major OIDC providers (Google, Azure AD, Okta, Auth0, Keycloak) serve this endpoint. Supplying only `--oidc-client-id` works for Google out of the box since the issuer defaults to `https://accounts.google.com`.

### 3. Algorithm Discovery
The `axum-jwt-auth` / `jsonwebtoken` stack already validates the `alg` header against what the JWKS endpoint advertises. Currently we hardcode `RS256` in `Validation::new(Algorithm::RS256)`. Instead, derive the allowed algorithms from the JWKS key metadata. At JWKS fetch time, extract the `alg` field from each JWK and build the allowed algorithm set from those values. This ensures only algorithms actually advertised by the provider are accepted, preventing algorithm confusion attacks. If a JWK has no `alg` field, skip it (RFC 7517 makes `alg` optional but all major OIDC providers include it). If the discovered set is empty (all JWKs omit `alg`), fall back to `RS256` only and log a warning — RS256 is by far the most common OIDC signing algorithm. Note: `Validation::new()` takes a single algorithm; use `Validation::default()` and set `validation.algorithms` to the discovered set.

### 4. CSP Generalization
The current `CSP_WITH_AUTH` hardcodes Google domains. Replace with a generic CSP that:
- Keeps `default-src 'self'`
- Parses `config.issuer` as a `Url`, validates it is HTTPS, and extracts only `scheme://host[:port]` as the origin. Rejects malformed or non-HTTPS issuers at startup. Adds the origin to `script-src`, `connect-src`, and `frame-src`
- Builds `img-src` from `config.image_domains`: `img-src 'self' https://{domain1} https://{domain2} ...`. Defaults to `lh3.googleusercontent.com` if not configured. Validates each domain at startup: must match `[a-zA-Z0-9.-]+` only (bare hostname, no scheme, no path, no whitespace, no semicolons). Reject any domain containing characters outside this set to prevent CSP directive injection (e.g., `evil.com; script-src 'unsafe-inline'` would break the entire policy).

Since CSP construction depends on runtime config, change from a `const` to a function that builds the CSP string from `AuthConfig`.

### 5. Auth Callback Generalization
The `auth_callback` endpoint currently handles Google's specific redirect flow (POST with `credential` + `g_csrf_token`). Different providers have different redirect flows:

**Approach**: Keep the existing `auth_callback` as a generic "credential submission" endpoint that:
1. Accepts a `credential` field (the JWT) via POST form
2. Validates it through the standard JWKS/issuer pipeline
3. Sets the HttpOnly cookie

For CSRF: Replace the Google-specific `g_csrf_token` with the existing `X-Requested-With` check, OR accept a generic `csrf_token` field. Since the callback comes from a provider redirect (cross-origin POST), we can't use `X-Requested-With`. Instead, use the `state` parameter (standard OIDC) stored in a cookie before the redirect, validated on return. However, this adds complexity.

**Decision**: Keep `auth_callback` as-is but mark it as **Google-frontend-specific**. It's tightly coupled to Google's Sign-In library (which controls the POST body and `g_csrf_token` cookie). Non-Google frontends should use `/auth/refresh` instead — it accepts a JWT via JSON POST, validates it through the full JWKS/issuer/allowlist pipeline, sets the HttpOnly cookie, and is protected by the standard `X-Requested-With` CSRF check. When the frontend is eventually updated to a generic OIDC library, `auth_callback` can be removed entirely.

### 6. JWT Cookie Size
The raw JWT is stored as the cookie value. Google tokens are ~1KB, but other providers (e.g., Azure AD) can produce 2-4KB tokens. Browser cookie limits are typically 4096 bytes total (including name, attributes). If a token exceeds this, the browser silently drops the cookie and the user appears unauthenticated.

**Approach**: Log a warning at cookie-set time if the token exceeds 3800 bytes (leaving headroom for cookie metadata). This makes the failure mode visible in server logs rather than a silent auth mystery. If a provider's tokens are genuinely too large, the fix is server-side sessions — but that's a separate effort driven by actual need.

### 7. `email_verified` Handling
Two independent mechanisms, needed for different reasons:

`#[serde(default)]` on `email_verified` in `OidcClaims`. This handles providers (e.g., Azure AD) that omit the claim entirely — the field deserializes as `false` (safe default) rather than failing. Providers that omit the claim will be rejected since `email_verified` is always enforced.

## Work Items

### Phase 1: Backend Core (auth.rs + context.rs)

- [x] Rename `GoogleClaims` to `OidcClaims` in `auth.rs`
  - Keep the same fields (`sub`, `email`, `email_verified`, `name`, `picture`)
  - Make `email_verified` default to `false` via `#[serde(default)]` (safe default; providers that omit it require `--no-require-email-verified`)


- [x] Add OIDC fields to `AuthConfig`
  ```rust
  pub struct AuthConfig {
      pub client_id: String,
      pub issuer: String,              // e.g. "https://accounts.google.com"
      pub image_domains: Vec<String>,  // CSP img-src domains for profile pictures
      pub allowed_emails: Option<Vec<String>>,
      pub allowed_domains: Option<Vec<String>>,
  }
  ```
  Note: no `jwks_url` field — it is discovered at startup from the issuer's `/.well-known/openid-configuration`.

- [x] Add `discover_jwks_url(issuer: &str) -> Result<String>` function
  - Validate that `issuer` is a well-formed HTTPS URL before making any network request (reject HTTP, malformed URLs)
  - Fetch `{issuer}/.well-known/openid-configuration`
  - Parse the JSON response and extract the `jwks_uri` field
  - Validate that `jwks_uri` is an HTTPS URL
  - Validate that the `issuer` field in the discovery document matches `config.issuer` (prevents issuer spoofing)
  - On fetch failure, return a clear error with the URL that was attempted
  - Use a short timeout (e.g., 10 seconds) to fail fast at startup

- [x] Update `build_auth_state()` to use `AuthConfig` fields + discovery
  - Take `&AuthConfig` instead of `&str` (client_id)
  - Call `discover_jwks_url(&config.issuer)` to get the JWKS URL
  - Use `config.issuer` instead of hardcoded Google issuer
  - Extract allowed algorithms from JWKS key `alg` fields; set `validation.algorithms` to that discovered set; if empty, fall back to `RS256` with a warning
  - Set `validation.set_audience(&[config.client_id])` to enforce `aud` claim validation (prevents confused deputy attacks)
  - Ensure `exp` validation is enabled (on by default) and explicitly set `validation.validate_nbf = true` (`nbf` defaults to `false` in `jsonwebtoken`). Leeway of 60 seconds (`validation.leeway = 60`, already the library default)
  - **Not needed**: On-demand JWKS refetch on unknown `kid` was evaluated and rejected. OIDC providers overlap old and new keys during rotation by design, so the library's 1-hour periodic refresh is always sufficient. Emergency rotation (key compromise) is extremely rare, and in that scenario the compromised key's tokens should be rejected anyway — users must re-authenticate regardless. Adding retry logic would be over-engineering for a near-zero probability case.

- [x] Verify `check_allowlists()` always enforces `email_verified` (no opt-out flag)

- [x] Update `validate_tls_config()` to use generic parameter name
  - `google_client_id: Option<&str>` → `oidc_client_id: Option<&str>` (internal only, no user-facing change)

- [x] Update `context.rs` to use `OidcClaims` instead of `GoogleClaims`
  - `authenticate_claims()` return type
  - Import path

- [x] Update all tests in `auth.rs` to use `OidcClaims` and new `AuthConfig` fields

### Phase 2: Server (server.rs)

- [x] Replace `CSP_WITH_AUTH` const with `fn build_csp(config: &AuthConfig) -> String`
  - Parse `config.issuer` as a `Url`, validate HTTPS, extract origin (scheme + host + port)
  - Build CSP dynamically:
    - `script-src 'self' {issuer_origin}`
    - `connect-src 'self' {issuer_origin}`
    - `frame-src {issuer_origin}`
    - `style-src 'self' 'unsafe-inline'` (drop Google Fonts — provider-agnostic)
    - `font-src 'self'` (drop Google Fonts CDN)
    - `img-src 'self' https://{domain}...` (from `config.image_domains`)

- [x] Add JWT size warning in `build_auth_cookie()`: if token length > 3800 bytes, log a warning about potential browser cookie size limits

- [x] Keep `AuthCallbackForm` field as `g_csrf_token` (set by Google's Sign-In library, not our code)

- [x] Update `auth_callback()` handler
  - Keep `g_csrf_token` cookie name (unchanged)
  - Add doc comment marking this endpoint as Google-frontend-specific
  - Add doc comment pointing to `/auth/refresh` as the generic credential submission endpoint
  - Update error messages to be provider-agnostic

- [x] Update `auth_me()` — no changes needed (already returns generic fields)

- [x] Update `build_router()` to use `build_csp()` instead of `CSP_WITH_AUTH`

- [x] Update error messages in `build_router()` ("Google JWKS decoder" → "OIDC JWKS decoder")

- [x] Update server.rs tests to remove Google-specific assertions (CSP test)

### Phase 3: CLI Flags (3 files)

- [x] `crates/quarto-hub/src/main.rs`: Replace `--google-client-id` with generic OIDC flags
  ```
  --oidc-client-id <ID>              OIDC client ID (enables auth)
  --oidc-issuer <URL>                Expected JWT issuer (default: https://accounts.google.com)
  --oidc-image-domains <DOMAINS>     Comma-separated domains for CSP img-src (default: lh3.googleusercontent.com)
  ```
  - Remove `--google-client-id` flag and `QUARTO_HUB_GOOGLE_CLIENT_ID` env var entirely
  - Add env vars: `OIDC_CLIENT_ID`, `OIDC_ISSUER`, `OIDC_IMAGE_DOMAINS`
  - No `--oidc-jwks-url` flag; JWKS URL is discovered from `{issuer}/.well-known/openid-configuration`

- [x] `crates/quarto/src/main.rs`: Replace `--google-client-id` with the same generic OIDC flags (`--oidc-client-id`, `--oidc-issuer`); remove `QUARTO_HUB_GOOGLE_CLIENT_ID`

- [x] `crates/quarto/src/commands/hub.rs`: Update `HubArgs` struct — remove `google_client_id`, add `oidc_client_id`, `oidc_issuer`, `oidc_image_domains`; update `run_hub()` to build `AuthConfig`

- [x] Update `validate_tls_config()` calls to pass generic client ID

**Note**: The `validate_tls_config()` parameter rename (Phase 1) and CLI flag changes (Phase 3) must be done together — both `main.rs` files call `validate_tls_config()` with the old parameter name. Implement Phases 1–3 before expecting compilation to succeed.

### Phase 4: Documentation & Module Docs

- [x] Update module doc comment in `auth.rs` (currently says "Google OAuth2")
- [x] Update module doc comment in `context.rs`
- [x] Update doc comments in `server.rs`:
  - `auth_callback()` doc: "Google OAuth2 redirect callback" → generic
  - `auth_refresh()` doc: "Validate a fresh Google JWT" / "Google One Tap" → generic
  - `AUTH_COOKIE_MAX_AGE` comment: "matches Google ID token lifetime" → generic
- [x] Update code comments in `auth.rs`:
  - `check_allowlists()` line 56: "Google normalizes emails to lowercase" → provider-agnostic
  - `build_auth_state()` line 114: "Fetch the initial JWKS keys from Google" → "Discover JWKS URL and fetch initial keys"
- [x] Update `validate_tls_config()` error message text: `--google-client-id requires TLS` → `--oidc-client-id requires TLS`
- [x] Update `--help` text to be provider-agnostic where appropriate

### Phase 5: Tests

- [x] Add unit test: `OidcClaims` deserializes Google-style JWT payload
- [x] Add unit test: `OidcClaims` deserializes Azure AD-style JWT payload (no `picture`, `email_verified` absent)
- [x] Add unit test: `OidcClaims` without `email_verified` claim defaults to `false` and is rejected
- [x] Add unit test: `build_csp()` generates correct CSP for Google issuer
- [x] Add unit test: `build_csp()` generates correct CSP for custom issuer (e.g. `https://login.microsoftonline.com/...`)
- [x] Add unit test: `build_csp()` includes custom image domains in `img-src`
- [x] Add unit test: `build_csp()` uses default Google image domain when `image_domains` is empty
- [x] Add unit test: `AuthCallbackForm` deserializes with `g_csrf_token` field
- [ ] Add unit test: JWT with wrong `aud` claim is rejected — requires live JWKS (integration test)
- [ ] Add unit test: `discover_jwks_url()` parses a valid discovery document and extracts `jwks_uri` — requires mock HTTP server or live network
- [ ] Add unit test: `discover_jwks_url()` rejects a discovery document where `issuer` doesn't match — requires mock HTTP server
- [ ] Add unit test: `discover_jwks_url()` rejects a non-HTTPS `jwks_uri` — requires mock HTTP server
- [x] Add unit test: `discover_jwks_url()` rejects an HTTP (non-HTTPS) issuer before fetching
- [x] Add unit test: `discover_jwks_url()` rejects a malformed issuer URL before fetching
- [x] Add unit test: `build_csp()` rejects a non-HTTPS issuer
- [x] Add unit test: `build_csp()` rejects a malformed issuer URL
- [x] Add unit test: image domain validation rejects CSP injection (`evil.com; script-src 'unsafe-inline'`)
- [x] Add unit test: image domain validation rejects domains with whitespace
- [x] Add unit test: image domain validation rejects domains with scheme prefix (`https://example.com`)
- [x] Add unit test: image domain validation accepts valid domains (`lh3.googleusercontent.com`, `cdn.example.co.uk`)
- [x] Verify existing tests pass with renamed types

## Migration Guide (for operators)

### Before (Google-only)
```bash
hub --google-client-id <ID>
```

### After (Google — issuer and JWKS URL default to Google)
```bash
hub --oidc-client-id <ID>
```

### After (Custom OIDC provider)
```bash
hub \
  --oidc-client-id <CLIENT_ID> \
  --oidc-issuer https://your-provider.com \
  --oidc-image-domains avatars.your-provider.com,cdn.your-provider.com
```
The JWKS URL is automatically discovered from `https://your-provider.com/.well-known/openid-configuration` at startup. Profile picture domains default to `lh3.googleusercontent.com` if `--oidc-image-domains` is not set.

## Non-Goals

- **Frontend changes**: The frontend remains Google-specific. When a new provider is needed, the frontend will be updated separately (likely replacing `@react-oauth/google` with a generic OIDC client library).
- **Multi-provider support**: Only one OIDC provider at a time. Supporting multiple simultaneous providers would require more complex routing and is not needed now.
- **OIDC Discovery beyond JWKS**: We use `/.well-known/openid-configuration` to discover the `jwks_uri`, but we do not auto-discover other fields (e.g., `authorization_endpoint`, `token_endpoint`, `supported_scopes`). The backend only needs the JWKS URL for token validation; other discovery fields are relevant to the frontend's login flow.

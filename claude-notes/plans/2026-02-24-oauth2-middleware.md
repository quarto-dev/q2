# OAuth2 Middleware Implementation Plan

*2026-02-24*

Google OAuth2 authentication for quarto-hub. Design doc: `claude-notes/plans/2026-02-24-oauth2-middleware-design.md`

## Phase 1: Server Auth Module (Rust — `crates/quarto-hub`)

- [x] Add `axum-jwt-auth` and `jsonwebtoken` dependencies to Cargo.toml
- [x] Create `src/auth.rs`: `AuthConfig`, `GoogleClaims`, `AuthState`, `check_allowlists()`, `build_auth_state()`
- [x] Add `auth_config: Option<AuthConfig>` to `HubConfig`, `OnceLock<AuthState>` to `HubContext`
- [x] Add `HubContext::authenticate()` and `HubContext::auth_config()` methods
- [x] Add `HubContext::set_auth_state()` method
- [x] Update `server.rs`: `build_router` becomes async, initializes auth state
- [x] REST handlers: extract Bearer token from header, call `ctx.authenticate()`
- [x] WebSocket handler: extract `id_token` from query param, call `ctx.authenticate()`
- [x] Add `RedactedMakeSpan` to prevent token logging
- [x] Add `validate_tls_config()` check at startup
- [x] Add `unauthorized()` JSON error helper
- [x] Update `run_server()` to accept auth config and call validation
- [x] Add unit tests for `check_allowlists()` (9 tests)

## Phase 2: CLI Flags (Rust — `crates/quarto` + `crates/quarto-hub`)

- [x] Add `--google-client-id`, `--behind-tls-proxy`, `--allow-insecure-auth` flags to hub binary
- [x] Add `--allowed-emails`, `--allowed-domains` flags to hub binary
- [x] Add same flags to `quarto hub` subcommand in CLI
- [x] Wire flags through to `HubConfig` → `AuthConfig`

## Phase 3: Browser Client (TypeScript — `hub-client`)

- [x] Install `@react-oauth/google`
- [x] Add `VITE_GOOGLE_CLIENT_ID` env var type definition
- [x] Create `src/services/authService.ts` (store/get/clear auth, JWT decode)
- [x] Create `src/hooks/useAuth.ts` (auth state, expiry monitoring)
- [x] Create `src/components/auth/LoginButton.tsx`
- [x] Wrap app in `GoogleOAuthProvider` (conditional on env var)
- [x] Add auth gate to `App.tsx`
- [x] Append ID token to WebSocket URL in `automergeSync.ts` connect()

## Phase 4: CLI Client Auth (Rust — `crates/quarto`)

- [x] Add `yup-oauth2` and `dirs` dependencies
- [x] Create `crates/quarto/src/auth.rs` (get_id_token, clear_tokens, status)
- [x] Add `quarto auth login/logout/status` subcommands
- [ ] Append ID token to WebSocket URL when connecting as client (deferred: no client connect command exists yet)

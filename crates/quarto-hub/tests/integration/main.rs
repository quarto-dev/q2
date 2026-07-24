//! Consolidated integration-test binary for quarto-hub.
//!
//! One `integration` binary per crate (see
//! `.claude/rules/integration-tests.md`): add new integration tests as
//! `pub mod <name>;` here, never as top-level `tests/<name>.rs` files.
//! Shared fixtures (mock OIDC provider, test hub, tracing capture)
//! live in [`support`].

pub mod auth_bearer;
pub mod session_auth;
pub mod support;

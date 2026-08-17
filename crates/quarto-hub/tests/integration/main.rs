//! Consolidated integration-test binary for quarto-hub.
//!
//! One `integration` binary per crate (see
//! `.claude/rules/integration-tests.md`): add new integration tests as
//! `pub mod <name>;` here, never as top-level `tests/<name>.rs` files.
//! Keep the module list alphabetized. Shared fixtures (mock OIDC
//! provider, test hub, tracing capture) live in [`support`].

pub mod admin_collect_lifecycle;
pub mod admin_doc_id_recovery;
pub mod admin_scan_real_store;
pub mod auth_bearer;
pub mod login_nonce;
pub mod session_auth;
pub mod support;

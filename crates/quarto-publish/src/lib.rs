//! Publishing infrastructure for Quarto 2.
//!
//! Provides the `PublishProvider` trait, a registry of providers, and a
//! `gh-pages` provider implementation. The `quarto` binary's
//! `publish` subcommand drives this crate.
//!
//! See `claude-notes/plans/2026-05-03-publish-command-and-gh-pages.md`
//! for the design.

pub mod cli;
pub mod common;
pub mod config;
pub mod execute;
pub mod gh_pages;
pub mod host;
pub mod provider;
pub mod renderer;
pub mod types;

pub use execute::{ExecuteArgs, execute, execute_with_builtins};

pub use host::{NativeHost, PublishHost};
pub use provider::{ProviderRegistry, PublishProvider};
pub use renderer::PublishRenderer;
pub use types::{
    AccountToken, PublishAction, PublishDestination, PublishError, PublishEvent, PublishFiles,
    PublishInput, PublishKind, PublishOutcome, PublishRecord, PublishSummary, PublishUx,
};

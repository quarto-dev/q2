/*
 * quarto-test/src/assertions/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Assertion trait and implementations for verifying rendered output.
 */

//! Assertion system for verifying rendered document output.

mod css_regex;
mod file_exists;
mod file_regex;
mod html_elements;
mod no_errors;
mod prints_message;
mod should_error;

pub use css_regex::EnsureCssRegexMatches;
pub use file_exists::{FileExists, FolderExists, PathDoesNotExist};
pub use file_regex::EnsureFileRegexMatches;
pub use html_elements::EnsureHtmlElements;
pub use no_errors::{NoErrors, NoErrorsOrWarnings};
pub use prints_message::PrintsMessage;
pub use should_error::ShouldError;

use std::fmt::Debug;
use std::path::PathBuf;

/// Log level for captured messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// A captured log message from the render process.
#[derive(Debug, Clone)]
pub struct LogMessage {
    pub level: LogLevel,
    pub message: String,
}

/// Context provided to assertions for verification.
#[derive(Debug)]
pub struct VerifyContext {
    /// Path to the rendered output file (may not exist if render failed).
    pub output_path: PathBuf,
    /// Path to the original input file.
    pub input_path: PathBuf,
    /// Format that was rendered.
    pub format: String,
    /// Error from rendering, if any.
    pub render_error: Option<String>,
    /// Log messages captured during rendering.
    pub messages: Vec<LogMessage>,
}

/// Trait for assertions that verify rendered output.
pub trait Assertion: Debug + Send + Sync {
    /// Human-readable name of this assertion.
    fn name(&self) -> &str;

    /// Verify the assertion against the rendered output.
    ///
    /// Returns `Ok(())` if the assertion passes, or an error describing
    /// the failure.
    fn verify(&self, context: &VerifyContext) -> anyhow::Result<()>;
}

//! Error code catalog and lookup.
//!
//! This module provides access to the centralized error catalog, which maps
//! error codes (like "Q-1-1") to their metadata (title, message template, docs URL, etc.).

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata for an error code.
///
/// Each entry in the error catalog describes a specific error code,
/// including its subsystem, title, default message, and documentation URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorCodeInfo {
    /// Subsystem name (e.g., "yaml", "markdown", "engine")
    pub subsystem: String,

    /// Short title for the error
    pub title: String,

    /// Default message template (may include placeholders)
    pub message_template: String,

    /// URL to documentation (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs_url: Option<String>,

    /// When this error was introduced (version)
    pub since_version: String,
}

/// Global error catalog, loaded lazily from JSON at compile time.
///
/// The catalog is loaded from `error_catalog.json` using `include_str!()`,
/// which embeds the JSON at compile time. This means no runtime file I/O.
///
/// # Panics
///
/// Panics if the embedded JSON is invalid. This should only happen during
/// development if someone manually edits the catalog incorrectly.
pub static ERROR_CATALOG: Lazy<HashMap<String, ErrorCodeInfo>> = Lazy::new(|| {
    let json_data = include_str!("../error_catalog.json");
    serde_json::from_str(json_data).expect("Invalid error catalog JSON - this is a bug in Quarto")
});

/// Look up error code information.
///
/// Returns `None` if the error code is not found in the catalog.
///
/// # Example
///
/// ```
/// use quarto_error_reporting::catalog::get_error_info;
///
/// if let Some(info) = get_error_info("Q-0-1") {
///     println!("Error: {} - {}", info.title, info.message_template);
/// }
/// ```
pub fn get_error_info(code: &str) -> Option<&ErrorCodeInfo> {
    ERROR_CATALOG.get(code)
}

/// Get documentation URL for an error code.
///
/// Returns `None` if the error code is not found or has no documentation URL.
///
/// # Example
///
/// ```
/// use quarto_error_reporting::catalog::get_docs_url;
///
/// if let Some(url) = get_docs_url("Q-0-1") {
///     println!("See {} for more information", url);
/// }
/// ```
pub fn get_docs_url(code: &str) -> Option<&str> {
    ERROR_CATALOG
        .get(code)
        .and_then(|info| info.docs_url.as_deref())
}

/// Get the subsystem name for an error code.
///
/// Returns `None` if the error code is not found.
///
/// # Example
///
/// ```
/// use quarto_error_reporting::catalog::get_subsystem;
///
/// assert_eq!(get_subsystem("Q-0-1"), Some("internal"));
/// ```
pub fn get_subsystem(code: &str) -> Option<&str> {
    ERROR_CATALOG.get(code).map(|info| info.subsystem.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_loads() {
        // Just accessing ERROR_CATALOG will trigger loading
        // If the JSON is invalid, this will panic
        assert!(!ERROR_CATALOG.is_empty());
    }

    #[test]
    fn test_internal_error_exists() {
        let info = get_error_info("Q-0-1");
        assert!(info.is_some());

        let info = info.unwrap();
        assert_eq!(info.subsystem, "internal");
        assert_eq!(info.title, "Internal Error");
        assert!(info.docs_url.is_some());
    }

    #[test]
    fn test_get_docs_url() {
        let url = get_docs_url("Q-0-1");
        assert!(url.is_some());
        assert!(url.unwrap().starts_with("https://quarto.org/docs/errors/"));
    }

    #[test]
    fn test_get_subsystem() {
        assert_eq!(get_subsystem("Q-0-1"), Some("internal"));
        assert_eq!(get_subsystem("Q-999-999"), None); // quarto-error-code-audit-ignore
    }

    #[test]
    fn test_nonexistent_code() {
        assert!(get_error_info("Q-999-999").is_none()); // quarto-error-code-audit-ignore
        assert!(get_docs_url("Q-999-999").is_none()); // quarto-error-code-audit-ignore
    }
}

//! Version handling for Quarto
//!
//! This module implements the versioning strategy where:
//! - Cargo.toml version: 0.x.y (idiomatic Rust, signals instability)
//! - CLI reported version: 99.9.9-dev (for extension compatibility)
//!
//! When the crate version is 2.x.y or higher, the CLI will report the actual version.

/// Development version used for compatibility with extensions
const DEV_VERSION: &str = "99.9.9-dev";

/// Get the version string that should be reported by the CLI
///
/// During development (version 0.x.y), this returns "99.9.9-dev" to ensure
/// compatibility with all existing Quarto extensions while clearly indicating
/// this is a development build.
///
/// Once released as 2.0.0+, this will return the actual version.
pub fn cli_version() -> &'static str {
    let cargo_version = env!("CARGO_PKG_VERSION");

    if cargo_version.starts_with("0.") {
        DEV_VERSION
    } else {
        cargo_version
    }
}

/// Get the Cargo package version (for internal use)
pub fn cargo_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_version() {
        let version = cli_version();
        // During development, should be dev version
        assert!(
            version == DEV_VERSION || version.starts_with("2."),
            "CLI version should be either dev version or 2.x.y"
        );
    }

    #[test]
    fn test_cargo_version() {
        let version = cargo_version();
        assert!(!version.is_empty(), "Cargo version should not be empty");
    }
}

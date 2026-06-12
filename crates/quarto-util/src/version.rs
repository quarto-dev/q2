//! Version handling for Quarto
//!
//! The CLI reports the workspace Cargo.toml version (e.g. "0.1.0").
//!
//! History: until 2026-06-12 the CLI reported a "99.9.9-dev"
//! placeholder while the crate version was 0.x, so extensions with
//! minimum-quarto-version checks would always pass against dev builds.
//! With binary releases (bd-c6l13j79) the version must be verifiable
//! against the release tag, and the Lua-side `quarto.version` already
//! reported the real {0,1,0}; Carlos chose the real version and
//! accepted the consequences for extension minimum-version checks.

/// Get the version string that should be reported by the CLI.
pub fn cli_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Get the Cargo package version (for internal use)
pub fn cargo_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_version_is_the_cargo_version() {
        // Decision 2026-06-12 (bd-c6l13j79): the CLI reports the real
        // workspace version (e.g. "0.1.0"), not the old 99.9.9-dev
        // placeholder, so release artifacts are verifiable against
        // their tag. Carlos accepted the minimum-quarto-version
        // consequences for extensions.
        assert_eq!(cli_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_cargo_version() {
        let version = cargo_version();
        assert!(!version.is_empty(), "Cargo version should not be empty");
    }
}

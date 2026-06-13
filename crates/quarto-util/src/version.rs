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

/// The display form for the q2 CLI's `--version` value: clap renders
/// `"<command name> <version>"`, so with command name "q2" this yields
/// `q2 (quarto 2) 0.1.0` — "(quarto 2)" disambiguates from TS Quarto
/// (bd-qyjsncfx). `&'static str` via `concat!` because the workspace
/// clap has no `string` feature (owned values). The release workflow
/// parses the LAST whitespace token of the output as the bare version;
/// anything appended here must keep the version last.
pub fn cli_version_display() -> &'static str {
    concat!("(quarto 2) ", env!("CARGO_PKG_VERSION"))
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

    #[test]
    fn display_form_is_the_decorated_cli_version() {
        // The concat! in cli_version_display() and the env! in
        // cli_version() must never drift apart.
        assert_eq!(
            cli_version_display(),
            format!("(quarto 2) {}", cli_version())
        );
        // Release-workflow contract: the bare version is the last token.
        assert_eq!(
            cli_version_display().split_whitespace().last(),
            Some(cli_version())
        );
    }
}

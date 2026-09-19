//! Version handling for Quarto
//!
//! The CLI reports the workspace Cargo.toml version (e.g. "0.1.0"),
//! unless a **nightly build override** was supplied at build time.
//!
//! History: until 2026-06-12 the CLI reported a "99.9.9-dev"
//! placeholder while the crate version was 0.x, so extensions with
//! minimum-quarto-version checks would always pass against dev builds.
//! With binary releases (bd-c6l13j79) the version must be verifiable
//! against the release tag, and the Lua-side `quarto.version` already
//! reported the real {0,1,0}; Carlos chose the real version and
//! accepted the consequences for extension minimum-version checks.
//!
//! # Nightly builds (bd-p4ljdp2e)
//!
//! The Nightly workflow builds `main` between releases. `main` carries
//! the *last released* version (the bump lands at release time), a
//! nightly must not commit to `main`, and the build stays `--locked`, so
//! the nightly's version cannot come from `Cargo.toml`. Instead the
//! workflow sets [`VERSION_OVERRIDE_ENV`] (`QUARTO_VERSION_OVERRIDE`) to
//! the full string it publishes under — e.g. `0.33.0-nightly.20260919`,
//! the *next* minor with a dated prerelease so it sorts above the release
//! it contains — and every consumer of the "quarto version" reads it
//! through [`cli_version`]. Unset (every local and release build), the
//! behaviour is byte-identical to before. A malformed override fails the
//! build (compile-time assertion), not the release verify gate.
//! Plan: claude-notes/plans/2026-09-19-nightly-release-workflow.md.

use std::sync::OnceLock;

/// The build-time environment variable that overrides the reported
/// version (nightly builds only). Read via `option_env!`, so it takes
/// effect when *this crate* is compiled.
pub const VERSION_OVERRIDE_ENV: &str = "QUARTO_VERSION_OVERRIDE";

const VERSION_OVERRIDE: Option<&str> = option_env!("QUARTO_VERSION_OVERRIDE");

// Fail the build, not the release, on a malformed override: the verify
// gate compares `--version` to the workflow's version string, but a typo
// there should never get as far as a 90-minute build matrix.
const _: () = assert!(
    override_is_well_formed(VERSION_OVERRIDE),
    "QUARTO_VERSION_OVERRIDE must be a bare version such as 0.33.0-nightly.20260919 \
     (three numeric components, optional -prerelease; no leading 'v', no '+' metadata)"
);

const fn override_is_well_formed(o: Option<&str>) -> bool {
    match o {
        None => true,
        Some(s) => is_plausible_version(s),
    }
}

/// Whether `s` has the shape of a version this project publishes:
/// `MAJOR.MINOR.PATCH` with an optional `-prerelease` made of
/// `[0-9A-Za-z.-]` identifiers. Deliberately rejects a leading `v` (that
/// is the *tag* spelling) and `+build` metadata (a `+` would land in
/// asset filenames and download URLs). `const` so the override can be
/// checked at compile time.
pub const fn is_plausible_version(s: &str) -> bool {
    let b = s.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut component = 0;
    while component < 3 {
        let start = i;
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
        component += 1;
        if component < 3 {
            if i >= n || b[i] != b'.' {
                return false;
            }
            i += 1;
        }
    }
    if i == n {
        return true;
    }
    if b[i] != b'-' {
        return false;
    }
    i += 1;
    if i == n {
        return false; // "0.33.0-": empty prerelease
    }
    let mut prev_dot = true; // a leading '.' is an empty identifier
    while i < n {
        let c = b[i];
        if c == b'.' {
            if prev_dot {
                return false; // ".." or "-."
            }
            prev_dot = true;
        } else if c.is_ascii_alphanumeric() || c == b'-' {
            prev_dot = false;
        } else {
            return false;
        }
        i += 1;
    }
    !prev_dot // no trailing '.'
}

/// The version a build reports: the override when one was supplied at
/// build time, else the manifest version. Pure so it can be tested
/// without rebuilding.
pub fn effective_version(cargo: &'static str, nightly: Option<&'static str>) -> &'static str {
    nightly.unwrap_or(cargo)
}

/// The build-time override, if this binary was built with one.
pub fn version_override() -> Option<&'static str> {
    VERSION_OVERRIDE
}

/// Get the version string that should be reported by the CLI — and by
/// everything else that names "the Quarto version" (the HTML
/// `generator` meta tag, the project cache key, template variables).
pub fn cli_version() -> &'static str {
    effective_version(env!("CARGO_PKG_VERSION"), VERSION_OVERRIDE)
}

/// The display form for the q2 CLI's `--version` value: clap renders
/// `"<command name> <version>"`, so with command name "q2" this yields
/// `q2 (quarto 2) 0.1.0` — "(quarto 2)" disambiguates from TS Quarto
/// (bd-qyjsncfx). Built once into a `static` because the workspace clap
/// has no `string` feature (it wants `&'static str`). The release
/// workflow parses the LAST whitespace token of the output as the bare
/// version; anything appended here must keep the version last.
pub fn cli_version_display() -> &'static str {
    static DISPLAY: OnceLock<String> = OnceLock::new();
    DISPLAY.get_or_init(|| format!("(quarto 2) {}", cli_version()))
}

/// The Cargo manifest version, ignoring any nightly override. For the
/// few places that must match the workspace as written rather than the
/// version this build was published under.
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
        //
        // Nightly builds (bd-p4ljdp2e) may override it at build time;
        // in that case the CLI reports the override verbatim. A test
        // build normally has no override, so this is the plain case.
        assert_eq!(
            cli_version(),
            version_override().unwrap_or(env!("CARGO_PKG_VERSION"))
        );
    }

    // --- nightly version override (bd-p4ljdp2e) ---------------------------

    #[test]
    fn effective_version_is_the_cargo_version_without_an_override() {
        assert_eq!(effective_version("0.32.0", None), "0.32.0");
    }

    #[test]
    fn effective_version_is_the_override_verbatim() {
        // A full override, not a suffix: the nightly base is the NEXT
        // minor, which Cargo.toml does not know about.
        assert_eq!(
            effective_version("0.32.0", Some("0.33.0-nightly.20260919")),
            "0.33.0-nightly.20260919"
        );
    }

    #[test]
    fn cargo_version_is_always_the_manifest_version() {
        // The manifest version is still reachable for the things that
        // must not follow the override (e.g. matching the workspace).
        assert_eq!(cargo_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn plausible_version_accepts_release_and_prerelease_shapes() {
        for v in [
            "0.32.0",
            "0.33.0-nightly.20260919",
            "1.0.0-rc.1",
            "10.20.30",
            "0.4.1-rc.1",
        ] {
            assert!(is_plausible_version(v), "{v:?} should be accepted");
        }
    }

    #[test]
    fn plausible_version_rejects_malformed_strings() {
        for v in [
            "",
            "v0.33.0",                 // the tag form, not the version
            "0.33",                    // not three components
            "0.33.0.1",                // too many components
            "0.33.0-",                 // empty prerelease
            "0.33.0-nightly 20260919", // whitespace
            "0.33.0+nightly.20260919", // build metadata: '+' in filenames/URLs
            "nightly",
            "0.a.0",
            "-0.33.0",
        ] {
            assert!(!is_plausible_version(v), "{v:?} should be rejected");
        }
    }

    #[test]
    fn override_env_var_name_is_the_documented_one() {
        // The workflow sets this exact name; the runbook documents it.
        assert_eq!(VERSION_OVERRIDE_ENV, "QUARTO_VERSION_OVERRIDE");
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

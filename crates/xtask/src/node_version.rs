//! Node toolchain check — makes the repo's Node pin *enforced* rather than
//! advisory.
//!
//! The pin lives in two places that tooling reads: `engines.node` in the root
//! `package.json` (npm; this module) and `.nvmrc` (version managers such as
//! fnm/nvm/mise). Neither does anything on its own: `engines` only warns at
//! `npm install` unless `engine-strict` is set, and `.nvmrc` needs a version
//! manager to be installed. That is how the May 2026 pin (`ca6d47c8`) was
//! silently undone by a `brew upgrade` that relinked `node` to a newer major,
//! surfacing months later as 23 unrelated-looking vitest failures
//! (bd-lh30hlvd). This module turns the drift into an immediate, named
//! failure at the two places people actually hit it: `cargo xtask verify`
//! (fails, with an explicit escape hatch) and `cargo xtask dev-setup` (warns
//! with install hints).
//!
//! Design: the process/IO edge (`node --version`, reading `package.json`) is
//! kept thin; everything that decides or explains is a pure function with
//! unit tests. The range is interpreted with `nodejs-semver`, a Rust
//! implementation of npm's own range grammar (node-semver), so
//! `engines.node` means here exactly what it means to npm — including the
//! space-separated `>=24 <25` form that Cargo's `semver` crate rejects. A
//! range that does not parse is an *error*, never a silent pass, so a future
//! `engines` edit cannot disable the check by accident.

use anyhow::{Context, Result, bail};
use nodejs_semver::{Range, Version};
use std::path::Path;

/// Environment variable that lets a deliberate experiment run `cargo xtask
/// verify` against a Node outside the pinned range. Set to `1` or `true`.
pub const ALLOW_MISMATCH_ENV: &str = "Q2_ALLOW_NODE_MISMATCH";

/// Where the pin is documented for humans.
pub const DOCS_PATH: &str = "claude-notes/instructions/node-version.md";

/// The Node requirement declared by the repo.
#[derive(Debug, Clone)]
pub struct NodeRequirement {
    /// Parsed range (npm grammar).
    pub range: Range,
    /// The range exactly as written in `package.json`, for messages.
    pub raw: String,
    /// Human-readable provenance of the range, e.g. `package.json`.
    pub source: String,
}

/// Result of comparing the installed Node against the requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The installed Node is inside the pinned range.
    Satisfied { found: Version },
    /// A Node was found, but outside the pinned range.
    Mismatch { found: Version },
    /// No usable `node` on `PATH` (missing, failed to run, or unparsable
    /// `--version` output). `detail` says which.
    NotFound { detail: String },
}

/// What `verify` should do with an [`Outcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    /// Continue (possibly after printing a warning).
    Proceed,
    /// Stop before running anything that needs Node.
    Fail,
}

/// Parse the output of `node --version` (`v24.20.0\n`) into a [`Version`].
/// A missing leading `v` is tolerated.
pub fn parse_node_version(output: &str) -> Result<Version> {
    let trimmed = output.trim();
    let bare = trimmed.strip_prefix('v').unwrap_or(trimmed);
    Version::parse(bare).with_context(|| {
        format!("cannot parse `node --version` output {output:?} as a semantic version")
    })
}

/// Extract and parse `engines.node` from the text of a `package.json`.
///
/// Errors when the field is absent or the range is not interpretable — both
/// are configuration bugs that must surface, not pass.
pub fn engines_node_requirement(package_json: &str) -> Result<NodeRequirement> {
    let value: serde_json::Value =
        serde_json::from_str(package_json).context("package.json is not valid JSON")?;
    let raw = value
        .get("engines")
        .and_then(|engines| engines.get("node"))
        .and_then(|node| node.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "package.json declares no `engines.node` range — the Node pin is missing"
            )
        })?;
    let range = Range::parse(raw).with_context(|| {
        format!(
            "cannot interpret the engines.node range {raw:?} in package.json \
             (expected npm range syntax, e.g. `^24.0.0`, `>=24 <25`, `24.x`)"
        )
    })?;
    Ok(NodeRequirement {
        range,
        raw: raw.to_string(),
        source: "package.json".to_string(),
    })
}

/// Compare a probe result (`Ok(version)` from `node --version`, or
/// `Err(detail)` describing why none was obtained) against the requirement.
pub fn outcome(requirement: &NodeRequirement, probe: Result<Version, String>) -> Outcome {
    match probe {
        Ok(found) if requirement.range.satisfies(&found) => Outcome::Satisfied { found },
        Ok(found) => Outcome::Mismatch { found },
        Err(detail) => Outcome::NotFound { detail },
    }
}

/// Decide whether `verify` proceeds. `allow_mismatch` is the parsed
/// [`ALLOW_MISMATCH_ENV`] escape hatch.
pub fn enforcement(outcome: &Outcome, allow_mismatch: bool) -> Enforcement {
    match outcome {
        Outcome::Satisfied { .. } => Enforcement::Proceed,
        Outcome::Mismatch { .. } | Outcome::NotFound { .. } if allow_mismatch => {
            Enforcement::Proceed
        }
        Outcome::Mismatch { .. } | Outcome::NotFound { .. } => Enforcement::Fail,
    }
}

/// Interpret the raw value of [`ALLOW_MISMATCH_ENV`]: `1` or `true`
/// (case-insensitive) enable the escape hatch; anything else, including an
/// unset variable, does not.
pub fn allow_mismatch_from_env(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some(v) if v == "1" || v.eq_ignore_ascii_case("true")
    )
}

/// Human-readable explanation of an outcome: what was found, what is
/// required and where that is declared, and — for anything but
/// `Satisfied` — how to fix it and how to override it.
pub fn describe(requirement: &NodeRequirement, outcome: &Outcome) -> String {
    let required = format!("engines.node {} ({})", requirement.raw, requirement.source);
    match outcome {
        Outcome::Satisfied { found } => format!("Node v{found} satisfies {required}"),
        Outcome::Mismatch { found } => format!(
            "Node v{found} does not satisfy {required}.\n{}",
            remedy(requirement)
        ),
        Outcome::NotFound { detail } => format!(
            "Node not found: {detail}. This repository requires {required}.\n{}",
            remedy(requirement)
        ),
    }
}

/// The fix-it paragraph shared by the non-`Satisfied` messages.
fn remedy(requirement: &NodeRequirement) -> String {
    format!(
        "  This repository pins Node to {raw} (package.json `engines.node`, mirrored in `.nvmrc`).\n  \
         With fnm, mise, or nvm installed, `.nvmrc` selects the pinned version automatically;\n  \
         see {DOCS_PATH} for setup (and for the Homebrew relink trap).\n  \
         To run against this Node anyway, as a deliberate experiment: {ALLOW_MISMATCH_ENV}=1",
        raw = requirement.raw
    )
}

/// Read the requirement from `<project_root>/package.json`.
pub fn load_requirement(project_root: &Path) -> Result<NodeRequirement> {
    let path = project_root.join("package.json");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    engines_node_requirement(&text).with_context(|| format!("in {}", path.display()))
}

/// Run `node --version` and parse it. `Err(detail)` explains the failure in
/// one line suitable for [`Outcome::NotFound`].
pub fn installed_node_version() -> Result<Version, String> {
    // `nested_command` strips the inherited cargo package env (see util.rs);
    // `node` is a real executable on every platform, so no `.cmd` shim.
    let output = crate::util::nested_command("node")
        .arg("--version")
        .output()
        .map_err(|e| format!("could not run `node --version` ({e})"))?;
    if !output.status.success() {
        return Err(format!("`node --version` exited with {}", output.status));
    }
    parse_node_version(&String::from_utf8_lossy(&output.stdout)).map_err(|e| format!("{e:#}"))
}

/// `cargo xtask verify` preflight: fail on a mismatched or missing Node
/// unless the escape hatch is set, in which case warn loudly and continue.
/// Prints a one-line confirmation on success so a verify log names the
/// toolchain it ran with.
pub fn preflight_verify(project_root: &Path) -> Result<()> {
    let requirement = load_requirement(project_root)?;
    let result = outcome(&requirement, installed_node_version());
    let allow = allow_mismatch_from_env(std::env::var(ALLOW_MISMATCH_ENV).ok().as_deref());
    let text = describe(&requirement, &result);
    match enforcement(&result, allow) {
        Enforcement::Proceed if matches!(result, Outcome::Satisfied { .. }) => {
            println!("  ✓ {text}");
            Ok(())
        }
        Enforcement::Proceed => {
            println!(
                "  ⚠ {text}\n  ⚠ {ALLOW_MISMATCH_ENV} is set — continuing on an unsupported Node; \
                 results may not match CI."
            );
            Ok(())
        }
        Enforcement::Fail => bail!("{text}"),
    }
}

/// `cargo xtask dev-setup` report: warn-only, mirroring the other tool checks.
pub fn report_dev_setup(project_root: &Path) {
    let requirement = match load_requirement(project_root) {
        Ok(requirement) => requirement,
        Err(e) => {
            println!("\n  Warning: cannot read the repository's Node pin: {e:#}");
            return;
        }
    };
    let result = outcome(&requirement, installed_node_version());
    let text = describe(&requirement, &result);
    if matches!(result, Outcome::Satisfied { .. }) {
        println!("\n  {text}");
        return;
    }
    println!("\n  Warning: {text}");
    println!("  Install a version manager so `.nvmrc` is honoured, e.g.:");
    for line in version_manager_install_hints() {
        println!("    {line}");
    }
}

/// Platform-specific fnm install commands. `fnm install` with no version
/// reads `.nvmrc` when run inside the repository.
fn version_manager_install_hints() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &[
            "brew install fnm",
            "eval \"$(fnm env --use-on-cd --version-file-strategy=recursive)\"   # in ~/.zprofile",
            "fnm install    # inside the repo: installs the .nvmrc version",
        ]
    }
    #[cfg(windows)]
    {
        &[
            "winget install Schniz.fnm",
            "fnm env --use-on-cd | Out-String | Invoke-Expression   # in $PROFILE",
            "fnm install    # inside the repo: installs the .nvmrc version",
        ]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &[
            "curl -fsSL https://fnm.vercel.app/install | bash",
            "eval \"$(fnm env --use-on-cd --version-file-strategy=recursive)\"   # in your shell rc",
            "fnm install    # inside the repo: installs the .nvmrc version",
        ]
    }
    #[cfg(not(any(windows, unix)))]
    {
        &["install fnm (https://github.com/Schniz/fnm) and run `fnm install` inside the repo"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(raw: &str) -> NodeRequirement {
        engines_node_requirement(&format!(r#"{{"engines":{{"node":"{raw}"}}}}"#))
            .expect("requirement parses")
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    // --- parse_node_version -------------------------------------------------

    #[test]
    fn parses_node_version_output_with_leading_v_and_newline() {
        assert_eq!(parse_node_version("v24.20.0\n").unwrap(), v("24.20.0"));
    }

    #[test]
    fn parses_node_version_without_leading_v() {
        assert_eq!(parse_node_version("26.8.1").unwrap(), v("26.8.1"));
    }

    #[test]
    fn rejects_garbage_node_version_output() {
        assert!(parse_node_version("garbage").is_err());
        assert!(parse_node_version("").is_err());
        assert!(parse_node_version("v24").is_err());
    }

    // --- engines_node_requirement --------------------------------------------

    #[test]
    fn reads_caret_range_from_engines() {
        let r = req("^24.0.0");
        assert_eq!(r.raw, "^24.0.0");
        assert_eq!(r.source, "package.json");
        assert!(r.range.satisfies(&v("24.0.0")));
        assert!(r.range.satisfies(&v("24.20.0")));
        assert!(!r.range.satisfies(&v("26.8.1")));
        assert!(!r.range.satisfies(&v("23.11.0")));
    }

    #[test]
    fn reads_other_range_forms_used_by_npm() {
        assert!(req(">=24 <25").range.satisfies(&v("24.5.0")));
        assert!(!req(">=24 <25").range.satisfies(&v("25.0.0")));
        assert!(req("24.x").range.satisfies(&v("24.20.0")));
        assert!(!req("24.x").range.satisfies(&v("26.8.1")));
    }

    #[test]
    fn errors_when_engines_node_is_missing() {
        let err = engines_node_requirement(r#"{"name":"x"}"#).unwrap_err();
        assert!(err.to_string().contains("engines.node"), "{err:#}");
        let err = engines_node_requirement(r#"{"engines":{}}"#).unwrap_err();
        assert!(err.to_string().contains("engines.node"), "{err:#}");
    }

    #[test]
    fn errors_instead_of_passing_on_unparsable_range() {
        let err = engines_node_requirement(r#"{"engines":{"node":"latest-ish"}}"#).unwrap_err();
        assert!(err.to_string().contains("latest-ish"), "{err:#}");
    }

    #[test]
    fn errors_on_invalid_package_json() {
        assert!(engines_node_requirement("{not json").is_err());
    }

    #[test]
    fn repo_package_json_engines_node_is_parsable() {
        // Guards the real pin: if someone rewrites `engines.node` into a form
        // the check cannot interpret, this test fails before the check
        // starts failing everyone's `verify`.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let r = load_requirement(&root).expect("root package.json declares engines.node");
        assert!(!r.raw.is_empty());
    }

    // --- outcome ---------------------------------------------------------------

    #[test]
    fn outcome_classifies_probe_results() {
        let r = req("^24.0.0");
        assert_eq!(
            outcome(&r, Ok(v("24.20.0"))),
            Outcome::Satisfied {
                found: v("24.20.0")
            }
        );
        assert_eq!(
            outcome(&r, Ok(v("26.8.1"))),
            Outcome::Mismatch { found: v("26.8.1") }
        );
        assert_eq!(
            outcome(&r, Err("no `node` on PATH".to_string())),
            Outcome::NotFound {
                detail: "no `node` on PATH".to_string()
            }
        );
    }

    // --- enforcement -----------------------------------------------------------

    #[test]
    fn satisfied_always_proceeds() {
        let ok = Outcome::Satisfied {
            found: v("24.20.0"),
        };
        assert_eq!(enforcement(&ok, false), Enforcement::Proceed);
        assert_eq!(enforcement(&ok, true), Enforcement::Proceed);
    }

    #[test]
    fn mismatch_fails_unless_escape_hatch_is_set() {
        let bad = Outcome::Mismatch { found: v("26.8.1") };
        assert_eq!(enforcement(&bad, false), Enforcement::Fail);
        assert_eq!(enforcement(&bad, true), Enforcement::Proceed);
    }

    #[test]
    fn not_found_fails_unless_escape_hatch_is_set() {
        let missing = Outcome::NotFound {
            detail: "x".to_string(),
        };
        assert_eq!(enforcement(&missing, false), Enforcement::Fail);
        assert_eq!(enforcement(&missing, true), Enforcement::Proceed);
    }

    #[test]
    fn escape_hatch_env_parsing() {
        assert!(allow_mismatch_from_env(Some("1")));
        assert!(allow_mismatch_from_env(Some("true")));
        assert!(allow_mismatch_from_env(Some("TRUE")));
        assert!(!allow_mismatch_from_env(Some("0")));
        assert!(!allow_mismatch_from_env(Some("")));
        assert!(!allow_mismatch_from_env(Some("yes")));
        assert!(!allow_mismatch_from_env(None));
    }

    // --- describe --------------------------------------------------------------

    #[test]
    fn describe_satisfied_names_version_and_requirement() {
        let r = req("^24.0.0");
        let text = describe(
            &r,
            &Outcome::Satisfied {
                found: v("24.20.0"),
            },
        );
        assert_eq!(
            text,
            "Node v24.20.0 satisfies engines.node ^24.0.0 (package.json)"
        );
    }

    #[test]
    fn describe_mismatch_is_actionable() {
        let r = req("^24.0.0");
        let text = describe(&r, &Outcome::Mismatch { found: v("26.8.1") });
        assert!(
            text.starts_with("Node v26.8.1 does not satisfy engines.node ^24.0.0"),
            "{text}"
        );
        for needle in [
            "(package.json)",
            ".nvmrc",
            "fnm",
            DOCS_PATH,
            ALLOW_MISMATCH_ENV,
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
    }

    #[test]
    fn describe_not_found_carries_detail_and_remedy() {
        let r = req("^24.0.0");
        let text = describe(
            &r,
            &Outcome::NotFound {
                detail: "no `node` on PATH".to_string(),
            },
        );
        assert!(text.starts_with("Node not found"), "{text}");
        for needle in [
            "no `node` on PATH",
            "^24.0.0",
            ".nvmrc",
            DOCS_PATH,
            ALLOW_MISMATCH_ENV,
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
    }
}

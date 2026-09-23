//! Lint rule: the pandoc version pin agrees everywhere it's recorded.
//!
//! The Pandoc-hybrid docx/pptx leg pins a specific pandoc version
//! (`PANDOC_PIN` in `crates/quarto-core/src/pandoc_filters/mod.rs`), but
//! that number is *recorded* independently in four places: the two CI
//! workflows' `PANDOC_VERSION` env var (which installs an exact release),
//! `crates/xtask/src/dev_setup.rs`'s advisory `PANDOC_HYBRID_MIN_VERSION`
//! constant, and the vendored-filters README's `## Source` section. Nothing
//! reconciled these before this rule — a routine `PANDOC_VERSION` bump in CI
//! could silently drift from the floor the Pandoc-hybrid `L`-tier tests
//! actually require, or vice versa. See
//! `claude-notes/plans/2026-09-18-pandoc-hybrid-P4-implementation.md` Task 7.
//!
//! `PANDOC_PIN` is the authority; every other location must equal it
//! exactly (not merely "at least").

use std::path::Path;

use anyhow::{Context, Result};

use super::Violation;

const RULE_NAME: &str = "pandoc-pin-agreement";

const MOD_RS_REL: &str = "crates/quarto-core/src/pandoc_filters/mod.rs";
const DEV_SETUP_REL: &str = "crates/xtask/src/dev_setup.rs";
const README_REL: &str = "resources/pandoc-filters/README.md";
const WORKFLOW_RELS: &[&str] = &[
    ".github/workflows/test-suite.yml",
    ".github/workflows/ts-test-suite.yml",
];

/// Run the repo-level pandoc-pin-agreement check.
///
/// `workspace_root` anchors every file this rule reads, so tests can point
/// it at a synthetic tree instead of the real one.
pub fn check(workspace_root: &Path) -> Result<Vec<Violation>> {
    let mod_rs_path = workspace_root.join(MOD_RS_REL);
    let mod_rs = std::fs::read_to_string(&mod_rs_path)
        .with_context(|| format!("reading {}", mod_rs_path.display()))?;
    let Some((pin, pin_line)) = find_quoted_const(&mod_rs, "PANDOC_PIN") else {
        return Ok(vec![Violation {
            file: mod_rs_path,
            line: 1,
            column: 1,
            rule: RULE_NAME,
            message: "could not find a `PANDOC_PIN: &str = \"...\"` constant".to_string(),
            suggestion: None,
        }]);
    };

    let mut violations = Vec::new();

    for workflow_rel in WORKFLOW_RELS {
        let path = workspace_root.join(workflow_rel);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        match find_quoted_const(&content, "PANDOC_VERSION") {
            None => violations.push(Violation {
                file: path,
                line: 1,
                column: 1,
                rule: RULE_NAME,
                message: format!(
                    "could not find a `PANDOC_VERSION: \"...\"` line in {workflow_rel}"
                ),
                suggestion: None,
            }),
            Some((version, _)) if version == pin => {}
            Some((version, line)) => violations.push(Violation {
                file: path,
                line,
                column: 1,
                rule: RULE_NAME,
                message: format!(
                    "{workflow_rel}'s PANDOC_VERSION is {version:?}, but PANDOC_PIN at \
                     {MOD_RS_REL}:{pin_line} is {pin:?}"
                ),
                suggestion: Some(format!("Set PANDOC_VERSION: \"{pin}\"")),
            }),
        }
    }

    let dev_setup_path = workspace_root.join(DEV_SETUP_REL);
    let dev_setup = std::fs::read_to_string(&dev_setup_path)
        .with_context(|| format!("reading {}", dev_setup_path.display()))?;
    match find_tuple_const(&dev_setup, "PANDOC_HYBRID_MIN_VERSION") {
        None => violations.push(Violation {
            file: dev_setup_path,
            line: 1,
            column: 1,
            rule: RULE_NAME,
            message: format!(
                "could not find a `PANDOC_HYBRID_MIN_VERSION: (u32, u32) = (...)` constant in \
                 {DEV_SETUP_REL}"
            ),
            suggestion: None,
        }),
        Some((major, minor, _)) if format!("{major}.{minor}") == pin => {}
        Some((major, minor, line)) => violations.push(Violation {
            file: dev_setup_path,
            line,
            column: 1,
            rule: RULE_NAME,
            message: format!(
                "{DEV_SETUP_REL}'s PANDOC_HYBRID_MIN_VERSION is ({major}, {minor}), but \
                 PANDOC_PIN at {MOD_RS_REL}:{pin_line} is {pin:?}"
            ),
            suggestion: Some(format!(
                "Set PANDOC_HYBRID_MIN_VERSION: (u32, u32) = ({}, {})",
                pin.split('.').next().unwrap_or(""),
                pin.split('.').nth(1).unwrap_or("")
            )),
        }),
    }

    let readme_path = workspace_root.join(README_REL);
    let readme = std::fs::read_to_string(&readme_path)
        .with_context(|| format!("reading {}", readme_path.display()))?;
    match find_readme_pandoc_version(&readme) {
        None => violations.push(Violation {
            file: readme_path,
            line: 1,
            column: 1,
            rule: RULE_NAME,
            message: format!("could not find a `- pandoc version: \\`...\\`` line in {README_REL}"),
            suggestion: None,
        }),
        Some((version, _)) if version == pin => {}
        Some((version, line)) => violations.push(Violation {
            file: readme_path,
            line,
            column: 1,
            rule: RULE_NAME,
            message: format!(
                "{README_REL}'s recorded pandoc version is {version:?}, but PANDOC_PIN at \
                 {MOD_RS_REL}:{pin_line} is {pin:?}"
            ),
            suggestion: Some(format!("Set `- pandoc version: \\`{pin}\\``")),
        }),
    }

    Ok(violations)
}

/// Finds the first line naming `name` immediately followed (on that same
/// line) by a `"..."`-quoted value — matches both a Rust
/// `const {name}: &str = "..."` declaration and a YAML `{name}: "..."` env
/// entry — and returns the quoted value plus its 1-based line number.
/// Skips (rather than bailing on) a line that names `name` but has no
/// quoted value on it, e.g. a later `${name}` shell interpolation.
fn find_quoted_const(source: &str, name: &str) -> Option<(String, usize)> {
    for (idx, line) in source.lines().enumerate() {
        let Some(name_pos) = line.find(name) else {
            continue;
        };
        let rest = &line[name_pos + name.len()..];
        let Some(quote_start) = rest.find('"') else {
            continue;
        };
        let quote_rest = &rest[quote_start + 1..];
        let Some(quote_end) = quote_rest.find('"') else {
            continue;
        };
        return Some((quote_rest[..quote_end].to_string(), idx + 1));
    }
    None
}

/// Finds `{name}: (u32, u32) = (MAJOR, MINOR)` and returns the parsed tuple
/// plus its 1-based line number.
fn find_tuple_const(source: &str, name: &str) -> Option<(u32, u32, usize)> {
    let needle = format!("{name}: (u32, u32) = (");
    for (idx, line) in source.lines().enumerate() {
        let Some(pos) = line.find(&needle) else {
            continue;
        };
        let rest = &line[pos + needle.len()..];
        let end = rest.find(')')?;
        let mut parts = rest[..end].split(',').map(str::trim);
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        return Some((major, minor, idx + 1));
    }
    None
}

/// Finds `- pandoc version: \`X.Y\`` in the vendored-filters README and
/// returns the recorded version plus its 1-based line number.
fn find_readme_pandoc_version(source: &str) -> Option<(String, usize)> {
    let needle = "pandoc version: `";
    for (idx, line) in source.lines().enumerate() {
        let Some(pos) = line.find(needle) else {
            continue;
        };
        let rest = &line[pos + needle.len()..];
        let end = rest.find('`')?;
        return Some((rest[..end].to_string(), idx + 1));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_tree(
        dir: &Path,
        pin: &str,
        workflow_version: &str,
        dev_setup_floor: &str,
        readme_version: &str,
    ) {
        fs::create_dir_all(dir.join("crates/quarto-core/src/pandoc_filters")).unwrap();
        fs::write(
            dir.join(MOD_RS_REL),
            format!("pub const PANDOC_PIN: &str = \"{pin}\";\n"),
        )
        .unwrap();

        fs::create_dir_all(dir.join(".github/workflows")).unwrap();
        for workflow_rel in WORKFLOW_RELS {
            fs::write(
                dir.join(workflow_rel),
                format!("env:\n  PANDOC_VERSION: \"{workflow_version}\"\n"),
            )
            .unwrap();
        }

        fs::create_dir_all(dir.join("crates/xtask/src")).unwrap();
        let (major, minor) = dev_setup_floor.split_once('.').unwrap();
        fs::write(
            dir.join(DEV_SETUP_REL),
            format!("const PANDOC_HYBRID_MIN_VERSION: (u32, u32) = ({major}, {minor});\n"),
        )
        .unwrap();

        fs::create_dir_all(dir.join("resources/pandoc-filters")).unwrap();
        fs::write(
            dir.join(README_REL),
            format!("- pandoc version: `{readme_version}`\n"),
        )
        .unwrap();
    }

    /// T7.4: all four values agreeing produces zero violations.
    #[test]
    fn test_pandoc_pin_agrees_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        write_tree(dir.path(), "3.10", "3.10", "3.10", "3.10");

        let violations = check(dir.path()).unwrap();
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
    }

    /// T7.4 revert hunk: reverting a CI workflow's `PANDOC_VERSION` back to
    /// an older value makes this RED.
    #[test]
    fn test_stale_workflow_version_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        write_tree(dir.path(), "3.10", "3.8.3", "3.10", "3.10");

        let violations = check(dir.path()).unwrap();
        assert!(
            violations
                .iter()
                .any(|v| v.file.ends_with("test-suite.yml") && v.message.contains("3.8.3")),
            "expected a violation naming the stale workflow version: {violations:?}"
        );
    }

    /// Any one of the four values changed independently is flagged —
    /// exercises the dev-setup and README arms specifically.
    #[test]
    fn test_stale_dev_setup_floor_and_readme_are_each_flagged() {
        let dir = tempfile::tempdir().unwrap();
        write_tree(dir.path(), "3.10", "3.10", "3.6", "3.9");

        let violations = check(dir.path()).unwrap();
        assert!(
            violations
                .iter()
                .any(|v| v.file.ends_with("dev_setup.rs") && v.message.contains("(3, 6)")),
            "expected a violation naming the stale dev_setup.rs floor: {violations:?}"
        );
        assert!(
            violations
                .iter()
                .any(|v| v.file.ends_with("README.md") && v.message.contains("\"3.9\"")),
            "expected a violation naming the stale README version: {violations:?}"
        );
    }

    /// Positive-direction check against the real tree — the tests above
    /// prove the rule discriminates; this proves the real tree is clean.
    #[test]
    fn test_real_tree_pins_agree() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let violations = check(&workspace_root).unwrap();
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
    }
}

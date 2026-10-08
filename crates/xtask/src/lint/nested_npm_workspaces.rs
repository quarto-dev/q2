//! Lint: no npm workspace may live inside another npm workspace's directory,
//! unless the pair is listed in `ALLOWED` with a reason.
//!
//! Why this exists (bd-verify-css-lint-nested-ws-r0dd5vdk, 2026-10-08): npm
//! resolves `-w <path>` to *every* workspace at or under that path. When
//! `hub-client/vscode-sync-experiment` was registered as a root workspace
//! (909eada30), every `npm run <script> -w hub-client` silently started
//! fanning out to it too. `cargo xtask verify`'s `lint:css` step then failed
//! on `main` because the nested package had no `lint:css` script, and CI's
//! hub-client build began esbuilding the extension as a side effect. Selecting
//! by package name doesn't help when the name equals the directory name, as
//! `hub-client`'s does. This rule stops the next nesting at the source.
//!
//! Like `ci_test_wiring`, this is a *repo-level* rule: it runs once per lint
//! invocation against the root `package.json`.

use std::path::Path;

use anyhow::{Context, Result};

use super::Violation;
use super::ci_test_wiring::{workspace_dirs, workspace_globs};

const RULE: &str = "nested-npm-workspace";

/// Nested workspaces tolerated for now, as repo-relative paths of the *inner*
/// workspace, with the reason. Always name the tracking strand — an entry here
/// is a promise to move the package, not a place to park it.
pub(crate) const ALLOWED: &[(&str, &str)] = &[(
    "hub-client/vscode-sync-experiment",
    "Experiment predates this rule; moving it out from under hub-client/ is \
     tracked by bd-prfbxrth. Until then, invoke hub-client scripts with \
     `--prefix hub-client`, not `-w hub-client`.",
)];

/// 1-indexed line of `"<entry>"` in the root package.json, or 1 if not found.
fn entry_line(manifest: &str, entry: &str) -> usize {
    let quoted = format!("\"{entry}\"");
    manifest
        .lines()
        .position(|line| line.contains(&quoted))
        .map_or(1, |i| i + 1)
}

pub fn check(workspace_root: &Path) -> Result<Vec<Violation>> {
    let root_manifest_path = workspace_root.join("package.json");
    let Ok(root_manifest) = std::fs::read_to_string(&root_manifest_path) else {
        // No npm workspace here (e.g. a Rust-only checkout) — nothing to check.
        return Ok(Vec::new());
    };
    let root: serde_json::Value = serde_json::from_str(&root_manifest)
        .with_context(|| format!("Failed to parse {}", root_manifest_path.display()))?;

    let globs = workspace_globs(&root);
    let rel_dirs: Vec<String> = workspace_dirs(workspace_root, &globs)
        .iter()
        .map(|dir| {
            dir.strip_prefix(workspace_root)
                .unwrap_or(dir)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    let mut violations = Vec::new();
    for inner in &rel_dirs {
        let Some(outer) = rel_dirs
            .iter()
            .find(|outer| inner.starts_with(&format!("{outer}/")))
        else {
            continue;
        };
        if ALLOWED.iter().any(|(allowed, _)| allowed == inner) {
            continue;
        }

        // Point at the workspaces entry that brought the inner package in:
        // its literal path if listed, else the glob that expanded to it.
        let entry = globs
            .iter()
            .find(|g| *g == inner)
            .or_else(|| {
                globs.iter().find(|g| {
                    g.strip_suffix("/*")
                        .is_some_and(|prefix| inner.starts_with(&format!("{prefix}/")))
                })
            })
            .map_or(inner.as_str(), String::as_str);

        violations.push(Violation {
            file: root_manifest_path.clone(),
            line: entry_line(&root_manifest, entry),
            column: 1,
            rule: RULE,
            message: format!(
                "npm workspace `{inner}` is nested inside workspace `{outer}`, so \
                 every `npm ... -w {outer}` also selects `{inner}`"
            ),
            suggestion: Some(format!(
                "Move `{inner}` out from under `{outer}/`, or add it to ALLOWED \
                 in crates/xtask/src/lint/nested_npm_workspaces.rs with a reason \
                 and a tracking strand."
            )),
        });
    }

    Ok(violations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a fake repo: root package.json with `workspaces`, and an empty
    /// package.json in each of `packages`.
    fn scaffold(globs: &[&str], packages: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let globs_json = globs
            .iter()
            .map(|g| format!("\"{g}\""))
            .collect::<Vec<_>>()
            .join(",\n    ");
        fs::write(
            tmp.path().join("package.json"),
            format!("{{\n  \"workspaces\": [\n    {globs_json}\n  ]\n}}\n"),
        )
        .unwrap();
        for pkg in packages {
            let dir = tmp.path().join(pkg);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("package.json"), "{}").unwrap();
        }
        tmp
    }

    #[test]
    fn accepts_sibling_workspaces() {
        let tmp = scaffold(
            &["ts-packages/*", "hub-client", "hub-client-extras"],
            &["ts-packages/a", "hub-client", "hub-client-extras"],
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn flags_a_literal_workspace_nested_in_another() {
        let tmp = scaffold(&["app", "app/plugin"], &["app", "app/plugin"]);
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].message.contains("`app/plugin`"));
        assert!(violations[0].message.contains("`app`"));
        assert_eq!(violations[0].line, 4, "points at the \"app/plugin\" entry");
    }

    #[test]
    fn flags_a_glob_expanded_workspace_nested_in_another() {
        let tmp = scaffold(&["app", "app/plugins/*"], &["app", "app/plugins/x"]);
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].message.contains("`app/plugins/x`"));
        assert_eq!(violations[0].line, 4, "points at the glob entry");
    }

    #[test]
    fn ignores_allowed_nestings() {
        let (inner, _) = ALLOWED[0];
        let outer = inner.rsplit_once('/').unwrap().0;
        let tmp = scaffold(&[outer, inner], &[outer, inner]);
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn missing_root_manifest_yields_no_violations() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(check(tmp.path()).unwrap().is_empty());
    }
}

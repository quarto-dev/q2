//! Lint rule: vendored pandoc filters' "Ours vs. pinned" section lists all Q2-only customizations.
//!
//! The pandoc filters are vendored at v1.11.3 from quarto-cli and embedded at compile time.
//! A re-vendor (delete-and-recopy) would remove any Q2 customizations (Task 8 shim, P3
//! crossref patches, etc.) unless they are explicitly listed in the README's
//! `## Ours vs. pinned` section. This rule checks that every listed file exists.
//!
//! The rule operates on the README file, anchoring violations at the section heading
//! and at individual list entries when files are missing.

use std::path::Path;

use anyhow::Result;

use super::Violation;

/// The name of this lint rule.
const RULE_NAME: &str = "vendored-pandoc-filters";

/// Path to the pandoc filters README, relative to workspace root.
const README_REL: &str = "resources/pandoc-filters/README.md";

/// Run the repo-level vendored pandoc filters check.
///
/// `workspace_root` anchors the README and the files it lists, so tests can
/// point the check at a synthetic tree instead of the real one.
pub fn check(workspace_root: &Path) -> Result<Vec<Violation>> {
    let readme_path = workspace_root.join(README_REL);

    if !readme_path.exists() {
        // If README doesn't exist yet, no violations (will be caught by other rules)
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&readme_path)?;

    let mut violations = Vec::new();
    let mut in_ours_section = false;

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = line_idx + 1;

        if line.trim() == "## Ours vs. pinned" {
            in_ours_section = true;
            continue;
        }

        // Stop at the next section heading or end of content
        if in_ours_section && line.starts_with("## ") {
            in_ours_section = false;
            continue;
        }

        if in_ours_section
            && line.trim().starts_with("- `")
            && let Some(path_start) = line.find('`').map(|i| i + 1)
            && let Some(path_end) = line[path_start..].find('`')
        {
            // Extract path from list item: - `path/to/file` — description
            let file_path = &line[path_start..path_start + path_end];
            let full_path = workspace_root.join(file_path);

            if !full_path.exists() {
                violations.push(Violation {
                    file: readme_path.clone(),
                    line: line_num,
                    column: 3,
                    rule: RULE_NAME,
                    message: format!(
                        "File listed in '## Ours vs. pinned' does not exist: {}",
                        file_path
                    ),
                    suggestion: Some(
                        "Either create the file or remove this entry from the README".to_string(),
                    ),
                });
            }
        }
    }

    violations.sort_by(|a, b| (a.line, &a.message).cmp(&(b.line, &b.message)));
    Ok(violations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_missing_ours_file_is_flagged() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create directory structure
        std::fs::create_dir_all(temp_path.join("resources/pandoc-filters")).unwrap();

        // Create README with a missing file
        let readme_content = r#"# Pandoc Filters

## Ours vs. pinned

The following files are *not* from `v1.11.3`:
- `resources/pandoc-filters/filters/shim.lua` — custom Q2 shim file

"#;
        std::fs::write(
            temp_path.join("resources/pandoc-filters/README.md"),
            readme_content,
        )
        .unwrap();

        // Run check - should find 1 violation
        let violations = check(temp_path).unwrap();
        assert_eq!(
            violations.len(),
            1,
            "Expected 1 violation for missing file, got {}",
            violations.len()
        );
        assert_eq!(
            violations[0].rule, RULE_NAME,
            "Violation should be from our rule"
        );
        assert!(
            violations[0]
                .message
                .contains("resources/pandoc-filters/filters/shim.lua"),
            "Violation should mention the missing file"
        );
    }

    #[test]
    fn test_real_tree_is_clean() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();

        // Run against real tree - should have no violations (all "ours" files should exist)
        let violations = check(workspace_root).unwrap();
        assert!(
            violations.is_empty(),
            "Real tree should have no violations, but got: {:?}",
            violations
        );
    }
}

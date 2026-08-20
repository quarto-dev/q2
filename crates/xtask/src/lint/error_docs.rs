//! Lint rule: every error code has a documentation page, and its `docs_url`
//! points at that page.
//!
//! Every entry in `crates/quarto-error-catalog/error_catalog.json` carries a
//! `docs_url` of the form `https://quarto.org/docs/errors/<subsystem>/<code>`,
//! and the docs website serves that URL from
//! `docs/errors/<subsystem>/<code>.qmd`. Nothing used to check that the page
//! exists, so a diagnostic could ship pointing at a 404 — which is what
//! happened repeatedly: `Q-16-*`, `Q-5-8`..`Q-5-22`, `Q-11-2`..`Q-11-5`,
//! `Q-14-3`, `Q-3-42`/`Q-3-43`, and `Q-2-42` all reached `main` page-less,
//! each one added by a feature PR that introduced codes and stopped there.
//!
//! This rule closes that loop. It reports two problem classes:
//!
//! - **missing page** — the catalog declares the code; no `.qmd` exists at
//!   the path the `docs_url` resolves to.
//! - **`docs_url` drift** — the entry's `docs_url` is not the canonical
//!   `https://quarto.org/docs/errors/<subsystem>/<code>`. `Q-3-42` and
//!   `Q-3-43` shipped with the `<subsystem>` segment missing entirely, so a
//!   file-existence check alone would have passed them while the link the
//!   user actually clicks stayed broken.
//!
//! Every code needs a page, with no opt-out for "internal" codes: the page is
//! the first landing spot for anyone who hits the code, and the page itself
//! is where the internal/user-facing distinction gets explained.
//!
//! Deliberately *not* checked here (see bd-8otua, which owns the richer
//! `cargo xtask error-docs` audit): orphan pages whose code left the catalog,
//! pages filed under the wrong subsystem directory, and front-matter drift.
//! Page `title` is free to differ from catalog `title`, the same way page
//! `description` is free to differ from `message_template`.
//!
//! Unlike the other rules in this module, this one is **repo-level**: it has
//! no per-Rust-file anchor. Violations are reported against the catalog entry's
//! own line in `error_catalog.json`, because that is where the declaration
//! promising the page lives.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::Violation;

/// The name of this lint rule.
const RULE_NAME: &str = "error-docs-page-missing";

/// Catalog path, relative to the workspace root.
const CATALOG_REL: &str = "crates/quarto-error-catalog/error_catalog.json";

/// Root of the error-documentation pages, relative to the workspace root.
const DOCS_ERRORS_REL: &str = "docs/errors";

/// Prefix every canonical `docs_url` shares.
const DOCS_URL_PREFIX: &str = "https://quarto.org/docs/errors";

/// The fields of a catalog entry this rule cares about.
///
/// The catalog carries more (`title`, `message_template`, `since_version`);
/// they are irrelevant here and left to `serde` to ignore.
#[derive(Debug, Deserialize)]
struct CatalogEntry {
    subsystem: String,
    #[serde(default)]
    docs_url: Option<String>,
}

/// Run the repo-level error-docs check.
///
/// `workspace_root` anchors both the catalog and the docs tree, so tests can
/// point the check at a synthetic tree instead of the real one.
pub fn check(workspace_root: &Path) -> Result<Vec<Violation>> {
    let catalog_path = workspace_root.join(CATALOG_REL);
    let raw = std::fs::read_to_string(&catalog_path)
        .with_context(|| format!("Failed to read error catalog: {}", catalog_path.display()))?;
    let catalog: BTreeMap<String, CatalogEntry> = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse error catalog: {}", catalog_path.display()))?;

    let line_of = key_lines(&raw);
    let docs_root = workspace_root.join(DOCS_ERRORS_REL);

    let mut violations = Vec::new();

    for (code, entry) in &catalog {
        let line = line_of.get(code).copied().unwrap_or(1);
        let expected_url = format!("{DOCS_URL_PREFIX}/{}/{code}", entry.subsystem);
        let page_rel = format!("{DOCS_ERRORS_REL}/{}/{code}.qmd", entry.subsystem);

        if entry.docs_url.as_deref() != Some(expected_url.as_str()) {
            violations.push(Violation {
                file: catalog_path.clone(),
                line,
                column: 3,
                rule: RULE_NAME,
                message: format!(
                    "{code}: docs_url is {}, but the page for a `{}` code is served from {expected_url}",
                    entry
                        .docs_url
                        .as_deref()
                        .map_or_else(|| "absent".to_string(), |u| format!("`{u}`")),
                    entry.subsystem,
                ),
                suggestion: Some(format!("set \"docs_url\": \"{expected_url}\"")),
            });
        }

        if !docs_root
            .join(&entry.subsystem)
            .join(format!("{code}.qmd"))
            .exists()
        {
            violations.push(Violation {
                file: catalog_path.clone(),
                line,
                column: 3,
                rule: RULE_NAME,
                message: format!(
                    "{code} has no documentation page; diagnostics carrying this code link to \
                     {expected_url}, which 404s until {page_rel} exists"
                ),
                suggestion: Some(format!(
                    "create {page_rel} following docs/errors/README.md (front matter from the \
                     catalog entry, `status: stub`)"
                )),
            });
        }
    }

    violations.sort_by(|a, b| (a.line, &a.message).cmp(&(b.line, &b.message)));
    Ok(violations)
}

/// Map each top-level catalog key to the 1-indexed line it is declared on.
///
/// The catalog is pretty-printed with one key per line at two-space indent,
/// so a prefix match on `  "Q-` is enough — and being wrong only costs a
/// slightly-off line number in an error message, never a missed violation.
fn key_lines(raw: &str) -> BTreeMap<String, usize> {
    let mut lines = BTreeMap::new();
    for (idx, text) in raw.lines().enumerate() {
        let Some(rest) = text.strip_prefix("  \"") else {
            continue;
        };
        let Some((code, _)) = rest.split_once('"') else {
            continue;
        };
        lines.entry(code.to_string()).or_insert(idx + 1);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic workspace: a catalog with the given entries, and a
    /// docs tree containing exactly `pages`.
    fn fixture(entries: &[(&str, &str, &str)], pages: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        let body: Vec<String> = entries
            .iter()
            .map(|(code, subsystem, url)| {
                format!(
                    "  \"{code}\": {{\n    \"subsystem\": \"{subsystem}\",\n    \"title\": \"T\",\n    \"message_template\": \"M\",\n    \"docs_url\": \"{url}\",\n    \"since_version\": \"99.9.9\"\n  }}"
                )
            })
            .collect();
        let catalog = format!("{{\n{}\n}}\n", body.join(",\n"));

        let catalog_path = root.join(CATALOG_REL);
        std::fs::create_dir_all(catalog_path.parent().unwrap()).unwrap();
        std::fs::write(&catalog_path, catalog).unwrap();

        for (subsystem, code) in pages {
            let dir = root.join(DOCS_ERRORS_REL).join(subsystem);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{code}.qmd")), "---\ncode: x\n---\n").unwrap();
        }

        dir
    }

    #[test]
    fn code_with_a_page_and_canonical_url_is_clean() {
        let fx = fixture(
            &[(
                "Q-2-1",
                "markdown",
                "https://quarto.org/docs/errors/markdown/Q-2-1",
            )],
            &[("markdown", "Q-2-1")],
        );
        assert!(check(fx.path()).unwrap().is_empty());
    }

    #[test]
    fn code_without_a_page_is_reported() {
        let fx = fixture(
            &[(
                "Q-2-42",
                "markdown",
                "https://quarto.org/docs/errors/markdown/Q-2-42",
            )],
            &[],
        );
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].message.contains("Q-2-42"));
        assert!(violations[0].message.contains("has no documentation page"));
        assert!(
            violations[0]
                .suggestion
                .as_ref()
                .unwrap()
                .contains("docs/errors/markdown/Q-2-42.qmd")
        );
    }

    /// A page filed under some *other* subsystem does not satisfy the code:
    /// the URL resolves by subsystem, so only the canonical path counts.
    #[test]
    fn page_under_the_wrong_subsystem_does_not_count() {
        let fx = fixture(
            &[(
                "Q-16-1",
                "extension",
                "https://quarto.org/docs/errors/extension/Q-16-1",
            )],
            &[("project", "Q-16-1")],
        );
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].message.contains("has no documentation page"));
    }

    /// The `Q-3-42` shape: page exists, but `docs_url` omits the subsystem
    /// segment, so the link a user clicks still 404s.
    #[test]
    fn docs_url_missing_the_subsystem_segment_is_reported() {
        let fx = fixture(
            &[("Q-3-42", "writer", "https://quarto.org/docs/errors/Q-3-42")],
            &[("writer", "Q-3-42")],
        );
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].message.contains("docs_url is"));
        assert!(
            violations[0]
                .suggestion
                .as_ref()
                .unwrap()
                .contains("https://quarto.org/docs/errors/writer/Q-3-42")
        );
    }

    #[test]
    fn a_code_can_trip_both_checks_at_once() {
        let fx = fixture(
            &[("Q-3-43", "writer", "https://quarto.org/docs/errors/Q-3-43")],
            &[],
        );
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 2, "{violations:#?}");
    }

    #[test]
    fn violations_anchor_at_the_catalog_entry_line() {
        let fx = fixture(
            &[
                ("Q-1-1", "yaml", "https://quarto.org/docs/errors/yaml/Q-1-1"),
                (
                    "Q-2-42",
                    "markdown",
                    "https://quarto.org/docs/errors/markdown/Q-2-42",
                ),
            ],
            &[("yaml", "Q-1-1")],
        );
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 1, "{violations:#?}");
        // Entry one occupies lines 2..8; entry two opens on line 9.
        assert_eq!(violations[0].line, 9);
        assert!(violations[0].file.ends_with("error_catalog.json"));
    }

    #[test]
    fn missing_docs_url_is_reported_rather_than_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let catalog_path = dir.path().join(CATALOG_REL);
        std::fs::create_dir_all(catalog_path.parent().unwrap()).unwrap();
        std::fs::write(
            &catalog_path,
            "{\n  \"Q-9-1\": {\n    \"subsystem\": \"yaml\",\n    \"title\": \"T\"\n  }\n}\n",
        )
        .unwrap();
        let dir_yaml = dir.path().join(DOCS_ERRORS_REL).join("yaml");
        std::fs::create_dir_all(&dir_yaml).unwrap();
        std::fs::write(dir_yaml.join("Q-9-1.qmd"), "---\n---\n").unwrap();

        let violations = check(dir.path()).unwrap();
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].message.contains("absent"));
    }

    /// The real tree must be clean — this is the regression test that keeps
    /// a new code from shipping without its page.
    #[test]
    fn the_real_catalog_and_docs_tree_agree() {
        let root = super::super::find_workspace_root().expect("workspace root");
        let violations = check(&root).expect("check runs");
        assert!(
            violations.is_empty(),
            "error catalog and docs/errors/ have drifted:\n{}",
            violations
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

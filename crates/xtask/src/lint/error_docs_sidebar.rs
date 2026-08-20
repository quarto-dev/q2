//! Lint rule: every error-documentation page is reachable from the errors
//! sidebar, and every sidebar entry points at a page that exists.
//!
//! The error-reference sidebar in `docs/_quarto.yml` enumerates every page by
//! hand — one `- errors/<subsystem>/<code>.qmd` line each, grouped under a
//! `- section: "<subsystem>"` block. That was a deliberate v1 choice (see
//! `claude-notes/plans/2026-05-22-error-docs-foundation.md` §"Navbar + sidebar
//! wiring": "populated by hand as pages are added"), on the reasoning that the
//! listing at `docs/errors/index.qmd` is the canonical browse surface and the
//! sidebar is supplementary.
//!
//! Nothing checked that the hand-maintained list stayed complete, and it did
//! not: by 2026-08-18 the sidebar listed 153 of 207 pages. Two whole
//! subsystems (`crossref`, `extension`) had no `- section:` block at all, so
//! every `Q-15-*` and `Q-16-*` page was unreachable by navigation. The pages
//! rendered and resolved by direct URL — the `docs_url` in each catalog entry
//! was fine, so no diagnostic shipped a 404 — but a reader browsing the error
//! reference could not find them.
//!
//! The existing `error-docs-page-missing` rule does not catch this: it
//! reconciles the catalog against page *existence* and checks `docs_url`.
//! Sidebar membership was outside its scope, so a whole subsystem could go
//! unlisted while the lint stayed green (bd-wcmk1fsq).
//!
//! This rule closes that loop with three problem classes:
//!
//! - **unlisted page** — a page exists under `docs/errors/<subsystem>/` that
//!   no sidebar entry references.
//! - **stale entry** — a sidebar entry references a page that does not exist,
//!   so the rendered sidebar carries a dead link.
//! - **out-of-order entry** — entries within a `- section:` block must ascend
//!   by code number. Appending new entries alphabetically would drift the
//!   sidebar into lexicographic order, putting `Q-1-10` before `Q-1-2`.
//!
//! Only the code's **second** numeric segment sequences, because the section
//! already groups by subsystem. Beware that a numerically ordered section can
//! still *look* lexicographic where codes are sparse: the real `yaml` section
//! runs `Q-1-1, Q-1-10, … Q-1-29, Q-1-99` because there is no `Q-1-2` through
//! `Q-1-9`. The `-99` catch-all codes sort last with no special case.
//!
//! # Deliberately not checked
//!
//! **Section order.** The `- section:` blocks sit in an arbitrary historical
//! order (`yaml, markdown, writer, listing, xml, cli, navigation, template,
//! project, include, internal, theme, lua`, with `crossref` and `extension`
//! appended by bd-wcmk1fsq). Policing that would mean picking a canonical
//! order this rule has no authority to choose, and would churn every section
//! for no reader benefit.
//!
//! **The listing's order.** Sorting `docs/errors/index.qmd` by code number is
//! a different problem, tracked as bd-otmqu — its sort key has to come from
//! front matter, and Q2's listing sort reaches neither top-level front-matter
//! keys nor a numeric-aware comparator. Q1 cannot do it either: its build-time
//! sort is lodash `orderBy`, which compares strings with plain relational
//! operators.
//!
//! Like `error_docs`, this is a **repo-level** rule: it reconciles two trees
//! rather than grepping one Rust file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};

use super::Violation;

/// The name of this lint rule.
const RULE_NAME: &str = "error-docs-sidebar-unlisted";

/// The website config carrying the errors sidebar, relative to the workspace
/// root.
const QUARTO_YML_REL: &str = "docs/_quarto.yml";

/// Root of the error-documentation pages, relative to the workspace root.
const DOCS_ERRORS_REL: &str = "docs/errors";

/// Run the repo-level errors-sidebar check.
///
/// `workspace_root` anchors both the config and the docs tree, so tests can
/// point the check at a synthetic tree instead of the real one.
pub fn check(workspace_root: &Path) -> Result<Vec<Violation>> {
    let config_path = workspace_root.join(QUARTO_YML_REL);
    let raw = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read website config: {}", config_path.display()))?;

    let listed = sidebar_entries(&raw);
    let on_disk = pages_on_disk(&workspace_root.join(DOCS_ERRORS_REL))?;

    // Anchor "unlisted page" violations at the sidebar's own declaration —
    // that is where the missing line belongs. Falling back to line 1 keeps a
    // hand-edited config from panicking the lint.
    let sidebar_line = errors_sidebar_line(&raw).unwrap_or(1);

    let mut violations = Vec::new();

    for rel in &on_disk {
        if listed.contains_key(rel) {
            continue;
        }
        let (subsystem, code) = split_rel(rel);
        violations.push(Violation {
            file: config_path.clone(),
            line: sidebar_line,
            column: 5,
            rule: RULE_NAME,
            message: format!(
                "{code} has a page at {DOCS_ERRORS_REL}/{rel} that no errors-sidebar entry \
                 references; readers browsing the error reference cannot reach it"
            ),
            suggestion: Some(format!(
                "add `- errors/{rel}` under the `- section: \"{subsystem}\"` block in \
                 {QUARTO_YML_REL} (create the section if it does not exist)"
            )),
        });
    }

    for (rel, line) in &listed {
        if on_disk.contains(rel) {
            continue;
        }
        violations.push(Violation {
            file: config_path.clone(),
            line: *line,
            column: 5,
            rule: RULE_NAME,
            message: format!(
                "the errors sidebar references errors/{rel}, but no page exists at \
                 {DOCS_ERRORS_REL}/{rel}; the rendered sidebar carries a dead link"
            ),
            suggestion: Some(format!(
                "remove the entry, or create {DOCS_ERRORS_REL}/{rel} following \
                 docs/errors/README.md"
            )),
        });
    }

    for section in sections(&raw) {
        let mut prev: Option<(u32, String)> = None;
        for entry in &section.entries {
            if let Some((prev_num, prev_code)) = &prev
                && entry.number < *prev_num
            {
                violations.push(Violation {
                    file: config_path.clone(),
                    line: entry.line,
                    column: 13,
                    rule: RULE_NAME,
                    message: format!(
                        "{} is listed after {} in the `{}` section; entries within a section \
                         must ascend by code number",
                        entry.code, prev_code, section.name
                    ),
                    suggestion: Some(format!(
                        "move `- errors/{}/{}.qmd` above `- errors/{}/{}.qmd`",
                        section.name, entry.code, section.name, prev_code
                    )),
                });
            }
            prev = Some((entry.number, entry.code.clone()));
        }
    }

    violations.sort_by(|a, b| (a.line, &a.message).cmp(&(b.line, &b.message)));
    Ok(violations)
}

/// One `- section: "<name>"` block of the errors sidebar, with the entries
/// nested under it in the order they appear.
struct SectionBlock {
    name: String,
    entries: Vec<SectionEntry>,
}

/// A single `- errors/<subsystem>/<code>.qmd` line inside a section.
struct SectionEntry {
    /// The bare code, e.g. `Q-1-10`.
    code: String,
    /// The code's second numeric segment — the only part that sequences
    /// within a section, since the section already groups by subsystem.
    number: u32,
    /// 1-indexed line the entry appears on.
    line: usize,
}

/// Parse the errors sidebar into its section blocks.
///
/// Scoped to the `- id: errors` sidebar rather than the whole file: unlike the
/// membership scan, ordering is only meaningful relative to the section an
/// entry sits in, and other sidebars in `docs/_quarto.yml` have sections of
/// their own. The block ends at the first non-blank line indented at or left
/// of the `- id: errors` line itself.
fn sections(raw: &str) -> Vec<SectionBlock> {
    let lines: Vec<&str> = raw.lines().collect();
    let Some(start) = lines.iter().position(|l| l.trim() == "- id: errors") else {
        return Vec::new();
    };
    let base_indent = indent_of(lines[start]);

    let mut blocks: Vec<SectionBlock> = Vec::new();
    for (offset, text) in lines.iter().enumerate().skip(start + 1) {
        if text.trim().is_empty() {
            continue;
        }
        if indent_of(text) <= base_indent {
            break;
        }
        if let Some(name) = section_name(text) {
            blocks.push(SectionBlock {
                name,
                entries: Vec::new(),
            });
            continue;
        }
        // Entries before the first section (e.g. `- errors/index.qmd`) have no
        // section to sequence within, so they are skipped.
        let Some(block) = blocks.last_mut() else {
            continue;
        };
        if let Some(rel) = entry_path(text) {
            let (_, code) = split_rel(&rel);
            if let Some(number) = code_number(code) {
                block.entries.push(SectionEntry {
                    code: code.to_string(),
                    number,
                    line: offset + 1,
                });
            }
        }
    }

    blocks
}

/// Number of leading spaces on a line.
fn indent_of(text: &str) -> usize {
    text.len() - text.trim_start().len()
}

/// Extract the name from a `- section: "<name>"` line.
fn section_name(text: &str) -> Option<String> {
    let rest = text.trim().strip_prefix("- section:")?.trim();
    let unquoted = rest.trim_matches('"');
    if unquoted.is_empty() {
        return None;
    }
    Some(unquoted.to_string())
}

/// The second numeric segment of a `Q-<subsystem>-<number>` code.
///
/// This is the part that sequences within a section. `-99` catch-all codes
/// sort last with no special case, which is the order the sidebar already
/// uses.
fn code_number(code: &str) -> Option<u32> {
    code.rsplit_once('-')?.1.parse().ok()
}

/// Collect every `errors/<subsystem>/<code>.qmd` path referenced anywhere in
/// the config, mapped to the 1-indexed line it appears on.
///
/// Scanning the whole file rather than isolating the `- id: errors` block
/// keeps this free of YAML structure: those paths appear nowhere else in
/// `docs/_quarto.yml`, and a stray duplicate would only be reported once
/// (`BTreeMap` keeps the first line seen).
fn sidebar_entries(raw: &str) -> BTreeMap<String, usize> {
    let mut entries = BTreeMap::new();
    for (idx, text) in raw.lines().enumerate() {
        let Some(rel) = entry_path(text) else {
            continue;
        };
        entries.entry(rel).or_insert(idx + 1);
    }
    entries
}

/// Extract the `<subsystem>/<code>.qmd` tail of a sidebar entry line, if the
/// line is one.
///
/// Accepts the `- errors/yaml/Q-1-1.qmd` list-item form and the
/// `- href: errors/yaml/Q-1-1.qmd` mapping form, since both are legal Quarto
/// sidebar spellings.
fn entry_path(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix("- ")?;
    let value = rest.strip_prefix("href:").map_or(rest, str::trim_start);
    let tail = value.trim().strip_prefix("errors/")?;
    // Reject the index page and anything that is not a `Q-*.qmd` leaf.
    let (subsystem, file) = tail.split_once('/')?;
    if subsystem.is_empty() || !file.starts_with("Q-") || !file.ends_with(".qmd") {
        return None;
    }
    Some(tail.to_string())
}

/// Collect every `<subsystem>/<code>.qmd` page present under the docs-errors
/// root.
///
/// Only `Q-*.qmd` files one level deep are pages; `index.qmd`, `README.md`,
/// and any deeper nesting are not.
fn pages_on_disk(docs_root: &Path) -> Result<BTreeSet<String>> {
    let mut pages = BTreeSet::new();

    let subsystems = match std::fs::read_dir(docs_root) {
        Ok(iter) => iter,
        // A missing docs tree is not this rule's problem to report.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(pages),
        Err(e) => {
            return Err(e).with_context(|| format!("Failed to read {}", docs_root.display()));
        }
    };

    for entry in subsystems {
        let entry = entry.with_context(|| format!("Failed to read {}", docs_root.display()))?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(subsystem) = entry.file_name().to_str().map(String::from) else {
            continue;
        };
        for page in std::fs::read_dir(entry.path())
            .with_context(|| format!("Failed to read {}", entry.path().display()))?
        {
            let page = page?;
            let Some(name) = page.file_name().to_str().map(String::from) else {
                continue;
            };
            if name.starts_with("Q-") && name.ends_with(".qmd") {
                pages.insert(format!("{subsystem}/{name}"));
            }
        }
    }

    Ok(pages)
}

/// Split a `<subsystem>/<code>.qmd` key back into its parts.
fn split_rel(rel: &str) -> (&str, &str) {
    let (subsystem, file) = rel.split_once('/').unwrap_or(("", rel));
    (subsystem, file.trim_end_matches(".qmd"))
}

/// Find the 1-indexed line of the errors sidebar's `- id: errors` declaration.
fn errors_sidebar_line(raw: &str) -> Option<usize> {
    raw.lines()
        .position(|text| text.trim() == "- id: errors")
        .map(|idx| idx + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic workspace: a `docs/_quarto.yml` with the given
    /// sidebar entries, and a docs tree containing exactly `pages`.
    ///
    /// Both are given as `("<subsystem>", "<code>")` pairs so a test can make
    /// the two sets disagree in either direction.
    fn fixture(listed: &[(&str, &str)], pages: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        let mut yml = String::from(
            "website:\n  sidebar:\n    - id: errors\n      title: \"Error reference\"\n      \
             contents:\n        - errors/index.qmd\n",
        );
        for (subsystem, code) in listed {
            yml.push_str(&format!(
                "        - section: \"{subsystem}\"\n          contents:\n            - \
                 errors/{subsystem}/{code}.qmd\n"
            ));
        }

        let config_path = root.join(QUARTO_YML_REL);
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(&config_path, yml).unwrap();

        for (subsystem, code) in pages {
            let dir = root.join(DOCS_ERRORS_REL).join(subsystem);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{code}.qmd")), "---\ncode: x\n---\n").unwrap();
        }

        dir
    }

    #[test]
    fn page_listed_in_the_sidebar_is_clean() {
        let fx = fixture(&[("markdown", "Q-2-1")], &[("markdown", "Q-2-1")]);
        assert!(check(fx.path()).unwrap().is_empty());
    }

    #[test]
    fn unlisted_page_is_reported() {
        let fx = fixture(
            &[("markdown", "Q-2-1")],
            &[("markdown", "Q-2-1"), ("markdown", "Q-2-2")],
        );
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert!(
            violations[0].message.contains("Q-2-2"),
            "expected Q-2-2 in {:?}",
            violations[0].message
        );
        assert!(
            violations[0]
                .suggestion
                .as_deref()
                .unwrap()
                .contains("- errors/markdown/Q-2-2.qmd"),
            "suggestion should name the line to add: {:?}",
            violations[0].suggestion
        );
    }

    /// The bd-wcmk1fsq shape: a whole subsystem with no `- section:` block, so
    /// every one of its pages is unreachable.
    #[test]
    fn subsystem_with_no_section_reports_every_page() {
        let fx = fixture(
            &[("markdown", "Q-2-1")],
            &[
                ("markdown", "Q-2-1"),
                ("extension", "Q-16-1"),
                ("extension", "Q-16-2"),
                ("extension", "Q-16-10"),
            ],
        );
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 3, "got {violations:?}");
        for code in ["Q-16-1", "Q-16-2", "Q-16-10"] {
            assert!(
                violations.iter().any(|v| v.message.contains(code)),
                "expected {code} to be reported, got {violations:?}"
            );
        }
    }

    #[test]
    fn sidebar_entry_without_a_page_is_reported() {
        let fx = fixture(
            &[("markdown", "Q-2-1"), ("markdown", "Q-2-99")],
            &[("markdown", "Q-2-1")],
        );
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert!(
            violations[0].message.contains("dead link"),
            "expected a dead-link message, got {:?}",
            violations[0].message
        );
        assert!(
            violations[0].message.contains("Q-2-99"),
            "expected Q-2-99 in {:?}",
            violations[0].message
        );
    }

    /// Build a fixture from a literal sidebar body, for tests that need
    /// control over entry order within a section. `body` is spliced in at the
    /// `contents:` indent level; `pages` is the docs tree as usual.
    fn fixture_raw(body: &str, pages: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        let yml = format!(
            "website:\n  sidebar:\n    - id: errors\n      title: \"Error reference\"\n      \
             contents:\n        - errors/index.qmd\n{body}"
        );
        let config_path = root.join(QUARTO_YML_REL);
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(&config_path, yml).unwrap();

        for (subsystem, code) in pages {
            let dir = root.join(DOCS_ERRORS_REL).join(subsystem);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{code}.qmd")), "---\ncode: x\n---\n").unwrap();
        }

        dir
    }

    /// A section whose entries ascend by code number is clean. Note the real
    /// `yaml` section's shape: codes jump 1 → 10 → 29 → 99, so a numerically
    /// ordered section can still *look* lexicographic.
    #[test]
    fn section_in_numeric_code_order_is_clean() {
        let fx = fixture_raw(
            "        - section: \"yaml\"\n          contents:\n            - \
             errors/yaml/Q-1-1.qmd\n            - errors/yaml/Q-1-10.qmd\n            - \
             errors/yaml/Q-1-29.qmd\n            - errors/yaml/Q-1-99.qmd\n",
            &[
                ("yaml", "Q-1-1"),
                ("yaml", "Q-1-10"),
                ("yaml", "Q-1-29"),
                ("yaml", "Q-1-99"),
            ],
        );
        assert!(
            check(fx.path()).unwrap().is_empty(),
            "{:?}",
            check(fx.path()).unwrap()
        );
    }

    /// Lexicographic order within a section is reported: `Q-1-10` must not
    /// precede `Q-1-2`.
    #[test]
    fn out_of_order_entries_within_a_section_are_reported() {
        let fx = fixture_raw(
            "        - section: \"yaml\"\n          contents:\n            - \
             errors/yaml/Q-1-1.qmd\n            - errors/yaml/Q-1-10.qmd\n            - \
             errors/yaml/Q-1-2.qmd\n",
            &[("yaml", "Q-1-1"), ("yaml", "Q-1-10"), ("yaml", "Q-1-2")],
        );
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert!(
            violations[0].message.contains("Q-1-2") && violations[0].message.contains("Q-1-10"),
            "message should name both codes: {:?}",
            violations[0].message
        );
        // Anchored at the offending entry — the `Q-1-2` line, which is line 11
        // of the fixture config (6 header lines, `- section:`, `contents:`,
        // then the three entries).
        assert_eq!(violations[0].line, 11, "{:?}", violations[0]);
    }

    /// Ordering is enforced *within* a section only. The 13 historical
    /// sections are in no particular order and must stay that way.
    #[test]
    fn section_order_is_not_policed() {
        let fx = fixture_raw(
            "        - section: \"lua\"\n          contents:\n            - \
             errors/lua/Q-11-1.qmd\n        - section: \"yaml\"\n          contents:\n            \
             - errors/yaml/Q-1-1.qmd\n        - section: \"cli\"\n          contents:\n            \
             - errors/cli/Q-7-1.qmd\n",
            &[("lua", "Q-11-1"), ("yaml", "Q-1-1"), ("cli", "Q-7-1")],
        );
        assert!(
            check(fx.path()).unwrap().is_empty(),
            "{:?}",
            check(fx.path()).unwrap()
        );
    }

    /// A subsystem number that differs within one section is not this check's
    /// business — the section groups by subsystem, so only the code's second
    /// segment sequences. (Guards against comparing the wrong tuple element.)
    #[test]
    fn ordering_compares_the_code_number_not_the_whole_string() {
        let fx = fixture_raw(
            "        - section: \"markdown\"\n          contents:\n            - \
             errors/markdown/Q-2-9.qmd\n            - errors/markdown/Q-2-50.qmd\n",
            &[("markdown", "Q-2-9"), ("markdown", "Q-2-50")],
        );
        assert!(
            check(fx.path()).unwrap().is_empty(),
            "9 before 50 is ascending: {:?}",
            check(fx.path()).unwrap()
        );
    }

    #[test]
    fn index_page_and_non_page_files_are_ignored() {
        let fx = fixture(&[("markdown", "Q-2-1")], &[("markdown", "Q-2-1")]);
        // `index.qmd` and `README.md` sit alongside the subsystem dirs and are
        // not pages; a stray non-`Q-` file inside a subsystem is not either.
        let errors_root = fx.path().join(DOCS_ERRORS_REL);
        std::fs::write(errors_root.join("index.qmd"), "---\n---\n").unwrap();
        std::fs::write(errors_root.join("README.md"), "# readme\n").unwrap();
        std::fs::write(errors_root.join("markdown").join("_partial.qmd"), "x\n").unwrap();
        assert!(check(fx.path()).unwrap().is_empty());
    }

    #[test]
    fn href_form_entries_count_as_listed() {
        let fx = fixture(&[], &[("markdown", "Q-2-1")]);
        // Rewrite the sidebar using the `- href:` mapping spelling.
        let config_path = fx.path().join(QUARTO_YML_REL);
        std::fs::write(
            &config_path,
            "website:\n  sidebar:\n    - id: errors\n      contents:\n        - href: \
             errors/markdown/Q-2-1.qmd\n",
        )
        .unwrap();
        assert!(check(fx.path()).unwrap().is_empty());
    }

    #[test]
    fn unlisted_page_anchors_at_the_sidebar_declaration() {
        let fx = fixture(&[], &[("markdown", "Q-2-1")]);
        let violations = check(fx.path()).unwrap();
        assert_eq!(violations.len(), 1, "got {violations:?}");
        // `- id: errors` is the third line of the fixture config.
        assert_eq!(violations[0].line, 3);
    }
}

// reference-links: migrate reference-style links to the inline form.
//
// This is the *mechanical* arm of bd-reference-links-unsupported-ddc4skac.
// Every edit it makes is determined by a definition the author already
// wrote, so it never has to guess at intent — that is `literal-brackets`'
// problem, and it lives in a separate rule for exactly that reason.
//
//   Input:  See [the docs][gcc].
//
//           [gcc]: https://example.com/gcc
//   Output: See [the docs](https://example.com/gcc).
//
// Definitions are dropped once their last use is gone. Unused definitions
// are dropped too: Quarto 1 consumes them and renders nothing, while q2
// renders them as a stray visible paragraph, so removing them is what
// restores parity.

use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use crate::conversions::bracket_analysis::{Analysis, Definition, Finding, PartKind, analyze};
use crate::rule::{CheckResult, ConvertResult, Rule};
use crate::utils::file_io::{read_file, write_file};

use super::bracket_edit::{Edit, apply_edits, source_location, tidy_blank_lines};

pub struct ReferenceLinksConverter {}

impl ReferenceLinksConverter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Rewrite every resolvable reference and drop the definitions that are
    /// no longer needed. Returns the new text and the number of distinct
    /// problems fixed.
    fn rewrite(&self, source: &str, analysis: &Analysis) -> (String, usize) {
        // A definition an ambiguous run might resolve against has to survive,
        // so whoever reviews the run by hand still has it.
        let protected: HashSet<&str> = analysis
            .findings
            .iter()
            .filter_map(|f| match f {
                Finding::Ambiguous { labels, .. } => Some(labels),
                _ => None,
            })
            .flatten()
            .map(String::as_str)
            .collect();

        let mut edits = Vec::new();
        let mut fixes = 0;

        for finding in &analysis.findings {
            let Finding::Reference {
                start,
                end,
                kind,
                label,
                definition_label,
            } = finding
            else {
                continue;
            };
            let Some(definition) = analysis.definition(definition_label) else {
                continue;
            };
            edits.push(Edit::replace(
                *start,
                *end,
                inline_form(*kind, label, definition),
            ));
            fixes += 1;
        }

        for definition in &analysis.definitions {
            if protected.contains(definition.label.as_str()) {
                continue;
            }
            // An unused definition is a problem in its own right — it renders
            // as a stray paragraph — so it counts as a fix. A used one was
            // already counted by the reference that consumed it.
            if !analysis.is_used(&definition.label) {
                fixes += 1;
            }
            edits.push(Edit::delete(definition.line_start, definition.line_end));
        }

        if edits.is_empty() {
            return (source.to_string(), 0);
        }

        (tidy_blank_lines(&apply_edits(source, edits)), fixes)
    }
}

/// Render a reference as an inline link or image.
fn inline_form(kind: PartKind, label: &str, definition: &Definition) -> String {
    let bang = match kind {
        PartKind::Image => "!",
        PartKind::Span => "",
    };
    let title = match &definition.title {
        // qmd inline links accept only double-quoted titles, so a single- or
        // paren-quoted definition title has to be re-quoted rather than
        // copied through.
        Some(title) => format!(" \"{}\"", title.replace('"', "\\\"")),
        None => String::new(),
    };
    format!("{bang}[{label}]({}{title})", encode_url(&definition.url))
}

/// Percent-encode the characters qmd's url token forbids.
///
/// Backslash escapes are *not* an option: q2 parses `[a](u\ v)` but leaves
/// the backslash in the `href`, silently producing a broken link. This
/// matches the existing Q-2-33 converter, which rewrites spaces as `%20`.
fn encode_url(url: &str) -> String {
    url.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '\t' => "%09".to_string(),
            ')' => "%29".to_string(),
            '{' => "%7B".to_string(),
            other => other.to_string(),
        })
        .collect()
}

impl Rule for ReferenceLinksConverter {
    fn name(&self) -> &str {
        "reference-links"
    }

    fn description(&self) -> &str {
        "Migrate reference-style links and images to the inline form"
    }

    /// Findings come from walking the parsed AST; without one there is
    /// nothing to say, and "no findings" must not read as clean.
    fn requires_parse(&self) -> bool {
        true
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        let content = read_file(file_path)?;
        let analysis = analyze(&content, &file_path.to_string_lossy())?;
        let path = file_path.to_string_lossy().to_string();

        let mut results = Vec::new();

        for finding in &analysis.findings {
            let text = content
                .get(finding.start()..finding.end())
                .unwrap_or_default();

            let message = match finding {
                Finding::Reference {
                    definition_label, ..
                } => format!(
                    "`{text}` is a reference-style link to `{definition_label}`, which does \
                     not render; it can be rewritten to the inline form"
                ),
                Finding::Ambiguous { count, .. } => format!(
                    "`{text}` has {count} adjacent bracketed groups and is ambiguous; \
                     left unchanged for human review"
                ),
                // Undefined brackets belong to `literal-brackets`.
                Finding::Literal { .. } => continue,
            };

            results.push(CheckResult {
                rule_name: self.name().to_string(),
                file_path: path.clone(),
                has_issue: true,
                issue_count: 1,
                message: Some(message),
                location: Some(source_location(&content, finding.start())),
                error_code: None,
                error_codes: None,
                ..Default::default()
            });
        }

        for definition in &analysis.definitions {
            if analysis.is_used(&definition.label) {
                continue;
            }
            results.push(CheckResult {
                rule_name: self.name().to_string(),
                file_path: path.clone(),
                has_issue: true,
                issue_count: 1,
                message: Some(format!(
                    "Unused link reference definition `{}` renders as a visible paragraph",
                    definition.label
                )),
                location: Some(source_location(&content, definition.line_start)),
                error_code: None,
                error_codes: None,
                ..Default::default()
            });
        }

        Ok(results)
    }

    fn convert(
        &self,
        file_path: &Path,
        in_place: bool,
        check_mode: bool,
        _verbose: bool,
    ) -> Result<ConvertResult> {
        let content = read_file(file_path)?;
        let analysis = analyze(&content, &file_path.to_string_lossy())?;
        let (rewritten, fixes) = self.rewrite(&content, &analysis);

        let name = self.name().to_string();
        let path = file_path.to_string_lossy().to_string();

        if fixes == 0 && rewritten == content {
            return Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: 0,
                message: Some(if in_place || check_mode {
                    "No reference-style links found".to_string()
                } else {
                    content
                }),
            });
        }

        if check_mode {
            return Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: fixes,
                message: Some(format!("Would migrate {fixes} reference-style link(s)")),
            });
        }

        if in_place {
            write_file(file_path, &rewritten)?;
            return Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: fixes,
                message: Some(format!("Migrated {fixes} reference-style link(s)")),
            });
        }

        Ok(ConvertResult {
            rule_name: name,
            file_path: path,
            fixes_applied: fixes,
            message: Some(rewritten),
        })
    }
}

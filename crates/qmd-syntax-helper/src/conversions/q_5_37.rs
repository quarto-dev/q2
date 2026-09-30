// Q-5-37: Brand File Not Referenced
//
// Quarto 1 applies a project's `_brand.yml` (or `_brand.yaml`,
// `_brand/_brand.yml`, `_brand/_brand.yaml`) without any `brand:` key;
// Quarto 2 requires one. This rule finds the `_quarto.yml` above a `.qmd`
// file and, when such a brand file exists but nothing references it,
// appends `brand: <file>` to that `_quarto.yml`.
//
// It skips a document whose front matter or `_metadata.yml` layers already
// declare `brand:` (the layers Quarto merges per document), matching the
// per-document Q-5-37 diagnostic; profile overlays are not modeled.
//
// Unlike the other rules this one edits a file other than the one it was
// given, and the edit turns a brand *on* — so it is `opt_in_only`: `check
// --rule all` reports it, `convert --rule all` does not apply it.
//
// Error catalog entry: crates/quarto-error-catalog/error_catalog.json
// Error code: Q-5-37
//
// Example (`_quarto.yml`, with a sibling `_brand.yml`):
//   Input:  project:\n  type: website\n
//   Output: project:\n  type: website\n\n# Added by qmd-syntax-helper (Q-5-37)\nbrand: _brand.yml\n

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::rule::{CheckResult, ConvertResult, Rule};
use crate::utils::file_io::{read_file, write_file};

/// The brand files Quarto 1 discovers, in its probe order. Q1's loop has
/// no `break`, so the **last** existing one is what its renders used.
const Q1_BRAND_FILES: [&str; 4] = [
    "_brand.yml",
    "_brand.yaml",
    "_brand/_brand.yml",
    "_brand/_brand.yaml",
];

const CONFIG_FILES: [&str; 2] = ["_quarto.yml", "_quarto.yaml"];

pub struct Q537Converter {}

/// An unreferenced brand file and the config that should reference it.
#[derive(Debug)]
struct Q537Finding {
    config_path: PathBuf,
    config_text: String,
    brand_file: &'static str,
}

impl Q537Converter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    fn find_config(file_path: &Path) -> Option<PathBuf> {
        let start = file_path.parent()?;
        start.ancestors().find_map(|dir| {
            CONFIG_FILES
                .iter()
                .map(|name| dir.join(name))
                .find(|p| p.is_file())
        })
    }

    fn find(&self, file_path: &Path) -> Result<Option<Q537Finding>> {
        let Some(config_path) = Self::find_config(file_path) else {
            return Ok(None);
        };
        let root = config_path.parent().unwrap_or(Path::new("."));
        let Some(brand_file) = Q1_BRAND_FILES
            .iter()
            .rev()
            .find(|name| root.join(name).is_file())
        else {
            return Ok(None);
        };

        let config_text = read_file(&config_path)?;
        let value: serde_yaml::Value = serde_yaml::from_str(&config_text)
            .with_context(|| format!("Failed to parse {}", config_path.display()))?;
        let declared = value.get("brand").is_some()
            || value
                .get("format")
                .and_then(|f| f.as_mapping())
                .is_some_and(|m| m.values().any(|fmt| fmt.get("brand").is_some()));
        if declared || Self::declared_closer_to_document(file_path, root)? {
            return Ok(None);
        }
        Ok(Some(Q537Finding {
            config_path,
            config_text,
            brand_file,
        }))
    }

    /// Whether a `brand:` is declared closer to the document than
    /// `_quarto.yml`: in the document's front matter, or in a
    /// `_metadata.yml` in the document's directory or any directory up to
    /// (not including) the project root. This mirrors the layers Quarto
    /// merges per document, so the rule agrees with the Q-5-37 diagnostic
    /// for the common cases. It does not model profile overlays.
    fn declared_closer_to_document(file_path: &Path, root: &Path) -> Result<bool> {
        let has_brand = |text: &str| {
            serde_yaml::from_str::<serde_yaml::Value>(text)
                .ok()
                .is_some_and(|v| v.get("brand").is_some())
        };

        if let Ok(doc) = std::fs::read_to_string(file_path)
            && let Some(rest) = doc.strip_prefix("---\n")
            && let Some(end) = rest.find("\n---")
            && has_brand(&rest[..end])
        {
            return Ok(true);
        }

        let mut dir = file_path.parent();
        while let Some(d) = dir {
            if d == root {
                break;
            }
            for name in ["_metadata.yml", "_metadata.yaml"] {
                let candidate = d.join(name);
                if candidate.is_file() && has_brand(&read_file(&candidate)?) {
                    return Ok(true);
                }
            }
            dir = d.parent();
        }
        Ok(false)
    }

    fn fixed_config(finding: &Q537Finding) -> String {
        let mut text = finding.config_text.clone();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!(
            "\n# Added by qmd-syntax-helper (Q-5-37)\nbrand: {}\n",
            finding.brand_file
        ));
        text
    }
}

impl Rule for Q537Converter {
    fn name(&self) -> &str {
        "q-5-37"
    }

    fn description(&self) -> &str {
        "Fix Q-5-37: Add a `brand:` key to _quarto.yml for an unreferenced brand file"
    }

    fn opt_in_only(&self) -> bool {
        true
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        Ok(self
            .find(file_path)?
            .map(|f| CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some(format!(
                    "Q-5-37 {} is not referenced by a `brand:` key in {}",
                    f.brand_file,
                    f.config_path.display()
                )),
                error_code: Some("Q-5-37".to_string()),
                ..Default::default()
            })
            .into_iter()
            .collect())
    }

    fn convert(
        &self,
        file_path: &Path,
        in_place: bool,
        check_mode: bool,
        _verbose: bool,
    ) -> Result<ConvertResult> {
        let result = |fixes_applied, message: String| ConvertResult {
            rule_name: self.name().to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            fixes_applied,
            message: Some(message),
        };

        let Some(finding) = self.find(file_path)? else {
            return Ok(result(0, "No Q-5-37 unreferenced brand file found".into()));
        };
        let fixed = Self::fixed_config(&finding);

        if check_mode {
            return Ok(result(
                1,
                format!(
                    "Would add `brand: {}` to {}",
                    finding.brand_file,
                    finding.config_path.display()
                ),
            ));
        }
        if in_place {
            write_file(&finding.config_path, &fixed)?;
            Ok(result(
                1,
                format!(
                    "Added `brand: {}` to {}",
                    finding.brand_file,
                    finding.config_path.display()
                ),
            ))
        } else {
            // Consistent with the other rules: the converted content, here
            // that of the `_quarto.yml` this rule edits.
            Ok(result(1, fixed))
        }
    }
}

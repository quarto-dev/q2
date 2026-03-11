/*
 * quarto-test/src/spec.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Test specification parsing from YAML metadata.
 */

//! Parsing of `_quarto.tests` YAML metadata into test specifications.

use std::path::Path;

use anyhow::{Context, Result};
use serde_yaml::Value;

use crate::assertions::{
    Assertion, EnsureCssRegexMatches, EnsureFileRegexMatches, EnsureHtmlElements, FileExists,
    FolderExists, NoErrors, NoErrorsOrWarnings, PathDoesNotExist, PrintsMessage, ShouldError,
};

/// Configuration for when/whether to run tests.
#[derive(Debug, Clone, Default)]
pub struct RunConfig {
    /// Skip this test entirely (with optional reason).
    pub skip: Option<String>,
    /// Whether to skip on CI (if false, skip when running in CI).
    pub ci: Option<bool>,
    /// Only run on these operating systems.
    pub os: Option<Vec<String>>,
    /// Do not run on these operating systems.
    pub not_os: Option<Vec<String>>,
}

impl RunConfig {
    /// Parse run configuration from YAML value.
    pub fn from_yaml(value: &Value) -> Result<Self> {
        let mut config = RunConfig::default();

        if let Some(map) = value.as_mapping() {
            if let Some(skip) = map.get("skip") {
                config.skip = match skip {
                    Value::Bool(true) => Some("skip: true".to_string()),
                    Value::Bool(false) => None,
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                };
            }

            if let Some(ci) = map.get("ci") {
                config.ci = ci.as_bool();
            }

            if let Some(os) = map.get("os") {
                config.os = parse_string_or_array(os);
            }

            if let Some(not_os) = map.get("not_os") {
                config.not_os = parse_string_or_array(not_os);
            }
        }

        Ok(config)
    }

    /// Check if tests should be skipped based on current environment.
    pub fn should_skip(&self) -> Option<String> {
        // Explicit skip
        if let Some(reason) = &self.skip {
            return Some(reason.clone());
        }

        // CI check
        if let Some(false) = self.ci {
            if is_ci() {
                return Some("tests.run.ci is false".to_string());
            }
        }

        // OS whitelist
        if let Some(allowed_os) = &self.os {
            let current = current_os();
            if !allowed_os.iter().any(|os| os == current) {
                return Some(format!("tests.run.os does not include {}", current));
            }
        }

        // OS blacklist
        if let Some(blocked_os) = &self.not_os {
            let current = current_os();
            if blocked_os.iter().any(|os| os == current) {
                return Some(format!("tests.run.not_os includes {}", current));
            }
        }

        None
    }
}

/// A test specification for a single format.
#[derive(Debug)]
pub struct TestSpec {
    /// The format to render (e.g., "html", "pdf").
    pub format: String,
    /// Assertions to run after rendering.
    pub assertions: Vec<Box<dyn Assertion>>,
    /// Whether to check for errors/warnings (default behavior).
    pub check_warnings: bool,
    /// Whether render failure is expected (shouldError).
    pub expects_error: bool,
}

/// Parse test specifications from document YAML metadata.
///
/// Looks for `_quarto.tests` in the metadata and extracts test specs
/// for each format key found.
pub fn parse_test_specs(
    metadata: &Value,
    input_path: &Path,
) -> Result<(Option<RunConfig>, Vec<TestSpec>)> {
    let quarto = match metadata.get("_quarto") {
        Some(v) => v,
        None => return Ok((None, vec![])),
    };

    let tests = match quarto.get("tests") {
        Some(v) => v,
        None => return Ok((None, vec![])),
    };

    let tests_map = tests
        .as_mapping()
        .context("_quarto.tests must be a mapping")?;

    // Parse run configuration
    let run_config = tests_map.get("run").map(RunConfig::from_yaml).transpose()?;

    // Parse format-specific test specs
    let mut specs = Vec::new();

    for (format_key, format_value) in tests_map {
        let format_name = format_key.as_str().context("format key must be a string")?;

        // Skip the "run" key - it's configuration, not a format
        if format_name == "run" {
            continue;
        }

        let spec = parse_format_spec(format_name, format_value, input_path)
            .with_context(|| format!("parsing test spec for format '{}'", format_name))?;
        specs.push(spec);
    }

    Ok((run_config, specs))
}

/// Parse a single format's test specification.
fn parse_format_spec(format: &str, value: &Value, _input_path: &Path) -> Result<TestSpec> {
    let mut assertions: Vec<Box<dyn Assertion>> = Vec::new();
    let mut check_warnings = true;
    let mut expects_error = false;

    if let Some(map) = value.as_mapping() {
        for (key, assertion_value) in map {
            let key_str = key.as_str().context("assertion key must be a string")?;

            match key_str {
                "ensureHtmlElements" => {
                    let assertion = parse_ensure_html_elements(assertion_value)?;
                    assertions.push(Box::new(assertion));
                }
                "ensureFileRegexMatches" => {
                    let assertion = parse_ensure_file_regex_matches(assertion_value)?;
                    assertions.push(Box::new(assertion));
                }
                "ensureCssRegexMatches" => {
                    let assertion = parse_ensure_css_regex_matches(assertion_value)?;
                    assertions.push(Box::new(assertion));
                }
                "noErrors" => {
                    check_warnings = false;
                    assertions.push(Box::new(NoErrors::new()));
                }
                "noErrorsOrWarnings" => {
                    check_warnings = false;
                    assertions.push(Box::new(NoErrorsOrWarnings::new()));
                }
                "shouldError" => {
                    check_warnings = false;
                    expects_error = true;
                    // Value can be "default" or true, we just need presence
                    let _ = assertion_value; // Acknowledge unused
                    assertions.push(Box::new(ShouldError::new()));
                }
                "printsMessage" => {
                    // Support both single object and array of printsMessage checks
                    if let Some(arr) = assertion_value.as_sequence() {
                        for item in arr {
                            let assertion = parse_prints_message(item)?;
                            assertions.push(Box::new(assertion));
                        }
                    } else {
                        let assertion = parse_prints_message(assertion_value)?;
                        assertions.push(Box::new(assertion));
                    }
                }
                "fileExists" => {
                    parse_file_exists(assertion_value, &mut assertions)?;
                }
                // Support both spellings
                "pathDoesNotExist" | "pathDoNotExists" => {
                    let path = assertion_value
                        .as_str()
                        .context("pathDoesNotExist must be a string path")?
                        .to_string();
                    assertions.push(Box::new(PathDoesNotExist::new(path)));
                }
                "folderExists" => {
                    let path = assertion_value
                        .as_str()
                        .context("folderExists must be a string path")?
                        .to_string();
                    assertions.push(Box::new(FolderExists::new(path)));
                }
                other => {
                    anyhow::bail!("Unknown assertion type: '{}' in format '{}'", other, format);
                }
            }
        }
    }

    Ok(TestSpec {
        format: format.to_string(),
        assertions,
        check_warnings,
        expects_error,
    })
}

/// Parse `fileExists` assertion.
///
/// Supports the TS Quarto format:
/// ```yaml
/// fileExists:
///   outputPath: "filename.html"    # relative to output directory
///   supportPath: "filename.css"    # relative to support files directory
/// ```
fn parse_file_exists(value: &Value, assertions: &mut Vec<Box<dyn Assertion>>) -> Result<()> {
    let map = value
        .as_mapping()
        .context("fileExists must be a mapping with outputPath and/or supportPath keys")?;

    for (key, file_value) in map {
        let key_str = key.as_str().context("fileExists key must be a string")?;
        let file = file_value
            .as_str()
            .context("fileExists value must be a string path")?
            .to_string();

        match key_str {
            "outputPath" => {
                assertions.push(Box::new(FileExists::new(file)));
            }
            "supportPath" => {
                assertions.push(Box::new(FileExists::new_support_path(file)));
            }
            other => {
                anyhow::bail!(
                    "Unknown fileExists key: '{}' (expected 'outputPath' or 'supportPath')",
                    other
                );
            }
        }
    }

    Ok(())
}

/// Parse `ensureCssRegexMatches` assertion.
///
/// Same format as `ensureFileRegexMatches` but checks linked CSS files
/// instead of the output HTML.
fn parse_ensure_css_regex_matches(value: &Value) -> Result<EnsureCssRegexMatches> {
    let arr = value
        .as_sequence()
        .context("ensureCssRegexMatches must be an array")?;

    let matches = if !arr.is_empty() {
        parse_pattern_array(&arr[0])?
    } else {
        vec![]
    };

    let no_matches = if arr.len() > 1 {
        parse_pattern_array(&arr[1])?
    } else {
        vec![]
    };

    EnsureCssRegexMatches::new(matches, no_matches)
}

/// Parse `ensureFileRegexMatches` assertion.
///
/// Format:
/// ```yaml
/// ensureFileRegexMatches:
///   - ["pattern1", "pattern2"]  # must match
///   - ["noMatch1", "noMatch2"]  # must NOT match (optional)
/// ```
fn parse_ensure_file_regex_matches(value: &Value) -> Result<EnsureFileRegexMatches> {
    let arr = value
        .as_sequence()
        .context("ensureFileRegexMatches must be an array")?;

    let matches = if !arr.is_empty() {
        parse_pattern_array(&arr[0])?
    } else {
        vec![]
    };

    let no_matches = if arr.len() > 1 {
        parse_pattern_array(&arr[1])?
    } else {
        vec![]
    };

    EnsureFileRegexMatches::new(matches, no_matches)
}

/// Parse `ensureHtmlElements` assertion.
///
/// Format (same two-array structure as ensureFileRegexMatches):
/// ```yaml
/// ensureHtmlElements:
///   - ["nav#TOC", "div.cell"]          # selectors that must match
///   - ["div.do-not-be-here"]           # selectors that must NOT match (optional)
/// ```
fn parse_ensure_html_elements(value: &Value) -> Result<EnsureHtmlElements> {
    let arr = value
        .as_sequence()
        .context("ensureHtmlElements must be an array")?;

    let selectors = if !arr.is_empty() {
        parse_pattern_array(&arr[0])?
    } else {
        vec![]
    };

    let no_match_selectors = if arr.len() > 1 {
        parse_pattern_array(&arr[1])?
    } else {
        vec![]
    };

    EnsureHtmlElements::new(selectors, no_match_selectors)
}

/// Parse `printsMessage` assertion.
///
/// Format:
/// ```yaml
/// printsMessage:
///   level: ERROR  # DEBUG, INFO, WARN, ERROR
///   regex: "pattern to match"
///   negate: false  # optional, defaults to false
/// ```
fn parse_prints_message(value: &Value) -> Result<PrintsMessage> {
    let map = value
        .as_mapping()
        .context("printsMessage must be a mapping")?;

    let level_str = map
        .get("level")
        .and_then(|v| v.as_str())
        .context("printsMessage.level is required")?;

    let level = PrintsMessage::parse_level(level_str)?;

    let regex = map
        .get("regex")
        .and_then(|v| v.as_str())
        .context("printsMessage.regex is required")?
        .to_string();

    let negate = map.get("negate").and_then(|v| v.as_bool()).unwrap_or(false);

    PrintsMessage::new(level, regex, negate)
}

/// Parse an array of regex patterns from YAML.
fn parse_pattern_array(value: &Value) -> Result<Vec<String>> {
    match value {
        Value::Sequence(arr) => arr
            .iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .context("pattern must be a string")
            })
            .collect(),
        Value::Null => Ok(vec![]),
        _ => anyhow::bail!("expected array of patterns"),
    }
}

/// Parse a string or array of strings from YAML.
fn parse_string_or_array(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(s) => Some(vec![s.clone()]),
        Value::Sequence(arr) => {
            let strings: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if strings.is_empty() {
                None
            } else {
                Some(strings)
            }
        }
        _ => None,
    }
}

/// Check if running in CI environment.
fn is_ci() -> bool {
    std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok()
}

/// Get the current operating system name.
fn current_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_run_config_skip() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            skip: true
            "#,
        )
        .unwrap();

        let config = RunConfig::from_yaml(&yaml).unwrap();
        assert!(config.skip.is_some());
    }

    #[test]
    fn test_parse_run_config_skip_with_message() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            skip: "needs special setup"
            "#,
        )
        .unwrap();

        let config = RunConfig::from_yaml(&yaml).unwrap();
        assert_eq!(config.skip, Some("needs special setup".to_string()));
    }

    #[test]
    fn test_parse_run_config_os() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            os: [darwin, linux]
            "#,
        )
        .unwrap();

        let config = RunConfig::from_yaml(&yaml).unwrap();
        assert_eq!(
            config.os,
            Some(vec!["darwin".to_string(), "linux".to_string()])
        );
    }

    #[test]
    fn test_parse_ensure_file_regex_matches() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            - ["pattern1", "pattern2"]
            - ["noMatch"]
            "#,
        )
        .unwrap();

        let assertion = parse_ensure_file_regex_matches(&yaml).unwrap();
        assert_eq!(assertion.matches.len(), 2);
        assert_eq!(assertion.no_matches.len(), 1);
    }

    #[test]
    fn test_unknown_assertion_fails() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  unknownAssertion: true
            "#,
        )
        .unwrap();

        let result = parse_test_specs(&yaml, std::path::Path::new("test.qmd"));
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("Unknown assertion type") && err.contains("unknownAssertion"),
            "Expected error about unknown assertion, got: {}",
            err
        );
    }

    #[test]
    fn test_file_exists_output_path() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  noErrors: true
                  fileExists:
                    outputPath: "test.html"
            "#,
        )
        .unwrap();

        let (_, specs) = parse_test_specs(&yaml, std::path::Path::new("test.qmd")).unwrap();
        assert_eq!(specs.len(), 1);
        // noErrors + fileExists = 2 assertions
        assert_eq!(specs[0].assertions.len(), 2);
        assert_eq!(specs[0].assertions[1].name(), "fileExists");
    }

    #[test]
    fn test_file_exists_support_path() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  noErrors: true
                  fileExists:
                    supportPath: "styles.css"
            "#,
        )
        .unwrap();

        let (_, specs) = parse_test_specs(&yaml, std::path::Path::new("test.qmd")).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].assertions.len(), 2);
        assert_eq!(specs[0].assertions[1].name(), "fileExists");
    }

    #[test]
    fn test_file_exists_both_paths() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  noErrors: true
                  fileExists:
                    outputPath: "test.html"
                    supportPath: "styles.css"
            "#,
        )
        .unwrap();

        let (_, specs) = parse_test_specs(&yaml, std::path::Path::new("test.qmd")).unwrap();
        assert_eq!(specs.len(), 1);
        // noErrors + 2 fileExists = 3 assertions
        assert_eq!(specs[0].assertions.len(), 3);
    }

    #[test]
    fn test_prints_message_single() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  shouldError: default
                  printsMessage:
                    level: ERROR
                    regex: "test error"
            "#,
        )
        .unwrap();

        let (_, specs) = parse_test_specs(&yaml, std::path::Path::new("test.qmd")).unwrap();
        assert_eq!(specs.len(), 1);
        // shouldError + printsMessage = 2 assertions
        assert_eq!(specs[0].assertions.len(), 2);
    }

    #[test]
    fn test_prints_message_array() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  shouldError: default
                  printsMessage:
                    - level: ERROR
                      regex: "first error"
                    - level: WARN
                      regex: "a warning"
            "#,
        )
        .unwrap();

        let (_, specs) = parse_test_specs(&yaml, std::path::Path::new("test.qmd")).unwrap();
        assert_eq!(specs.len(), 1);
        // shouldError + 2 printsMessage = 3 assertions
        assert_eq!(specs[0].assertions.len(), 3);
    }

    #[test]
    fn test_ensure_html_elements_parsed() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  noErrors: true
                  ensureHtmlElements:
                    - ["nav#TOC"]
            "#,
        )
        .unwrap();

        let (_, specs) = parse_test_specs(&yaml, std::path::Path::new("test.qmd")).unwrap();
        assert_eq!(specs.len(), 1);
        // noErrors + ensureHtmlElements
        assert_eq!(specs[0].assertions.len(), 2);
        assert_eq!(specs[0].assertions[1].name(), "ensureHtmlElements");
    }

    #[test]
    fn test_parse_ensure_file_regex_matches_empty_no_matches() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            - ["pattern1"]
            - []
            "#,
        )
        .unwrap();

        let assertion = parse_ensure_file_regex_matches(&yaml).unwrap();
        assert_eq!(assertion.matches.len(), 1);
        assert_eq!(assertion.no_matches.len(), 0);
    }
}

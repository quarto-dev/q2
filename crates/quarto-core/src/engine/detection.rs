/*
 * engine/detection.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Engine detection from document metadata.
 */

//! Engine detection from document metadata.
//!
//! This module provides functionality to determine which execution engine
//! should be used for a Quarto document based on its YAML frontmatter.
//!
//! # Detection Algorithm
//!
//! The detection checks for engine declarations in this order:
//!
//! 1. Explicit `engine:` key with string value: `engine: knitr`
//! 2. Explicit `engine:` key with map value: `engine: { jupyter: { kernel: python3 } }`
//! 3. Engine-specific top-level keys: `jupyter: { kernel: python3 }`
//! 4. Default to "markdown" if no engine is declared
//!
//! # Future Enhancements
//!
//! In future phases, detection will also consider:
//! - Code block languages (`{python}` → jupyter, `{r}` → knitr)
//! - File extension (`.ipynb` → jupyter, `.Rmd` → knitr)

use quarto_pandoc_types::ConfigValue;

/// Known execution engine names.
pub const KNOWN_ENGINES: &[&str] = &["markdown", "knitr", "jupyter"];

/// Result of engine detection.
///
/// Contains the detected engine name and any configuration
/// specified in the document metadata.
#[derive(Debug, Clone)]
pub struct DetectedEngine {
    /// Engine name (e.g., "markdown", "knitr", "jupyter").
    pub name: String,

    /// Engine-specific configuration from YAML, if any.
    ///
    /// For `engine: { jupyter: { kernel: python3 } }`, this would be
    /// the `{ kernel: python3 }` ConfigValue.
    pub config: Option<ConfigValue>,
}

impl DetectedEngine {
    /// Create a new detected engine with just a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            config: None,
        }
    }

    /// Create a detected engine with configuration.
    pub fn with_config(name: impl Into<String>, config: ConfigValue) -> Self {
        Self {
            name: name.into(),
            config: Some(config),
        }
    }

    /// Check if this is the markdown (no-op) engine.
    pub fn is_markdown(&self) -> bool {
        self.name == "markdown"
    }

    /// Check if this engine requires external runtimes.
    ///
    /// Returns true for knitr (needs R) and jupyter (needs Python/Julia).
    pub fn requires_runtime(&self) -> bool {
        matches!(self.name.as_str(), "knitr" | "jupyter")
    }
}

impl Default for DetectedEngine {
    fn default() -> Self {
        Self::new("markdown")
    }
}

/// Check if a name is a known engine.
pub fn is_known_engine(name: &str) -> bool {
    KNOWN_ENGINES.contains(&name)
}

/// Extract a string value from a ConfigValue.
///
/// This handles multiple representations:
/// - `Scalar(Yaml::String(s))` → Some(s)
/// - `Path(s)`, `Glob(s)`, `Expr(s)` → Some(s)
/// - `PandocInlines` with single Str → Some(text)
///
/// Returns None for non-string values.
fn extract_string_value(value: &ConfigValue) -> Option<&str> {
    // First try the simple case
    if let Some(s) = value.as_str() {
        return Some(s);
    }

    // Check for PandocInlines with single Str
    // This handles YAML strings that were parsed as markdown
    use quarto_pandoc_types::Inline;
    use quarto_pandoc_types::config_value::ConfigValueKind;

    if let ConfigValueKind::PandocInlines(inlines) = &value.value {
        if inlines.len() == 1 {
            if let Inline::Str(str_node) = &inlines[0] {
                return Some(&str_node.text);
            }
        }
    }

    None
}

/// Detect the execution engine from document metadata.
///
/// Examines the document's YAML frontmatter to determine which
/// execution engine should be used for code cells.
///
/// # Arguments
///
/// * `metadata` - The document's metadata (from `Pandoc.meta`)
///
/// # Returns
///
/// A `DetectedEngine` with the engine name and any configuration.
/// Defaults to "markdown" if no engine is specified.
///
/// # Examples
///
/// ```ignore
/// // engine: knitr
/// let detected = detect_engine(&meta);
/// assert_eq!(detected.name, "knitr");
///
/// // engine: { jupyter: { kernel: python3 } }
/// let detected = detect_engine(&meta);
/// assert_eq!(detected.name, "jupyter");
/// assert!(detected.config.is_some());
///
/// // No engine specified
/// let detected = detect_engine(&meta);
/// assert_eq!(detected.name, "markdown");
/// ```
pub fn detect_engine(metadata: &ConfigValue) -> DetectedEngine {
    // Case 1: Look for explicit "engine" key
    if let Some(engine_value) = metadata.get("engine") {
        // Case 1a: engine: markdown|knitr|jupyter (string value)
        if let Some(name) = extract_string_value(engine_value) {
            // Return the engine name even if unknown - the pipeline stage
            // will handle fallback and warning for unknown engines
            return DetectedEngine::new(name);
        }

        // Case 1b: engine: { knitr: ... } or engine: { jupyter: ... }
        if let Some(entries) = engine_value.as_map_entries() {
            // The first key should be the engine name
            if let Some(first_entry) = entries.first() {
                let engine_name = &first_entry.key;

                // Return even if unknown - the pipeline stage will
                // handle fallback and warning for unknown engines
                return DetectedEngine::with_config(engine_name.clone(), first_entry.value.clone());
            }
        }
    }

    // Case 2: Look for engine-specific top-level keys
    // This handles cases like:
    //   jupyter:
    //     kernel: python3
    for engine_name in KNOWN_ENGINES {
        // Skip "markdown" - it doesn't have a top-level config key
        if *engine_name == "markdown" {
            continue;
        }

        if let Some(config) = metadata.get(engine_name) {
            return DetectedEngine::with_config(engine_name.to_string(), config.clone());
        }
    }

    // Default: markdown engine (no execution)
    DetectedEngine::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    /// Helper to create a simple string ConfigValue
    fn string_config(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::default())
    }

    /// Helper to create a map ConfigValue
    fn map_config(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(key, value)| ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::default(),
                value,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    // === DetectedEngine tests ===

    #[test]
    fn test_detected_engine_new() {
        let engine = DetectedEngine::new("knitr");
        assert_eq!(engine.name, "knitr");
        assert!(engine.config.is_none());
    }

    #[test]
    fn test_detected_engine_with_config() {
        let config = string_config("python3");
        let engine = DetectedEngine::with_config("jupyter", config);
        assert_eq!(engine.name, "jupyter");
        assert!(engine.config.is_some());
    }

    #[test]
    fn test_detected_engine_default() {
        let engine = DetectedEngine::default();
        assert_eq!(engine.name, "markdown");
        assert!(engine.config.is_none());
    }

    #[test]
    fn test_detected_engine_is_markdown() {
        assert!(DetectedEngine::new("markdown").is_markdown());
        assert!(!DetectedEngine::new("knitr").is_markdown());
        assert!(!DetectedEngine::new("jupyter").is_markdown());
    }

    #[test]
    fn test_detected_engine_requires_runtime() {
        assert!(!DetectedEngine::new("markdown").requires_runtime());
        assert!(DetectedEngine::new("knitr").requires_runtime());
        assert!(DetectedEngine::new("jupyter").requires_runtime());
    }

    // === is_known_engine tests ===

    #[test]
    fn test_is_known_engine() {
        assert!(is_known_engine("markdown"));
        assert!(is_known_engine("knitr"));
        assert!(is_known_engine("jupyter"));
        assert!(!is_known_engine("unknown"));
        assert!(!is_known_engine(""));
    }

    // === detect_engine tests ===

    #[test]
    fn test_detect_engine_simple_string() {
        // engine: knitr
        let meta = map_config(vec![("engine", string_config("knitr"))]);

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "knitr");
        assert!(detected.config.is_none());
    }

    #[test]
    fn test_detect_engine_simple_string_jupyter() {
        // engine: jupyter
        let meta = map_config(vec![("engine", string_config("jupyter"))]);

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "jupyter");
    }

    #[test]
    fn test_detect_engine_simple_string_markdown() {
        // engine: markdown
        let meta = map_config(vec![("engine", string_config("markdown"))]);

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "markdown");
    }

    #[test]
    fn test_detect_engine_map_with_config() {
        // engine:
        //   jupyter:
        //     kernel: python3
        let jupyter_config = map_config(vec![("kernel", string_config("python3"))]);
        let engine_value = map_config(vec![("jupyter", jupyter_config)]);
        let meta = map_config(vec![("engine", engine_value)]);

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "jupyter");
        assert!(detected.config.is_some());

        // Verify config contains kernel
        let config = detected.config.unwrap();
        assert!(config.get("kernel").is_some());
    }

    #[test]
    fn test_detect_engine_map_with_default_value() {
        // engine:
        //   knitr: default
        let engine_value = map_config(vec![("knitr", string_config("default"))]);
        let meta = map_config(vec![("engine", engine_value)]);

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "knitr");
        assert!(detected.config.is_some());

        // Config should be the "default" string value
        let config = detected.config.unwrap();
        assert!(config.is_string_value("default"));
    }

    #[test]
    fn test_detect_engine_top_level_key() {
        // jupyter:
        //   kernel: python3
        let jupyter_config = map_config(vec![("kernel", string_config("python3"))]);
        let meta = map_config(vec![("jupyter", jupyter_config)]);

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "jupyter");
        assert!(detected.config.is_some());
    }

    #[test]
    fn test_detect_engine_top_level_knitr() {
        // knitr:
        //   opts_chunk:
        //     echo: false
        let opts = map_config(vec![("echo", string_config("false"))]);
        let knitr_config = map_config(vec![("opts_chunk", opts)]);
        let meta = map_config(vec![("knitr", knitr_config)]);

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "knitr");
        assert!(detected.config.is_some());
    }

    #[test]
    fn test_detect_engine_default_no_engine_key() {
        // title: My Document
        // (no engine key)
        let meta = map_config(vec![("title", string_config("My Document"))]);

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "markdown");
        assert!(detected.config.is_none());
    }

    #[test]
    fn test_detect_engine_empty_metadata() {
        let meta = ConfigValue::new_map(vec![], SourceInfo::default());

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "markdown");
    }

    #[test]
    fn test_detect_engine_unknown_engine_returns_unknown_name() {
        // engine: unknown-engine
        // Detection returns the name as-is; pipeline stage handles fallback/warning
        let meta = map_config(vec![("engine", string_config("unknown-engine"))]);

        let detected = detect_engine(&meta);
        // Returns the unknown name - pipeline stage will handle fallback
        assert_eq!(detected.name, "unknown-engine");
    }

    #[test]
    fn test_detect_engine_engine_key_takes_precedence() {
        // Both engine: and jupyter: are present
        // engine: takes precedence
        let jupyter_config = map_config(vec![("kernel", string_config("python3"))]);
        let meta = map_config(vec![
            ("engine", string_config("knitr")),
            ("jupyter", jupyter_config),
        ]);

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "knitr");
    }

    #[test]
    fn test_detect_engine_null_metadata() {
        let meta = ConfigValue::null(SourceInfo::default());

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "markdown");
    }
}

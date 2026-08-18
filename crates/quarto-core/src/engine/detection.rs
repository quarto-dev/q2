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
//! - File extension (`.ipynb` → jupyter, `.Rmd` → knitr)
//!
//! Code block languages (`{python}` → jupyter, `{r}` → knitr) are now
//! handled in `engine/resolution.rs` via `computational_languages` + the
//! four-tier resolver. This module remains metadata-only (the explicit
//! `engine:`/`engines:` list); the language axis lives in `resolution.rs`.

use quarto_pandoc_types::ConfigValue;

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

    if let ConfigValueKind::PandocInlines(inlines) = &value.value
        && inlines.len() == 1
        && let Inline::Str(str_node) = &inlines[0]
    {
        return Some(&str_node.text);
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
    // Back-compat shim: the first engine of the detected sequence (or the
    // default markdown engine). Single-engine call sites that predate
    // multi-engine support (bd-5yff4) keep their exact behavior because
    // the string/map/top-level forms each yield a one-element sequence.
    detect_engines(metadata)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// Interpret a single engine specifier — either the whole `engine:`
/// value or one element of an `engine:` array — as a [`DetectedEngine`].
///
/// - A bare string is the engine name (`engine: knitr`, or `- knitr`).
/// - A single-key map is `name: config`
///   (`engine: { jupyter: { kernel: python3 } }`, or
///   `- mermaidjs: { theme: dark }`). The first key wins, matching the
///   long-standing single-engine behavior.
///
/// Returns `None` for shapes that name no engine (e.g. an empty map),
/// so callers can skip them.
fn detected_from_value(value: &ConfigValue) -> Option<DetectedEngine> {
    // String form first (also covers Path/Glob/Expr and single-Str
    // PandocInlines via `extract_string_value`).
    if let Some(name) = extract_string_value(value) {
        return Some(DetectedEngine::new(name));
    }
    // Single-key map form: `name: config`.
    if let Some(entries) = value.as_map_entries()
        && let Some(first_entry) = entries.first()
    {
        return Some(DetectedEngine::with_config(
            first_entry.key.clone(),
            first_entry.value.clone(),
        ));
    }
    None
}

/// Detect the **ordered** execution-engine sequence from document
/// metadata (bd-5yff4).
///
/// Accepted `engine:` shapes, in addition to the single-engine forms
/// [`detect_engine`] documents:
///
/// ```yaml
/// engine: [knitr, mermaidjs]          # ordered array of names
/// engine:
///   - knitr
///   - mermaidjs: { theme: dark }       # array elements may carry config
/// ```
///
/// Order is significant — engine N may emit code cells that engine N+1
/// consumes. The returned sequence may contain **duplicates**; callers
/// that require distinct engines should go through
/// [`detect_engine_sequence`], which de-duplicates and reports the drops.
///
/// Unknown engine names are returned as-is; the pipeline stage handles
/// fallback and warnings. An `engine:` array that names no engines (empty
/// or all-unparseable) falls through to the same default as no `engine:`
/// key at all.
pub fn detect_engines(metadata: &ConfigValue) -> Vec<DetectedEngine> {
    // Explicit `engine:` key.
    if let Some(engine_value) = metadata.get("engine") {
        // Array form: ordered, may mix bare names and single-key maps.
        if let Some(items) = engine_value.as_array() {
            let engines: Vec<DetectedEngine> =
                items.iter().filter_map(detected_from_value).collect();
            if !engines.is_empty() {
                return engines;
            }
            // Array named no engines: fall through to the default below
            // (mirrors the pre-array behavior for a non-string,
            // non-map `engine:` value).
        } else if let Some(detected) = detected_from_value(engine_value) {
            // Scalar string or single-key map: a one-element sequence.
            return vec![detected];
        }
    }

    // Default: markdown engine (no execution)
    vec![DetectedEngine::default()]
}

/// An ordered, de-duplicated engine sequence plus the names of any
/// duplicate engines that were dropped.
///
/// Engines in a sequence must be **distinct** (decision in the
/// multi-engine plan): order is significant, so a repeated engine is
/// ambiguous. Duplicates arise most often from the `!concat` array-merge
/// default when two config layers each name the same engine (e.g. a
/// project `engine: [jupyter]` and a document that restates
/// `engine: [jupyter]`). We keep the **first** occurrence and record the
/// rest in [`dropped_duplicates`](EngineSequence::dropped_duplicates) so
/// the stage can emit a diagnostic. To intentionally replace a sequence
/// (including reordering) across layers, use the `!prefer` merge tag.
#[derive(Debug, Clone)]
pub struct EngineSequence {
    /// The engines to run, in order, de-duplicated (first occurrence
    /// kept).
    pub engines: Vec<DetectedEngine>,
    /// Names of duplicate engines that were dropped, in the order they
    /// were encountered. Empty in the common case.
    pub dropped_duplicates: Vec<String>,
}

/// Detect the engine sequence and de-duplicate it (keeping the first
/// occurrence of each engine name). See [`EngineSequence`].
pub fn detect_engine_sequence(metadata: &ConfigValue) -> EngineSequence {
    let mut seen = std::collections::HashSet::new();
    let mut engines = Vec::new();
    let mut dropped_duplicates = Vec::new();
    for detected in detect_engines(metadata) {
        if seen.insert(detected.name.clone()) {
            engines.push(detected);
        } else {
            dropped_duplicates.push(detected.name);
        }
    }
    EngineSequence {
        engines,
        dropped_duplicates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    /// Helper to create a simple string ConfigValue
    fn string_config(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::for_test())
    }

    /// Helper to create a map ConfigValue
    fn map_config(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(key, value)| ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::for_test(),
                value,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
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
        let meta = ConfigValue::new_map(vec![], SourceInfo::for_test());

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
        let meta = ConfigValue::null(SourceInfo::for_test());

        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "markdown");
    }

    // === bd-5yff4: multi-engine detection (detect_engines) ===

    fn array_config(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    #[test]
    fn test_detect_engines_single_string_backcompat() {
        // engine: knitr → one-element sequence
        let meta = map_config(vec![("engine", string_config("knitr"))]);
        let engines = detect_engines(&meta);
        assert_eq!(engines.len(), 1);
        assert_eq!(engines[0].name, "knitr");
    }

    #[test]
    fn test_detect_engines_array_of_strings_preserves_order() {
        // engine: [knitr, mermaidjs]
        let meta = map_config(vec![(
            "engine",
            array_config(vec![string_config("knitr"), string_config("mermaidjs")]),
        )]);
        let engines = detect_engines(&meta);
        let names: Vec<_> = engines.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["knitr", "mermaidjs"]);
        assert!(engines.iter().all(|e| e.config.is_none()));
    }

    #[test]
    fn test_detect_engines_array_element_with_config() {
        // engine:
        //   - knitr
        //   - mermaidjs: { theme: dark }
        let mermaid_cfg = map_config(vec![("theme", string_config("dark"))]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![
                string_config("knitr"),
                map_config(vec![("mermaidjs", mermaid_cfg)]),
            ]),
        )]);
        let engines = detect_engines(&meta);
        let names: Vec<_> = engines.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["knitr", "mermaidjs"]);
        assert!(engines[0].config.is_none());
        assert!(engines[1].config.is_some());
        assert!(engines[1].config.as_ref().unwrap().get("theme").is_some());
    }

    #[test]
    fn test_detect_engines_empty_array_defaults_to_markdown() {
        let meta = map_config(vec![("engine", array_config(vec![]))]);
        let engines = detect_engines(&meta);
        assert_eq!(engines.len(), 1);
        assert_eq!(engines[0].name, "markdown");
    }

    #[test]
    fn test_detect_engines_map_form_still_single() {
        // engine: { jupyter: { kernel: python3 } } stays one engine
        let jupyter_cfg = map_config(vec![("kernel", string_config("python3"))]);
        let meta = map_config(vec![("engine", map_config(vec![("jupyter", jupyter_cfg)]))]);
        let engines = detect_engines(&meta);
        assert_eq!(engines.len(), 1);
        assert_eq!(engines[0].name, "jupyter");
        assert!(engines[0].config.is_some());
    }

    #[test]
    fn test_detect_engine_backcompat_returns_first_of_array() {
        // The singular shim returns the first engine of the sequence.
        let meta = map_config(vec![(
            "engine",
            array_config(vec![string_config("knitr"), string_config("mermaidjs")]),
        )]);
        assert_eq!(detect_engine(&meta).name, "knitr");
    }

    // === bd-5yff4: de-duplication (detect_engine_sequence) ===

    #[test]
    fn test_detect_engine_sequence_distinct_passes_through() {
        let meta = map_config(vec![(
            "engine",
            array_config(vec![string_config("knitr"), string_config("mermaidjs")]),
        )]);
        let seq = detect_engine_sequence(&meta);
        let names: Vec<_> = seq.engines.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["knitr", "mermaidjs"]);
        assert!(seq.dropped_duplicates.is_empty());
    }

    #[test]
    fn test_detect_engine_sequence_dedups_keeping_first() {
        // [jupyter, knitr, jupyter] → [jupyter, knitr], dropped [jupyter]
        let meta = map_config(vec![(
            "engine",
            array_config(vec![
                string_config("jupyter"),
                string_config("knitr"),
                string_config("jupyter"),
            ]),
        )]);
        let seq = detect_engine_sequence(&meta);
        let names: Vec<_> = seq.engines.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["jupyter", "knitr"]);
        assert_eq!(seq.dropped_duplicates, vec!["jupyter".to_string()]);
    }

    #[test]
    fn test_detect_engine_sequence_single_engine_no_drops() {
        let meta = map_config(vec![("engine", string_config("knitr"))]);
        let seq = detect_engine_sequence(&meta);
        assert_eq!(seq.engines.len(), 1);
        assert_eq!(seq.engines[0].name, "knitr");
        assert!(seq.dropped_duplicates.is_empty());
    }
}

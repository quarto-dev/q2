/*
 * engine/knitr/format.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Format configuration types for knitr engine.
 */

//! Format configuration types for knitr execution.
//!
//! These types define the format configuration sent to the R scripts during
//! knitr execution. They mirror the TypeScript `Format` interface and are
//! serialized to JSON for communication with R.
//!
//! # JSON Structure
//!
//! The format configuration follows this structure:
//!
//! ```json
//! {
//!   "pandoc": { "to": "html", "from": "markdown" },
//!   "execute": { "fig-width": 7, "fig-height": 5, "echo": true },
//!   "render": { "keep-hidden": false },
//!   "identifier": { "base-format": "html" },
//!   "metadata": { "title": "My Document" }
//! }
//! ```
//!
//! # Field Naming
//!
//! All fields use kebab-case in the JSON output to match the R scripts'
//! expectations (e.g., `fig-width`, `base-format`).

use std::collections::HashMap;

use quarto_pandoc_types::ConfigValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use yaml_rust2::Yaml;

/// Complete format configuration for knitr execution.
///
/// This structure contains all the configuration needed by the R scripts
/// to execute code and format output correctly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnitrFormatConfig {
    /// Pandoc options (output format, input format, etc.)
    pub pandoc: PandocConfig,

    /// Code execution options (figure sizes, evaluation settings, etc.)
    pub execute: ExecuteConfig,

    /// Render options (output preferences, post-processing, etc.)
    pub render: RenderConfig,

    /// Format identifier info (base format, target format)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<IdentifierConfig>,

    /// Additional metadata passed through to R
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,

    /// Language/localization strings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<HashMap<String, String>>,
}

/// Pandoc-specific options.
///
/// Controls how Pandoc processes the document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PandocConfig {
    /// Output format (e.g., "html", "pdf", "latex", "docx")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,

    /// Input format (usually "markdown")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,

    /// Additional pandoc arguments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,

    /// Additional options not explicitly defined
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Code execution options.
///
/// These control knitr's behavior for code chunks. Field names use kebab-case
/// in JSON to match R's expectations (e.g., `fig-width`, `fig-height`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ExecuteConfig {
    /// Figure width in inches (default: 7)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fig_width: Option<f64>,

    /// Figure height in inches (default: 5)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fig_height: Option<f64>,

    /// Figure aspect ratio (overrides fig_height if set)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fig_asp: Option<f64>,

    /// Figure DPI (default: 96)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fig_dpi: Option<u32>,

    /// Figure format (e.g., "png", "svg", "pdf")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fig_format: Option<String>,

    /// Whether to evaluate code chunks (default: true)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval: Option<bool>,

    /// Whether to display code.
    /// Can be `true`, `false`, or `"fenced"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub echo: Option<Value>,

    /// Whether to display warnings (default: true)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<bool>,

    /// Whether to halt on errors (default: false)
    /// If true, errors in code chunks stop execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<bool>,

    /// Whether to include chunk output (default: true)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,

    /// How to output results.
    /// Can be `true`, `false`, or `"asis"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,

    /// Cache behavior.
    /// Can be `true`, `false`, or `"refresh"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<Value>,

    /// How to print data frames (e.g., "default", "kable", "tibble", "paged")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub df_print: Option<String>,

    /// Whether execution is enabled at all
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Enable debug mode
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<bool>,

    /// Freeze execution (use cached results)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze: Option<Value>,

    /// Additional options not explicitly defined
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Render options.
///
/// Controls post-processing and output generation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RenderConfig {
    /// Keep hidden chunks in output
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_hidden: Option<bool>,

    /// Enable code linking via downlit (R package linking)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_link: Option<bool>,

    /// Keep intermediate TeX source (for PDF output)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_tex: Option<bool>,

    /// Default figure position for LaTeX (e.g., "H", "htbp")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fig_pos: Option<String>,

    /// Prefer HTML output for widgets in markdown formats
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer_html: Option<bool>,

    /// Preserve notebook cells during conversion
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notebook_preserve_cells: Option<bool>,

    /// Produce source notebook alongside output
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub produce_source_notebook: Option<bool>,

    /// Additional options not explicitly defined
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Format identifier info.
///
/// Identifies the format type and inheritance.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IdentifierConfig {
    /// Base format name (e.g., "html", "pdf", "dashboard")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_format: Option<String>,

    /// Target format name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_format: Option<String>,

    /// Display name for the format
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Extension name (for custom formats)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_name: Option<String>,
}

impl KnitrFormatConfig {
    /// Create a new format config with the given output format.
    ///
    /// Creates a minimal config without execution defaults. Use [`with_defaults`]
    /// for a config with sensible execution options. This constructor is useful
    /// when you want to customize all settings yourself.
    ///
    /// [`with_defaults`]: Self::with_defaults
    #[allow(dead_code)]
    pub fn new(output_format: &str) -> Self {
        Self {
            pandoc: PandocConfig {
                to: Some(output_format.to_string()),
                from: Some("markdown".to_string()),
                ..Default::default()
            },
            execute: ExecuteConfig::default(),
            render: RenderConfig::default(),
            identifier: None,
            metadata: None,
            language: None,
        }
    }

    /// Create a format config with default execution options.
    pub fn with_defaults(output_format: &str) -> Self {
        Self {
            pandoc: PandocConfig {
                to: Some(output_format.to_string()),
                from: Some("markdown".to_string()),
                ..Default::default()
            },
            execute: ExecuteConfig::with_defaults(),
            render: RenderConfig::default(),
            identifier: None,
            metadata: None,
            language: None,
        }
    }
}

impl ExecuteConfig {
    /// Create execution config with sensible defaults.
    ///
    /// These defaults match knitr's defaults and Quarto's conventions.
    pub fn with_defaults() -> Self {
        Self {
            fig_width: Some(7.0),
            fig_height: Some(5.0),
            fig_asp: None,
            fig_dpi: Some(96),
            fig_format: Some("png".to_string()),
            eval: Some(true),
            echo: Some(Value::Bool(true)),
            warning: Some(true),
            error: Some(false),
            include: Some(true),
            output: None,
            cache: None,
            df_print: Some("default".to_string()),
            enabled: None,
            debug: None,
            freeze: None,
            extra: HashMap::new(),
        }
    }

    /// Overlay the document's merged `execute:` scope onto these
    /// defaults (bd-nn2fou8h).
    ///
    /// **Overlay, never replace.** `resources/rmd/execute.R` reads
    /// several keys through `isTRUE(...)`, so an *absent* key there
    /// means **false**, not "unset":
    ///
    /// ```r
    /// warning = isTRUE(format$execute[["warning"]]),
    /// include = isTRUE(format$execute[["include"]]),
    /// ```
    ///
    /// [`Self::with_defaults`] exists precisely to supply those
    /// `true`s. Handing R the document's `execute:` map verbatim would
    /// therefore set `include` to false for every chunk and render a
    /// blank document — which is why this method starts from the
    /// defaults and only overwrites keys the document actually set.
    ///
    /// Keys are exactly the ones the R scripts read (`execute.R`
    /// `opts_chunk`, `hooks.R`). `freeze` is deliberately not
    /// forwarded: nothing on the R side consumes it — it is resolved
    /// higher up, before an engine ever runs.
    pub fn overlay_document_scope(&mut self, scope: &ConfigValue) {
        if let Some(v) = number(scope, "fig-width") {
            self.fig_width = Some(v);
        }
        if let Some(v) = number(scope, "fig-height") {
            self.fig_height = Some(v);
        }
        if let Some(v) = number(scope, "fig-asp") {
            self.fig_asp = Some(v);
        }
        if let Some(v) = number(scope, "fig-dpi") {
            self.fig_dpi = Some(v as u32);
        }
        if let Some(v) = text(scope, "fig-format") {
            self.fig_format = Some(v);
        }
        if let Some(v) = text(scope, "df-print") {
            self.df_print = Some(v);
        }
        if let Some(v) = flag(scope, "eval") {
            self.eval = Some(v);
        }
        if let Some(v) = flag(scope, "warning") {
            self.warning = Some(v);
        }
        if let Some(v) = flag(scope, "error") {
            self.error = Some(v);
        }
        if let Some(v) = flag(scope, "include") {
            self.include = Some(v);
        }
        if let Some(v) = flag(scope, "enabled") {
            self.enabled = Some(v);
        }
        if let Some(v) = flag(scope, "debug") {
            self.debug = Some(v);
        }
        // Tri-state keys: a boolean or a keyword (`echo: fenced`,
        // `output: asis`). `execute.R` compares both forms.
        if let Some(v) = flag_or_keyword(scope, "echo") {
            self.echo = Some(v);
        }
        if let Some(v) = flag_or_keyword(scope, "output") {
            self.output = Some(v);
        }
        if let Some(v) = flag_or_keyword(scope, "cache") {
            self.cache = Some(v);
        }
    }
}

fn flag(scope: &ConfigValue, key: &str) -> Option<bool> {
    scope.get(key)?.as_bool()
}

/// A string-valued option. Uses `as_plain_text`, not `as_str`: a bare
/// YAML string in document-metadata context is stored as
/// `ConfigValueKind::PandocInlines`, for which `as_str` returns `None`
/// and the option would be silently dropped (the `metadata-as-str`
/// lint exists for exactly this trap).
fn text(scope: &ConfigValue, key: &str) -> Option<String> {
    scope.get(key)?.as_plain_text()
}

/// A numeric option, accepting both YAML integer and real spellings
/// (`fig-width: 7` and `fig-width: 7.5`).
fn number(scope: &ConfigValue, key: &str) -> Option<f64> {
    let value = scope.get(key)?;
    if let Some(i) = value.as_int() {
        return Some(i as f64);
    }
    match value.as_yaml()? {
        Yaml::Real(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// An option that is either a boolean or a keyword string, preserved
/// as-is for the R side to compare (`echo: fenced`, `output: asis`).
fn flag_or_keyword(scope: &ConfigValue, key: &str) -> Option<Value> {
    let value = scope.get(key)?;
    if let Some(b) = value.as_bool() {
        return Some(Value::Bool(b));
    }
    value.as_plain_text().map(Value::String)
}

#[cfg(test)]
mod tests {
    use super::*;

    // === bd-nn2fou8h: document `execute:` scope overlay ===

    /// A document `execute:` map, as the pipeline hands it to the
    /// engine.
    fn scope(yaml: &str) -> ConfigValue {
        let parsed = quarto_yaml::parse(yaml).expect("test YAML parses");
        let (config, _) = crate::cell_options::options_to_config(parsed);
        config
    }

    fn overlaid(yaml: &str) -> ExecuteConfig {
        let mut execute = ExecuteConfig::with_defaults();
        execute.overlay_document_scope(&scope(yaml));
        execute
    }

    /// The trap this method exists to avoid: `execute.R` reads
    /// `include`/`warning` through `isTRUE()`, so dropping the
    /// defaults for keys the document didn't mention would render a
    /// blank document.
    #[test]
    fn test_overlay_preserves_defaults_for_untouched_keys() {
        let execute = overlaid("echo: false");
        assert_eq!(execute.echo, Some(Value::Bool(false)), "echo overridden");
        assert_eq!(execute.include, Some(true), "include default preserved");
        assert_eq!(execute.warning, Some(true), "warning default preserved");
        assert_eq!(execute.eval, Some(true), "eval default preserved");
        assert_eq!(execute.error, Some(false), "error default preserved");
        assert_eq!(execute.fig_width, Some(7.0), "fig-width default preserved");
    }

    #[test]
    fn test_overlay_reads_booleans() {
        let execute = overlaid("warning: false\ninclude: false\nerror: true\neval: false");
        assert_eq!(execute.warning, Some(false));
        assert_eq!(execute.include, Some(false));
        assert_eq!(execute.error, Some(true));
        assert_eq!(execute.eval, Some(false));
    }

    /// `echo` and `output` are tri-state: the keyword forms must reach
    /// R intact, because `execute.R` compares them by string
    /// (`identical(format$execute[["echo"]], "fenced")`).
    #[test]
    fn test_overlay_preserves_keyword_forms() {
        assert_eq!(
            overlaid("echo: fenced").echo,
            Some(Value::String("fenced".to_string()))
        );
        assert_eq!(
            overlaid("output: asis").output,
            Some(Value::String("asis".to_string()))
        );
        assert_eq!(overlaid("output: false").output, Some(Value::Bool(false)));
    }

    /// Numbers arrive as YAML integers or reals; both must land as
    /// `f64` rather than being silently dropped.
    #[test]
    fn test_overlay_reads_numbers_in_both_spellings() {
        let execute = overlaid("fig-width: 9.5\nfig-height: 4\nfig-dpi: 300");
        assert_eq!(execute.fig_width, Some(9.5), "real");
        assert_eq!(execute.fig_height, Some(4.0), "integer");
        assert_eq!(execute.fig_dpi, Some(300));
    }

    #[test]
    fn test_overlay_reads_strings() {
        assert_eq!(
            overlaid("df-print: kable").df_print,
            Some("kable".to_string())
        );
        assert_eq!(
            overlaid("fig-format: svg").fig_format,
            Some("svg".to_string())
        );
    }

    /// `freeze` is resolved before an engine runs and no R code reads
    /// it, so it is deliberately not forwarded.
    #[test]
    fn test_overlay_does_not_forward_freeze() {
        assert_eq!(overlaid("freeze: true").freeze, None);
    }

    /// The overlaid config still serializes under the kebab-case names
    /// the R scripts index by.
    #[test]
    fn test_overlaid_config_serializes_with_kebab_case_keys() {
        let json = serde_json::to_value(overlaid("echo: false\nfig-width: 9.5")).unwrap();
        assert_eq!(json["echo"], Value::Bool(false));
        assert_eq!(json["fig-width"], serde_json::json!(9.5));
        assert_eq!(json["include"], Value::Bool(true));
    }

    #[test]
    fn test_format_config_serialization() {
        let config = KnitrFormatConfig::new("html");
        let json = serde_json::to_string(&config).unwrap();

        assert!(json.contains("\"pandoc\""));
        assert!(json.contains("\"to\":\"html\""));
        assert!(json.contains("\"from\":\"markdown\""));
    }

    #[test]
    fn test_format_config_with_defaults() {
        let config = KnitrFormatConfig::with_defaults("pdf");
        let json = serde_json::to_string_pretty(&config).unwrap();

        // Check pandoc settings
        assert!(json.contains("\"to\": \"pdf\""));

        // Check execute settings use kebab-case
        assert!(json.contains("\"fig-width\": 7.0"));
        assert!(json.contains("\"fig-height\": 5.0"));
        assert!(json.contains("\"fig-dpi\": 96"));
    }

    #[test]
    fn test_execute_config_kebab_case() {
        let config = ExecuteConfig::with_defaults();
        let json = serde_json::to_string(&config).unwrap();

        // Field names should be kebab-case
        assert!(json.contains("\"fig-width\""));
        assert!(json.contains("\"fig-height\""));
        assert!(json.contains("\"fig-dpi\""));
        assert!(json.contains("\"fig-format\""));
        assert!(json.contains("\"df-print\""));

        // Should NOT contain snake_case
        assert!(!json.contains("fig_width"));
        assert!(!json.contains("fig_height"));
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)] // config is reused across echo true/false cases
    fn test_execute_config_echo_values() {
        // echo: true
        let mut config = ExecuteConfig::default();
        config.echo = Some(Value::Bool(true));
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"echo\":true"));

        // echo: false
        config.echo = Some(Value::Bool(false));
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"echo\":false"));

        // echo: "fenced"
        config.echo = Some(Value::String("fenced".to_string()));
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"echo\":\"fenced\""));
    }

    #[test]
    fn test_identifier_config_kebab_case() {
        let config = IdentifierConfig {
            base_format: Some("html".to_string()),
            target_format: Some("html5".to_string()),
            display_name: None,
            extension_name: None,
        };
        let json = serde_json::to_string(&config).unwrap();

        assert!(json.contains("\"base-format\""));
        assert!(json.contains("\"target-format\""));
        assert!(!json.contains("base_format"));
    }

    #[test]
    fn test_render_config_kebab_case() {
        let config = RenderConfig {
            keep_hidden: Some(false),
            code_link: Some(true),
            keep_tex: Some(false),
            prefer_html: Some(true),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();

        assert!(json.contains("\"keep-hidden\""));
        assert!(json.contains("\"code-link\""));
        assert!(json.contains("\"keep-tex\""));
        assert!(json.contains("\"prefer-html\""));
    }

    #[test]
    fn test_format_config_roundtrip() {
        let config = KnitrFormatConfig::with_defaults("html");
        let json = serde_json::to_string(&config).unwrap();
        let parsed: KnitrFormatConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.pandoc.to, Some("html".to_string()));
        assert_eq!(parsed.execute.fig_width, Some(7.0));
    }

    #[test]
    fn test_format_config_extra_fields() {
        // Test that extra fields are preserved through flatten
        let json = r#"{
            "pandoc": {
                "to": "html",
                "custom-option": "value"
            },
            "execute": {
                "fig-width": 8,
                "my-custom-setting": 42
            },
            "render": {}
        }"#;

        let config: KnitrFormatConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.pandoc.to, Some("html".to_string()));
        assert!(config.pandoc.extra.contains_key("custom-option"));
        assert_eq!(config.execute.fig_width, Some(8.0));
        assert!(config.execute.extra.contains_key("my-custom-setting"));
    }

    #[test]
    fn test_format_config_null_fields_omitted() {
        let config = KnitrFormatConfig::new("html");
        let json = serde_json::to_string(&config).unwrap();

        // Optional None fields should not appear in JSON
        assert!(!json.contains("\"identifier\""));
        assert!(!json.contains("\"metadata\""));
        assert!(!json.contains("\"language\""));
    }
}

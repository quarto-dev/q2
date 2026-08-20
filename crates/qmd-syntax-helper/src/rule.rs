use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Location information for a violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub row: usize,
    pub column: usize,
}

/// Result of checking a file for a specific rule
/// Each CheckResult represents a single violation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckResult {
    pub rule_name: String,
    pub file_path: String,
    pub has_issue: bool,
    pub issue_count: usize, // Kept for backwards compatibility, always 1 when has_issue=true
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
    /// Error code (e.g., "Q-2-5") for parse errors
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// All error codes found (for parse rule with multiple errors)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_codes: Option<Vec<String>>,
    /// True on the synthesized per-file record (`rule_name: "unanalyzable"`)
    /// emitted when requires-parse rules were skipped because the file does
    /// not parse. Not an issue — the file was *not checked* by those rules,
    /// and the summary counts it separately from both clean and has-issue
    /// (bd-syntax-helper-parse-masking-w88mhedp).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unanalyzable: bool,
    /// The requires-parse rules that were skipped, sorted by name. Only set
    /// on the synthesized `unanalyzable` record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_rules: Option<Vec<String>>,
}

/// Result of converting/fixing a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertResult {
    pub rule_name: String,
    pub file_path: String,
    pub fixes_applied: usize,
    pub message: Option<String>,
}

/// A rule that can check for and fix issues in Quarto Markdown files
pub trait Rule {
    /// The name of this rule (e.g., "grid-tables", "div-whitespace")
    fn name(&self) -> &str;

    /// A short description of what this rule checks/fixes
    fn description(&self) -> &str;

    /// Check if a file violates this rule
    /// Returns a vector of CheckResults, one per violation found
    fn check(&self, file_path: &Path, verbose: bool) -> Result<Vec<CheckResult>>;

    /// Convert/fix rule violations in a file
    /// If in_place is false, returns the converted content as a string in the message field
    fn convert(
        &self,
        file_path: &Path,
        in_place: bool,
        check_mode: bool,
        verbose: bool,
    ) -> Result<ConvertResult>;

    /// Whether this rule needs the file to parse into an AST before it can
    /// say anything about it.
    ///
    /// Most rules read parse *failures* as their input (the diagnostic-driven
    /// `q-2-*` rules) or work on raw text (`grid-tables`), so they run on any
    /// file. A rule that walks the parsed AST instead — `reference-links`,
    /// `literal-brackets`, `q-2-30` — can only report "no findings" when it
    /// actually saw an AST; on an unparseable file the check/convert drivers
    /// skip it and account for the file as *unanalyzable* rather than clean
    /// (bd-syntax-helper-parse-masking-w88mhedp).
    fn requires_parse(&self) -> bool {
        false
    }

    /// Whether `convert --rule all` applies this rule.
    ///
    /// A rule opts out when its edits cannot afterwards be distinguished
    /// from the author's own intent, so that a bulk conversion never makes
    /// such an edit unasked. Opting out affects `convert` only: `check
    /// --rule all` still reports the rule's findings, and the rule can
    /// always be applied deliberately with `-r <name>`.
    ///
    /// See `literal_brackets.rs`, whose escaping pass is the reason this
    /// exists (bd-reference-links-unsupported-ddc4skac).
    fn opt_in_only(&self) -> bool {
        false
    }
}

/// Registry of all available rules
pub struct RuleRegistry {
    rules: HashMap<String, Arc<dyn Rule + Send + Sync>>,
}

impl RuleRegistry {
    /// Create a new registry and register all known rules
    pub fn new() -> Result<Self> {
        let mut registry = Self {
            rules: HashMap::new(),
        };

        // Register diagnostic rules first (parse check should run before conversion rules)
        registry.register(Arc::new(
            crate::diagnostics::parse_check::ParseChecker::new()?,
        ));
        registry.register(Arc::new(crate::diagnostics::q_2_30::Q230Checker::new()?));

        // Register conversion rules
        registry.register(Arc::new(
            crate::conversions::apostrophe_quotes::ApostropheQuotesConverter::new()?,
        ));
        registry.register(Arc::new(
            crate::conversions::attribute_ordering::AttributeOrderingConverter::new()?,
        ));
        registry.register(Arc::new(
            crate::conversions::grid_tables::GridTableConverter::new()?,
        ));
        registry.register(Arc::new(
            crate::conversions::reference_links::ReferenceLinksConverter::new()?,
        ));
        registry.register(Arc::new(
            crate::conversions::literal_brackets::LiteralBracketsConverter::new()?,
        ));
        registry.register(Arc::new(
            crate::conversions::definition_lists::DefinitionListConverter::new()?,
        ));
        registry.register(Arc::new(crate::conversions::q_2_5::Q25Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_7::Q27Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_11::Q211Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_12::Q212Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_13::Q213Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_15::Q215Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_16::Q216Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_17::Q217Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_18::Q218Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_19::Q219Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_20::Q220Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_21::Q221Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_22::Q222Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_23::Q223Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_24::Q224Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_25::Q225Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_26::Q226Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_28::Q228Converter::new()?));
        registry.register(Arc::new(crate::conversions::q_2_33::Q233Converter::new()?));

        Ok(registry)
    }

    /// Register a rule
    fn register(&mut self, rule: Arc<dyn Rule + Send + Sync>) {
        self.rules.insert(rule.name().to_string(), rule);
    }

    /// Get a rule by name, or return an error if not found
    pub fn get(&self, name: &str) -> Result<Arc<dyn Rule + Send + Sync>> {
        self.rules
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown rule: {}", name))
    }

    /// Get all registered rules
    pub fn all(&self) -> Vec<Arc<dyn Rule + Send + Sync>> {
        self.rules.values().cloned().collect()
    }

    /// Get the rules a bulk `convert --rule all` should apply — every rule
    /// except those that are [`Rule::opt_in_only`].
    pub fn all_auto_convertible(&self) -> Vec<Arc<dyn Rule + Send + Sync>> {
        self.rules
            .values()
            .filter(|rule| !rule.opt_in_only())
            .cloned()
            .collect()
    }

    /// List all rule names
    pub fn list_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.rules.keys().cloned().collect();
        names.sort();
        names
    }
}

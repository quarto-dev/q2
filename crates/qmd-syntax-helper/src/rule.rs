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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// List all rule names
    pub fn list_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.rules.keys().cloned().collect();
        names.sort();
        names
    }
}

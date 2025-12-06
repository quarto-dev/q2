/*
 * template/bundle.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Template bundle format for self-contained template distribution.
//!
//! A template bundle is a JSON file containing a main template and all its
//! partials, making it fully self-contained and suitable for use in
//! environments without filesystem access (e.g., WASM).

use quarto_doctemplate::{MemoryResolver, Template, TemplateError};
use quarto_source_map::SourceContext;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A self-contained template bundle.
///
/// Bundle format (JSON):
/// ```json
/// {
///   "version": "1.0.0",
///   "main": "<!DOCTYPE html><html>$body$</html>",
///   "partials": {
///     "header": "<header>$title$</header>",
///     "footer": "<footer>$date$</footer>"
///   }
/// }
/// ```
///
/// Version semantics:
/// - Missing `version`: Best-effort parsing, no schema guarantees
/// - `version: "1.0.0"`: Conforms to quarto-doctemplate 1.0.0 schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateBundle {
    /// Schema version (semver string). Optional for backwards compatibility.
    #[serde(default)]
    pub version: Option<String>,

    /// The main template source.
    pub main: String,

    /// Partial templates, keyed by name.
    #[serde(default)]
    pub partials: HashMap<String, String>,
}

/// Error type for bundle operations.
#[derive(Debug)]
pub enum BundleError {
    /// JSON parsing failed.
    JsonParse(serde_json::Error),
    /// Template compilation failed.
    TemplateCompile(TemplateError),
    /// Unsupported bundle version.
    UnsupportedVersion(String),
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleError::JsonParse(e) => write!(f, "failed to parse bundle JSON: {}", e),
            BundleError::TemplateCompile(e) => write!(f, "failed to compile template: {}", e),
            BundleError::UnsupportedVersion(v) => write!(f, "unsupported bundle version: {}", v),
        }
    }
}

impl std::error::Error for BundleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BundleError::JsonParse(e) => Some(e),
            BundleError::TemplateCompile(e) => Some(e),
            BundleError::UnsupportedVersion(_) => None,
        }
    }
}

impl From<serde_json::Error> for BundleError {
    fn from(e: serde_json::Error) -> Self {
        BundleError::JsonParse(e)
    }
}

impl From<TemplateError> for BundleError {
    fn from(e: TemplateError) -> Self {
        BundleError::TemplateCompile(e)
    }
}

/// Currently supported bundle versions.
const SUPPORTED_VERSIONS: &[&str] = &["1.0.0"];

impl TemplateBundle {
    /// Create a new template bundle.
    pub fn new(main: impl Into<String>) -> Self {
        Self {
            version: Some("1.0.0".to_string()),
            main: main.into(),
            partials: HashMap::new(),
        }
    }

    /// Add a partial to the bundle.
    pub fn with_partial(mut self, name: impl Into<String>, content: impl Into<String>) -> Self {
        self.partials.insert(name.into(), content.into());
        self
    }

    /// Parse a bundle from JSON.
    pub fn from_json(json: &str) -> Result<Self, BundleError> {
        let bundle: Self = serde_json::from_str(json)?;
        bundle.validate_version()?;
        Ok(bundle)
    }

    /// Serialize the bundle to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Validate the bundle version.
    fn validate_version(&self) -> Result<(), BundleError> {
        if let Some(version) = &self.version {
            if !SUPPORTED_VERSIONS.contains(&version.as_str()) {
                return Err(BundleError::UnsupportedVersion(version.clone()));
            }
        }
        // No version = best-effort, no error
        Ok(())
    }

    /// Create a memory resolver from this bundle's partials.
    pub fn to_resolver(&self) -> MemoryResolver {
        MemoryResolver::with_partials(self.partials.iter().map(|(k, v)| (k.as_str(), v.as_str())))
    }

    /// Compile the bundle into a ready-to-use template.
    ///
    /// This parses the main template and resolves all partials from the bundle.
    /// The resulting `Template` can be used for rendering.
    ///
    /// This creates an internal `SourceContext` for standalone use.
    /// For integrated use with a shared context, use [`compile_with_context`].
    ///
    /// # Arguments
    ///
    /// * `template_name` - A name for the template (used in error messages).
    ///   Typically "bundle" or the source filename.
    pub fn compile(&self, template_name: &str) -> Result<Template, BundleError> {
        let resolver = self.to_resolver();
        let path = Path::new(template_name);
        let template = Template::compile_with_resolver(&self.main, path, &resolver, 0)?;
        Ok(template)
    }

    /// Compile the bundle into a ready-to-use template with a shared `SourceContext`.
    ///
    /// This allows the template and its partials to share the same `SourceContext`
    /// as the main document, ensuring unique file IDs across all files. This is
    /// essential for correct diagnostic reporting.
    ///
    /// # Arguments
    ///
    /// * `template_name` - A name for the template (used in error messages).
    /// * `source_context` - The shared source context (template files will be added to this)
    pub fn compile_with_context(
        &self,
        template_name: &str,
        source_context: &mut SourceContext,
    ) -> Result<Template, BundleError> {
        let resolver = self.to_resolver();
        let path = Path::new(template_name);
        let template = Template::compile_with_resolver_and_context(
            &self.main,
            path,
            &resolver,
            0,
            source_context,
        )?;
        Ok(template)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_doctemplate::PartialResolver;

    #[test]
    fn test_bundle_new() {
        let bundle = TemplateBundle::new("Hello $name$!");
        assert_eq!(bundle.version, Some("1.0.0".to_string()));
        assert_eq!(bundle.main, "Hello $name$!");
        assert!(bundle.partials.is_empty());
    }

    #[test]
    fn test_bundle_with_partial() {
        let bundle =
            TemplateBundle::new("$header()$content").with_partial("header", "<h1>$title$</h1>");

        assert_eq!(bundle.partials.len(), 1);
        assert_eq!(
            bundle.partials.get("header"),
            Some(&"<h1>$title$</h1>".to_string())
        );
    }

    #[test]
    fn test_bundle_from_json() {
        let json = r#"{
            "version": "1.0.0",
            "main": "Hello $name$!",
            "partials": {
                "header": "<h1>$title$</h1>"
            }
        }"#;

        let bundle = TemplateBundle::from_json(json).unwrap();
        assert_eq!(bundle.version, Some("1.0.0".to_string()));
        assert_eq!(bundle.main, "Hello $name$!");
        assert_eq!(
            bundle.partials.get("header"),
            Some(&"<h1>$title$</h1>".to_string())
        );
    }

    #[test]
    fn test_bundle_from_json_no_version() {
        let json = r#"{
            "main": "Hello $name$!"
        }"#;

        let bundle = TemplateBundle::from_json(json).unwrap();
        assert_eq!(bundle.version, None);
        assert_eq!(bundle.main, "Hello $name$!");
    }

    #[test]
    fn test_bundle_from_json_unsupported_version() {
        let json = r#"{
            "version": "99.0.0",
            "main": "Hello"
        }"#;

        let result = TemplateBundle::from_json(json);
        assert!(matches!(result, Err(BundleError::UnsupportedVersion(_))));
    }

    #[test]
    fn test_bundle_to_json() {
        let bundle = TemplateBundle::new("Hello!").with_partial("footer", "Goodbye!");

        let json = bundle.to_json().unwrap();
        assert!(json.contains("\"version\": \"1.0.0\""));
        assert!(json.contains("\"main\": \"Hello!\""));
        assert!(json.contains("\"footer\": \"Goodbye!\""));
    }

    #[test]
    fn test_bundle_to_resolver() {
        let bundle = TemplateBundle::new("main")
            .with_partial("a", "content a")
            .with_partial("b", "content b");

        let resolver = bundle.to_resolver();
        assert_eq!(
            resolver.get_partial("a", Path::new("test")),
            Some("content a".to_string())
        );
        assert_eq!(
            resolver.get_partial("b", Path::new("test")),
            Some("content b".to_string())
        );
        assert_eq!(resolver.get_partial("c", Path::new("test")), None);
    }

    #[test]
    fn test_bundle_compile() {
        let bundle = TemplateBundle::new("Hello $name$!");
        let template = bundle.compile("test.html").unwrap();

        let mut ctx = quarto_doctemplate::TemplateContext::new();
        ctx.insert(
            "name",
            quarto_doctemplate::TemplateValue::String("World".to_string()),
        );

        let result = template.render(&ctx).unwrap();
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_bundle_compile_with_partials() {
        let bundle =
            TemplateBundle::new("$header()$\nContent").with_partial("header", "<h1>$title$</h1>");

        let template = bundle.compile("test.html").unwrap();

        let mut ctx = quarto_doctemplate::TemplateContext::new();
        ctx.insert(
            "title",
            quarto_doctemplate::TemplateValue::String("My Title".to_string()),
        );

        let result = template.render(&ctx).unwrap();
        assert_eq!(result, "<h1>My Title</h1>\nContent");
    }
}

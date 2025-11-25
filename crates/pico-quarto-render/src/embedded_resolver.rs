/*
 * embedded_resolver.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Embedded template resolver for pico-quarto-render.
//!
//! This module provides a `PartialResolver` implementation that loads templates
//! from resources compiled into the binary via `include_dir`.

use include_dir::{Dir, include_dir};
use quarto_doctemplate::resolver::{PartialResolver, resolve_partial_path};
use std::path::Path;

/// Embedded HTML templates directory.
static HTML_TEMPLATES: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/resources/html-template");

/// Resolver that loads templates from embedded resources.
///
/// Templates are compiled into the binary at build time using `include_dir`.
/// This resolver implements `PartialResolver` to support partial template loading.
pub struct EmbeddedResolver;

impl PartialResolver for EmbeddedResolver {
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String> {
        // Resolve the partial path following Pandoc rules
        let partial_path = resolve_partial_path(name, base_path);

        // Get the filename portion for embedded lookup
        // (templates are flat in our structure, so we just need the filename)
        let filename = partial_path.file_name()?.to_str()?;

        HTML_TEMPLATES
            .get_file(filename)
            .and_then(|f| f.contents_utf8())
            .map(|s| s.to_string())
    }
}

/// Get the main template source.
///
/// Returns the content of `template.html` from the embedded resources.
pub fn get_main_template() -> Option<&'static str> {
    HTML_TEMPLATES
        .get_file("template.html")
        .and_then(|f| f.contents_utf8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_main_template() {
        let template = get_main_template();
        assert!(template.is_some());
        let content = template.unwrap();
        assert!(content.contains("<!DOCTYPE html>"));
        assert!(content.contains("$body$"));
    }

    #[test]
    fn test_embedded_resolver_finds_partials() {
        let resolver = EmbeddedResolver;
        let base_path = Path::new("template.html");

        // Should find metadata.html partial
        let metadata = resolver.get_partial("metadata", base_path);
        assert!(metadata.is_some());

        // Should find title-block.html partial
        let title_block = resolver.get_partial("title-block", base_path);
        assert!(title_block.is_some());

        // Should find styles.html partial
        let styles = resolver.get_partial("styles", base_path);
        assert!(styles.is_some());
    }

    #[test]
    fn test_embedded_resolver_missing_partial() {
        let resolver = EmbeddedResolver;
        let base_path = Path::new("template.html");

        let missing = resolver.get_partial("nonexistent", base_path);
        assert!(missing.is_none());
    }
}

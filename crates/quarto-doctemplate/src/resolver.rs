/*
 * resolver.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Partial template resolution.
//!
//! This module provides traits and implementations for loading partial templates
//! from various sources (filesystem, memory, etc.).

use std::path::{Path, PathBuf};

/// Trait for loading partial templates.
///
/// Implementations of this trait are responsible for finding and loading
/// partial template content given a partial name and the base template path.
pub trait PartialResolver {
    /// Load a partial template by name.
    ///
    /// # Arguments
    /// * `name` - The partial name (e.g., "header", "footer.html")
    /// * `base_path` - The path of the template that references this partial
    ///
    /// # Returns
    /// The partial template source text, or `None` if not found.
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String>;
}

/// Resolver that loads partials from the filesystem.
///
/// Path resolution follows Pandoc/doctemplates rules:
/// - If partial name has no extension, use the base template's extension
/// - If partial name has an extension, use it as-is
/// - Partials are loaded from the same directory as the base template
#[derive(Debug, Clone, Default)]
pub struct FileSystemResolver;

impl PartialResolver for FileSystemResolver {
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String> {
        let partial_path = resolve_partial_path(name, base_path);
        std::fs::read_to_string(&partial_path).ok()
    }
}

/// Resolver that returns nothing (for testing without file I/O).
///
/// Use this resolver when you want to compile templates that don't use partials,
/// or in test scenarios where partials should be ignored.
#[derive(Debug, Clone, Default)]
pub struct NullResolver;

impl PartialResolver for NullResolver {
    fn get_partial(&self, _name: &str, _base_path: &Path) -> Option<String> {
        None
    }
}

/// Resolver that loads partials from an in-memory map.
///
/// Useful for testing and for scenarios where templates are bundled
/// into the application.
#[derive(Debug, Clone, Default)]
pub struct MemoryResolver {
    partials: std::collections::HashMap<String, String>,
}

impl MemoryResolver {
    /// Create a new empty memory resolver.
    pub fn new() -> Self {
        Self {
            partials: std::collections::HashMap::new(),
        }
    }

    /// Add a partial to the resolver.
    ///
    /// The name should match what will be used in the template (e.g., "header").
    pub fn add(&mut self, name: impl Into<String>, content: impl Into<String>) -> &mut Self {
        self.partials.insert(name.into(), content.into());
        self
    }

    /// Create a resolver with the given partials.
    pub fn with_partials(
        partials: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let mut resolver = Self::new();
        for (name, content) in partials {
            resolver.add(name, content);
        }
        resolver
    }
}

impl PartialResolver for MemoryResolver {
    fn get_partial(&self, name: &str, _base_path: &Path) -> Option<String> {
        self.partials.get(name).cloned()
    }
}

/// Resolve the path to a partial file.
///
/// Follows Pandoc/doctemplates path resolution rules:
/// 1. If partial name has no extension: use the base template's extension
/// 2. If partial name has an extension: use it as-is
/// 3. Directory is always the base template's directory
///
/// # Examples
///
/// ```ignore
/// // Base: /templates/doc.html, Partial: "header" → /templates/header.html
/// // Base: /templates/doc.html, Partial: "header.tex" → /templates/header.tex
/// // Base: /templates/doc.html, Partial: "inc/header" → /templates/inc/header.html
/// ```
pub fn resolve_partial_path(partial_name: &str, base_path: &Path) -> PathBuf {
    let partial_path = Path::new(partial_name);
    let base_dir = base_path.parent().unwrap_or(Path::new("."));

    if partial_path.extension().is_some() {
        // Partial has explicit extension: use it
        base_dir.join(partial_name)
    } else {
        // No extension: use base template's extension
        let ext = base_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.is_empty() {
            base_dir.join(partial_name)
        } else {
            base_dir.join(partial_name).with_extension(ext)
        }
    }
}

/// Remove the final newline from partial content.
///
/// This prevents extra blank lines when composing templates with partials.
pub fn remove_final_newline(content: &str) -> &str {
    content.strip_suffix('\n').unwrap_or(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_partial_path_no_extension() {
        let base = Path::new("/templates/doc.html");
        let result = resolve_partial_path("header", base);
        assert_eq!(result, PathBuf::from("/templates/header.html"));
    }

    #[test]
    fn test_resolve_partial_path_with_extension() {
        let base = Path::new("/templates/doc.html");
        let result = resolve_partial_path("header.tex", base);
        assert_eq!(result, PathBuf::from("/templates/header.tex"));
    }

    #[test]
    fn test_resolve_partial_path_subdirectory() {
        let base = Path::new("/templates/doc.html");
        let result = resolve_partial_path("inc/header", base);
        assert_eq!(result, PathBuf::from("/templates/inc/header.html"));
    }

    #[test]
    fn test_resolve_partial_path_no_base_extension() {
        let base = Path::new("/templates/doc");
        let result = resolve_partial_path("header", base);
        assert_eq!(result, PathBuf::from("/templates/header"));
    }

    #[test]
    fn test_remove_final_newline() {
        assert_eq!(remove_final_newline("hello\n"), "hello");
        assert_eq!(remove_final_newline("hello"), "hello");
        assert_eq!(remove_final_newline("hello\n\n"), "hello\n");
        assert_eq!(remove_final_newline(""), "");
    }

    #[test]
    fn test_null_resolver() {
        let resolver = NullResolver;
        assert!(
            resolver
                .get_partial("anything", Path::new("/foo/bar.html"))
                .is_none()
        );
    }

    #[test]
    fn test_memory_resolver() {
        let mut resolver = MemoryResolver::new();
        resolver.add("header", "<h1>Title</h1>");
        resolver.add("footer", "<footer>End</footer>");

        assert_eq!(
            resolver.get_partial("header", Path::new("/any/path.html")),
            Some("<h1>Title</h1>".to_string())
        );
        assert_eq!(
            resolver.get_partial("footer", Path::new("/any/path.html")),
            Some("<footer>End</footer>".to_string())
        );
        assert!(
            resolver
                .get_partial("missing", Path::new("/any/path.html"))
                .is_none()
        );
    }

    #[test]
    fn test_memory_resolver_with_partials() {
        let resolver =
            MemoryResolver::with_partials([("a", "content a"), ("b", "content b")]);

        assert_eq!(
            resolver.get_partial("a", Path::new("/x.html")),
            Some("content a".to_string())
        );
        assert_eq!(
            resolver.get_partial("b", Path::new("/x.html")),
            Some("content b".to_string())
        );
    }
}

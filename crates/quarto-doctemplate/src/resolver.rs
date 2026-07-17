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

/// Resolver that chains two resolvers: tries the primary first, falls back to the secondary.
///
/// Useful for combining explicit partials (e.g., from extensions) with
/// a fallback that loads from disk or runtime.
pub struct ChainedResolver<A, B> {
    primary: A,
    fallback: B,
}

impl<A, B> ChainedResolver<A, B> {
    /// Create a new chained resolver.
    pub fn new(primary: A, fallback: B) -> Self {
        Self { primary, fallback }
    }
}

impl<A: PartialResolver, B: PartialResolver> PartialResolver for ChainedResolver<A, B> {
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String> {
        self.primary
            .get_partial(name, base_path)
            .or_else(|| self.fallback.get_partial(name, base_path))
    }
}

/// References to resolvers resolve like the resolver itself, so chains
/// can borrow a caller-owned resolver instead of taking it by value.
impl<T: PartialResolver + ?Sized> PartialResolver for &T {
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String> {
        (**self).get_partial(name, base_path)
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

/// Resolver chain suitable for project-scoped template lookup.
///
/// The chain order is:
///
/// 1. [`FileSystemResolver`] — loads partials relative to the
///    template's `base_path` (typically the host page's directory).
///    Used by author-supplied custom templates referencing local
///    partial files.
/// 2. [`MemoryResolver`] carrying built-in partials (the `builtins`
///    argument). Used by the listing render transform to embed the
///    canonical `default` / `grid` / `table` templates.
///
/// The two resolvers are chained primary-first, so a custom template
/// can shadow a built-in name by placing a file with the same name
/// next to the host page. Built-ins act as the fallback when no
/// matching file exists.
///
/// For lookups that miss both layers, the result is `None`; callers
/// fall through to whatever the template engine does for unresolved
/// partials (today: a `Q-10-3` "Partial Not Found" diagnostic).
///
/// This helper exists so the listing render transform — the first
/// known consumer — has a one-call construction site instead of
/// open-coding the chain. Future consumers (e.g. L8 custom
/// templates) reuse the same shape.
pub fn project_listing_resolver(
    builtins: MemoryResolver,
) -> ChainedResolver<FileSystemResolver, MemoryResolver> {
    ChainedResolver::new(FileSystemResolver, builtins)
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
        let resolver = MemoryResolver::with_partials([("a", "content a"), ("b", "content b")]);

        assert_eq!(
            resolver.get_partial("a", Path::new("/x.html")),
            Some("content a".to_string())
        );
        assert_eq!(
            resolver.get_partial("b", Path::new("/x.html")),
            Some("content b".to_string())
        );
    }

    #[test]
    fn test_chained_resolver_primary_wins() {
        let primary = MemoryResolver::with_partials([("header", "<h1>Primary</h1>")]);
        let fallback = MemoryResolver::with_partials([("header", "<h1>Fallback</h1>")]);
        let chained = ChainedResolver::new(primary, fallback);

        assert_eq!(
            chained.get_partial("header", Path::new("/t.html")),
            Some("<h1>Primary</h1>".to_string())
        );
    }

    #[test]
    fn test_chained_resolver_fallback_when_primary_missing() {
        let primary = MemoryResolver::new();
        let fallback = MemoryResolver::with_partials([("footer", "<footer>End</footer>")]);
        let chained = ChainedResolver::new(primary, fallback);

        assert_eq!(
            chained.get_partial("footer", Path::new("/t.html")),
            Some("<footer>End</footer>".to_string())
        );
    }

    #[test]
    fn test_chained_resolver_none_when_both_missing() {
        let primary = MemoryResolver::new();
        let fallback = MemoryResolver::new();
        let chained = ChainedResolver::new(primary, fallback);

        assert!(
            chained
                .get_partial("missing", Path::new("/t.html"))
                .is_none()
        );
    }

    #[test]
    fn project_listing_resolver_serves_builtins_when_no_filesystem_match() {
        // For a synthetic base path that doesn't exist on disk, the
        // FileSystemResolver returns None and falls through to the
        // MemoryResolver carrying the listing built-ins.
        let builtins = MemoryResolver::with_partials([("listing-default", "BUILTIN")]);
        let resolver = project_listing_resolver(builtins);
        let result = resolver.get_partial(
            "listing-default",
            Path::new("/nonexistent-host-page-dir/host.qmd"),
        );
        assert_eq!(result, Some("BUILTIN".to_string()));
    }

    #[test]
    fn project_listing_resolver_filesystem_shadows_builtin() {
        use std::io::Write;
        // A custom template file next to the host page shadows a
        // built-in with the same name.
        let tmp = tempfile::tempdir().expect("tempdir");
        // The base_path passed to get_partial is the template's
        // path; FileSystemResolver looks for partials in
        // base_path.parent(). We put the partial file in the same
        // directory as the synthetic host_page path.
        let host_path = tmp.path().join("host.template");
        let partial_path = tmp.path().join("listing-default.template");
        let mut f = std::fs::File::create(&partial_path).expect("create");
        writeln!(f, "CUSTOM").expect("write");
        drop(f);

        let builtins = MemoryResolver::with_partials([("listing-default", "BUILTIN")]);
        let resolver = project_listing_resolver(builtins);
        let result = resolver.get_partial("listing-default", &host_path);
        // FileSystemResolver wins → "CUSTOM\n"
        assert_eq!(result.as_deref(), Some("CUSTOM\n"));
    }
}

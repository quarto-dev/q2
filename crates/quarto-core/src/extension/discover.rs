/*
 * extension/discover.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Extension discovery from _extensions/ directories.
 */

//! Extension discovery from `_extensions/` directories.

use std::path::Path;

use quarto_system_runtime::{PathKind, SystemRuntime};

use super::read::{read_extension, read_extension_with_org};
use super::types::Extension;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

/// Discover all extensions available for a document.
///
/// Searches `_extensions/` directories in the project hierarchy,
/// walking from the input file's directory up to the project root.
///
/// When `builtin_extensions_dir` is provided, it is scanned **first**
/// (lowest priority). User extensions discovered later appear later in
/// the vec, and `find_extension()` returns the last match — so user
/// extensions override built-ins with the same name.
pub fn discover_extensions(
    input: &Path,
    project_dir: Option<&Path>,
    builtin_extensions_dir: Option<&Path>,
    runtime: &dyn SystemRuntime,
) -> (Vec<Extension>, Vec<DiagnosticMessage>) {
    let mut extensions = Vec::new();
    let mut diagnostics = Vec::new();
    let mut dirs_to_search = Vec::new();

    // Built-in extensions first (lowest priority)
    if let Some(builtin_dir) = builtin_extensions_dir
        && runtime
            .path_exists(builtin_dir, Some(PathKind::Directory))
            .unwrap_or(false)
    {
        scan_extensions_dir(builtin_dir, runtime, &mut extensions, &mut diagnostics);
    }

    let start_dir = input.parent().unwrap_or(input);

    if let Some(proj_dir) = project_dir {
        // Walk from input directory up to project root
        let mut current = start_dir.to_path_buf();
        loop {
            dirs_to_search.push(current.join("_extensions"));
            if current == proj_dir {
                break;
            }
            match current.parent() {
                Some(parent) if parent != current => current = parent.to_path_buf(),
                _ => break,
            }
        }
        // Reverse so project-level extensions come first (lower priority)
        dirs_to_search.reverse();
    } else {
        // Single-file mode: only check input's directory
        dirs_to_search.push(start_dir.join("_extensions"));
    }

    for ext_dir in &dirs_to_search {
        if !runtime
            .path_exists(ext_dir, Some(PathKind::Directory))
            .unwrap_or(false)
        {
            continue;
        }

        scan_extensions_dir(ext_dir, runtime, &mut extensions, &mut diagnostics);
    }

    (extensions, diagnostics)
}

/// Scan all entries in an extensions directory.
fn scan_extensions_dir(
    ext_dir: &Path,
    runtime: &dyn SystemRuntime,
    extensions: &mut Vec<Extension>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    let entries = match runtime.dir_list(ext_dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries {
        scan_extension_entry(&entry, runtime, extensions, diagnostics);
    }
}

/// Scan a single entry in an `_extensions/` directory.
///
/// Could be an unorganized extension (has `_extension.yml` directly)
/// or an organization directory (contains named extension subdirs).
fn scan_extension_entry(
    entry: &Path,
    runtime: &dyn SystemRuntime,
    extensions: &mut Vec<Extension>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    let ext_file = entry.join("_extension.yml");

    // Check for direct _extension.yml (unorganized extension)
    if runtime
        .path_exists(&ext_file, Some(PathKind::File))
        .unwrap_or(false)
    {
        match read_extension(&ext_file, runtime) {
            Ok(ext) => extensions.push(ext),
            Err(e) => diagnostics.push(extension_not_loaded_diagnostic(&ext_file, &e)),
        }
        return;
    }

    // Check subdirectories (organized: org/name/)
    // The entry directory name is the organization.
    let org_name = entry.file_name().map(|n| n.to_string_lossy().to_string());
    let sub_entries = match runtime.dir_list(entry) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for sub_entry in sub_entries {
        let sub_ext_file = sub_entry.join("_extension.yml");
        if runtime
            .path_exists(&sub_ext_file, Some(PathKind::File))
            .unwrap_or(false)
        {
            match read_extension_with_org(&sub_ext_file, org_name.as_deref(), runtime) {
                Ok(ext) => extensions.push(ext),
                Err(e) => diagnostics.push(extension_not_loaded_diagnostic(&sub_ext_file, &e)),
            }
        }
    }
}

/// Build the Q-16-1 diagnostic for a manifest that could not be loaded.
///
/// Surfacing this at discovery time (instead of a bare log line) is what
/// prevents the failure from being misattributed later as an unknown
/// shortcode in the user's document (bd-nzdm1wry).
fn extension_not_loaded_diagnostic(
    ext_file: &Path,
    err: &crate::error::QuartoError,
) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Extension not loaded")
        .with_code("Q-16-1")
        .problem(format!(
            "The extension manifest `{}` could not be loaded: {}",
            ext_file.display(),
            err
        ))
        .add_hint(
            "Shortcodes, filters, and formats contributed by this extension will be unavailable.",
        )
        .build()
}

/// Find a specific extension by name among discovered extensions.
///
/// Returns the **last** match so that user extensions (appended after
/// built-ins) take priority. This matches TS Quarto's "later overwrites
/// earlier" semantics in `loadExtensions()`.
///
/// If `name` contains `/`, split into `org/name` and match both.
/// Otherwise, match by name only (any organization).
pub fn find_extension<'a>(name: &str, extensions: &'a [Extension]) -> Option<&'a Extension> {
    if let Some((org, ext_name)) = name.split_once('/') {
        extensions
            .iter()
            .rfind(|e| e.id.name == ext_name && e.id.organization.as_deref() == Some(org))
    } else {
        extensions.iter().rfind(|e| e.id.name == name)
    }
}

/// Parse a format string into extension name and base format.
///
/// Examples:
/// - "html" -> (None, "html")
/// - "acm-html" -> (Some("acm"), "html")
/// - "my-journal-pdf" -> (Some("my-journal"), "pdf")
pub struct FormatDescriptor {
    pub extension_name: Option<String>,
    pub base_format: String,
}

const KNOWN_BASE_FORMATS: &[&str] = &[
    "html",
    "pdf",
    "docx",
    "epub",
    "typst",
    "revealjs",
    "gfm",
    "commonmark",
];

pub fn parse_format_descriptor(format: &str) -> FormatDescriptor {
    // Try splitting on the last hyphen where the suffix is a known base format
    if let Some(pos) = format.rfind('-') {
        let suffix = &format[pos + 1..];
        if KNOWN_BASE_FORMATS.contains(&suffix) {
            let prefix = &format[..pos];
            if !prefix.is_empty() {
                return FormatDescriptor {
                    extension_name: Some(prefix.to_string()),
                    base_format: suffix.to_string(),
                };
            }
        }
    }

    FormatDescriptor {
        extension_name: None,
        base_format: format.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_runtime() -> quarto_system_runtime::NativeRuntime {
        quarto_system_runtime::NativeRuntime::new()
    }

    fn write_extension(dir: &Path, yaml: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("_extension.yml"), yaml).unwrap();
    }

    // === Discovery tests ===

    #[test]
    fn test_discover_simple_extension() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        write_extension(
            &ext_dir,
            r#"
title: Test
author: Author
contributes:
  formats:
    html:
      toc: true
"#,
        );

        let runtime = make_runtime();
        let input = tmp.path().join("test.qmd");
        let (extensions, _diags) = discover_extensions(&input, None, None, &runtime);

        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].id.name, "test-ext");
    }

    #[test]
    fn test_discover_organized_extension() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/org/ext");
        write_extension(
            &ext_dir,
            r#"
title: Org Extension
author: Author
contributes:
  shortcodes:
    - hello.lua
"#,
        );

        let runtime = make_runtime();
        let input = tmp.path().join("test.qmd");
        let (extensions, _diags) = discover_extensions(&input, None, None, &runtime);

        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].id.name, "ext");
        assert_eq!(extensions[0].id.organization.as_deref(), Some("org"));
    }

    #[test]
    fn test_discover_multiple_levels() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path();

        // Project-level extension
        write_extension(
            &project_dir.join("_extensions/proj-ext"),
            r#"
title: Project Extension
author: Author
contributes:
  formats:
    html:
      toc: true
"#,
        );

        // Subdirectory-level extension
        let sub_dir = project_dir.join("subdir");
        fs::create_dir_all(&sub_dir).unwrap();
        write_extension(
            &sub_dir.join("_extensions/sub-ext"),
            r#"
title: Subdir Extension
author: Author
contributes:
  formats:
    html:
      theme: cosmo
"#,
        );

        let runtime = make_runtime();
        let input = sub_dir.join("test.qmd");
        let (extensions, _diags) = discover_extensions(&input, Some(project_dir), None, &runtime);

        assert_eq!(extensions.len(), 2);
        // Project-level should come first (lower priority)
        assert_eq!(extensions[0].id.name, "proj-ext");
        assert_eq!(extensions[1].id.name, "sub-ext");
    }

    #[test]
    fn test_discover_empty_extensions_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("_extensions")).unwrap();

        let runtime = make_runtime();
        let input = tmp.path().join("test.qmd");
        let (extensions, _diags) = discover_extensions(&input, None, None, &runtime);

        assert!(extensions.is_empty());
    }

    #[test]
    fn test_discover_no_extensions_dir() {
        let tmp = TempDir::new().unwrap();

        let runtime = make_runtime();
        let input = tmp.path().join("test.qmd");
        let (extensions, _diags) = discover_extensions(&input, None, None, &runtime);

        assert!(extensions.is_empty());
    }

    #[test]
    fn test_discover_invalid_extension_skipped() {
        let tmp = TempDir::new().unwrap();

        // Valid extension
        write_extension(
            &tmp.path().join("_extensions/good-ext"),
            r#"
title: Good
author: Author
contributes:
  shortcodes:
    - hello.lua
"#,
        );

        // Invalid extension: unparseable YAML. (Missing title/author is NOT
        // invalid — Q1-compat intake, bd-8b0af414.)
        write_extension(
            &tmp.path().join("_extensions/bad-ext"),
            "contributes: [unclosed\n  nonsense: {{{{",
        );

        let runtime = make_runtime();
        let input = tmp.path().join("test.qmd");
        let (extensions, diags) = discover_extensions(&input, None, None, &runtime);

        // Only the valid extension should be discovered, and the broken one
        // must surface as a Q-16-1 diagnostic naming its manifest file
        // (bd-nzdm1wry), not vanish into a log line.
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].id.name, "good-ext");
        assert_eq!(diags.len(), 1);
        let rendered = format!("{:?}", diags[0]);
        assert!(rendered.contains("Q-16-1"), "diagnostic: {rendered}");
        assert!(rendered.contains("bad-ext"), "diagnostic: {rendered}");
    }

    // === find_extension tests ===

    #[test]
    fn test_find_extension_by_name() {
        let ext = Extension {
            id: super::super::types::ExtensionId::new("lightbox"),
            title: Some("Lightbox".to_string()),
            author: Some("Author".to_string()),
            version: None,
            quarto_required: None,
            path: PathBuf::from("/ext"),
            contributes: Default::default(),
        };
        let extensions = vec![ext];

        assert!(find_extension("lightbox", &extensions).is_some());
        assert!(find_extension("other", &extensions).is_none());
    }

    #[test]
    fn test_find_extension_by_org_name() {
        let ext = Extension {
            id: super::super::types::ExtensionId::with_organization("acm", "quarto-journals"),
            title: Some("ACM".to_string()),
            author: Some("Author".to_string()),
            version: None,
            quarto_required: None,
            path: PathBuf::from("/ext"),
            contributes: Default::default(),
        };
        let extensions = vec![ext];

        assert!(find_extension("quarto-journals/acm", &extensions).is_some());
        assert!(find_extension("acm", &extensions).is_some()); // name-only match
        assert!(find_extension("other-org/acm", &extensions).is_none());
    }

    #[test]
    fn test_find_extension_returns_last_match() {
        // Built-in (first in vec) should be overridden by user (last in vec)
        let builtin = Extension {
            id: super::super::types::ExtensionId::with_organization("lipsum", "quarto"),
            title: Some("Lipsum Built-in".to_string()),
            author: Some("Built-in Author".to_string()),
            version: None,
            quarto_required: None,
            path: PathBuf::from("/builtin/quarto/lipsum"),
            contributes: Default::default(),
        };
        let user = Extension {
            id: super::super::types::ExtensionId::new("lipsum"),
            title: Some("Lipsum User".to_string()),
            author: Some("User Author".to_string()),
            version: None,
            quarto_required: None,
            path: PathBuf::from("/user/lipsum"),
            contributes: Default::default(),
        };
        let extensions = vec![builtin, user];

        // Name-only lookup: should find user (last match)
        let found = find_extension("lipsum", &extensions).unwrap();
        assert_eq!(found.title.as_deref(), Some("Lipsum User"));
    }

    #[test]
    fn test_find_extension_org_name_returns_last_match() {
        let builtin = Extension {
            id: super::super::types::ExtensionId::with_organization("lipsum", "quarto"),
            title: Some("Lipsum Built-in".to_string()),
            author: Some("Built-in Author".to_string()),
            version: None,
            quarto_required: None,
            path: PathBuf::from("/builtin/quarto/lipsum"),
            contributes: Default::default(),
        };
        let user = Extension {
            id: super::super::types::ExtensionId::with_organization("lipsum", "quarto"),
            title: Some("Lipsum User Override".to_string()),
            author: Some("User Author".to_string()),
            version: None,
            quarto_required: None,
            path: PathBuf::from("/user/quarto/lipsum"),
            contributes: Default::default(),
        };
        let extensions = vec![builtin, user];

        // Org/name lookup: should find user (last match)
        let found = find_extension("quarto/lipsum", &extensions).unwrap();
        assert_eq!(found.title.as_deref(), Some("Lipsum User Override"));
    }

    // === Built-in extension discovery tests ===

    #[test]
    fn test_discover_builtin_extensions() {
        let tmp = TempDir::new().unwrap();
        let builtin_dir = tmp.path().join("builtin");
        // Create org/name structure matching resources/extensions/
        write_extension(
            &builtin_dir.join("quarto/lipsum"),
            r#"
title: Lipsum
author: Charles Teague
contributes:
  shortcodes:
    - lipsum.lua
"#,
        );

        let runtime = make_runtime();
        let input_dir = tmp.path().join("project");
        fs::create_dir_all(&input_dir).unwrap();
        let input = input_dir.join("test.qmd");

        let (extensions, _diags) = discover_extensions(&input, None, Some(&builtin_dir), &runtime);

        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].id.name, "lipsum");
        assert_eq!(extensions[0].id.organization.as_deref(), Some("quarto"));
    }

    #[test]
    fn test_user_extension_overrides_builtin() {
        let tmp = TempDir::new().unwrap();

        // Built-in extension
        let builtin_dir = tmp.path().join("builtin");
        write_extension(
            &builtin_dir.join("quarto/lipsum"),
            r#"
title: Lipsum Built-in
author: Charles Teague
contributes:
  shortcodes:
    - lipsum.lua
"#,
        );

        // User extension (unorganized, same name)
        let project_dir = tmp.path().join("project");
        write_extension(
            &project_dir.join("_extensions/lipsum"),
            r#"
title: Lipsum User
author: User
contributes:
  shortcodes:
    - lipsum.lua
"#,
        );

        let runtime = make_runtime();
        let input = project_dir.join("test.qmd");

        let (extensions, _diags) = discover_extensions(&input, None, Some(&builtin_dir), &runtime);

        // Both should be discovered
        assert_eq!(extensions.len(), 2);
        // Built-in first, user second
        assert_eq!(extensions[0].title.as_deref(), Some("Lipsum Built-in"));
        assert_eq!(extensions[1].title.as_deref(), Some("Lipsum User"));

        // find_extension should return user (last match)
        let found = find_extension("lipsum", &extensions).unwrap();
        assert_eq!(found.title.as_deref(), Some("Lipsum User"));
    }

    #[test]
    fn test_user_org_extension_overrides_builtin() {
        let tmp = TempDir::new().unwrap();

        // Built-in extension
        let builtin_dir = tmp.path().join("builtin");
        write_extension(
            &builtin_dir.join("quarto/lipsum"),
            r#"
title: Lipsum Built-in
author: Charles Teague
contributes:
  shortcodes:
    - lipsum.lua
"#,
        );

        // User extension with org (quarto/lipsum)
        let project_dir = tmp.path().join("project");
        write_extension(
            &project_dir.join("_extensions/quarto/lipsum"),
            r#"
title: Lipsum User Org
author: User
contributes:
  shortcodes:
    - lipsum.lua
"#,
        );

        let runtime = make_runtime();
        let input = project_dir.join("test.qmd");

        let (extensions, _diags) = discover_extensions(&input, None, Some(&builtin_dir), &runtime);

        assert_eq!(extensions.len(), 2);

        // find_extension with org/name should return user (last match)
        let found = find_extension("quarto/lipsum", &extensions).unwrap();
        assert_eq!(found.title.as_deref(), Some("Lipsum User Org"));

        // find_extension with bare name should also return user
        let found = find_extension("lipsum", &extensions).unwrap();
        assert_eq!(found.title.as_deref(), Some("Lipsum User Org"));
    }

    // === Format descriptor tests ===

    #[test]
    fn test_parse_format_descriptor_plain() {
        let desc = parse_format_descriptor("html");
        assert!(desc.extension_name.is_none());
        assert_eq!(desc.base_format, "html");
    }

    #[test]
    fn test_parse_format_descriptor_extension_html() {
        let desc = parse_format_descriptor("acm-pdf");
        assert_eq!(desc.extension_name.as_deref(), Some("acm"));
        assert_eq!(desc.base_format, "pdf");
    }

    #[test]
    fn test_parse_format_descriptor_multi_hyphen() {
        let desc = parse_format_descriptor("my-cool-journal-html");
        assert_eq!(desc.extension_name.as_deref(), Some("my-cool-journal"));
        assert_eq!(desc.base_format, "html");
    }

    #[test]
    fn test_parse_format_descriptor_unknown_format() {
        let desc = parse_format_descriptor("unknown");
        assert!(desc.extension_name.is_none());
        assert_eq!(desc.base_format, "unknown");
    }

    #[test]
    fn test_parse_format_descriptor_unknown_suffix() {
        // "bar" is not a known base format, so the whole string is the format
        let desc = parse_format_descriptor("foo-bar");
        assert!(desc.extension_name.is_none());
        assert_eq!(desc.base_format, "foo-bar");
    }

    #[test]
    fn test_parse_format_descriptor_all_base_formats() {
        for base in KNOWN_BASE_FORMATS {
            let input = format!("ext-{}", base);
            let desc = parse_format_descriptor(&input);
            assert_eq!(
                desc.extension_name.as_deref(),
                Some("ext"),
                "Failed for {}",
                base
            );
            assert_eq!(desc.base_format, *base, "Failed for {}", base);
        }
    }
}

/*
 * extension/discover.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Extension discovery from _extensions/ directories.
 */

//! Extension discovery from `_extensions/` directories.

use std::path::Path;

use quarto_system_runtime::{PathKind, SystemRuntime};
use tracing::warn;

use super::read::read_extension;
use super::types::Extension;

/// Discover all extensions available for a document.
///
/// Searches `_extensions/` directories in the project hierarchy,
/// walking from the input file's directory up to the project root.
pub fn discover_extensions(
    input: &Path,
    project_dir: Option<&Path>,
    runtime: &dyn SystemRuntime,
) -> Vec<Extension> {
    let mut extensions = Vec::new();
    let mut dirs_to_search = Vec::new();

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

        let entries = match runtime.dir_list(ext_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries {
            scan_extension_entry(&entry, runtime, &mut extensions);
        }
    }

    extensions
}

/// Scan a single entry in an `_extensions/` directory.
///
/// Could be an unorganized extension (has `_extension.yml` directly)
/// or an organization directory (contains named extension subdirs).
fn scan_extension_entry(
    entry: &Path,
    runtime: &dyn SystemRuntime,
    extensions: &mut Vec<Extension>,
) {
    let ext_file = entry.join("_extension.yml");

    // Check for direct _extension.yml (unorganized extension)
    if runtime
        .path_exists(&ext_file, Some(PathKind::File))
        .unwrap_or(false)
    {
        match read_extension(&ext_file, runtime) {
            Ok(ext) => extensions.push(ext),
            Err(e) => warn!("Failed to read extension {}: {}", ext_file.display(), e),
        }
        return;
    }

    // Check subdirectories (organized: org/name/)
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
            match read_extension(&sub_ext_file, runtime) {
                Ok(ext) => extensions.push(ext),
                Err(e) => warn!("Failed to read extension {}: {}", sub_ext_file.display(), e),
            }
        }
    }
}

/// Find a specific extension by name among discovered extensions.
///
/// If `name` contains `/`, split into `org/name` and match both.
/// Otherwise, match by name only (any organization).
pub fn find_extension<'a>(name: &str, extensions: &'a [Extension]) -> Option<&'a Extension> {
    if let Some((org, ext_name)) = name.split_once('/') {
        extensions
            .iter()
            .find(|e| e.id.name == ext_name && e.id.organization.as_deref() == Some(org))
    } else {
        extensions.iter().find(|e| e.id.name == name)
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
        let extensions = discover_extensions(&input, None, &runtime);

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
        let extensions = discover_extensions(&input, None, &runtime);

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
        let extensions = discover_extensions(&input, Some(project_dir), &runtime);

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
        let extensions = discover_extensions(&input, None, &runtime);

        assert!(extensions.is_empty());
    }

    #[test]
    fn test_discover_no_extensions_dir() {
        let tmp = TempDir::new().unwrap();

        let runtime = make_runtime();
        let input = tmp.path().join("test.qmd");
        let extensions = discover_extensions(&input, None, &runtime);

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

        // Invalid extension (missing title)
        write_extension(
            &tmp.path().join("_extensions/bad-ext"),
            r#"
author: Author
contributes:
  shortcodes:
    - hello.lua
"#,
        );

        let runtime = make_runtime();
        let input = tmp.path().join("test.qmd");
        let extensions = discover_extensions(&input, None, &runtime);

        // Only the valid extension should be discovered
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].id.name, "good-ext");
    }

    // === find_extension tests ===

    #[test]
    fn test_find_extension_by_name() {
        let ext = Extension {
            id: super::super::types::ExtensionId::new("lightbox"),
            title: "Lightbox".to_string(),
            author: "Author".to_string(),
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
            title: "ACM".to_string(),
            author: "Author".to_string(),
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

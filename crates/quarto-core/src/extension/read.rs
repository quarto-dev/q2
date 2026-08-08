/*
 * extension/read.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Parse _extension.yml files into Extension structs.
 */

//! Parser for `_extension.yml` files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use quarto_config::MergedConfig;
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_system_runtime::SystemRuntime;

use super::types::{Contributes, Extension, ExtensionFilter, ExtensionId};
use crate::error::Result;

/// Read and parse an `_extension.yml` file.
///
/// All relative paths in the extension are resolved to absolute paths
/// relative to the extension directory (parent of the `_extension.yml` file).
///
/// When `organization` is `Some`, it is used directly as the extension's
/// org (the scanner already determined this from directory structure).
/// When `None`, the organization is derived from the path heuristic
/// (checking for `_extensions/org/name/` layout).
pub fn read_extension(extension_file: &Path, runtime: &dyn SystemRuntime) -> Result<Extension> {
    read_extension_with_org(extension_file, None, runtime)
}

/// Read an extension with an explicit organization override.
pub fn read_extension_with_org(
    extension_file: &Path,
    organization: Option<&str>,
    runtime: &dyn SystemRuntime,
) -> Result<Extension> {
    let content = runtime.file_read_string(extension_file).map_err(|e| {
        crate::error::QuartoError::Other(format!(
            "Failed to read {}: {}",
            extension_file.display(),
            e
        ))
    })?;

    let ext_dir = extension_file.parent().ok_or_else(|| {
        crate::error::QuartoError::Other("Extension file has no parent directory".to_string())
    })?;

    // Use explicit org if provided, otherwise derive from directory structure.
    let (ext_name, ext_org) = if let Some(org) = organization {
        let name = ext_dir.file_name().map_or_else(
            || "unknown".to_string(),
            |n| n.to_string_lossy().to_string(),
        );
        (name, Some(org.to_string()))
    } else {
        derive_extension_id(ext_dir)
    };

    let filename = extension_file.display().to_string();
    let yaml = quarto_yaml::parse_file(&content, &filename).map_err(|e| {
        crate::error::QuartoError::Other(format!(
            "Failed to parse {}: {}",
            extension_file.display(),
            e
        ))
    })?;

    let mut diagnostics = pampa::utils::diagnostic_collector::DiagnosticCollector::new();
    let config = pampa::pandoc::yaml_to_config_value(
        yaml,
        quarto_config::InterpretationContext::ProjectConfig,
        &mut diagnostics,
    );

    // Optional metadata fields (Q1-compat: no named field is required —
    // bd-8b0af414). `as_plain_text` rather than `as_str` so values that
    // parse as PandocInlines still come through.
    let title = config.get("title").and_then(|v| v.as_plain_text());
    let author = config.get("author").and_then(|v| v.as_plain_text());

    let version = config.get("version").and_then(|v| v.as_plain_text());
    let quarto_required = config
        .get("quarto-required")
        .and_then(|v| v.as_plain_text());

    // Extract contributes
    let contributes_cv = config.get("contributes").ok_or_else(|| {
        crate::error::QuartoError::Other(format!(
            "{}: missing required 'contributes' field",
            extension_file.display()
        ))
    })?;

    let contributes = parse_contributes(contributes_cv, ext_dir, runtime)?;

    Ok(Extension {
        id: if let Some(org) = ext_org {
            ExtensionId::with_organization(ext_name, org)
        } else {
            ExtensionId::new(ext_name)
        },
        title,
        author,
        version,
        quarto_required,
        path: ext_dir.to_path_buf(),
        contributes,
    })
}

/// Derive extension name and organization from the directory path.
///
/// `_extensions/org/name/` → (name, Some(org))
/// `_extensions/name/` → (name, None)
fn derive_extension_id(ext_dir: &Path) -> (String, Option<String>) {
    let name = ext_dir.file_name().map_or_else(
        || "unknown".to_string(),
        |n| n.to_string_lossy().to_string(),
    );

    // Check if parent's parent is named "_extensions" (organized layout)
    let org = ext_dir.parent().and_then(|parent| {
        let grandparent = parent.parent()?;
        if grandparent.file_name()?.to_str()? == "_extensions" {
            Some(parent.file_name()?.to_string_lossy().to_string())
        } else {
            None
        }
    });

    (name, org)
}

/// Parse the `contributes` section of an `_extension.yml`.
fn parse_contributes(
    contributes: &ConfigValue,
    ext_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<Contributes> {
    let mut result = Contributes::default();

    // Parse formats with "common" key merging
    if let Some(formats_cv) = contributes.get("formats") {
        result.formats = parse_formats(formats_cv, ext_dir, runtime)?;
    }

    // Parse filters
    if let Some(filters_cv) = contributes.get("filters") {
        result.filters = parse_filters(filters_cv, ext_dir);
    }

    // Parse shortcodes
    if let Some(shortcodes_cv) = contributes.get("shortcodes") {
        result.shortcodes = parse_shortcodes(shortcodes_cv, ext_dir);
    }

    // Store raw metadata and project contributions
    result.metadata = contributes.get("metadata").cloned();
    result.project = contributes.get("project").cloned();

    // Validate that contributes has at least one sub-field
    if result.formats.is_empty()
        && result.filters.is_empty()
        && result.shortcodes.is_empty()
        && result.metadata.is_none()
        && result.project.is_none()
    {
        return Err(crate::error::QuartoError::Other(
            "Extension 'contributes' must have at least one of: formats, filters, shortcodes, metadata, project".to_string(),
        ));
    }

    Ok(result)
}

/// Parse formats with "common" key merging.
///
/// The `common` key's values serve as defaults for all other format keys.
fn parse_formats(
    formats_cv: &ConfigValue,
    ext_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<HashMap<String, ConfigValue>> {
    let mut result = HashMap::new();

    let ConfigValueKind::Map(entries) = &formats_cv.value else {
        return Ok(result);
    };

    // Extract common key if present
    let common = entries.iter().find(|e| e.key == "common").map(|e| &e.value);

    // Process each non-common format
    for entry in entries {
        if entry.key == "common" {
            continue;
        }

        let mut merged_value = if let Some(common_cv) = common {
            // Merge: common is lower priority, format-specific is higher
            let layers: Vec<&ConfigValue> = vec![common_cv, &entry.value];
            let merged = MergedConfig::new(layers);
            merged.materialize().unwrap_or_else(|_| entry.value.clone())
        } else {
            entry.value.clone()
        };

        // Convert known path-valued keys to ConfigValueKind::Path so that
        // adjust_paths_to_document_dir() will rebase them during metadata merge.
        mark_path_valued_keys(&mut merged_value);

        // Existence-driven marking for keys whose strings may be either
        // bundled files or something else (builtin theme names, doc-relative
        // references) — the filesystem disambiguates (bd-of20unsb).
        super::paths::mark_bundled_format_assets(&mut merged_value, ext_dir, runtime);

        result.insert(entry.key.clone(), merged_value);
    }

    Ok(result)
}

/// Keys in extension format config whose values are file paths relative to
/// the extension directory.
const PATH_VALUED_KEYS: &[&str] = &["template", "template-partials", "shortcodes"];

/// Reserved filter names that should NOT be marked as Path.
/// These are special identifiers, not file paths.
const FILTER_RESERVED_NAMES: &[&str] = &["citeproc", "quarto"];

/// Convert scalar string values for known path-valued keys to
/// `ConfigValueKind::Path`. For array-valued keys (like `template-partials`),
/// each element is converted.
fn mark_path_valued_keys(format_config: &mut ConfigValue) {
    let ConfigValueKind::Map(entries) = &mut format_config.value else {
        return;
    };
    for entry in entries.iter_mut() {
        // Handle filters separately: array of strings and maps with reserved name exclusion
        if entry.key == "filters" {
            if let ConfigValueKind::Array(items) = &mut entry.value.value {
                for item in items.iter_mut() {
                    match &mut item.value {
                        // String form: mark as Path unless reserved
                        ConfigValueKind::Scalar(yaml) => {
                            if let Some(s) = yaml.as_str()
                                && !FILTER_RESERVED_NAMES.contains(&s)
                            {
                                item.value = ConfigValueKind::Path(s.to_string());
                            }
                        }
                        // Map form: {path: "filter.lua", at: "post-render"}
                        // Always mark the path sub-key
                        ConfigValueKind::Map(map_entries) => {
                            if let Some(path_entry) =
                                map_entries.iter_mut().find(|e| e.key == "path")
                                && let ConfigValueKind::Scalar(yaml) = &path_entry.value.value
                                && let Some(s) = yaml.as_str()
                            {
                                path_entry.value.value = ConfigValueKind::Path(s.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            continue;
        }

        if !PATH_VALUED_KEYS.contains(&entry.key.as_str()) {
            continue;
        }
        match &mut entry.value.value {
            ConfigValueKind::Scalar(yaml) => {
                if let Some(s) = yaml.as_str() {
                    entry.value.value = ConfigValueKind::Path(s.to_string());
                }
            }
            ConfigValueKind::Array(items) => {
                for item in items.iter_mut() {
                    if let ConfigValueKind::Scalar(yaml) = &item.value
                        && let Some(s) = yaml.as_str()
                    {
                        item.value = ConfigValueKind::Path(s.to_string());
                    }
                }
            }
            _ => {}
        }
    }
}

/// Parse filters from the contributes section.
fn parse_filters(filters_cv: &ConfigValue, ext_dir: &Path) -> Vec<ExtensionFilter> {
    let ConfigValueKind::Array(items) = &filters_cv.value else {
        return vec![];
    };

    items
        .iter()
        .filter_map(|item| {
            match &item.value {
                // Simple string form: "filter.lua"
                ConfigValueKind::Scalar(_) => {
                    let path_str = item.as_str()?;
                    Some(ExtensionFilter {
                        path: ext_dir.join(path_str),
                        at: None,
                    })
                }
                // Map form: { path: "filter.lua", at: "post-render" }
                ConfigValueKind::Map(_) => {
                    let path_str = item.get("path")?.as_str()?;
                    let at = item
                        .get("at")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    Some(ExtensionFilter {
                        path: ext_dir.join(path_str),
                        at,
                    })
                }
                _ => None,
            }
        })
        .collect()
}

/// Parse shortcodes from the contributes section.
fn parse_shortcodes(shortcodes_cv: &ConfigValue, ext_dir: &Path) -> Vec<PathBuf> {
    let ConfigValueKind::Array(items) = &shortcodes_cv.value else {
        return vec![];
    };

    items
        .iter()
        .filter_map(|item| {
            let path_str = item.as_str()?;
            Some(ext_dir.join(path_str))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_runtime() -> quarto_system_runtime::NativeRuntime {
        quarto_system_runtime::NativeRuntime::new()
    }

    fn write_extension(dir: &Path, yaml: &str) -> std::path::PathBuf {
        fs::create_dir_all(dir).unwrap();
        let file = dir.join("_extension.yml");
        fs::write(&file, yaml).unwrap();
        file
    }

    #[test]
    fn test_read_q1_compat_manifest_without_title_author() {
        // Q1 requires no named fields in _extension.yml; real extensions
        // (julia-engine, marimo) omit title/author. Only `contributes` is
        // structurally required (bd-8b0af414).
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/bare-ext");
        let file = write_extension(
            &ext_dir,
            r#"
contributes:
  shortcodes:
    - bare.lua
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        assert_eq!(ext.id.name, "bare-ext");
        assert_eq!(ext.title, None);
        assert_eq!(ext.author, None);
        assert_eq!(ext.contributes.shortcodes.len(), 1);
    }

    #[test]
    fn test_read_minimal_extension() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Test Extension
author: Test Author
contributes:
  shortcodes:
    - hello.lua
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        assert_eq!(ext.id.name, "test-ext");
        assert_eq!(ext.id.organization, None);
        assert_eq!(ext.title.as_deref(), Some("Test Extension"));
        assert_eq!(ext.author.as_deref(), Some("Test Author"));
        assert!(ext.version.is_none());
        assert_eq!(ext.contributes.shortcodes.len(), 1);
        assert_eq!(ext.contributes.shortcodes[0], ext_dir.join("hello.lua"));
    }

    #[test]
    fn test_read_extension_with_formats_and_common() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Format Extension
author: Author
contributes:
  formats:
    common:
      toc: true
      number-sections: true
    html:
      theme: cosmo
    pdf:
      documentclass: article
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        // HTML should have toc + number-sections + theme
        let html_meta = &ext.contributes.formats["html"];
        assert_eq!(html_meta.get("toc").unwrap().as_bool(), Some(true));
        assert_eq!(
            html_meta.get("number-sections").unwrap().as_bool(),
            Some(true)
        );
        assert_eq!(html_meta.get("theme").unwrap().as_str(), Some("cosmo"));

        // PDF should have toc + number-sections + documentclass
        let pdf_meta = &ext.contributes.formats["pdf"];
        assert_eq!(pdf_meta.get("toc").unwrap().as_bool(), Some(true));
        assert_eq!(
            pdf_meta.get("number-sections").unwrap().as_bool(),
            Some(true)
        );
        assert_eq!(
            pdf_meta.get("documentclass").unwrap().as_str(),
            Some("article")
        );

        // common key should not be present
        assert!(!ext.contributes.formats.contains_key("common"));
    }

    #[test]
    fn test_format_specific_overrides_common() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Override Test
author: Author
contributes:
  formats:
    common:
      toc: true
    html:
      toc: false
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        assert_eq!(html_meta.get("toc").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn test_read_extension_with_filters() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Filter Extension
author: Author
contributes:
  filters:
    - filter.lua
    - path: other.lua
      at: post-render
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        assert_eq!(ext.contributes.filters.len(), 2);
        assert_eq!(ext.contributes.filters[0].path, ext_dir.join("filter.lua"));
        assert!(ext.contributes.filters[0].at.is_none());
        assert_eq!(ext.contributes.filters[1].path, ext_dir.join("other.lua"));
        assert_eq!(
            ext.contributes.filters[1].at.as_deref(),
            Some("post-render")
        );
    }

    #[test]
    fn test_read_extension_missing_title() {
        // Q1-compat intake (bd-8b0af414): missing title is NOT an error;
        // it just loads with title: None.
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        let file = write_extension(
            &ext_dir,
            r#"
author: Author
contributes:
  shortcodes:
    - hello.lua
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();
        assert_eq!(ext.title, None);
        assert_eq!(ext.author.as_deref(), Some("Author"));
    }

    #[test]
    fn test_read_extension_missing_contributes() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: No Contributes
author: Author
"#,
        );

        let runtime = make_runtime();
        let err = read_extension(&file, &runtime).unwrap_err();
        assert!(
            err.to_string().contains("contributes"),
            "Error should mention 'contributes': {}",
            err
        );
    }

    #[test]
    fn test_read_extension_empty_contributes() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Empty Contributes
author: Author
contributes:
  formats:
"#,
        );

        let runtime = make_runtime();
        let err = read_extension(&file, &runtime).unwrap_err();
        assert!(
            err.to_string().contains("at least one"),
            "Error should mention at least one sub-field: {}",
            err
        );
    }

    #[test]
    fn test_organized_extension_id() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/quarto-journals/acm");
        let file = write_extension(
            &ext_dir,
            r#"
title: ACM
author: Quarto
contributes:
  formats:
    pdf:
      documentclass: acmart
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        assert_eq!(ext.id.name, "acm");
        assert_eq!(ext.id.organization.as_deref(), Some("quarto-journals"));
    }

    #[test]
    fn test_extension_with_version_and_quarto_required() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Versioned Extension
author: Author
version: 1.2.3
quarto-required: ">= 1.4.0"
contributes:
  shortcodes:
    - hello.lua
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        assert_eq!(ext.version.as_deref(), Some("1.2.3"));
        assert_eq!(ext.quarto_required.as_deref(), Some(">= 1.4.0"));
    }

    #[test]
    fn test_template_converted_to_path_kind() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/acm");
        let file = write_extension(
            &ext_dir,
            r#"
title: ACM Format
author: Author
contributes:
  formats:
    html:
      template: template.html
      toc: true
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];

        // template should be ConfigValueKind::Path, not Scalar
        let template_cv = html_meta.get("template").unwrap();
        assert!(
            matches!(&template_cv.value, ConfigValueKind::Path(s) if s == "template.html"),
            "expected Path(\"template.html\"), got {:?}",
            template_cv.value
        );

        // toc should remain unchanged (boolean, not converted)
        assert_eq!(html_meta.get("toc").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn test_template_partials_converted_to_path_kind() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/acm");
        let file = write_extension(
            &ext_dir,
            r#"
title: ACM Format
author: Author
contributes:
  formats:
    html:
      template-partials:
        - title-block.html
        - header.html
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let partials = html_meta.get("template-partials").unwrap();
        let items = partials.as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            matches!(&items[0].value, ConfigValueKind::Path(s) if s == "title-block.html"),
            "expected Path(\"title-block.html\"), got {:?}",
            items[0].value
        );
        assert!(
            matches!(&items[1].value, ConfigValueKind::Path(s) if s == "header.html"),
            "expected Path(\"header.html\"), got {:?}",
            items[1].value
        );
    }

    #[test]
    fn test_format_filter_string_marked_as_path() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/acm");
        let file = write_extension(
            &ext_dir,
            r#"
title: ACM Format
author: Author
contributes:
  formats:
    html:
      filters:
        - filter.lua
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let filters = html_meta.get("filters").unwrap();
        let items = filters.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            matches!(&items[0].value, ConfigValueKind::Path(s) if s == "filter.lua"),
            "expected Path(\"filter.lua\"), got {:?}",
            items[0].value
        );
    }

    #[test]
    fn test_format_filter_map_path_marked() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/acm");
        let file = write_extension(
            &ext_dir,
            r#"
title: ACM Format
author: Author
contributes:
  formats:
    html:
      filters:
        - path: f.lua
          at: post-render
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let filters = html_meta.get("filters").unwrap();
        let items = filters.as_array().unwrap();
        assert_eq!(items.len(), 1);
        // The path sub-key value should be marked as Path
        let path_val = items[0].get("path").unwrap();
        assert!(
            matches!(&path_val.value, ConfigValueKind::Path(s) if s == "f.lua"),
            "expected Path(\"f.lua\"), got {:?}",
            path_val.value
        );
        // The at sub-key should remain unchanged
        let at_val = items[0].get("at").unwrap();
        assert_eq!(at_val.as_str(), Some("post-render"));
    }

    #[test]
    fn test_format_filter_citeproc_not_marked() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/acm");
        let file = write_extension(
            &ext_dir,
            r#"
title: ACM Format
author: Author
contributes:
  formats:
    html:
      filters:
        - citeproc
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let filters = html_meta.get("filters").unwrap();
        let items = filters.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            matches!(&items[0].value, ConfigValueKind::Scalar(_)),
            "expected Scalar for citeproc, got {:?}",
            items[0].value
        );
    }

    #[test]
    fn test_format_filter_quarto_not_marked() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/acm");
        let file = write_extension(
            &ext_dir,
            r#"
title: ACM Format
author: Author
contributes:
  formats:
    html:
      filters:
        - quarto
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let filters = html_meta.get("filters").unwrap();
        let items = filters.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            matches!(&items[0].value, ConfigValueKind::Scalar(_)),
            "expected Scalar for quarto, got {:?}",
            items[0].value
        );
    }

    #[test]
    fn test_format_filter_mixed_entries() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/acm");
        let file = write_extension(
            &ext_dir,
            r#"
title: ACM Format
author: Author
contributes:
  formats:
    html:
      filters:
        - pre.lua
        - citeproc
        - quarto
        - path: post.lua
          at: post-render
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let filters = html_meta.get("filters").unwrap();
        let items = filters.as_array().unwrap();
        assert_eq!(items.len(), 4);

        // pre.lua → Path
        assert!(
            matches!(&items[0].value, ConfigValueKind::Path(s) if s == "pre.lua"),
            "expected Path(\"pre.lua\"), got {:?}",
            items[0].value
        );
        // citeproc → Scalar (not marked)
        assert!(
            matches!(&items[1].value, ConfigValueKind::Scalar(_)),
            "expected Scalar for citeproc, got {:?}",
            items[1].value
        );
        // quarto → Scalar (not marked)
        assert!(
            matches!(&items[2].value, ConfigValueKind::Scalar(_)),
            "expected Scalar for quarto, got {:?}",
            items[2].value
        );
        // post.lua map → path sub-key marked as Path
        let path_val = items[3].get("path").unwrap();
        assert!(
            matches!(&path_val.value, ConfigValueKind::Path(s) if s == "post.lua"),
            "expected Path(\"post.lua\"), got {:?}",
            path_val.value
        );
    }

    #[test]
    fn test_non_path_metadata_unaffected_by_path_conversion() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test");
        let file = write_extension(
            &ext_dir,
            r#"
title: Test
author: Author
contributes:
  formats:
    html:
      toc: true
      theme: cosmo
      number-sections: true
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        assert_eq!(html_meta.get("toc").unwrap().as_bool(), Some(true));
        assert_eq!(html_meta.get("theme").unwrap().as_str(), Some("cosmo"));
        assert_eq!(
            html_meta.get("number-sections").unwrap().as_bool(),
            Some(true)
        );
    }

    #[test]
    fn test_format_shortcode_paths_marked() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test");
        let file = write_extension(
            &ext_dir,
            r#"
title: Test
author: Author
contributes:
  formats:
    html:
      shortcodes:
        - handler.lua
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let shortcodes = html_meta.get("shortcodes").unwrap();
        let items = shortcodes.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            matches!(&items[0].value, ConfigValueKind::Path(s) if s == "handler.lua"),
            "expected Path(\"handler.lua\"), got {:?}",
            items[0].value
        );
    }

    #[test]
    fn test_format_shortcode_multiple_paths_marked() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test");
        let file = write_extension(
            &ext_dir,
            r#"
title: Test
author: Author
contributes:
  formats:
    html:
      shortcodes:
        - hello.lua
        - goodbye.lua
        - utils/helper.lua
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let shortcodes = html_meta.get("shortcodes").unwrap();
        let items = shortcodes.as_array().unwrap();
        assert_eq!(items.len(), 3);
        for (i, expected) in ["hello.lua", "goodbye.lua", "utils/helper.lua"]
            .iter()
            .enumerate()
        {
            assert!(
                matches!(&items[i].value, ConfigValueKind::Path(s) if s == *expected),
                "expected Path(\"{}\"), got {:?}",
                expected,
                items[i].value
            );
        }
    }

    // --- bd-of20unsb: existence-driven Path marking for bundled ---
    // --- format assets (theme / css / include-* / format-resources) ---

    /// Create an asset file at `rel` under the extension dir, so the
    /// existence-driven marking classifies the string as a bundled file.
    fn write_asset(ext_dir: &Path, rel: &str) {
        let p = ext_dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "/* asset */\n").unwrap();
    }

    #[test]
    fn test_theme_bundled_scss_marked_as_path_builtin_stays_scalar() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/fancyfmt");
        let file = write_extension(
            &ext_dir,
            r#"
contributes:
  formats:
    html:
      theme: [cosmo, fmt-theme.scss]
"#,
        );
        write_asset(&ext_dir, "fmt-theme.scss");

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let items = html_meta.get("theme").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 2);
        // Built-in theme name: no such file under the extension dir,
        // so it must stay Scalar (the theme stage resolves it by name).
        assert!(
            matches!(&items[0].value, ConfigValueKind::Scalar(_)),
            "expected Scalar for builtin name `cosmo`, got {:?}",
            items[0].value
        );
        // Bundled file: exists next to _extension.yml, so it must be
        // marked Path (ext-dir-relative) for the merge-time rebase.
        assert!(
            matches!(&items[1].value, ConfigValueKind::Path(s) if s == "fmt-theme.scss"),
            "expected Path(\"fmt-theme.scss\"), got {:?}",
            items[1].value
        );
    }

    #[test]
    fn test_theme_scalar_bundled_file_marked_as_path() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/fancyfmt");
        let file = write_extension(
            &ext_dir,
            r#"
contributes:
  formats:
    html:
      theme: fmt-theme.scss
"#,
        );
        write_asset(&ext_dir, "fmt-theme.scss");

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let theme = html_meta.get("theme").unwrap();
        assert!(
            matches!(&theme.value, ConfigValueKind::Path(s) if s == "fmt-theme.scss"),
            "expected Path(\"fmt-theme.scss\"), got {:?}",
            theme.value
        );
    }

    #[test]
    fn test_theme_missing_file_stays_scalar() {
        // A `.scss` string with no matching bundled file is NOT the
        // extension's to claim — it stays Scalar and resolves (or
        // hard-errors) downstream relative to the document.
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/fancyfmt");
        let file = write_extension(
            &ext_dir,
            r#"
contributes:
  formats:
    html:
      theme: [missing.scss]
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let items = html_meta.get("theme").unwrap().as_array().unwrap();
        assert!(
            matches!(&items[0].value, ConfigValueKind::Scalar(_)),
            "expected Scalar for missing.scss (no bundled file), got {:?}",
            items[0].value
        );
    }

    #[test]
    fn test_theme_nested_map_leaves_marked() {
        // The light/dark map form: pattern exhaustion at `theme` marks
        // every string leaf underneath, existence-driven per leaf.
        // (Consumption of light/dark maps is bd-o76p01wb; marking now
        // means that fix composes with this one.)
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/fancyfmt");
        let file = write_extension(
            &ext_dir,
            r#"
contributes:
  formats:
    html:
      theme:
        light: [flatly, fmt-light.scss]
        dark: fmt-dark.scss
"#,
        );
        write_asset(&ext_dir, "fmt-light.scss");
        write_asset(&ext_dir, "fmt-dark.scss");

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let theme = html_meta.get("theme").unwrap();
        let light = theme.get("light").unwrap().as_array().unwrap();
        assert!(
            matches!(&light[0].value, ConfigValueKind::Scalar(_)),
            "expected Scalar for builtin `flatly`, got {:?}",
            light[0].value
        );
        assert!(
            matches!(&light[1].value, ConfigValueKind::Path(s) if s == "fmt-light.scss"),
            "expected Path(\"fmt-light.scss\"), got {:?}",
            light[1].value
        );
        let dark = theme.get("dark").unwrap();
        assert!(
            matches!(&dark.value, ConfigValueKind::Path(s) if s == "fmt-dark.scss"),
            "expected Path(\"fmt-dark.scss\"), got {:?}",
            dark.value
        );
    }

    #[test]
    fn test_css_bundled_file_marked_missing_stays_scalar() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/fancyfmt");
        let file = write_extension(
            &ext_dir,
            r#"
contributes:
  formats:
    html:
      css: [fmt-style.css, not-bundled.css]
"#,
        );
        write_asset(&ext_dir, "fmt-style.css");

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let items = html_meta.get("css").unwrap().as_array().unwrap();
        assert!(
            matches!(&items[0].value, ConfigValueKind::Path(s) if s == "fmt-style.css"),
            "expected Path(\"fmt-style.css\"), got {:?}",
            items[0].value
        );
        assert!(
            matches!(&items[1].value, ConfigValueKind::Scalar(_)),
            "expected Scalar for not-bundled.css, got {:?}",
            items[1].value
        );
    }

    #[test]
    fn test_include_keys_bundled_files_marked() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/fancyfmt");
        let file = write_extension(
            &ext_dir,
            r#"
contributes:
  formats:
    html:
      include-in-header: header.html
      include-before-body: [before.html]
      include-after-body: after.html
"#,
        );
        write_asset(&ext_dir, "header.html");
        write_asset(&ext_dir, "before.html");
        write_asset(&ext_dir, "after.html");

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        for (key, expected) in [
            ("include-in-header", "header.html"),
            ("include-after-body", "after.html"),
        ] {
            let v = html_meta.get(key).unwrap();
            assert!(
                matches!(&v.value, ConfigValueKind::Path(s) if s == expected),
                "{}: expected Path(\"{}\"), got {:?}",
                key,
                expected,
                v.value
            );
        }
        let before = html_meta
            .get("include-before-body")
            .unwrap()
            .as_array()
            .unwrap();
        assert!(
            matches!(&before[0].value, ConfigValueKind::Path(s) if s == "before.html"),
            "expected Path(\"before.html\"), got {:?}",
            before[0].value
        );
    }

    #[test]
    fn test_format_resources_bundled_subdir_file_marked() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/fancyfmt");
        let file = write_extension(
            &ext_dir,
            r#"
contributes:
  formats:
    html:
      format-resources: [fonts/fancy.woff2]
"#,
        );
        write_asset(&ext_dir, "fonts/fancy.woff2");

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        let items = html_meta
            .get("format-resources")
            .unwrap()
            .as_array()
            .unwrap();
        assert!(
            matches!(&items[0].value, ConfigValueKind::Path(s) if s == "fonts/fancy.woff2"),
            "expected Path(\"fonts/fancy.woff2\"), got {:?}",
            items[0].value
        );
    }

    #[test]
    fn test_shortcode_marking_doesnt_affect_other_keys() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test");
        let file = write_extension(
            &ext_dir,
            r#"
title: Test
author: Author
contributes:
  formats:
    html:
      shortcodes:
        - handler.lua
      toc: true
      theme: cosmo
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        let html_meta = &ext.contributes.formats["html"];
        // shortcodes should be marked
        let shortcodes = html_meta.get("shortcodes").unwrap();
        let items = shortcodes.as_array().unwrap();
        assert!(matches!(&items[0].value, ConfigValueKind::Path(_)));
        // other keys should be unchanged
        assert_eq!(html_meta.get("toc").unwrap().as_bool(), Some(true));
        assert_eq!(html_meta.get("theme").unwrap().as_str(), Some("cosmo"));
    }
}

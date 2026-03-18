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
pub fn read_extension(extension_file: &Path, runtime: &dyn SystemRuntime) -> Result<Extension> {
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

    // Derive extension name and organization from directory structure.
    // _extensions/org/name/ → org + name
    // _extensions/name/ → just name
    let (ext_name, ext_org) = derive_extension_id(ext_dir);

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

    // Extract required fields
    let title = config
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            crate::error::QuartoError::Other(format!(
                "{}: missing required 'title' field",
                extension_file.display()
            ))
        })?
        .to_string();

    let author = config
        .get("author")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            crate::error::QuartoError::Other(format!(
                "{}: missing required 'author' field",
                extension_file.display()
            ))
        })?
        .to_string();

    let version = config
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let quarto_required = config
        .get("quarto-required")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract contributes
    let contributes_cv = config.get("contributes").ok_or_else(|| {
        crate::error::QuartoError::Other(format!(
            "{}: missing required 'contributes' field",
            extension_file.display()
        ))
    })?;

    let contributes = parse_contributes(contributes_cv, ext_dir)?;

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
fn parse_contributes(contributes: &ConfigValue, ext_dir: &Path) -> Result<Contributes> {
    let mut result = Contributes::default();

    // Parse formats with "common" key merging
    if let Some(formats_cv) = contributes.get("formats") {
        result.formats = parse_formats(formats_cv, ext_dir)?;
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
    _ext_dir: &Path,
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

        result.insert(entry.key.clone(), merged_value);
    }

    Ok(result)
}

/// Keys in extension format config whose values are file paths relative to
/// the extension directory.
const PATH_VALUED_KEYS: &[&str] = &["template", "template-partials"];

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
                            if let Some(s) = yaml.as_str() {
                                if !FILTER_RESERVED_NAMES.contains(&s) {
                                    item.value = ConfigValueKind::Path(s.to_string());
                                }
                            }
                        }
                        // Map form: {path: "filter.lua", at: "post-render"}
                        // Always mark the path sub-key
                        ConfigValueKind::Map(map_entries) => {
                            if let Some(path_entry) =
                                map_entries.iter_mut().find(|e| e.key == "path")
                            {
                                if let ConfigValueKind::Scalar(yaml) = &path_entry.value.value {
                                    if let Some(s) = yaml.as_str() {
                                        path_entry.value.value =
                                            ConfigValueKind::Path(s.to_string());
                                    }
                                }
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
                    if let ConfigValueKind::Scalar(yaml) = &item.value {
                        if let Some(s) = yaml.as_str() {
                            item.value = ConfigValueKind::Path(s.to_string());
                        }
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
        assert_eq!(ext.title, "Test Extension");
        assert_eq!(ext.author, "Test Author");
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
        let html_meta = ext.contributes.formats.get("html").unwrap();
        assert_eq!(html_meta.get("toc").unwrap().as_bool(), Some(true));
        assert_eq!(
            html_meta.get("number-sections").unwrap().as_bool(),
            Some(true)
        );
        assert_eq!(html_meta.get("theme").unwrap().as_str(), Some("cosmo"));

        // PDF should have toc + number-sections + documentclass
        let pdf_meta = ext.contributes.formats.get("pdf").unwrap();
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
        assert!(ext.contributes.formats.get("common").is_none());
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

        let html_meta = ext.contributes.formats.get("html").unwrap();
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
        let err = read_extension(&file, &runtime).unwrap_err();
        assert!(
            err.to_string().contains("title"),
            "Error should mention 'title': {}",
            err
        );
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

        let html_meta = ext.contributes.formats.get("html").unwrap();

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

        let html_meta = ext.contributes.formats.get("html").unwrap();
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

        let html_meta = ext.contributes.formats.get("html").unwrap();
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

        let html_meta = ext.contributes.formats.get("html").unwrap();
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

        let html_meta = ext.contributes.formats.get("html").unwrap();
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

        let html_meta = ext.contributes.formats.get("html").unwrap();
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

        let html_meta = ext.contributes.formats.get("html").unwrap();
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

        let html_meta = ext.contributes.formats.get("html").unwrap();
        assert_eq!(html_meta.get("toc").unwrap().as_bool(), Some(true));
        assert_eq!(html_meta.get("theme").unwrap().as_str(), Some("cosmo"));
        assert_eq!(
            html_meta.get("number-sections").unwrap().as_bool(),
            Some(true)
        );
    }
}

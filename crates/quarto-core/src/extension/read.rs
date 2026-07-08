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

use super::types::{
    ClaimKind, Contributes, EngineContribution, Extension, ExtensionFilter, ExtensionId, FileClaim,
    StaticLanguageClaim,
};
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

    // `title` and `author` are OPTIONAL, matching Quarto 1: `schema/extension.yml`
    // has no top-level `required:` marker and `readExtension` reads both leniently.
    // `title` falls back to the extension's id name (Q1's `title || id.name`);
    // `author` stays `None` when absent (Q1 stores it as `string | undefined`, and
    // a real shipped Q1 extension — julia-engine — has no `author`). Requiring
    // either here silently dropped otherwise-valid Q1-era extensions (bd-8b0af414).
    let title = config
        .get("title")
        .and_then(|v| v.as_str())
        .map_or_else(|| ext_name.clone(), |s| s.to_string());

    let author = config
        .get("author")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

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

    let contributes = parse_contributes(contributes_cv, ext_dir, extension_file)?;

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
///
/// `extension_file` is the absolute path to the `_extension.yml` itself
/// (distinct from `ext_dir`, its parent directory) — threaded down to
/// `parse_external_engine` so each `EngineContribution::External` carries
/// its provenance (Plan 6 Phase 5).
fn parse_contributes(
    contributes: &ConfigValue,
    ext_dir: &Path,
    extension_file: &Path,
) -> Result<Contributes> {
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

    // Parse engines
    if let Some(engines_cv) = contributes.get("engines") {
        result.engines = parse_engines(engines_cv, ext_dir, extension_file)?;
    }

    // Validate that contributes has at least one sub-field (engines count too)
    if result.formats.is_empty()
        && result.filters.is_empty()
        && result.shortcodes.is_empty()
        && result.metadata.is_none()
        && result.project.is_none()
        && result.engines.is_empty()
    {
        return Err(crate::error::QuartoError::Other(
            "Extension 'contributes' must have at least one of: formats, filters, shortcodes, metadata, project, engines".to_string(),
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

/// Parse the `contributes.engines` array.
///
/// Each element is either:
/// - A bare string → `EngineContribution::Reorder { name }`.
/// - A map with a `path` key → `EngineContribution::External { .. }`.
fn parse_engines(
    engines_cv: &ConfigValue,
    ext_dir: &Path,
    extension_file: &Path,
) -> Result<Vec<EngineContribution>> {
    let ConfigValueKind::Array(items) = &engines_cv.value else {
        return Ok(vec![]);
    };

    let mut result = Vec::new();
    for item in items {
        match &item.value {
            ConfigValueKind::Scalar(_) | ConfigValueKind::PandocInlines(_) => {
                // Bare string → Reorder hint
                if let Some(name) = item.as_str().map(|s| s.to_string()) {
                    result.push(EngineContribution::Reorder { name });
                }
            }
            ConfigValueKind::Map(_) => {
                let contribution = parse_external_engine(item, ext_dir, extension_file)?;
                result.push(contribution);
            }
            _ => {}
        }
    }
    Ok(result)
}

/// Parse one object element of `contributes.engines` into an `External` contribution.
fn parse_external_engine(
    item: &ConfigValue,
    ext_dir: &Path,
    extension_file: &Path,
) -> Result<EngineContribution> {
    // `path` is required
    let path_str = item.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
        crate::error::QuartoError::Other(
            "Engine entry is missing required 'path' field".to_string(),
        )
    })?;

    // Validate: path must end in a lowercase `.js` extension
    let raw_path = std::path::Path::new(path_str);
    let ext_lower = raw_path
        .extension()
        .map(|e| e.to_string_lossy().into_owned());

    if ext_lower.as_deref() != Some("js") {
        let ext_name = ext_dir.file_name().map_or_else(
            || "unknown".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        // Best-effort: replace extension with .js
        let expected_js_path = raw_path.with_extension("js").to_string_lossy().into_owned();
        return Err(crate::error::QuartoError::Other(format!(
            "Engine extension '{}' has 'path: {}'; only pre-built lowercase '.js' bundles are \
            loadable. Run 'q2 build-ts-extension' to produce {} and update _extension.yml.",
            ext_name, path_str, expected_js_path,
        )));
    }

    let resolved_path = ext_dir.join(path_str);

    // Optional: `name`
    let name = item
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Optional: `claims` — None if absent, Some(map) if present (including empty)
    let claims = item.get("claims").map(parse_claims_map);

    // Optional: `file-extensions` — None if absent, Some(vec) if present.
    // Each element is normalized to undotted lowercase at parse time (change B).
    let file_extensions = item.get("file-extensions").map(parse_normalized_ext_list);

    // Optional: `claims-files` — None if absent, Some(vec) if present. Accepts
    // bare-string shorthand AND `{extension: ...}` mapping form (change A);
    // each element's extension is normalized to undotted lowercase (change B).
    let claims_files = item.get("claims-files").map(parse_file_claims);

    Ok(EngineContribution::External {
        path: resolved_path,
        extension_yml_path: extension_file.to_path_buf(),
        name,
        claims,
        file_extensions,
        claims_files,
    })
}

/// Parse a `claims` map value into a `HashMap<String, Vec<StaticLanguageClaim>>`
/// (4c0 Vec-per-language form).
///
/// Returns an empty map for `claims: {}` and `claims: []`. The top-level value
/// may be:
/// - a YAML **sequence of strings** (shorthand) → each string `lang` gets a
///   1-element Vec of a default-priority Primary claim (mirrors §3.2's `true`
///   normalization, applied at the top level instead of per-language);
///   non-string elements are skipped;
/// - a per-language **map**, where a language key's value may be:
///   - a YAML sequence of claim objects → each element parsed via
///     `parse_static_language_claim`, collected into the Vec (elements that
///     parse to `None`, e.g. `false`/null, are dropped);
///   - a scalar/bool/int/single-object value (pre-4c0 shape) → parsed via the
///     same single-claim path and wrapped as a 1-element Vec (back-compat).
///
/// A key whose value produces zero claims (an empty sequence, or a
/// scalar/object that parses to `None`) is omitted from the map entirely —
/// matching the pre-4c0 "false/null → skip" behavior.
pub(crate) fn parse_claims_map(cv: &ConfigValue) -> HashMap<String, Vec<StaticLanguageClaim>> {
    if let ConfigValueKind::Array(items) = &cv.value {
        let mut map = HashMap::new();
        for item in items {
            // `as_plain_text()`, not `as_str()`: a bare YAML string in
            // document-frontmatter interpretation context is
            // `ConfigValueKind::PandocInlines`, not `Scalar(String)` (the
            // `metadata-as-str` lint's exact concern) — Plan 6 Phase 3 calls
            // this parser on merged document metadata, not just
            // `_extension.yml`, which never produces `PandocInlines` here.
            if let Some(lang) = item.as_plain_text() {
                map.insert(
                    lang,
                    vec![StaticLanguageClaim {
                        kind: ClaimKind::Primary,
                        priority: None,
                        when_class: None,
                    }],
                );
            }
        }
        return map;
    }
    let ConfigValueKind::Map(entries) = &cv.value else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for entry in entries {
        let claims = parse_static_language_claims(&entry.key, &entry.value);
        if !claims.is_empty() {
            map.insert(entry.key.clone(), claims);
        }
    }
    map
}

/// Parse one `claims` map entry's value into a `Vec<StaticLanguageClaim>`.
///
/// A YAML sequence parses each element via `parse_static_language_claim` and
/// collects the `Some` results (order-preserving; dropped `None` elements do
/// not shift the rest). Any other shape (scalar/object) delegates to the
/// single-claim parser and wraps a `Some` result as a 1-element Vec.
pub(crate) fn parse_static_language_claims(
    key: &str,
    cv: &ConfigValue,
) -> Vec<StaticLanguageClaim> {
    if let ConfigValueKind::Array(items) = &cv.value {
        return items
            .iter()
            .filter_map(|item| parse_static_language_claim(key, item))
            .collect();
    }
    parse_static_language_claim(key, cv).into_iter().collect()
}

/// Parse one entry in the `claims` map.
///
/// Rules (§3.2 of engine-resolution.md):
/// - `true`  → Primary, priority: None
/// - integer → Primary, priority: Some(n)
/// - `false` / null → skip (no entry)
/// - bare string `primary`/`interop`/`fallback` → that kind, priority: None
///   (concise sugar for `{ kind: <string> }`); any other string → skip
/// - object (key == "fallback") → `{ priority?: int }` with kind Fallback
/// - object (other key) → `{ kind: primary|interop|fallback, priority?: int, whenClass?: str }`
pub(crate) fn parse_static_language_claim(
    key: &str,
    cv: &ConfigValue,
) -> Option<StaticLanguageClaim> {
    match &cv.value {
        // false or null → skip
        ConfigValueKind::Scalar(yaml_rust2::Yaml::Boolean(false) | yaml_rust2::Yaml::Null) => None,
        // true → Primary, use default priority
        ConfigValueKind::Scalar(yaml_rust2::Yaml::Boolean(true)) => Some(StaticLanguageClaim {
            kind: ClaimKind::Primary,
            priority: None,
            when_class: None,
        }),
        // integer → Primary with explicit priority
        ConfigValueKind::Scalar(yaml_rust2::Yaml::Integer(n)) => Some(StaticLanguageClaim {
            kind: ClaimKind::Primary,
            priority: Some(*n as i32),
            when_class: None,
        }),
        // bare kind string → that kind, default priority (sugar for `{ kind: <string> }`)
        ConfigValueKind::Scalar(yaml_rust2::Yaml::String(s)) => {
            let kind = match s.as_str() {
                "primary" => ClaimKind::Primary,
                "interop" => ClaimKind::Interop,
                "fallback" => ClaimKind::Fallback,
                _ => return None,
            };
            Some(StaticLanguageClaim {
                kind,
                priority: None,
                when_class: None,
            })
        }
        // object
        ConfigValueKind::Map(_) => {
            if key == "fallback" {
                // fallback key: object form is `{ priority?: int }`, kind is implicit Fallback
                let priority = cv
                    .get("priority")
                    .and_then(|v| v.as_int())
                    .map(|n| n as i32);
                Some(StaticLanguageClaim {
                    kind: ClaimKind::Fallback,
                    priority,
                    when_class: None,
                })
            } else {
                // regular claim: { kind: primary|interop|fallback, priority?: int, whenClass?: str }
                let kind_str = cv.get("kind")?.as_str()?;
                let kind = match kind_str {
                    "primary" => ClaimKind::Primary,
                    "interop" => ClaimKind::Interop,
                    "fallback" => ClaimKind::Fallback,
                    _ => return None,
                };
                let priority = cv
                    .get("priority")
                    .and_then(|v| v.as_int())
                    .map(|n| n as i32);
                let when_class = cv
                    .get("whenClass")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Some(StaticLanguageClaim {
                    kind,
                    priority,
                    when_class,
                })
            }
        }
        _ => None,
    }
}

/// Normalize a declared extension to the canonical Rust-side form: strip a
/// single leading `.` if present, then lowercase. `".ECHO"` -> `"echo"`,
/// `"Echo"` -> `"echo"`, `""` -> `""`.
///
/// Extensions are undotted everywhere on the Rust side (change C); the wire
/// adapter (`to_wire_ext` in `engine/ts_engine.rs`) re-dots only at the
/// Rust -> TS seam.
fn normalize_ext(raw: &str) -> String {
    let stripped = raw.strip_prefix('.').unwrap_or(raw);
    stripped.to_lowercase()
}

/// Parse a YAML array of strings into a `Vec<String>`, normalizing each
/// element via `normalize_ext` (undotted, lowercase).
///
/// Non-string elements are silently skipped.
fn parse_normalized_ext_list(cv: &ConfigValue) -> Vec<String> {
    let ConfigValueKind::Array(items) = &cv.value else {
        return vec![];
    };
    items
        .iter()
        .filter_map(|item| item.as_str().map(normalize_ext))
        .collect()
}

/// Parse a `claims-files` array into `Vec<FileClaim>`.
///
/// Each element is either:
/// - a **scalar** string (`.echo`) -> `FileClaim { extension: normalize_ext(...) }`
/// - a **mapping** `{extension: ".echo"}` -> same, reading the `extension` key
///
/// Any other shape (or a mapping missing `extension`) is silently skipped,
/// mirroring `parse_normalized_ext_list`'s non-string-skip behavior.
fn parse_file_claims(cv: &ConfigValue) -> Vec<FileClaim> {
    let ConfigValueKind::Array(items) = &cv.value else {
        return vec![];
    };
    items
        .iter()
        .filter_map(|item| match &item.value {
            ConfigValueKind::Scalar(_) | ConfigValueKind::PandocInlines(_) => {
                item.as_str().map(|s| FileClaim {
                    extension: normalize_ext(s),
                })
            }
            ConfigValueKind::Map(_) => {
                item.get("extension")
                    .and_then(|v| v.as_str())
                    .map(|s| FileClaim {
                        extension: normalize_ext(s),
                    })
            }
            _ => None,
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

    /// Q1 does not require `title` (`schema/extension.yml` has no top-level
    /// `required:` marker; `readExtension` reads it leniently and falls back to
    /// `extension.title || extension.id.name`). A manifest omitting `title` must
    /// therefore load, with the title defaulting to the extension's id name.
    ///
    /// Named revert (H-title, read.rs ~L82): restoring the `.ok_or_else(...)?`
    /// requiredness makes this `.unwrap()` panic (missing-title error). The
    /// `== "test-ext"` discriminator also reddens under a lazy
    /// `unwrap_or_default()` (empty string ≠ id name), pinning the id-name default.
    #[test]
    fn test_read_extension_missing_title_defaults_to_id_name() {
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

        // Title defaults to the extension id name (Q1's `title || id.name`).
        assert_eq!(ext.id.name, "test-ext");
        assert_eq!(ext.title, "test-ext");
        // `author` was present, so it round-trips.
        assert_eq!(ext.author.as_deref(), Some("Author"));
        // Contribution is actually carried (not just a parsed header).
        assert_eq!(ext.contributes.shortcodes.len(), 1);
    }

    /// Q1 does not require `author` (no schema `required:`; a real shipped Q1
    /// extension — julia-engine — has none). A manifest omitting `author` must
    /// load, with `author` left `None`.
    ///
    /// Named revert (H-author, read.rs ~L93): restoring `.ok_or_else(...)?`
    /// requiredness makes this `.unwrap()` panic (missing-author error). `title`
    /// is present here, so H-title's revert does not affect this test (clean
    /// isolation of the author relaxation).
    #[test]
    fn test_read_extension_author_optional() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/test-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Author-less Extension
contributes:
  shortcodes:
    - hello.lua
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        assert_eq!(ext.title, "Author-less Extension");
        assert!(ext.author.is_none(), "author should be None when omitted");
        // Contribution is actually carried.
        assert_eq!(ext.contributes.shortcodes.len(), 1);
    }

    /// Reproduces the exact confirmed failure shape: a Q1 engine extension
    /// (modeled on julia-engine) — `title` + `version` + `quarto-required` +
    /// `contributes.engines`, and **no `author`**. Before the relaxation this
    /// was rejected outright and the engine never registered; now it must load.
    ///
    /// Named revert (H-author): restoring author requiredness reddens this.
    #[test]
    fn test_read_extension_q1_engine_shape() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/julia-engine");
        let file = write_extension(
            &ext_dir,
            r#"
title: Julia Engine
version: 0.5.0
quarto-required: ">=1.4.0"
contributes:
  engines:
    - path: julia-engine.js
"#,
        );

        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        assert_eq!(ext.title, "Julia Engine");
        assert!(ext.author.is_none(), "author should be None when omitted");
        assert_eq!(ext.version.as_deref(), Some("0.5.0"));
        assert_eq!(ext.contributes.engines.len(), 1);
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

    // --- Engine parsing tests (P1-8, P1-9, happy path, None vs Some(empty), shorthand) ---

    /// P1-8: a `.ts` path is rejected with an actionable message naming
    /// `q2 build-ts-extension`. (RED if the .js validation is removed.)
    #[test]
    fn test_engine_ts_path_rejected() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: src/engine.ts
"#,
        );
        let runtime = make_runtime();
        let err = read_extension(&file, &runtime).unwrap_err();
        assert!(
            err.to_string().contains("build-ts-extension"),
            "Error should mention 'build-ts-extension': {}",
            err
        );
    }

    /// P1-9a: uppercase `.JS` extension is rejected. (Shares P1-8 revert hunk.)
    #[test]
    fn test_engine_uppercase_js_rejected() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: bundle.JS
"#,
        );
        let runtime = make_runtime();
        let err = read_extension(&file, &runtime).unwrap_err();
        assert!(
            err.to_string().contains("build-ts-extension"),
            "Error should mention 'build-ts-extension': {}",
            err
        );
    }

    /// P1-9b: `.mjs` extension is rejected. (Shares P1-8 revert hunk.)
    #[test]
    fn test_engine_mjs_path_rejected() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: bundle.mjs
"#,
        );
        let runtime = make_runtime();
        let err = read_extension(&file, &runtime).unwrap_err();
        assert!(
            err.to_string().contains("build-ts-extension"),
            "Error should mention 'build-ts-extension': {}",
            err
        );
    }

    /// Happy parse: full External engine + a bare Reorder string alongside it.
    #[test]
    fn test_engine_external_happy_parse() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      name: echo
      claims:
        echo:
          kind: primary
          priority: 1
      file-extensions:
        - ".echo"
      claims-files:
        - ".echo"
    - jupyter
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        assert_eq!(ext.contributes.engines.len(), 2);

        match &ext.contributes.engines[0] {
            EngineContribution::External {
                path,
                extension_yml_path,
                name,
                claims,
                file_extensions,
                claims_files,
            } => {
                assert!(path.is_absolute(), "path should be absolute");
                assert!(
                    path.to_string_lossy().ends_with(".js"),
                    "path should end with .js"
                );
                assert_eq!(
                    extension_yml_path, &file,
                    "extension_yml_path must be the _extension.yml this engine was parsed from"
                );
                assert_eq!(name.as_deref(), Some("echo"));
                let claims = claims.as_ref().unwrap();
                let echo_claims = claims.get("echo").unwrap();
                assert_eq!(
                    echo_claims.len(),
                    1,
                    "single object claim value must parse to a 1-element Vec"
                );
                let echo_claim = &echo_claims[0];
                assert_eq!(echo_claim.kind, ClaimKind::Primary);
                assert_eq!(echo_claim.priority, Some(1));
                assert!(echo_claim.when_class.is_none());
                assert_eq!(
                    file_extensions.as_deref(),
                    Some(&["echo".to_string()][..]),
                    "file-extensions must normalize to undotted lowercase at parse time"
                );
                assert_eq!(
                    claims_files.as_deref(),
                    Some(
                        &[FileClaim {
                            extension: "echo".to_string()
                        }][..]
                    ),
                    "claims-files bare-string shorthand must normalize to undotted lowercase"
                );
            }
            other => panic!("expected External, got {:?}", other),
        }

        match &ext.contributes.engines[1] {
            EngineContribution::Reorder { name } => assert_eq!(name, "jupyter"),
            other => panic!("expected Reorder, got {:?}", other),
        }
    }

    /// T9 (1c.2 Task 1, changes A+B): `claims-files` parses BOTH the
    /// bare-string shorthand (`.Echo`) AND the `{extension: ...}` mapping
    /// form, and BOTH axes (`file-extensions`, `claims-files`) normalize to
    /// undotted lowercase at parse time.
    ///
    /// REDs (manually verified — see task report):
    /// 1. Revert `normalize_ext` to a no-op (identity fn) → stored values are
    ///    `".ECHO"` / `".Echo"`, which != `"echo"` → assertions fail.
    /// 2. Revert the `ConfigValueKind::Map` arm out of `parse_file_claims`
    ///    (drop scalar-or-mapping accept) → the object-form element parses to
    ///    `None` and is dropped → `claims.len()` is 1, not 2 → assertion fails.
    #[test]
    fn t9_claims_files_bare_and_object_forms_normalize_undotted_lowercase() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      file-extensions:
        - ".ECHO"
      claims-files:
        - ".Echo"
        - extension: ".Echo"
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External {
                file_extensions,
                claims_files,
                ..
            } => {
                assert_eq!(
                    file_extensions.as_deref(),
                    Some(&["echo".to_string()][..]),
                    "file-extensions must normalize undotted lowercase"
                );
                let claims = claims_files.as_ref().expect("claims-files must be Some");
                assert_eq!(
                    claims.len(),
                    2,
                    "both bare-string and object-map forms must parse to one entry each; got {:?}",
                    claims
                );
                assert_eq!(
                    claims[0],
                    FileClaim {
                        extension: "echo".to_string()
                    },
                    "bare-string form must normalize undotted lowercase"
                );
                assert_eq!(
                    claims[1],
                    FileClaim {
                        extension: "echo".to_string()
                    },
                    "object-map form must normalize undotted lowercase"
                );
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// Field-absent → `None`; present-but-empty → `Some(empty)`.
    #[test]
    fn test_engine_claims_present_but_empty_is_some() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      claims: {}
      file-extensions: []
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External {
                claims,
                file_extensions,
                ..
            } => {
                assert!(
                    claims.is_some(),
                    "present-but-empty claims should be Some(empty), not None"
                );
                assert!(claims.as_ref().unwrap().is_empty());
                assert!(
                    file_extensions.is_some(),
                    "present-but-empty file-extensions should be Some(empty), not None"
                );
                assert!(file_extensions.as_ref().unwrap().is_empty());
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// Absent `claims` and `file-extensions` fields → `None`.
    #[test]
    fn test_engine_absent_optional_fields_are_none() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External {
                name,
                claims,
                file_extensions,
                claims_files,
                ..
            } => {
                assert!(name.is_none(), "absent name should be None");
                assert!(claims.is_none(), "absent claims should be None");
                assert!(
                    file_extensions.is_none(),
                    "absent file-extensions should be None"
                );
                assert!(claims_files.is_none(), "absent claims-files should be None");
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// Shorthand claim values: `true` → Primary(None), number → Primary(Some(n)),
    /// `fallback: { priority: 0 }` → Fallback entry. Each still parses to a
    /// 1-element Vec (4c0 back-compat for the scalar/single-object shape).
    #[test]
    fn test_engine_claims_shorthand_forms() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      claims:
        echo: true
        fast: 3
        fallback:
          priority: 0
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External { claims, .. } => {
                let claims = claims.as_ref().unwrap();

                let echo_claims = claims.get("echo").unwrap();
                assert_eq!(
                    echo_claims.len(),
                    1,
                    "scalar `true` must parse to a 1-element Vec"
                );
                assert_eq!(echo_claims[0].kind, ClaimKind::Primary);
                assert_eq!(echo_claims[0].priority, None);

                let fast_claims = claims.get("fast").unwrap();
                assert_eq!(
                    fast_claims.len(),
                    1,
                    "scalar integer must parse to a 1-element Vec"
                );
                assert_eq!(fast_claims[0].kind, ClaimKind::Primary);
                assert_eq!(fast_claims[0].priority, Some(3));

                let fallback_claims = claims.get("fallback").unwrap();
                assert_eq!(
                    fallback_claims.len(),
                    1,
                    "single fallback object must parse to a 1-element Vec"
                );
                assert_eq!(fallback_claims[0].kind, ClaimKind::Fallback);
                assert_eq!(fallback_claims[0].priority, Some(0));
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// SC2: a YAML **sequence** value for a `claims` key parses to a
    /// multi-element Vec — one `StaticLanguageClaim` per sequence element,
    /// preserving each element's kind/priority/whenClass (the marimo
    /// bare-`{sql}` shape: a whenClass-conditioned primary claim alongside
    /// an unconditional interop claim, both under the `sql` key).
    #[test]
    fn sc2_engine_claims_sequence_form_parses_multi_element_vec() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      claims:
        sql:
          - kind: primary
            priority: 2
            whenClass: marimo
          - kind: interop
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External { claims, .. } => {
                let claims = claims.as_ref().unwrap();
                let sql = claims.get("sql").unwrap();
                assert_eq!(
                    sql.len(),
                    2,
                    "a 2-element YAML sequence must parse to a 2-element Vec"
                );
                assert_eq!(sql[0].kind, ClaimKind::Primary);
                assert_eq!(sql[0].priority, Some(2));
                assert_eq!(sql[0].when_class.as_deref(), Some("marimo"));
                assert_eq!(sql[1].kind, ClaimKind::Interop);
                assert_eq!(sql[1].priority, None);
                assert!(sql[1].when_class.is_none());
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// Top-level `claims: [r, sql]` shorthand: each language string maps to a
    /// 1-element Vec of a default-priority Primary claim (mirrors §3.2's
    /// `true` normalization, applied at the top level instead of per-language).
    #[test]
    fn parse_claims_list_shorthand_primary_default() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      claims: [r, sql]
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External { claims, .. } => {
                let claims = claims.as_ref().unwrap();
                assert_eq!(claims.len(), 2, "both languages must be present");

                let r_claims = claims.get("r").unwrap();
                assert_eq!(r_claims.len(), 1);
                assert_eq!(r_claims[0].kind, ClaimKind::Primary);
                assert_eq!(r_claims[0].priority, None);
                assert!(r_claims[0].when_class.is_none());

                let sql_claims = claims.get("sql").unwrap();
                assert_eq!(sql_claims.len(), 1);
                assert_eq!(sql_claims[0].kind, ClaimKind::Primary);
                assert_eq!(sql_claims[0].priority, None);
                assert!(sql_claims[0].when_class.is_none());
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// A non-string element in the top-level list shorthand is skipped
    /// (same lenient behavior the map parser uses for unparseable entries);
    /// the remaining string element still parses.
    #[test]
    fn parse_claims_list_shorthand_skips_non_string() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      claims: [r, 3]
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External { claims, .. } => {
                let claims = claims.as_ref().unwrap();
                assert_eq!(claims.len(), 1, "the non-string entry must be skipped");
                assert!(claims.get("r").is_some());
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// The top-level string-list shorthand must NOT be confused with 4c0's
    /// *per-language* claim-object sequence: `claims: {sql: [...]}` still
    /// dispatches through `parse_static_language_claims` exactly as before.
    #[test]
    fn parse_claims_list_shorthand_distinct_from_4c0_form() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      claims:
        sql:
          - kind: primary
          - kind: interop
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External { claims, .. } => {
                let claims = claims.as_ref().unwrap();
                let sql = claims.get("sql").unwrap();
                assert_eq!(
                    sql.len(),
                    2,
                    "per-language claim-object sequence must still be a 2-element Vec"
                );
                assert_eq!(sql[0].kind, ClaimKind::Primary);
                assert_eq!(sql[1].kind, ClaimKind::Interop);
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// Both an empty map (`{}`) and an empty list (`[]`) for `claims` parse
    /// to an empty map — no error, no panic. Present-but-empty stays
    /// distinguishable from absent at the caller (`Some(empty)`), which
    /// `test_engine_claims_present_but_empty_is_some` already covers for the
    /// `{}` shape; this test adds the `[]` shape.
    #[test]
    fn parse_claims_empty_table_yields_empty_map() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      claims: []
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External { claims, .. } => {
                assert!(
                    claims.is_some(),
                    "present-but-empty claims should be Some(empty), not None"
                );
                assert!(claims.as_ref().unwrap().is_empty());
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// Per-language MAP-VALUE claim sugar: a bare kind string (`{r: primary}`)
    /// is concise sugar for `{r: {kind: primary}}` — same three kinds as the
    /// object form's `kind` field, default priority/when_class. Do not
    /// confuse this with the *top-level* list shorthand (`claims: [r]`,
    /// tested above) — that's sugar one level up; this is sugar for a single
    /// per-language map entry's value. An unrecognized bare string (`banana`)
    /// stays lenient and is dropped, same as any other unparseable claim
    /// value.
    #[test]
    fn parse_claims_map_bare_kind_string() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Extension
author: Author
contributes:
  engines:
    - path: engine.js
      claims:
        r: primary
        s: interop
        f: fallback
        x: banana
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        match &ext.contributes.engines[0] {
            EngineContribution::External { claims, .. } => {
                let claims = claims.as_ref().unwrap();

                let r_claims = claims.get("r").unwrap();
                assert_eq!(r_claims.len(), 1);
                assert_eq!(r_claims[0].kind, ClaimKind::Primary);
                assert_eq!(r_claims[0].priority, None);
                assert!(r_claims[0].when_class.is_none());

                let s_claims = claims.get("s").unwrap();
                assert_eq!(s_claims.len(), 1);
                assert_eq!(s_claims[0].kind, ClaimKind::Interop);
                assert_eq!(s_claims[0].priority, None);
                assert!(s_claims[0].when_class.is_none());

                let f_claims = claims.get("f").unwrap();
                assert_eq!(f_claims.len(), 1);
                assert_eq!(f_claims[0].kind, ClaimKind::Fallback);
                assert_eq!(f_claims[0].priority, None);
                assert!(f_claims[0].when_class.is_none());

                assert!(
                    claims.get("x").is_none(),
                    "unrecognized bare kind string must be dropped, not panic"
                );
            }
            other => panic!("expected External, got {:?}", other),
        }
    }

    /// Regression (found while wiring Plan 6 Phase 3's claim-table reader,
    /// which calls `parse_claims_map` on merged DOCUMENT metadata, not just
    /// `_extension.yml`): the top-level list-shorthand branch used
    /// `item.as_str()`, which returns `None` for `ConfigValueKind::PandocInlines`
    /// — the shape a bare YAML string takes in document-frontmatter
    /// interpretation context (the `metadata-as-str` lint's exact concern).
    /// `_extension.yml`'s own interpretation context never produces
    /// `PandocInlines`, so this silently never fired for the ORIGINAL caller
    /// — only for Phase 3's new document-metadata call site. Constructs the
    /// `PandocInlines` shape directly (bypassing the YAML pipeline) to
    /// isolate the parser from the interpretation-context question.
    ///
    /// Revert binding: `item.as_str()` → `item.as_plain_text()` in the
    /// array branch — reverting it makes both languages vanish (RED).
    #[test]
    fn parse_claims_list_shorthand_accepts_pandoc_inlines() {
        use quarto_pandoc_types::{Inline, Str};
        use quarto_source_map::SourceInfo;

        fn inline_str(s: &str) -> ConfigValue {
            ConfigValue::new_inlines(
                vec![Inline::Str(Str {
                    text: s.to_string(),
                    source_info: SourceInfo::for_test(),
                })],
                SourceInfo::for_test(),
            )
        }

        let claims_value = ConfigValue::new_array(
            vec![inline_str("r"), inline_str("sql")],
            SourceInfo::for_test(),
        );

        let claims = parse_claims_map(&claims_value);

        assert_eq!(
            claims.len(),
            2,
            "both PandocInlines-shaped language names must parse; got: {:?}",
            claims
        );
        assert_eq!(claims["r"][0].kind, ClaimKind::Primary);
        assert_eq!(claims["sql"][0].kind, ClaimKind::Primary);
    }

    /// An extension that contributes ONLY engines (no formats/filters/etc.) is valid.
    /// (RED if the `&& result.engines.is_empty()` conjunct is missing.)
    #[test]
    fn test_engines_only_extension_is_valid() {
        let tmp = TempDir::new().unwrap();
        let ext_dir = tmp.path().join("_extensions/my-ext");
        let file = write_extension(
            &ext_dir,
            r#"
title: Engine Only Extension
author: Author
contributes:
  engines:
    - path: engine.js
"#,
        );
        let runtime = make_runtime();
        let ext = read_extension(&file, &runtime).unwrap();

        assert_eq!(ext.contributes.engines.len(), 1);
        assert!(ext.contributes.formats.is_empty());
        assert!(ext.contributes.filters.is_empty());
        assert!(ext.contributes.shortcodes.is_empty());
        assert!(ext.contributes.metadata.is_none());
        assert!(ext.contributes.project.is_none());
    }
}

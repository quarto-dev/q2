/*
 * lua/quarto_doc.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Implements the `quarto.doc` Lua namespace:
 * - quarto.doc.is_format(fmt) / quarto.doc.isFormat(fmt)
 * - quarto.doc.has_bootstrap()  / quarto.doc.hasBootstrap()
 * - quarto.doc.add_html_dependency(dep) / quarto.doc.addHtmlDependency(dep)
 * - quarto.doc.include_text(location, text) / quarto.doc.includeText(location, text)
 *
 * Also provides extraction functions to read back collected dependencies
 * and text includes from the Lua state after execution.
 */

use std::path::PathBuf;

use mlua::{Lua, Result, Table, Value};

use super::quarto_api::current_script_dir;

// =========================================================================
// Public types for extraction
// =========================================================================

/// An HTML dependency registered by an extension via `quarto.doc.add_html_dependency`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HtmlDependency {
    pub name: String,
    /// Optional version, as declared by `quarto.doc.add_html_dependency`.
    ///
    /// Carried into the artifact path and key so two renders that produce
    /// different versions of one dependency do not collapse onto the same
    /// asset — the requirement `freeze` will depend on. See
    /// `claude-notes/plans/2026-08-14-add-html-dependency-version.md`.
    pub version: Option<String>,
    pub stylesheets: Vec<PathBuf>,
    pub scripts: Vec<PathBuf>,
}

/// A text include registered by an extension via `quarto.doc.include_text`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextInclude {
    pub location: IncludeLocation,
    pub content: String,
}

/// Where to inject text in the output document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IncludeLocation {
    InHeader,
    BeforeBody,
    AfterBody,
}

// =========================================================================
// Known fields for add_html_dependency validation
// =========================================================================

/// Fields we support.
const SUPPORTED_FIELDS: &[&str] = &["name", "version", "scripts", "stylesheets"];

/// Fields that TS Quarto supports but we don't yet.
const UNSUPPORTED_FIELDS: &[&str] = &["meta", "links", "resources", "serviceworkers", "head"];

// =========================================================================
// Format alias matching (matching TS Quarto's _format.lua)
// =========================================================================

/// Check if FORMAT matches a query using TS Quarto's alias-based matching.
/// `format` is the current FORMAT global value, `query` is what the extension asked for.
/// Public: quarto-core's conditional-content transform reuses this for
/// `when-format` / `unless-format` (bd-fu16z22k) so Lua's
/// `quarto.doc.is_format` and the attribute syntax can never disagree.
pub fn is_format_match(format: &str, query: &str) -> bool {
    // Exact match
    if format == query {
        return true;
    }

    match query {
        "html" => is_html_output(format),
        "html:js" => is_html_output(format) && !is_epub_output(format),
        "latex" | "pdf" => is_latex_output(format),
        "epub" => is_epub_output(format),
        "markdown" => is_markdown_output(format),
        "asciidoc" | "asciidoctor" => is_asciidoc_output(format),
        _ => false,
    }
}

fn is_html_output(format: &str) -> bool {
    matches!(
        format,
        "html"
            | "html4"
            | "html5"
            | "epub"
            | "epub2"
            | "epub3"
            | "revealjs"
            | "s5"
            | "slidy"
            | "slideous"
            | "dzslides"
    )
}

fn is_epub_output(format: &str) -> bool {
    format.starts_with("epub")
}

fn is_latex_output(format: &str) -> bool {
    matches!(format, "latex" | "beamer" | "pdf")
}

fn is_markdown_output(format: &str) -> bool {
    matches!(
        format,
        "markdown" | "markdown_github" | "gfm" | "commonmark" | "commonmark_x" | "markua"
    )
}

fn is_asciidoc_output(format: &str) -> bool {
    matches!(format, "asciidoc" | "asciidoctor")
}

// =========================================================================
// Path resolution for dependencies
// =========================================================================

/// Resolve a dependency file path against the current script directory.
fn resolve_dep_path(lua: &Lua, path: &str) -> Result<PathBuf> {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return Ok(PathBuf::from(path));
    }
    let script_dir = current_script_dir(lua)?;
    if script_dir.is_empty() {
        return Ok(PathBuf::from(path));
    }
    Ok(PathBuf::from(&script_dir).join(path))
}

/// Normalize a dependency file entry: either a bare string or a {name, path} table.
/// Returns the resolved absolute path.
fn normalize_dep_file(lua: &Lua, value: &Value) -> Result<PathBuf> {
    match value {
        Value::String(s) => resolve_dep_path(lua, &s.to_str()?),
        Value::Table(t) => {
            let path: String = t.get("path")?;
            resolve_dep_path(lua, &path)
        }
        _ => Err(mlua::Error::runtime(
            "dependency file entry must be a string or table with 'path' field",
        )),
    }
}

/// Read an optional array of dependency files from a table field.
fn read_dep_files(lua: &Lua, dep: &Table, field: &str) -> Result<Vec<PathBuf>> {
    let value: Value = dep.get(field)?;
    match value {
        Value::Nil => Ok(Vec::new()),
        Value::Table(arr) => {
            let mut paths = Vec::new();
            for i in 1..=arr.raw_len() {
                let entry: Value = arr.get(i)?;
                paths.push(normalize_dep_file(lua, &entry)?);
            }
            Ok(paths)
        }
        _ => Err(mlua::Error::runtime(format!(
            "add_html_dependency: '{}' must be an array",
            field
        ))),
    }
}

// =========================================================================
// Registration
// =========================================================================

/// Register the `quarto.doc` namespace on the existing `quarto` global.
///
/// Must be called after `register_quarto_api()` (which creates the `quarto` global
/// with json/log/utils sub-namespaces).
pub fn register_quarto_doc(lua: &Lua) -> Result<()> {
    let quarto: Table = lua.globals().get("quarto")?;
    let doc = lua.create_table()?;

    // Internal storage tables
    doc.set("_dependencies", lua.create_table()?)?;
    doc.set("_text_includes", lua.create_table()?)?;
    // bd-o8pr Phase 3: Lua-filter-declared resources. Filters call
    // `quarto.doc.add_resource(path)` and entries are drained back
    // out via `extract_resources(&lua)` after each filter pass.
    doc.set("_resources", lua.create_table()?)?;

    // quarto.doc.is_format(fmt) — alias-based format matching
    let is_format_fn = lua.create_function(|lua, fmt: String| {
        let format: String = lua.globals().get("FORMAT")?;
        Ok(is_format_match(&format, &fmt))
    })?;
    doc.set("is_format", is_format_fn.clone())?;
    doc.set("isFormat", is_format_fn)?;

    // quarto.doc.has_bootstrap() — true for HTML (not epub)
    doc.set(
        "has_bootstrap",
        lua.create_function(|lua, ()| {
            let format: String = lua.globals().get("FORMAT")?;
            Ok(is_html_output(&format) && !is_epub_output(&format))
        })?,
    )?;
    let has_bootstrap_fn: mlua::Function = doc.get("has_bootstrap")?;
    doc.set("hasBootstrap", has_bootstrap_fn)?;

    // quarto.doc.add_html_dependency(dep)
    doc.set(
        "add_html_dependency",
        lua.create_function(|lua, dep: Table| {
            // Validate name
            let name: String = dep.get::<Value>("name").and_then(|v| match v {
                Value::String(s) => Ok(s.to_str()?.to_string()),
                _ => Err(mlua::Error::runtime(
                    "HTML dependencies must include a name",
                )),
            })?;

            let version: Option<String> = dep.get("version")?;

            // Reject unknown fields. This runs on EVERY call, including one
            // whose name is already registered: a misspelled field is a bug in
            // the extension regardless of whether this particular call would
            // have registered anything. Only the *warning* below may be
            // deferred past the dedup early-return
            // (bd-add-html-dependency-version-5tnub5ds).
            for pair in dep.pairs::<String, Value>() {
                let (key, _) = pair?;
                if !SUPPORTED_FIELDS.contains(&key.as_str())
                    && !UNSUPPORTED_FIELDS.contains(&key.as_str())
                {
                    return Err(mlua::Error::runtime(format!(
                        "add_html_dependency: unknown field '{}'",
                        key
                    )));
                }
            }

            let quarto: Table = lua.globals().get("quarto")?;
            let warn_fn: mlua::Function = quarto.get("warn")?;
            let doc_table: Table = quarto.get("doc")?;
            let deps: Table = doc_table.get("_dependencies")?;

            // Deduplicate by name — deliberately NOT by (name, version).
            // Extensions are written to call this once per matching element,
            // so the repeat call is the normal case and must stay silent.
            // Registering the same name at a *different* version within one
            // document would inject two copies of one library into a single
            // page, which is a mistake worth reporting; cross-render
            // coexistence (what `freeze` needs) is handled downstream by the
            // versioned artifact key, not here.
            for i in 1..=deps.raw_len() {
                let existing: Table = deps.get(i)?;
                let existing_name: String = existing.get("name")?;
                if existing_name == name {
                    let existing_version: Option<String> = existing.get("version")?;
                    if existing_version != version {
                        let describe = |v: &Option<String>| match v {
                            Some(v) => format!("version '{}'", v),
                            None => "no version".to_string(),
                        };
                        warn_fn.call::<()>(format!(
                            "add_html_dependency: dependency '{}' is already registered with {}; \
                             ignoring this registration with {}. Only the first registration in a \
                             document is used.",
                            name,
                            describe(&existing_version),
                            describe(&version)
                        ))?;
                    }
                    return Ok(()); // Already registered
                }
            }

            // Warn about fields we don't implement yet. After the dedup
            // early-return, so this fires once per distinct dependency rather
            // than once per call.
            for pair in dep.pairs::<String, Value>() {
                let (key, _) = pair?;
                if UNSUPPORTED_FIELDS.contains(&key.as_str()) {
                    warn_fn.call::<()>(format!(
                        "add_html_dependency: field '{}' is not yet supported and will be ignored",
                        key
                    ))?;
                }
            }

            // Read and resolve file paths
            let stylesheets = read_dep_files(lua, &dep, "stylesheets")?;
            let scripts = read_dep_files(lua, &dep, "scripts")?;

            // Store as a Lua table
            let entry = lua.create_table()?;
            entry.set("name", name)?;
            entry.set("version", version)?;

            let ss_table = lua.create_table()?;
            for (i, path) in stylesheets.iter().enumerate() {
                ss_table.set(i + 1, path.to_string_lossy().to_string())?;
            }
            entry.set("stylesheets", ss_table)?;

            let sc_table = lua.create_table()?;
            for (i, path) in scripts.iter().enumerate() {
                sc_table.set(i + 1, path.to_string_lossy().to_string())?;
            }
            entry.set("scripts", sc_table)?;

            let len = deps.raw_len();
            deps.set(len + 1, entry)?;

            Ok(())
        })?,
    )?;
    let add_dep_fn: mlua::Function = doc.get("add_html_dependency")?;
    doc.set("addHtmlDependency", add_dep_fn)?;

    // quarto.doc.include_text(location, text)
    doc.set(
        "include_text",
        lua.create_function(|lua, (location, text): (String, String)| {
            match location.as_str() {
                "in-header" | "before-body" | "after-body" => {}
                _ => {
                    return Err(mlua::Error::runtime(format!(
                        "include_text: invalid location '{}', must be 'in-header', 'before-body', or 'after-body'",
                        location
                    )));
                }
            }

            let quarto: Table = lua.globals().get("quarto")?;
            let doc_table: Table = quarto.get("doc")?;
            let includes: Table = doc_table.get("_text_includes")?;

            let entry = lua.create_table()?;
            entry.set("location", location)?;
            entry.set("text", text)?;

            let len = includes.raw_len();
            includes.set(len + 1, entry)?;

            Ok(())
        })?,
    )?;
    let include_text_fn: mlua::Function = doc.get("include_text")?;
    doc.set("includeText", include_text_fn)?;

    // quarto.doc.add_resource(path) — bd-o8pr Phase 3.
    //
    // Append `path` to the per-document resource report. The path is
    // interpreted relative to the document being processed (Q1
    // parity). The orchestrator drains the report after the filter
    // pass and copies the file into the output dir.
    //
    // Calls are append-only by design: there is no remove_resource.
    // A filter that mutates `meta.resources` to remove an entry is
    // still defended against by the additivity-defense in the
    // ResourceReportStage (re-reads meta.resources, unions with the
    // profile snapshot).
    doc.set(
        "add_resource",
        lua.create_function(|lua, path: String| {
            if path.is_empty() {
                return Err(mlua::Error::runtime(
                    "add_resource: path must be a non-empty string",
                ));
            }
            let quarto: Table = lua.globals().get("quarto")?;
            let doc_table: Table = quarto.get("doc")?;
            let resources: Table = doc_table.get("_resources")?;
            let len = resources.raw_len();
            resources.set(len + 1, path)?;
            Ok(())
        })?,
    )?;
    let add_resource_fn: mlua::Function = doc.get("add_resource")?;
    doc.set("addResource", add_resource_fn)?;

    quarto.set("doc", doc)?;
    Ok(())
}

// =========================================================================
// Extraction functions
// =========================================================================

/// Extract HTML dependencies collected during Lua execution.
pub fn extract_html_dependencies(lua: &Lua) -> Result<Vec<HtmlDependency>> {
    let quarto: Table = lua.globals().get("quarto")?;
    let doc: Table = quarto.get("doc")?;
    let deps: Table = doc.get("_dependencies")?;

    let mut result = Vec::new();
    for i in 1..=deps.raw_len() {
        let entry: Table = deps.get(i)?;
        let name: String = entry.get("name")?;
        let version: Option<String> = entry.get("version")?;

        let ss_table: Table = entry.get("stylesheets")?;
        let mut stylesheets = Vec::new();
        for j in 1..=ss_table.raw_len() {
            let path: String = ss_table.get(j)?;
            stylesheets.push(PathBuf::from(path));
        }

        let sc_table: Table = entry.get("scripts")?;
        let mut scripts = Vec::new();
        for j in 1..=sc_table.raw_len() {
            let path: String = sc_table.get(j)?;
            scripts.push(PathBuf::from(path));
        }

        result.push(HtmlDependency {
            name,
            version,
            stylesheets,
            scripts,
        });
    }

    Ok(result)
}

/// Extract resource paths collected during Lua execution
/// (`bd-o8pr` Phase 3).
///
/// Returns the raw path strings the filter passed to
/// `quarto.doc.add_resource(...)`. Resolution against the project
/// root happens upstream in
/// `quarto_core::project_resources::resolve_reported_resources`.
pub fn extract_resources(lua: &Lua) -> Result<Vec<PathBuf>> {
    let quarto: Table = lua.globals().get("quarto")?;
    let doc: Table = quarto.get("doc")?;
    let resources: Table = doc.get("_resources")?;

    let mut result = Vec::new();
    for i in 1..=resources.raw_len() {
        let path: String = resources.get(i)?;
        result.push(PathBuf::from(path));
    }
    Ok(result)
}

/// Extract text includes collected during Lua execution.
pub fn extract_text_includes(lua: &Lua) -> Result<Vec<TextInclude>> {
    let quarto: Table = lua.globals().get("quarto")?;
    let doc: Table = quarto.get("doc")?;
    let includes: Table = doc.get("_text_includes")?;

    let mut result = Vec::new();
    for i in 1..=includes.raw_len() {
        let entry: Table = includes.get(i)?;
        let location_str: String = entry.get("location")?;
        let content: String = entry.get("text")?;

        let location = match location_str.as_str() {
            "in-header" => IncludeLocation::InHeader,
            "before-body" => IncludeLocation::BeforeBody,
            "after-body" => IncludeLocation::AfterBody,
            _ => {
                return Err(mlua::Error::runtime(format!(
                    "invalid include location: {}",
                    location_str
                )));
            }
        };

        result.push(TextInclude { location, content });
    }

    Ok(result)
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::constructors::register_pandoc_namespace;
    use crate::lua::mediabag::create_shared_mediabag;
    use crate::lua::quarto_api::{push_script_dir, register_quarto_api};
    use std::sync::Arc;

    fn create_test_lua() -> Lua {
        let lua = Lua::new();
        let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
        register_pandoc_namespace(&lua, runtime, create_shared_mediabag()).unwrap();
        register_quarto_api(&lua).unwrap();
        register_quarto_doc(&lua).unwrap();
        lua
    }

    fn create_test_lua_with_format(format: &str) -> Lua {
        let lua = create_test_lua();
        lua.globals().set("FORMAT", format).unwrap();
        lua
    }

    /// Messages accumulated in `quarto._diagnostics` by `quarto.warn` /
    /// `quarto.error`. Used to assert *how many times* a diagnostic fired,
    /// which is the whole point of
    /// `bd-add-html-dependency-version-5tnub5ds`.
    fn diagnostic_messages(lua: &Lua) -> Vec<String> {
        let quarto: Table = lua.globals().get("quarto").unwrap();
        let diagnostics: Table = quarto.get("_diagnostics").unwrap();
        (1..=diagnostics.raw_len())
            .map(|i| {
                let entry: Table = diagnostics.get(i).unwrap();
                entry.get::<String>("message").unwrap()
            })
            .collect()
    }

    // =====================================================================
    // is_format tests
    // =====================================================================

    #[test]
    fn test_is_format_exact_match() {
        let lua = create_test_lua_with_format("html");
        let result: bool = lua
            .load(r#"return quarto.doc.is_format("html")"#)
            .eval()
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_is_format_html_matches_html5() {
        let lua = create_test_lua_with_format("html5");
        let result: bool = lua
            .load(r#"return quarto.doc.is_format("html")"#)
            .eval()
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_is_format_html_js_true_for_html() {
        let lua = create_test_lua_with_format("html");
        let result: bool = lua
            .load(r#"return quarto.doc.is_format("html:js")"#)
            .eval()
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_is_format_html_js_false_for_epub3() {
        let lua = create_test_lua_with_format("epub3");
        let result: bool = lua
            .load(r#"return quarto.doc.is_format("html:js")"#)
            .eval()
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_is_format_latex_matches_pdf() {
        let lua = create_test_lua_with_format("pdf");
        let result: bool = lua
            .load(r#"return quarto.doc.is_format("latex")"#)
            .eval()
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_is_format_unknown_returns_false() {
        let lua = create_test_lua_with_format("html");
        let result: bool = lua
            .load(r#"return quarto.doc.is_format("unknown")"#)
            .eval()
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_is_format_alias_works() {
        let lua = create_test_lua_with_format("html");
        let result: bool = lua
            .load(r#"return quarto.doc.isFormat("html")"#)
            .eval()
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_is_format_reads_format_at_call_time() {
        let lua = create_test_lua_with_format("html");
        let result1: bool = lua
            .load(r#"return quarto.doc.is_format("html")"#)
            .eval()
            .unwrap();
        assert!(result1);

        // Change FORMAT
        lua.globals().set("FORMAT", "latex").unwrap();
        let result2: bool = lua
            .load(r#"return quarto.doc.is_format("html")"#)
            .eval()
            .unwrap();
        assert!(!result2);

        let result3: bool = lua
            .load(r#"return quarto.doc.is_format("latex")"#)
            .eval()
            .unwrap();
        assert!(result3);
    }

    // =====================================================================
    // has_bootstrap tests
    // =====================================================================

    #[test]
    fn test_has_bootstrap_true_for_html() {
        let lua = create_test_lua_with_format("html");
        let result: bool = lua
            .load(r#"return quarto.doc.has_bootstrap()"#)
            .eval()
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_has_bootstrap_false_for_latex() {
        let lua = create_test_lua_with_format("latex");
        let result: bool = lua
            .load(r#"return quarto.doc.has_bootstrap()"#)
            .eval()
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_has_bootstrap_false_for_epub() {
        let lua = create_test_lua_with_format("epub3");
        let result: bool = lua
            .load(r#"return quarto.doc.has_bootstrap()"#)
            .eval()
            .unwrap();
        assert!(!result);
    }

    // =====================================================================
    // add_html_dependency tests
    // =====================================================================

    #[test]
    fn test_add_html_dependency_stores_and_deduplicates() {
        let lua = create_test_lua_with_format("html");
        lua.load(
            r#"
            quarto.doc.add_html_dependency({name = "test-dep", stylesheets = {"a.css"}})
            quarto.doc.add_html_dependency({name = "test-dep", stylesheets = {"b.css"}})
            quarto.doc.add_html_dependency({name = "other-dep", scripts = {"c.js"}})
            "#,
        )
        .exec()
        .unwrap();

        let deps = extract_html_dependencies(&lua).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "test-dep");
        assert_eq!(deps[0].stylesheets.len(), 1); // Not 2 — deduplicated
        assert_eq!(deps[1].name, "other-dep");
    }

    #[test]
    fn test_add_html_dependency_string_and_table_entries() {
        let lua = create_test_lua_with_format("html");
        push_script_dir(&lua, "/ext").unwrap();
        lua.load(
            r#"
            quarto.doc.add_html_dependency({
                name = "mixed",
                stylesheets = {"style.css", {name = "other.css", path = "other.css"}},
            })
            "#,
        )
        .exec()
        .unwrap();

        let deps = extract_html_dependencies(&lua).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].stylesheets.len(), 2);
        assert_eq!(deps[0].stylesheets[0], PathBuf::from("/ext/style.css"));
        assert_eq!(deps[0].stylesheets[1], PathBuf::from("/ext/other.css"));
    }

    #[test]
    fn test_add_html_dependency_resolves_relative_paths() {
        let lua = create_test_lua_with_format("html");
        push_script_dir(&lua, "/my/extension").unwrap();
        lua.load(
            r#"
            quarto.doc.add_html_dependency({
                name = "dep",
                stylesheets = {"style.css"},
                scripts = {"script.js"},
            })
            "#,
        )
        .exec()
        .unwrap();

        let deps = extract_html_dependencies(&lua).unwrap();
        assert_eq!(
            deps[0].stylesheets[0],
            PathBuf::from("/my/extension/style.css")
        );
        assert_eq!(deps[0].scripts[0], PathBuf::from("/my/extension/script.js"));
    }

    #[test]
    fn test_add_html_dependency_warns_unsupported_fields() {
        let lua = create_test_lua_with_format("html");
        // This should not error, just warn
        lua.load(
            r#"
            quarto.doc.add_html_dependency({
                name = "dep",
                stylesheets = {"style.css"},
                meta = {viewport = "width=device-width"},
            })
            "#,
        )
        .exec()
        .unwrap();

        // Verify the dep was still stored
        let deps = extract_html_dependencies(&lua).unwrap();
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_add_html_dependency_errors_unknown_fields() {
        let lua = create_test_lua_with_format("html");
        let result = lua
            .load(
                r#"
            quarto.doc.add_html_dependency({
                name = "dep",
                stylesheets = {"style.css"},
                totally_bogus = true,
            })
            "#,
            )
            .exec();

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown field"), "Error was: {}", err);
    }

    // ---------------------------------------------------------------------
    // bd-add-html-dependency-version-5tnub5ds
    //
    // Extensions are written to call add_html_dependency unconditionally for
    // every matching element — that is the documented idiom, and the dedup
    // by name is what makes it safe. The unsupported-field warning must
    // therefore fire once per *distinct dependency*, not once per *call*.
    // ---------------------------------------------------------------------

    #[test]
    fn test_add_html_dependency_warns_unsupported_field_once_per_dependency() {
        let lua = create_test_lua_with_format("html");
        lua.load(
            r#"
            for _ = 1, 3 do
                quarto.doc.add_html_dependency({
                    name = "dep",
                    stylesheets = {"style.css"},
                    meta = {viewport = "width=device-width"},
                })
            end
            "#,
        )
        .exec()
        .unwrap();

        // Three calls, one distinct dependency, one warning.
        let warnings = diagnostic_messages(&lua);
        assert_eq!(
            warnings.len(),
            1,
            "expected exactly one warning for one distinct dependency, got: {warnings:?}"
        );
        assert!(warnings[0].contains("'meta'"), "was: {}", warnings[0]);
        assert_eq!(extract_html_dependencies(&lua).unwrap().len(), 1);
    }

    /// The field loop does double duty: it warns on unsupported fields *and*
    /// hard-errors on unknown ones. Only the warning may move after the dedup
    /// early-return — a typo must keep erroring on every call, including one
    /// whose name is already registered.
    #[test]
    fn test_add_html_dependency_errors_unknown_field_on_repeat_call() {
        let lua = create_test_lua_with_format("html");
        lua.load(r#"quarto.doc.add_html_dependency({name = "dep", scripts = {"a.js"}})"#)
            .exec()
            .unwrap();

        let result = lua
            .load(
                r#"
                quarto.doc.add_html_dependency({
                    name = "dep",
                    scripts = {"a.js"},
                    totally_bogus = true,
                })
                "#,
            )
            .exec();

        assert!(
            result.is_err(),
            "an unknown field must error even when the dependency name is already registered"
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown field"), "Error was: {}", err);
    }

    #[test]
    fn test_add_html_dependency_version_does_not_warn() {
        let lua = create_test_lua_with_format("html");
        lua.load(
            r#"
            quarto.doc.add_html_dependency({
                name = "dep",
                version = "1.0.0",
                scripts = {"dep.js"},
            })
            "#,
        )
        .exec()
        .unwrap();

        assert_eq!(
            diagnostic_messages(&lua),
            Vec::<String>::new(),
            "'version' is supported; it must not warn"
        );
    }

    #[test]
    fn test_add_html_dependency_version_is_extracted() {
        let lua = create_test_lua_with_format("html");
        lua.load(
            r#"
            quarto.doc.add_html_dependency({name = "versioned", version = "1.0.0", scripts = {"a.js"}})
            quarto.doc.add_html_dependency({name = "plain", scripts = {"b.js"}})
            "#,
        )
        .exec()
        .unwrap();

        let deps = extract_html_dependencies(&lua).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].version.as_deref(), Some("1.0.0"));
        assert_eq!(deps[1].version, None);
    }

    /// Within one document, registering the same name at a different version
    /// is first-wins (as in Quarto 1) *and* warns — injecting two versions of
    /// one library into a single page is a bug, not a feature. Cross-*render*
    /// coexistence, which is what `freeze` needs, is handled downstream by the
    /// versioned artifact key, not here.
    #[test]
    fn test_add_html_dependency_same_name_different_version_warns_and_keeps_first() {
        let lua = create_test_lua_with_format("html");
        lua.load(
            r#"
            quarto.doc.add_html_dependency({name = "dep", version = "1.0.0", scripts = {"a.js"}})
            quarto.doc.add_html_dependency({name = "dep", version = "2.0.0", scripts = {"b.js"}})
            "#,
        )
        .exec()
        .unwrap();

        let deps = extract_html_dependencies(&lua).unwrap();
        assert_eq!(deps.len(), 1, "first registration wins");
        assert_eq!(deps[0].version.as_deref(), Some("1.0.0"));

        let warnings = diagnostic_messages(&lua);
        assert_eq!(
            warnings.len(),
            1,
            "a version conflict within one document should warn, got: {warnings:?}"
        );
        assert!(
            warnings[0].contains("dep") && warnings[0].contains("2.0.0"),
            "warning should name the dependency and the rejected version, was: {}",
            warnings[0]
        );
    }

    /// The same name at the *same* version is the ordinary
    /// call-once-per-element idiom, and must stay silent.
    #[test]
    fn test_add_html_dependency_same_name_same_version_is_silent() {
        let lua = create_test_lua_with_format("html");
        lua.load(
            r#"
            quarto.doc.add_html_dependency({name = "dep", version = "1.0.0", scripts = {"a.js"}})
            quarto.doc.add_html_dependency({name = "dep", version = "1.0.0", scripts = {"a.js"}})
            "#,
        )
        .exec()
        .unwrap();

        assert_eq!(extract_html_dependencies(&lua).unwrap().len(), 1);
        assert_eq!(diagnostic_messages(&lua), Vec::<String>::new());
    }

    #[test]
    fn test_add_html_dependency_camel_case_alias() {
        let lua = create_test_lua_with_format("html");
        lua.load(r#"quarto.doc.addHtmlDependency({name = "dep", stylesheets = {"a.css"}})"#)
            .exec()
            .unwrap();

        let deps = extract_html_dependencies(&lua).unwrap();
        assert_eq!(deps.len(), 1);
    }

    // =====================================================================
    // include_text tests
    // =====================================================================

    #[test]
    fn test_include_text_stores_correctly() {
        let lua = create_test_lua_with_format("html");
        lua.load(
            r#"
            quarto.doc.include_text("in-header", "<meta name='test'>")
            quarto.doc.include_text("before-body", "<div>before</div>")
            quarto.doc.include_text("after-body", "<div>after</div>")
            "#,
        )
        .exec()
        .unwrap();

        let includes = extract_text_includes(&lua).unwrap();
        assert_eq!(includes.len(), 3);
        assert_eq!(includes[0].location, IncludeLocation::InHeader);
        assert_eq!(includes[0].content, "<meta name='test'>");
        assert_eq!(includes[1].location, IncludeLocation::BeforeBody);
        assert_eq!(includes[2].location, IncludeLocation::AfterBody);
    }

    #[test]
    fn test_include_text_rejects_invalid_location() {
        let lua = create_test_lua_with_format("html");
        let result = lua
            .load(r#"quarto.doc.include_text("invalid-location", "text")"#)
            .exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_include_text_camel_case_alias() {
        let lua = create_test_lua_with_format("html");
        lua.load(r#"quarto.doc.includeText("in-header", "<style></style>")"#)
            .exec()
            .unwrap();

        let includes = extract_text_includes(&lua).unwrap();
        assert_eq!(includes.len(), 1);
    }

    // =====================================================================
    // Extraction tests
    // =====================================================================

    #[test]
    fn test_extract_empty() {
        let lua = create_test_lua_with_format("html");
        let deps = extract_html_dependencies(&lua).unwrap();
        assert!(deps.is_empty());
        let includes = extract_text_includes(&lua).unwrap();
        assert!(includes.is_empty());
    }
}

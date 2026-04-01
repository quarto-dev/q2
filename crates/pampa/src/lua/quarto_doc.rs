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
#[derive(Debug, Clone)]
pub struct HtmlDependency {
    pub name: String,
    pub stylesheets: Vec<PathBuf>,
    pub scripts: Vec<PathBuf>,
}

/// A text include registered by an extension via `quarto.doc.include_text`.
#[derive(Debug, Clone)]
pub struct TextInclude {
    pub location: IncludeLocation,
    pub content: String,
}

/// Where to inject text in the output document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncludeLocation {
    InHeader,
    BeforeBody,
    AfterBody,
}

// =========================================================================
// Known fields for add_html_dependency validation
// =========================================================================

/// Fields we support.
const SUPPORTED_FIELDS: &[&str] = &["name", "scripts", "stylesheets"];

/// Fields that TS Quarto supports but we don't yet.
const UNSUPPORTED_FIELDS: &[&str] = &[
    "version",
    "meta",
    "links",
    "resources",
    "serviceworkers",
    "head",
];

// =========================================================================
// Format alias matching (matching TS Quarto's _format.lua)
// =========================================================================

/// Check if FORMAT matches a query using TS Quarto's alias-based matching.
/// `format` is the current FORMAT global value, `query` is what the extension asked for.
fn is_format_match(format: &str, query: &str) -> bool {
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

            // Check for unsupported and unknown fields
            for pair in dep.pairs::<String, Value>() {
                let (key, _) = pair?;
                if SUPPORTED_FIELDS.contains(&key.as_str()) {
                    continue;
                }
                if UNSUPPORTED_FIELDS.contains(&key.as_str()) {
                    // Emit warning via quarto.warn
                    let quarto: Table = lua.globals().get("quarto")?;
                    let warn_fn: mlua::Function = quarto.get("warn")?;
                    warn_fn.call::<()>(format!(
                        "add_html_dependency: field '{}' is not yet supported and will be ignored",
                        key
                    ))?;
                } else {
                    return Err(mlua::Error::runtime(format!(
                        "add_html_dependency: unknown field '{}'",
                        key
                    )));
                }
            }

            // Check for deduplication by name
            let quarto: Table = lua.globals().get("quarto")?;
            let doc_table: Table = quarto.get("doc")?;
            let deps: Table = doc_table.get("_dependencies")?;
            for i in 1..=deps.raw_len() {
                let existing: Table = deps.get(i)?;
                let existing_name: String = existing.get("name")?;
                if existing_name == name {
                    return Ok(()); // Already registered
                }
            }

            // Read and resolve file paths
            let stylesheets = read_dep_files(lua, &dep, "stylesheets")?;
            let scripts = read_dep_files(lua, &dep, "scripts")?;

            // Store as a Lua table
            let entry = lua.create_table()?;
            entry.set("name", name)?;

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
            stylesheets,
            scripts,
        });
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

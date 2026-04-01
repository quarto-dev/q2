/*
 * dofile_wasm.rs
 *
 * Override `dofile` and `loadfile` globals for WASM and test environments.
 *
 * On native, the C `fopen`-based implementations from mlua's base library
 * work fine. On WASM, `fopen` returns null (from `c_shim.rs`), so we need
 * Rust implementations backed by SystemRuntime.
 *
 * `dofile` pushes the loaded file's directory onto the script-dir stack
 * before execution and pops after, so that nested `resolve_path` calls
 * resolve against the loaded file's directory.
 *
 * `loadfile` does NOT push/pop — it returns an unexecuted chunk. The
 * script dir at execution time determines path resolution.
 */

use std::path::Path;
use std::sync::Arc;

use mlua::{Lua, MultiValue, Result, Value};

use super::quarto_api::{current_script_dir, pop_script_dir, push_script_dir};
use super::runtime::SystemRuntime;

/// Resolve a path for dofile/loadfile in the restricted environment.
///
/// Relative paths are resolved against the current script dir (top of stack).
/// If the stack is empty and the path is relative, apply the `/project/` prefix
/// matching `io_wasm.rs` conventions.
fn resolve_dofile_path(lua: &Lua, path: &str) -> Result<String> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Ok(path.to_string());
    }

    let script_dir = current_script_dir(lua)?;
    if !script_dir.is_empty() {
        let resolved = Path::new(&script_dir).join(path);
        return Ok(resolved.to_string_lossy().to_string());
    }

    // No script dir — use /project/ prefix for WASM VFS compatibility
    Ok(format!("/project/{}", path))
}

/// Register `dofile` and `loadfile` overrides for the restricted Lua environment.
pub fn register_wasm_dofile(lua: &Lua, runtime: Arc<dyn SystemRuntime>) -> Result<()> {
    // dofile(path) — read, compile, push script dir, execute, pop, return results
    let rt = runtime.clone();
    lua.globals().set(
        "dofile",
        lua.create_function(move |lua, path: String| {
            let resolved = resolve_dofile_path(lua, &path)?;

            let content = rt.file_read_string(Path::new(&resolved)).map_err(|e| {
                mlua::Error::runtime(format!("dofile: cannot read '{}': {}", path, e))
            })?;

            let chunk = lua.load(&content).set_name(&path);

            // Push the loaded file's directory onto the script-dir stack
            let file_dir = Path::new(&resolved)
                .parent()
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .to_string();
            push_script_dir(lua, &file_dir)?;

            let result = chunk.eval::<MultiValue>();

            pop_script_dir(lua)?;

            result
        })?,
    )?;

    // loadfile(path) — read and compile only, return chunk (or nil + error)
    lua.globals().set(
        "loadfile",
        lua.create_function(move |lua, path: String| {
            let resolved = resolve_dofile_path(lua, &path)?;

            let content = match runtime.file_read_string(Path::new(&resolved)) {
                Ok(c) => c,
                Err(e) => {
                    // Lua semantics: loadfile returns (nil, error_message) on failure
                    return Ok(MultiValue::from_iter([
                        Value::Nil,
                        Value::String(lua.create_string(format!("cannot read '{}': {}", path, e))?),
                    ]));
                }
            };

            match lua.load(&content).set_name(&path).into_function() {
                Ok(func) => Ok(MultiValue::from_iter([Value::Function(func)])),
                Err(e) => Ok(MultiValue::from_iter([
                    Value::Nil,
                    Value::String(lua.create_string(e.to_string())?),
                ])),
            }
        })?,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::filter::apply_lua_filter;
    use crate::pandoc::ASTContext;
    use crate::pandoc::*;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn native_runtime() -> Arc<dyn quarto_system_runtime::SystemRuntime> {
        Arc::new(quarto_system_runtime::NativeRuntime::new())
    }

    fn empty_pandoc() -> Pandoc {
        Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        }
    }

    #[test]
    fn test_dofile_executes_and_returns_values() {
        let dir = TempDir::new().unwrap();
        let helper_path = dir.path().join("helper.lua");
        fs::write(&helper_path, "return 42").unwrap();

        let filter_path = dir.path().join("filter.lua");
        fs::write(
            &filter_path,
            &format!(
                r#"
local val = dofile("{}")
function Str(elem)
    return pandoc.Str(tostring(val))
end
"#,
                helper_path.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();

        let pandoc = empty_pandoc();
        let context = ASTContext::new();
        let filtered = apply_lua_filter(&pandoc, &context, &filter_path, "html", native_runtime())
            .unwrap()
            .pandoc;

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "42"),
                other => panic!("Expected Str, got {:?}", other),
            },
            other => panic!("Expected Paragraph, got {:?}", other),
        }
    }

    #[test]
    fn test_loadfile_returns_callable_chunk() {
        let dir = TempDir::new().unwrap();
        let helper_path = dir.path().join("helper.lua");
        fs::write(&helper_path, "return 99").unwrap();

        let filter_path = dir.path().join("filter.lua");
        fs::write(
            &filter_path,
            &format!(
                r#"
local chunk = loadfile("{}")
local val = chunk()
function Str(elem)
    return pandoc.Str(tostring(val))
end
"#,
                helper_path.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();

        let pandoc = empty_pandoc();
        let context = ASTContext::new();
        let filtered = apply_lua_filter(&pandoc, &context, &filter_path, "html", native_runtime())
            .unwrap()
            .pandoc;

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "99"),
                other => panic!("Expected Str, got {:?}", other),
            },
            other => panic!("Expected Paragraph, got {:?}", other),
        }
    }

    #[test]
    fn test_dofile_nonexistent_errors() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("filter.lua");
        fs::write(
            &filter_path,
            r#"
local ok, err = pcall(dofile, "/nonexistent/file.lua")
function Str(elem)
    if ok then
        return pandoc.Str("unexpected-success")
    else
        return pandoc.Str("error-caught")
    end
end
"#,
        )
        .unwrap();

        let pandoc = empty_pandoc();
        let context = ASTContext::new();
        let filtered = apply_lua_filter(&pandoc, &context, &filter_path, "html", native_runtime())
            .unwrap()
            .pandoc;

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "error-caught"),
                other => panic!("Expected Str, got {:?}", other),
            },
            other => panic!("Expected Paragraph, got {:?}", other),
        }
    }

    #[test]
    fn test_loadfile_nonexistent_returns_nil_and_error() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("filter.lua");
        fs::write(
            &filter_path,
            r#"
local chunk, err = loadfile("/nonexistent/file.lua")
function Str(elem)
    if chunk == nil and err ~= nil then
        return pandoc.Str("nil-plus-error")
    else
        return pandoc.Str("unexpected")
    end
end
"#,
        )
        .unwrap();

        let pandoc = empty_pandoc();
        let context = ASTContext::new();
        let filtered = apply_lua_filter(&pandoc, &context, &filter_path, "html", native_runtime())
            .unwrap()
            .pandoc;

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "nil-plus-error"),
                other => panic!("Expected Str, got {:?}", other),
            },
            other => panic!("Expected Paragraph, got {:?}", other),
        }
    }

    #[test]
    fn test_dofile_script_dir_stack() {
        // Extension in /ext/ calls dofile("helpers/ui.lua"), and ui.lua calls
        // quarto.utils.resolve_path("style.css") — should resolve to
        // /ext/helpers/style.css, not /ext/style.css
        let dir = TempDir::new().unwrap();

        // Create helpers subdirectory
        let helpers_dir = dir.path().join("helpers");
        fs::create_dir(&helpers_dir).unwrap();

        let helper_path = helpers_dir.join("ui.lua");
        fs::write(
            &helper_path,
            r#"return quarto.utils.resolve_path("style.css")"#,
        )
        .unwrap();

        let filter_path = dir.path().join("filter.lua");
        fs::write(
            &filter_path,
            &format!(
                r#"
local resolved = dofile("{}")
function Str(elem)
    return pandoc.Str(resolved)
end
"#,
                helper_path.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();

        let pandoc = empty_pandoc();
        let context = ASTContext::new();
        let filtered = apply_lua_filter(&pandoc, &context, &filter_path, "html", native_runtime())
            .unwrap()
            .pandoc;

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => {
                    let expected = helpers_dir.join("style.css").to_string_lossy().to_string();
                    assert_eq!(
                        quarto_util::to_forward_slashes(Path::new(&s.text)),
                        quarto_util::to_forward_slashes(Path::new(&expected)),
                        "resolve_path inside dofile'd helper should resolve against helper's dir"
                    );
                }
                other => panic!("Expected Str, got {:?}", other),
            },
            other => panic!("Expected Paragraph, got {:?}", other),
        }
    }
}

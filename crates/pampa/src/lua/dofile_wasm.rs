/*
 * dofile_wasm.rs
 *
 * Override `dofile` and `loadfile` globals for WASM environments.
 *
 * On native, the C `fopen`-based implementations from mlua's base library
 * work fine. On WASM, `fopen` returns null (from `c_shim.rs`), so we need
 * Rust implementations backed by SystemRuntime.
 *
 * These overrides handle file I/O only. They do NOT modify the script-dir
 * stack — path resolution during execution uses whatever script dir was
 * set by the caller (filter.rs or shortcode.rs), matching native Lua and
 * Pandoc/TS Quarto behavior.
 */

use std::path::Path;
use std::sync::Arc;

use mlua::{Lua, MultiValue, Result, Value};

use super::quarto_api::current_script_dir;
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
    // dofile(path) — read, compile, execute, return results
    let rt = runtime.clone();
    lua.globals().set(
        "dofile",
        lua.create_function(move |lua, path: String| {
            let resolved = resolve_dofile_path(lua, &path)?;

            let content = rt.file_read_string(Path::new(&resolved)).map_err(|e| {
                mlua::Error::runtime(format!("dofile: cannot read '{}': {}", path, e))
            })?;

            lua.load(&content).set_name(&path).eval::<MultiValue>()
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

    #[tokio::test]
    async fn test_dofile_executes_and_returns_values() {
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
            .await
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

    #[tokio::test]
    async fn test_loadfile_returns_callable_chunk() {
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
            .await
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

    #[tokio::test]
    async fn test_dofile_nonexistent_errors() {
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
            .await
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

    #[tokio::test]
    async fn test_loadfile_nonexistent_returns_nil_and_error() {
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
            .await
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
}

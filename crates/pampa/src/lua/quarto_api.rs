/*
 * lua/quarto_api.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Extends the `quarto` global table with additional API sub-namespaces:
 * quarto.json, quarto.log, quarto.utils.
 *
 * Must be called AFTER register_pandoc_namespace() (which creates both
 * `pandoc` and `quarto` globals).
 */

use mlua::{Lua, MultiValue, Result, Table, Value};
use std::path::{Component, Path, PathBuf};

/// Extends the `quarto` global (already created by register_quarto_namespace)
/// with additional API sub-namespaces: quarto.json, quarto.log, quarto.utils.
pub fn register_quarto_api(lua: &Lua) -> Result<()> {
    let quarto: Table = lua.globals().get("quarto")?;

    register_quarto_json(lua, &quarto)?;
    register_quarto_log(lua, &quarto)?;
    register_quarto_utils(lua, &quarto)?;

    Ok(())
}

/// `quarto.json` — alias of `pandoc.json`
fn register_quarto_json(lua: &Lua, quarto: &Table) -> Result<()> {
    let pandoc: Table = lua.globals().get("pandoc")?;
    let pandoc_json: Table = pandoc.get("json")?;
    quarto.set("json", pandoc_json)?;
    Ok(())
}

/// `quarto.log` — Rust-backed stderr logging
fn register_quarto_log(lua: &Lua, quarto: &Table) -> Result<()> {
    let log = lua.create_table()?;
    log.set("loglevel", 0)?; // Default: warnings + errors

    // quarto.log.output(...) — always writes
    log.set(
        "output",
        lua.create_function(|lua, args: MultiValue| {
            let text = stringify_log_args(lua, &args)?;
            eprintln!("{}", text);
            Ok(())
        })?,
    )?;

    // Helper macro-like approach: create level-gated log functions
    // quarto.log.error(...) — writes if loglevel >= -1
    log.set("error", create_log_fn(lua, "(E)", -1)?)?;
    // quarto.log.warning(...) — writes if loglevel >= 0
    log.set("warning", create_log_fn(lua, "(W)", 0)?)?;
    // quarto.log.info(...) — writes if loglevel >= 1
    log.set("info", create_log_fn(lua, "(I)", 1)?)?;
    // quarto.log.debug(...) — writes if loglevel >= 2
    log.set("debug", create_log_fn(lua, "(D)", 2)?)?;
    // quarto.log.trace(...) — writes if loglevel >= 3
    log.set("trace", create_log_fn(lua, "(T)", 3)?)?;

    // quarto.log.setloglevel(level) — set level, return old
    log.set(
        "setloglevel",
        lua.create_function(|lua, level: i32| {
            let log_table: Table = lua.globals().get::<Table>("quarto")?.get::<Table>("log")?;
            let old: i32 = log_table.get("loglevel")?;
            log_table.set("loglevel", level)?;
            Ok(old)
        })?,
    )?;

    quarto.set("log", log)?;
    Ok(())
}

/// Create a level-gated log function with a prefix.
fn create_log_fn(lua: &Lua, prefix: &'static str, min_level: i32) -> Result<mlua::Function> {
    lua.create_function(move |lua, args: MultiValue| {
        let log_table: Table = lua.globals().get::<Table>("quarto")?.get::<Table>("log")?;
        let level: i32 = log_table.get("loglevel")?;
        if level >= min_level {
            let text = stringify_log_args(lua, &args)?;
            eprintln!("{} {}", prefix, text);
        }
        Ok(())
    })
}

/// Stringify log arguments. Each arg is converted to a string and joined with tabs.
fn stringify_log_args(lua: &Lua, args: &MultiValue) -> Result<String> {
    let mut parts = Vec::new();
    for arg in args.iter() {
        parts.push(stringify_log_value(lua, arg, 0)?);
    }
    Ok(parts.join("\t"))
}

/// Stringify a single value for logging.
fn stringify_log_value(lua: &Lua, value: &Value, depth: usize) -> Result<String> {
    if depth > 5 {
        return Ok("<table (max depth)>".to_string());
    }
    match value {
        Value::Nil => Ok("nil".to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        Value::Integer(n) => Ok(n.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => Ok(s.to_str()?.to_string()),
        Value::Table(t) => stringify_table(lua, t, depth),
        Value::UserData(_) => {
            // Use Lua tostring() which invokes __tostring metamethod
            let tostring: mlua::Function = lua.globals().get("tostring")?;
            let result: String = tostring.call(value.clone())?;
            Ok(result)
        }
        Value::Function(_) => Ok("<function>".to_string()),
        _ => Ok(format!("<{}>", value.type_name())),
    }
}

/// Stringify a Lua table recursively for logging.
fn stringify_table(lua: &Lua, table: &Table, depth: usize) -> Result<String> {
    let mut parts = Vec::new();
    let len = table.raw_len();

    // Check if it's a sequence (array-like)
    if len > 0 {
        for i in 1..=len {
            let val: Value = table.get(i)?;
            parts.push(stringify_log_value(lua, &val, depth + 1)?);
        }
        return Ok(format!("{{{}}}", parts.join(", ")));
    }

    // Otherwise treat as key-value
    for pair in table.pairs::<Value, Value>() {
        let (k, v) = pair?;
        let ks = stringify_log_value(lua, &k, depth + 1)?;
        let vs = stringify_log_value(lua, &v, depth + 1)?;
        parts.push(format!("{}={}", ks, vs));
    }
    Ok(format!("{{{}}}", parts.join(", ")))
}

/// `quarto.utils` — utility functions
fn register_quarto_utils(lua: &Lua, quarto: &Table) -> Result<()> {
    let utils = lua.create_table()?;

    // quarto.utils.resolve_path(path) — resolve relative to script dir
    utils.set(
        "resolve_path",
        lua.create_function(|lua, path: String| {
            let p = Path::new(&path);
            if p.is_absolute() {
                return Ok(path);
            }
            let script_dir: String = lua
                .globals()
                .get::<String>("_quarto_script_dir")
                .unwrap_or_default();
            if script_dir.is_empty() {
                return Ok(path);
            }
            let resolved = PathBuf::from(&script_dir).join(&path);
            Ok(normalize_path(&resolved))
        })?,
    )?;

    // quarto.utils.type(value) — alias pandoc.utils.type
    let pandoc: Table = lua.globals().get("pandoc")?;
    let pandoc_utils: Table = pandoc.get("utils")?;
    let type_fn: mlua::Function = pandoc_utils.get("type")?;
    utils.set("type", type_fn)?;

    quarto.set("utils", utils)?;
    Ok(())
}

/// Normalize a path by collapsing `.` and `..` without touching the filesystem.
fn normalize_path(path: &Path) -> String {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {} // skip `.`
            Component::ParentDir => {
                // Pop last component if it's a normal component
                if let Some(last) = components.last() {
                    if !matches!(last, Component::RootDir | Component::Prefix(_)) {
                        components.pop();
                        continue;
                    }
                }
                components.push(component);
            }
            _ => components.push(component),
        }
    }
    let result: PathBuf = components.iter().collect();
    result.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::constructors::register_pandoc_namespace;
    use crate::lua::mediabag::create_shared_mediabag;
    use crate::lua::runtime::NativeRuntime;
    use std::sync::Arc;

    fn create_test_lua() -> Lua {
        let lua = Lua::new();
        let runtime = Arc::new(NativeRuntime::new());
        register_pandoc_namespace(&lua, runtime, create_shared_mediabag()).unwrap();
        register_quarto_api(&lua).unwrap();
        lua
    }

    // =========================================================================
    // quarto.json tests
    // =========================================================================

    #[test]
    fn test_quarto_json_decode() {
        let lua = create_test_lua();
        let result: i32 = lua
            .load(r#"return quarto.json.decode('{"a":1}').a"#)
            .eval()
            .unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_quarto_json_encode() {
        let lua = create_test_lua();
        let result: String = lua
            .load(r#"return quarto.json.encode({a = 1})"#)
            .eval()
            .unwrap();
        assert!(result.contains("\"a\""));
        assert!(result.contains("1"));
    }

    // =========================================================================
    // quarto.log tests
    // =========================================================================

    #[test]
    fn test_quarto_log_error_runs() {
        let lua = create_test_lua();
        lua.load(r#"quarto.log.error("test error")"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn test_quarto_log_warning_runs() {
        let lua = create_test_lua();
        lua.load(r#"quarto.log.warning("test warning")"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn test_quarto_log_output_runs() {
        let lua = create_test_lua();
        lua.load(r#"quarto.log.output("test output")"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn test_quarto_log_respects_level() {
        let lua = create_test_lua();
        // Default level is 0; info requires >= 1, so this should be silent (no error)
        lua.load(r#"quarto.log.info("should be silent")"#)
            .exec()
            .unwrap();
        // debug requires >= 2
        lua.load(r#"quarto.log.debug("should be silent")"#)
            .exec()
            .unwrap();
        // trace requires >= 3
        lua.load(r#"quarto.log.trace("should be silent")"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn test_quarto_log_setloglevel() {
        let lua = create_test_lua();
        let old: i32 = lua
            .load(r#"return quarto.log.setloglevel(2)"#)
            .eval()
            .unwrap();
        assert_eq!(old, 0);

        let current: i32 = lua.load(r#"return quarto.log.loglevel"#).eval().unwrap();
        assert_eq!(current, 2);

        // Now info should work (level 2 >= 1)
        lua.load(r#"quarto.log.info("visible at level 2")"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn test_quarto_log_multiple_args() {
        let lua = create_test_lua();
        lua.load(r#"quarto.log.output("hello", "world", 42)"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn test_quarto_log_table_arg() {
        let lua = create_test_lua();
        lua.load(r#"quarto.log.output({1, 2, 3})"#).exec().unwrap();
    }

    // =========================================================================
    // quarto.utils.resolve_path tests
    // =========================================================================

    #[test]
    fn test_quarto_utils_resolve_path_absolute() {
        let lua = create_test_lua();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("/abs/path/file.json")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "/abs/path/file.json");
    }

    #[test]
    fn test_quarto_utils_resolve_path_relative() {
        let lua = create_test_lua();
        lua.globals()
            .set("_quarto_script_dir", "/some/extension/dir")
            .unwrap();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("data.json")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "/some/extension/dir/data.json");
    }

    #[test]
    fn test_quarto_utils_resolve_path_relative_with_subdir() {
        let lua = create_test_lua();
        lua.globals().set("_quarto_script_dir", "/ext/dir").unwrap();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("sub/data.json")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "/ext/dir/sub/data.json");
    }

    #[test]
    fn test_quarto_utils_resolve_path_no_script_dir() {
        let lua = create_test_lua();
        // No _quarto_script_dir set, should return as-is
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("data.json")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "data.json");
    }

    #[test]
    fn test_quarto_utils_resolve_path_with_dotdot() {
        let lua = create_test_lua();
        lua.globals()
            .set("_quarto_script_dir", "/some/extension/dir")
            .unwrap();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("../shared/data.json")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "/some/extension/shared/data.json");
    }

    // =========================================================================
    // quarto.utils.type tests
    // =========================================================================

    #[test]
    fn test_quarto_utils_type_str() {
        let lua = create_test_lua();
        let result: String = lua
            .load(r#"return quarto.utils.type(pandoc.Str("x"))"#)
            .eval()
            .unwrap();
        assert_eq!(result, "Str");
    }

    #[test]
    fn test_quarto_utils_type_table() {
        let lua = create_test_lua();
        let result: String = lua.load(r#"return quarto.utils.type({})"#).eval().unwrap();
        assert_eq!(result, "table");
    }

    // =========================================================================
    // normalize_path unit tests
    // =========================================================================

    #[test]
    fn test_normalize_path_simple() {
        assert_eq!(normalize_path(Path::new("/a/b/c")), "/a/b/c");
    }

    #[test]
    fn test_normalize_path_with_dots() {
        assert_eq!(normalize_path(Path::new("/a/./b/c")), "/a/b/c");
    }

    #[test]
    fn test_normalize_path_with_dotdot() {
        assert_eq!(normalize_path(Path::new("/a/b/../c")), "/a/c");
    }

    #[test]
    fn test_normalize_path_with_dotdot_at_root() {
        assert_eq!(normalize_path(Path::new("/a/../b")), "/b");
    }

    #[test]
    fn test_normalize_path_relative() {
        assert_eq!(normalize_path(Path::new("a/b/c")), "a/b/c");
    }

    #[test]
    fn test_normalize_path_relative_with_dotdot() {
        assert_eq!(normalize_path(Path::new("a/b/../c")), "a/c");
    }
}

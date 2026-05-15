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

use base64::prelude::*;
use mlua::{Lua, MultiValue, Result, Table, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::attribution::AttributionLookup;

/// Extends the `quarto` global (already created by register_quarto_namespace)
/// with additional API sub-namespaces: quarto.json, quarto.log, quarto.utils.
pub fn register_quarto_api(lua: &Lua) -> Result<()> {
    let quarto: Table = lua.globals().get("quarto")?;

    init_script_dir_stack(lua)?;
    register_quarto_version(lua, &quarto)?;
    register_quarto_base64(lua, &quarto)?;
    register_quarto_json(lua, &quarto)?;
    register_quarto_log(lua, &quarto)?;
    register_quarto_utils(lua, &quarto)?;

    Ok(())
}

/// `quarto.version` — table {0, 1, 0} so extensions can do
/// `table.concat(quarto.version, '.')` to get "0.1.0"
fn register_quarto_version(lua: &Lua, quarto: &Table) -> Result<()> {
    let version = lua.create_table()?;
    version.set(1, 0)?;
    version.set(2, 1)?;
    version.set(3, 0)?;
    quarto.set("version", version)?;
    Ok(())
}

/// `quarto.base64` — base64 encoding
fn register_quarto_base64(lua: &Lua, quarto: &Table) -> Result<()> {
    let base64_table = lua.create_table()?;
    base64_table.set(
        "encode",
        lua.create_function(|_, data: mlua::String| Ok(BASE64_STANDARD.encode(data.as_bytes())))?,
    )?;
    quarto.set("base64", base64_table)?;
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

// =========================================================================
// Script-dir stack
// =========================================================================

/// Initialize the script-dir stack in the Lua state.
/// Must be called during Lua state setup, before any script execution.
pub fn init_script_dir_stack(lua: &Lua) -> Result<()> {
    lua.globals()
        .set("_quarto_script_dir_stack", lua.create_table()?)?;
    Ok(())
}

/// Push a directory onto the script-dir stack.
pub fn push_script_dir(lua: &Lua, dir: &str) -> Result<()> {
    let stack: Table = lua.globals().get("_quarto_script_dir_stack")?;
    let len = stack.raw_len();
    stack.set(len + 1, dir)?;
    Ok(())
}

/// Pop the top entry from the script-dir stack.
pub fn pop_script_dir(lua: &Lua) -> Result<()> {
    let stack: Table = lua.globals().get("_quarto_script_dir_stack")?;
    let len = stack.raw_len();
    if len > 0 {
        stack.set(len, mlua::Value::Nil)?;
    }
    Ok(())
}

/// Get the current script directory (top of the stack), or empty string if empty.
pub fn current_script_dir(lua: &Lua) -> Result<String> {
    let stack: Table = lua.globals().get("_quarto_script_dir_stack")?;
    let len = stack.raw_len();
    if len > 0 {
        Ok(stack.get::<String>(len).unwrap_or_default())
    } else {
        Ok(String::new())
    }
}

// =========================================================================
// quarto.attribution — bd-0fd0 host binding (read-only)
// =========================================================================

/// Register the `quarto.attribution.*` host binding on the existing
/// `quarto` global. Reads from the optional [`AttributionLookup`]
/// handle supplied by the filter runner (typically wrapping the
/// `Arc<AttributionData>` sidecar produced by
/// [`crate::stage::stages::AttributionGenerateStage`](../../quarto_core/stage/stages/struct.AttributionGenerateStage.html)
/// in `quarto-core`).
///
/// API exposed:
///
/// ```lua
/// -- Convenience: resolves identity automatically, reads
/// -- `el.source_info:byte_range()` internally. Returns nil for nodes
/// -- whose chain resolves to Concat/FilterProvenance, or non-primary
/// -- file (v1 single-doc invariant), or with no provider installed.
/// local hit = quarto.attribution.lookup(el)
/// -- => { actor = "alice@example.com", time = 1715000000000,
/// --      name = "Alice", color = "#ff0000" }
///
/// -- Primitive: arbitrary byte range against the primary file. No
/// -- identity join. Returns nil on no overlap, no provider, or
/// -- start >= end.
/// local raw = quarto.attribution.lookup_range(start, end)
/// -- => { actor = "alice@example.com", time = 1715000000000 }
///
/// -- Identity map snapshot, keyed by actor. Empty table when no
/// -- provider is installed.
/// local idents = quarto.attribution.identities()
/// -- => { ["alice@example.com"] = { name = "Alice", color = "#ff0000" } }
/// ```
///
/// When `handle` is `None`, the registered functions become no-op
/// stubs (lookup/lookup_range return nil; identities returns an
/// empty table). This keeps the binding present in the Lua
/// environment regardless of whether attribution is on, so filters
/// can call into it unconditionally.
pub fn register_quarto_attribution(
    lua: &Lua,
    handle: Option<Arc<dyn AttributionLookup>>,
) -> Result<()> {
    let quarto: Table = lua.globals().get("quarto")?;
    let attribution = lua.create_table()?;

    // `quarto.attribution.lookup_range(start, end)` — primitive raw
    // lookup. Returns `{actor, time}` table on hit, nil otherwise.
    let h_for_range = handle.clone();
    attribution.set(
        "lookup_range",
        lua.create_function(move |lua, (start, end_): (usize, usize)| {
            let Some(h) = h_for_range.as_ref() else {
                return Ok(Value::Nil);
            };
            let Some(hit) = h.lookup_range(start, end_) else {
                return Ok(Value::Nil);
            };
            let t = lua.create_table()?;
            t.set("actor", hit.actor)?;
            t.set("time", hit.time)?;
            Ok(Value::Table(t))
        })?,
    )?;

    // `quarto.attribution.identities()` — read-only snapshot of the
    // identity map keyed by actor. Empty table when no handle.
    let h_for_ids = handle.clone();
    attribution.set(
        "identities",
        lua.create_function(move |lua, ()| {
            let t = lua.create_table()?;
            if let Some(h) = h_for_ids.as_ref() {
                for entry in h.identities() {
                    let inner = lua.create_table()?;
                    inner.set("name", entry.name)?;
                    inner.set("color", entry.color)?;
                    t.set(entry.actor, inner)?;
                }
            }
            Ok(t)
        })?,
    )?;

    // Publish the table on `quarto.attribution` *before* compiling
    // the `lookup` thunk below — the thunk reads
    // `quarto.attribution.{lookup_range,identities}` at chunk-load
    // time into upvalues, so the table must already be reachable
    // through the `quarto` global.
    quarto.set("attribution", attribution.clone())?;

    // `quarto.attribution.lookup(el)` — convenience. Reads
    // `el.source_info:byte_range()`, calls `lookup_range`, joins the
    // identity entry. Returns nil when the source_info chain doesn't
    // resolve (Concat/FilterProvenance), when the resolved file_id
    // isn't 0 (v1 single-doc invariant), when no run overlaps the
    // range, or when no handle is installed.
    //
    // The implementation is a small Lua thunk that delegates to the
    // Rust-backed functions registered above — keeps the per-call
    // path purely Lua-side (no Rust closure to capture).
    lua.load(
        r#"
        local lookup_range = quarto.attribution.lookup_range
        local identities = quarto.attribution.identities
        return function(el)
            local si = el.source_info
            if si == nil then return nil end
            local r = si:byte_range()
            if r == nil then return nil end
            -- v1 single-doc invariant: skip non-primary file.
            local fid = si:file_id()
            if fid ~= nil and fid ~= 0 then return nil end
            local hit = lookup_range(r[1], r[2])
            if hit == nil then return nil end
            local idents = identities()
            local id = idents[hit.actor]
            if id == nil then
                return { actor = hit.actor, time = hit.time }
            end
            return {
                actor = hit.actor,
                time = hit.time,
                name = id.name,
                color = id.color,
            }
        end
        "#,
    )
    .eval::<mlua::Function>()
    .and_then(|f| attribution.set("lookup", f))?;

    Ok(())
}

/// `quarto.utils` — utility functions
fn register_quarto_utils(lua: &Lua, quarto: &Table) -> Result<()> {
    let utils = lua.create_table()?;

    // quarto.utils.resolve_path(path) — resolve relative to script dir (stack top)
    utils.set(
        "resolve_path",
        lua.create_function(|lua, path: String| {
            let p = Path::new(&path);
            if p.is_absolute() {
                return Ok(path);
            }
            let script_dir = current_script_dir(lua)?;
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

    // quarto.utils.as_inlines(obj) — coerce various types to pandoc.Inlines
    // Matches TS Quarto's _utils.lua:280-300. Type coercion, not markdown parsing.
    //
    // Note: our pandoc.utils.type() returns specific names (e.g., "Para", "Str")
    // rather than generic "Block"/"Inline" like real Pandoc. We use lookup tables
    // to classify element types.
    lua.load(
        r#"
        local pandoc_utils_type = pandoc.utils.type
        local blocks_to_inlines = pandoc.utils.blocks_to_inlines
        local block_types = {
            Para = true, Plain = true, Header = true, CodeBlock = true,
            RawBlock = true, BlockQuote = true, BulletList = true,
            OrderedList = true, DefinitionList = true, Div = true,
            LineBlock = true, Table = true, Figure = true,
            HorizontalRule = true,
        }
        local inline_types = {
            Str = true, Space = true, SoftBreak = true, LineBreak = true,
            Emph = true, Strong = true, Underline = true, Strikeout = true,
            Superscript = true, Subscript = true, SmallCaps = true,
            Quoted = true, Code = true, Math = true, RawInline = true,
            Link = true, Image = true, Span = true, Note = true,
            Cite = true, Shortcode = true,
        }
        return function(obj)
            local pt = pandoc_utils_type(obj)
            if pt == "Inlines" then
                return obj
            elseif inline_types[pt] then
                return pandoc.Inlines({obj})
            elseif pt == "Blocks" then
                return blocks_to_inlines(obj)
            elseif block_types[pt] then
                return blocks_to_inlines({obj})
            elseif pt == "List" or pt == "table" then
                if obj[1] and block_types[pandoc_utils_type(obj[1])] then
                    return blocks_to_inlines(obj)
                end
                return pandoc.Inlines(obj)
            else
                return pandoc.Inlines(obj or {})
            end
        end
        "#,
    )
    .eval::<mlua::Function>()
    .and_then(|f| utils.set("as_inlines", f))?;

    // quarto.utils.as_blocks(obj) — coerce various types to pandoc.Blocks
    // Companion to as_inlines, matches TS Quarto's _utils.lua:310-330.
    lua.load(
        r#"
        local pandoc_utils_type = pandoc.utils.type
        local block_types = {
            Para = true, Plain = true, Header = true, CodeBlock = true,
            RawBlock = true, BlockQuote = true, BulletList = true,
            OrderedList = true, DefinitionList = true, Div = true,
            LineBlock = true, Table = true, Figure = true,
            HorizontalRule = true,
        }
        local inline_types = {
            Str = true, Space = true, SoftBreak = true, LineBreak = true,
            Emph = true, Strong = true, Underline = true, Strikeout = true,
            Superscript = true, Subscript = true, SmallCaps = true,
            Quoted = true, Code = true, Math = true, RawInline = true,
            Link = true, Image = true, Span = true, Note = true,
            Cite = true, Shortcode = true,
        }
        return function(obj)
            local pt = pandoc_utils_type(obj)
            if pt == "Blocks" then
                return obj
            elseif block_types[pt] then
                return pandoc.Blocks({obj})
            elseif pt == "Inlines" then
                return pandoc.Blocks({pandoc.Plain(obj)})
            elseif inline_types[pt] then
                return pandoc.Blocks({pandoc.Plain({obj})})
            elseif pt == "List" or pt == "table" then
                if obj[1] and inline_types[pandoc_utils_type(obj[1])] then
                    return pandoc.Blocks({pandoc.Plain(obj)})
                end
                return pandoc.Blocks(obj)
            else
                return pandoc.Blocks(obj or {})
            end
        end
        "#,
    )
    .eval::<mlua::Function>()
    .and_then(|f| utils.set("as_blocks", f))?;

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
        push_script_dir(&lua, "/some/extension/dir").unwrap();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("data.json")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "/some/extension/dir/data.json");
    }

    #[test]
    fn test_quarto_utils_resolve_path_relative_with_subdir() {
        let lua = create_test_lua();
        push_script_dir(&lua, "/ext/dir").unwrap();
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
        push_script_dir(&lua, "/some/extension/dir").unwrap();
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

    // =========================================================================
    // quarto.version tests
    // =========================================================================

    #[test]
    fn test_quarto_version_is_table() {
        let lua = create_test_lua();
        let result: String = lua.load(r#"return type(quarto.version)"#).eval().unwrap();
        assert_eq!(result, "table");
    }

    #[test]
    fn test_quarto_version_concat() {
        let lua = create_test_lua();
        let result: String = lua
            .load(r#"return table.concat(quarto.version, '.')"#)
            .eval()
            .unwrap();
        assert_eq!(result, "0.1.0");
    }

    // =========================================================================
    // quarto.base64 tests
    // =========================================================================

    #[test]
    fn test_quarto_base64_encode_hello() {
        let lua = create_test_lua();
        let result: String = lua
            .load(r#"return quarto.base64.encode("hello")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "aGVsbG8=");
    }

    #[test]
    fn test_quarto_base64_encode_empty() {
        let lua = create_test_lua();
        let result: String = lua
            .load(r#"return quarto.base64.encode("")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "");
    }

    // =========================================================================
    // Script-dir stack tests
    // =========================================================================

    #[test]
    fn test_script_dir_stack_push_pop() {
        let lua = create_test_lua();
        assert_eq!(current_script_dir(&lua).unwrap(), "");

        push_script_dir(&lua, "/ext").unwrap();
        assert_eq!(current_script_dir(&lua).unwrap(), "/ext");

        push_script_dir(&lua, "/ext/helpers").unwrap();
        assert_eq!(current_script_dir(&lua).unwrap(), "/ext/helpers");

        pop_script_dir(&lua).unwrap();
        assert_eq!(current_script_dir(&lua).unwrap(), "/ext");

        pop_script_dir(&lua).unwrap();
        assert_eq!(current_script_dir(&lua).unwrap(), "");
    }

    #[test]
    fn test_script_dir_stack_resolve_path_uses_top() {
        let lua = create_test_lua();

        push_script_dir(&lua, "/ext").unwrap();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("style.css")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "/ext/style.css");

        // Push nested dir — resolve_path should use the new top
        push_script_dir(&lua, "/ext/helpers").unwrap();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("style.css")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "/ext/helpers/style.css");

        // Pop — back to /ext
        pop_script_dir(&lua).unwrap();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("style.css")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "/ext/style.css");
    }

    #[test]
    fn test_script_dir_stack_pop_on_empty_is_noop() {
        let lua = create_test_lua();
        // Should not error
        pop_script_dir(&lua).unwrap();
        assert_eq!(current_script_dir(&lua).unwrap(), "");
    }

    // =========================================================================
    // quarto.utils.as_inlines tests
    // =========================================================================

    #[test]
    fn test_as_inlines_from_string() {
        let lua = create_test_lua();
        let result: i64 = lua
            .load(
                r#"
                local inlines = quarto.utils.as_inlines("hello")
                return #inlines
                "#,
            )
            .eval()
            .unwrap();
        assert!(result > 0);
    }

    #[test]
    fn test_as_inlines_from_nil() {
        let lua = create_test_lua();
        let result: i64 = lua
            .load(
                r#"
                local inlines = quarto.utils.as_inlines(nil)
                return #inlines
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_as_inlines_from_inline() {
        let lua = create_test_lua();
        let result: String = lua
            .load(
                r#"
                local inlines = quarto.utils.as_inlines(pandoc.Str("test"))
                return pandoc.utils.stringify(inlines)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "test");
    }

    #[test]
    fn test_as_inlines_from_inlines_passthrough() {
        let lua = create_test_lua();
        let result: String = lua
            .load(
                r#"
                local orig = pandoc.Inlines({pandoc.Str("hello")})
                local inlines = quarto.utils.as_inlines(orig)
                return pandoc.utils.stringify(inlines)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_as_inlines_from_block() {
        let lua = create_test_lua();
        let result: String = lua
            .load(
                r#"
                local block = pandoc.Para({pandoc.Str("content")})
                local inlines = quarto.utils.as_inlines(block)
                return pandoc.utils.stringify(inlines)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "content");
    }

    #[test]
    fn test_as_inlines_from_blocks() {
        let lua = create_test_lua();
        let result: String = lua
            .load(
                r#"
                local blocks = pandoc.Blocks({pandoc.Para({pandoc.Str("a")}), pandoc.Para({pandoc.Str("b")})})
                local inlines = quarto.utils.as_inlines(blocks)
                return pandoc.utils.stringify(inlines)
                "#,
            )
            .eval()
            .unwrap();
        // blocks_to_inlines joins with LineBreak separator, stringify renders as space
        assert!(result.contains('a'));
        assert!(result.contains('b'));
    }

    #[test]
    fn test_as_inlines_from_table_of_inlines() {
        let lua = create_test_lua();
        let result: String = lua
            .load(
                r#"
                local t = {pandoc.Str("x"), pandoc.Space(), pandoc.Str("y")}
                local inlines = quarto.utils.as_inlines(t)
                return pandoc.utils.stringify(inlines)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "x y");
    }

    #[test]
    fn test_as_inlines_from_table_of_blocks() {
        let lua = create_test_lua();
        let result: String = lua
            .load(
                r#"
                local t = {pandoc.Para({pandoc.Str("block")})}
                local inlines = quarto.utils.as_inlines(t)
                return pandoc.utils.stringify(inlines)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "block");
    }

    // =========================================================================
    // quarto.utils.as_blocks tests
    // =========================================================================

    #[test]
    fn test_as_blocks_from_blocks_passthrough() {
        let lua = create_test_lua();
        let result: i64 = lua
            .load(
                r#"
                local blocks = pandoc.Blocks({pandoc.Para({pandoc.Str("a")})})
                local result = quarto.utils.as_blocks(blocks)
                return #result
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_as_blocks_from_block() {
        let lua = create_test_lua();
        let result: String = lua
            .load(
                r#"
                local block = pandoc.Para({pandoc.Str("single")})
                local blocks = quarto.utils.as_blocks(block)
                return pandoc.utils.stringify(blocks[1])
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "single");
    }

    #[test]
    fn test_as_blocks_from_inline() {
        let lua = create_test_lua();
        let result: String = lua
            .load(
                r#"
                local blocks = quarto.utils.as_blocks(pandoc.Str("text"))
                return pandoc.utils.stringify(blocks[1])
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "text");
    }

    #[test]
    fn test_as_blocks_from_nil() {
        let lua = create_test_lua();
        let result: i64 = lua
            .load(
                r#"
                local blocks = quarto.utils.as_blocks(nil)
                return #blocks
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 0);
    }
}

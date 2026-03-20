/*
 * lua/shortcode.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Lua shortcode engine for loading and dispatching shortcode handlers.
 *
 * This module is distinct from the filter engine — no AST traversal,
 * just function dispatch. A single LuaShortcodeEngine is created per
 * render and reused for all shortcode invocations in the document.
 */

use mlua::{Function, Lua, Result, Table, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::pandoc::{Block, Inline};

use super::constructors::register_pandoc_namespace;
use super::filter::{extract_lua_block, extract_lua_inline};
use super::mediabag::create_shared_mediabag;
use super::quarto_api::register_quarto_api;
use super::runtime::SystemRuntime;
use super::types::{LuaBlock, LuaInline};

/// Context in which a shortcode is being resolved.
/// Named `ShortcodeCallContext` to avoid conflict with the existing
/// `ShortcodeContext` struct in `shortcode_resolve.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcodeCallContext {
    Block,
    Inline,
    Text,
}

impl ShortcodeCallContext {
    fn as_str(&self) -> &'static str {
        match self {
            ShortcodeCallContext::Block => "block",
            ShortcodeCallContext::Inline => "inline",
            ShortcodeCallContext::Text => "text",
        }
    }
}

/// Result of calling a Lua shortcode handler.
#[derive(Debug)]
pub enum LuaShortcodeResult {
    Inlines(Vec<Inline>),
    Blocks(Vec<Block>),
    Text(String),
    Error(String),
}

/// Lua shortcode engine for loading and dispatching handlers.
///
/// This is `!Send + !Sync` because it holds a `Lua` state. It must only
/// be created as a local variable inside `ShortcodeResolveTransform::transform()`.
pub struct LuaShortcodeEngine {
    lua: Lua,
    handlers: HashMap<String, mlua::RegistryKey>,
    handler_script_dirs: HashMap<String, String>,
    runtime: Arc<dyn SystemRuntime>,
}

impl LuaShortcodeEngine {
    /// Create engine, set up Lua state with pandoc/quarto globals.
    pub fn new(
        target_format: &str,
        runtime: Arc<dyn SystemRuntime>,
    ) -> std::result::Result<Self, LuaShortcodeError> {
        #[cfg(target_arch = "wasm32")]
        let lua = {
            use mlua::StdLib;
            let libs =
                StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
            Lua::new_with(libs, mlua::LuaOptions::default()).map_err(LuaShortcodeError::LuaError)?
        };
        #[cfg(not(target_arch = "wasm32"))]
        let lua = Lua::new();

        let mediabag = create_shared_mediabag();
        register_pandoc_namespace(&lua, runtime.clone(), mediabag)
            .map_err(LuaShortcodeError::LuaError)?;

        // Register quarto.json, quarto.log, quarto.utils
        register_quarto_api(&lua).map_err(LuaShortcodeError::LuaError)?;

        lua.globals()
            .set("FORMAT", target_format)
            .map_err(LuaShortcodeError::LuaError)?;

        // Register quarto.shortcode sub-namespace
        register_shortcode_api(&lua).map_err(LuaShortcodeError::LuaError)?;

        Ok(Self {
            lua,
            handlers: HashMap::new(),
            handler_script_dirs: HashMap::new(),
            runtime,
        })
    }

    /// Load a shortcode Lua script. Registers all handlers it defines.
    /// Supports both return-table and environment-function conventions.
    pub fn load_script(
        &mut self,
        script_path: &Path,
    ) -> std::result::Result<(), LuaShortcodeError> {
        let script_bytes = self.runtime.file_read(script_path).map_err(|e| {
            LuaShortcodeError::FileReadError(
                script_path.to_owned(),
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            )
        })?;
        let script_source = String::from_utf8(script_bytes).map_err(|e| {
            LuaShortcodeError::FileReadError(
                script_path.to_owned(),
                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
            )
        })?;

        // Set script dir for quarto.utils.resolve_path
        let script_dir = script_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .to_string();
        self.lua
            .globals()
            .set("_quarto_script_dir", script_dir.as_str())
            .map_err(LuaShortcodeError::LuaError)?;

        // Execute script in a sandboxed environment that inherits globals
        let env = self
            .lua
            .create_table()
            .map_err(LuaShortcodeError::LuaError)?;
        let env_mt = self
            .lua
            .create_table()
            .map_err(LuaShortcodeError::LuaError)?;
        env_mt
            .set("__index", self.lua.globals())
            .map_err(LuaShortcodeError::LuaError)?;
        env.set_metatable(Some(env_mt))
            .map_err(LuaShortcodeError::LuaError)?;

        let chunk = self
            .lua
            .load(&script_source)
            .set_name(script_path.to_string_lossy())
            .set_environment(env.clone());

        let ret: Value = chunk.eval().map_err(LuaShortcodeError::LuaError)?;

        // Convention 1: script returns a table of handlers
        if let Value::Table(ref table) = ret {
            for pair in table.pairs::<String, Value>() {
                let (name, value) = pair.map_err(LuaShortcodeError::LuaError)?;
                if is_callable(&value) {
                    let key = self
                        .lua
                        .create_registry_value(value)
                        .map_err(LuaShortcodeError::LuaError)?;
                    self.handler_script_dirs
                        .insert(name.clone(), script_dir.clone());
                    self.handlers.insert(name, key);
                }
            }
            return Ok(());
        }

        // Convention 2: scan environment for callable values
        for pair in env.pairs::<String, Value>() {
            let (name, value) = pair.map_err(LuaShortcodeError::LuaError)?;
            if is_callable(&value) {
                // Skip globals that were inherited (only register new ones)
                if let Ok(global_val) = self.lua.globals().get::<Value>(name.as_str()) {
                    if same_lua_value(&value, &global_val) {
                        continue;
                    }
                }
                let key = self
                    .lua
                    .create_registry_value(value)
                    .map_err(LuaShortcodeError::LuaError)?;
                self.handler_script_dirs
                    .insert(name.clone(), script_dir.clone());
                self.handlers.insert(name, key);
            }
        }

        Ok(())
    }

    /// Call a named shortcode handler.
    /// Returns None if no handler is registered for the name.
    pub fn call(
        &self,
        name: &str,
        args: &ShortcodeArgs,
        context: ShortcodeCallContext,
    ) -> Option<LuaShortcodeResult> {
        let reg_key = self.handlers.get(name)?;
        let func: Function = self.lua.registry_value(reg_key).ok()?;

        // Set script dir for this handler's extension
        if let Some(dir) = self.handler_script_dirs.get(name) {
            let _ = self.lua.globals().set("_quarto_script_dir", dir.as_str());
        }

        Some(self.call_handler(name, func, args, context))
    }

    /// Check if a handler is registered for the given name.
    pub fn has_handler(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    fn call_handler(
        &self,
        name: &str,
        func: Function,
        args: &ShortcodeArgs,
        context: ShortcodeCallContext,
    ) -> LuaShortcodeResult {
        match self.build_and_call(func, args, context) {
            Ok(result) => result,
            Err(e) => {
                LuaShortcodeResult::Error(format!("Shortcode '{}' handler error: {}", name, e))
            }
        }
    }

    fn build_and_call(
        &self,
        func: Function,
        args: &ShortcodeArgs,
        context: ShortcodeCallContext,
    ) -> std::result::Result<LuaShortcodeResult, mlua::Error> {
        // Build args table: pandoc.List of {value = string} or {name = key, value = string}
        let lua_args = self.build_args_table(args)?;

        // Build kwargs table
        let lua_kwargs = self.build_kwargs_table(args)?;

        // Build meta proxy table
        let lua_meta = self.build_meta_table(args)?;

        // Build raw_args list
        let lua_raw_args = self.build_raw_args(args)?;

        // Context string
        let ctx_str = context.as_str();

        let ret: Value = func.call((lua_args, lua_kwargs, lua_meta, lua_raw_args, ctx_str))?;

        Ok(convert_return_value(&self.lua, ret))
    }

    fn build_args_table(&self, args: &ShortcodeArgs) -> Result<Value> {
        let table = self.lua.create_table()?;
        let mut idx = 1;
        for arg in &args.positional {
            let entry = self.lua.create_table()?;
            entry.set("value", arg.as_str())?;
            table.set(idx, entry)?;
            idx += 1;
        }
        for (key, val) in &args.keyword {
            let entry = self.lua.create_table()?;
            entry.set("name", key.as_str())?;
            entry.set("value", val.as_str())?;
            table.set(idx, entry)?;
            idx += 1;
        }
        Ok(Value::Table(table))
    }

    fn build_kwargs_table(&self, args: &ShortcodeArgs) -> Result<Value> {
        let table = self.lua.create_table()?;
        for (key, val) in &args.keyword {
            table.set(key.as_str(), val.as_str())?;
        }
        Ok(Value::Table(table))
    }

    fn build_meta_table(&self, args: &ShortcodeArgs) -> Result<Value> {
        let table = self.lua.create_table()?;
        for (key, val) in &args.metadata {
            table.set(key.as_str(), val.as_str())?;
        }
        Ok(Value::Table(table))
    }

    fn build_raw_args(&self, args: &ShortcodeArgs) -> Result<Value> {
        let table = self.lua.create_table()?;
        for (i, arg) in args.positional.iter().enumerate() {
            table.set(i + 1, arg.as_str())?;
        }
        Ok(Value::Table(table))
    }
}

/// Arguments prepared for a shortcode handler call.
/// This is a simplified representation that the caller builds from
/// the Shortcode struct and document metadata.
pub struct ShortcodeArgs {
    pub positional: Vec<String>,
    pub keyword: Vec<(String, String)>,
    pub metadata: Vec<(String, String)>,
}

/// Errors from the shortcode engine.
#[derive(Debug)]
pub enum LuaShortcodeError {
    FileReadError(std::path::PathBuf, std::io::Error),
    LuaError(mlua::Error),
}

impl std::fmt::Display for LuaShortcodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LuaShortcodeError::FileReadError(path, err) => {
                write!(
                    f,
                    "Failed to read shortcode script '{}': {}",
                    path.display(),
                    err
                )
            }
            LuaShortcodeError::LuaError(err) => {
                write!(f, "Lua shortcode error: {}", err)
            }
        }
    }
}

impl std::error::Error for LuaShortcodeError {}

/// Register `quarto.shortcode` sub-namespace with helper functions.
fn register_shortcode_api(lua: &Lua) -> Result<()> {
    let quarto: Table = lua.globals().get("quarto")?;

    let shortcode_ns = lua.create_table()?;

    // quarto.shortcode.read_arg(args, n)
    shortcode_ns.set(
        "read_arg",
        lua.create_function(|_lua, (args, n): (Table, usize)| -> Result<Value> {
            // 1-based index
            let entry: Value = args.get(n)?;
            match entry {
                Value::Table(t) => t.get("value"),
                Value::Nil => Ok(Value::Nil),
                other => Ok(other),
            }
        })?,
    )?;

    // quarto.shortcode.error_output(name, message, context)
    shortcode_ns.set(
        "error_output",
        lua.create_function(
            |lua, (name, message, context): (String, String, String)| -> Result<Value> {
                let err_text = format!("[Shortcode Error ({}): {}]", name, message);
                let make_strong_inline = |text: String| -> Inline {
                    Inline::Strong(crate::pandoc::Strong {
                        content: vec![Inline::Str(crate::pandoc::Str {
                            text,
                            source_info: Default::default(),
                        })],
                        source_info: Default::default(),
                    })
                };
                match context.as_str() {
                    "block" => {
                        let para = lua.create_userdata(LuaBlock(Block::Paragraph(
                            crate::pandoc::Paragraph {
                                content: vec![make_strong_inline(err_text)],
                                source_info: Default::default(),
                            },
                        )))?;
                        Ok(Value::UserData(para))
                    }
                    "inline" => {
                        let strong =
                            lua.create_userdata(LuaInline(make_strong_inline(err_text)))?;
                        Ok(Value::UserData(strong))
                    }
                    _ => Ok(Value::String(lua.create_string(&err_text)?)),
                }
            },
        )?,
    )?;

    quarto.set("shortcode", shortcode_ns)?;
    Ok(())
}

/// Convert a Lua return value to a LuaShortcodeResult.
fn convert_return_value(_lua: &Lua, ret: Value) -> LuaShortcodeResult {
    match ret {
        Value::Nil => LuaShortcodeResult::Error("Shortcode returned nil".to_string()),
        Value::String(s) => {
            LuaShortcodeResult::Text(s.to_str().map(|s| s.to_string()).unwrap_or_default())
        }
        Value::UserData(ud) => {
            if let Ok(inline) = extract_lua_inline(&ud) {
                LuaShortcodeResult::Inlines(vec![inline])
            } else if let Ok(block) = extract_lua_block(&ud) {
                LuaShortcodeResult::Blocks(vec![block])
            } else {
                LuaShortcodeResult::Error(
                    "Shortcode returned unsupported userdata type".to_string(),
                )
            }
        }
        Value::Table(table) => classify_table_result(&table),
        _ => LuaShortcodeResult::Error("Shortcode returned unsupported type".to_string()),
    }
}

/// Classify a table return as Inlines or Blocks.
fn classify_table_result(table: &mlua::Table) -> LuaShortcodeResult {
    let len = table.raw_len();
    if len == 0 {
        return LuaShortcodeResult::Inlines(vec![]);
    }

    let mut inlines = Vec::new();
    let mut blocks = Vec::new();
    let mut has_blocks = false;

    for i in 1..=len {
        let value: std::result::Result<Value, _> = table.get(i);
        match value {
            Ok(Value::UserData(ud)) => {
                if let Ok(inline) = extract_lua_inline(&ud) {
                    inlines.push(inline);
                } else if let Ok(block) = extract_lua_block(&ud) {
                    has_blocks = true;
                    blocks.push(block);
                }
            }
            _ => {}
        }
    }

    if has_blocks {
        LuaShortcodeResult::Blocks(blocks)
    } else if !inlines.is_empty() {
        LuaShortcodeResult::Inlines(inlines)
    } else {
        LuaShortcodeResult::Error(
            "Shortcode returned table with no recognizable elements".to_string(),
        )
    }
}

fn is_callable(value: &Value) -> bool {
    matches!(value, Value::Function(_))
}

/// Check if two Lua values are the same reference.
fn same_lua_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Function(fa), Value::Function(fb)) => {
            // Compare by pointer identity
            fa == fb
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::runtime::NativeRuntime;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_runtime() -> Arc<dyn SystemRuntime> {
        Arc::new(NativeRuntime::new())
    }

    fn write_script(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn make_empty_args() -> ShortcodeArgs {
        ShortcodeArgs {
            positional: vec![],
            keyword: vec![],
            metadata: vec![],
        }
    }

    #[test]
    fn test_load_script_return_table() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "hello.lua",
            r#"
return {
    hello = function(args, kwargs, meta, raw_args, context)
        return pandoc.Str("hello-world")
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();
        assert!(engine.has_handler("hello"));
    }

    #[test]
    fn test_load_script_env_function() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "hello.lua",
            r#"
function hello(args, kwargs, meta, raw_args, context)
    return pandoc.Str("hello-world")
end
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();
        assert!(engine.has_handler("hello"));
    }

    #[test]
    fn test_call_returns_inlines() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "hello.lua",
            r#"
return {
    hello = function(args, kwargs, meta, raw_args, context)
        return pandoc.Inlines{pandoc.Str("hi")}
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let result = engine
            .call("hello", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Inlines(inlines) => {
                assert_eq!(inlines.len(), 1);
                match &inlines[0] {
                    Inline::Str(s) => assert_eq!(s.text, "hi"),
                    other => panic!("Expected Str, got {:?}", other),
                }
            }
            other => panic!("Expected Inlines, got {:?}", other),
        }
    }

    #[test]
    fn test_call_returns_blocks() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "brk.lua",
            r#"
return {
    brk = function(args, kwargs, meta, raw_args, context)
        return pandoc.RawBlock("html", "<hr>")
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let result = engine
            .call("brk", &make_empty_args(), ShortcodeCallContext::Block)
            .unwrap();
        match result {
            LuaShortcodeResult::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    Block::RawBlock(rb) => {
                        assert_eq!(rb.format, "html");
                        assert_eq!(rb.text, "<hr>");
                    }
                    other => panic!("Expected RawBlock, got {:?}", other),
                }
            }
            other => panic!("Expected Blocks, got {:?}", other),
        }
    }

    #[test]
    fn test_call_returns_string() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "ver.lua",
            r#"
return {
    ver = function(args, kwargs, meta, raw_args, context)
        return "1.0.0"
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let result = engine
            .call("ver", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "1.0.0"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_call_returns_nil() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "bad.lua",
            r#"
return {
    bad = function(args, kwargs, meta, raw_args, context)
        return nil
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let result = engine
            .call("bad", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Error(msg) => {
                assert!(msg.contains("nil"), "Expected nil error, got: {}", msg);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_call_unknown_handler() {
        let runtime = make_runtime();
        let engine = LuaShortcodeEngine::new("html", runtime).unwrap();

        let result = engine.call(
            "nonexistent",
            &make_empty_args(),
            ShortcodeCallContext::Inline,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_handler_receives_args() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "echo.lua",
            r#"
return {
    echo = function(args, kwargs, meta, raw_args, context)
        return args[1].value
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let args = ShortcodeArgs {
            positional: vec!["world".to_string()],
            keyword: vec![],
            metadata: vec![],
        };
        let result = engine
            .call("echo", &args, ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "world"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_handler_receives_kwargs() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "kwarg.lua",
            r#"
return {
    kwarg = function(args, kwargs, meta, raw_args, context)
        return kwargs.greeting
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let args = ShortcodeArgs {
            positional: vec![],
            keyword: vec![("greeting".to_string(), "howdy".to_string())],
            metadata: vec![],
        };
        let result = engine
            .call("kwarg", &args, ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "howdy"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_handler_receives_meta() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "meta.lua",
            r#"
return {
    meta_reader = function(args, kwargs, meta, raw_args, context)
        return meta.title
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let args = ShortcodeArgs {
            positional: vec![],
            keyword: vec![],
            metadata: vec![("title".to_string(), "My Doc".to_string())],
        };
        let result = engine
            .call("meta_reader", &args, ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "My Doc"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_handler_receives_context() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "ctx.lua",
            r#"
return {
    ctx = function(args, kwargs, meta, raw_args, context)
        return context
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let result = engine
            .call("ctx", &make_empty_args(), ShortcodeCallContext::Block)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "block"),
            other => panic!("Expected Text('block'), got {:?}", other),
        }

        let result = engine
            .call("ctx", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "inline"),
            other => panic!("Expected Text('inline'), got {:?}", other),
        }

        let result = engine
            .call("ctx", &make_empty_args(), ShortcodeCallContext::Text)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "text"),
            other => panic!("Expected Text('text'), got {:?}", other),
        }
    }

    #[test]
    fn test_later_script_overrides_earlier() {
        let tmp = TempDir::new().unwrap();
        let script1 = write_script(
            tmp.path(),
            "first.lua",
            r#"
return {
    greeting = function(args, kwargs, meta, raw_args, context)
        return "first"
    end
}
"#,
        );
        let script2 = write_script(
            tmp.path(),
            "second.lua",
            r#"
return {
    greeting = function(args, kwargs, meta, raw_args, context)
        return "second"
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script1).unwrap();
        engine.load_script(&script2).unwrap();

        let result = engine
            .call("greeting", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "second"),
            other => panic!("Expected Text('second'), got {:?}", other),
        }
    }

    #[test]
    fn test_read_arg_helper() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "readarg.lua",
            r#"
return {
    readarg = function(args, kwargs, meta, raw_args, context)
        local val = quarto.shortcode.read_arg(args, 1)
        return val
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let args = ShortcodeArgs {
            positional: vec!["test-value".to_string()],
            keyword: vec![],
            metadata: vec![],
        };
        let result = engine
            .call("readarg", &args, ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "test-value"),
            other => panic!("Expected Text('test-value'), got {:?}", other),
        }
    }

    #[test]
    fn test_shortcode_resolve_path() {
        let tmp = TempDir::new().unwrap();
        // Write a data file next to the script
        std::fs::write(tmp.path().join("data.json"), r#"{"key":"value"}"#).unwrap();

        let script = write_script(
            tmp.path(),
            "resolver.lua",
            r#"
return {
    resolver = function(args, kwargs, meta, raw_args, context)
        return quarto.utils.resolve_path("data.json")
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let result = engine
            .call("resolver", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => {
                let expected = tmp.path().join("data.json").to_string_lossy().to_string();
                assert_eq!(s, expected);
            }
            other => panic!("Expected Text with resolved path, got {:?}", other),
        }
    }

    #[test]
    fn test_shortcode_quarto_json() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "jsontest.lua",
            r#"
return {
    jsontest = function(args, kwargs, meta, raw_args, context)
        local t = quarto.json.decode('{"x":42}')
        return tostring(t.x)
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let result = engine
            .call("jsontest", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "42"),
            other => panic!("Expected Text('42'), got {:?}", other),
        }
    }

    #[test]
    fn test_shortcode_quarto_log() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "logtest.lua",
            r#"
return {
    logtest = function(args, kwargs, meta, raw_args, context)
        quarto.log.warning("test warning from shortcode")
        return "ok"
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).unwrap();

        let result = engine
            .call("logtest", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "ok"),
            other => panic!("Expected Text('ok'), got {:?}", other),
        }
    }

    #[test]
    fn test_shortcode_resolve_path_multi_extension() {
        // Test that script dirs are tracked per handler, not globally
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();

        let script1 = write_script(
            tmp1.path(),
            "ext1.lua",
            r#"
return {
    ext1 = function(args, kwargs, meta, raw_args, context)
        return quarto.utils.resolve_path("data1.json")
    end
}
"#,
        );

        let script2 = write_script(
            tmp2.path(),
            "ext2.lua",
            r#"
return {
    ext2 = function(args, kwargs, meta, raw_args, context)
        return quarto.utils.resolve_path("data2.json")
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script1).unwrap();
        engine.load_script(&script2).unwrap();

        // ext1 should resolve relative to tmp1
        let result1 = engine
            .call("ext1", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result1 {
            LuaShortcodeResult::Text(s) => {
                let expected = tmp1.path().join("data1.json").to_string_lossy().to_string();
                assert_eq!(s, expected);
            }
            other => panic!("Expected Text for ext1, got {:?}", other),
        }

        // ext2 should resolve relative to tmp2
        let result2 = engine
            .call("ext2", &make_empty_args(), ShortcodeCallContext::Inline)
            .unwrap();
        match result2 {
            LuaShortcodeResult::Text(s) => {
                let expected = tmp2.path().join("data2.json").to_string_lossy().to_string();
                assert_eq!(s, expected);
            }
            other => panic!("Expected Text for ext2, got {:?}", other),
        }
    }
}

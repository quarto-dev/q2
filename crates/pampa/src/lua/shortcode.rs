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
use quarto_source_map::{By, SourceInfo};
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
            let lua = Lua::new_with(libs, mlua::LuaOptions::default())
                .map_err(LuaShortcodeError::LuaError)?;
            super::os_wasm::register_wasm_os(&lua, runtime.clone())
                .map_err(LuaShortcodeError::LuaError)?;
            super::io_wasm::register_wasm_io(&lua, runtime.clone())
                .map_err(LuaShortcodeError::LuaError)?;
            super::dofile_wasm::register_wasm_dofile(&lua, runtime.clone())
                .map_err(LuaShortcodeError::LuaError)?;
            lua
        };
        #[cfg(not(target_arch = "wasm32"))]
        let lua = Lua::new();

        let mediabag = create_shared_mediabag();
        register_pandoc_namespace(&lua, runtime.clone(), mediabag)
            .map_err(LuaShortcodeError::LuaError)?;

        // Register quarto.json, quarto.log, quarto.utils
        register_quarto_api(&lua).map_err(LuaShortcodeError::LuaError)?;

        // Register quarto.doc namespace (is_format, add_html_dependency, etc.)
        super::quarto_doc::register_quarto_doc(&lua).map_err(LuaShortcodeError::LuaError)?;

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
    pub async fn load_script(
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

        // Push script dir for quarto.utils.resolve_path
        let script_dir = script_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .to_string();
        super::quarto_api::push_script_dir(&self.lua, &script_dir)
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

        let ret: Value = chunk
            .eval_async()
            .await
            .map_err(LuaShortcodeError::LuaError)?;

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
    pub async fn call(
        &self,
        name: &str,
        args: &ShortcodeArgs,
        context: ShortcodeCallContext,
    ) -> Option<LuaShortcodeResult> {
        let reg_key = self.handlers.get(name)?;
        let func: Function = self.lua.registry_value(reg_key).ok()?;

        // Push script dir for this handler's extension, pop after call
        if let Some(dir) = self.handler_script_dirs.get(name) {
            let _ = super::quarto_api::push_script_dir(&self.lua, dir);
        }

        let result = self.call_handler(name, func, args, context).await;

        if self.handler_script_dirs.contains_key(name) {
            let _ = super::quarto_api::pop_script_dir(&self.lua);
        }

        Some(result)
    }

    /// Check if a handler is registered for the given name.
    pub fn has_handler(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    /// Extract diagnostics collected during shortcode execution.
    pub fn extract_diagnostics(
        &self,
    ) -> std::result::Result<Vec<quarto_error_reporting::DiagnosticMessage>, LuaShortcodeError>
    {
        super::diagnostics::extract_lua_diagnostics(&self.lua).map_err(LuaShortcodeError::LuaError)
    }

    /// Extract HTML dependencies collected during shortcode execution.
    pub fn extract_html_dependencies(
        &self,
    ) -> std::result::Result<Vec<super::quarto_doc::HtmlDependency>, LuaShortcodeError> {
        super::quarto_doc::extract_html_dependencies(&self.lua).map_err(LuaShortcodeError::LuaError)
    }

    /// Extract text includes collected during shortcode execution.
    pub fn extract_text_includes(
        &self,
    ) -> std::result::Result<Vec<super::quarto_doc::TextInclude>, LuaShortcodeError> {
        super::quarto_doc::extract_text_includes(&self.lua).map_err(LuaShortcodeError::LuaError)
    }

    async fn call_handler(
        &self,
        name: &str,
        func: Function,
        args: &ShortcodeArgs,
        context: ShortcodeCallContext,
    ) -> LuaShortcodeResult {
        match self.build_and_call(func, args, context).await {
            Ok(result) => result,
            Err(e) => {
                LuaShortcodeResult::Error(format!("Shortcode '{}' handler error: {}", name, e))
            }
        }
    }

    async fn build_and_call(
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

        let ret: Value = func
            .call_async((lua_args, lua_kwargs, lua_meta, lua_raw_args, ctx_str))
            .await?;

        Ok(convert_return_value(&self.lua, ret))
    }

    fn build_args_table(&self, args: &ShortcodeArgs) -> Result<Value> {
        let table = self.lua.create_table()?;
        for (i, arg) in args.positional.iter().enumerate() {
            table.set(i + 1, arg.as_str())?;
        }
        Ok(Value::Table(table))
    }

    fn build_kwargs_table(&self, args: &ShortcodeArgs) -> Result<Value> {
        let table = self.lua.create_table()?;
        for (key, val) in &args.keyword {
            table.set(key.as_str(), val.as_str())?;
        }
        // TS Quarto compat: missing keys return empty pandoc.Inlines({})
        let mt = self.lua.create_table()?;
        mt.set(
            "__index",
            self.lua
                .load("function(t, k) return pandoc.Inlines({}) end")
                .eval::<Function>()?,
        )?;
        table.set_metatable(Some(mt))?;
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

    // quarto.shortcode.read_arg(args, n) — TS Quarto compat
    shortcode_ns.set(
        "read_arg",
        lua.load(
            r#"
            function(args, n)
                local arg = args[n or 1]
                if arg == nil then return nil end
                if type(arg) ~= "string" then
                    return pandoc.utils.stringify(arg)
                end
                return arg
            end
            "#,
        )
        .eval::<Function>()?,
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
                            source_info: SourceInfo::generated(By::unknown()),
                        })],
                        source_info: SourceInfo::generated(By::unknown()),
                    })
                };
                match context.as_str() {
                    "block" => {
                        let para = lua.create_userdata(LuaBlock::new(Block::Paragraph(
                            crate::pandoc::Paragraph {
                                content: vec![make_strong_inline(err_text)],
                                source_info: SourceInfo::generated(By::unknown()),
                            },
                        )))?;
                        Ok(Value::UserData(para))
                    }
                    "inline" => {
                        let strong =
                            lua.create_userdata(LuaInline::new(make_strong_inline(err_text)))?;
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

    #[tokio::test]
    async fn test_load_script_return_table() {
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
        engine.load_script(&script).await.unwrap();
        assert!(engine.has_handler("hello"));
    }

    #[tokio::test]
    async fn test_load_script_env_function() {
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
        engine.load_script(&script).await.unwrap();
        assert!(engine.has_handler("hello"));
    }

    #[tokio::test]
    async fn test_call_returns_inlines() {
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
        engine.load_script(&script).await.unwrap();

        let result = engine
            .call("hello", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
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

    #[tokio::test]
    async fn test_call_returns_blocks() {
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
        engine.load_script(&script).await.unwrap();

        let result = engine
            .call("brk", &make_empty_args(), ShortcodeCallContext::Block)
            .await
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

    #[tokio::test]
    async fn test_call_returns_string() {
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
        engine.load_script(&script).await.unwrap();

        let result = engine
            .call("ver", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "1.0.0"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_call_returns_nil() {
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
        engine.load_script(&script).await.unwrap();

        let result = engine
            .call("bad", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Error(msg) => {
                assert!(msg.contains("nil"), "Expected nil error, got: {}", msg);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_call_unknown_handler() {
        let runtime = make_runtime();
        let engine = LuaShortcodeEngine::new("html", runtime).unwrap();

        let result = engine
            .call(
                "nonexistent",
                &make_empty_args(),
                ShortcodeCallContext::Inline,
            )
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_handler_receives_args() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "echo.lua",
            r#"
return {
    echo = function(args, kwargs, meta, raw_args, context)
        return args[1]
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec!["world".to_string()],
            keyword: vec![],
            metadata: vec![],
        };
        let result = engine
            .call("echo", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "world"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_handler_receives_kwargs() {
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
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec![],
            keyword: vec![("greeting".to_string(), "howdy".to_string())],
            metadata: vec![],
        };
        let result = engine
            .call("kwarg", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "howdy"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_handler_receives_meta() {
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
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec![],
            keyword: vec![],
            metadata: vec![("title".to_string(), "My Doc".to_string())],
        };
        let result = engine
            .call("meta_reader", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "My Doc"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_handler_receives_context() {
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
        engine.load_script(&script).await.unwrap();

        let result = engine
            .call("ctx", &make_empty_args(), ShortcodeCallContext::Block)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "block"),
            other => panic!("Expected Text('block'), got {:?}", other),
        }

        let result = engine
            .call("ctx", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "inline"),
            other => panic!("Expected Text('inline'), got {:?}", other),
        }

        let result = engine
            .call("ctx", &make_empty_args(), ShortcodeCallContext::Text)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "text"),
            other => panic!("Expected Text('text'), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_later_script_overrides_earlier() {
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
        engine.load_script(&script1).await.unwrap();
        engine.load_script(&script2).await.unwrap();

        let result = engine
            .call("greeting", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "second"),
            other => panic!("Expected Text('second'), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_read_arg_helper() {
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
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec!["test-value".to_string()],
            keyword: vec![],
            metadata: vec![],
        };
        let result = engine
            .call("readarg", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "test-value"),
            other => panic!("Expected Text('test-value'), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_shortcode_resolve_path() {
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
        engine.load_script(&script).await.unwrap();

        let result = engine
            .call("resolver", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => {
                let expected = tmp.path().join("data.json").to_string_lossy().to_string();
                assert_eq!(s, expected);
            }
            other => panic!("Expected Text with resolved path, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_shortcode_quarto_json() {
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
        engine.load_script(&script).await.unwrap();

        let result = engine
            .call("jsontest", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "42"),
            other => panic!("Expected Text('42'), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_shortcode_quarto_log() {
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
        engine.load_script(&script).await.unwrap();

        let result = engine
            .call("logtest", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "ok"),
            other => panic!("Expected Text('ok'), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_shortcode_resolve_path_multi_extension() {
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
        engine.load_script(&script1).await.unwrap();
        engine.load_script(&script2).await.unwrap();

        // ext1 should resolve relative to tmp1
        let result1 = engine
            .call("ext1", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
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
            .await
            .unwrap();
        match result2 {
            LuaShortcodeResult::Text(s) => {
                let expected = tmp2.path().join("data2.json").to_string_lossy().to_string();
                assert_eq!(s, expected);
            }
            other => panic!("Expected Text for ext2, got {:?}", other),
        }
    }

    // --- Phase 1: TS Quarto compat tests (should fail before fix) ---

    #[tokio::test]
    async fn test_args_are_plain_strings() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "stringify_arg.lua",
            r#"
return {
    stringify_arg = function(args, kwargs, meta, raw_args, context)
        return pandoc.utils.stringify(args[1])
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec!["5".to_string()],
            keyword: vec![],
            metadata: vec![],
        };
        let result = engine
            .call("stringify_arg", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "5"),
            other => panic!("Expected Text(\"5\"), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_args_exclude_kwargs() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "count_args.lua",
            r#"
return {
    count_args = function(args, kwargs, meta, raw_args, context)
        return tostring(#args)
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec!["hello".to_string()],
            keyword: vec![("key".to_string(), "val".to_string())],
            metadata: vec![],
        };
        let result = engine
            .call("count_args", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "1", "kwargs should not be in args"),
            other => panic!("Expected Text(\"1\"), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_kwargs_missing_key_returns_inlines() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "kwargs_type.lua",
            r#"
return {
    kwargs_type = function(args, kwargs, meta, raw_args, context)
        local val = kwargs["nonexistent"]
        return tostring(type(val))
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        let result = engine
            .call(
                "kwargs_type",
                &make_empty_args(),
                ShortcodeCallContext::Inline,
            )
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => {
                // pandoc.Inlines({}) returns a table in our Lua env
                assert_ne!(s, "nil", "missing kwargs should not return nil");
                assert!(
                    s == "table" || s == "userdata",
                    "missing kwargs should return Inlines (table or userdata), got: {s}"
                )
            }
            other => panic!("Expected Text(\"userdata\"), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_stringify_args_lipsum_pattern() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "lipsum_pattern.lua",
            r#"
return {
    lipsum_pattern = function(args, kwargs, meta, raw_args, context)
        local range = pandoc.utils.stringify(args[1])
        local _, _, bareVal = range:find("^(%d+)$")
        if bareVal then
            return bareVal
        else
            return "NO_MATCH"
        end
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec!["3".to_string()],
            keyword: vec![],
            metadata: vec![],
        };
        let result = engine
            .call("lipsum_pattern", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "3"),
            other => panic!("Expected Text(\"3\"), got {:?}", other),
        }
    }

    /// Test the fontawesome/unsplash pattern: stringify a kwarg value.
    #[tokio::test]
    async fn test_kwargs_stringify_value() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "fa.lua",
            r#"
return {
    fa = function(args, kwargs, meta, raw_args, context)
        local title = pandoc.utils.stringify(kwargs["title"])
        if title ~= "" then
            return title
        end
        return "NO_TITLE"
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec!["icon-name".to_string()],
            keyword: vec![("title".to_string(), "My Icon".to_string())],
            metadata: vec![],
        };
        let result = engine
            .call("fa", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "My Icon"),
            other => panic!("Expected Text(\"My Icon\"), got {:?}", other),
        }
    }

    /// Test the unsplash pattern: kwargs missing key with length check.
    /// In TS Quarto, missing kwargs return pandoc.Inlines({}), which has
    /// length 0. Extensions check `kwargs['key'] ~= nil and #kwargs['key'] > 0`.
    #[tokio::test]
    async fn test_kwargs_missing_key_length_check() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "unsplash.lua",
            r#"
return {
    unsplash = function(args, kwargs, meta, raw_args, context)
        local width = nil
        if kwargs['width'] ~= nil and #kwargs['width'] > 0 then
            width = pandoc.utils.stringify(kwargs['width'])
        end
        if width then
            return width
        end
        return "DEFAULT"
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        // With width kwarg present
        let args_with = ShortcodeArgs {
            positional: vec![],
            keyword: vec![("width".to_string(), "800".to_string())],
            metadata: vec![],
        };
        let result = engine
            .call("unsplash", &args_with, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "800"),
            other => panic!("Expected Text(\"800\"), got {:?}", other),
        }

        // Without width kwarg — missing key should have length 0
        let args_without = ShortcodeArgs {
            positional: vec![],
            keyword: vec![],
            metadata: vec![],
        };
        let result = engine
            .call("unsplash", &args_without, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "DEFAULT"),
            other => panic!("Expected Text(\"DEFAULT\"), got {:?}", other),
        }
    }

    /// Test meta parameter access pattern: handler reads document metadata.
    #[tokio::test]
    async fn test_meta_access_pattern() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "meta_sc.lua",
            r#"
return {
    meta_sc = function(args, kwargs, meta, raw_args, context)
        local title = meta["title"]
        local author = meta["author"]
        if title and author then
            return title .. " by " .. author
        elseif title then
            return title
        end
        return "NO_META"
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec![],
            keyword: vec![],
            metadata: vec![
                ("title".to_string(), "My Document".to_string()),
                ("author".to_string(), "Jane".to_string()),
            ],
        };
        let result = engine
            .call("meta_sc", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "My Document by Jane"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    /// Test raw_args parameter: handlers can access the unparsed argument strings.
    #[tokio::test]
    async fn test_raw_args_access() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "raw.lua",
            r#"
return {
    raw = function(args, kwargs, meta, raw_args, context)
        local parts = {}
        for i, v in ipairs(raw_args) do
            parts[i] = v
        end
        return table.concat(parts, ",")
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        let args = ShortcodeArgs {
            positional: vec!["hello".to_string(), "world".to_string()],
            keyword: vec![],
            metadata: vec![],
        };
        let result = engine
            .call("raw", &args, ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "hello,world"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    /// Test context parameter with block output: handler returns different
    /// types based on context string.
    #[tokio::test]
    async fn test_context_driven_output() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "ctx_out.lua",
            r#"
return {
    ctx_out = function(args, kwargs, meta, raw_args, context)
        if context == "block" then
            return pandoc.Para({pandoc.Str("block-output")})
        elseif context == "inline" then
            return pandoc.Str("inline-output")
        else
            return "text-output"
        end
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        // Block context → Para block
        let result = engine
            .call("ctx_out", &make_empty_args(), ShortcodeCallContext::Block)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    Block::Paragraph(p) => match &p.content[0] {
                        Inline::Str(s) => assert_eq!(s.text, "block-output"),
                        other => panic!("Expected Str, got {:?}", other),
                    },
                    other => panic!("Expected Paragraph, got {:?}", other),
                }
            }
            other => panic!("Expected Blocks, got {:?}", other),
        }

        // Inline context → Str inline
        let result = engine
            .call("ctx_out", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Inlines(inlines) => {
                assert_eq!(inlines.len(), 1);
                match &inlines[0] {
                    Inline::Str(s) => assert_eq!(s.text, "inline-output"),
                    other => panic!("Expected Str, got {:?}", other),
                }
            }
            other => panic!("Expected Inlines, got {:?}", other),
        }

        // Text context → plain string
        let result = engine
            .call("ctx_out", &make_empty_args(), ShortcodeCallContext::Text)
            .await
            .unwrap();
        match result {
            LuaShortcodeResult::Text(s) => assert_eq!(s, "text-output"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    // =====================================================================
    // Extraction tests (Phase 4)
    // =====================================================================

    #[tokio::test]
    async fn test_shortcode_extract_html_dependencies() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "dep_sc.lua",
            r#"
return {
    dep_sc = function(args, kwargs, meta, raw_args, context)
        quarto.doc.add_html_dependency({
            name = "my-dep",
            stylesheets = {"style.css"},
            scripts = {"script.js"},
        })
        return "ok"
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        engine
            .call("dep_sc", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();

        let deps = engine.extract_html_dependencies().unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "my-dep");
        assert_eq!(deps[0].stylesheets.len(), 1);
        assert_eq!(deps[0].scripts.len(), 1);
    }

    #[tokio::test]
    async fn test_shortcode_extract_diagnostics() {
        let tmp = TempDir::new().unwrap();
        let script = write_script(
            tmp.path(),
            "warn_sc.lua",
            r#"
return {
    warn_sc = function(args, kwargs, meta, raw_args, context)
        quarto.warn("test warning from shortcode")
        return "ok"
    end
}
"#,
        );

        let runtime = make_runtime();
        let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
        engine.load_script(&script).await.unwrap();

        engine
            .call("warn_sc", &make_empty_args(), ShortcodeCallContext::Inline)
            .await
            .unwrap();

        let diagnostics = engine.extract_diagnostics().unwrap();
        assert_eq!(diagnostics.len(), 1);
    }
}

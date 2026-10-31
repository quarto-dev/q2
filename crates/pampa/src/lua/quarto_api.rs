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
    register_quarto_component(lua, &quarto)?;
    register_quarto_markdown(lua, &quarto)?;
    register_quarto_paths(lua, None)?;
    register_fatal(lua)?;

    Ok(())
}

/// `quarto.paths` — the external binaries the host found (Q1's `init.lua`).
/// Only `typst()` so far, returning the binary's path or nil. Called with
/// `None` by [`register_quarto_api`]; the filter and shortcode environments
/// call it again with [`LuaOptions::typst`](crate::lua_options::LuaOptions).
pub fn register_quarto_paths(lua: &Lua, typst: Option<&std::path::Path>) -> Result<()> {
    let quarto: Table = lua.globals().get("quarto")?;
    let paths = lua.create_table()?;
    let typst = typst.map(|p| p.to_string_lossy().into_owned());
    paths.set(
        "typst",
        lua.create_function(move |_, ()| Ok(typst.clone()))?,
    )?;
    quarto.set("paths", paths)?;
    Ok(())
}

/// Global `fatal(message)` — Q1's (`filters/common/log.lua`) for errors a
/// script cannot recover from. Here it just raises, at the caller's line;
/// the host reports it like any script error (a shortcode's becomes an
/// error diagnostic). Defined before scripts load, so a script's own
/// `fatal` replaces it.
fn register_fatal(lua: &Lua) -> Result<()> {
    lua.load("function fatal(message, offset) error(message, (offset or 1) + 1) end")
        .set_name("=quarto.fatal")
        .exec()
}

/// `quarto.markdown(str)`: read `str` as Quarto markdown and return a
/// `pandoc.Nodes` holding its blocks, for markdown that arrives as a string
/// at run time (a metadata value, a data file, a listing item's
/// description), where an lmd `{= … =}` fragment cannot be written
/// (bd-76ogq4ew).
///
/// The string is finished as a fragment, like a `{= … =}` fragment is at
/// desugar time (`crate::transforms::fragment`): footnotes it defines are
/// paired with its references into `Note`s, and the ids the reader derived
/// for its headings are dropped so the document derives them from the
/// final text. Being `Nodes`, the value adapts to where it lands: a
/// one-paragraph string unwraps to inlines in inline context. A parse
/// error raises, naming the reader's diagnostics.
fn register_quarto_markdown(lua: &Lua, quarto: &Table) -> Result<()> {
    quarto.set(
        "markdown",
        lua.create_function(|lua, source: mlua::String| {
            // The reader's output stream is its verbose trace, not a
            // diagnostic channel (see `pandoc.read`).
            let mut trace = crate::utils::output::VerboseOutput::Sink(std::io::sink());
            let bytes = source.as_bytes();
            match crate::readers::qmd::read(
                &bytes,
                false,
                "<quarto.markdown>",
                &mut trace,
                true,
                None,
            ) {
                Ok((pandoc, _context, _warnings)) => {
                    let blocks =
                        crate::transforms::fragment::prepare_fragment_blocks(pandoc.blocks);
                    let shape = if blocks.is_empty() {
                        super::nodes::NodesShape::Empty
                    } else {
                        super::nodes::NodesShape::Blocks(blocks)
                    };
                    lua.create_userdata(super::nodes::LuaNodes { shape })
                }
                Err(diagnostics) => {
                    let messages: Vec<String> =
                        diagnostics.iter().map(|d| d.title.clone()).collect();
                    Err(mlua::Error::runtime(format!(
                        "quarto.markdown: {}",
                        messages.join("; ")
                    )))
                }
            }
        })?,
    )
}

/// `quarto.component(name, props, content)` — an element bound to a
/// component (bd-react-components-a8flgldm), for Lua filters and lmd
/// cells that build markup.
///
/// Returns a `Div` whose attributes carry `data-quarto-component=name`,
/// holding `content` (blocks: a `Block`, a `Blocks`, a plain list, a
/// `Nodes`, or nil) preceded by a `<script type="application/json"
/// class="quarto-component-props">` raw block with `props` encoded by
/// `pandoc.json.encode` (so `Inlines` values become their text and `meta`
/// proxies their plain JSON). When `content` is inline (a string, an
/// `Inline`, or an `Inlines`) the result is a `Span` with a raw inline
/// instead. `props` may be nil. The static content is the no-script
/// version; the page runtime mounts the component over it and reads the
/// props script. The `components:` metadata must declare `name`, or the
/// transform reports Q-25-7 at the element.
fn register_quarto_component(lua: &Lua, quarto: &Table) -> Result<()> {
    use super::types::{
        LuaBlock, LuaInline, filter_source_info, peek_blocks_fuzzy, peek_inlines_fuzzy,
    };
    use crate::pandoc::{AttrSourceInfo, Block, Div, Inline, RawBlock, RawInline, Span};
    use hashlink::LinkedHashMap;

    quarto.set(
        "component",
        lua.create_function(|lua, (name, props, content): (String, Value, Value)| {
            if name.trim().is_empty() {
                return Err(mlua::Error::runtime(
                    "quarto.component: the component name must be a non-empty string",
                ));
            }
            let json = match props {
                Value::Nil => "{}".to_string(),
                v => {
                    let pandoc: Table = lua.globals().get("pandoc")?;
                    let json_ns: Table = pandoc.get("json")?;
                    let encode: mlua::Function = json_ns.get("encode")?;
                    let encoded: String = encode.call(v)?;
                    if !encoded.trim_start().starts_with('{') {
                        return Err(mlua::Error::runtime(format!(
                            "quarto.component: props for `{name}` must encode to a JSON object, got {encoded}"
                        )));
                    }
                    encoded
                }
            };
            // The same escape as the page-side `script_json`: no `<`, `>`
            // or `&` can end the script element or start a comment.
            let json = json
                .replace('<', "\\u003c")
                .replace('>', "\\u003e")
                .replace('&', "\\u0026");
            let script = format!(
                "<script type=\"application/json\" class=\"quarto-component-props\">{json}</script>"
            );
            let mut kvs = LinkedHashMap::new();
            kvs.insert("data-quarto-component".to_string(), name.clone());
            let attr = (String::new(), Vec::new(), kvs);
            let source_info = filter_source_info(lua);

            let inline = match &content {
                Value::String(_) => true,
                Value::UserData(ud) => ud.is::<LuaInline>(),
                Value::Table(t) => t
                    .metatable()
                    .and_then(|mt| mt.get::<String>("__name").ok())
                    .is_some_and(|n| n == "Inlines"),
                _ => false,
            };
            if inline {
                let mut inlines = vec![Inline::RawInline(RawInline {
                    format: "html".to_string(),
                    text: script,
                    source_info: source_info.clone(),
                })];
                inlines.extend(peek_inlines_fuzzy(lua, content)?);
                return lua.create_userdata(LuaInline::new(Inline::Span(Span {
                    attr,
                    content: inlines,
                    source_info,
                    attr_source: AttrSourceInfo::empty(),
                })));
            }
            let mut blocks = vec![Block::RawBlock(RawBlock {
                format: "html".to_string(),
                text: script,
                source_info: source_info.clone(),
            })];
            if !matches!(content, Value::Nil) {
                blocks.extend(peek_blocks_fuzzy(lua, content)?);
            }
            lua.create_userdata(LuaBlock::new(Block::Div(Div {
                attr,
                content: blocks,
                source_info,
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;
    Ok(())
}

/// `quarto.version` — the version this build reports
/// (`quarto_util::cli_version()`, so a nightly's `QUARTO_VERSION_OVERRIDE`
/// is honored), as a `pandoc.types.Version`: comparable with strings and
/// other Versions (`quarto.version >= "1.4"`), `tostring` gives `"0.36.0"`,
/// and `table.concat(quarto.version, '.')` still works (GH #942). Only the
/// leading dotted-numeric part is carried: `0.37.0-nightly.20261027` is
/// `0.37.0`, as in Quarto 1.
fn register_quarto_version(lua: &Lua, quarto: &Table) -> Result<()> {
    let version = super::version::LuaVersion::leading(quarto_util::cli_version());
    quarto.set("version", super::version::create_version(lua, &version.0)?)?;
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
//
// THE CONTRACT (settled for `dofile` in #112, generalized in GH #588 /
// bd-sr0nipl7): the script-dir stack moves only at top-level script
// boundaries — filter script load (`filter.rs`), shortcode script load and
// shortcode handler invocation (`shortcode.rs`). Loaders — `require`,
// `dofile`, `loadfile` — never move it. A loader that needs the location of
// the file it is currently executing tracks that privately (see
// `REQUIRE_DIR_STACK_KEY`); it must not publish it through this stack,
// because `quarto.utils.resolve_path`, `quarto.doc` dependency resolution,
// and the WASM `dofile`/`loadfile` overrides all define "current path" as
// *the innermost running script's directory*, exactly as Quarto 1's
// `scriptFile` stack does (quarto-cli `init.lua:167-186`).
//
// Corollary: at rest — no script executing — the stack holds exactly the
// load-time entries of the states that keep their script for their
// lifetime (the per-filter state's single entry). The shared shortcode
// state balances every push with a pop (`ScriptDirGuard`), so entries
// from one extension never linger while another runs (bd-9xa0yui7).
// =========================================================================

/// Initialize the script-dir stack in the Lua state.
/// Must be called during Lua state setup, before any script execution.
pub fn init_script_dir_stack(lua: &Lua) -> Result<()> {
    lua.globals()
        .set("_quarto_script_dir_stack", lua.create_table()?)?;
    Ok(())
}

/// Registry key for the require-private module-dir stack: the directories
/// of the modules currently being loaded by the scoped `require`, innermost
/// last. Lives in the Lua registry (not globals) because it is an
/// implementation detail of `require`'s candidate search — per the
/// script-dir contract (see the section comment above), loaders must not
/// publish the location of the file they are executing through the
/// script-dir stack that `resolve_path` and friends read (GH #588).
const REQUIRE_DIR_STACK_KEY: &str = "quarto_require_dir_stack";

/// `quarto.include(file, params)`: evaluate a file and return its blocks
/// as a `pandoc.Blocks` (bd-76ogq4ew, phase 3). Where `require` yields a
/// module *value*, cached once per state, an include is a *call*: run every
/// time, with `params` (a table, or nil) as the chunk's vararg, so
/// `partials/card.lmd` can start `local p = ...` and be included with
/// different arguments from several places.
///
/// - An `.lmd` file (any registered source dialect) is compiled by its
///   dialect; a `.lua` file is loaded as it is. The chunk runs in an
///   environment inheriting the globals, with its own directory on the
///   require-private module-dir stack so its `require`s resolve beside it.
///   Its return value is read under the block-cell contract
///   (`crate::lua::lmd`): a Block, a list of Blocks, `Nodes`, or nil for
///   nothing. The structural desugaring set is applied over the result
///   (`dev-docs/syntax-notes.md`), its diagnostics raised as Lua warnings.
/// - A `.qmd` / `.md` file is read by the qmd reader and finished as a
///   fragment (`crate::transforms::fragment`); it takes no params.
///
/// `file` resolves relative to the including file (the innermost loading
/// module, else the running script; for an lmd cell the document), or
/// against the project root when it starts with `/` (an existing absolute
/// path is read as itself, as `require` does for Quarto 1 filters). Every
/// read goes
/// through the [`SystemRuntime`], so the VFS serves it on WASM and a
/// `SandboxedRuntime` policy governs it.
pub fn register_quarto_include(
    lua: &Lua,
    runtime: Arc<dyn crate::lua::runtime::SystemRuntime>,
) -> Result<()> {
    let quarto: Table = lua.globals().get("quarto")?;
    quarto.set(
        "include",
        lua.create_function(move |lua, (file, params): (String, Option<Table>)| {
            let path = resolve_include_path(lua, runtime.as_ref(), &file)?;
            let bytes = runtime.file_read(&path).map_err(|e| {
                mlua::Error::runtime(format!(
                    "quarto.include: cannot read `{}`: {e}",
                    path.display()
                ))
            })?;
            let source = String::from_utf8(bytes).map_err(mlua::Error::external)?;
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let blocks = match ext.as_str() {
                "qmd" | "md" | "markdown" => {
                    if params.is_some() {
                        return Err(mlua::Error::runtime(format!(
                            "quarto.include: a markdown file takes no params; `{}` is read as it is",
                            path.display()
                        )));
                    }
                    include_markdown(&source, &path)?
                }
                _ => include_chunk(lua, &source, &path, &ext, params)?,
            };
            super::list::create_blocks_table(lua, &blocks)
        })?,
    )
}

/// Where `quarto.include(file)` reads from.
///
/// A leading `/` is the project root, as for `require`, unless the path
/// exists as an absolute OS path (`quarto.utils.resolve_path` results),
/// which is then read as itself; the runtime's policy governs either
/// way. Otherwise
/// the path is relative to the *including file*: the chunk that called
/// `quarto.include`, read off the Lua stack when it was loaded from a file
/// (`@path` chunk names: modules, includes, the document's cells), so a
/// function defined in a module and called later still includes beside
/// its module. When the calling chunk has no file, the innermost loading
/// module's directory, then the running script's, stand in.
fn resolve_include_path(
    lua: &Lua,
    runtime: &dyn crate::lua::runtime::SystemRuntime,
    file: &str,
) -> Result<PathBuf> {
    if let Some(rooted) = file.strip_prefix('/') {
        if runtime
            .path_exists(Path::new(file), Some(quarto_system_runtime::PathKind::File))
            .unwrap_or(false)
        {
            return Ok(PathBuf::from(file));
        }
        return match module_roots(lua).project {
            Some(project) => Ok(project.join(rooted)),
            None => Err(mlua::Error::runtime(format!(
                "quarto.include: `{file}` starts with `/` but no project root is set"
            ))),
        };
    }
    // Level 0 is this function; level 1 is the Lua caller.
    let caller_dir: Option<PathBuf> = lua
        .inspect_stack(1, |debug| {
            debug
                .source()
                .source
                .as_deref()
                .and_then(|src| src.strip_prefix('@'))
                .map(Path::new)
                .filter(|p| p.is_absolute())
                .and_then(|p| p.parent().map(Path::to_path_buf))
        })
        .flatten();
    if let Some(dir) = caller_dir {
        return Ok(dir.join(file));
    }
    let module_stack: Table = lua.named_registry_value(REQUIRE_DIR_STACK_KEY)?;
    let len = module_stack.raw_len();
    let base = if len > 0 {
        module_stack.get::<String>(len).unwrap_or_default()
    } else {
        current_script_dir(lua)?
    };
    Ok(if base.is_empty() {
        super::process_view::resolve_path(lua, file)
    } else {
        Path::new(&base).join(file)
    })
}

/// A `.qmd` / `.md` include: the reader, then fragment preparation.
fn include_markdown(source: &str, path: &Path) -> Result<Vec<crate::pandoc::Block>> {
    let mut trace = crate::utils::output::VerboseOutput::Sink(std::io::sink());
    match crate::readers::qmd::read(
        source.as_bytes(),
        false,
        &path.display().to_string(),
        &mut trace,
        true,
        None,
    ) {
        Ok((pandoc, _context, _warnings)) => Ok(
            crate::transforms::fragment::prepare_fragment_blocks(pandoc.blocks),
        ),
        Err(diagnostics) => {
            let messages: Vec<String> = diagnostics.iter().map(|d| d.title.clone()).collect();
            Err(mlua::Error::runtime(format!(
                "quarto.include: `{}` did not parse: {}",
                path.display(),
                messages.join("; ")
            )))
        }
    }
}

/// An `.lmd` (dialect) or `.lua` include: compile if needed, run with
/// `params`, read the result as blocks, apply the structural desugaring set.
fn include_chunk(
    lua: &Lua,
    source: &str,
    path: &Path,
    ext: &str,
    params: Option<Table>,
) -> Result<Vec<crate::pandoc::Block>> {
    let dialects = super::dialect::registered_dialects(lua);
    let lua_source = if ext == "lua" {
        source.to_string()
    } else if let Some(dialect) = dialects.iter().find(|d| d.extension() == ext) {
        dialect.compile(source, path).map_err(|error| {
            mlua::Error::external(crate::dialect::ModuleCompileError {
                module: path.display().to_string(),
                path: path.to_path_buf(),
                dialect: dialect.name().to_string(),
                error,
            })
        })?
    } else {
        let known: Vec<String> = ["md", "qmd", "lua"]
            .into_iter()
            .map(str::to_string)
            .chain(dialects.iter().map(|d| d.extension().to_string()))
            .collect();
        return Err(mlua::Error::runtime(format!(
            "quarto.include: `{}` is not a file quarto.include can evaluate (one of: .{})",
            path.display(),
            known.join(", .")
        )));
    };

    let env = lua.create_table()?;
    let env_mt = lua.create_table()?;
    env_mt.set("__index", lua.globals())?;
    env.set_metatable(Some(env_mt))?;

    let module_stack: Table = lua.named_registry_value(REQUIRE_DIR_STACK_KEY)?;
    let dir = path
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .to_string();
    let len = module_stack.raw_len();
    module_stack.set(len + 1, dir)?;
    let args = match params {
        Some(t) => Value::Table(t),
        None => Value::Nil,
    };
    let result = lua
        .load(&lua_source)
        .set_name(format!("@{}", path.display()))
        .set_environment(env)
        .call::<Value>(args);
    module_stack.set(len + 1, Value::Nil)?;
    let value = result?;

    let blocks = match super::lmd::classify_cell_value(lua, value) {
        Ok(super::lmd::LmdCellValue::Nothing) => Vec::new(),
        Ok(super::lmd::LmdCellValue::Blocks(blocks)) => blocks,
        Err(err) => {
            return Err(mlua::Error::runtime(format!(
                "quarto.include: `{}`: {err}",
                path.display()
            )));
        }
    };
    // The structural desugaring set, as for a cell's return value; the
    // include has no cell to relocate a diagnostic onto, so it is a warning
    // naming the file.
    let mut collector = crate::utils::diagnostic_collector::DiagnosticCollector::new();
    let blocks = crate::pandoc::desugar::desugar_blocks(blocks, &mut collector);
    for diag in collector.diagnostics() {
        warn_from_loader(lua, &format!("{}: {}", path.display(), diag.title));
    }
    Ok(blocks)
}

/// Install the host's [`ModuleRoots`](crate::lua_options::ModuleRoots) in
/// the state: the project root a leading `/` resolves against, and the
/// extension directories a module name's first segment may select
/// (bd-76ogq4ew, phase 3). Replaces any roots set before. Without this
/// call the loader searches the script directories only.
pub fn set_module_roots(lua: &Lua, roots: crate::lua_options::ModuleRoots) {
    lua.set_app_data(roots);
}

/// The roots in effect, or none.
pub(crate) fn module_roots(lua: &Lua) -> crate::lua_options::ModuleRoots {
    lua.app_data_ref::<crate::lua_options::ModuleRoots>()
        .map(|r| r.clone())
        .unwrap_or_default()
}

/// Replace the global `require` with a script-dir-aware loader.
///
/// Q1 extensions `require` sibling modules relative to the requiring
/// script's directory (TS Quarto patches `require` through its script-file
/// stack, `init.lua:259-305`). This loader:
///
/// 1. resolves `name` against the require-private module-dir stack
///    (innermost loading module first) and then the script-dir stack
///    top-down, trying `<dir>/<name>.lua`, `<dir>/<name with . -> />.lua`,
///    and `<dir>/<name>/init.lua`; then, when the host set
///    [`ModuleRoots`](crate::lua_options::ModuleRoots), the extension
///    directory the name's first segment selects (`require("cards/deck")`
///    tries `_extensions/cards/deck`); a name with a leading `/` resolves
///    against the project root only (`require("/components/cards")`);
/// 2. executes the module in a sandboxed environment (globals-inheriting,
///    like shortcode scripts) with the module's own directory pushed on the
///    private module-dir stack — NOT the script-dir stack — so nested
///    `require`s resolve relative to the module while
///    `quarto.utils.resolve_path` (and every other script-dir consumer)
///    keeps seeing the requiring script's root (GH #588);
/// 3. caches by resolved absolute path (a module evaluating to `nil` caches
///    as `true`, matching Lua's `require`);
/// 4. falls back to the original `require` (when the target has one — the
///    WASM stdlib has no `package` lib) for anything not found on disk.
///
/// File access goes through [`SystemRuntime`], so the same code serves
/// native and WASM (VFS) targets.
pub fn register_scoped_require(
    lua: &Lua,
    runtime: Arc<dyn crate::lua::runtime::SystemRuntime>,
) -> Result<()> {
    let globals = lua.globals();
    let original: Option<mlua::Function> = globals.get("require").ok();
    globals.set("_quarto_require_cache", lua.create_table()?)?;
    lua.set_named_registry_value(REQUIRE_DIR_STACK_KEY, lua.create_table()?)?;

    let require = lua.create_function(move |lua, name: String| {
        let cache: Table = lua.globals().get("_quarto_require_cache")?;

        // Candidate dirs: the private module-dir stack top-down (the
        // currently-loading module's dir first — sibling-relative
        // requires), then the script-dir stack top-down (Q1-style
        // extension-root-relative paths like
        // `require("modules/brand/brand")` from a nested module). This is
        // the same effective order as when module dirs lived on the shared
        // stack; only the storage moved.
        let mut dirs: Vec<String> = Vec::new();
        let module_stack: Table = lua.named_registry_value(REQUIRE_DIR_STACK_KEY)?;
        for i in (1..=module_stack.raw_len()).rev() {
            if let Ok(d) = module_stack.get::<String>(i)
                && !d.is_empty()
                && !dirs.contains(&d)
            {
                dirs.push(d);
            }
        }
        let stack: Table = lua.globals().get("_quarto_script_dir_stack")?;
        for i in (1..=stack.raw_len()).rev() {
            if let Ok(d) = stack.get::<String>(i)
                && !d.is_empty()
                && !dirs.contains(&d)
            {
                dirs.push(d);
            }
        }
        // Candidate files, in the order they are tried. For each form of
        // the name (as written, dots as separators, `<name>/init`) every
        // registered source dialect's extension comes before `.lua`, so a
        // module kept in dialect source wins over a compiled copy beside
        // it (bd-76ogq4ew): the copy may be stale, the source is what the
        // author edits, and the shadowed copy is reported as a warning.
        struct Candidate {
            path: PathBuf,
            dialect: Option<Arc<dyn super::dialect::SourceDialect>>,
            /// For a dialect candidate, the `.lua` of the same stem, whose
            /// presence is worth a warning.
            shadowed: Option<PathBuf>,
        }
        let dialects = super::dialect::registered_dialects(lua);
        let roots = module_roots(lua);
        // The forms a name is tried in, under one base directory.
        let stems_of = |name: &str| -> Vec<String> {
            if name.is_empty() {
                return vec!["init".to_string()];
            }
            let mut stems: Vec<String> = vec![name.to_string()];
            if name.contains('.') {
                stems.push(name.replace('.', "/"));
            }
            stems.push(format!("{name}/init"));
            stems
        };
        // (base directory, stems) pairs in search order.
        let mut searches: Vec<(PathBuf, Vec<String>)> = Vec::new();
        if let Some(rooted) = name.strip_prefix('/') {
            // The name as a path of its own first: Quarto 1 filters
            // `require(quarto.utils.resolve_path("_modules/x"))`, an
            // absolute path (GH #587 contract; on the WASM VFS too, where
            // `Path::is_absolute` would say no). Then `/components/cards`:
            // the project root. The two never both exist for one name.
            searches.push((PathBuf::new(), stems_of(&name)));
            if let Some(project) = &roots.project {
                searches.push((project.clone(), stems_of(rooted)));
            }
        } else {
            for dir in &dirs {
                searches.push((PathBuf::from(dir), stems_of(&name)));
            }
            // `cards/deck` or `cards.deck`: the extension named by the first
            // segment, after the script directories, so a local `cards/`
            // directory wins over an extension of the same name.
            let (head, rest) = match name.find(['/', '.']) {
                Some(i) => (&name[..i], &name[i + 1..]),
                None => (name.as_str(), ""),
            };
            if let Some(ext) = roots.extension(head) {
                searches.push((ext.dir.clone(), stems_of(rest)));
            }
        }
        let mut candidates: Vec<Candidate> = Vec::new();
        for (base, stems) in &searches {
            for stem in stems {
                let lua_path = base.join(format!("{stem}.lua"));
                for dialect in &dialects {
                    candidates.push(Candidate {
                        path: base.join(format!("{stem}.{}", dialect.extension())),
                        dialect: Some(dialect.clone()),
                        shadowed: Some(lua_path.clone()),
                    });
                }
                candidates.push(Candidate {
                    path: lua_path,
                    dialect: None,
                    shadowed: None,
                });
            }
        }

        for candidate in &candidates {
            let key = candidate.path.to_string_lossy().to_string();
            if let Ok(cached) = cache.get::<Value>(key.as_str())
                && !matches!(cached, Value::Nil)
            {
                return Ok(cached);
            }
            let Ok(bytes) = runtime.file_read(&candidate.path) else {
                continue;
            };
            let source = String::from_utf8(bytes).map_err(mlua::Error::external)?;

            // A dialect source is compiled to Lua before it is loaded; the
            // compile step is the dialect's, the file access was the
            // runtime's, so a read policy on the runtime governs both.
            let source = match &candidate.dialect {
                None => source,
                Some(dialect) => {
                    if let Some(shadowed) = &candidate.shadowed
                        && runtime
                            .path_exists(shadowed, Some(quarto_system_runtime::PathKind::File))
                            .unwrap_or(false)
                    {
                        warn_from_loader(
                            lua,
                            &format!(
                                "`{}` is loaded and `{}` beside it is ignored: a `.{}` module takes precedence over a `.lua` file of the same name. Delete the `.lua` file if it is a stale compiled copy.",
                                candidate.path.display(),
                                shadowed.display(),
                                dialect.extension()
                            ),
                        );
                    }
                    dialect
                        .compile(&source, &candidate.path)
                        .map_err(|error| {
                            mlua::Error::external(crate::dialect::ModuleCompileError {
                                module: name.clone(),
                                path: candidate.path.clone(),
                                dialect: dialect.name().to_string(),
                                error,
                            })
                        })?
                }
            };

            // Sandboxed module environment inheriting globals.
            let env = lua.create_table()?;
            let env_mt = lua.create_table()?;
            env_mt.set("__index", lua.globals())?;
            env.set_metatable(Some(env_mt))?;

            let module_dir = candidate
                .path
                .parent()
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .to_string();
            // Push on the require-private stack only: the script-dir stack
            // must not move while a module loads (GH #588).
            let len = module_stack.raw_len();
            module_stack.set(len + 1, module_dir)?;
            // `@path` is Lua's convention for a chunk that came from a
            // file: messages and tracebacks print `path:line:`.
            let result = lua
                .load(&source)
                .set_name(format!("@{key}"))
                .set_environment(env)
                .eval::<Value>();
            module_stack.set(len + 1, Value::Nil)?;

            let value = result?;
            // A module that returns nothing caches as `true`, like Lua.
            let value = if matches!(value, Value::Nil) {
                Value::Boolean(true)
            } else {
                value
            };
            cache.set(key.as_str(), value.clone())?;
            return Ok(value);
        }

        // Not found relative to the script dirs. The scoped search is the
        // one the author can act on, so it is reported first; on native
        // the stock `require` (package.path, C modules) gets a turn and
        // its own report becomes the hint when it fails too.
        let no_root_note = if name.starts_with('/') && roots.project.is_none() {
            "; no project root is set for a `/`-rooted module name"
        } else {
            ""
        };
        let searched = if candidates.is_empty() {
            format!("no script directory on the stack{no_root_note}")
        } else {
            format!(
                "{}{no_root_note}",
                candidates
                    .iter()
                    .map(|c| c.path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let scoped = format!(
            "module '{name}' not found (searched relative to the current script directory: {searched})"
        );
        if let Some(orig) = &original {
            return match orig.call::<Value>(name.clone()) {
                Ok(value) => Ok(value),
                Err(err) => Err(mlua::Error::RuntimeError(format!(
                    "{scoped}\nLua's own search failed as well: {err}"
                ))),
            };
        }
        Err(mlua::Error::RuntimeError(scoped))
    })?;

    globals.set("require", require)?;
    Ok(())
}

/// Record a warning from the loader through `quarto.warn`, so it reaches
/// the same diagnostics the host drains after the script ran, located at
/// the Lua caller (the `require` line). A state without `quarto.warn`
/// (not one pampa builds) drops it.
fn warn_from_loader(lua: &Lua, message: &str) {
    let Ok(quarto) = lua.globals().get::<Table>("quarto") else {
        return;
    };
    let Ok(warn) = quarto.get::<mlua::Function>("warn") else {
        return;
    };
    let _ = warn.call::<()>(message);
}

/// Push a directory onto the script-dir stack.
pub fn push_script_dir(lua: &Lua, dir: &str) -> Result<()> {
    let stack: Table = lua.globals().get("_quarto_script_dir_stack")?;
    let len = stack.raw_len();
    stack.set(len + 1, dir)?;
    Ok(())
}

/// RAII guard that pops the script-dir stack when dropped, so a push is
/// balanced on every exit path (early `?` returns, errors, panics). Obtain
/// via [`push_script_dir_scoped`]. Owns a cheap clone of the `Lua` handle
/// so it can outlive borrows of the caller's fields.
pub struct ScriptDirGuard {
    lua: Lua,
}

impl Drop for ScriptDirGuard {
    fn drop(&mut self) {
        let _ = pop_script_dir(&self.lua);
    }
}

/// Push a directory onto the script-dir stack for the lifetime of the
/// returned guard.
pub fn push_script_dir_scoped(lua: &Lua, dir: &str) -> Result<ScriptDirGuard> {
    push_script_dir(lua, dir)?;
    Ok(ScriptDirGuard { lua: lua.clone() })
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

    // `quarto.attribution.lookup_range(start, end[, file_id])` —
    // primitive raw lookup. Returns `{actor, time}` table on hit, nil
    // otherwise. The optional third argument declares which file the
    // byte range indexes into: when given and different from the
    // handle's blamed file, the call returns nil instead of colliding
    // into the blamed file's runs (bd-thagcbfq). Omitting it keeps
    // the historical "caller asserts primary-doc offsets" contract —
    // pass `r.file_id` from `si:byte_range()` whenever the range came
    // from a node.
    let h_for_range = handle.clone();
    attribution.set(
        "lookup_range",
        lua.create_function(
            move |lua, (start, end_, file_id): (usize, usize, Option<usize>)| {
                let Some(h) = h_for_range.as_ref() else {
                    return Ok(Value::Nil);
                };
                if let Some(fid) = file_id
                    && fid != h.blamed_file_id()
                {
                    return Ok(Value::Nil);
                }
                let Some(hit) = h.lookup_range(start, end_) else {
                    return Ok(Value::Nil);
                };
                let t = lua.create_table()?;
                t.set("actor", hit.actor)?;
                t.set("time", hit.time)?;
                Ok(Value::Table(t))
            },
        )?,
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
            -- Single-doc invariant: lookup_range refuses ranges from
            -- any file other than the blamed one (compared Rust-side
            -- against the handle's actual blamed file, bd-thagcbfq).
            local hit = lookup_range(r[1], r[2], r.file_id)
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
            // Rooted paths (including WASM `/project/...` VFS paths) are
            // returned as-is. Use `is_rooted`, not `is_absolute`: on wasm32,
            // and on Windows for drive-less rooted paths like `/project/x`,
            // `is_absolute` is `false` and the path would wrongly join onto the
            // script dir (bd-picv). Output is forward-slash normalized so the
            // Lua-visible result is platform-independent.
            if quarto_util::is_rooted(p) {
                return Ok(quarto_util::to_forward_slashes(p));
            }
            let script_dir = current_script_dir(lua)?;
            if script_dir.is_empty() {
                return Ok(quarto_util::to_forward_slashes(p));
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
                if let Some(last) = components.last()
                    && !matches!(last, Component::RootDir | Component::Prefix(_))
                {
                    components.pop();
                    continue;
                }
                components.push(component);
            }
            _ => components.push(component),
        }
    }
    let result: PathBuf = components.iter().collect();
    // Forward-slash normalize so the Lua-visible path is identical on all
    // platforms (matches quarto-cli's pathWithForwardSlashes convention).
    quarto_util::to_forward_slashes(&result)
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
    // fatal / quarto.paths tests (bd-gebcr1sa)
    // =========================================================================

    #[test]
    fn fatal_raises_the_message_at_the_callers_line() {
        let lua = create_test_lua();
        let err = lua
            .load("local x = 1\nfatal('placeholder: no typst')")
            .set_name("=script")
            .exec()
            .unwrap_err()
            .to_string();
        assert!(err.contains("script:2: placeholder: no typst"), "{err}");
    }

    #[test]
    fn a_scripts_own_fatal_replaces_the_global() {
        let lua = create_test_lua();
        let v: String = lua
            .load("function fatal(m) return 'mine: ' .. m end\nreturn fatal('x')")
            .eval()
            .unwrap();
        assert_eq!(v, "mine: x");
    }

    #[test]
    fn quarto_paths_typst_is_nil_until_the_host_sets_it() {
        let lua = create_test_lua();
        let none: Option<String> = lua.load("return quarto.paths.typst()").eval().unwrap();
        assert_eq!(none, None);

        register_quarto_paths(&lua, Some(std::path::Path::new("/opt/typst"))).unwrap();
        let some: Option<String> = lua.load("return quarto.paths.typst()").eval().unwrap();
        assert_eq!(some.as_deref(), Some("/opt/typst"));
    }

    // =========================================================================
    // quarto.markdown tests (bd-76ogq4ew)
    // =========================================================================

    fn markdown_nodes(lua: &Lua, code: &str) -> crate::lua::nodes::LuaNodes {
        let v: Value = lua.load(code).eval().unwrap();
        match v {
            Value::UserData(ud) => ud.borrow::<crate::lua::nodes::LuaNodes>().unwrap().clone(),
            other => panic!("expected a Nodes userdata, got {other:?}"),
        }
    }

    #[test]
    fn quarto_markdown_reads_a_string_into_nodes() {
        let lua = create_test_lua();
        let nodes = markdown_nodes(
            &lua,
            r#"return quarto.markdown("Hello *there*\n\nSecond.")"#,
        );
        let crate::lua::nodes::NodesShape::Blocks(blocks) = &nodes.shape else {
            panic!("{nodes:?}")
        };
        assert_eq!(blocks.len(), 2);
        assert!(
            matches!(&blocks[0], crate::pandoc::Block::Paragraph(p) if p.content.iter().any(|i| matches!(i, crate::pandoc::Inline::Emph(_))))
        );
        // Nodes: a one-paragraph string unwraps where inlines go.
        let text: String = lua
            .load(r#"return pandoc.utils.stringify(pandoc.Strong(quarto.markdown("a *b*")))"#)
            .eval()
            .unwrap();
        assert_eq!(text, "a b");
        // And an empty string is empty Nodes.
        let empty = markdown_nodes(&lua, r#"return quarto.markdown("")"#);
        assert!(
            matches!(empty.shape, crate::lua::nodes::NodesShape::Empty),
            "{empty:?}"
        );
    }

    #[test]
    fn quarto_markdown_finishes_the_string_as_a_fragment() {
        let lua = create_test_lua();
        let nodes = markdown_nodes(
            &lua,
            r"return quarto.markdown('## Title\n\ntext[^1]\n\n[^1]: the note')",
        );
        let crate::lua::nodes::NodesShape::Blocks(blocks) = &nodes.shape else {
            panic!("{nodes:?}")
        };
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        let crate::pandoc::Block::Header(h) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(h.attr.0, "", "derived id dropped");
        let crate::pandoc::Block::Paragraph(p) = &blocks[1] else {
            panic!("{blocks:?}")
        };
        assert!(
            p.content
                .iter()
                .any(|i| matches!(i, crate::pandoc::Inline::Note(_))),
            "{:?}",
            p.content
        );
    }

    #[test]
    fn quarto_markdown_reports_a_parse_error() {
        let lua = create_test_lua();
        let err = lua
            // A `'` directly after a word is an apostrophe since GH #943, so
            // the stray quote needs whitespace in front of it to be an error.
            .load(r#"return quarto.markdown("the groups ' Unique IDs")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("quarto.markdown:"), "{err}");
        // A number coerces to its string, as Lua does everywhere; a table
        // does not.
        let nodes = markdown_nodes(&lua, "return quarto.markdown(42)");
        assert!(
            matches!(nodes.shape, crate::lua::nodes::NodesShape::Blocks(_)),
            "{nodes:?}"
        );
        let err = lua
            .load("return quarto.markdown({})")
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("string"), "{err}");
    }

    // =========================================================================
    // quarto.component tests (bd-react-components-a8flgldm)
    // =========================================================================

    fn component_div(lua: &Lua, code: &str) -> crate::pandoc::Block {
        let v: Value = lua.load(code).eval().unwrap();
        match v {
            Value::UserData(ud) => ud
                .borrow::<crate::lua::types::LuaBlock>()
                .unwrap()
                .borrow_block()
                .clone(),
            other => panic!("expected a block userdata, got {other:?}"),
        }
    }

    #[test]
    fn quarto_component_builds_a_div_with_host_attr_and_props_script() {
        let lua = create_test_lua();
        let block = component_div(
            &lua,
            r#"return quarto.component("table", { rows = { { title = "A", n = 1 } }, label = "x<y" }, { pandoc.Para({ pandoc.Str("fallback") }) })"#,
        );
        let crate::pandoc::Block::Div(div) = block else {
            panic!("not a div")
        };
        assert_eq!(div.attr.0, "");
        assert_eq!(
            div.attr.2.get("data-quarto-component").map(String::as_str),
            Some("table")
        );
        assert_eq!(div.content.len(), 2);
        let crate::pandoc::Block::RawBlock(raw) = &div.content[0] else {
            panic!("first child is the props script")
        };
        assert_eq!(raw.format, "html");
        assert!(
            raw.text
                .starts_with("<script type=\"application/json\" class=\"quarto-component-props\">"),
            "{}",
            raw.text
        );
        assert!(raw.text.contains("\"rows\":[{"), "{}", raw.text);
        assert!(
            raw.text.contains("x\\u003cy"),
            "markup characters are escaped: {}",
            raw.text
        );
        assert!(matches!(div.content[1], crate::pandoc::Block::Paragraph(_)));
    }

    #[test]
    fn quarto_component_accepts_nil_props_and_nil_content() {
        let lua = create_test_lua();
        let block = component_div(&lua, r#"return quarto.component("x", nil, nil)"#);
        let crate::pandoc::Block::Div(div) = block else {
            panic!("not a div")
        };
        assert_eq!(div.content.len(), 1);
        let crate::pandoc::Block::RawBlock(raw) = &div.content[0] else {
            panic!()
        };
        assert!(raw.text.contains(">{}</script>"), "{}", raw.text);
    }

    #[test]
    fn quarto_component_with_inline_content_builds_a_span() {
        let lua = create_test_lua();
        let v: Value = lua
            .load(r#"return quarto.component("spark", { values = { 1, 2 } }, pandoc.Inlines({ pandoc.Str("1 2") }))"#)
            .eval()
            .unwrap();
        let Value::UserData(ud) = v else { panic!() };
        let inline = ud
            .borrow::<crate::lua::types::LuaInline>()
            .unwrap()
            .borrow_inline()
            .clone();
        let crate::pandoc::Inline::Span(span) = inline else {
            panic!("not a span")
        };
        assert_eq!(
            span.attr.2.get("data-quarto-component").map(String::as_str),
            Some("spark")
        );
        assert!(matches!(
            span.content[0],
            crate::pandoc::Inline::RawInline(_)
        ));
        assert_eq!(span.content.len(), 2);

        // A bare string is inline content too.
        let v: Value = lua
            .load(r#"return quarto.component("spark", nil, "1 2 3")"#)
            .eval()
            .unwrap();
        let Value::UserData(ud) = v else { panic!() };
        assert!(ud.is::<crate::lua::types::LuaInline>());
    }

    #[test]
    fn quarto_component_rejects_bad_name_and_non_object_props() {
        let lua = create_test_lua();
        let err = lua
            .load(r#"return quarto.component("", nil, nil)"#)
            .eval::<Value>()
            .unwrap_err();
        assert!(err.to_string().contains("non-empty"), "{err}");
        let err = lua
            .load(r#"return quarto.component("x", { 1, 2 }, nil)"#)
            .eval::<Value>()
            .unwrap_err();
        assert!(err.to_string().contains("JSON object"), "{err}");
    }

    #[test]
    fn quarto_component_props_encode_inlines_as_text() {
        let lua = create_test_lua();
        let block = component_div(
            &lua,
            r#"return quarto.component("t", { title = pandoc.Inlines({ pandoc.Str("Hello"), pandoc.Space(), pandoc.Emph({ pandoc.Str("there") }) }), node = pandoc.Str("one") }, nil)"#,
        );
        let crate::pandoc::Block::Div(div) = block else {
            panic!()
        };
        let crate::pandoc::Block::RawBlock(raw) = &div.content[0] else {
            panic!()
        };
        assert!(
            raw.text.contains("\"title\":\"Hello there\""),
            "{}",
            raw.text
        );
        assert!(raw.text.contains("\"node\":\"one\""), "{}", raw.text);
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
        assert!(result.contains('1'));
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

    // resolve_path must return forward slashes even when the pushed script dir
    // carries native (backslash) separators. On Windows the production callers
    // (filter.rs / shortcode.rs) push `filter_path.parent().to_string_lossy()`,
    // which is a native path like `C:\ext\dir` — without normalization the
    // joined result leaks backslashes into the Lua-visible path.
    #[test]
    fn test_quarto_utils_resolve_path_backslash_script_dir() {
        let lua = create_test_lua();
        push_script_dir(&lua, "C:\\ext\\dir").unwrap();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("data.json")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "C:/ext/dir/data.json");
    }

    // A rooted input must be returned as-is, never joined onto the script dir,
    // even when a script dir is active. This pins the `is_rooted` (not
    // `is_absolute`) gate: on Windows `/abs/x.json` is rooted but NOT absolute
    // (no drive prefix), so the old `is_absolute` gate let it fall through and
    // join onto the script dir — leaking the script dir's drive prefix
    // (`C:\abs\x.json`). On WASM the same gate would mis-join `/project/...`
    // VFS paths. `is_rooted` fixes both. Output must also be forward-slashed.
    #[test]
    fn test_quarto_utils_resolve_path_rooted_ignores_script_dir() {
        let lua = create_test_lua();
        push_script_dir(&lua, "C:\\some\\dir").unwrap();
        let result: String = lua
            .load(r#"return quarto.utils.resolve_path("/abs/x.json")"#)
            .eval()
            .unwrap();
        assert_eq!(result, "/abs/x.json");
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
    fn test_quarto_version_is_a_pandoc_version() {
        let lua = create_test_lua();
        let result: String = lua
            .load(r#"return pandoc.utils.type(quarto.version)"#)
            .eval()
            .unwrap();
        assert_eq!(result, "Version");
    }

    #[test]
    fn test_quarto_version_tracks_the_build_version() {
        let lua = create_test_lua();
        let expected = super::super::version::LuaVersion::leading(quarto_util::cli_version());
        let expected = expected
            .0
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(".");
        assert_ne!(expected, "0.1.0", "the hard-coded placeholder must be gone");
        let shown: String = lua
            .load(r#"return tostring(quarto.version)"#)
            .eval()
            .unwrap();
        assert_eq!(shown, expected);
        // Q1-era extensions build the string this way; it must keep working
        // on the userdata (GH #942).
        let concat: String = lua
            .load(r#"return table.concat(quarto.version, '.')"#)
            .eval()
            .unwrap();
        assert_eq!(concat, expected);
    }

    #[test]
    fn test_quarto_version_compares_with_strings() {
        let lua = create_test_lua();
        // The probe from GH #942, which used to fail with "attempt to
        // compare string with table".
        let ok: bool = lua
            .load(r#"return (quarto.version >= "1.4") == false and quarto.version >= "0.1""#)
            .eval()
            .unwrap();
        assert!(ok);
        let wrapped: String = lua
            .load(r#"return tostring(pandoc.types.Version(quarto.version))"#)
            .eval()
            .unwrap();
        assert_eq!(
            wrapped,
            lua.load(r#"return tostring(quarto.version)"#)
                .eval::<String>()
                .unwrap()
        );
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
    // Scoped require × script-dir contract tests (GH #588, bd-sr0nipl7)
    //
    // Loaders never move the script-dir stack: `resolve_path` called from
    // inside a module loaded via the scoped `require` must resolve against
    // the requiring script's root, exactly as it does from the top-level
    // script (Q1's scriptFile stack moves only at script boundaries).
    // =========================================================================

    fn create_require_test_lua() -> Lua {
        let lua = create_test_lua();
        let runtime = Arc::new(NativeRuntime::new());
        register_scoped_require(&lua, runtime).unwrap();
        lua
    }

    fn write_file(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
    }

    #[test]
    fn test_resolve_path_inside_required_module_uses_script_root() {
        let lua = create_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        let root = quarto_util::to_forward_slashes(tmp.path());

        // The module resolves an extension-root-relative path at load time.
        write_file(
            tmp.path(),
            "_modules/probe.lua",
            r#"return { resolved = quarto.utils.resolve_path("_modules/greet.lua") }"#,
        );
        write_file(tmp.path(), "_modules/greet.lua", r#"return "GREET""#);

        push_script_dir(&lua, &root).unwrap();
        let top: String = lua
            .load(r#"return quarto.utils.resolve_path("_modules/greet.lua")"#)
            .eval()
            .unwrap();
        let via_require: String = lua
            .load(r#"return require("_modules/probe").resolved"#)
            .eval()
            .unwrap();

        assert_eq!(top, format!("{}/_modules/greet.lua", root));
        // GH #588: this used to come back as .../_modules/_modules/greet.lua
        // because the scoped require pushed the module's own dir onto the
        // script-dir stack that resolve_path reads.
        assert_eq!(via_require, top);
    }

    #[test]
    fn test_script_dir_stack_unchanged_across_require() {
        let lua = create_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        let root = quarto_util::to_forward_slashes(tmp.path());

        write_file(tmp.path(), "_modules/noop.lua", r#"return true"#);

        push_script_dir(&lua, &root).unwrap();
        lua.load(r#"require("_modules/noop")"#).exec().unwrap();
        assert_eq!(current_script_dir(&lua).unwrap(), root);
    }

    #[test]
    fn test_require_bare_sibling_name_from_nested_module() {
        // Regression guard for #450's behavior: a loaded module can require
        // a file in its own directory by bare name. This must keep working
        // when the module dir moves off the shared script-dir stack.
        let lua = create_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        let root = quarto_util::to_forward_slashes(tmp.path());

        write_file(tmp.path(), "_modules/greet.lua", r#"return "GREET""#);
        write_file(
            tmp.path(),
            "_modules/probe.lua",
            r#"return { sib = require("greet") }"#,
        );

        push_script_dir(&lua, &root).unwrap();
        let sib: String = lua
            .load(r#"return require("_modules/probe").sib"#)
            .eval()
            .unwrap();
        assert_eq!(sib, "GREET");
    }

    #[test]
    fn test_require_root_relative_name_from_nested_module() {
        // Q1-style extension-root-relative require from inside a nested
        // module (`require("_modules/x")` written in a file under
        // `_modules/`): resolved via the script-dir stack, not the module
        // dir.
        let lua = create_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        let root = quarto_util::to_forward_slashes(tmp.path());

        write_file(tmp.path(), "_modules/greet.lua", r#"return "GREET""#);
        write_file(
            tmp.path(),
            "_modules/probe.lua",
            r#"return { sib = require("_modules/greet") }"#,
        );

        push_script_dir(&lua, &root).unwrap();
        let sib: String = lua
            .load(r#"return require("_modules/probe").sib"#)
            .eval()
            .unwrap();
        assert_eq!(sib, "GREET");
    }

    // =========================================================================
    // Scoped require × source dialects (bd-76ogq4ew)
    //
    // A registered dialect's extension is tried ahead of `.lua` for every
    // candidate form; a hit is compiled before `load`; a `.lua` of the same
    // stem beside it is a warning, not the winner.
    // =========================================================================

    fn create_dialect_require_test_lua() -> Lua {
        let lua = create_require_test_lua();
        super::super::dialect::register_source_dialect(
            &lua,
            Arc::new(super::super::dialect::test_support::Shout),
        )
        .unwrap();
        lua
    }

    fn drained_warnings(lua: &Lua) -> Vec<String> {
        super::super::diagnostics::extract_lua_diagnostics(lua)
            .unwrap()
            .into_iter()
            .map(|d| d.title)
            .collect()
    }

    #[test]
    fn test_require_loads_a_dialect_file_by_bare_name() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(tmp.path(), "greet.shout", r#"SHOUT "hello")"#);
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let got: String = lua.load(r#"return require("greet")"#).eval().unwrap();
        assert_eq!(got, "HELLO");
        assert!(drained_warnings(&lua).is_empty());
    }

    #[test]
    fn test_require_dialect_dotted_and_init_forms() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(tmp.path(), "ui/cards.shout", r#"SHOUT "cards")"#);
        write_file(tmp.path(), "widgets/init.shout", r#"SHOUT "widgets")"#);
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let dotted: String = lua.load(r#"return require("ui.cards")"#).eval().unwrap();
        let slashed: String = lua.load(r#"return require("ui/cards")"#).eval().unwrap();
        let init: String = lua.load(r#"return require("widgets")"#).eval().unwrap();
        assert_eq!(
            (dotted.as_str(), slashed.as_str(), init.as_str()),
            ("CARDS", "CARDS", "WIDGETS")
        );
    }

    #[test]
    fn test_require_dialect_wins_over_lua_sibling_with_a_warning() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(tmp.path(), "greet.shout", r#"SHOUT "from dialect")"#);
        write_file(tmp.path(), "greet.lua", r#"return "from stale lua""#);
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let got: String = lua.load(r#"return require("greet")"#).eval().unwrap();
        assert_eq!(got, "FROM DIALECT");
        let warnings = drained_warnings(&lua);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("greet.shout") && warnings[0].contains("greet.lua"),
            "{warnings:?}"
        );
    }

    // ---- quarto.include (bd-76ogq4ew, phase 3) ----

    fn create_include_test_lua() -> Lua {
        let lua = create_dialect_require_test_lua();
        register_quarto_include(&lua, Arc::new(NativeRuntime::new())).unwrap();
        lua
    }

    fn block_texts(lua: &Lua, code: &str) -> Vec<String> {
        lua.load(format!(
            "local bs = {code}\nlocal out = {{}}\nfor i, b in ipairs(bs) do out[i] = b.t .. ':' .. pandoc.utils.stringify(b) end\nreturn out"
        ))
        .eval::<Vec<String>>()
        .unwrap()
    }

    #[test]
    fn test_include_runs_a_lua_file_with_params_and_returns_blocks() {
        let lua = create_include_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "partials/card.lua",
            "local p = ...\nreturn { pandoc.Header(3, p.title), pandoc.Para(p.body) }",
        );
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let texts = block_texts(
            &lua,
            r#"quarto.include("partials/card.lua", { title = "Hi", body = "there" })"#,
        );
        assert_eq!(texts, vec!["Header:Hi", "Para:there"]);
        // A call, not a module: different params, different result, and
        // the file's own `require` cache is not involved.
        let again = block_texts(
            &lua,
            r#"quarto.include("partials/card.lua", { title = "Bye", body = "now" })"#,
        );
        assert_eq!(again, vec!["Header:Bye", "Para:now"]);
        // nil and no return mean no blocks; the result is a Blocks list.
        write_file(tmp.path(), "partials/empty.lua", "local _ = ...");
        let n: usize = lua
            .load(r#"return #quarto.include("partials/empty.lua")"#)
            .eval()
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn test_include_compiles_a_dialect_file() {
        // The toy dialect turns `SHOUT "x")` into `return string.upper("x")`,
        // a string, which is not blocks: the contract error names the file.
        let lua = create_include_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(tmp.path(), "partials/loud.shout", r#"SHOUT "x")"#);
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let err = lua
            .load(r#"return quarto.include("partials/loud.shout")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("loud.shout") && err.contains("must return a Block"),
            "{err}"
        );
        // A compile failure surfaces as the typed module error.
        write_file(tmp.path(), "partials/broken.shout", "BOOM");
        let err = lua
            .load(r#"return quarto.include("partials/broken.shout")"#)
            .eval::<mlua::Value>()
            .unwrap_err();
        assert!(
            crate::dialect::ModuleCompileError::find_in(&err).is_some(),
            "{err}"
        );
    }

    #[test]
    fn test_include_reads_a_markdown_file_as_a_fragment() {
        let lua = create_include_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "partials/note.md",
            "## A heading\n\nText[^1] here.\n\n[^1]: the note\n",
        );
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let texts = block_texts(&lua, r#"quarto.include("partials/note.md")"#);
        // `stringify` flattens the Note's content into the paragraph.
        assert_eq!(texts, vec!["Header:A heading", "Para:Textthe note here."]);
        let (id, has_note): (String, bool) = lua
            .load(
                r#"local bs = quarto.include("partials/note.md")
                   local note = false
                   bs[2]:walk({ Note = function() note = true end })
                   return bs[1].identifier, note"#,
            )
            .eval()
            .unwrap();
        assert_eq!(id, "", "derived heading id dropped");
        assert!(has_note, "local footnote paired into a Note");
        let err = lua
            .load(r#"return quarto.include("partials/note.md", { x = 1 })"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("takes no params"), "{err}");
    }

    #[test]
    fn test_include_resolves_beside_the_including_module_and_against_the_project_root() {
        let lua = create_include_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "mods/m.lua",
            r#"return { part = function() return quarto.include("part.md") end }"#,
        );
        write_file(tmp.path(), "mods/part.md", "beside the module\n");
        write_file(tmp.path(), "part.md", "beside the script\n");
        write_file(tmp.path(), "shared/footer.md", "at the root\n");
        set_module_roots(&lua, roots(Some(tmp.path()), &[]));
        push_script_dir(
            &lua,
            &quarto_util::to_forward_slashes(&tmp.path().join("docs")),
        )
        .unwrap();
        // From the script: its own directory (docs/) has no part.md.
        let err = lua
            .load(r#"return quarto.include("part.md")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot read"), "{err}");
        // From inside a module: the module's directory.
        pop_script_dir(&lua).unwrap();
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let texts = block_texts(&lua, r#"require("mods/m").part()"#);
        assert_eq!(texts, vec!["Para:beside the module"]);
        let texts = block_texts(&lua, r#"quarto.include("part.md")"#);
        assert_eq!(texts, vec!["Para:beside the script"]);
        // A leading slash: the project root.
        let texts = block_texts(&lua, r#"quarto.include("/shared/footer.md")"#);
        assert_eq!(texts, vec!["Para:at the root"]);
        // An existing absolute path is read as itself.
        let abs = quarto_util::to_forward_slashes(&tmp.path().join("mods/part.md"));
        let texts = block_texts(&lua, &format!(r#"quarto.include("{abs}")"#));
        assert_eq!(texts, vec!["Para:beside the module"]);
    }

    #[test]
    fn test_include_applies_the_structural_desugaring_set() {
        // A paragraph holding one image is an implicit figure, as it would
        // be from the reader (dev-docs/syntax-notes.md).
        let lua = create_include_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "fig.lua",
            r#"return pandoc.Para({ pandoc.Image({pandoc.Str("cap")}, "a.png", "fig:") })"#,
        );
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let tags: Vec<String> = lua
            .load(r#"local bs = quarto.include("fig.lua"); return { bs[1].t }"#)
            .eval()
            .unwrap();
        assert_eq!(tags, vec!["Figure"]);
    }

    #[test]
    fn test_include_reads_only_through_the_runtime_so_a_read_policy_governs_it() {
        use quarto_system_runtime::{SandboxedRuntime, SecurityPolicy};
        let tmp = tempfile::TempDir::new().unwrap();
        let allowed = tmp.path().join("proj");
        let denied = tmp.path().join("vendor");
        write_file(&allowed, "ok.md", "fine\n");
        write_file(&denied, "secret.md", "secret\n");
        let lua = create_test_lua();
        let sandboxed: Arc<dyn crate::lua::runtime::SystemRuntime> =
            Arc::new(SandboxedRuntime::new(
                NativeRuntime::new(),
                SecurityPolicy::untrusted(allowed.clone()),
            ));
        register_scoped_require(&lua, sandboxed.clone()).unwrap();
        register_quarto_include(&lua, sandboxed).unwrap();
        push_script_dir(&lua, &quarto_util::to_forward_slashes(&allowed)).unwrap();
        let texts = block_texts(&lua, r#"quarto.include("ok.md")"#);
        assert_eq!(texts, vec!["Para:fine"]);
        // Relative traversal out of the allowed directory: the read is
        // refused by the policy, which judges the normalized path.
        let err = lua
            .load(r#"return quarto.include("../vendor/secret.md")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot read"), "{err}");
    }

    // ---- module roots (bd-76ogq4ew, phase 3) ----

    fn roots(
        project: Option<&Path>,
        extensions: &[(&str, &Path)],
    ) -> crate::lua_options::ModuleRoots {
        crate::lua_options::ModuleRoots {
            project: project.map(Path::to_path_buf),
            extensions: extensions
                .iter()
                .map(|(name, dir)| crate::lua_options::ExtensionRoot {
                    name: name.to_string(),
                    dir: dir.to_path_buf(),
                })
                .collect(),
        }
    }

    #[test]
    fn test_require_with_a_leading_slash_resolves_against_the_project_root() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "components/cards.lua",
            r#"return "project cards""#,
        );
        write_file(
            tmp.path(),
            "components/deck.shout",
            r#"SHOUT "project deck")"#,
        );
        write_file(
            tmp.path(),
            "docs/components/cards.lua",
            r#"return "local cards""#,
        );
        set_module_roots(&lua, roots(Some(tmp.path()), &[]));
        push_script_dir(
            &lua,
            &quarto_util::to_forward_slashes(&tmp.path().join("docs")),
        )
        .unwrap();
        let rooted: String = lua
            .load(r#"return require("/components/cards")"#)
            .eval()
            .unwrap();
        assert_eq!(rooted, "project cards");
        let dialect: String = lua
            .load(r#"return require("/components/deck")"#)
            .eval()
            .unwrap();
        assert_eq!(dialect, "PROJECT DECK");
        // Without the slash the script directory is searched, as before.
        let local: String = lua
            .load(r#"return require("components/cards")"#)
            .eval()
            .unwrap();
        assert_eq!(local, "local cards");
        // A rooted name never falls back to the script directories.
        let err = lua
            .load(r#"return require("/nowhere")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("not found"), "{err}");
        assert!(err.contains("/nowhere"), "{err}");
    }

    /// GH #587 contract, kept: an absolute path is required as itself
    /// (Quarto 1 filters do `require(quarto.utils.resolve_path(...))`),
    /// whether or not a project root is set.
    #[test]
    fn test_require_of_an_absolute_path_is_literal() {
        let lua = create_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(tmp.path(), "_modules/greet.lua", r#"return "abs""#);
        set_module_roots(&lua, roots(Some(&tmp.path().join("elsewhere")), &[]));
        push_script_dir(
            &lua,
            &quarto_util::to_forward_slashes(&tmp.path().join("docs")),
        )
        .unwrap();
        // On Windows the spelling is `C:/…`, which takes the ordinary
        // path (a join with an absolute path is that path); on Unix the
        // leading `/` takes the rooted branch, which tries it as itself.
        let abs = quarto_util::to_forward_slashes(&tmp.path().join("_modules/greet"));
        let got: String = lua
            .load(format!(r#"return require("{abs}")"#))
            .eval()
            .unwrap();
        assert_eq!(got, "abs");
    }

    #[test]
    fn test_require_with_a_leading_slash_and_no_project_root_says_so() {
        let lua = create_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let err = lua
            .load(r#"return require("/components/cards")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("no project root"), "{err}");
    }

    #[test]
    fn test_require_resolves_an_extension_by_its_name() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        let ext = tmp.path().join("_extensions/cards");
        write_file(&ext, "deck.lua", r#"return "ext deck""#);
        write_file(&ext, "init.shout", r#"SHOUT "ext init")"#);
        write_file(&ext, "ui/badge.shout", r#"SHOUT "ext badge")"#);
        set_module_roots(&lua, roots(None, &[("cards", &ext)]));
        push_script_dir(
            &lua,
            &quarto_util::to_forward_slashes(&tmp.path().join("docs")),
        )
        .unwrap();
        let slashed: String = lua.load(r#"return require("cards/deck")"#).eval().unwrap();
        let dotted: String = lua
            .load(r#"return require("cards.ui.badge")"#)
            .eval()
            .unwrap();
        let bare: String = lua.load(r#"return require("cards")"#).eval().unwrap();
        assert_eq!(
            (slashed.as_str(), dotted.as_str(), bare.as_str()),
            ("ext deck", "EXT BADGE", "EXT INIT")
        );
        // An extension that is not registered is not searched.
        let err = lua
            .load(r#"return require("other/thing")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("not found"), "{err}");
    }

    #[test]
    fn test_require_prefers_a_local_directory_over_an_extension_of_the_same_name() {
        let lua = create_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        let ext = tmp.path().join("_extensions/cards");
        write_file(&ext, "deck.lua", r#"return "ext deck""#);
        write_file(tmp.path(), "docs/cards/deck.lua", r#"return "local deck""#);
        set_module_roots(&lua, roots(None, &[("cards", &ext)]));
        push_script_dir(
            &lua,
            &quarto_util::to_forward_slashes(&tmp.path().join("docs")),
        )
        .unwrap();
        let got: String = lua.load(r#"return require("cards/deck")"#).eval().unwrap();
        assert_eq!(got, "local deck");
    }

    #[test]
    fn test_module_roots_later_extension_of_the_same_name_wins() {
        let lua = create_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        let builtin = tmp.path().join("builtin/cards");
        let user = tmp.path().join("_extensions/cards");
        write_file(&builtin, "deck.lua", r#"return "builtin""#);
        write_file(&user, "deck.lua", r#"return "user""#);
        set_module_roots(&lua, roots(None, &[("cards", &builtin), ("cards", &user)]));
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let got: String = lua.load(r#"return require("cards/deck")"#).eval().unwrap();
        assert_eq!(got, "user");
    }

    #[test]
    fn test_require_dialect_file_is_ignored_when_no_dialect_is_registered() {
        let lua = create_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(tmp.path(), "greet.shout", r#"SHOUT "hello")"#);
        write_file(tmp.path(), "greet.lua", r#"return "plain""#);
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let got: String = lua.load(r#"return require("greet")"#).eval().unwrap();
        assert_eq!(got, "plain");
    }

    #[test]
    fn test_require_dialect_compile_error_names_module_and_position() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(tmp.path(), "bad.shout", "SHOUT \"ok\")\nBOOM\n");
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let err = lua
            .load(r#"return require("bad")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("module 'bad'"), "{err}");
        assert!(err.contains("bad.shout:2: BOOM is not a word"), "{err}");
    }

    #[test]
    fn test_require_dialect_runtime_error_reports_module_file_and_line() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(tmp.path(), "boom.shout", "local x = 1\nerror('kaboom')\n");
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let err = lua
            .load(r#"return require("boom")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        // `@path` chunk names make Lua print `path:line:`; no `[string "…"]`.
        assert!(err.contains("boom.shout:2: kaboom"), "{err}");
        assert!(!err.contains("[string"), "{err}");
    }

    #[test]
    fn test_require_from_inside_a_dialect_module_resolves_beside_it() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "lib/outer.shout",
            "local inner = require(\"inner\")\nSHOUT inner)\n",
        );
        write_file(tmp.path(), "lib/inner.shout", r#"SHOUT "nested")"#);
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let got: String = lua.load(r#"return require("lib/outer")"#).eval().unwrap();
        assert_eq!(got, "NESTED");
    }

    #[test]
    fn test_require_dialect_module_is_cached_per_state() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "counter.shout",
            "_G.loads = (_G.loads or 0) + 1\nSHOUT \"x\")\n",
        );
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let loads: i64 = lua
            .load(r#"require("counter"); require("counter"); return loads"#)
            .eval()
            .unwrap();
        assert_eq!(loads, 1);
    }

    #[test]
    fn test_require_miss_lists_dialect_candidates_before_the_stock_search() {
        let lua = create_dialect_require_test_lua();
        let tmp = tempfile::TempDir::new().unwrap();
        push_script_dir(&lua, &quarto_util::to_forward_slashes(tmp.path())).unwrap();
        let err = lua
            .load(r#"return require("nowhere")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("module 'nowhere' not found"), "{err}");
        assert!(err.contains("nowhere.shout"), "{err}");
        assert!(err.contains("nowhere.lua"), "{err}");
        // The scoped search is reported first; Lua's own search is the hint.
        let scoped = err.find("nowhere.shout").unwrap();
        let stock = err.find("package.preload").unwrap_or(usize::MAX);
        assert!(scoped < stock, "{err}");
    }

    #[test]
    fn test_require_reads_only_through_the_runtime_so_a_read_policy_governs_it() {
        // The loader never touches std::fs: a SandboxedRuntime whose policy
        // excludes the module's directory makes `require` fail with the
        // policy's error, for `.lua` and dialect files alike (bd-76ogq4ew).
        use quarto_system_runtime::{SandboxedRuntime, SecurityPolicy};
        let tmp = tempfile::TempDir::new().unwrap();
        let allowed = tmp.path().join("proj");
        let denied = tmp.path().join("vendor");
        write_file(&allowed, "ok.shout", r#"SHOUT "ok")"#);
        write_file(&denied, "secret.lua", r#"return "secret""#);
        write_file(&denied, "secret.shout", r#"SHOUT "secret")"#);

        let lua = create_test_lua();
        let sandboxed: Arc<dyn crate::lua::runtime::SystemRuntime> =
            Arc::new(SandboxedRuntime::new(
                NativeRuntime::new(),
                SecurityPolicy::untrusted(allowed.clone()),
            ));
        register_scoped_require(&lua, sandboxed).unwrap();
        super::super::dialect::register_source_dialect(
            &lua,
            Arc::new(super::super::dialect::test_support::Shout),
        )
        .unwrap();

        push_script_dir(&lua, &quarto_util::to_forward_slashes(&allowed)).unwrap();
        let ok: String = lua.load(r#"return require("ok")"#).eval().unwrap();
        assert_eq!(ok, "OK");

        pop_script_dir(&lua).unwrap();
        push_script_dir(&lua, &quarto_util::to_forward_slashes(&denied)).unwrap();
        let err = lua
            .load(r#"return require("secret")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("module 'secret' not found"), "{err}");
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

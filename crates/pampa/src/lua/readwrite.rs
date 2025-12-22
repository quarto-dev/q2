/*
 * lua/readwrite.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Lua bindings for pandoc.read and pandoc.write functions.
 *
 * This module provides Pandoc-compatible API for reading and writing documents
 * in Lua filters. Options are represented as plain Lua tables with metatables
 * for type identification.
 */

use mlua::{Error, Lua, LuaSerdeExt, Result, Table, Value};

use crate::options::{
    ParsedFormat, SUPPORTED_READER_FORMATS, SUPPORTED_WRITER_FORMATS, default_reader_options,
    default_writer_options, is_supported_reader_format, is_supported_writer_format,
    merge_with_defaults, normalize_reader_format, normalize_writer_format, parse_format_string,
};
use crate::pandoc::{MetaValueWithSourceInfo, Pandoc};

use super::types::{blocks_to_lua_table, lua_table_to_blocks, meta_value_to_lua};

/// Register pandoc.read, pandoc.write, and option constructors on the pandoc table.
pub fn register_pandoc_readwrite(lua: &Lua, pandoc: &Table) -> Result<()> {
    // Register format lists
    register_format_lists(lua, pandoc)?;

    // Register option constructors
    register_option_constructors(lua, pandoc)?;

    // Register pandoc.read
    pandoc.set(
        "read",
        lua.create_function(|lua, args: mlua::MultiValue| pandoc_read(lua, args))?,
    )?;

    // Register pandoc.write
    pandoc.set(
        "write",
        lua.create_function(|lua, args: mlua::MultiValue| pandoc_write(lua, args))?,
    )?;

    Ok(())
}

/// Register pandoc.readers and pandoc.writers format lists.
fn register_format_lists(lua: &Lua, pandoc: &Table) -> Result<()> {
    // pandoc.readers - set of supported reader format names
    let readers = lua.create_table()?;
    for format in SUPPORTED_READER_FORMATS {
        readers.set(*format, true)?;
    }
    pandoc.set("readers", readers)?;

    // pandoc.writers - set of supported writer format names
    let writers = lua.create_table()?;
    for format in SUPPORTED_WRITER_FORMATS {
        writers.set(*format, true)?;
    }
    pandoc.set("writers", writers)?;

    Ok(())
}

/// Register pandoc.ReaderOptions and pandoc.WriterOptions constructors.
fn register_option_constructors(lua: &Lua, pandoc: &Table) -> Result<()> {
    // pandoc.ReaderOptions(opts?) - creates a ReaderOptions table
    pandoc.set(
        "ReaderOptions",
        lua.create_function(|lua, opts: Option<Table>| create_reader_options_table(lua, opts))?,
    )?;

    // pandoc.WriterOptions(opts?) - creates a WriterOptions table
    pandoc.set(
        "WriterOptions",
        lua.create_function(|lua, opts: Option<Table>| create_writer_options_table(lua, opts))?,
    )?;

    Ok(())
}

/// Create a ReaderOptions table with defaults and optional overrides.
pub fn create_reader_options_table(lua: &Lua, opts: Option<Table>) -> Result<Table> {
    // Get default options as serde_json::Value
    let defaults = default_reader_options();

    // Merge with user-provided options if any
    let merged = if let Some(user_table) = opts {
        let user_json: serde_json::Value = lua.from_value(Value::Table(user_table))?;
        merge_with_defaults(defaults, &user_json)
    } else {
        defaults
    };

    // Convert to Lua table
    let lua_val = lua.to_value(&merged)?;
    let table = lua_val
        .as_table()
        .ok_or_else(|| Error::runtime("Failed to create ReaderOptions table"))?
        .clone();

    // Set metatable for type identification
    let mt = lua.create_table()?;
    mt.set("__name", "ReaderOptions")?;
    table.set_metatable(Some(mt));

    Ok(table)
}

/// Create a WriterOptions table with defaults and optional overrides.
pub fn create_writer_options_table(lua: &Lua, opts: Option<Table>) -> Result<Table> {
    // Get default options as serde_json::Value
    let defaults = default_writer_options();

    // Merge with user-provided options if any
    let merged = if let Some(user_table) = opts {
        let user_json: serde_json::Value = lua.from_value(Value::Table(user_table))?;
        merge_with_defaults(defaults, &user_json)
    } else {
        defaults
    };

    // Convert to Lua table
    let lua_val = lua.to_value(&merged)?;
    let table = lua_val
        .as_table()
        .ok_or_else(|| Error::runtime("Failed to create WriterOptions table"))?
        .clone();

    // Set metatable for type identification
    let mt = lua.create_table()?;
    mt.set("__name", "WriterOptions")?;
    table.set_metatable(Some(mt));

    Ok(table)
}

/// Parse a format specification from a Lua value.
///
/// The format can be:
/// - A string like "markdown" or "markdown+smart-citations"
/// - A table with `format` and optional `extensions` fields
fn parse_format_spec_from_lua(_lua: &Lua, value: Value) -> Result<ParsedFormat> {
    match value {
        Value::String(s) => {
            let spec = s.to_str()?;
            Ok(parse_format_string(&spec))
        }
        Value::Table(t) => {
            // Table form: { format = "markdown", extensions = {...} }
            let base_format: String = t.get("format").unwrap_or_else(|_| "markdown".to_string());

            let mut enable = Vec::new();
            let mut disable = Vec::new();

            if let Ok(ext_table) = t.get::<Table>("extensions") {
                // Extensions can be a list or key-value table
                for pair in ext_table.pairs::<Value, Value>() {
                    let (key, val) = pair?;
                    match key {
                        Value::Integer(_) => {
                            // List form: extensions = {"smart", "footnotes"}
                            if let Value::String(ext_name) = val {
                                enable.push(ext_name.to_str()?.to_string());
                            }
                        }
                        Value::String(ext_name) => {
                            // Key-value form: extensions = { smart = true, citations = false }
                            let ext = ext_name.to_str()?.to_string();
                            match val {
                                Value::Boolean(true) => enable.push(ext),
                                Value::Boolean(false) => disable.push(ext),
                                Value::String(s) if s.to_str()? == "enable" => enable.push(ext),
                                Value::String(s) if s.to_str()? == "disable" => disable.push(ext),
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }

            Ok(ParsedFormat {
                base_format,
                extensions: crate::options::ExtensionsDiff { enable, disable },
            })
        }
        Value::Nil => Ok(ParsedFormat {
            base_format: "markdown".to_string(),
            extensions: crate::options::ExtensionsDiff::default(),
        }),
        _ => Err(Error::runtime(
            "format must be a string or table with 'format' field",
        )),
    }
}

/// Convert MetaValueWithSourceInfo to a Lua table (loses source info).
fn meta_with_source_to_lua(lua: &Lua, meta: &MetaValueWithSourceInfo) -> Result<Value> {
    // Convert to the old MetaValue format and then to Lua
    let meta_value = meta.to_meta_value();
    meta_value_to_lua(lua, &meta_value)
}

/// Convert a Lua table to MetaValueWithSourceInfo.
fn lua_to_meta_with_source(lua: &Lua, val: Value) -> Result<MetaValueWithSourceInfo> {
    use crate::pandoc::MetaMapEntry;
    use quarto_source_map::SourceInfo;

    match val {
        Value::Nil => Ok(MetaValueWithSourceInfo::MetaMap {
            entries: Vec::new(),
            source_info: SourceInfo::default(),
        }),
        Value::Boolean(b) => Ok(MetaValueWithSourceInfo::MetaBool {
            value: b,
            source_info: SourceInfo::default(),
        }),
        Value::String(s) => Ok(MetaValueWithSourceInfo::MetaString {
            value: s.to_str()?.to_string(),
            source_info: SourceInfo::default(),
        }),
        Value::Table(t) => {
            // Check if it's a sequence (array) or a map
            let len = t.raw_len();
            if len > 0 {
                // It's a list
                let mut items = Vec::new();
                for i in 1..=len {
                    let item: Value = t.get(i)?;
                    items.push(lua_to_meta_with_source(lua, item)?);
                }
                Ok(MetaValueWithSourceInfo::MetaList {
                    items,
                    source_info: SourceInfo::default(),
                })
            } else {
                // It's a map
                let mut entries = Vec::new();
                for pair in t.pairs::<String, Value>() {
                    let (k, v) = pair?;
                    entries.push(MetaMapEntry {
                        key: k,
                        key_source: SourceInfo::default(),
                        value: lua_to_meta_with_source(lua, v)?,
                    });
                }
                Ok(MetaValueWithSourceInfo::MetaMap {
                    entries,
                    source_info: SourceInfo::default(),
                })
            }
        }
        _ => Err(Error::runtime(
            "cannot convert value to MetaValueWithSourceInfo",
        )),
    }
}

/// Convert a Rust Pandoc document to a Lua table.
///
/// Returns a table with `blocks`, `meta`, and `pandoc-api-version` fields.
fn rust_pandoc_to_lua_table(lua: &Lua, pandoc: &Pandoc) -> Result<Value> {
    let doc = lua.create_table()?;

    // Convert blocks
    let blocks_lua = blocks_to_lua_table(lua, &pandoc.blocks)?;
    doc.set("blocks", blocks_lua)?;

    // Convert meta (MetaValueWithSourceInfo -> Lua table)
    let meta_lua = meta_with_source_to_lua(lua, &pandoc.meta)?;
    doc.set("meta", meta_lua)?;

    // Set pandoc-api-version (we use 1.23 for compatibility)
    let api_version = lua.create_table()?;
    api_version.set(1, 1)?;
    api_version.set(2, 23)?;
    doc.set("pandoc-api-version", api_version)?;

    // Set metatable with __name for type identification
    let mt = lua.create_table()?;
    mt.set("__name", "Pandoc")?;
    doc.set_metatable(Some(mt));

    Ok(Value::Table(doc))
}

/// Convert a Lua Pandoc table to a Rust Pandoc struct.
fn lua_pandoc_to_rust(lua: &Lua, val: Value) -> Result<Pandoc> {
    match val {
        Value::Table(t) => {
            // Get blocks
            let blocks_val: Value = t.get("blocks").unwrap_or(Value::Nil);
            let blocks = lua_table_to_blocks(lua, blocks_val)?;

            // Get meta (convert to MetaValueWithSourceInfo)
            let meta_val: Value = t.get("meta").unwrap_or(Value::Nil);
            let meta = lua_to_meta_with_source(lua, meta_val)?;

            Ok(Pandoc { blocks, meta })
        }
        _ => Err(Error::runtime("Expected Pandoc document table")),
    }
}

/// Implementation of pandoc.read(content, format?, reader_options?, read_env?)
///
/// Reads content in the specified format and returns a Pandoc document.
fn pandoc_read(lua: &Lua, args: mlua::MultiValue) -> Result<Value> {
    let mut args_iter = args.into_iter();

    // First argument: content (required)
    let content_val = args_iter
        .next()
        .ok_or_else(|| Error::runtime("pandoc.read requires at least one argument (content)"))?;
    let content = match content_val {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return Err(Error::runtime("pandoc.read: content must be a string")),
    };

    // Second argument: format (optional, defaults to "markdown")
    let format_val = args_iter.next().unwrap_or(Value::Nil);
    let parsed_format = parse_format_spec_from_lua(lua, format_val)?;

    // Normalize and validate format
    let base_format = normalize_reader_format(&parsed_format.base_format);
    if !is_supported_reader_format(base_format) {
        return Err(Error::runtime(format!(
            "Unsupported reader format: {}. Supported formats: {:?}",
            parsed_format.base_format, SUPPORTED_READER_FORMATS
        )));
    }

    // Third argument: reader_options (optional)
    let reader_options_val = args_iter.next().unwrap_or(Value::Nil);

    // Build options JSON for internal use
    let mut opts_json = serde_json::json!({
        "format": base_format,
        "extensions": {
            "enable": parsed_format.extensions.enable,
            "disable": parsed_format.extensions.disable,
        }
    });

    // Merge reader_options (excluding extensions field per Pandoc behavior)
    if let Value::Table(opts_table) = reader_options_val {
        let user_opts: serde_json::Value = lua.from_value(Value::Table(opts_table))?;
        if let serde_json::Value::Object(user_map) = user_opts {
            if let serde_json::Value::Object(ref mut opts_map) = opts_json {
                for (key, value) in user_map {
                    if key != "extensions" {
                        opts_map.insert(key, value);
                    }
                }
            }
        }
    }

    // Fourth argument: read_env (optional, not implemented yet)
    // let _read_env = args_iter.next();

    // Dispatch to the appropriate reader
    match base_format {
        "qmd" => {
            // Use the QMD reader
            let mut stderr = std::io::stderr();
            match crate::readers::qmd::read(
                &content,
                false, // loose
                "<pandoc.read>",
                &mut stderr,
                true, // prune_errors
                None, // parent_source_info
            ) {
                Ok((pandoc, _context, _warnings)) => {
                    // Convert Rust Pandoc to Lua
                    rust_pandoc_to_lua_table(lua, &pandoc)
                }
                Err(diagnostics) => {
                    // Convert diagnostics to error message
                    let messages: Vec<String> =
                        diagnostics.iter().map(|d| d.title.clone()).collect();
                    Err(Error::runtime(format!(
                        "pandoc.read failed: {}",
                        messages.join("; ")
                    )))
                }
            }
        }
        "json" => {
            // Use the JSON reader
            let mut cursor = std::io::Cursor::new(&content);
            match crate::readers::json::read(&mut cursor) {
                Ok((pandoc, _context)) => rust_pandoc_to_lua_table(lua, &pandoc),
                Err(e) => Err(Error::runtime(format!("pandoc.read (json) failed: {}", e))),
            }
        }
        _ => Err(Error::runtime(format!(
            "Unsupported reader format: {}",
            base_format
        ))),
    }
}

/// Implementation of pandoc.write(doc, format?, writer_options?)
///
/// Writes a Pandoc document to a string in the specified format.
fn pandoc_write(lua: &Lua, args: mlua::MultiValue) -> Result<mlua::String> {
    let mut args_iter = args.into_iter();

    // First argument: doc (required)
    let doc_val = args_iter
        .next()
        .ok_or_else(|| Error::runtime("pandoc.write requires at least one argument (doc)"))?;

    // Convert Lua Pandoc to Rust
    let pandoc = lua_pandoc_to_rust(lua, doc_val)?;

    // Second argument: format (optional, defaults to "html")
    let format_val = args_iter.next().unwrap_or(Value::Nil);
    let parsed_format = if format_val == Value::Nil {
        ParsedFormat {
            base_format: "html".to_string(),
            extensions: crate::options::ExtensionsDiff::default(),
        }
    } else {
        parse_format_spec_from_lua(lua, format_val)?
    };

    // Normalize and validate format
    let base_format = normalize_writer_format(&parsed_format.base_format);
    if !is_supported_writer_format(base_format) {
        return Err(Error::runtime(format!(
            "Unsupported writer format: {}. Supported formats: {:?}",
            parsed_format.base_format, SUPPORTED_WRITER_FORMATS
        )));
    }

    // Third argument: writer_options (optional)
    let writer_options_val = args_iter.next().unwrap_or(Value::Nil);

    // Build options JSON for internal use (unused for now, but stored for future)
    let mut _opts_json = serde_json::json!({
        "format": base_format,
        "extensions": {
            "enable": parsed_format.extensions.enable,
            "disable": parsed_format.extensions.disable,
        }
    });

    // Merge writer_options (excluding extensions field)
    if let Value::Table(opts_table) = writer_options_val {
        let user_opts: serde_json::Value = lua.from_value(Value::Table(opts_table))?;
        if let serde_json::Value::Object(user_map) = user_opts {
            if let serde_json::Value::Object(ref mut opts_map) = _opts_json {
                for (key, value) in user_map {
                    if key != "extensions" {
                        opts_map.insert(key, value);
                    }
                }
            }
        }
    }

    // Dispatch to the appropriate writer
    let output = match base_format {
        "html" => {
            // Need an ASTContext for HTML writer (for source tracking support)
            let context = crate::pandoc::ast_context::ASTContext::anonymous();
            let mut buf = Vec::new();
            crate::writers::html::write(&pandoc, &context, &mut buf)
                .map_err(|e| Error::runtime(format!("pandoc.write (html) failed: {}", e)))?;
            String::from_utf8(buf)
                .map_err(|e| Error::runtime(format!("Invalid UTF-8 in output: {}", e)))?
        }
        "json" => {
            // Need an ASTContext for JSON writer
            let context = crate::pandoc::ast_context::ASTContext::anonymous();
            let mut buf = Vec::new();
            crate::writers::json::write(&pandoc, &context, &mut buf).map_err(|e| {
                let messages: Vec<String> = e.iter().map(|d| d.title.clone()).collect();
                Error::runtime(format!(
                    "pandoc.write (json) failed: {}",
                    messages.join("; ")
                ))
            })?;
            String::from_utf8(buf)
                .map_err(|e| Error::runtime(format!("Invalid UTF-8 in output: {}", e)))?
        }
        "native" => {
            let context = crate::pandoc::ast_context::ASTContext::anonymous();
            let mut buf = Vec::new();
            crate::writers::native::write(&pandoc, &context, &mut buf).map_err(|e| {
                let messages: Vec<String> = e.iter().map(|d| d.title.clone()).collect();
                Error::runtime(format!(
                    "pandoc.write (native) failed: {}",
                    messages.join("; ")
                ))
            })?;
            String::from_utf8(buf)
                .map_err(|e| Error::runtime(format!("Invalid UTF-8 in output: {}", e)))?
        }
        "qmd" => {
            let mut buf = Vec::new();
            crate::writers::qmd::write(&pandoc, &mut buf).map_err(|e| {
                let messages: Vec<String> = e.iter().map(|d| d.title.clone()).collect();
                Error::runtime(format!(
                    "pandoc.write (qmd) failed: {}",
                    messages.join("; ")
                ))
            })?;
            String::from_utf8(buf)
                .map_err(|e| Error::runtime(format!("Invalid UTF-8 in output: {}", e)))?
        }
        "plain" => {
            // plaintext writer works on blocks, so we write them directly
            let mut buf = Vec::new();
            let mut ctx = crate::writers::plaintext::PlainTextWriterContext::new();
            crate::writers::plaintext::write_blocks(&pandoc.blocks, &mut buf, &mut ctx)
                .map_err(|e| Error::runtime(format!("pandoc.write (plain) failed: {}", e)))?;
            String::from_utf8(buf)
                .map_err(|e| Error::runtime(format!("Invalid UTF-8 in output: {}", e)))?
        }
        _ => {
            return Err(Error::runtime(format!(
                "Unsupported writer format: {}",
                base_format
            )));
        }
    };

    lua.create_string(&output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_format_string_simple() {
        let parsed = parse_format_string("markdown");
        assert_eq!(parsed.base_format, "markdown");
        assert!(parsed.extensions.enable.is_empty());
        assert!(parsed.extensions.disable.is_empty());
    }

    #[test]
    fn test_parse_format_string_with_extensions() {
        let parsed = parse_format_string("markdown+smart-citations");
        assert_eq!(parsed.base_format, "markdown");
        assert_eq!(parsed.extensions.enable, vec!["smart"]);
        assert_eq!(parsed.extensions.disable, vec!["citations"]);
    }

    #[test]
    fn test_reader_options_default() {
        let lua = Lua::new();
        let opts = create_reader_options_table(&lua, None).unwrap();

        // Check default values
        assert_eq!(opts.get::<i64>("columns").unwrap(), 80);
        assert_eq!(opts.get::<i64>("tab_stop").unwrap(), 4);
        assert!(!opts.get::<bool>("standalone").unwrap());

        // Check metatable
        let mt = opts.metatable().unwrap();
        assert_eq!(mt.get::<String>("__name").unwrap(), "ReaderOptions");
    }

    #[test]
    fn test_writer_options_default() {
        let lua = Lua::new();
        let opts = create_writer_options_table(&lua, None).unwrap();

        // Check default values
        assert_eq!(opts.get::<i64>("columns").unwrap(), 72);
        assert_eq!(opts.get::<i64>("dpi").unwrap(), 96);
        assert!(!opts.get::<bool>("number_sections").unwrap());

        // Check metatable
        let mt = opts.metatable().unwrap();
        assert_eq!(mt.get::<String>("__name").unwrap(), "WriterOptions");
    }

    #[test]
    fn test_reader_options_with_overrides() {
        let lua = Lua::new();
        let user_opts = lua.create_table().unwrap();
        user_opts.set("columns", 120).unwrap();
        user_opts.set("standalone", true).unwrap();

        let opts = create_reader_options_table(&lua, Some(user_opts)).unwrap();

        assert_eq!(opts.get::<i64>("columns").unwrap(), 120);
        assert!(opts.get::<bool>("standalone").unwrap());
        // Other defaults should still be present
        assert_eq!(opts.get::<i64>("tab_stop").unwrap(), 4);
    }

    #[test]
    fn test_register_format_lists() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_format_lists(&lua, &pandoc).unwrap();

        // Check readers
        let readers: Table = pandoc.get("readers").unwrap();
        assert!(readers.get::<bool>("qmd").unwrap());
        assert!(readers.get::<bool>("markdown").unwrap());
        assert!(readers.get::<bool>("json").unwrap());

        // Check writers
        let writers: Table = pandoc.get("writers").unwrap();
        assert!(writers.get::<bool>("html").unwrap());
        assert!(writers.get::<bool>("json").unwrap());
        assert!(writers.get::<bool>("native").unwrap());
        assert!(writers.get::<bool>("qmd").unwrap());
        assert!(writers.get::<bool>("plain").unwrap());
    }

    #[test]
    fn test_pandoc_read_markdown() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_readwrite(&lua, &pandoc).unwrap();
        lua.globals().set("pandoc", pandoc).unwrap();

        // Read simple markdown
        let result: Value = lua
            .load(r#"return pandoc.read("Hello *world*", "markdown")"#)
            .eval()
            .unwrap();

        // Check it's a table with blocks
        let doc = result.as_table().unwrap();
        let blocks: Table = doc.get("blocks").unwrap();
        assert!(blocks.raw_len() > 0);
    }

    #[test]
    fn test_pandoc_write_html() {
        let lua = Lua::new();

        // We need the full pandoc namespace for read/write
        super::super::constructors::register_pandoc_namespace(
            &lua,
            std::sync::Arc::new(super::super::runtime::NativeRuntime::new()),
            super::super::mediabag::create_shared_mediabag(),
        )
        .unwrap();

        // Create a simple document via pandoc.read and write it as HTML
        let result: String = lua
            .load(
                r#"
                local doc = pandoc.read("Hello", "markdown")
                return pandoc.write(doc, "html")
            "#,
            )
            .eval()
            .unwrap();

        assert!(result.contains("<p>"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_pandoc_read_write_roundtrip() {
        let lua = Lua::new();

        // Register full namespace
        super::super::constructors::register_pandoc_namespace(
            &lua,
            std::sync::Arc::new(super::super::runtime::NativeRuntime::new()),
            super::super::mediabag::create_shared_mediabag(),
        )
        .unwrap();

        // Read markdown, write as JSON, read back, write as markdown
        let result: String = lua
            .load(
                "
                local doc = pandoc.read('# Hello\\n\\nWorld', 'markdown')
                local json = pandoc.write(doc, 'json')
                local doc2 = pandoc.read(json, 'json')
                return pandoc.write(doc2, 'qmd')
            ",
            )
            .eval()
            .unwrap();

        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
    }

    #[test]
    fn test_pandoc_write_native() {
        let lua = Lua::new();

        super::super::constructors::register_pandoc_namespace(
            &lua,
            std::sync::Arc::new(super::super::runtime::NativeRuntime::new()),
            super::super::mediabag::create_shared_mediabag(),
        )
        .unwrap();

        let result: String = lua
            .load(
                r#"
                local doc = pandoc.read("Test", "markdown")
                return pandoc.write(doc, "native")
            "#,
            )
            .eval()
            .unwrap();

        // Native format outputs blocks in Haskell-like syntax
        assert!(result.contains("Para"));
        assert!(result.contains("Str"));
        assert!(result.contains("Test"));
    }

    #[test]
    fn test_unsupported_reader_format() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_readwrite(&lua, &pandoc).unwrap();
        lua.globals().set("pandoc", pandoc).unwrap();

        let result: Result<Value> = lua.load(r#"return pandoc.read("test", "docx")"#).eval();

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported reader format"));
    }

    #[test]
    fn test_unsupported_writer_format() {
        let lua = Lua::new();

        super::super::constructors::register_pandoc_namespace(
            &lua,
            std::sync::Arc::new(super::super::runtime::NativeRuntime::new()),
            super::super::mediabag::create_shared_mediabag(),
        )
        .unwrap();

        let result: Result<Value> = lua
            .load(
                r#"
                local doc = pandoc.read("test", "markdown")
                return pandoc.write(doc, "docx")
            "#,
            )
            .eval();

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported writer format"));
    }
}

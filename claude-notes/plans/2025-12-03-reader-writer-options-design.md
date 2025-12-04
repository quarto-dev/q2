# Design: Reader/Writer Options for quarto-markdown-pandoc

**Date**: 2025-12-03
**Related Issue**: k-491 (Phase 6: Reader/Writer)
**Status**: Draft v2 - Revised after feedback

## Executive Summary

This document proposes a design for adding reader and writer options to quarto-markdown-pandoc, enabling the `pandoc.read` and `pandoc.write` Lua API functions. The primary goal is **API compatibility** - allowing Pandoc-targeting filters to run on our infrastructure - rather than implementing the full behavior of every option.

---

## Design Principles

### 1. API Compatibility Over Behavioral Compatibility

Filters should be able to call `pandoc.read()`, `pandoc.write()`, and construct `ReaderOptions`/`WriterOptions` without errors. However, most options will be **accepted but ignored** initially.

### 2. Lua Tables, Not Userdata

Unlike Pandoc (which uses userdata), we represent options as **plain Lua tables** with metatables for type identification. This is simpler and sufficient for compatibility.

### 3. Flexible Rust Representation

On the Rust side, options are represented as `serde_json::Value` rather than strongly-typed structs. This allows Rust code to inspect what was requested without committing to a schema.

### 4. Extensions: Parse and Store, Don't Implement

Extensions (e.g., `+smart`, `-citations`) are parsed from format strings and stored in the options, but have no effect on parsing or rendering initially.

---

## Supported Formats

### Readers

| Format String | Maps To | Notes |
|---------------|---------|-------|
| `qmd` | QMD reader | Native format |
| `markdown` | QMD reader | Alias for compatibility |
| `json` | JSON reader | Pandoc AST JSON |

Any other format string should produce a clear error: "Unsupported reader format: {format}".

### Writers

| Format String | Writer | Notes |
|---------------|--------|-------|
| `html`, `html5` | HTML writer | |
| `json` | JSON writer | Pandoc AST JSON |
| `native` | Native writer | Haskell-like AST dump |
| `markdown`, `qmd` | QMD writer | |
| `plain` | Plaintext writer | |

---

## Extensions Handling

### Where Extensions Come From

Extensions can be specified in two places:

1. **Format string**: `"markdown+smart-citations"`
2. **Options table**: `{ extensions = {...} }`

Per Pandoc's documented behavior:
> Note: The extensions field in `reader_options` is ignored, as the function will always use the format extensions specified via the `format` parameter.

### Parsing Format Strings

The format string `"markdown+smart-citations"` is parsed into:
- Base format: `"markdown"`
- Enable: `["smart"]`
- Disable: `["citations"]`

```
format_spec := base_format extension_mod*
extension_mod := ('+' | '-') extension_name
```

### Storage in Options

After parsing, extensions are stored in the options JSON:

```json
{
  "format": "qmd",
  "extensions": {
    "enable": ["smart"],
    "disable": ["citations"]
  },
  "columns": 80
}
```

### Behavior

Initially: **None**. Extensions are parsed and stored for transparency and future use, but do not affect reader/writer behavior. This should be documented clearly.

---

## Lua API Design

### ReaderOptions / WriterOptions

Options are plain Lua tables with a metatable for type identification:

```lua
-- Metatable for ReaderOptions
local ReaderOptionsMT = {
    __name = "ReaderOptions",  -- Used by pandoc.utils.type()
}

-- Constructor
function pandoc.ReaderOptions(t)
    local opts = {
        -- Defaults (matching Pandoc)
        abbreviations = default_abbreviations(),
        columns = 80,
        default_image_extension = "",
        extensions = {},
        indented_code_classes = {},
        standalone = false,
        strip_comments = false,
        tab_stop = 4,
        track_changes = "accept-changes",
    }

    -- Override with provided values
    if t then
        for k, v in pairs(t) do
            opts[k] = v
        end
    end

    return setmetatable(opts, ReaderOptionsMT)
end
```

Similarly for `WriterOptions` with its defaults.

### pandoc.utils.type() Compatibility

For `pandoc.utils.type()` to return `"ReaderOptions"` or `"WriterOptions"`, we check the metatable's `__name` field:

```lua
function pandoc.utils.type(value)
    local mt = getmetatable(value)
    if mt and mt.__name then
        return mt.__name
    end
    return type(value)
end
```

### pandoc.read

```lua
function pandoc.read(content, format_spec, reader_options, read_env)
    -- Default format
    format_spec = format_spec or "markdown"

    -- Parse format specification
    local base_format, ext_enable, ext_disable = parse_format_spec(format_spec)

    -- Map format aliases
    if base_format == "markdown" then
        base_format = "qmd"
    end

    -- Validate format
    if base_format ~= "qmd" and base_format ~= "json" then
        error("Unsupported reader format: " .. base_format)
    end

    -- Build options for Rust (as JSON-serializable table)
    local rust_opts = {
        format = base_format,
        extensions = {
            enable = ext_enable,
            disable = ext_disable,
        },
    }

    -- Merge reader_options (excluding extensions, per Pandoc behavior)
    if reader_options then
        for k, v in pairs(reader_options) do
            if k ~= "extensions" then
                rust_opts[k] = v
            end
        end
    end

    -- Call Rust implementation
    -- read_env handling deferred (sandboxing)
    return _G._quarto_read(content, rust_opts)
end
```

### pandoc.write

```lua
function pandoc.write(doc, format_spec, writer_options)
    -- Default format
    format_spec = format_spec or "html"

    -- Parse format specification
    local base_format, ext_enable, ext_disable = parse_format_spec(format_spec)

    -- Map format aliases
    if base_format == "markdown" then
        base_format = "qmd"
    end

    -- Validate format
    local valid_formats = {html=true, html5=true, json=true, native=true, qmd=true, plain=true}
    if not valid_formats[base_format] then
        error("Unsupported writer format: " .. base_format)
    end

    -- html5 -> html
    if base_format == "html5" then
        base_format = "html"
    end

    -- Build options for Rust
    local rust_opts = {
        format = base_format,
        extensions = {
            enable = ext_enable,
            disable = ext_disable,
        },
    }

    -- Merge writer_options (excluding extensions)
    if writer_options then
        for k, v in pairs(writer_options) do
            if k ~= "extensions" then
                rust_opts[k] = v
            end
        end
    end

    return _G._quarto_write(doc, rust_opts)
end
```

### Format String Parser (Lua)

```lua
-- Parse "markdown+smart-citations" into ("markdown", {"smart"}, {"citations"})
local function parse_format_spec(spec)
    if type(spec) == "table" then
        -- Table form: { format = "markdown", extensions = {...} }
        local base = spec.format or "markdown"
        local enable = {}
        local disable = {}

        if spec.extensions then
            if type(spec.extensions) == "table" then
                -- Could be a list or a key-value table
                for k, v in pairs(spec.extensions) do
                    if type(k) == "number" then
                        -- List form: {"smart", "citations"}
                        table.insert(enable, v)
                    elseif v == true or v == "enable" then
                        table.insert(enable, k)
                    elseif v == false or v == "disable" then
                        table.insert(disable, k)
                    end
                end
            end
        end

        return base, enable, disable
    end

    -- String form: "markdown+smart-citations"
    local enable = {}
    local disable = {}

    -- Find first + or -
    local base_end = spec:find("[%+%-]") or (#spec + 1)
    local base_format = spec:sub(1, base_end - 1)

    -- Parse extension modifiers
    local remaining = spec:sub(base_end)
    for modifier, ext_name in remaining:gmatch("([%+%-])([^%+%-]+)") do
        if modifier == "+" then
            table.insert(enable, ext_name)
        else
            table.insert(disable, ext_name)
        end
    end

    return base_format, enable, disable
end
```

---

## Rust API Design

### Options Type

Options are represented as `serde_json::Value`:

```rust
// src/options.rs

use serde_json::Value;

/// Reader options passed from Lua.
///
/// This is a flexible JSON value rather than a strongly-typed struct,
/// allowing us to accept any Pandoc-compatible options without committing
/// to implementing their behavior.
///
/// Expected structure:
/// ```json
/// {
///   "format": "qmd",
///   "extensions": { "enable": ["smart"], "disable": ["citations"] },
///   "columns": 80,
///   "tab_stop": 4,
///   ...
/// }
/// ```
pub type ReaderOptions = Value;

/// Writer options passed from Lua.
///
/// Expected structure:
/// ```json
/// {
///   "format": "html",
///   "extensions": { "enable": [], "disable": [] },
///   "columns": 72,
///   "wrap_text": "auto",
///   ...
/// }
/// ```
pub type WriterOptions = Value;
```

### Helper Functions

```rust
// src/options.rs

/// Extract a string field with a default
pub fn get_str<'a>(opts: &'a Value, key: &str, default: &'a str) -> &'a str {
    opts.get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
}

/// Extract an integer field with a default
pub fn get_i64(opts: &Value, key: &str, default: i64) -> i64 {
    opts.get(key)
        .and_then(Value::as_i64)
        .unwrap_or(default)
}

/// Extract a boolean field with a default
pub fn get_bool(opts: &Value, key: &str, default: bool) -> bool {
    opts.get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

/// Get the format from options
pub fn get_format(opts: &Value) -> &str {
    get_str(opts, "format", "qmd")
}

/// Get enabled extensions
pub fn get_enabled_extensions(opts: &Value) -> Vec<&str> {
    opts.get("extensions")
        .and_then(|e| e.get("enable"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Get disabled extensions
pub fn get_disabled_extensions(opts: &Value) -> Vec<&str> {
    opts.get("extensions")
        .and_then(|e| e.get("disable"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}
```

### Reader/Writer Interface

```rust
// src/readers/mod.rs

use serde_json::Value;
use crate::pandoc::Pandoc;

/// Read content using the specified options
pub fn read(content: &[u8], options: &Value) -> Result<Pandoc, Error> {
    let format = crate::options::get_format(options);

    match format {
        "qmd" | "markdown" => {
            // Log any requested extensions (for debugging/transparency)
            let enabled = crate::options::get_enabled_extensions(options);
            let disabled = crate::options::get_disabled_extensions(options);
            if !enabled.is_empty() || !disabled.is_empty() {
                log::debug!(
                    "Extensions requested but not implemented: +{:?} -{:?}",
                    enabled, disabled
                );
            }

            // Call existing QMD reader
            // Note: options like tab_stop, columns are available but ignored for now
            read_qmd(content, options)
        }
        "json" => read_json(content),
        _ => Err(Error::UnsupportedFormat(format.to_string())),
    }
}

// src/writers/mod.rs

/// Write document using the specified options
pub fn write(doc: &Pandoc, options: &Value) -> Result<String, Error> {
    let format = crate::options::get_format(options);

    match format {
        "html" | "html5" => write_html(doc, options),
        "json" => write_json(doc),
        "native" => write_native(doc),
        "qmd" | "markdown" => write_qmd(doc, options),
        "plain" => write_plaintext(doc),
        _ => Err(Error::UnsupportedFormat(format.to_string())),
    }
}
```

### Lua Bindings

```rust
// src/lua/readwrite.rs

use mlua::{Lua, Result, Value, Function};
use serde_json;

/// Register pandoc.read and pandoc.write
pub fn register(lua: &Lua) -> Result<()> {
    let globals = lua.globals();

    // Internal read function called by Lua wrapper
    globals.set("_quarto_read", lua.create_function(quarto_read)?)?;
    globals.set("_quarto_write", lua.create_function(quarto_write)?)?;

    Ok(())
}

fn quarto_read(lua: &Lua, (content, opts_table): (mlua::String, mlua::Table)) -> Result<Value> {
    // Convert Lua table to serde_json::Value
    let opts: serde_json::Value = lua.from_value(Value::Table(opts_table))?;

    // Call Rust reader
    let pandoc = crate::readers::read(content.as_bytes(), &opts)
        .map_err(|e| mlua::Error::runtime(e.to_string()))?;

    // Convert Pandoc to Lua
    Ok(crate::lua::types::push_pandoc(lua, &pandoc)?)
}

fn quarto_write(lua: &Lua, (doc, opts_table): (Value, mlua::Table)) -> Result<mlua::String> {
    // Convert Lua Pandoc to Rust
    let pandoc = crate::lua::types::peek_pandoc(lua, doc)?;

    // Convert Lua table to serde_json::Value
    let opts: serde_json::Value = lua.from_value(Value::Table(opts_table))?;

    // Call Rust writer
    let output = crate::writers::write(&pandoc, &opts)
        .map_err(|e| mlua::Error::runtime(e.to_string()))?;

    lua.create_string(&output)
}
```

---

## Global Variables

When running filters, we set `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS` as plain Lua tables:

```rust
// In filter execution
fn set_global_options(lua: &Lua, reader_opts: &Value, writer_opts: &Value) -> Result<()> {
    // Convert serde_json::Value to Lua table
    let reader_table: mlua::Value = lua.to_value(reader_opts)?;
    let writer_table: mlua::Value = lua.to_value(writer_opts)?;

    // Set metatables for type identification
    if let mlua::Value::Table(t) = &reader_table {
        let mt = lua.create_table()?;
        mt.set("__name", "ReaderOptions")?;
        t.set_metatable(Some(mt));
    }

    if let mlua::Value::Table(t) = &writer_table {
        let mt = lua.create_table()?;
        mt.set("__name", "WriterOptions")?;
        t.set_metatable(Some(mt));
    }

    lua.globals().set("PANDOC_READER_OPTIONS", reader_table)?;
    lua.globals().set("PANDOC_WRITER_OPTIONS", writer_table)?;

    Ok(())
}
```

---

## Implementation Plan

### Step 1: Create options.rs

New file with:
- Type aliases (`ReaderOptions`, `WriterOptions` as `serde_json::Value`)
- Helper functions for extracting fields
- Format string parsing (if we want a Rust implementation)

### Step 2: Add Lua Format Parser and Wrappers

In `src/lua/mod.rs` or new `src/lua/readwrite.rs`:
- Format string parser (Lua implementation)
- `pandoc.ReaderOptions` constructor
- `pandoc.WriterOptions` constructor
- `pandoc.read` wrapper
- `pandoc.write` wrapper
- Internal `_quarto_read` / `_quarto_write` bindings

### Step 3: Update Reader/Writer Signatures

Modify existing functions to accept `&serde_json::Value`:
- `readers::qmd::read()` - accept options (can ignore most)
- `writers::html::write()` - accept options (can ignore most)
- etc.

### Step 4: Wire Up Global Variables

Set `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS` when running filters.

### Step 5: Add pandoc.readers / pandoc.writers Fields

The `pandoc` module should expose:
```lua
pandoc.readers  -- Set of supported input format names
pandoc.writers  -- Set of supported output format names
```

---

## Testing Strategy

### API Compatibility Tests

```lua
-- Test: ReaderOptions construction
local opts = pandoc.ReaderOptions({ tab_stop = 2 })
assert(opts.tab_stop == 2)
assert(opts.columns == 80)  -- default
assert(pandoc.utils.type(opts) == "ReaderOptions")

-- Test: WriterOptions construction
local wopts = pandoc.WriterOptions({ columns = 100 })
assert(wopts.columns == 100)
assert(pandoc.utils.type(wopts) == "WriterOptions")

-- Test: pandoc.read basic
local doc = pandoc.read("# Hello", "markdown")
assert(doc.blocks[1].t == "Header")

-- Test: pandoc.read with extensions (accepted but ignored)
local doc2 = pandoc.read("# Hello", "markdown+smart")
assert(doc2.blocks[1].t == "Header")

-- Test: pandoc.write basic
local html = pandoc.write(doc, "html")
assert(html:match("<h1"))

-- Test: format validation
local ok, err = pcall(function()
    pandoc.read("test", "latex")  -- unsupported
end)
assert(not ok)
assert(err:match("Unsupported reader format"))
```

### Format Parsing Tests

```lua
-- Test: parse_format_spec
local base, enable, disable = parse_format_spec("markdown+smart-citations")
assert(base == "markdown")
assert(#enable == 1 and enable[1] == "smart")
assert(#disable == 1 and disable[1] == "citations")

-- Test: table form
local base2, enable2, disable2 = parse_format_spec({
    format = "markdown",
    extensions = { smart = true, citations = false }
})
assert(base2 == "markdown")
-- (check enable2/disable2)
```

---

## Documentation Notes

When this is implemented, we should document:

1. **Supported formats**: qmd, markdown (alias), json for reading; html, json, native, qmd/markdown, plain for writing

2. **Extensions**: Parsed and stored but not implemented. Filters that rely on extension behavior (e.g., `+smart` for smart quotes) will not see that behavior.

3. **Options**: Accepted for API compatibility but mostly ignored. Document which options (if any) actually affect behavior.

---

## Appendix: Pandoc Field Reference

### ReaderOptions Fields (for reference)

| Field | Type | Default |
|-------|------|---------|
| `abbreviations` | Set<String> | Common abbreviations |
| `columns` | int | 80 |
| `default_image_extension` | string | "" |
| `extensions` | Extensions | (format-dependent) |
| `indented_code_classes` | [string] | [] |
| `standalone` | bool | false |
| `strip_comments` | bool | false |
| `tab_stop` | int | 4 |
| `track_changes` | string | "accept-changes" |

### WriterOptions Fields (subset)

| Field | Type | Default |
|-------|------|---------|
| `columns` | int | 72 |
| `dpi` | int | 96 |
| `extensions` | Extensions | (format-dependent) |
| `highlight_style` | table/nil | nil |
| `html_math_method` | string/table | "plain" |
| `html_q_tags` | bool | false |
| `identifier_prefix` | string | "" |
| `incremental` | bool | false |
| `number_sections` | bool | false |
| `prefer_ascii` | bool | false |
| `reference_links` | bool | false |
| `section_divs` | bool | false |
| `tab_stop` | int | 4 |
| `table_of_contents` | bool | false |
| `template` | Template/nil | nil |
| `toc_depth` | int | 3 |
| `variables` | table | {} |
| `wrap_text` | string | "wrap-auto" |

(Plus many format-specific fields: epub_*, cite_method, etc.)

---

## Open Questions

### Q1: Should format parsing happen in Lua or Rust?

**Recommendation**: Lua. The format parsing is straightforward and keeps the Rust API clean - Rust just receives a JSON blob with format and extensions already extracted.

### Q2: What default options should PANDOC_READER_OPTIONS contain?

When running a filter, what ReaderOptions should be set? Options:
- A) Empty/minimal defaults
- B) Reflect actual CLI arguments passed to quarto-markdown-pandoc
- C) Match what Pandoc would set for the same input

**Recommendation**: Start with (A), can enhance to (B) later.

### Q3: Should we validate option field names?

If a filter passes `{ columsn = 80 }` (typo), should we warn?

**Recommendation**: No validation initially. Accept anything silently for maximum compatibility.

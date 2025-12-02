# Lua Global Variables Report

The Lua subsystem exposes several global variables for compatibility with Pandoc Lua filters. These are set up in two files:

## 1. Standard Globals (`filter.rs:84-107`)

| Global | Type | Value | Description |
|--------|------|-------|-------------|
| `FORMAT` | string | Passed from caller | Target output format (e.g., "html", "latex") |
| `PANDOC_VERSION` | table | `{3, 0, 0}` | Emulated Pandoc version (table with numeric indices 1-3) |
| `PANDOC_API_VERSION` | table | `{1, 23, 1}` | Pandoc-types API version (table with numeric indices 1-3) |
| `PANDOC_SCRIPT_FILE` | string | Filter path | Absolute path to the current filter script |

## 2. The `pandoc` Namespace (`constructors.rs`)

The `pandoc` global is a table containing element constructors. It's registered via `register_pandoc_namespace()` at `constructors.rs:26-42`.

### Inline Constructors

| Constructor | Signature | Notes |
|-------------|-----------|-------|
| `pandoc.Str(text)` | `string → Inline` | |
| `pandoc.Space()` | `() → Inline` | |
| `pandoc.SoftBreak()` | `() → Inline` | |
| `pandoc.LineBreak()` | `() → Inline` | |
| `pandoc.Emph(content)` | `{Inline} → Inline` | |
| `pandoc.Strong(content)` | `{Inline} → Inline` | |
| `pandoc.Underline(content)` | `{Inline} → Inline` | |
| `pandoc.Strikeout(content)` | `{Inline} → Inline` | |
| `pandoc.Superscript(content)` | `{Inline} → Inline` | |
| `pandoc.Subscript(content)` | `{Inline} → Inline` | |
| `pandoc.SmallCaps(content)` | `{Inline} → Inline` | |
| `pandoc.Quoted(type, content)` | `string, {Inline} → Inline` | type: "SingleQuote" or "DoubleQuote" |
| `pandoc.Code(text, attr?)` | `string, Attr? → Inline` | |
| `pandoc.Math(type, text)` | `string, string → Inline` | type: "InlineMath" or "DisplayMath" |
| `pandoc.RawInline(format, text)` | `string, string → Inline` | |
| `pandoc.Link(content, target, title?, attr?)` | `{Inline}, string, string?, Attr? → Inline` | |
| `pandoc.Image(content, src, title?, attr?)` | `{Inline}, string, string?, Attr? → Inline` | |
| `pandoc.Span(content, attr?)` | `{Inline}, Attr? → Inline` | |
| `pandoc.Note(content)` | `{Block} → Inline` | |

### Block Constructors

| Constructor | Signature | Notes |
|-------------|-----------|-------|
| `pandoc.Para(content)` | `{Inline} → Block` | |
| `pandoc.Plain(content)` | `{Inline} → Block` | |
| `pandoc.Header(level, content, attr?)` | `int, {Inline}, Attr? → Block` | |
| `pandoc.CodeBlock(text, attr?)` | `string, Attr? → Block` | |
| `pandoc.RawBlock(format, text)` | `string, string → Block` | |
| `pandoc.BlockQuote(content)` | `{Block} → Block` | |
| `pandoc.BulletList(items)` | `{{Block}} → Block` | |
| `pandoc.OrderedList(items, attrs?)` | `{{Block}}, ListAttr? → Block` | ListAttr currently ignored |
| `pandoc.Div(content, attr?)` | `{Block}, Attr? → Block` | |
| `pandoc.HorizontalRule()` | `() → Block` | |

### Utility Constructors

| Constructor | Signature | Notes |
|-------------|-----------|-------|
| `pandoc.Attr(id?, classes?, attrs?)` | `string?, {string}?, table? → Attr` | Creates attribute object |

## Rust Code Structure

```
src/lua/
├── mod.rs           # Module exports
├── filter.rs        # Main filter execution & standard globals (FORMAT, PANDOC_VERSION, etc.)
├── constructors.rs  # pandoc.* namespace with element constructors
└── types.rs         # LuaInline, LuaBlock, LuaAttr UserData implementations
```

The initialization order in `apply_lua_filter()` is:
1. `register_pandoc_namespace(&lua)` — sets up `pandoc` table
2. Set `FORMAT`, `PANDOC_VERSION`, `PANDOC_API_VERSION`, `PANDOC_SCRIPT_FILE`
3. Load and execute filter script
4. Apply filter to document

## Not Yet Implemented

These Pandoc globals are **not** currently implemented:
- `PANDOC_READER_OPTIONS` — reader options table
- `PANDOC_WRITER_OPTIONS` — writer options table
- `PANDOC_STATE` — reader state (input files, log messages, etc.)
- `pandoc.utils.*` — utility functions
- `pandoc.mediabag.*` — media file handling
- `pandoc.system.*` — system utilities
- `pandoc.List` — list metatable/constructor

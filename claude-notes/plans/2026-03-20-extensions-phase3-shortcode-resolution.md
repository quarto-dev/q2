# Extensions Phase 3: Shortcode Resolution

**Created**: 2026-03-20
**Status**: COMPLETE
**Branch**: `feature/shortcode-extensions`
**Parent Plan**: `claude-notes/plans/2026-03-16-extensions-grand-plan.md`
**Depends on**: Phase 1 (complete), Phase 2 (complete), Lua filter support (complete)

## HANDOFF STATUS (read this first if resuming)

Phases 3.1, 3.2, 3.3 are **complete and committed** (commits `19c926b2` and `d3570f4d`).

Phase 3.4.1-3.4.5 are **complete** (uncommitted). All 6912 workspace tests pass.

Remaining work:
- **3.5**: Integration tests (metadata merge + end-to-end)
- **3.6**: Smoke tests (real extension shortcode rendering)
- **3.7**: Workspace verification (`cargo xtask verify`) and commit

## What Already Exists

### q2 shortcode infrastructure

- **Parser**: Tree-sitter grammar parses `{{< name args >}}` into `Inline::Shortcode` nodes.
  Shortcodes are always inline — there is no `Block::Shortcode` variant.
- **AST type**: `Shortcode` struct in `quarto-pandoc-types/src/shortcode.rs` with `is_escaped`,
  `name`, `positional_args`, `keyword_args`, `source_info`.
- **Transform**: `ShortcodeResolveTransform` in `quarto-core/src/transforms/shortcode_resolve.rs`
  walks the AST and dispatches to `ShortcodeHandler` trait implementations.
- **Built-in handlers**: Only `MetaShortcodeHandler` (`{{< meta key >}}`).
- **Pipeline position**: Runs in `AstTransformsStage`, after callout resolution, before metadata
  normalization. The transform pipeline is built statically by `build_transform_pipeline()`.
- **Result type**: `ShortcodeResult` has `Inlines(Vec<Inline>)`, `Error(ShortcodeError)`, `Preserve`.
  No block-level result variant.
- **Context limitation**: `ShortcodeResolveTransform` receives `RenderContext` which has no access
  to extensions, runtime, or target format. `AstTransformsStage` has `StageContext` (with extensions
  and runtime) but doesn't pass them to transforms.

### Extension shortcode storage (Phase 1)

- **Top-level**: `Contributes.shortcodes: Vec<PathBuf>` — absolute paths, parsed by
  `parse_shortcodes()` in `extension/read.rs`.
- **Per-format**: `Contributes.formats: HashMap<String, ConfigValue>` — format metadata may
  contain a `shortcodes` key with relative paths as plain strings. NOT currently marked as
  `ConfigValueKind::Path` by `mark_path_valued_keys()`.
- **Discovery**: `ctx.extensions: Vec<Extension>` on `StageContext`, populated during context
  creation.

### Lua engine (pampa)

- `apply_lua_filter()` in `pampa/src/lua/filter.rs` creates a fresh `Lua` state per filter
  invocation. Calls `register_pandoc_namespace()` to set up `pandoc.*`, `quarto.*` globals.
- `register_pandoc_namespace(lua, runtime, mediabag)` in `pampa/src/lua/constructors.rs`
  registers inline/block constructors, utils, text, JSON, path namespaces. Requires a
  `SharedMediaBag` argument (created via `create_shared_mediabag()` from
  `pampa/src/lua/mediabag.rs`).
- Lua state setup handles WASM (restricted StdLib) vs native (full).
- No shortcode-specific Lua API exists yet.

## How TS Quarto Does It

### Shortcode handler loading

`initShortcodeHandlers()` in `quarto-pre/shortcodes-handlers.lua` loads all shortcode Lua
scripts into a shared `handlers` table. Each script can register handlers two ways:

1. **Return a table**: `return { hello = function(args, kwargs, meta, raw_args, context) ... end }`
2. **Define in environment**: Functions defined in the script's environment are harvested.

Built-in handlers (`meta`, `env`, `var`, `pagebreak`, `brand`, `contents`) are registered
AFTER user/extension handlers, so built-ins override same-named user handlers.

### Shortcode calling convention

`callShortcodeHandler()` calls: `handler.handle(args, kwargs, meta, raw_args, context)` where:
- `args` — `pandoc.List` of `{value: string}` or `{name: key, value: string}` tables
- `kwargs` — table keyed by name, with metatable defaulting missing keys to empty Inlines
- `meta` — metatable proxy that reads document metadata via `readMetadata()`
- `raw_args` — flat list of raw string values
- `context` — `"block"`, `"inline"`, or `"text"`

### Block vs inline context (two-pass)

`shortcodes_filter()` in `customnodes/shortcodes.lua` uses two passes:

1. **First pass** (block context): Walks `Para` and `Plain` nodes. If the node contains a
   single shortcode, calls handler with `context = "block"`. Result is converted via
   `shortcodeResultAsBlocks()` — handler can return Blocks, Inlines (wrapped in Para),
   or a string (wrapped in Para). The block result replaces the original Para/Plain.

2. **Second pass** (inline context): Walks remaining `Shortcode` nodes. Calls handler with
   `context = "inline"`. Result is converted via `shortcodeResultAsInlines()` — Blocks
   are flattened to inlines, strings become `Str`.

A third context `"text"` is used for shortcodes found inside code blocks and attributes
(parsed by LPEG, resolved to plain strings).

### Extension shortcode collection

Two independent paths (same pattern as filters):

- **Top-level** (`contributes.shortcodes`): `extensionShortcodes()` in `filters.ts` iterates
  all extensions, collects `contributes.shortcodes` paths. These are always active.
- **Per-format** (`contributes.formats.html.shortcodes`): Flow through format metadata
  resolution in `readExtensionFormat()`. Paths resolved to absolute. Active only for
  matching format.

Both sets are passed as the `kShortcodes` parameter to the Lua filter pipeline.

### `quarto.shortcode` Lua API

Two helper functions available to shortcode handlers:
- `quarto.shortcode.read_arg(args, n)` — reads nth argument, handles Inlines-to-string
- `quarto.shortcode.error_output(shortcode, message_or_args, context)` — formatted error
  output as Blocks, Inlines, or text depending on context

## Design Decisions

### Threading extensions and runtime into the transform

`ShortcodeResolveTransform` needs access to extension shortcode paths, `SystemRuntime`
(to read Lua files via VFS), and the target format string. Rather than expanding
`RenderContext`, the transform is constructed with these as **owned data** at setup time.

**Key constraint**: `AstTransform: Send + Sync`, but mlua's `Lua` is `!Send + !Sync`
(the `send` feature is not enabled). Therefore the `LuaShortcodeEngine` (which holds a
`Lua` state) **cannot be stored as a field** on `ShortcodeResolveTransform`. Instead:

- `ShortcodeResolveTransform::new()` stores only `Send + Sync` data:
  `Vec<PathBuf>`, `Vec<Extension>`, `Arc<dyn SystemRuntime>`, `String`.
  The format string stored is `ctx.format.identifier.as_str()` (the base format like
  `"html"`, not the full `"acm-html"` descriptor). This is passed to
  `LuaShortcodeEngine::new()` and set as the Lua `FORMAT` global.
  Note: `Format.identifier` is a `FormatIdentifier` enum; `.as_str()` returns the
  base format string. `Format.target_format` is the full descriptor including
  extension prefix (e.g., `"acm-html"`). Use `identifier.as_str()` for the Lua
  `FORMAT` global, matching `UserFiltersStage` at `user_filters.rs:135`.
- `ShortcodeResolveTransform::transform()` creates a `LuaShortcodeEngine` **on the stack**,
  loads scripts, resolves all shortcodes, and lets the engine drop at the end.
- This gives one Lua state per render, reused across all shortcodes in the document —
  same as described in Performance Notes.

**Pipeline construction timing**: Currently `AstTransformsStage::new()` builds the
transform pipeline at construction time via `build_transform_pipeline()`. But the
extensions, runtime, and format are only available at run time via `StageContext`.
Therefore:

- `AstTransformsStage` stores `Option<TransformPipeline>` instead of `TransformPipeline`.
- `new()` stores `None`. `run()` calls `build_transform_pipeline(...)` with `StageContext`
  data to build the pipeline just-in-time.
- `with_pipeline(p)` stores `Some(p)` — used as-is in `run()`, preserving the existing
  test pattern (`AstTransformsStage::with_pipeline(TransformPipeline::new())`).

`build_transform_pipeline()` gains parameters for the shortcode transform. Other transforms
in the pipeline are unaffected — they continue to take no construction parameters.

**Verified**: `SystemRuntime: Send + Sync` (trait bound in `quarto_system_runtime`).
`Extension` derives `Clone` with all `Send + Sync` fields. `Arc<dyn SystemRuntime>` is
`Send + Sync`. pampa re-exports `quarto_system_runtime::SystemRuntime` — they are the
same trait.

### Per-format shortcode paths through metadata merge

Per-format shortcodes (`contributes.formats.html.shortcodes`) go through metadata merge
(Option A from discussion). Add `"shortcodes"` to `mark_path_valued_keys()` — simpler
than filters since shortcode entries are always plain string paths (no map form, no
reserved names like `citeproc`/`quarto`).

After merge, `meta["shortcodes"]` contains rebased paths from both extensions and user
frontmatter. The transform collects these paths and loads them into the Lua state.

Top-level shortcodes (`contributes.shortcodes`) are collected by name-based resolution
when the transform encounters an unknown shortcode name — same pattern as Phase 2's
filter name resolution.

### Block-level shortcode support

Add `ShortcodeResult::Blocks(Vec<Block>)` variant. The resolution logic uses two passes
matching TS Quarto:

1. In `resolve_blocks()`, detect `Para`/`Plain` containing exactly one `Inline::Shortcode`.
   Call handler with block context. If result is `Blocks`, splice them in place of the
   `Para`/`Plain`. If `Inlines`, replace the shortcode inline as usual.

2. Remaining inline shortcodes resolved as before.

This means Lua handlers (e.g., a `pagebreak` extension) can return `pandoc.RawBlock()`
and it will replace the paragraph. Rust built-in handlers can also return
`ShortcodeResult::Blocks` if needed in the future.

### Lua shortcode engine in pampa

New module `pampa/src/lua/shortcode.rs` with a `LuaShortcodeEngine` struct.

**Important**: `LuaShortcodeEngine` is `!Send + !Sync` because it holds a `Lua` state.
It is **never stored as a struct field** on any `Send + Sync` type. It is only created
as a local variable inside `ShortcodeResolveTransform::transform()`, used for the
duration of that call, and dropped afterward.

```rust
pub struct LuaShortcodeEngine {
    lua: Lua,                            // !Send + !Sync
    handler_names: Vec<String>,          // registered handler names for diagnostics
    runtime: Arc<dyn SystemRuntime>,     // for on-demand script loading
}

impl LuaShortcodeEngine {
    /// Create engine, set up Lua state with pandoc/quarto globals.
    /// Internally creates a SharedMediaBag (via create_shared_mediabag()) and calls
    /// register_pandoc_namespace(lua, runtime, mediabag) — same as apply_lua_filter().
    pub fn new(target_format: &str, runtime: Arc<dyn SystemRuntime>) -> Result<Self>;

    /// Load a shortcode Lua script. Registers all handlers it defines.
    /// Supports both return-table and environment-function conventions.
    pub fn load_script(&mut self, script_path: &Path) -> Result<()>;

    /// Call a named shortcode handler.
    /// Returns None if no handler is registered for the name.
    pub fn call(
        &self,
        name: &str,
        shortcode: &Shortcode,
        metadata: &ConfigValue,
        context: ShortcodeCallContext,
    ) -> Option<LuaShortcodeResult>;

    /// Check if a handler is registered for the given name.
    pub fn has_handler(&self, name: &str) -> bool;
}

/// Context in which a shortcode is being resolved (block, inline, or text).
/// Named `ShortcodeCallContext` to avoid conflict with the existing
/// `ShortcodeContext` struct in `shortcode_resolve.rs` (which holds
/// metadata + source_info for resolution).
pub enum ShortcodeCallContext { Block, Inline, Text }

pub enum LuaShortcodeResult {
    Inlines(Vec<Inline>),
    Blocks(Vec<Block>),
    Text(String),
    Error(String),
}
```

This is distinct from `apply_lua_filter` — no AST traversal, just function dispatch.
A single `LuaShortcodeEngine` is created per render (inside `transform()`) and reused
for all shortcode invocations in the document. Cross-render caching (reusing the engine
between keystrokes) is a future optimization that would benefit both filters and shortcodes.

### Handler name collision priority

1. **Built-in Rust handlers** (e.g., `MetaShortcodeHandler`) — always win
2. **Lua handlers loaded later** override earlier ones (within the Lua engine)
3. Extension shortcodes are loaded before user-specified shortcodes, so user wins

The **outcome** matches TS Quarto (built-ins always win, user overrides extensions),
but the **mechanism** differs: TS Quarto registers built-ins last so they overwrite
earlier entries. In q2, the Rust handlers are checked first (before Lua dispatch),
achieving the same effect without registering built-ins in Lua.

### `quarto.shortcode` Lua API

Register `quarto.shortcode.read_arg(args, n)` and `quarto.shortcode.error_output(name, args, context)`
in the Lua state during `LuaShortcodeEngine::new()`. These are compatibility APIs needed
by existing TS Quarto extensions.

### Handler calling convention

Convert q2's `Shortcode` struct to the TS Quarto Lua calling convention:
- `positional_args` → `args` (pandoc.List of `{value = string}`)
- `keyword_args` → `kwargs` (table keyed by name)
- `metadata` → `meta` (metatable proxy reading from `ConfigValue`)
- positional args as strings → `raw_args`
- block/inline/text → `context`

The result conversion does **not** reuse `handle_inline_return`/`handle_block_return`
from `filter.rs`. Those functions have filter-specific semantics that are wrong for
shortcodes: `nil` → clone original (filters mean "no change"; shortcodes mean "handler
failed"), catch-all `_` → clone original (swallows `Value::String` which is the most
common shortcode return type), and they require an `&Inline`/`&Block` "original"
parameter that doesn't exist in shortcode context (shortcodes call freestanding Lua
functions, not element callbacks).

Instead, `LuaShortcodeEngine::call()` has its own top-level return dispatch matching
TS Quarto's `shortcodeResultAsInlines`/`shortcodeResultAsBlocks`:

- `Value::Nil` → `LuaShortcodeResult::Error("Shortcode '...' returned nil")`
- `Value::String(s)` → `LuaShortcodeResult::Text(s)`
- `Value::UserData` → try `LuaInline` first, then `LuaBlock`; produce `Inlines`
  or `Blocks` accordingly
- `Value::Table` → iterate elements, try each as `LuaInline` or `LuaBlock`,
  classify the collection (all inlines → `Inlines`, any blocks → `Blocks`)
- Other → `LuaShortcodeResult::Error(...)`

To avoid duplicating the low-level userdata extraction, new `pub(crate)` helper
functions are created in `filter.rs` wrapping the borrow pattern (the existing code
inlines `ud.borrow::<LuaInline>()?.0.clone()` directly in match arms):
- `extract_lua_inline(ud: &UserData) -> Result<Inline>`
- `extract_lua_block(ud: &UserData) -> Result<Block>`
- `extract_lua_inlines_from_table(table: &Table) -> Result<Vec<Inline>>`
- `extract_lua_blocks_from_table(table: &Table) -> Result<Vec<Block>>`

The shortcode engine composes these primitives with its own nil/string/error handling.

---

## Work Items

### Phase 3.1: Mark per-format shortcode paths as `!path`

Add `"shortcodes"` to `PATH_VALUED_KEYS` in `extension/read.rs`. Simpler than filters —
entries are always plain string paths, no map form, no reserved names. The existing
generic array handling (lines 276-284) already converts each `Scalar(String)` element
to `ConfigValueKind::Path`, so no custom code is needed.

- [x] **3.1.1** Add `"shortcodes"` to the `PATH_VALUED_KEYS` constant (line 219).

- [x] **3.1.2** Tests in `read.rs`:
  - `test_format_shortcode_paths_marked`: `shortcodes: [handler.lua]` → `ConfigValueKind::Path`
  - `test_format_shortcode_multiple_paths_marked`: array with multiple entries, all marked
  - `test_shortcode_marking_doesnt_affect_other_keys`: `toc`, `theme` etc unchanged

### Phase 3.2: `LuaShortcodeEngine` in pampa

New module for loading and dispatching Lua shortcode handlers.

- [x] **3.2.1** Create `pampa/src/lua/shortcode.rs` with `LuaShortcodeEngine` struct,
  `ShortcodeCallContext` enum, `LuaShortcodeResult` enum.

- [x] **3.2.2** Implement `LuaShortcodeEngine::new()`: create Lua state (WASM-aware),
  create a `SharedMediaBag` via `create_shared_mediabag()` (required third argument to
  `register_pandoc_namespace(lua, runtime, mediabag)`), call
  `register_pandoc_namespace()`, set `FORMAT` global, register `quarto.shortcode`
  sub-namespace with `read_arg` and `error_output`.

- [x] **3.2.3** Implement `LuaShortcodeEngine::load_script()`: read script via runtime,
  execute in sandboxed environment. Scan for handlers via both conventions:
  - If script returns a table, iterate keys as handler names
  - Otherwise, scan environment for callable values
  Register all found handlers in an internal `HashMap<String, LuaFunction>`.

- [x] **3.2.4** Implement `LuaShortcodeEngine::call()`: look up handler by name, convert
  `Shortcode` args to Lua tables matching TS Quarto convention, call handler, convert
  result back to Rust types using shortcode-specific dispatch:
  - `Value::Nil` → `LuaShortcodeResult::Error` (handler produced no output)
  - `Value::String` → `LuaShortcodeResult::Text`
  - `Value::UserData` → try `extract_lua_inline`, then `extract_lua_block`
  - `Value::Table` → iterate with `extract_lua_inlines_from_table` /
    `extract_lua_blocks_from_table`, classify collection
  - Other → `LuaShortcodeResult::Error`

- [x] **3.2.4a** Create new `pub(crate)` helper functions in `pampa/src/lua/filter.rs`
  for reuse by the shortcode engine. These are **new functions**, not extractions of
  existing ones — the current code inlines the borrow pattern directly in match arms
  (e.g., `ud.borrow::<LuaInline>()?.0.clone()` at `filter.rs:318-319`). The helpers
  wrap this pattern:
  - `extract_lua_inline(ud: &UserData) -> Result<Inline>` — `ud.borrow::<LuaInline>()?.0.clone()`
  - `extract_lua_block(ud: &UserData) -> Result<Block>` — `ud.borrow::<LuaBlock>()?.0.clone()`
  - `extract_lua_inlines_from_table(table: &Table) -> Result<Vec<Inline>>` — iterate
    table entries, call `extract_lua_inline` on each UserData
  - `extract_lua_blocks_from_table(table: &Table) -> Result<Vec<Block>>` — iterate
    table entries, call `extract_lua_block` on each UserData
  The top-level `handle_inline_return` / `handle_block_return` are NOT reused (their
  nil/fallback semantics are filter-specific and wrong for shortcodes).
  Optionally, refactor `handle_inline_return`/`handle_block_return` to call these
  new helpers, but this is not required — the existing code works fine.

- [x] **3.2.5** Register module in `pampa/src/lua/mod.rs`.

- [x] **3.2.6** Tests:
  - `test_load_script_return_table`: script returns `{hello = function() ... end}` →
    handler registered
  - `test_load_script_env_function`: script defines `function hello() ... end` →
    handler registered
  - `test_call_returns_inlines`: handler returns `pandoc.Inlines{pandoc.Str("hi")}` →
    `LuaShortcodeResult::Inlines`
  - `test_call_returns_blocks`: handler returns `pandoc.RawBlock("html", "<hr>")` →
    `LuaShortcodeResult::Blocks`
  - `test_call_returns_string`: handler returns `"hello"` →
    `LuaShortcodeResult::Text`
  - `test_call_returns_nil`: handler returns `nil` →
    `LuaShortcodeResult::Error`
  - `test_call_unknown_handler`: no handler for name → returns `None`
  - `test_handler_receives_args`: handler that echoes first arg → correct value
  - `test_handler_receives_kwargs`: handler that reads named arg → correct value
  - `test_handler_receives_meta`: handler reads `meta.title` → correct value
  - `test_handler_receives_context`: handler returns context string → matches
  - `test_later_script_overrides_earlier`: two scripts defining same name → last wins
  - `test_read_arg_helper`: Lua code using `quarto.shortcode.read_arg()` works
  - `test_wasm_lua_state`: (cfg wasm32) engine creates successfully with restricted libs

### Phase 3.3: Block-level shortcode support

Add `ShortcodeResult::Blocks` variant and two-pass resolution logic.

- [x] **3.3.1** Add `Blocks(Vec<Block>)` variant to `ShortcodeResult` enum.

- [x] **3.3.2** Add `ResolutionContext` enum to `shortcode_resolve.rs`: `Block`, `Inline`.
  This is distinct from the existing `ShortcodeContext` struct (which holds metadata +
  source_info) and from pampa's `ShortcodeCallContext` (which also includes `Text`).
  Pass `ResolutionContext` to `ShortcodeHandler::resolve()` (signature change).
  Known affected sites (compiler will find any missed):
  - Trait def (line 86), `resolve_shortcode()` (line 237) — add parameter
  - `MetaShortcodeHandler::resolve()` (line 102) — add param, ignore it
  - `resolve_inlines()` (line 442) — pass `ResolutionContext::Inline`
  - 5 test functions (lines 744, 776, 797, 823, 838) — add `ResolutionContext::Inline`
  Run `cargo check -p quarto-core` after the signature change to find all sites.

- [x] **3.3.3** Update `resolve_blocks()`: change from `for block in blocks.iter_mut()`
  to **index-based iteration** (like `resolve_inlines()` already does at line 434),
  because splicing block results requires replacing one element with multiple.

  The current structure delegates per-block work to `resolve_block()`. The new logic
  adds a **block-context shortcode check** at the `resolve_blocks()` level, before
  falling through to `resolve_block()` for the general case:

  ```rust
  fn resolve_blocks(blocks: &mut Vec<Block>, transform: &..., metadata: &..., diagnostics: &mut ...) {
      let mut i = 0;
      while i < blocks.len() {
          // Check for block-context shortcode: Para/Plain with exactly one Shortcode
          if let Some(shortcode) = single_shortcode_in_para_or_plain(&blocks[i]) {
              let ctx = ShortcodeContext { metadata, source_info: &shortcode.source_info };
              match transform.resolve_shortcode(shortcode, &ctx, ResolutionContext::Block) {
                  ShortcodeResult::Blocks(new_blocks) => {
                      let n = new_blocks.len();
                      blocks.splice(i..=i, new_blocks);
                      i += n.max(1);  // advance past spliced blocks
                      continue;
                  }
                  ShortcodeResult::Inlines(inlines) => {
                      // Replace the shortcode inline within the Para/Plain
                      replace_shortcode_in_block(&mut blocks[i], inlines);
                      i += 1;
                      continue;
                  }
                  ShortcodeResult::Error(err) => { /* same as inline error handling */ }
                  ShortcodeResult::Preserve => { /* convert to literal */ }
              }
          }
          // General case: recurse into block (handles mixed-content Para, Div, lists, etc.)
          resolve_block(&mut blocks[i], transform, metadata, diagnostics);
          i += 1;
      }
  }
  ```

  Helper `single_shortcode_in_para_or_plain(block: &Block) -> Option<&Shortcode>`:
  returns `Some` if the block is `Para`/`Plain` with `content.len() == 1` and
  `content[0]` is `Inline::Shortcode` (and not escaped). Returns `None` otherwise.

  Helper `replace_shortcode_in_block(block: &mut Block, inlines: Vec<Inline>)`:
  replaces the single `Inline::Shortcode` in the Para/Plain content with the inlines.

- [x] **3.3.4** Handle `ShortcodeResult::Blocks` in `resolve_inlines()` (graceful
  degradation). When a shortcode in inline context returns `Blocks`, flatten them
  to inlines using the existing `flatten_blocks_to_inlines()` helper (line 200).
  Add a new match arm in `resolve_inlines()` after the `Inlines` arm:
  ```rust
  ShortcodeResult::Blocks(blocks) => {
      let replacement = flatten_blocks_to_inlines(&blocks);
      let replacement_len = replacement.len();
      inlines.splice(i..=i, replacement);
      i += replacement_len.max(1);
  }
  ```

- [x] **3.3.5** Update `MetaShortcodeHandler` to accept context parameter (always returns
  Inlines regardless of context — no behavior change).

- [x] **3.3.6** Tests:
  - `test_block_shortcode_replaces_para`: Para with single shortcode returning Blocks →
    Para replaced by those Blocks
  - `test_inline_shortcode_in_para_stays_inline`: Para with text + shortcode → resolved
    as inline (not block context)
  - `test_block_result_in_inline_context`: shortcode in inline context returns Blocks →
    flattened to Inlines via `flatten_blocks_to_inlines` (graceful degradation)
  - `test_escaped_shortcode_block_context`: escaped shortcode alone in Para → preserved
    as literal text

### Phase 3.4: Wire extensions and Lua into `ShortcodeResolveTransform`

Connect the Lua engine to the transform, collecting shortcode scripts from both metadata
and extension lookup.

- [x] **3.4.1** Add parameterized constructor to `ShortcodeResolveTransform`:
  ```rust
  pub fn with_lua_support(
      lua_shortcode_paths: Vec<PathBuf>,    // from merged metadata
      extensions: Vec<Extension>,            // owned, for name-based lookup
      runtime: Arc<dyn SystemRuntime>,
      target_format: String,
  ) -> Self
  ```
  Constructor stores all parameters as owned fields (`Send + Sync`). Does **not** create
  `LuaShortcodeEngine` — that happens in `transform()`. Keep existing `new()` (no args)
  for backward compatibility in tests that only need built-in handlers.

- [x] **3.4.2** Update `transform()` to create `LuaShortcodeEngine` on the stack:
  ```rust
  fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
      // Create Lua engine if we have paths or extensions
      let mut engine = if !self.lua_shortcode_paths.is_empty()
          || !self.extensions.is_empty() {
          let mut e = LuaShortcodeEngine::new(&self.target_format, self.runtime.clone())?;
          for path in &self.lua_shortcode_paths {
              e.load_script(path)?;
          }
          Some(e)
      } else {
          None
      };
      // Pass engine.as_mut() to resolution functions alongside &self.handlers
      // ...
      // engine dropped here
  }
  ```
  Resolution functions (`resolve_blocks`, `resolve_inlines`, etc.) gain an
  `Option<&mut LuaShortcodeEngine>` parameter alongside the existing
  `&ShortcodeResolveTransform` (or just `&[Box<dyn ShortcodeHandler>]`).

- [x] **3.4.3** Update `build_transform_pipeline()` to accept parameters needed by the
  shortcode transform. Other transforms continue to take no parameters.
  The `target_format` is `ctx.format.identifier.as_str()` (base format like `"html"`).
  ```rust
  pub fn build_transform_pipeline(
      shortcode_paths: Vec<PathBuf>,    // from extract_shortcode_paths()
      extensions: Vec<Extension>,        // from ctx.extensions.clone()
      runtime: Arc<dyn SystemRuntime>,   // from ctx.runtime.clone()
      target_format: String,             // from ctx.format.identifier.as_str().to_string()
  ) -> TransformPipeline
  ```
  **Only one call site** exists: `AstTransformsStage::new()` at `ast_transforms.rs:64`.
  Pipeline builders (`build_html_pipeline_stages`, etc.) call `AstTransformsStage::new()`,
  not `build_transform_pipeline()` directly. Since `new()` is changing to store `None`
  (pipeline built lazily in `run()`), the no-arg `build_transform_pipeline()` becomes
  unused by `new()`. Change its signature to accept the shortcode params directly.
  No other call sites need updating.

- [x] **3.4.4** Update `AstTransformsStage` to build the pipeline in `run()`:
  - Change field from `pipeline: TransformPipeline` to
    `custom_pipeline: Option<TransformPipeline>`.
  - `new()` stores `None`.
  - `with_pipeline(p)` stores `Some(p)` — preserves existing test pattern.
  - In `run()`: if `custom_pipeline` is `Some`, use it. Otherwise, extract data from
    `StageContext` and call `build_transform_pipeline(...)`:
    - `ctx.extensions.clone()` — `Vec<Extension>`, all discovered extensions
    - `ctx.runtime.clone()` — `Arc<dyn SystemRuntime>`
    - `ctx.format.identifier.as_str().to_string()` — base format (e.g., `"html"`)
    - Shortcode paths from merged metadata: extract `doc.ast.meta["shortcodes"]` as
      `Vec<PathBuf>`. After metadata merge + `adjust_paths_to_document_dir()`, the
      array contains `ConfigValueKind::Path(s)` entries where `s` is a path relative
      to the document directory. Convert via:
      ```rust
      fn extract_shortcode_paths(meta: &ConfigValue, document_dir: &Path) -> Vec<PathBuf> {
          let Some(sc_val) = meta.get("shortcodes") else { return vec![] };
          let Some(items) = sc_val.as_array() else { return vec![] };
          items.iter().filter_map(|item| {
              match &item.value {
                  ConfigValueKind::Path(s) => Some(document_dir.join(s)),
                  ConfigValueKind::Scalar(_) => item.as_str().map(|s| document_dir.join(s)),
                  _ => None,
              }
          }).collect()
      }
      ```
      The `document_dir` is `ctx.document.input.parent()` (same pattern as
      `UserFiltersStage` at `user_filters.rs:101-105`). The `Scalar` fallback
      handles user-frontmatter shortcodes that weren't marked as Path (user
      paths aren't processed by `mark_path_valued_keys`, which only runs on
      extension format metadata).

- [x] **3.4.5** Update resolution logic: when a shortcode name doesn't match a built-in
  Rust handler AND isn't in the Lua engine's loaded handlers, try name-based extension
  lookup via `find_extension(name, &self.extensions)` (import from
  `crate::extension::discover::find_extension`, same as `filter_resolve.rs:15` and
  `metadata_merge.rs:31`). If the extension contributes shortcodes
  (`ext.contributes.shortcodes`), load them into the engine on demand
  (`engine.load_script()`) and retry dispatch. The engine is `&mut` so on-demand
  loading works.

- [x] **3.4.6** Tests:
  - `test_lua_shortcode_from_metadata_paths`: Lua script path in metadata → handler works
  - `test_lua_shortcode_by_extension_name`: `{{< my-ext >}}` with matching extension →
    extension's shortcode scripts loaded and handler called
  - `test_rust_handler_overrides_lua`: both Rust `meta` and Lua `meta` handler →
    Rust handler wins
  - `test_unknown_shortcode_error`: no handler anywhere → diagnostic error
  - `test_extension_shortcode_block_context`: extension shortcode returning Blocks → works

### Phase 3.5: Integration tests

- [x] **3.5.1** Test in `metadata_merge.rs`:
  `test_extension_format_shortcode_paths_rebased_through_merge`: extension contributes
  `formats.html.shortcodes: [handler.lua]` → after merge, path resolves to extension dir.

- [x] **3.5.2** Test in `shortcode_resolve.rs`:
  `test_full_transform_with_lua_shortcode`: end-to-end test with Lua script file, extension
  discovery, shortcode in AST → resolved content appears.

- [x] **3.5.3** Test in `shortcode_resolve.rs`:
  `test_full_transform_block_shortcode`: Lua handler returns `pandoc.RawBlock(...)`,
  shortcode alone in Para → Para replaced with RawBlock.

### Phase 3.6: Smoke tests

- [x] **3.6.1** Create `crates/quarto/tests/smoke-all/extensions/shortcode-extension/`:
  Extension with `contributes.shortcodes: [hello.lua]`. Document uses `{{< hello >}}`.
  `hello.lua` returns `pandoc.Inlines{pandoc.Str("HELLO-SHORTCODE-ACTIVE")}`.
  Assert: `ensureFileRegexMatches: [["HELLO-SHORTCODE-ACTIVE"]]`.

- [x] **3.6.2** Create `crates/quarto/tests/smoke-all/extensions/format-with-shortcodes/`:
  Format extension with `contributes.formats.html.shortcodes: [greeting.lua]`.
  Document uses test key `myext-html` and `{{< greeting >}}`.
  Assert: shortcode output appears in HTML.

- [x] **3.6.3** Create smoke test for block-level shortcode:
  Extension shortcode that returns `pandoc.RawBlock("html", "<hr class=\"ext-break\">")`.
  Document has `{{< break >}}` alone on a line.
  Assert: `ensureHtmlElements: [["hr.ext-break"]]`.

- ~~**3.6.4**~~ Removed — lipsum is a future built-in shortcode (see Future Work section),
  not part of the infrastructure delivered by Phase 3.

### Phase 3.7: Workspace verification

- [x] **3.7.1** `cargo build --workspace` — clean build
- [x] **3.7.2** `cargo nextest run --workspace` — all 6919 tests pass
- [x] **3.7.3** `cargo xtask verify` — lint, format, build with `-D warnings` all pass
  (tree-sitter CLI not installed on this machine — pre-existing)
- [x] **3.7.4** Update grand plan to mark Phase 3 complete

---

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/pampa/src/lua/shortcode.rs` | Create | (3.2) `LuaShortcodeEngine`, `ShortcodeCallContext`, `LuaShortcodeResult` |
| `crates/pampa/src/lua/filter.rs` | Modify | (3.2.4a) Extract userdata helpers as `pub(crate)` |
| `crates/pampa/src/lua/mod.rs` | Modify | (3.2) Register shortcode module |
| `crates/quarto-core/src/extension/read.rs` | Modify | (3.1) Add `"shortcodes"` to `mark_path_valued_keys()` |
| `crates/quarto-core/src/transforms/shortcode_resolve.rs` | Modify | (3.3-3.4) Block support, Lua dispatch, extension lookup. Also remove unused `use quarto_analysis::AnalysisContext` import (line 46). |
| `crates/quarto-core/src/pipeline.rs` | Modify | (3.4) `build_transform_pipeline()` accepts shortcode params |
| `crates/quarto-core/src/stage/stages/ast_transforms.rs` | Modify | (3.4) Pass StageContext data to transform pipeline |
| `crates/quarto/tests/smoke-all/extensions/shortcode-extension/` | Create | (3.6) Smoke test |
| `crates/quarto/tests/smoke-all/extensions/format-with-shortcodes/` | Create | (3.6) Smoke test |

---

## Performance Notes

The `LuaShortcodeEngine` creates one Lua state per render (inside `transform()`) and
reuses it for all shortcode invocations in the document. This is important because a
document may contain dozens of shortcodes — creating a Lua state per invocation would
be expensive.

The transform pipeline is also rebuilt per render (in `AstTransformsStage::run()`).
This is negligible — it allocates a `Vec` of ~11 small structs. The shortcode transform's
`with_lua_support()` constructor stores only owned data; no Lua initialization happens
until `transform()` is called.

The Lua state is NOT cached across renders. In the hub-client, the entire pipeline
(including `AstTransformsStage`) is rebuilt per keystroke. Cross-render caching of the
Lua state is a future optimization opportunity that would benefit both shortcodes and
filters — it's not specific to this phase. The key insight is that extension Lua files
don't change between keystrokes; only document content does.

Lua filters have the same per-render cost today (`apply_lua_filter` creates a fresh
`Lua::new()` per filter per render). A caching layer would be pipeline-level
infrastructure, tracked separately.

---

## Risks and Open Questions

All risks have been resolved through research. See "Resolved Design Questions" below.

## Resolved Design Questions

These were identified during plan review and are now resolved:

- **`Lua` is `!Send + !Sync`**: Resolved by creating the `LuaShortcodeEngine` inside
  `transform()` as a stack-local variable, never storing it in the transform struct.
  Same pattern as `apply_lua_filter()` which creates a fresh `Lua` per call.

- **Pipeline construction timing**: Resolved by moving pipeline construction from
  `AstTransformsStage::new()` to `run()`, using `Option<TransformPipeline>` to preserve
  the `with_pipeline()` test pattern.

- **`SystemRuntime` trait identity**: Confirmed that pampa re-exports
  `quarto_system_runtime::SystemRuntime` — they are the same trait. `Arc<dyn SystemRuntime>`
  is `Send + Sync` because the trait has `Send + Sync` supertraits.

- **`ShortcodeContext` naming conflict**: The existing `ShortcodeContext` struct (metadata +
  source_info) is kept. The new block/inline enum in `shortcode_resolve.rs` is named
  `ResolutionContext`. The pampa-side enum is named `ShortcodeCallContext`.

- **Ownership for extensions**: `ShortcodeResolveTransform` stores `Vec<Extension>` (owned,
  cloned from `StageContext`). `Extension` derives `Clone` with all `Send + Sync` fields.

- **Userdata unwrapping for shortcode results**: The filter engine's
  `handle_inline_return` / `handle_block_return` (`filter.rs:313,343`) are **not**
  reused — their semantics (nil → keep original, unknown → keep original, requires
  `&Inline`/`&Block` original param) are filter-specific and wrong for shortcodes.
  Instead, **new** `pub(crate)` helper functions are created in `filter.rs` wrapping
  the borrow pattern (the existing code inlines `ud.borrow::<LuaInline>()?.0.clone()`
  directly in match arms — there are no existing standalone functions to extract).
  The shortcode engine builds its own top-level dispatch using these helpers:
  nil → error, string → text, userdata/table → use helpers, other → error.
  This matches TS Quarto's `shortcodeResultAsInlines`/`shortcodeResultAsBlocks`.

- **`quarto.shortcode.error_output`**: Implementable as pure Lua registered during
  `LuaShortcodeEngine::new()`. Calls existing pandoc constructors (`pandoc.Para`,
  `pandoc.Strong`, `pandoc.Str`) from `register_pandoc_namespace()`. No Rust needed.

- **`build_transform_pipeline` signature change**: Only one call site exists
  (`AstTransformsStage::new()` → moves to `run()`). `with_pipeline()` bypasses it
  entirely. Tests using `ShortcodeResolveTransform::new()` (no args) are unaffected.

- **`Format` field for target format string**: `Format` has three relevant fields:
  `identifier: FormatIdentifier` (enum, use `.as_str()` for `"html"`),
  `target_format: String` (full string like `"acm-html"`),
  `extension_name: Option<String>` (e.g., `Some("acm")`).
  Use `identifier.as_str()` for the Lua `FORMAT` global — this gives the base format
  like `"html"`, matching `UserFiltersStage` at `user_filters.rs:135`.

- **Extracting shortcode paths from merged metadata**: After metadata merge,
  `doc.ast.meta["shortcodes"]` contains an array of `ConfigValueKind::Path` entries
  (rebased relative to document dir by `adjust_paths_to_document_dir()`). Extract
  with `document_dir.join(s)` for each `Path(s)` entry. Also handle `Scalar` fallback
  for user-frontmatter shortcodes not processed by `mark_path_valued_keys`.

- **`ShortcodeResult::Blocks` in inline context**: When `resolve_inlines()` encounters
  a `Blocks` result, it calls the existing `flatten_blocks_to_inlines()` (line 200)
  for graceful degradation. This matches TS Quarto's `shortcodeResultAsInlines()`.

- **`ShortcodeHandler::resolve()` signature change**: Mechanical update. Affected sites
  in `shortcode_resolve.rs` (all add `ResolutionContext::Inline`):
  - Trait def (line 86), `resolve_shortcode()` (line 237) — add parameter
  - `MetaShortcodeHandler::resolve()` (line 102) — add param, ignore it
  - `resolve_inlines()` (line 442) — pass `ResolutionContext::Inline`
  - 5 test functions (lines 744, 776, 797, 823, 838) — add `ResolutionContext::Inline`

---

## Future Work: Built-in Shortcodes

Phase 3 delivers the shortcode resolution **infrastructure** (Lua engine, block/inline
dispatch, extension loading). The following built-in shortcodes from TS Quarto are not
yet implemented in q2 and will need separate work items. This list is exhaustive as of
Quarto 1.x.

### Already implemented in q2 (Rust)

| Shortcode | Status | Notes |
|-----------|--------|-------|
| `meta` | Done | `MetaShortcodeHandler` in `shortcode_resolve.rs` |

### Core Lua handlers (from `shortcodes-handlers.lua`)

These are registered as built-in handlers in TS Quarto. In q2, they could be
implemented as either Rust `ShortcodeHandler` impls or bundled Lua scripts.

| Shortcode | Args | Returns | Notes |
|-----------|------|---------|-------|
| `var` | key (dot notation) | Inlines | Reads from `_variables.yml` file. Requires variables file support. |
| `env` | name, optional default | Inlines (`pandoc.Str`) | Reads `os.getenv()`. Straightforward Lua or Rust impl. |
| `pagebreak` | none | RawBlock (format-specific) | Returns `\newpage{}` (LaTeX), `<div style="page-break-after:always">` (HTML), OpenXML for DOCX, etc. Context-insensitive (always block). |
| `brand` | subcommand (color/logo), name, optional mode | Inlines or Blocks | Reads `_brand.yml`. `brand color primary` → color string. `brand logo main` → Image element(s) with light/dark classes. Requires brand config support. |
| `contents` | optional ID | RawInline (JSON) | Generates TOC marker. Internal use for callout TOC integration. |

### Built-in extension shortcodes (from `resources/extensions/quarto/`)

These ship as built-in Quarto extensions. In q2, they would be bundled Lua scripts
loaded via the extension mechanism.

| Shortcode | Args | Returns | Notes |
|-----------|------|---------|-------|
| `lipsum` | range (e.g., `1-3`), count, or none (default `1-5`); `random=true` | Blocks (list of Para) | Lorem ipsum placeholder text. Good first candidate — no dependencies, exercises block-level return. |
| `kbd` | default shortcut; `mac=`, `win=`, `linux=` named args; `mode=plain` | RawInline (HTML `<kbd>`) or Inlines | Keyboard shortcut display with OS-specific variants. |
| `video` | src URL; `width=`, `height=`, `title=`, `start=`, `aspect-ratio=` | RawBlock (HTML) or Link | Embeds YouTube, Vimeo, Brightcove, or local video. Format-aware. |
| `placeholder` | width, height; `format=svg\|png` | Image (data URI) | Generates colored placeholder images. Returns differently for text vs visual contexts. |
| `version` | none | string | Returns Quarto version. Trivial impl. |

### Pre-engine shortcodes (TypeScript in TS Quarto)

These are handled before engine execution in TS Quarto and operate on raw markdown
text, not the Pandoc AST. They may need a different mechanism in q2.

| Shortcode | Args | Returns | Notes |
|-----------|------|---------|-------|
| `include` | filename | Raw markdown content | Inserts content from another file. Handled pre-parse in TS Quarto. May need a pre-parse stage or tree-sitter integration in q2. |
| `embed` | notebook filename; `echo=`, `outputs=` | Raw markdown content | Embeds Jupyter notebook cells. Post-engine in TS Quarto. Requires notebook support. |

### Recommended implementation order

1. **`env`** — trivial, no dependencies, good smoke test for the Lua engine
2. **`lipsum`** — no dependencies, exercises block-level returns, useful for testing
3. **`pagebreak`** — format-specific RawBlock, exercises format dispatch
4. **`version`** — trivial string return
5. **`kbd`** — exercises named args, OS detection, HTML generation
6. **`var`** — requires `_variables.yml` support (separate feature)
7. **`video`** — complex HTML generation, URL parsing
8. **`placeholder`** — image generation, data URIs
9. **`brand`** — requires `_brand.yml` support (separate feature)
10. **`contents`** — internal, depends on callout/TOC infrastructure
11. **`include`** / **`embed`** — pre-engine, needs separate architecture

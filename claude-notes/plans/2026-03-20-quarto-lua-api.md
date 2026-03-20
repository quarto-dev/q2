# Quarto Lua API: `quarto.*` Namespace Implementation

**Created**: 2026-03-20
**Status**: IN PROGRESS
**Branch**: `feature/shortcode-extensions`
**Triggered by**: lipsum extension fails because `quarto.utils` is nil

## READ THIS FIRST (Zero-Knowledge Bootstrap)

This plan is for the **pampa** crate (`crates/pampa/`), which is the core Quarto
Markdown engine. It has a Lua subsystem in `crates/pampa/src/lua/` that provides
Pandoc-compatible Lua APIs to extension authors.

**What is this about?** Quarto extensions (written in Lua) expect a `quarto.*` global
namespace with utility functions. We have the `pandoc.*` namespace fully implemented,
but the `quarto.*` namespace is nearly empty. This causes real extensions to crash.

**Reproducing the bug:**
```bash
cd ~/docs/lipsum
cargo run --manifest-path /Users/gordon/src/q2/Cargo.toml --bin q2 -- render index.qmd
```
Error: `attempt to index a nil value (field 'utils')` — because `quarto.utils` doesn't exist.

**Key files in `crates/pampa/src/lua/`:**

| File | Purpose |
|------|---------|
| `constructors.rs` | `register_pandoc_namespace()` — sets up `pandoc.*` globals, then calls `register_quarto_namespace()` at the end (line 262) |
| `diagnostics.rs` | `register_quarto_namespace()` — creates the `quarto` global table with ONLY `warn`, `error`, `_diagnostics` |
| `shortcode.rs` | `LuaShortcodeEngine` — loads/dispatches shortcode Lua scripts. `register_shortcode_api()` adds `quarto.shortcode.*` |
| `filter.rs` | `apply_lua_filter()` — runs Lua filters on ASTs. Sets up Lua state, calls `register_pandoc_namespace()` |
| `json.rs` | `register_pandoc_json()` — implements `pandoc.json.decode/encode/null` |
| `utils.rs` | `register_pandoc_utils()` — implements `pandoc.utils.stringify/type/sha1/etc` |
| `path.rs` | `register_pandoc_path()` — implements `pandoc.path.directory/join/normalize/etc` |
| `system.rs` | `register_pandoc_system()` — implements `pandoc.system.get_working_directory/etc` |
| `types.rs` | `LuaInline`, `LuaBlock` wrapper types for passing AST nodes to/from Lua |
| `mod.rs` | Module declarations and public re-exports |

**Lua state initialization sequence** (both shortcode and filter engines):

```
1. Lua::new()                           // Create Lua VM
2. register_pandoc_namespace(lua, ...)   // Sets up pandoc.* (constructors.rs)
   ├── pandoc.Str, pandoc.Para, ...     // AST constructors
   ├── pandoc.utils.*                   // stringify, type, sha1, etc.
   ├── pandoc.json.*                    // decode, encode, null
   ├── pandoc.path.*                    // directory, join, normalize, etc.
   ├── pandoc.system.*                  // get_working_directory, etc.
   └── register_quarto_namespace(lua)   // Creates quarto table (diagnostics.rs)
       ├── quarto.warn(msg, elem?)
       ├── quarto.error(msg, elem?)
       └── quarto._diagnostics          // internal storage
3. lua.globals().set("FORMAT", ...)      // Set target format global
4. [shortcode only] register_shortcode_api(lua)  // Adds quarto.shortcode.*
```

**What's missing** (needed by real extensions):
- `quarto.utils` sub-table (especially `resolve_path`)
- `quarto.json` sub-table (should alias `pandoc.json`)
- `quarto.log` sub-table (logging to stderr)

## Problem Statement

The `quarto` Lua global currently only exposes `warn`, `error`, `_diagnostics`, and
`shortcode.*`. Real-world Quarto extensions (including built-in ones like lipsum, kbd,
video, placeholder) expect a much richer `quarto.*` API surface. The lipsum extension
specifically needs `quarto.utils.resolve_path()`, `quarto.json.decode()`, and
`quarto.log.error()`.

**The test extension** (`~/docs/lipsum/`) contains:
- `index.qmd` — uses `{{< lipsum 3 >}}`
- `_extensions/lipsum/_extension.yml` — declares `contributes.shortcodes: [lipsum.lua]`
- `_extensions/lipsum/lipsum.lua` — the shortcode handler (100 lines)
- `_extensions/lipsum/lipsum.json` — lorem ipsum paragraph data

The lipsum handler does three things that fail:
1. Line 20: `quarto.utils.resolve_path("lipsum.json")` — resolve path relative to script
2. Line 23: `quarto.json.decode(fileContents)` — parse JSON
3. Line 26: `quarto.log.error("Unable to read lipsum data file.")` — log error

It also uses `io.open()` (native Lua file I/O), `math.randomseed(os.time())`, and
`pandoc.utils.stringify()` / `pandoc.Para()` — these all already work.

## TS Quarto Reference

In TypeScript Quarto (`~/src/quarto-cli/`), the `quarto` namespace is defined in:
`src/resources/pandoc/datadir/init.lua` (lines 812-1047)

Key implementation details from TS Quarto:

### `quarto.utils.resolve_path(path)` (line 970)
```lua
-- resolve_path = resolvePathExt  (line 970)
local function resolvePathExt(path)    -- line 322
  if isRelativeRef(path) then
    return resolvePath(pandoc.path.join({scriptDir(), pandoc.path.normalize(path)}))
  else
    return path
  end
end
```
Where `scriptDir()` returns the directory of the currently-executing script file.
`scriptDir()` (line 180) reads from a `scriptFile` stack that tracks which Lua file
is being loaded. `resolvePath` (line 313) joins with the working directory if relative.

### `quarto.json` (line 993)
```lua
local json = require '_json'   -- line 151
-- ...
json = json,                   -- line 993
```
It's a separate JSON library (`_json.lua`), but functionally equivalent to `pandoc.json`.

### `quarto.log` (line 995)
```lua
local logging = require 'logging'  -- line 153
-- ...
log = logging,                     -- line 995
```
Defined in `logging.lua`. Pure Lua module that writes to `io.stderr`. Key functions:
- `output(...)` — write stringified args to stderr
- `error(...)` — prefix `(E)`, only if loglevel >= -1
- `warning(...)` — prefix `(W)`, only if loglevel >= 0
- `info(...)` — prefix `(I)`, only if loglevel >= 1
- `debug(...)` — prefix `(D)`, only if loglevel >= 2
- `trace(...)` — prefix `(T)`, only if loglevel >= 3
- `setloglevel(level)` — set level, return old
- `dump(value, maxlen)` — pretty-print value

Default loglevel is 0 (warnings and errors shown).

Stringify logic: uses `pandoc.utils.type()` to detect types, dumps tables recursively,
calls `tostring()` on primitives.

### Built-in extension API usage

| Extension | `quarto.*` APIs used |
|-----------|---------------------|
| **lipsum** | `utils.resolve_path`, `json.decode`, `log.error` |
| **kbd** | `shortcode.read_arg`, `shortcode.error_output`, `doc.is_format`, `doc.add_html_dependency`, `log.warning` |
| **video** | `doc.add_html_dependency`, `doc.include_text`, `doc.is_format`, `doc.has_bootstrap`, `utils.as_inlines` |
| **placeholder** | `utils.resolve_path`, `base64.encode`, `format.is_typst_output`, `shortcode.error_output` |
| **version** | `quarto.version` |

## What Already Exists Under `pandoc.*` (Can Be Aliased)

| API | Rust source | Notes |
|-----|-------------|-------|
| `pandoc.json.decode(str)` | `json.rs:48` | Full JSON decode impl |
| `pandoc.json.encode(value)` | `json.rs:28` | Full JSON encode impl |
| `pandoc.json.null` | `json.rs:22` | LightUserData sentinel |
| `pandoc.utils.stringify(elem)` | `utils.rs:24` | Full impl for all AST types |
| `pandoc.utils.type(value)` | `utils.rs:267` | Returns Pandoc-aware type name |
| `pandoc.path.directory(path)` | `path.rs` | Returns parent directory |
| `pandoc.path.join(parts)` | `path.rs` | Joins path components |
| `pandoc.path.normalize(path)` | `path.rs` | Normalizes path separators |
| `pandoc.path.is_absolute(path)` | `path.rs` | Check if absolute |
| `pandoc.path.is_relative(path)` | `path.rs` | Check if relative |
| `pandoc.system.get_working_directory()` | `system.rs` | Returns cwd |

## Design

### Architecture: `register_quarto_api()`

Create a new module `pampa/src/lua/quarto_api.rs` that extends the existing `quarto`
table (already created by `diagnostics.rs`) with additional sub-namespaces.

```rust
/// Extends the `quarto` global (already created by register_quarto_namespace)
/// with additional API sub-namespaces: quarto.json, quarto.log, quarto.utils.
///
/// Must be called AFTER register_pandoc_namespace() (which creates both
/// `pandoc` and `quarto` globals).
pub fn register_quarto_api(lua: &Lua) -> Result<()>
```

**No options struct needed.** The `_quarto_script_dir` global is set separately
by the caller (shortcode engine or filter engine) before script evaluation.

**Call sequence after this change:**

```
1. Lua::new()
2. register_pandoc_namespace(lua, ...)    // pandoc.* + base quarto table
3. register_quarto_api(lua)               // NEW: extends quarto with json/log/utils
4. lua.globals().set("FORMAT", ...)
5. [shortcode only] register_shortcode_api(lua)  // quarto.shortcode.*
```

The function retrieves the existing `quarto` table and adds sub-tables:
```rust
let quarto: Table = lua.globals().get("quarto")?;
// Add quarto.json (alias pandoc.json)
// Add quarto.log (new impl)
// Add quarto.utils (new impl)
```

### `quarto.utils.resolve_path(path)` — Key Design Decision

**How it works in TS Quarto:** Before each script is loaded, a `scriptFile` stack is
pushed with the script path. `scriptDir()` returns the directory of the top of the stack.
`resolve_path` joins the relative path with `scriptDir()`.

**Our approach:** Use a Lua global `_quarto_script_dir` (string). The shortcode engine
sets this before each `chunk.eval()` in `load_script()`, and restores it after.

```rust
// In LuaShortcodeEngine::load_script(), BEFORE chunk.eval():
let script_dir = script_path.parent().unwrap_or(Path::new(""));
self.lua.globals().set(
    "_quarto_script_dir",
    script_dir.to_string_lossy().to_string()
)?;
// ... chunk.eval() ...
// Restore is optional since the next load_script will overwrite it
```

The Lua function reads this global:
```rust
// Registered as a Rust closure in register_quarto_api():
lua.create_function(|lua, path: String| {
    // Check if relative
    let p = Path::new(&path);
    if p.is_absolute() {
        return Ok(path);
    }
    // Get script dir from Lua global
    let script_dir: String = lua.globals()
        .get::<String>("_quarto_script_dir")
        .unwrap_or_default();
    if script_dir.is_empty() {
        return Ok(path);  // No script dir set, return as-is
    }
    let resolved = PathBuf::from(&script_dir).join(&path);
    // Normalize (collapse .. and .)
    Ok(normalize_path(&resolved))
})?;
```

**Why a global and not a closure capture?** The `quarto.utils.resolve_path` function is
registered once during `register_quarto_api()`, but `_quarto_script_dir` changes for
each script loaded. A Lua global is the simplest way to communicate this.

**IMPORTANT:** lipsum's `readLipsum()` is called lazily (from the handler, not at
script load time), so the script dir must remain valid after `load_script()` returns.
Since the shortcode engine loads all scripts then calls handlers, the script dir will
be set to the LAST loaded script's directory. This is fine if there's only one script
per extension (the common case). For multiple scripts, the last one wins — matching
TS Quarto behavior since it also uses a stack that pops after load.

Actually, looking more carefully: the lipsum handler calls `readLipsum()` which calls
`resolve_path("lipsum.json")`. This happens during `engine.call()`, not during
`load_script()`. At that point, `_quarto_script_dir` is set to whatever the last
`load_script()` set it to. If we load scripts per-extension, this is the extension's
script directory — correct for lipsum.

But if we load scripts from multiple extensions, the script dir for earlier extensions
would be wrong when their handlers are called later. **Fix:** Store the script directory
per handler name when loading, and set `_quarto_script_dir` before each `call()`.

```rust
// In LuaShortcodeEngine:
struct LuaShortcodeEngine {
    lua: Lua,
    handlers: HashMap<String, mlua::RegistryKey>,
    handler_script_dirs: HashMap<String, String>,  // NEW: name -> script dir
    runtime: Arc<dyn SystemRuntime>,
}

// In load_script(): record script dir for each handler registered
// In call(): set _quarto_script_dir before calling the handler
```

### `quarto.json` — Simple alias

Just alias the already-registered `pandoc.json` table:
```rust
let pandoc: Table = lua.globals().get("pandoc")?;
let pandoc_json: Table = pandoc.get("json")?;
quarto.set("json", pandoc_json)?;
```

This gives `quarto.json.decode`, `quarto.json.encode`, and `quarto.json.null` for free.

### `quarto.log` — Rust-backed stderr logging

Implement as Rust closures that write to stderr via `eprintln!`. This works on both
native and WASM (where `io.stderr` doesn't exist).

The stringify logic for each argument:
- `string` / `number` / `boolean` → `tostring()`
- `table` / `userdata` → use `pandoc.utils.stringify()` (already registered)
- `nil` → `"nil"`

Log level is stored as a Lua number in `quarto.log.loglevel` (default 0).

```rust
fn register_quarto_log(lua: &Lua, quarto: &Table) -> Result<()> {
    let log = lua.create_table()?;
    log.set("loglevel", 0)?;  // Default: warnings + errors

    // quarto.log.output(...) — always writes
    log.set("output", lua.create_function(|lua, args: MultiValue| {
        let text = stringify_log_args(lua, &args)?;
        eprintln!("{}", text);
        Ok(())
    })?)?;

    // quarto.log.error(...) — writes if loglevel >= -1
    // quarto.log.warning(...) — writes if loglevel >= 0
    // etc.

    quarto.set("log", log)?;
    Ok(())
}
```

### Test patterns

Existing shortcode tests use this pattern (from `shortcode.rs` tests):
```rust
use tempfile::TempDir;

fn make_runtime() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn write_script(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn test_example() {
    let tmp = TempDir::new().unwrap();
    let script = write_script(tmp.path(), "hello.lua", r#"
        return { hello = function(args, kwargs, meta, raw_args, context)
            return pandoc.Str("hello-world")
        end }
    "#);

    let runtime = make_runtime();
    let mut engine = LuaShortcodeEngine::new("html", runtime).unwrap();
    engine.load_script(&script).unwrap();

    let result = engine.call("hello", &make_empty_args(), ShortcodeCallContext::Inline).unwrap();
    // ... assert on result ...
}
```

For `quarto_api.rs` unit tests, use the same pattern as `utils.rs` tests:
```rust
fn create_test_lua() -> Lua {
    let lua = Lua::new();
    let runtime = Arc::new(NativeRuntime::new());
    register_pandoc_namespace(&lua, runtime, create_shared_mediabag()).unwrap();
    register_quarto_api(&lua).unwrap();  // NEW
    lua
}

#[test]
fn test_quarto_json_decode() {
    let lua = create_test_lua();
    let result: String = lua.load(r#"return quarto.json.decode('{"a":1}').a"#).eval().unwrap();
    // Note: pandoc.json.decode returns Lua tables, so field access works
}
```

## Work Items

### Phase 1: Core infrastructure (`quarto_api.rs`)

- [x] **1.1** Create `pampa/src/lua/quarto_api.rs` with `pub fn register_quarto_api(lua: &Lua) -> Result<()>`.
  This function retrieves the existing `quarto` global table and extends it.

- [x] **1.2** Implement `quarto.json` as alias of `pandoc.json`.
  Three lines: get `pandoc.json` table, set as `quarto.json`.

- [x] **1.3** Implement `quarto.log` namespace with these functions:
  - `quarto.log.output(...)` — stringify args, write to stderr via `eprintln!`
  - `quarto.log.error(...)` — `(E)` prefix, only if `quarto.log.loglevel >= -1`
  - `quarto.log.warning(...)` — `(W)` prefix, only if `quarto.log.loglevel >= 0`
  - `quarto.log.info(...)` — `(I)` prefix, only if `quarto.log.loglevel >= 1`
  - `quarto.log.debug(...)` — `(D)` prefix, only if `quarto.log.loglevel >= 2`
  - `quarto.log.trace(...)` — `(T)` prefix, only if `quarto.log.loglevel >= 3`
  - `quarto.log.setloglevel(level)` — set `loglevel`, return old value
  - `quarto.log.loglevel` — numeric field, default 0
  - Use `eprintln!` in Rust for output (works on native and WASM)
  - Stringify: for each arg, use Lua `tostring()` for primitives (string, number, boolean,
    nil). For tables, do recursive key=value dumping. For userdata, try `tostring()` which
    will invoke the `__tostring` metamethod if present (our LuaInline/LuaBlock have this).
    Do NOT call `pandoc.utils.stringify()` — it expects specific AST types and would error
    on arbitrary tables. This matches TS Quarto's `logging.lua` approach.

- [x] **1.4** Implement `quarto.utils` sub-namespace:
  - `quarto.utils.resolve_path(path)` — if relative, join with `_quarto_script_dir` global;
    if absolute, return as-is. Uses `std::path::Path` in Rust. Needs a `normalize_path()`
    helper that collapses `.` and `..` without touching the filesystem (since
    `std::path::Path::canonicalize()` requires the path to exist on disk).
  - `quarto.utils.type(value)` — alias `pandoc.utils.type`
  - ~~`quarto.utils.resolve_path_relative_to_document(path)`~~ — DEFERRED to Tier 2.
    Needs the document path plumbed through, which neither shortcode nor filter engine
    currently provides. Not needed by lipsum or other Tier 1 extensions.

- [x] **1.5** Add `mod quarto_api;` to `pampa/src/lua/mod.rs`. Add `pub use`.

- [x] **1.6** Tests in `quarto_api.rs`:
  - `test_quarto_json_decode` — `quarto.json.decode('{"a":1}')` returns table
  - `test_quarto_json_encode` — `quarto.json.encode({a=1})` returns string
  - `test_quarto_log_error_runs` — `quarto.log.error("test")` doesn't panic
  - `test_quarto_log_respects_level` — `quarto.log.info("test")` is silent at level 0
  - `test_quarto_log_setloglevel` — returns old level, changes behavior
  - `test_quarto_utils_resolve_path_absolute` — `/abs/path` returned as-is
  - `test_quarto_utils_resolve_path_relative` — `foo.json` joined with script dir
  - `test_quarto_utils_resolve_path_no_script_dir` — returns relative path as-is
  - `test_quarto_utils_type` — `quarto.utils.type(pandoc.Str("x"))` returns `"Str"`

### Phase 2: Wire into shortcode engine

- [x] **2.1** In `LuaShortcodeEngine::new()` (shortcode.rs), call `register_quarto_api(&lua)`
  after `register_pandoc_namespace()` (line 82) and before `register_shortcode_api()` (line 89).
  Import: `use super::quarto_api::register_quarto_api;`

- [x] **2.2** Add `handler_script_dirs: HashMap<String, String>` field to `LuaShortcodeEngine`.

- [x] **2.3** In `load_script()`: before `chunk.eval()` (line 138), set `_quarto_script_dir`
  to `script_path.parent()`. After registering handlers (both conventions), store the
  script dir in `handler_script_dirs` for each handler name.

- [x] **2.4** In `call()`: before calling the handler function, set `_quarto_script_dir`
  to the value from `handler_script_dirs[name]`.

- [x] **2.5** Tests:
  - `test_shortcode_resolve_path` — write `data.json` next to script, handler calls
    `quarto.utils.resolve_path("data.json")`, verify it returns the correct absolute path
  - `test_shortcode_quarto_json` — handler calls `quarto.json.decode('{"x":1}')`,
    returns the value
  - `test_shortcode_quarto_log` — handler calls `quarto.log.warning("test")`, no crash

### Phase 3: Wire into filter engine

- [x] **3.1** In `apply_lua_filter()` (filter.rs), call `register_quarto_api(&lua)` after
  `register_pandoc_namespace()` (line 136) and before `lua.globals().set("FORMAT", ...)`.

- [x] **3.2** Set `_quarto_script_dir` to `filter_path.parent()` before loading the
  filter script (line 174). This is a one-shot setting since filters run one at a time.

- [x] **3.3** Tests:
  - `test_filter_quarto_json_available` — filter script can call `quarto.json.decode`
  - `test_filter_quarto_log_available` — filter script can call `quarto.log.warning`

### Phase 4: End-to-end validation

- [~] **4.1** Test lipsum extension manually (PARTIAL — quarto.* API works, but lipsum
  fails due to a separate issue: `pandoc.Para(string)` doesn't auto-convert strings to
  inlines. This is a `pandoc.Para` constructor compatibility issue, not a quarto API issue):
  ```bash
  cd ~/docs/lipsum
  cargo run --manifest-path /Users/gordon/src/q2/Cargo.toml --bin q2 -- render index.qmd
  ```
  Should produce HTML with 3 paragraphs of lorem ipsum text.

- [ ] **4.2** (DEFERRED) Create smoke test: `crates/quarto/tests/smoke-all/extensions/shortcode-resolve-path/`
  with a shortcode extension that uses `quarto.utils.resolve_path()` to load a JSON
  data file and `quarto.json.decode()` to parse it.

- [x] **4.3** `cargo nextest run --workspace` — all 6975 tests pass.

- [x] **4.4** `cargo build --workspace` — clean build.

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/pampa/src/lua/quarto_api.rs` | **Create** | `register_quarto_api()` — quarto.json, quarto.log, quarto.utils |
| `crates/pampa/src/lua/mod.rs` | Modify | Add `mod quarto_api;` and `pub use` |
| `crates/pampa/src/lua/shortcode.rs` | Modify | Call `register_quarto_api()`, add `handler_script_dirs`, set `_quarto_script_dir` per handler |
| `crates/pampa/src/lua/filter.rs` | Modify | Call `register_quarto_api()`, set `_quarto_script_dir` |

Files NOT modified: `constructors.rs`, `diagnostics.rs` — existing behavior preserved.

## mlua Patterns You'll Need

The Lua binding crate is `mlua`. Key patterns used throughout the codebase:

```rust
use mlua::{Function, Lua, MultiValue, Result, Table, Value};

// Get existing global table
let quarto: Table = lua.globals().get("quarto")?;

// Create sub-table
let utils = lua.create_table()?;
quarto.set("utils", utils)?;

// Create Rust-backed Lua function
utils.set("resolve_path", lua.create_function(|lua, path: String| {
    // ... Rust logic ...
    Ok(result_string)
})?)?;

// Variadic args (for quarto.log.*)
log.set("output", lua.create_function(|lua, args: MultiValue| {
    for arg in args.iter() {
        match arg {
            Value::String(s) => { /* ... */ }
            Value::Number(n) => { /* ... */ }
            _ => { /* ... */ }
        }
    }
    Ok(())
})?)?;

// Read Lua global from inside a closure
let script_dir: String = lua.globals()
    .get::<String>("_quarto_script_dir")
    .unwrap_or_default();

// Read table field from inside a closure
let log_table: Table = lua.globals().get::<Table>("quarto")?.get::<Table>("log")?;
let level: i32 = log_table.get("loglevel")?;
```

**WASM considerations:** On `#[cfg(target_arch = "wasm32")]`, the Lua state is created
with restricted StdLib (no `io`, `os`, `debug`, `package`). Our `eprintln!`-based
logging avoids the `io.stderr` dependency. `std::path::Path` works fine in WASM.

## Risks

- **`_quarto_script_dir` per-handler in shortcode engine**: We track script dir per
  handler name and restore it before each `call()`. This handles the case where multiple
  extensions are loaded. Without this, the last `load_script()` would set the dir for
  ALL handlers.

- **Log output in tests**: `eprintln!` writes to stderr. Tests can't easily capture
  this, so just test that the functions don't error. Don't assert on output content.

- **WASM log output**: `eprintln!` in WASM goes to the browser console (via
  `console.error`). This is acceptable.

## Future Work (Tier 2+)

After this plan, the most impactful next APIs to implement would be:
1. `quarto.doc.is_format(name)` — format detection with aliases (needed by kbd, video)
2. `quarto.doc.add_html_dependency(dep)` — HTML dependency injection
3. `quarto.base64.encode/decode` — needed by placeholder
4. `quarto.utils.as_inlines/as_blocks` — AST conversion helpers
5. `quarto.utils.string_to_inlines/string_to_blocks` — markdown parsing from Lua

## TS Quarto Full API Surface (Reference)

For completeness, here is every `quarto.*` API in TS Quarto. Only Tier 1 (above) is
in scope for this plan.

```
quarto.warn(msg, elem?)           ✅ Already implemented
quarto.error(msg, elem?)          ✅ Already implemented
quarto.shortcode.read_arg         ✅ Already implemented
quarto.shortcode.error_output     ✅ Already implemented
quarto.json.decode                🔨 This plan (alias pandoc.json)
quarto.json.encode                🔨 This plan (alias pandoc.json)
quarto.log.output/error/warning/info/debug/trace  🔨 This plan
quarto.log.setloglevel/loglevel   🔨 This plan
quarto.utils.resolve_path         🔨 This plan
quarto.utils.type                 🔨 This plan (alias pandoc.utils.type)
quarto.utils.resolve_path_relative_to_document  ⏳ Tier 2 (deferred — needs doc path plumbing)
quarto.doc.is_format              ⏳ Tier 2
quarto.doc.add_html_dependency    ⏳ Tier 2
quarto.doc.include_text           ⏳ Tier 2
quarto.doc.has_bootstrap          ⏳ Tier 2
quarto.doc.input_file             ⏳ Tier 3
quarto.doc.output_file            ⏳ Tier 3
quarto.doc.use_latex_package      ⏳ Tier 3
quarto.doc.cite_method            ⏳ Tier 3
quarto.doc.pdf_engine             ⏳ Tier 3
quarto.base64.encode/decode       ⏳ Tier 2
quarto.utils.as_inlines           ⏳ Tier 2
quarto.utils.as_blocks            ⏳ Tier 2
quarto.utils.string_to_inlines    ⏳ Tier 2
quarto.utils.string_to_blocks     ⏳ Tier 2
quarto.utils.dump                 ⏳ Tier 3
quarto.utils.match                ⏳ Tier 3
quarto.utils.is_empty_node        ⏳ Tier 3
quarto.project.*                  ⏳ Tier 3
quarto.metadata.get               ⏳ Tier 3
quarto.variables.get              ⏳ Tier 3
quarto.config.version             ⏳ Tier 3
quarto.brand.*                    ⏳ Tier 3
quarto.format.*                   ⏳ Tier 3
quarto.version                    ⏳ Tier 3
quarto.paths.*                    ⏳ Tier 3
quarto.Callout/Tabset/etc         ⏳ Tier 3 (custom AST constructors)
```

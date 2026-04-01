# Plan A: `quarto.doc` Lua API (pampa crate)

## Status: Complete

---

## Overview

Add Lua APIs needed by built-in extensions: `quarto.version`,
`quarto.base64.encode()`, the `quarto.doc` namespace (`is_format`,
`has_bootstrap`, `add_html_dependency`, `include_text`), and
`dofile`/`loadfile` overrides for the restricted (WASM/test) Lua
environment. Also expose diagnostic extraction from the shortcode
engine, and implement a script-dir stack for correct path resolution
during nested script execution.

All work is in the `pampa` crate. A follow-up plan (Plan B,
`2026-04-01-lua-api-pipeline-wiring.md`) wires these into the quarto-core
pipeline and template, introduces a `FilterOutput` struct to replace
the widening tuple, and updates all callers.

---

## Background

### What TS Quarto provides

TS Quarto's Lua environment (defined in `init.lua`) provides:

- `quarto.version` — table `{1, 6, 1}`, used as
  `table.concat(quarto.version, '.')` to produce `"1.6.1"`
- `quarto.base64.encode(data)` — base64 string encoding
- `quarto.doc.is_format(fmt)` — **alias-based** format matching (NOT
  prefix matching). Reads the `FORMAT` global at call time (not
  captured at registration). Exact match against `FORMAT` first, then
  a hardcoded alias table: `"html"` matches html/html4/html5/epub/
  revealjs, `"html:js"` matches HTML but not epub, `"latex"`/`"pdf"`
  are synonyms, `"markdown"` matches markdown/gfm/commonmark, etc.
- `quarto.doc.add_html_dependency({name, stylesheets, scripts})` —
  register CSS/JS deps, deduplicated by name
- `quarto.doc.include_text(location, text)` — inject raw text at
  `"in-header"` / `"before-body"` / `"after-body"`
- `quarto.doc.has_bootstrap()` — check if Bootstrap is active

TS Quarto also provides a **script-dir stack** via
`_quarto.withScriptFile(file, callback)`: a Lua-side stack that tracks
nested script execution. `scriptDir()` returns the directory of the
innermost script. This is used for resolving relative paths (e.g. in
`add_html_dependency`) and is critical when a filter/shortcode calls
`dofile()` to load a helper from a subdirectory that itself registers
dependencies with relative paths.

TS Quarto communicates dependencies to the host process via a temp
file (JSON Lines) because Pandoc is a separate process. We don't need
that — our Lua runs in-process, so we follow the diagnostics pattern:
store in Lua tables, extract synchronously after execution.

### TS Quarto `add_html_dependency` field support

TS Quarto's `add_html_dependency` supports many fields: `name`,
`version`, `meta`, `links`, `scripts`, `stylesheets`, `resources`,
`serviceworkers`, `head`. We support `name`, `scripts`, `stylesheets`.
For other known TS Quarto fields (`version`, `meta`, `links`,
`resources`, `serviceworkers`, `head`), emit a "not yet supported"
diagnostic. For completely unknown fields, emit an error.

### TS Quarto `add_html_dependency` path resolution

Stylesheets and scripts can be either bare strings (`"style.css"`) or
tables (`{name = "style.css", path = "style.css"}`). After
normalization, relative paths are resolved via `resolvePathExt()`:
join `scriptDir()` + path, then resolve against working directory if
still relative. This means paths are relative to the currently
executing script file, tracked via the script-dir stack.

### Current q2 state

- **No `quarto.doc` namespace**, no `quarto.version`, no `quarto.base64`
- **`quarto.utils.resolve_path(path)`** — exists in `quarto_api.rs`,
  resolves relative paths against `_quarto_script_dir` global
- **`FORMAT` global** — already set in both `LuaShortcodeEngine::new()`
  and `apply_lua_filter()` to the target format string (e.g. `"html"`)
- **Diagnostics pattern** (model for deps):
  1. `register_quarto_namespace()` creates `quarto._diagnostics` table
  2. `quarto.warn()` / `quarto.error()` push entries onto it
  3. `extract_lua_diagnostics(&lua)` reads them back after execution
  4. Filter returns `Vec<DiagnosticMessage>` alongside the AST
  - **Gap**: Shortcode execution does NOT extract diagnostics — the
    `LuaShortcodeEngine` holds the Lua state but never calls
    `extract_lua_diagnostics()`
- **base64** — `base64` crate is a dependency; `mediabag.rs` uses
  `BASE64_STANDARD.encode()` in `make_data_uri`
- **`dofile()`/`loadfile()`** — available from mlua's base library
  (C `luaopen_base`). On native they work fine via C `fopen`. On
  WASM, `fopen` returns null (from `c_shim.rs`) so they fail silently.
  Overwrite needed only in the restricted env (`#[cfg(any(target_arch
  = "wasm32", test))]`). Native is fine as-is.
- **`_quarto_script_dir`** — flat global, set once per filter or
  updated per shortcode handler call. No stack semantics. Insufficient
  for nested `dofile()` from subdirectories.

### Key files

| File | Role |
|---|---|
| `crates/pampa/src/lua/quarto_api.rs` | Registers `quarto.*` (json, log, utils); `resolve_path` reads `_quarto_script_dir` |
| `crates/pampa/src/lua/shortcode.rs` | `LuaShortcodeEngine` — sets up globals, calls handlers |
| `crates/pampa/src/lua/filter.rs` | `apply_lua_filter()` — creates Lua state, runs filter, extracts diagnostics |
| `crates/pampa/src/lua/diagnostics.rs` | `register_quarto_namespace()`, `extract_lua_diagnostics()` |
| `crates/pampa/src/lua/mediabag.rs` | `make_data_uri` uses base64 internally |
| `crates/pampa/src/lua/io_wasm.rs` | Synthetic `io.open` for restricted env |
| `crates/pampa/src/lua/mod.rs` | Module declarations and public exports |

---

## Work Items

### Phase 1: `quarto.version` and `quarto.base64`

- [x] **1.1** In `quarto_api.rs`, in `register_quarto_api()`, set
  `quarto.version` to a Lua table `{0, 1, 0}` (matching our current
  version). TS Quarto uses a list so extensions do
  `table.concat(quarto.version, '.')`.

- [x] **1.2** In `quarto_api.rs`, add `quarto.base64` table with an
  `encode(data)` function. Use the existing `base64` crate dependency
  with `BASE64_STANDARD.encode(data.as_bytes())`. Returns a string.

- [x] **1.3** Unit tests in pampa:
  - `quarto.version` is a table; `table.concat(quarto.version, '.')`
    produces `"0.1.0"`
  - `quarto.base64.encode("hello")` produces `"aGVsbG8="`
  - `quarto.base64.encode("")` produces `""`

### Phase 2: Script-dir stack and `dofile`/`loadfile`

- [x] **2.1** Replace the flat `_quarto_script_dir` global with a
  stack-based mechanism. Implement a Lua-side stack matching TS
  Quarto's pattern:
  - `_quarto_script_dir_stack` — Lua table used as a stack
  - `_quarto_push_script_dir(dir)` — pushes a directory onto the stack
  - `_quarto_pop_script_dir()` — pops the top entry
  - `_quarto_script_dir()` — returns the top of the stack (or empty
    string if empty)

  Update `quarto.utils.resolve_path` (in `quarto_api.rs`) to read
  from the stack top instead of the flat global.

  Update callers that currently set `_quarto_script_dir`:
  - `filter.rs`: push the filter file's directory onto the stack at
    setup, pop after execution
  - `shortcode.rs`: push the handler's script dir before each handler
    call, pop after

- [x] **2.2** In the restricted Lua setup path
  (`#[cfg(any(target_arch = "wasm32", test))]`) in both `filter.rs`
  and `shortcode.rs`, after registering synthetic io/os, overwrite
  `dofile` and `loadfile` globals with Rust functions that:
  - Read the file via `runtime.file_read_string()` (need string
    content for Lua source)
  - **Push** the loaded file's directory onto the script-dir stack
    before execution, **pop** after (so that nested `resolve_path`
    calls resolve against the loaded file's directory)
  - Compile via `lua.load(content).set_name(filename)`
  - For `dofile`: execute immediately and return all results
  - For `loadfile`: return the compiled chunk without executing
    (no stack push/pop — the chunk hasn't run yet; the caller may
    run it later)
  - On error, `loadfile` returns `(nil, error_message)` matching Lua
    semantics; `dofile` propagates the error

  The native code path is left alone — C `fopen` works fine there and
  we don't need to force all file access through `SystemRuntime` on
  native.

  **Path resolution for dofile/loadfile**: resolve relative paths
  against the current script dir (top of stack), matching
  `resolve_path` behavior. If the stack is empty, use the path as-is.
  In the restricted env (WASM/test), also apply the `/project/` prefix
  for paths that aren't absolute, matching `io_wasm.rs` conventions.

- [x] **2.3** Unit tests:
  - `dofile("path/to/script.lua")` executes and returns values
  - `loadfile("path/to/script.lua")` returns a callable chunk
  - `dofile` with nonexistent file returns an error
  - `loadfile` with nonexistent file returns `(nil, error_message)`
  - Test in both filter and shortcode contexts
  - **Script-dir stack test**: extension in `/ext/` calls
    `dofile("/ext/helpers/ui.lua")`, and `ui.lua` calls
    `quarto.utils.resolve_path("style.css")` — should resolve to
    `/ext/helpers/style.css`, not `/ext/style.css`

### Phase 3: `quarto.doc` namespace

- [x] **3.1** Create `crates/pampa/src/lua/quarto_doc.rs` with:

  ```rust
  pub fn register_quarto_doc(lua: &Lua) -> Result<()>
  ```

  No `format` parameter — `is_format` reads the `FORMAT` global at
  call time, matching TS Quarto's behavior and keeping the signature
  consistent with `register_quarto_api(lua)`.

  This registers the `quarto.doc` table on the existing `quarto`
  global with these functions:

  **`quarto.doc.is_format(fmt)`** — alias-based format matching.
  Reads `FORMAT` global at call time. Logic:
  1. Exact match: if `fmt == FORMAT`, return true
  2. Alias table (hardcoded, matching TS Quarto `_format.lua`):
     - `"html"` → true if FORMAT in {html, html4, html5, epub, epub2,
       epub3, revealjs, s5, slidy, slideous, dzslides}
     - `"html:js"` → true if `is_format("html")` and NOT
       `is_format("epub")`
     - `"latex"` or `"pdf"` → true if FORMAT in {latex, beamer, pdf}
     - `"epub"` → true if FORMAT starts with "epub"
     - `"markdown"` → true if FORMAT in {markdown, markdown_github,
       gfm, commonmark, commonmark_x, markua}
     - `"asciidoc"` or `"asciidoctor"` → true if FORMAT in
       {asciidoc, asciidoctor}
     - Everything else → false (exact match only)
  3. Also register `quarto.doc.isFormat` as alias (TS Quarto provides
     both; `kbd.lua` uses both forms)

  **`quarto.doc.has_bootstrap()`** — return `true` when format is HTML
  (we always use Bootstrap themes in HTML output). Implemented as
  `is_format("html") and not is_format("epub")`.

  **`quarto.doc.add_html_dependency(dep)`** — Lua function that:
  - Validates `dep.name` (required string)
  - Checks for known-but-unsupported TS Quarto fields (`version`,
    `meta`, `links`, `resources`, `serviceworkers`, `head`): if
    present, emit a diagnostic via `quarto.warn()` saying the field
    is not yet supported
  - Checks for completely unknown fields: emit an error
  - Reads optional `dep.stylesheets` and `dep.scripts`: each entry
    can be a string or a `{name, path}` table (matching TS Quarto's
    `resolveFileDependencies`). Strings are normalized to
    `{name = filename, path = original}`.
  - Resolves relative paths against the current script dir (using
    the script-dir stack), matching TS Quarto's `resolvePathExt`
  - Deduplicates by name: checks `quarto.doc._dependencies` table,
    skips if name already registered
  - Pushes `{name, stylesheets, scripts}` onto
    `quarto.doc._dependencies`
  - Also register `quarto.doc.addHtmlDependency` as camelCase alias

  **`quarto.doc.include_text(location, text)`** — Lua function that:
  - Validates `location` is one of `"in-header"`, `"before-body"`,
    `"after-body"`
  - Pushes `{location=location, text=text}` onto
    `quarto.doc._text_includes`
  - Also register `quarto.doc.includeText` as camelCase alias

  **Internal tables** (not called by extensions directly):
  - `quarto.doc._dependencies` — array table for deps
  - `quarto.doc._text_includes` — array table for text injections

- [x] **3.2** Create extraction functions in `quarto_doc.rs`:

  ```rust
  pub fn extract_html_dependencies(lua: &Lua) -> Result<Vec<HtmlDependency>>
  pub fn extract_text_includes(lua: &Lua) -> Result<Vec<TextInclude>>
  ```

  Where:
  ```rust
  pub struct HtmlDependency {
      pub name: String,
      pub stylesheets: Vec<PathBuf>,  // resolved absolute paths
      pub scripts: Vec<PathBuf>,      // resolved absolute paths
  }

  pub struct TextInclude {
      pub location: IncludeLocation,
      pub content: String,
  }

  pub enum IncludeLocation {
      InHeader,
      BeforeBody,
      AfterBody,
  }
  ```

  These read the Lua tables and convert to Rust types, same pattern
  as `extract_lua_diagnostics()` in `diagnostics.rs`.

- [x] **3.3** Add `mod quarto_doc;` to `crates/pampa/src/lua/mod.rs`
  and add public exports for `HtmlDependency`, `TextInclude`,
  `IncludeLocation`, `extract_html_dependencies`,
  `extract_text_includes`.

- [x] **3.4** Call `register_quarto_doc()` in both:
  - `shortcode.rs` `LuaShortcodeEngine::new()` — after
    `register_quarto_api()`
  - `filter.rs` `apply_lua_filter()` — after `register_quarto_api()`

- [x] **3.5** Unit tests for `quarto.doc.*`:
  - `is_format("html")` returns true when FORMAT is "html"
  - `is_format("html")` returns true when FORMAT is "html5"
  - `is_format("html:js")` returns true when FORMAT is "html"
  - `is_format("html:js")` returns false when FORMAT is "epub3"
  - `is_format("latex")` returns true when FORMAT is "pdf"
  - `is_format("unknown")` returns false
  - `isFormat` alias works
  - `is_format` reads FORMAT at call time (change FORMAT global
    between calls, verify result changes)
  - `has_bootstrap()` returns true for html, false for latex
  - `add_html_dependency` stores and deduplicates
  - `add_html_dependency` accepts string and `{name, path}` entries
  - `add_html_dependency` resolves relative paths via script-dir stack
  - `add_html_dependency` warns on unsupported fields (e.g. `meta`)
  - `add_html_dependency` errors on unknown fields
  - `addHtmlDependency` camelCase alias works
  - `include_text` stores with correct locations
  - `include_text` rejects invalid locations
  - `includeText` camelCase alias works
  - `extract_html_dependencies` returns correct data
  - `extract_text_includes` returns correct data

### Phase 4: Expose extraction from shortcode engine

- [x] **4.1** Add methods to `LuaShortcodeEngine`:

  ```rust
  pub fn extract_diagnostics(&self) -> Result<Vec<DiagnosticMessage>>
  pub fn extract_html_dependencies(&self) -> Result<Vec<HtmlDependency>>
  pub fn extract_text_includes(&self) -> Result<Vec<TextInclude>>
  ```

  These delegate to the corresponding `extract_*` functions on the
  engine's internal `Lua` state. This is the same data that
  `apply_lua_filter` already extracts for filters (diagnostics) —
  we're just making the shortcode engine expose it too.

- [x] **4.2** Also call `extract_html_dependencies` and
  `extract_text_includes` in `apply_lua_filter` (after the existing
  `extract_lua_diagnostics` call at filter.rs:208), but **discard**
  the results for now. This verifies extraction works without changing
  the return type. The return type widening and `FilterOutput` struct
  are deferred to Plan B.

  Add a `// TODO(plan-b): return these instead of discarding` comment.

- [x] **4.3** Unit tests:
  - Shortcode engine: call a shortcode that uses
    `quarto.doc.add_html_dependency()`, then call
    `engine.extract_html_dependencies()` and verify
  - Shortcode engine: call a shortcode that uses `quarto.warn()`,
    then call `engine.extract_diagnostics()` and verify
  - Filter: run a filter that calls `add_html_dependency()`, verify
    extraction works (call extract directly on the Lua state in a
    test helper)
  - Filter: run a filter that calls `include_text()`, verify
    extraction works

### Phase 5: Verify

- [x] **5.1** `cargo nextest run -p pampa` — all pampa tests pass
- [x] **5.2** `cargo build --workspace` — full workspace compiles.
  No callers outside pampa are affected because the return type of
  `apply_lua_filter` / `apply_lua_filters` / `apply_filters` is
  unchanged in this plan.

---

## Design Notes

### `is_format` reads `FORMAT` at call time, not at registration

TS Quarto's `isFormat` (in `_format.lua`) references the `FORMAT`
global directly: `if FORMAT == to then`. It does not capture format at
registration time. We match this behavior. This keeps
`register_quarto_doc(lua)` consistent with `register_quarto_api(lua)`
(no extra parameters) and faithful to TS Quarto's semantics.

### `is_format` is alias-based, not prefix-based

TS Quarto's `is_format` uses a hardcoded if/else chain mapping format
names to groups. `is_format("html")` doesn't do string prefix matching
— it checks if FORMAT is in a specific set (html, html4, html5, epub*,
slide formats). `"html:js"` is a special alias meaning "HTML that
supports JavaScript" (HTML minus epub), not a prefix:variant syntax.

We replicate the alias table for the formats we care about. Unrecognized
queries fall through to exact match only.

### Script-dir stack

TS Quarto maintains a Lua-side stack of script file paths
(`scriptFile = {}`) via `_quarto.withScriptFile(file, callback)`. The
`scriptDir()` function returns the directory of the topmost entry.
This is needed because extensions can `dofile()` helpers from
subdirectories, and those helpers may register dependencies with
relative paths that should resolve against their own directory.

Our previous flat `_quarto_script_dir` global would resolve all paths
against the outermost script's directory, which is wrong when a helper
in a subdirectory registers a dependency.

We implement the stack in Lua (matching TS Quarto) with push/pop
functions callable from both Lua and Rust. The `dofile` override
pushes the loaded file's directory before execution and pops after.

### `add_html_dependency` field support

We support the fields actually needed by built-in extensions: `name`,
`scripts`, `stylesheets`. For other fields that TS Quarto supports
(`version`, `meta`, `links`, `resources`, `serviceworkers`, `head`),
we emit a "not yet supported" diagnostic via `quarto.warn()` so the
user knows the field was ignored. For completely unknown fields, we
error — this catches typos and prevents silent data loss.

### `dofile`/`loadfile` — restricted env only

On native, the C `fopen`-based implementations work fine. We only
overwrite in the restricted Lua environment (`#[cfg(any(target_arch =
"wasm32", test))]`) where C file I/O doesn't work. This matches the
pattern used for `io.open` (in `io_wasm.rs`) and `os.*` (in
`os_wasm.rs`).

Note: `loadfile` does NOT push/pop the script-dir stack — it returns
an unexecuted chunk. The stack push/pop happens when `dofile` executes
the chunk. If the user calls `loadfile` then manually executes the
chunk, paths will resolve against whatever script dir is current at
execution time — this matches TS Quarto's behavior (only
`withScriptFile` manages the stack, and only `dofile`-like paths use
it).

### Return type unchanged in Plan A

Plan A does NOT widen the return type of `apply_lua_filter` or
`apply_lua_filters`. Extraction functions are called in filters but
results are discarded. This means:
- No changes to `unified_filter.rs`
- No changes to callers in `quarto-core`
- No changes to the ~100 filter test sites that destructure tuples
- `cargo build --workspace` succeeds without touching quarto-core

Plan B introduces a `FilterOutput` struct, widens the return types,
updates `unified_filter.rs` and quarto-core callers, and refactors
the test destructuring.

### Extraction pattern

All three extraction types (diagnostics, HTML deps, text includes)
follow the same pattern:
1. Lua function pushes data onto an internal `quarto.*` table
2. Rust `extract_*(&lua)` reads the table after execution
3. Caller gets a `Vec<T>` of typed Rust structs

No `Arc<Mutex>` needed — the pipeline is single-threaded through each
stage, and Lua state is not shared.

### Deduplication

HTML dependencies are deduplicated by `name` at Lua registration time
(matching TS Quarto). If a shortcode is called 5 times, the dependency
is registered once. Cross-engine deduplication (e.g. shortcode engine
+ filter engine both register same dep) is not needed — in practice,
extensions use either shortcodes or filters, not both for the same dep.

---

## Files Touched

| File | Change |
|---|---|
| `crates/pampa/src/lua/quarto_api.rs` | Add `quarto.version`, `quarto.base64`; update `resolve_path` to use script-dir stack |
| `crates/pampa/src/lua/quarto_doc.rs` | **New**: `quarto.doc` namespace, extraction functions, types |
| `crates/pampa/src/lua/mod.rs` | Add `quarto_doc` module, update public exports |
| `crates/pampa/src/lua/shortcode.rs` | Call `register_quarto_doc()`, add extraction methods, use script-dir stack |
| `crates/pampa/src/lua/filter.rs` | Call `register_quarto_doc()`, overwrite `dofile`/`loadfile`, use script-dir stack, extract (but discard) deps/includes |
| Unit test files | New tests for all APIs |

# Plan: WASM Lua `io` and `os` Support

## Overview

Lua extensions running in the browser (WASM) currently lack `io` and `os` globals,
causing real-world extensions like `lipsum` to fail. This plan registers synthetic
`io` and `os` tables from Rust, providing exactly the functions that can be safely
implemented in the WASM environment.

**Root cause of the CI blind spot:** native CI uses `Lua::new()` (full stdlib),
WASM uses `Lua::new_with(restricted)`. This plan fixes the blind spot first, which
will immediately reveal any existing tests that depend on `os` or `io`.

**Scope:** `crates/pampa/src/lua/` primarily, plus a small addition to
`SystemRuntime` in `crates/quarto-system-runtime/`.

---

## Why NOT loading `StdLib::IO | StdLib::OS`

Loading these via `new_with` would invoke the real `luaopen_io` / `luaopen_os` from
`liolib.c` / `loslib.c` (not the no-op stubs in c_shim.rs, which only satisfy linker
references from `linit.c`). Most of those functions require C symbols not present in
the WASM sysroot, causing linker errors:

| Missing C symbol | Needed by |
|---|---|
| `fseek`, `ftell` | `file:seek()` |
| `setvbuf` | `file:setvbuf()` |
| `tmpfile` | `io.tmpfile()` |
| `localtime`, `gmtime`, `strftime` | `os.date()` |
| `system` | `os.execute()` |
| `exit` | `os.exit()` |
| `getenv` | `os.getenv()` |
| `remove`, `rename` | `os.remove()`, `os.rename()` |
| `tmpnam` | `os.tmpname()` |
| `difftime` | `os.difftime()` |

Instead, we register hand-crafted Lua tables from Rust with only the functions
that can be safely implemented.

**What we expose:**

`io`: `open` (VFS-backed, read and write), `type`
`os`: `time` (via `SystemRuntime::unix_timestamp()`), `clock` (returns 0),
`difftime` (pure arithmetic)

---

## Work Items

### TDD Workflow

This plan follows strict TDD. Phases are numbered for grouping, but within each
implementation item the sequence is always: **write test → verify it fails →
implement → verify it passes**. Phase 1 is the exception (it only changes cfg
guards and records failures).

**Commit policy:** Do NOT commit after Phase 1 alone — the test suite will be
intentionally broken until Phase 3 provides the synthetic tables. Phases 1 through 3
should land together in a single commit (or a series of commits where each leaves
the test suite green).

### Phase 1: Expose the CI blind spot

The goal is to make existing shortcode/filter tests run against the WASM-restricted
stdlib on native CI, so any `os`/`io` dependencies fail immediately.

- [x] **1.1** In `shortcode.rs`, change the `#[cfg]` guard so that tests always use
  the restricted stdlib:
  ```rust
  #[cfg(any(target_arch = "wasm32", test))]
  let lua = {
      use mlua::StdLib;
      let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
      Lua::new_with(libs, mlua::LuaOptions::default()).map_err(LuaShortcodeError::LuaError)?
  };
  #[cfg(not(any(target_arch = "wasm32", test)))]
  let lua = Lua::new();
  ```
- [x] **1.2** Same change in `filter.rs`
- [x] **1.3** Run `cargo nextest run -p pampa` and record which tests now fail.
  **Result:** 8 filter tests fail, all with `attempt to index a nil value (global 'io')`:
  - `test_typewise_traversal_order`
  - `test_type_specific_overrides_generic`
  - `test_generic_block_fallback`
  - `test_topdown_document_level_traversal_order`
  - `test_topdown_stop_signal_prevents_descent`
  - `test_topdown_blocks_filter_order`
  - `test_elem_walk_typewise_traversal_order`
  - `test_inlines_walk_typewise_order`

  All use `io.open(path, "w")` with `file:write()` and `file:flush()` to track
  filter traversal order. These do NOT need migration — they will be fixed by
  Phase 3's synthetic `io` implementation which supports write mode.

- [x] **1.4** Document the convention in `claude-notes/instructions/testing.md`:
  shortcode/filter tests always run against the WASM-restricted stdlib so that
  native CI catches WASM incompatibilities.

### Phase 2: `SystemRuntime::unix_timestamp()`

`pampa` does not (and should not) depend on `js_sys`. Instead, `os.time()` goes
through the runtime abstraction.

- [x] **2.1** Add `fn unix_timestamp(&self) -> RuntimeResult<u64>` to the
  `SystemRuntime` trait in `crates/quarto-system-runtime/src/traits.rs`.
  Default implementation: `std::time::SystemTime::now().duration_since(UNIX_EPOCH)`.
- [x] **2.2** Override in `WasmRuntime` (`wasm.rs`):
  `(js_sys::Date::now() / 1000.0) as u64`
- [x] **2.3** `SandboxedRuntime` delegates to inner runtime (explicit override added).

### Phase 3: Synthetic `io` and `os` tables

For each item below, write the corresponding test (from Phase 4) **first**, verify
it fails, then implement.

- [x] **3.1** Create `crates/pampa/src/lua/os_wasm.rs` with
  `register_wasm_os(lua, runtime)`.

  Registers a fresh `os` table as a Lua global containing:

  - **`os.time()`** — calls `runtime.unix_timestamp()`, returns as integer.
  - **`os.clock()`** — returns `0.0` (no meaningful CPU time in browser).
  - **`os.difftime(t2, t1)`** — returns `t2 - t1` as a number (pure arithmetic).

- [x] **3.2** Create `crates/pampa/src/lua/io_wasm.rs` with
  `register_wasm_io(lua, runtime)`.

  Registers a fresh `io` table as a Lua global containing:

  - **`io.open(path, mode)`** — Rust closure backed by `SystemRuntime`:
    - Read modes (`"r"`, `"rb"`, or nil/default): calls `runtime.file_read()`,
      returns a **read file handle** (see 3.3)
      - Error: returns `nil, <error string>`
    - Write modes (`"w"`, `"wb"`): returns a **write file handle** (see 3.4)
      that buffers content and flushes to `runtime.file_write()` on close/flush.
    - Append modes (`"a"`, `"ab"`): returns a **write file handle** that
      pre-loads existing content (via `runtime.file_read()`, ignoring errors for
      new files), then appends. Flushes via `runtime.file_write()`.
  - **`io.type(x)`** — returns `"file"` if x is an open file handle table we
    created, `"closed file"` if handle is closed, `nil` otherwise.

  **Path resolution for `io.open`**: Follows standard Lua semantics.
  - Absolute paths (starting with `/`): used as-is (maps directly to VFS paths
    like `/project/...`).
  - Relative paths: resolved relative to the current working directory. In our
    WASM context this is `/project/` (the VFS project root). This matches standard
    Lua behaviour where `io.open` resolves relative to the process CWD.
  - Extensions that need script-relative paths already use
    `quarto.utils.resolve_path()` to produce absolute paths before calling
    `io.open` (confirmed in `lipsum.lua`).

- [x] **3.3** Implement the **read** file handle table returned by `io.open` in
  read mode.

  Plain Lua table with a metatable so `:read()` and `:close()` work as methods.
  Content is stored in a Lua string (set as a named field on the table); a
  numeric position field tracks the byte offset for stateful reads.

  Supported `file:read(fmt)` formats (per Lua 5.4 reference manual §6.8):
  - `"a"` or `"*a"` — return all remaining content from current position
  - `"l"` or `"*l"` — return next line without trailing newline (default when no
    argument given)
  - `"L"` or `"*L"` — return next line with trailing newline
  - `"n"` or `"*n"` — read a numeral: skip leading whitespace, then parse a
    number from the current position. Return `nil` if no valid number found.
  - numeric `n` — read exactly n bytes from current position

  `file:close()` — marks handle closed, returns `true`.
  `file:lines(...)` — not implemented; returns error if called.

- [x] **3.4** Implement the **write** file handle table returned by `io.open` in
  write/append mode.

  Plain Lua table with a metatable. Stores the file path and an internal buffer
  (Lua string or Rust-side `Vec<u8>` via the runtime).

  - **`file:write(...)`** — accepts one or more string/number arguments (per Lua
    5.4 §6.8). Appends each to the internal buffer. Returns the file handle (for
    chaining, e.g. `f:write("a"):write("b")`).
  - **`file:flush()`** — writes the accumulated buffer to the VFS via
    `runtime.file_write(path, buffer)`. The buffer is NOT cleared — subsequent
    writes keep appending, and the next flush overwrites the file with the full
    content. This ensures the file always contains the complete output.
  - **`file:close()`** — calls flush, then marks handle closed. Returns `true`.

  **Design rationale:** The VFS (`WasmRuntime::file_write`) and
  `NativeRuntime::file_write` both do full-file overwrites. By accumulating a
  buffer and writing the whole thing on each flush, we get correct behavior for
  incremental writes (like the filter test harness pattern:
  `file:write("line\n"); file:flush()`).

- [x] **3.5** Wire up both registration functions in `shortcode.rs` (inside the
  `#[cfg(any(target_arch = "wasm32", test))]` block, after `Lua::new_with`).
  Both functions receive `runtime.clone()`.
- [x] **3.6** Wire up both in `filter.rs` (same location). In `filter.rs`,
  `runtime` is a parameter (`Arc<dyn SystemRuntime>`).
- [x] **3.7** Add `io_wasm` and `os_wasm` to `mod.rs`

### Phase 4: Tests

TDD — write each test **before** implementing the corresponding Phase 3 item.

**`os` tests:**
- [x] **4.1** Unit test: `os.time()` returns a positive integer (not nil)
- [x] **4.2** Unit test: `os.clock()` returns a number
- [x] **4.3** Unit test: `os.difftime(10, 3)` returns `7`

**`io` read tests:**
- [x] **4.4** Unit test: `io.open("/nonexistent/file.txt")` returns nil + error string
- [x] **4.5** Unit test: `io.open` + `:read("*a")` returns full file content via native FS
- [x] **4.6** Unit test: `:read("*l")` returns lines one at a time
- [x] **4.7** Unit test: `:read("*n")` parses a number from current position
- [x] **4.8** Unit test: `:read(5)` reads exactly 5 bytes
- [x] **4.9** Unit test: `io.type()` correctly identifies handle vs closed handle
  vs non-handle

**`io` write tests:**
- [x] **4.10** Unit test: `io.open(path, "w")` + `file:write("hello")` +
  `file:close()` produces file with content "hello"
- [x] **4.11** Unit test: `file:write("a"); file:flush(); file:write("b");
  file:flush()` produces file with content "ab"
- [x] **4.12** Unit test: `io.open(path, "a")` appends to existing file content
- [x] **4.13** Unit test: `file:write()` returns the file handle (chaining)

**Path and integration tests:**
- [x] **4.14** Unit test: relative path resolves to `/project/<path>` in VFS
- [x] **4.15** Verify existing filter traversal tests pass (all 8 previously-broken
  tests now pass with the synthetic `io` write support)
- [ ] **4.16** Integration test: `lipsum` shortcode renders successfully end-to-end
  (add a `{{< lipsum 1 >}}` test case to the shortcode smoke tests)

### Phase 5: WASM build verification

- [ ] **5.1** Run `cargo xtask verify --skip-hub-tests` to confirm Rust + WASM build
- [ ] **5.2** Manual smoke test: start hub against `~/docs/lipsum`, open in browser,
  confirm `{{< lipsum 1 >}}` renders a paragraph of lorem ipsum

---

## Design Notes

### Why `#[cfg(any(target_arch = "wasm32", test))]`

Tests permanently run against the restricted stdlib so that any future Lua script
added to a test that uses `os`, `io`, `package`, or `debug` fails immediately on
native CI. This is the right ongoing behaviour: we want CI to be a proxy for WASM.

### Why synthetic tables instead of `StdLib::IO | OS`

Detailed analysis above. Short version: most functions in both libraries reference
C symbols absent from the WASM sysroot, causing linker errors. Registering our own
tables gives us precise control and zero linker risk.

### Why a plain Lua table (not UserData) for the file handle

The file handle only needs to survive within a single Lua chunk — created, read,
and closed in the same call. A plain table with a metatable is simpler than mlua
UserData and avoids lifetime issues with borrowed content.

### `os.time()` goes through `SystemRuntime`

`pampa` does not depend on `js_sys` and should not. The `SystemRuntime` trait
already abstracts platform differences. We add `unix_timestamp()` to the trait:
- `NativeRuntime`: `std::time::SystemTime::now()` (default impl)
- `WasmRuntime`: `js_sys::Date::now() / 1000.0`
- `SandboxedRuntime`: delegates to inner runtime

This makes `math.randomseed(os.time())` produce different sequences across runs,
unlike the C shim which returns a constant.

### Path resolution in `io.open`

`io.open` uses standard Lua path semantics: absolute paths are used as-is,
relative paths resolve relative to CWD. In the WASM environment, CWD is
effectively `/project/` (the VFS root).

Extensions that need script-relative file access use
`quarto.utils.resolve_path("filename")` to produce an absolute path, then pass
that to `io.open`. This is the pattern used by `lipsum` and is the recommended
approach. We do NOT add implicit script-relative resolution to `io.open` — that
would violate standard Lua semantics.

### Relationship to `pandoc.system.read_file()`

`pandoc.system.read_file(path)` (registered in `system.rs`) is a Quarto/Pandoc
API that returns file content as a string directly. `io.open(path)` is the
standard Lua API returning a file handle object with `:read()`, `:close()`, etc.

Both are needed:
- `io.open` is required for compatibility with existing Lua libraries and
  extensions that use standard Lua idioms (`io.open` + `file:read("*a")`).
- `pandoc.system.read_file` is the Quarto-specific convenience API.

Both ultimately go through `runtime.file_read()` for the actual I/O.

### Write mode — backed by VFS

Write mode is fully supported. Both `WasmRuntime` (VFS) and `NativeRuntime`
(filesystem) implement `file_write()`. The VFS writes into the in-memory
filesystem; `NativeRuntime` writes to disk.

The write file handle accumulates content in a buffer. `file:flush()` and
`file:close()` write the full buffer to the path via `runtime.file_write()`.
This "overwrite with full content" approach works because both VFS and native
`file_write` are full-file overwrites. It correctly supports the common pattern
of incremental `file:write()` + `file:flush()` calls.

This also means the existing filter traversal-order tests (which use
`io.open(path, "w")` + `file:write()` + `file:flush()`) work unmodified.

---

## Files touched

| File | Change |
|---|---|
| `crates/quarto-system-runtime/src/traits.rs` | Add `unix_timestamp()` with default impl |
| `crates/quarto-system-runtime/src/wasm.rs` | Override `unix_timestamp()` with `js_sys` |
| `crates/pampa/src/lua/io_wasm.rs` | New — synthetic `io` table with VFS-backed `open` (read + write) |
| `crates/pampa/src/lua/os_wasm.rs` | New — synthetic `os` table with `time`, `clock`, `difftime` |
| `crates/pampa/src/lua/mod.rs` | Add `io_wasm`, `os_wasm` modules |
| `crates/pampa/src/lua/shortcode.rs` | Extend cfg guard to `test`, register synthetic tables |
| `crates/pampa/src/lua/filter.rs` | Same |
| `claude-notes/instructions/testing.md` | Document WASM-compat test convention |

# Plan: Fix Shortcode Argument Passing for TS Quarto Compatibility

## Status: Complete (commit 5315af95)

---

## Codebase Context for New Agents

### Repository structure
This is a Rust monorepo ("q2" / "Rust Quarto"). The relevant crate is:
- **`pampa`** (`crates/pampa/`) — the core QMD→Pandoc AST engine. Contains the
  Lua filter and shortcode subsystem in `crates/pampa/src/lua/`.

### Lua shortcode engine
The shortcode engine lives in `crates/pampa/src/lua/shortcode.rs`. Key types:
- `LuaShortcodeEngine` — holds Lua state, handler registry, and runtime ref.
  Created via `LuaShortcodeEngine::new(target_format, runtime)`.
- `ShortcodeArgs` — struct with `positional: Vec<String>`,
  `keyword: Vec<(String, String)>`, `metadata: Vec<(String, String)>`.
- Handler signature (Lua side): `function(args, kwargs, meta, raw_args, context)`

### How handlers are called (current code, ~line 242-266)
`build_and_call()` constructs 5 arguments and calls the Lua handler:
1. `lua_args` — built by `build_args_table()` (THIS IS BROKEN)
2. `lua_kwargs` — built by `build_kwargs_table()`
3. `lua_meta` — built by `build_meta_table()`
4. `lua_raw_args` — built by `build_raw_args()` (this one is correct)
5. `ctx_str` — `"block"`, `"inline"`, or `"text"`

### The Pandoc/Quarto Lua API in pampa
- `pandoc.*` constructors are registered in `crates/pampa/src/lua/constructors.rs`
  — includes `pandoc.Str()`, `pandoc.Para()`, `pandoc.Inlines()`, etc.
- `pandoc.utils.*` is registered in `crates/pampa/src/lua/utils.rs` —
  includes `pandoc.utils.stringify()` which converts AST elements to plain text.
- `quarto.*` API is registered in `crates/pampa/src/lua/quarto_api.rs` —
  includes `quarto.json`, `quarto.log`, `quarto.utils`. The `quarto.shortcode`
  sub-namespace is registered in `shortcode.rs` (function
  `register_shortcode_api`, ~line 348).
- Metatable infrastructure exists: `set_metatable()`, `__index` patterns are
  used in `io_wasm.rs`, `shortcode.rs:150`, `readwrite.rs`, `constructors.rs`.
- `pandoc.Inlines({})` constructor is available (registered in
  `constructors.rs:1574`).

### Testing
- Use `cargo nextest run` (never `cargo test`, never pipe through `tail`).
- Tests run against restricted Lua stdlib (no raw `io`/`os` — use the synthetic
  versions from `io_wasm.rs` and `os_wasm.rs`).
- Shortcode tests are in `shortcode.rs` (bottom of file, `#[cfg(test)]` module).
- Smoke-all integration tests are in `crates/quarto/tests/smoke-all/`. Run with
  `cargo nextest run -p quarto --test smoke_all`.

### mlua API quirks
- `mlua::String::to_str()` returns `BorrowedStr`, not `&str`. Use `.as_ref()`
  when matching.
- `Table::set_metatable()` returns `Result` — must use `?`.
- To create a Lua function from a string: `lua.load("...").eval::<Function>()?`

### Reference: TS Quarto shortcode source
The TypeScript Quarto shortcode handler is at
`~/src/quarto-cli/src/resources/filters/customnodes/shortcodes.lua`
(function `callShortcodeHandler`, ~line 373).

---

## Overview

Shortcode handler arguments are currently passed in a format incompatible with
existing Lua extensions (e.g., lipsum). This causes extensions that call
`pandoc.utils.stringify(args[1])` to get empty strings instead of the argument
value.

**Root cause:** `build_args_table` (shortcode.rs ~line 268) wraps each positional
arg in a `{value = "string"}` table, but TS Quarto passes plain strings directly.

## How TS Quarto Does It

From `quarto-cli/src/resources/filters/customnodes/shortcodes.lua`:

```lua
function callShortcodeHandler(handler, shortCode, context)
  local args = pandoc.List()
  local kwargs = setmetatable({}, {
    __index = function() return pandoc.Inlines({}) end
  })
  for _, arg in ipairs(shortCode.args) do
    if arg.name then
      kwargs[arg.name] = arg.value
    else
      args:insert(arg.value)      -- plain value, not {value=...}
    end
  end
  local meta = setmetatable({}, {
    __index = function(t, i) return readMetadata(i) end
  })
  return handler.handle(args, kwargs, meta, shortCode.raw_args, context)
end
```

Key points:
- **`args`**: `pandoc.List` of plain strings (or pandoc Inlines for nested
  shortcode results). Only positional args — no keyword args mixed in.
- **`kwargs`**: table with `__index` metatable that returns empty
  `pandoc.Inlines({})` for missing keys (not nil).
- **`meta`**: table with `__index` metatable that lazily reads document metadata.
- **`raw_args`**: flat list of raw argument strings.
- **`context`**: string `"block"`, `"inline"`, or `"text"`.

TS Quarto also provides `quarto.shortcode.read_arg(args, n)` (defined in
`quarto-cli/src/resources/pandoc/datadir/init.lua` ~line 1003):
```lua
quarto.shortcode.read_arg = function(args, n)
  local arg = args[n or 1]
  if arg == nil then return nil end
  if type(arg) ~= "string" then
    return inlinesToString(arg)
  else
    return arg
  end
end
```

Where `inlinesToString` (in `quarto-cli/src/resources/filters/common/pandoc.lua`
~line 76) wraps inlines in a `pandoc.Span` and calls `pandoc.utils.stringify`.

## Current q2 Behavior vs Expected

| Aspect | TS Quarto (expected) | q2 (current) |
|---|---|---|
| `args[1]` for `{{< sc 5 >}}` | `"5"` (plain string) | `{value = "5"}` (table) |
| `args` contains kwargs? | No | Yes (appended after positional) |
| `kwargs` missing key | Returns `pandoc.Inlines({})` | Returns `nil` |
| `pandoc.utils.stringify(args[1])` | `"5"` | `""` (table has no sequence items) |

### Why `stringify` fails on `{value = "5"}`
`pandoc.utils.stringify` (in `utils.rs:741`) handles tables by iterating
`sequence_values` (integer-keyed items). The table `{value = "5"}` has only a
string key `"value"`, so `sequence_values` yields nothing → empty string.

## Work Items

### Phase 1: Tests first (TDD)

- [x]**1.0** Write failing tests before any implementation changes. Add these
  tests to the `#[cfg(test)]` module in `shortcode.rs`:

  - **`test_args_are_plain_strings`**: Call a handler with
    `return pandoc.utils.stringify(args[1])` and positional arg `"5"`. Assert
    result is `"5"` (currently fails: returns `""`).
  - **`test_args_exclude_kwargs`**: Call a handler with both positional and
    keyword args. Handler returns `tostring(#args)` (arg count). Assert only
    positional args are counted (currently fails: kwargs are included).
  - **`test_kwargs_missing_key_returns_inlines`**: Call a handler that does
    `return tostring(type(kwargs["nonexistent"]))`. Assert result is
    `"userdata"` (Inlines are userdata in mlua). Currently fails: returns
    `"nil"`.
  - **`test_stringify_args_lipsum_pattern`**: Reproduce the exact lipsum failure
    pattern: handler does `local range = pandoc.utils.stringify(args[1])`
    followed by `range:find("^(%d+)$")`. Assert it matches. This is the
    end-to-end regression test.

  Run these tests and verify all four fail before proceeding.

### Phase 2: Implementation

- [x]**2.1** Fix `build_args_table` (~shortcode.rs:268): pass positional args
  as plain strings in a sequential table. Do NOT include keyword args.

  Current (broken):
  ```rust
  fn build_args_table(&self, args: &ShortcodeArgs) -> Result<Value> {
      let table = self.lua.create_table()?;
      let mut idx = 1;
      for arg in &args.positional {
          let entry = self.lua.create_table()?;
          entry.set("value", arg.as_str())?;  // WRONG: wraps in table
          table.set(idx, entry)?;
          idx += 1;
      }
      // Also appends keyword args here — WRONG
      for (key, val) in &args.keyword { ... }
      Ok(Value::Table(table))
  }
  ```

  Fixed:
  ```rust
  fn build_args_table(&self, args: &ShortcodeArgs) -> Result<Value> {
      let table = self.lua.create_table()?;
      for (i, arg) in args.positional.iter().enumerate() {
          table.set(i + 1, arg.as_str())?;  // plain string
      }
      Ok(Value::Table(table))
  }
  ```

- [x]**2.2** Fix `build_kwargs_table` (~shortcode.rs:287): add `__index`
  metatable that returns empty `pandoc.Inlines({})` for missing keys.

  **Truthiness note:** This changes missing-key behavior from `nil` (falsy) to
  empty `pandoc.Inlines({})` (truthy). This matches TS Quarto. Extensions like
  lipsum that do `if kwargs["random"] then` will now enter the branch for
  missing keys, but the subsequent `stringify` returns `""` which won't match
  `"true"`, so behavior is preserved. This is the intentional TS Quarto compat
  choice.

  Add after the keyword-population loop:
  ```rust
  let mt = self.lua.create_table()?;
  mt.set("__index", self.lua.load(
      "function(t, k) return pandoc.Inlines({}) end"
  ).eval::<Function>()?)?;
  table.set_metatable(Some(mt))?;
  ```

- [x]**2.3** Replace the existing Rust `read_arg` implementation with a Lua
  one that matches TS Quarto. The current Rust implementation
  (shortcode.rs:355-365) unwraps `{value=...}` tables — this is dead code
  after item 2.1 changes args to plain strings.

  **Replace** the existing `shortcode_ns.set("read_arg", ...)` block with:
  ```rust
  shortcode_ns.set(
      "read_arg",
      lua.load(r#"
          function(args, n)
              local arg = args[n or 1]
              if arg == nil then return nil end
              if type(arg) ~= "string" then
                  return pandoc.utils.stringify(arg)
              end
              return arg
          end
      "#).eval::<Function>()?,
  )?;
  ```

  Note: TS Quarto's `inlinesToString` wraps in `pandoc.Span` before
  stringifying. We use `pandoc.utils.stringify` directly, which is equivalent
  for all current cases (plain strings). When nested shortcodes are added and
  produce Inlines, this may need revisiting.

### Phase 3: Update existing tests

- [x]**3.1** Update `test_handler_receives_args` (shortcode.rs:691-721).
  Currently the Lua handler does `return args[1].value` which relies on the
  old `{value=...}` wrapping. Change to `return args[1]` since args are now
  plain strings.

- [x]**3.2** Verify `test_read_arg_helper` (shortcode.rs:874-905) still
  passes without changes. After the fix, `args[1]` is a plain string, so
  `read_arg(args, 1)` hits the string pass-through path. This test should
  pass as-is.

### Phase 4: Integration test

- [x]**4.1** Update the lipsum smoke-all test fixture at
  `crates/quarto/tests/smoke-all/extensions/lipsum-shortcode/test.qmd`.
  Currently asserts `ensureFileRegexMatches: ["Lorem ipsum dolor sit amet"]`
  which passes even with 5 paragraphs (the default when arg parsing fails).
  Tighten to verify exactly 1 paragraph is produced for `{{< lipsum 1 >}}`.
  For example, use `ensureFileRegexMatches` with a pattern that matches
  exactly one `<p>` tag, or add `ensureFileRegexDoesNotMatch` for a second
  `<p>` tag if the test harness supports it.

### Phase 5: Verification

- [x]**5.1** Run `cargo nextest run -p pampa` — all shortcode tests pass.
- [x]**5.2** Run `cargo nextest run -p quarto --test smoke_all` — lipsum
  integration test passes with tightened assertion.
- [x]**5.3** Run `cargo nextest run --workspace` — no regressions across
  the monorepo.

## Design Notes

### Why plain strings (not pandoc.Inlines)?

TS Quarto passes plain strings for simple shortcode params and pandoc.Inlines
for nested shortcode results. Since we don't yet support nested shortcodes,
plain strings are correct for all current cases. When nested shortcode support
is added, the result of the inner shortcode (which will be pandoc Inlines)
should be passed directly — the infrastructure for this can be added then.

### Why `pandoc.Inlines({})` default for kwargs?

TS Quarto's kwargs metatable returns empty Inlines for missing keys. This means
`pandoc.utils.stringify(kwargs["nonexistent"])` returns `""` rather than
erroring on nil. This changes truthiness: `kwargs["missing"]` is now truthy
(empty Inlines) instead of falsy (nil). This is the intentional TS Quarto
behavior. Extensions written for TS Quarto expect this, and well-written
extensions (like lipsum) handle it correctly because the subsequent stringify
returns `""` which doesn't match their expected values.

### `build_raw_args` is already correct

The existing `build_raw_args` function (~shortcode.rs:303) already produces a
flat list of plain strings, matching TS Quarto's `raw_args`.

### `build_meta_table` may need a metatable too

TS Quarto's `meta` uses `__index` to lazily call `readMetadata(i)`. Our current
`build_meta_table` eagerly populates from `args.metadata`. This works for now
but may need updating when full metadata integration is implemented. Out of
scope for this plan.

### Existing Rust `read_arg` must be replaced, not augmented

The current `read_arg` (shortcode.rs:355-365) is a Rust closure that unwraps
`{value=...}` tables. After item 2.1, args are plain strings, making this
unwrapping logic dead code. Item 2.3 replaces it entirely with a Lua function
matching the TS Quarto implementation.

## Files Touched

| File | Change |
|---|---|
| `crates/pampa/src/lua/shortcode.rs` | Fix `build_args_table`, `build_kwargs_table`, replace `read_arg` |
| `crates/quarto/tests/smoke-all/extensions/lipsum-shortcode/test.qmd` | Tighten assertion |

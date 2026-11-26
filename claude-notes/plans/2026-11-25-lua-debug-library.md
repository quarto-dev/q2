---
title: 'lua: load the debug library in native filters and shortcodes'
date: 2026-11-25
description: 'The debug global was nil in Lua filters and shortcode handlers, and the script chunk name lacked the standard file prefix. Native Lua states now load debug, and the chunks follow the Lua file-chunk naming convention.'
status: in-progress
---

**Braid:** bd-92k4o4ff
**Issue:** <https://github.com/quarto-dev/q2/issues/1012>

## Overview

Current behaviour: the native filter environment and the shortcode engine
build their Lua state with `Lua::new()`, which loads only mlua's safe
libraries and leaves `debug` out. A filter that calls `debug.getinfo` stops
the render with "attempt to index a nil value (global 'debug')". The filter
and shortcode script chunks are named with the bare path, so even with
`debug` loaded, `debug.getinfo(1, "S").source` would not carry the
standard leading `@`.

New behaviour: native Lua states load `StdLib::ALL_SAFE | StdLib::DEBUG`
through a shared `native_lua_state()` helper in
`crates/pampa/src/lua/mod.rs`, used by `create_filter_environment_with`
(`filter.rs`) and `LuaShortcodeEngine::new_with` (`shortcode.rs`). Both
script chunks are named `@<path>`, as `quarto_api.rs` already does for
required modules. Lua error messages now print `path:line:` instead of
`[string "path"]:line:`. The WASM branch keeps its restricted library set.

## Work items

- [x] Failing tests: `test_debug_getinfo_source_is_at_filter_path`
  (`filter_tests.rs`) and `test_shortcode_debug_getinfo_source`
  (`shortcode.rs`).
- [x] Load `debug` on native, through `native_lua_state()`.
- [x] Name filter and shortcode chunks `@<path>`.
- [x] `cargo nextest run -p pampa`, clippy (host and Windows), lint, verify.

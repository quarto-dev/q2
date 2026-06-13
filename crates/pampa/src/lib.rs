#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![allow(dead_code)]

/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 */

pub mod apply_node_edit;
pub mod attribution;
#[cfg(feature = "filters")]
pub mod citeproc_filter;
pub mod config_json;
pub mod errors;
pub mod filter_context;
pub mod filters;
#[cfg(feature = "json-filter")]
pub mod json_filter;
#[cfg(feature = "lua-filter")]
pub mod lua;
pub mod node_lookup;
pub mod options;
pub mod pandoc;
pub mod readers;
pub mod regenerate_nested_buffers;
pub mod template;
pub mod toc;
pub mod transforms;
pub mod traversals;
#[cfg(feature = "filters")]
pub mod unified_filter;
pub mod utils;
pub mod wasm_entry_points;
pub mod writers;

/// Async entry point for WASM: creates an async Lua function and calls it.
/// Used to validate that mlua's `async` feature works on wasm32-unknown-unknown.
/// Returns "ok:<result>" on success or "error:<msg>" on failure.
#[cfg(feature = "lua-filter")]
pub async fn lua_wasm_async_test() -> String {
    use mlua::StdLib;
    use mlua::prelude::*;

    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;

    let lua = match Lua::new_with(libs, mlua::LuaOptions::default()) {
        Ok(lua) => lua,
        Err(e) => return format!("error:Failed to create Lua state: {e}"),
    };

    // Register an async Rust function that resolves immediately with a known string.
    let async_fn =
        match lua.create_async_function(|_lua, ()| async move { Ok("async_result".to_string()) }) {
            Ok(f) => f,
            Err(e) => return format!("error:create_async_function failed: {e}"),
        };

    if let Err(e) = lua.globals().set("rust_async_fn", async_fn) {
        return format!("error:set global failed: {e}");
    }

    // Call the async function from Lua via call_async, then return the result.
    let chunk = lua.load("return rust_async_fn()");
    match chunk.eval_async::<String>().await {
        Ok(result) => format!("ok:{result}"),
        Err(e) => format!("error:eval_async failed: {e}"),
    }
}

#[cfg(feature = "lua-filter")]
pub fn lua_wasm_test(script: &str) -> String {
    use mlua::StdLib;
    use mlua::prelude::*;

    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;

    // Wrap the whole thing in catch_unwind to catch any panics from Lua C code
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let lua = match Lua::new_with(libs, mlua::LuaOptions::default()) {
            Ok(lua) => lua,
            Err(e) => return format!("Failed to create Lua state: {e}"),
        };

        match lua.load(script).eval::<String>() {
            Ok(result) => result,
            Err(e) => format!("Lua error: {e}"),
        }
    }));

    match result {
        Ok(s) => s,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "unknown panic".to_string()
            };
            format!("PANIC during Lua: {msg}")
        }
    }
}

#[cfg(all(test, feature = "lua-filter"))]
mod async_lua_tests {
    use super::lua_wasm_async_test;

    #[tokio::test]
    async fn test_mlua_async_feature() {
        let result = lua_wasm_async_test().await;
        assert_eq!(
            result, "ok:async_result",
            "mlua async feature failed: {result}"
        );
    }

    /// Test that pcall works with async functions (critical for placeholder.lua)
    #[tokio::test]
    async fn test_pcall_with_async_function() {
        use mlua::prelude::*;

        let lua = Lua::new();

        let async_fn = lua
            .create_async_function(|_lua, ()| async move { Ok(("hello".to_string(), 42i32)) })
            .unwrap();
        lua.globals().set("my_async_fn", async_fn).unwrap();

        // Direct async call
        let result: String = lua
            .load("local a, b = my_async_fn(); return a .. tostring(b)")
            .eval_async()
            .await
            .unwrap();
        assert_eq!(result, "hello42");

        // pcall wrapping async function directly
        let result: String = lua
            .load(
                r#"
                local ok, a, b = pcall(my_async_fn)
                if ok then return "ok:" .. a .. tostring(b)
                else return "fail:" .. tostring(a) end
            "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert_eq!(result, "ok:hello42", "pcall direct: {result}");

        // pcall wrapping a closure that calls async (placeholder.lua pattern)
        let result: String = lua
            .load(
                r#"
                local ok, a, b = pcall(function()
                    local a, b = my_async_fn()
                    return a, b
                end)
                if ok then return "ok:" .. tostring(a) .. tostring(b)
                else return "fail:" .. tostring(a) end
            "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert_eq!(result, "ok:hello42", "pcall closure: {result}");
    }
}

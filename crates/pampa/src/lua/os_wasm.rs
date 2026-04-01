/*
 * os_wasm.rs
 *
 * Synthetic `os` table for WASM and test environments.
 * Provides os.time(), os.clock(), and os.difftime() backed by SystemRuntime.
 */

use std::sync::Arc;

use mlua::{Lua, Result};

use super::runtime::SystemRuntime;

/// Register a synthetic `os` global table with time-related functions.
///
/// This replaces the C-backed os library (which requires symbols absent from
/// the WASM sysroot) with a minimal set of functions backed by SystemRuntime.
pub fn register_wasm_os(lua: &Lua, runtime: Arc<dyn SystemRuntime>) -> Result<()> {
    let os = lua.create_table()?;

    // os.time() — current Unix timestamp as integer
    let rt = runtime.clone();
    os.set(
        "time",
        lua.create_function(move |_, ()| {
            rt.unix_timestamp()
                .map(|t| t as i64)
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // os.clock() — CPU time in seconds (no meaningful value in browser)
    os.set("clock", lua.create_function(|_, ()| Ok(0.0f64))?)?;

    // os.difftime(t2, t1) — difference in seconds (pure arithmetic)
    os.set(
        "difftime",
        lua.create_function(|_, (t2, t1): (f64, f64)| Ok(t2 - t1))?,
    )?;

    lua.globals().set("os", os)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::runtime::NativeRuntime;
    use mlua::StdLib;

    fn test_lua() -> (Lua, Arc<dyn SystemRuntime>) {
        let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
        let lua = Lua::new_with(libs, mlua::LuaOptions::default()).unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        register_wasm_os(&lua, runtime.clone()).unwrap();
        (lua, runtime)
    }

    #[test]
    fn test_os_time_returns_positive_integer() {
        let (lua, _) = test_lua();
        let result: i64 = lua.load("return os.time()").eval().unwrap();
        assert!(
            result > 0,
            "os.time() should return a positive integer, got {}",
            result
        );
    }

    #[test]
    fn test_os_clock_returns_number() {
        let (lua, _) = test_lua();
        let result: f64 = lua.load("return os.clock()").eval().unwrap();
        assert_eq!(
            result, 0.0,
            "os.clock() should return 0.0 in synthetic mode"
        );
    }

    #[test]
    fn test_os_difftime() {
        let (lua, _) = test_lua();
        let result: f64 = lua.load("return os.difftime(10, 3)").eval().unwrap();
        assert_eq!(result, 7.0);
    }
}

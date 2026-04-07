//! WASM integration tests for Lua filter and shortcode infrastructure.
//!
//! These tests verify that the restricted Lua stdlib setup, synthetic io/os
//! modules, and filter/shortcode execution work correctly when compiled to
//! the real wasm32 target.
//!
//! **When to add tests here:** Only when modifying WASM-specific code paths:
//! - The #[cfg(target_arch = "wasm32")] blocks in filter.rs / shortcode.rs
//! - io_wasm.rs (synthetic io module)
//! - os_wasm.rs (synthetic os module)
//!
//! Native filter logic is tested comprehensively by the existing native tests.
//! These WASM tests are smoke tests of the target-specific setup.
//!
//! **How to run:** (Linux/macOS only, requires nightly + Clang + wasm-sysroot)
//! ```text
//! CC_wasm32_unknown_unknown=clang \
//! CFLAGS_wasm32_unknown_unknown="-isystem $PWD/crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
//! cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
//!   --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind,panic_abort
//! ```

#![cfg(all(target_arch = "wasm32", feature = "lua-filter"))]

use wasm_bindgen_test::*;

// ============================================================================
// Test 1: Restricted Lua VM creation
// ============================================================================

/// Verify that a restricted Lua VM (matching the wasm32 stdlib set) can be
/// created and can evaluate basic expressions.
#[wasm_bindgen_test]
fn restricted_lua_vm_creation() {
    use mlua::{Lua, StdLib};

    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
    let lua =
        Lua::new_with(libs, mlua::LuaOptions::default()).expect("Failed to create restricted Lua");

    let result: i64 = lua.load("return 1 + 1").eval().expect("eval failed");
    assert_eq!(result, 2);

    // Verify string library is available
    let upper: String = lua
        .load(r#"return string.upper("hello")"#)
        .eval()
        .expect("string.upper failed");
    assert_eq!(upper, "HELLO");
}

// ============================================================================
// Test 2: Filter execution through WASM code path
// ============================================================================

/// Run a Lua filter through the real WASM code path: restricted VM + synthetic
/// io/os + filter execution. Uses WasmRuntime with a filter file in the VFS.
#[wasm_bindgen_test]
async fn filter_execution_wasm() {
    use pampa::lua::apply_lua_filters;
    use pampa::lua::runtime::{VirtualFileSystem, WasmRuntime};
    use pampa::pandoc::{ASTContext, Block, Inline, Pandoc, Paragraph, Str};
    use std::path::PathBuf;
    use std::sync::Arc;

    // Set up VFS with a simple uppercase filter
    let mut vfs = VirtualFileSystem::new();
    vfs.add_file(
        std::path::Path::new("/project/uppercase.lua"),
        br#"
function Str(elem)
    return pandoc.Str(elem.text:upper())
end
"#
        .to_vec(),
    );

    let runtime: Arc<dyn pampa::lua::runtime::SystemRuntime> = Arc::new(WasmRuntime::with_vfs(vfs));

    // Build a minimal Pandoc document with one paragraph containing "hello"
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: quarto_source_map::SourceInfo::default(),
            })],
            source_info: quarto_source_map::SourceInfo::default(),
        })],
    };
    let context = ASTContext::new();

    let output = apply_lua_filters(
        pandoc,
        context,
        &[PathBuf::from("/project/uppercase.lua")],
        "html",
        runtime,
    )
    .await
    .expect("filter execution failed");

    assert!(
        output.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        output.diagnostics
    );

    // Verify the filter uppercased the text
    match &output.pandoc.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "HELLO"),
            other => panic!("Expected Str, got {other:?}"),
        },
        other => panic!("Expected Paragraph, got {other:?}"),
    }
}

// ============================================================================
// Test 3: Shortcode engine initialization on WASM
// ============================================================================

/// Verify that LuaShortcodeEngine::new() succeeds on WASM (creates restricted
/// VM, registers synthetic io/os, sets up pandoc/quarto namespaces).
#[wasm_bindgen_test]
fn shortcode_engine_init_wasm() {
    use pampa::lua::LuaShortcodeEngine;
    use pampa::lua::runtime::{VirtualFileSystem, WasmRuntime};
    use std::sync::Arc;

    let runtime: Arc<dyn pampa::lua::runtime::SystemRuntime> =
        Arc::new(WasmRuntime::with_vfs(VirtualFileSystem::new()));

    let _engine =
        LuaShortcodeEngine::new("html", runtime).expect("shortcode engine creation failed");
}

// ============================================================================
// Test 4: Error handling (panic_unwind works)
// ============================================================================

/// Verify that Lua errors produce Err results rather than WASM traps.
/// This validates that -Zbuild-std=std,panic_unwind,panic_abort is working correctly.
#[wasm_bindgen_test]
fn lua_error_handling() {
    use mlua::{Lua, StdLib};

    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
    let lua =
        Lua::new_with(libs, mlua::LuaOptions::default()).expect("Failed to create restricted Lua");

    let result = lua.load("error('test error')").exec();
    assert!(result.is_err(), "expected Lua error, got Ok");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("test error"),
        "error message should contain 'test error', got: {err_msg}"
    );
}

// ============================================================================
// Test 5: Synthetic io module is registered in filter execution
// ============================================================================

/// Verify that the synthetic io module (io.open, io.type) is registered when
/// filters execute on wasm32. Uses a filter that asserts these globals exist.
#[wasm_bindgen_test]
async fn synthetic_io_available_in_filters() {
    use pampa::lua::apply_lua_filters;
    use pampa::lua::runtime::{VirtualFileSystem, WasmRuntime};
    use pampa::pandoc::{ASTContext, Block, Inline, Pandoc, Paragraph, Str};
    use std::path::PathBuf;
    use std::sync::Arc;

    let mut vfs = VirtualFileSystem::new();
    vfs.add_file(
        std::path::Path::new("/project/check_io.lua"),
        br#"
function Pandoc(doc)
    assert(type(io) == "table", "io should be a table")
    assert(type(io.open) == "function", "io.open should be a function")
    assert(type(io.type) == "function", "io.type should be a function")
    return doc
end
"#
        .to_vec(),
    );

    let runtime: Arc<dyn pampa::lua::runtime::SystemRuntime> = Arc::new(WasmRuntime::with_vfs(vfs));

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "test".to_string(),
                source_info: quarto_source_map::SourceInfo::default(),
            })],
            source_info: quarto_source_map::SourceInfo::default(),
        })],
    };

    let output = apply_lua_filters(
        pandoc,
        ASTContext::new(),
        &[PathBuf::from("/project/check_io.lua")],
        "html",
        runtime,
    )
    .await
    .expect("filter with io checks failed — synthetic io may not be registered");

    assert!(
        output.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        output.diagnostics
    );
}

// ============================================================================
// Test 6: Synthetic os module is registered in filter execution
// ============================================================================

/// Verify that the synthetic os module (os.time, os.clock, os.difftime) is
/// registered when filters execute on wasm32.
#[wasm_bindgen_test]
async fn synthetic_os_available_in_filters() {
    use pampa::lua::apply_lua_filters;
    use pampa::lua::runtime::{VirtualFileSystem, WasmRuntime};
    use pampa::pandoc::{ASTContext, Block, Inline, Pandoc, Paragraph, Str};
    use std::path::PathBuf;
    use std::sync::Arc;

    let mut vfs = VirtualFileSystem::new();
    vfs.add_file(
        std::path::Path::new("/project/check_os.lua"),
        br#"
function Pandoc(doc)
    assert(type(os) == "table", "os should be a table")
    assert(type(os.time) == "function", "os.time should be a function")
    assert(type(os.clock) == "function", "os.clock should be a function")
    assert(type(os.difftime) == "function", "os.difftime should be a function")
    return doc
end
"#
        .to_vec(),
    );

    let runtime: Arc<dyn pampa::lua::runtime::SystemRuntime> = Arc::new(WasmRuntime::with_vfs(vfs));

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "test".to_string(),
                source_info: quarto_source_map::SourceInfo::default(),
            })],
            source_info: quarto_source_map::SourceInfo::default(),
        })],
    };

    let output = apply_lua_filters(
        pandoc,
        ASTContext::new(),
        &[PathBuf::from("/project/check_os.lua")],
        "html",
        runtime,
    )
    .await
    .expect("filter with os checks failed — synthetic os may not be registered");

    assert!(
        output.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        output.diagnostics
    );
}

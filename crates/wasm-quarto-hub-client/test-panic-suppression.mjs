#!/usr/bin/env node
/**
 * Regression test for plan 2026-04-16-suppress-lua-panic-noise:
 *
 * Lua's LUAI_THROW is replaced in WASM by rust_lua_throw() which panics.
 * The panic is caught by rust_lua_protected_call — this is expected control
 * flow (happens on every pcall-caught error), NOT a bug.
 *
 * However, console_error_panic_hook prints every panic's stack trace to
 * console.error. This test captures console.error while triggering Lua
 * errors that are successfully caught by pcall, and fails if any of the
 * captured output contains "lua error" panic noise.
 *
 * Before the fix: FAILS (console.error is spammed with stack traces).
 * After the fix: PASSES (custom panic hook filters LuaThrow sentinel).
 *
 * Expected failure patterns the test looks for:
 *   panicked at src/c_shim.rs:<line>:5:
 *   lua error
 *   rust_lua_throw
 */

import { readFile, writeFile, unlink } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pkgDir = join(__dirname, 'pkg');

// Patch the generated JS (same technique as test-lua-wasm.mjs).
let jsSource = await readFile(join(pkgDir, 'wasm_quarto_hub_client.js'), 'utf-8');
jsSource = jsSource.replace(/^import .+ from ['"]\/src\/wasm-js-bridge\/[^'"]+['"];?\s*$/gm, '');
jsSource = jsSource.replace(/^import \* as \w+ from ['"]\/src\/wasm-js-bridge\/[^'"]+['"];?\s*$/gm, '');
jsSource = jsSource.replace(/^const .+ = require\(.*\/src\/wasm-js-bridge\/[^)]+\);?\s*$/gm, '');
const stubs = `
function jsCacheClearNamespace() { return undefined; }
function jsCacheDelete() { return undefined; }
function jsCacheGet() { return null; }
function jsCacheSet() { return undefined; }
function jsCompileSass() { return ''; }
function jsRenderEjs() { return ''; }
function jsRenderSimpleTemplate() { return ''; }
function jsTemplateAvailable() { return false; }
function jsSassAvailable() { return false; }
const import1 = { jsRenderEjs, jsRenderSimpleTemplate, jsTemplateAvailable };
const import2 = { jsCompileSass, jsSassAvailable };
`;
jsSource = stubs + jsSource;

const tmpFile = join(__dirname, '_test_panic_suppression_patched.mjs');
await writeFile(tmpFile, jsSource);

// Capture console.error calls into a buffer BEFORE we initialize the
// module. The wasm-bindgen init installs the panic hook inside the
// start function, so we need our capture in place before that runs.
const capturedErrors = [];
const origError = console.error;
console.error = (...args) => {
  // Join the args to a single searchable string. The panic hook calls
  // console.error(stackTrace) where stackTrace is a single string, but
  // be defensive in case multi-arg forms appear.
  const msg = args.map(a => (typeof a === 'string' ? a : String(a))).join(' ');
  capturedErrors.push(msg);
  // Do NOT forward to origError — we want clean test output. If there
  // are real errors, we'll surface them from the assertions below.
};

try {
  const mod = await import(tmpFile);
  const wasmBytes = await readFile(join(pkgDir, 'wasm_quarto_hub_client_bg.wasm'));
  const wasmModule = await WebAssembly.compile(wasmBytes);
  await mod.default(wasmModule);

  // Trigger several Lua scripts that error but are caught by pcall.
  // Each one should flow through rust_lua_throw → rust_lua_protected_call.
  // If the panic hook is not filtering, each will leave a stack trace
  // in capturedErrors.
  const scripts = [
    `local ok, err = pcall(function() error("boom") end)
     return tostring(ok) .. " " .. tostring(err):match("boom")`,
    `local ok, err = pcall(function() error({code=1, msg="structured"}) end)
     return tostring(ok)`,
    `local ok, err = pcall(function()
       local t = nil
       return t.x
     end)
     return tostring(ok)`,
    `local ok, err = pcall(function() error("e1") end)
     local ok2, err2 = pcall(function() error("e2") end)
     local ok3, err3 = pcall(function() error("e3") end)
     return tostring(ok) .. tostring(ok2) .. tostring(ok3)`,
  ];

  for (const script of scripts) {
    const result = mod.test_lua(script);
    // Sanity: each script should return a string (pcall caught the error).
    if (typeof result !== 'string') {
      console.error = origError;
      console.error(`FAIL: expected string result from test_lua, got ${typeof result}: ${result}`);
      process.exit(1);
    }
  }

  // Restore before reporting.
  console.error = origError;

  // Assertion: no captured error should contain "lua error" panic noise.
  const noisePatterns = [
    /panicked at.*c_shim\.rs/,
    /^lua error$/m,
    /rust_lua_throw/,
  ];
  const offending = [];
  for (const msg of capturedErrors) {
    for (const pattern of noisePatterns) {
      if (pattern.test(msg)) {
        offending.push({ pattern: pattern.toString(), msg });
        break;
      }
    }
  }

  if (offending.length > 0) {
    console.error = origError;
    console.error(`FAIL: ${offending.length} captured console.error call(s) contain Lua-panic noise.`);
    console.error(`Total console.error calls during pcall tests: ${capturedErrors.length}`);
    console.error('');
    console.error('Offending entries:');
    for (const { pattern, msg } of offending.slice(0, 3)) {
      console.error(`  matched ${pattern}:`);
      console.error(`    ${msg.split('\n').slice(0, 4).join('\n    ')}`);
      console.error('');
    }
    if (offending.length > 3) {
      console.error(`  ...and ${offending.length - 3} more`);
    }
    process.exit(1);
  }

  console.log(`PASS part 1: no lua-panic noise in ${capturedErrors.length} captured console.error call(s)`);

  // Part 2: verify that genuine (non-Lua) panics STILL produce
  // console.error output. test_unwind() internally calls
  // panic!("test panic") and catches it via catch_unwind. The default
  // panic hook should still fire for this.
  capturedErrors.length = 0;
  console.error = (...args) => {
    const msg = args.map(a => (typeof a === 'string' ? a : String(a))).join(' ');
    capturedErrors.push(msg);
  };
  const unwindResult = mod.test_unwind();
  console.error = origError;

  if (!unwindResult.includes('caught panic')) {
    console.error(`FAIL part 2: test_unwind did not report catching the panic: ${unwindResult}`);
    process.exit(1);
  }

  const sawTestPanic = capturedErrors.some(m => m.includes('test panic'));
  if (!sawTestPanic) {
    console.error('FAIL part 2: real panic ("test panic") was suppressed — the filter is too broad.');
    console.error(`Captured ${capturedErrors.length} console.error call(s):`);
    for (const m of capturedErrors.slice(0, 3)) {
      console.error(`  ${m.slice(0, 200)}`);
    }
    process.exit(1);
  }

  console.log(`PASS part 2: real panic still surfaces via console.error (${capturedErrors.length} call(s), "test panic" present)`);
  process.exit(0);
} finally {
  await unlink(tmpFile).catch(() => {});
}

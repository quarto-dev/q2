#!/usr/bin/env node
/**
 * Build the WASM module for hub-client
 *
 * Uses `cargo build --target wasm32-unknown-unknown` + `wasm-bindgen` CLI
 * instead of wasm-pack, because we need `-Zbuild-std=std,panic_unwind` for
 * Lua's error handling (panic/catch_unwind replacing setjmp/longjmp).
 *
 * Requirements:
 *   - Nightly Rust with rust-src: `rustup component add rust-src`
 *   - wasm-bindgen CLI: `cargo install wasm-bindgen-cli`
 *   - Homebrew LLVM (macOS): `brew install llvm`
 */

import { spawn, spawnSync } from 'child_process';
import { existsSync, mkdirSync, readFileSync, rmSync } from 'fs';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';
import { platform } from 'os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, '../..');
const wasmCrate = join(repoRoot, 'crates', 'wasm-quarto-hub-client');

function findLlvmClang() {
  if (platform() === 'darwin') {
    const locations = [
      '/opt/homebrew/opt/llvm/bin/clang',  // Apple Silicon
      '/usr/local/opt/llvm/bin/clang',      // Intel
    ];
    for (const loc of locations) {
      if (existsSync(loc)) return loc;
    }
    console.error('Error: Homebrew LLVM not found.');
    console.error('Apple clang does not support wasm32-unknown-unknown.');
    console.error('Install LLVM with: brew install llvm');
    process.exit(1);
  }
  // On Linux, system clang typically supports wasm32
  return 'clang';
}

function run(cmd, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const isWindows = platform() === 'win32';
    console.log(`  $ ${cmd} ${args.join(' ')}`);
    const child = spawn(cmd, args, {
      stdio: 'inherit',
      shell: isWindows,
      ...opts,
    });
    child.on('error', reject);
    child.on('close', (code) => {
      if (code === 0) resolve();
      else reject(new Error(`${cmd} exited with code ${code}`));
    });
  });
}

/**
 * Read the wasm-bindgen version locked in the wasm crate's Cargo.lock.
 * Returns a string like "0.2.108", or null if it can't be determined.
 */
function lockedWasmBindgenVersion() {
  try {
    const lockPath = join(wasmCrate, 'Cargo.lock');
    const text = readFileSync(lockPath, 'utf-8');
    const match = text.match(/name = "wasm-bindgen"\nversion = "([^"]+)"/);
    return match ? match[1] : null;
  } catch {
    return null;
  }
}

/**
 * Verify that the installed wasm-bindgen CLI matches the version locked in
 * Cargo.lock.  The versions must match exactly — a mismatch causes a hard
 * build error ("schema version … must exactly match").
 */
function checkWasmBindgenVersion() {
  const expected = lockedWasmBindgenVersion();
  if (!expected) {
    console.warn('Warning: could not read wasm-bindgen version from Cargo.lock');
    return;
  }

  const result = spawnSync('wasm-bindgen', ['--version'], { encoding: 'utf-8' });
  if (result.error || result.status !== 0) {
    console.error('Error: wasm-bindgen CLI not found.');
    console.error(`Install the pinned version with:`);
    console.error(`  cargo install -f wasm-bindgen-cli --version ${expected}`);
    console.error('Or run: cargo dev-setup');
    process.exit(1);
  }

  const installed = (result.stdout || result.stderr || '').trim();
  if (!installed.includes(expected)) {
    console.error(`Error: wasm-bindgen version mismatch.`);
    console.error(`  installed: ${installed}`);
    console.error(`  required:  wasm-bindgen ${expected}  (from Cargo.lock)`);
    console.error('');
    console.error('The CLI version must exactly match the wasm-bindgen crate version.');
    console.error(`Fix with:`);
    console.error(`  cargo install -f wasm-bindgen-cli --version ${expected}`);
    console.error('Or run: cargo dev-setup');
    process.exit(1);
  }
}

async function buildWasm() {
  checkWasmBindgenVersion();
  console.log('Building wasm-quarto-hub-client...\n');

  // Clean pkg/ directory
  const pkgDir = join(wasmCrate, 'pkg');
  if (existsSync(pkgDir)) {
    console.log('Cleaning existing pkg/ directory...');
    rmSync(pkgDir, { recursive: true });
  }
  mkdirSync(pkgDir, { recursive: true });

  // Environment for cargo build
  const env = { ...process.env };

  // Point CC to Homebrew LLVM clang (Apple clang can't target wasm32)
  const clang = findLlvmClang();
  env.CC_wasm32_unknown_unknown = clang;

  // Provide stub sysroot headers for all C dependencies (tree-sitter, lua-src, etc.)
  const wasmSysroot = join(wasmCrate, 'wasm-sysroot');
  env.CFLAGS_wasm32_unknown_unknown = `-isystem ${wasmSysroot}`;

  // Step 1: cargo build (uses .cargo/config.toml for -Zbuild-std and rustflags)
  console.log('Step 1/2: cargo build --target wasm32-unknown-unknown --release');
  await run('cargo', [
    'build',
    '--target', 'wasm32-unknown-unknown',
    '--release',
  ], { cwd: wasmCrate, env });

  // Step 2: wasm-bindgen to generate JS glue
  const wasmFile = join(
    wasmCrate,
    'target', 'wasm32-unknown-unknown', 'release',
    'wasm_quarto_hub_client.wasm',
  );

  console.log('\nStep 2/2: wasm-bindgen --target web');
  await run('wasm-bindgen', [
    '--target', 'web',
    '--out-dir', pkgDir,
    wasmFile,
  ]);

  console.log(`\nWASM build complete: ${pkgDir}/`);
}

// Main
buildWasm()
  .then(() => process.exit(0))
  .catch((err) => {
    console.error('\nBuild failed:', err.message);
    process.exit(1);
  });

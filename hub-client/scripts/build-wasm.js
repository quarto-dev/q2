#!/usr/bin/env node
/**
 * Build the WASM module for hub-client
 *
 * This script builds wasm-quarto-hub-client with the proper environment
 * variables. Works on macOS, Linux, and Windows.
 */

import { spawn } from 'child_process';
import { existsSync } from 'fs';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';
import { platform } from 'os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, '../..');
const wasmCrate = join(repoRoot, 'crates', 'wasm-quarto-hub-client');

function findLlvmPath() {
  if (platform() === 'darwin') {
    // macOS: Check Homebrew locations
    const locations = [
      '/opt/homebrew/opt/llvm/bin',  // Apple Silicon
      '/usr/local/opt/llvm/bin',      // Intel
    ];
    for (const loc of locations) {
      if (existsSync(loc)) {
        return loc;
      }
    }
    console.warn('Warning: LLVM not found in Homebrew locations.');
    console.warn('You may need to install it: brew install llvm');
  }
  // On Linux/Windows, assume LLVM is in PATH or not needed
  return null;
}

function buildWasm() {
  return new Promise((resolve, reject) => {
    console.log('Building wasm-quarto-hub-client...');

    // Set up environment
    const env = { ...process.env };

    // Add LLVM to PATH if found
    const llvmPath = findLlvmPath();
    if (llvmPath) {
      env.PATH = `${llvmPath}${platform() === 'win32' ? ';' : ':'}${env.PATH}`;
    }

    // Set CFLAGS for wasm32 target (needed for tree-sitter C code)
    const wasmSysroot = join(wasmCrate, 'wasm-sysroot');
    const cflags = [
      `-I${wasmSysroot}`,
      '-Wbad-function-cast',
      '-Wcast-function-type',
      '-fno-builtin',
      '-DHAVE_ENDIAN_H',
    ].join(' ');
    env.CFLAGS_wasm32_unknown_unknown = cflags;

    // Determine the command based on platform
    const isWindows = platform() === 'win32';
    const cmd = isWindows ? 'wasm-pack.cmd' : 'wasm-pack';

    const args = ['build', '--target', 'web'];

    console.log(`Running: ${cmd} ${args.join(' ')}`);
    console.log(`Working directory: ${wasmCrate}`);

    const child = spawn(cmd, args, {
      cwd: wasmCrate,
      env,
      stdio: 'inherit',
      shell: isWindows,
    });

    child.on('error', (err) => {
      if (err.code === 'ENOENT') {
        console.error('Error: wasm-pack is not installed.');
        console.error('Install it with: cargo install wasm-pack');
      }
      reject(err);
    });

    child.on('close', (code) => {
      if (code === 0) {
        console.log(`\nWASM build complete: ${join(wasmCrate, 'pkg')}/`);
        resolve();
      } else {
        reject(new Error(`wasm-pack exited with code ${code}`));
      }
    });
  });
}

// Main
buildWasm()
  .then(() => process.exit(0))
  .catch((err) => {
    console.error('Build failed:', err.message);
    process.exit(1);
  });

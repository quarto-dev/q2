#!/usr/bin/env node
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pkgDir = join(__dirname, 'pkg');

const wasmBytes = await readFile(join(pkgDir, 'test_unwind_bg.wasm'));
const mod = await import(join(pkgDir, 'test_unwind.js'));
await mod.default(wasmBytes);

console.log('Test 1 - catch_unwind (no panic):');
try {
  console.log('  ' + mod.test_catch_unwind());
} catch (e) {
  console.log('  TRAP: ' + e.message);
}

console.log('Test 2 - catch_unwind (with panic):');
try {
  console.log('  ' + mod.test_catch_panic());
} catch (e) {
  console.log('  TRAP: ' + e.message);
}

console.log('Test 3 - catch through C-unwind:');
try {
  console.log('  ' + mod.test_catch_through_c_unwind());
} catch (e) {
  console.log('  TRAP: ' + e.message);
}

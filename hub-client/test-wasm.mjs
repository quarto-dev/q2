/**
 * WASM End-to-End Test Script
 *
 * Tests the wasm-quarto-hub-client module directly from Node.js.
 * This is the fastest way to verify WASM behavior without a browser.
 *
 * Usage:
 *   cd hub-client
 *   npm run build:wasm   # Build WASM first
 *   node test-wasm.mjs
 */

import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));

// Load the WASM module from the same location vite uses (hub-client/wasm-quarto-hub-client/)
const pkgDir = join(__dirname, 'wasm-quarto-hub-client');
const wasmPath = join(pkgDir, 'wasm_quarto_hub_client_bg.wasm');
const wasm = await import(join(pkgDir, 'wasm_quarto_hub_client.js'));
const wasmBytes = await readFile(wasmPath);
await wasm.default(wasmBytes);

console.log('WASM module loaded successfully\n');

// Test content
const testContent = `---
title: Test Document
---

# Hello World

This is a paragraph.

## Section Two

Another paragraph here.
`;

// =============================================================================
// Test 1: Basic render (without source location)
// =============================================================================
console.log('=== Test 1: Basic render (no source location) ===');
const result1 = JSON.parse(wasm.render_qmd_content(testContent, ''));
console.log('Success:', result1.success);
if (result1.success) {
  const hasDataLoc = result1.html.includes('data-loc');
  console.log('Has data-loc attributes:', hasDataLoc);
  if (hasDataLoc) {
    console.log('WARNING: data-loc present without sourceLocation option');
  }
} else {
  console.log('Error:', result1.error);
}
console.log('');

// =============================================================================
// Test 2: Render with source location enabled
// =============================================================================
console.log('=== Test 2: Render with source location enabled ===');
const options = JSON.stringify({ source_location: true });
console.log('Options:', options);

// Check if the function exists
if (typeof wasm.render_qmd_content_with_options !== 'function') {
  console.error('FAIL: render_qmd_content_with_options is not exported from WASM');
  console.log('Available exports:', Object.keys(wasm).filter(k => typeof wasm[k] === 'function'));
  process.exit(1);
}

const result2 = JSON.parse(wasm.render_qmd_content_with_options(testContent, '', options));
console.log('Success:', result2.success);

if (result2.success) {
  const hasDataLoc = result2.html.includes('data-loc');
  console.log('Has data-loc attributes:', hasDataLoc);

  if (hasDataLoc) {
    // Extract and show some data-loc values
    const matches = result2.html.match(/data-loc="[^"]+"/g);
    console.log('Sample data-loc attributes:', matches?.slice(0, 5));
    console.log('\nPASS: Source location tracking is working!');
  } else {
    console.log('\nFAIL: Expected data-loc attributes in HTML but found none');
    console.log('\nHTML output (first 2000 chars):');
    console.log(result2.html.substring(0, 2000));
    process.exit(1);
  }
} else {
  console.log('Error:', result2.error);
  console.log('Diagnostics:', result2.diagnostics);
  process.exit(1);
}

console.log('\n=== All tests passed ===');

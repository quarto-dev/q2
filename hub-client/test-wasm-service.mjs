/**
 * Test the TypeScript wasmRenderer service pattern
 *
 * This tests the same flow as the React app uses, but in Node.js
 */

import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));

// Load WASM like vite does (from wasm-quarto-hub-client/)
const pkgDir = join(__dirname, 'wasm-quarto-hub-client');
const wasmPath = join(pkgDir, 'wasm_quarto_hub_client_bg.wasm');
const wasm = await import(join(pkgDir, 'wasm_quarto_hub_client.js'));
const wasmBytes = await readFile(wasmPath);
await wasm.default(wasmBytes);

console.log('WASM module loaded\n');

// Mimic the TypeScript service pattern
function getWasm() {
  return wasm;
}

function renderQmdContentWithOptions(content, templateBundle = '', options = {}) {
  const wasmModule = getWasm();
  const optionsJson = JSON.stringify({
    source_location: options.sourceLocation ?? false,
  });
  console.log('optionsJson:', optionsJson);
  return JSON.parse(wasmModule.render_qmd_content_with_options(content, templateBundle, optionsJson));
}

async function renderToHtml(qmdContent, options = {}) {
  console.log('[renderToHtml] sourceLocation option:', options.sourceLocation);

  const result = options.sourceLocation
    ? renderQmdContentWithOptions(qmdContent, '', { sourceLocation: options.sourceLocation })
    : JSON.parse(getWasm().render_qmd_content(qmdContent, ''));

  console.log('[renderToHtml] HTML has data-loc:', result.html?.includes('data-loc'));
  return result;
}

// Test content
const testContent = `---
title: Test
---

# Hello

Paragraph text.
`;

// Test 1: Without sourceLocation
console.log('=== Test 1: Without sourceLocation ===');
const result1 = await renderToHtml(testContent);
console.log('Success:', result1.success);
console.log('Has data-loc:', result1.html?.includes('data-loc'));
console.log('');

// Test 2: With sourceLocation = true
console.log('=== Test 2: With sourceLocation = true ===');
const result2 = await renderToHtml(testContent, { sourceLocation: true });
console.log('Success:', result2.success);
console.log('Has data-loc:', result2.html?.includes('data-loc'));

if (result2.html?.includes('data-loc')) {
  console.log('\nPASS: The TypeScript service pattern works correctly');
} else {
  console.log('\nFAIL: data-loc not found in HTML');
  console.log('HTML sample:', result2.html?.substring(0, 500));
  process.exit(1);
}

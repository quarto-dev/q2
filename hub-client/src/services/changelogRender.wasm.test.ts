/**
 * WASM End-to-End Tests for changelog and more-info rendering
 *
 * Verifies that the markdown files displayed in the About tab
 * render successfully through the QMD pipeline. This catches
 * syntax errors (e.g., unescaped underscores) before they reach
 * users as an "(unavailable)" changelog button.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  render_qmd_content: (content: string, templateBundle: string) => Promise<string>;
}

interface RenderResponse {
  success: boolean;
  html?: string;
  error?: string;
}

let wasm: WasmModule;

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);
});

describe('About tab markdown files render without errors', () => {
  it('changelog.md renders successfully', async () => {
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const content = await readFile(join(__dirname, '../../changelog.md'), 'utf-8');
    const result: RenderResponse = JSON.parse(await wasm.render_qmd_content(content, ''));
    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.html).toBeTruthy();
  });

  it('more-info.md renders successfully', async () => {
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const content = await readFile(join(__dirname, '../../resources/more-info.md'), 'utf-8');
    const result: RenderResponse = JSON.parse(await wasm.render_qmd_content(content, ''));
    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.html).toBeTruthy();
  });
});

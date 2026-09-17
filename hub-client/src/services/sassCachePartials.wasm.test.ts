/**
 * WASM end-to-end for bd-m3hga05o: the compiled-SCSS cache must notice
 * an edit to a partial the custom theme `@import`s.
 *
 * The cache key covers only the top-level theme file, so before the fix
 * a second render after editing `_colors.scss` served the previous
 * compile from the cache. Now the compile reports the VFS files
 * dart-sass loaded (`jsCompileSass` → `loadedUrls`), the stage stores
 * that list beside the CSS, and a lookup re-hashes the listed files.
 *
 * The bridge's `jsCompileSass` is spied on so the test can tell a cache
 * hit from a recompile: same inputs → no new compile; edited partial →
 * exactly one more. Runs the cache in its ephemeral in-memory mode (the
 * one `q2 preview` uses), since Node has no IndexedDB.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, vi } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { JSDOM } from 'jsdom';
import * as sassBridge from '/src/wasm-js-bridge/sass.js';

vi.mock('/src/wasm-js-bridge/sass.js', async (importOriginal) => {
  const original = await importOriginal<typeof import('/src/wasm-js-bridge/sass.js')>();
  return { ...original, jsCompileSass: vi.fn(original.jsCompileSass) };
});

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_read_file: (path: string) => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  render_qmd: (path: string) => Promise<string>;
}

interface RenderResponse {
  success: boolean;
  html?: string;
  error?: string;
}

let wasm: WasmModule;

function vfsRead(path: string): string | null {
  try {
    const result = JSON.parse(wasm.vfs_read_file(path)) as { success: boolean; content?: string };
    return result.success && result.content !== undefined ? result.content : null;
  } catch {
    return null;
  }
}

beforeAll(async () => {
  // The cache bridge's ephemeral mode (bd-91mdd056): a module-level Map
  // with the same semantics as IndexedDB, which Node does not have.
  (globalThis as { __Q2_EPHEMERAL_STORAGE__?: boolean }).__Q2_EPHEMERAL_STORAGE__ = true;

  const __dirname = dirname(fileURLToPath(import.meta.url));
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmBytes = await readFile(join(wasmDir, 'wasm_quarto_hub_client_bg.wasm'));
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);
  sassBridge.setVfsCallbacks(vfsRead, (path: string) => vfsRead(path) !== null);
});

/** The theme CSS the rendered page links, read back from the VFS. */
function linkedThemeCss(result: RenderResponse): string {
  expect(result.success, `Render failed: ${result.error}`).toBe(true);
  const dom = new JSDOM(result.html!);
  const hrefs = Array.from(dom.window.document.querySelectorAll('link[rel="stylesheet"]'))
    .map((l) => l.getAttribute('href') ?? '')
    .filter((h) => h.includes('quarto-theme-'));
  expect(hrefs, 'page links one fingerprinted theme CSS').toHaveLength(1);
  const href = hrefs[0];
  const css = vfsRead(href.startsWith('/') ? href : `/project/${href}`);
  expect(css, `theme CSS ${href} readable from VFS`).not.toBeNull();
  return css!;
}

async function render(): Promise<string> {
  const result: RenderResponse = JSON.parse(await wasm.render_qmd('/project/doc.qmd'));
  return linkedThemeCss(result);
}

describe('sass cache and @import\'ed partials (WASM)', () => {
  it('recompiles when an imported partial changes, and only then', async () => {
    const compiles = vi.mocked(sassBridge.jsCompileSass);
    wasm.vfs_clear();
    wasm.vfs_set_runtime_metadata('');
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Partials"\n');
    wasm.vfs_add_file(
      '/project/theme.scss',
      '/*-- scss:defaults --*/\n@import "colors";\n\n/*-- scss:rules --*/\n.partial-guard { color: $partial-fg; }\n',
    );
    wasm.vfs_add_file('/project/_colors.scss', '$partial-fg: #123457;\n');
    wasm.vfs_add_file('/project/doc.qmd', '---\ntheme: theme.scss\n---\n\n# Hello\n');

    const first = await render();
    expect(first).toContain('#123457');
    const afterFirst = compiles.mock.calls.length;
    expect(afterFirst).toBeGreaterThan(0);

    // Same inputs: served from the cache, nothing compiled.
    const again = await render();
    expect(again).toBe(first);
    expect(compiles.mock.calls.length, 'unchanged inputs hit the cache').toBe(afterFirst);

    // Edit only the partial: the stored manifest no longer matches, so
    // the theme compiles once more and the new colour comes through.
    wasm.vfs_add_file('/project/_colors.scss', '$partial-fg: #abcdef;\n');
    const edited = await render();
    expect(edited).toContain('#abcdef');
    expect(edited).not.toContain('#123457');
    expect(compiles.mock.calls.length, 'the partial edit forces one recompile').toBe(
      afterFirst + 1,
    );

    // And the recompiled entry is cached in turn.
    const warm = await render();
    expect(warm).toBe(edited);
    expect(compiles.mock.calls.length).toBe(afterFirst + 1);
  });
});

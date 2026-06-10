/**
 * WASM End-to-End Tests for the artifact-flush change-detection
 * (bd-q3bxnq2e).
 *
 * The render tail flushes every produced artifact into the session VFS
 * on every render; since bd-q3bxnq2e, byte-identical re-writes are
 * skipped (quarto_core::artifact_flush::flush_artifacts_to_vfs +
 * VirtualFileSystem::add_file_if_changed). These tests pin the
 * observable contract of the keystroke steady state — repeated renders
 * of an unchanged document — through the real WASM module:
 *
 *  1. repeated renders succeed and produce identical HTML;
 *  2. the CSS artifact the HTML links stays readable from the VFS
 *     after a fully-skipped flush (the iframe post-processor's
 *     read-back contract, Phase 9);
 *  3. an edited document still re-renders correctly (changed page,
 *     unchanged CSS artifact).
 *
 * Environment note: in this Node/vitest environment the bootswatch
 * theme sources are not in the VFS, so a `theme:` doc falls back to
 * the default CSS bundle at `/.quarto/project-artifacts/styles.css` —
 * which is exactly the artifact these tests read back. The
 * changed-artifact re-write path (new theme → new fingerprinted path)
 * is covered natively by quarto-core's artifact_flush unit tests
 * (`changed_artifact_rewritten_unchanged_skipped`).
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_read_file: (path: string) => string;
  render_qmd: (path: string) => Promise<string>;
}

interface VfsResponse {
  success: boolean;
  error?: string;
  content?: string | null;
}

interface RenderResponse {
  success: boolean;
  html?: string;
  error?: string;
  theme_fingerprint?: string;
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

beforeEach(() => {
  wasm.vfs_clear();
});

const CSS_ARTIFACT_PATH = '/.quarto/project-artifacts/styles.css';

function readCssArtifact(): VfsResponse {
  return JSON.parse(wasm.vfs_read_file(CSS_ARTIFACT_PATH));
}

describe('artifact flush change-detection (bd-q3bxnq2e)', () => {
  it('repeated renders of an unchanged doc produce identical HTML and keep the CSS artifact readable', async () => {
    wasm.vfs_add_file(
      '/project/doc.qmd',
      '---\ntitle: Steady\n---\n\n# Heading\n\nBody text.\n'
    );

    const render1: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(render1.success).toBe(true);
    expect(render1.html).toContain(CSS_ARTIFACT_PATH);

    const cssAfterFirst = readCssArtifact();
    expect(cssAfterFirst.success).toBe(true);
    expect(cssAfterFirst.content!.length).toBeGreaterThan(1000);

    // Steady state: nothing changed, the flush is all skips.
    const render2: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(render2.success).toBe(true);
    expect(render2.html).toBe(render1.html);

    // Read-back contract: the CSS the HTML links is still served out
    // of the VFS, byte-identical, after the fully-skipped flush.
    const cssAfterSecond = readCssArtifact();
    expect(cssAfterSecond.success).toBe(true);
    expect(cssAfterSecond.content).toBe(cssAfterFirst.content);
  });

  it('an edited doc re-renders with new HTML while the unchanged CSS artifact stays intact', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '# One\n\nFirst body.\n');
    const render1: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(render1.success).toBe(true);
    const cssAfterFirst = readCssArtifact();
    expect(cssAfterFirst.success).toBe(true);

    // Keystroke edit: page content changes, CSS artifact does not.
    wasm.vfs_add_file('/project/doc.qmd', '# One\n\nFirst body, edited.\n');
    const render2: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(render2.success).toBe(true);
    expect(render2.html).not.toBe(render1.html);
    expect(render2.html).toContain('edited');

    const cssAfterSecond = readCssArtifact();
    expect(cssAfterSecond.success).toBe(true);
    expect(cssAfterSecond.content).toBe(cssAfterFirst.content);
  });
});

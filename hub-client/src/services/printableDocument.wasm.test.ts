/**
 * End-to-end WASM test for the "Open printable version" feature
 * (issue #315, bd-vhdknrvl).
 *
 * Drives the **real** `render_printable` WASM export against a VFS
 * project, then runs the JS `makeSelfContainedHtml` inliner over the
 * result — the exact pipeline `openPrintableDocument` runs, minus the
 * `window.open`. Proves, against the actual engine:
 *
 *  - a `format: q2-preview` document renders to a **full HTML page**
 *    (not preview AST), with its relative image `src` preserved, and
 *  - after inlining, the document is **self-contained** (no
 *    `/.quarto/…` `<link>`/`<script>` refs survive; the user image is
 *    embedded as a `data:` URI), and
 *  - a `format: revealjs` document renders to a **standalone deck**
 *    that `forceRevealPrintMode` puts into reveal's print layout.
 *
 * Mirrors the WASM-load pattern in `assetManifestProject.wasm.test.ts`.
 * A jsdom `DOMParser` is installed globally because the wasm test env
 * is `node` (no DOM) but `makeSelfContainedHtml` parses HTML.
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { JSDOM } from 'jsdom';
import {
  initWasm,
  vfsAddFile,
  vfsAddBinaryFile,
  vfsClear,
  vfsReadFile,
  vfsReadBinaryFile,
} from '@quarto/preview-runtime';
import { type SelfContainedReaders } from '@quarto/preview-renderer/utils/makeSelfContainedHtml';
import { buildPrintableHtml } from './printableDocument';

interface RenderResponse {
  success: boolean;
  error?: string;
  html?: string;
  ast_json?: string;
}

let renderPrintableRaw: (path: string) => Promise<string>;

// Minimal 1×1 red PNG (same fixture bytes used across the wasm tests).
const PNG_BYTES = new Uint8Array(
  Buffer.from([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d,
    0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00,
    0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0x99, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x00, 0x03, 0x00, 0x01, 0x5b, 0xa9, 0x6b, 0xa3, 0x00, 0x00, 0x00,
    0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
  ]),
);

/** Readers bound to the live WASM VFS — mirrors the production binding. */
const vfsReaders: SelfContainedReaders = {
  readText: (p) => {
    const r = vfsReadFile(p);
    return r.success && r.content != null ? r.content : null;
  },
  readBinaryBase64: (p) => {
    const r = vfsReadBinaryFile(p);
    return r.success && r.content != null ? r.content : null;
  },
};

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);
  const wasm = (await import('wasm-quarto-hub-client')) as unknown as {
    default: (input?: BufferSource) => Promise<unknown>;
    render_printable: (path: string) => Promise<string>;
  };
  await wasm.default(wasmBytes);
  await initWasm();
  renderPrintableRaw = wasm.render_printable.bind(wasm);

  // `makeSelfContainedHtml` needs a DOM; the wasm test env is node.
  if (typeof (globalThis as { DOMParser?: unknown }).DOMParser !== 'function') {
    (globalThis as { DOMParser?: unknown }).DOMParser = new JSDOM().window.DOMParser;
  }
});

beforeEach(() => {
  vfsClear();
});

describe('render_printable → makeSelfContainedHtml (issue #315)', () => {
  it('renders a q2-preview doc to a self-contained HTML page with its image inlined', async () => {
    vfsAddFile('_quarto.yml', 'project:\n  type: default\n');
    vfsAddFile(
      'index.qmd',
      '---\ntitle: Doc\nformat: q2-preview\n---\n\n# Heading\n\n![A plot](figures/plot.png)\n',
    );
    vfsAddBinaryFile('figures/plot.png', PNG_BYTES);

    const result = JSON.parse(
      await renderPrintableRaw('/project/index.qmd'),
    ) as RenderResponse;

    // Coerced to the HTML pipeline: `html`, not preview `ast_json`.
    expect(result.success, `render failed: ${result.error}`).toBe(true);
    expect(result.html, 'expected HTML output, not AST').toBeTruthy();
    expect(result.ast_json).toBeFalsy();
    expect(result.html!).toContain('Heading');
    // The user image src is preserved for the inliner to resolve.
    expect(result.html!).toContain('src="figures/plot.png"');

    // Build exactly as production does (buildPrintableHtml) —
    // currentFilePath is the project-relative path (no `/project/`
    // prefix), matching Automerge `file.path`.
    const printable = buildPrintableHtml(
      result.html,
      'index.qmd',
      'q2-preview',
      vfsReaders,
    );

    // Fully self-contained: no external artifact refs, image embedded.
    expect(printable).not.toMatch(/(href|src)="\/\.quarto\//);
    expect(printable).not.toContain('src="figures/plot.png"');
    expect(printable).toContain('data:image/png;base64,');
    expect(printable.startsWith('<!DOCTYPE html>')).toBe(true);
    // Documents get the print-quality stylesheet.
    expect(printable).toContain('data-q2-print');
  });

  it('inlines a real Bootstrap-theme stylesheet without a leading BOM (theme applies)', async () => {
    // Regression for the field bug: the compiled theme CSS artifact
    // carries a UTF-8 BOM. Loaded via <link> the browser strips it, but
    // inlined verbatim into a <style> the BOM invalidates the first
    // selector (`:root,[data-bs-theme=light]{…}`), dropping Bootstrap's
    // CSS variables so the theme silently fails to apply.
    vfsAddFile(
      '_quarto.yml',
      'project:\n  type: default\nformat:\n  html:\n    theme: cosmo\n',
    );
    vfsAddFile(
      'index.qmd',
      '---\ntitle: Themed\nformat: q2-preview\n---\n\n# Heading\n\nbody\n',
    );

    const result = JSON.parse(
      await renderPrintableRaw('/project/index.qmd'),
    ) as RenderResponse;
    expect(result.success, `render failed: ${result.error}`).toBe(true);

    const printable = buildPrintableHtml(
      result.html,
      'index.qmd',
      'q2-preview',
      vfsReaders,
    );

    const doc = new DOMParser().parseFromString(printable, 'text/html');
    const styles = [...doc.querySelectorAll('style')];
    // The Bootstrap theme was inlined as a <style>…
    const themeStyle = styles.find((s) =>
      (s.textContent ?? '').includes('--bs-body-font-family:'),
    );
    expect(themeStyle, 'theme CSS not inlined').toBeTruthy();
    // …and no inlined <style> begins with a BOM (which would corrupt its
    // first rule and drop Bootstrap's `:root` variable block).
    for (const s of styles) {
      expect(s.textContent?.charCodeAt(0)).not.toBe(0xfeff);
    }
    // The light-theme variable definition survives in the inlined text.
    expect(themeStyle!.textContent).toContain('--bs-body-font-family:');
  });

  it('renders a revealjs doc to a standalone deck put into print layout', async () => {
    vfsAddFile('_quarto.yml', 'project:\n  type: default\n');
    vfsAddFile(
      'deck.qmd',
      '---\ntitle: Deck\nformat: revealjs\n---\n\n## Slide One\n\nHello.\n\n## Slide Two\n\n- a\n- b\n',
    );

    const result = JSON.parse(
      await renderPrintableRaw('/project/deck.qmd'),
    ) as RenderResponse;

    expect(result.success, `render failed: ${result.error}`).toBe(true);
    expect(result.html, 'expected deck HTML, not AST').toBeTruthy();
    expect(result.ast_json).toBeFalsy();
    // A real reveal deck shell.
    expect(result.html!).toContain('class="reveal"');
    expect(result.html!).toContain('class="slides"');

    const printable = buildPrintableHtml(
      result.html,
      'deck.qmd',
      'revealjs',
      vfsReaders,
    );

    // reveal.js and its CSS are inlined (no external artifact refs) and
    // the deck is forced into print layout. Decks skip the document
    // print stylesheet (reveal ships its own).
    expect(printable).not.toMatch(/(href|src)="\/\.quarto\//);
    expect(printable).toContain('view:"print"');
    expect(printable).not.toContain('data-q2-print');
  });
});

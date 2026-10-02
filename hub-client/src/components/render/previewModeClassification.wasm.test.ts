/**
 * The three-class classifier against the real wasm (pandoc pandoc-host H5, D8.4/D8.7): the
 * AST's `meta.format`, the Rust-owned resolver and `getQ2Format` agree on which mode each
 * kind of document gets.
 *
 * Run with: npm run test:wasm
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { classifyPreviewMode, type PreviewMode } from './getQ2Format';
import type { ResolvePandocFormatsResponse } from '@quarto/preview-runtime';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  parse_qmd_to_ast_with_attribution: (content: string, attribution?: string) => Promise<string>;
  resolve_pandoc_formats: (path: string) => string;
}

const NO_AST = JSON.stringify({ meta: {}, blocks: [] });

let wasm: WasmModule;

beforeAll(async () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const bytes = await readFile(join(here, '../../../wasm-quarto-hub-client', 'wasm_quarto_hub_client_bg.wasm'));
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(bytes);
});

beforeEach(() => {
  wasm.vfs_clear();
  wasm.vfs_set_runtime_metadata('');
});

async function modeOf(frontMatter: string, canDownload: (key: string) => boolean = () => true, canPreviewPdf?: () => boolean): Promise<PreviewMode> {
  const content = `---\n${frontMatter}\n---\n\nBody.\n`;
  wasm.vfs_add_file('/project/doc.qmd', content);
  const parsed = JSON.parse(await wasm.parse_qmd_to_ast_with_attribution(content));
  // A format the parser's metadata merge does not know (`latex`) fails the parse; the router
  // then classifies from the resolver alone (an AST with no `meta.format`).
  return classifyPreviewMode(parsed.success ? parsed.ast : NO_AST, '/project/doc.qmd', {
    resolve: (path) => JSON.parse(wasm.resolve_pandoc_formats(path)) as ResolvePandocFormatsResponse,
    canDownload,
    canPreviewPdf,
  });
}

describe('classifyPreviewMode against the real wasm', () => {
  it('a document with no format: key previews as q2-preview', async () => {
    expect(await modeOf('title: T')).toEqual({ mode: 'react', format: 'q2-preview' });
  });

  it('revealjs previews', async () => {
    expect((await modeOf('format: revealjs')).mode).toBe('react');
  });

  it('docx, pptx and epub are the download mode', async () => {
    expect(await modeOf('format: docx')).toEqual({ mode: 'download', formatKey: 'docx' });
    expect(await modeOf('format: pptx')).toEqual({ mode: 'download', formatKey: 'pptx' });
    expect(await modeOf('format: epub')).toEqual({ mode: 'download', formatKey: 'epub' });
  });

  it('a downloadable format the menu does not offer yet keeps the full-DOM renderer', async () => {
    expect(await modeOf('format: pptx', (key) => key === 'docx')).toEqual({ mode: 'dom' });
  });

  it('the first of several format keys decides', async () => {
    expect(await modeOf('format:\n  docx: default\n  html: default')).toEqual({ mode: 'download', formatKey: 'docx' });
  });

  it('pdf is the download mode (host H8 wired the chain); latex is neither', async () => {
    expect(await modeOf('format: pdf')).toEqual({ mode: 'download', formatKey: 'pdf' });
    expect(await modeOf('format: pdf', () => true, () => true)).toEqual({ mode: 'pdf', formatKey: 'pdf' });
    expect(await modeOf('format: latex')).toEqual({ mode: 'neither', formatKey: 'latex' });
  });
});

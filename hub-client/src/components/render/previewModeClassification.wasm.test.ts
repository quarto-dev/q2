/**
 * The three-class classifier against the real wasm (pandoc pandoc-host H5, D8.4/D8.7; the
 * project-aware resolver since format-resolution F6): each kind of document, and each kind of
 * project-level `format:`, gets the right preview mode.
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
  resolve_pandoc_formats: (path: string, options?: { contentOverride?: string }) => string;
}

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
  return modeOfBuffer('/project/doc.qmd', content, canDownload, canPreviewPdf);
}

/** What `PreviewRouter` does: classify `path` through the resolver, passing the editor buffer. */
function modeOfBuffer(path: string, buffer: string, canDownload: (key: string) => boolean = () => true, canPreviewPdf?: () => boolean): PreviewMode {
  return classifyPreviewMode(path, {
    resolve: (p) => JSON.parse(wasm.resolve_pandoc_formats(p, { contentOverride: buffer })) as ResolvePandocFormatsResponse,
    canDownload,
    canPreviewPdf,
  });
}

function book(format: string) {
  wasm.vfs_add_file('/project/_quarto.yml', `project:\n  type: book\nbook:\n  title: T\n  chapters:\n    - index.qmd\n    - ch1.qmd\nformat: ${format}\n`);
  wasm.vfs_add_file('/project/index.qmd', '# Index\n');
  wasm.vfs_add_file('/project/ch1.qmd', '# One\n');
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

  it('a typst document previews as the compiled PDF; latex and a document\'s own pdf are neither', async () => {
    expect(await modeOf('format: typst')).toEqual({ mode: 'download', formatKey: 'typst-pdf' });
    expect(await modeOf('format: typst', () => true, () => true)).toEqual({ mode: 'pdf', formatKey: 'typst-pdf' });
    expect(await modeOf('format: typst\noutput-ext: pdf', () => true, () => true)).toEqual({ mode: 'pdf', formatKey: 'typst-pdf' });
    // `format: pdf` is LaTeX, which has no meaning in the browser (use `format: typst`).
    expect(await modeOf('format: pdf')).toEqual({ mode: 'neither', formatKey: 'pdf' });
    expect(await modeOf('format: pdf', () => true, () => true)).toEqual({ mode: 'neither', formatKey: 'pdf' });
    expect(await modeOf('format: latex')).toEqual({ mode: 'neither', formatKey: 'latex' });
  });

  it('a typst document with another output-ext is the typst-source download, not the PDF viewer', async () => {
    expect(await modeOf('format: typst\noutput-ext: typ', () => true, () => true)).toEqual({ mode: 'download', formatKey: 'typst' });
    expect(await modeOf('format:\n  typst:\n    output-ext: typst', () => true, () => true)).toEqual({ mode: 'download', formatKey: 'typst' });
  });

  // The strand's third symptom: the project's `format:` reaches a chapter that names none.
  describe('project-level format', () => {
    it('a front-matter-less chapter of a `format: typst` book gets the PDF pane', () => {
      book('typst');
      expect(modeOfBuffer('/project/ch1.qmd', '# One\n', () => true, () => true)).toEqual({ mode: 'pdf', formatKey: 'typst-pdf' });
    });

    it('a chapter whose front matter says `format: html` gets the html preview (the document wins in chapter scope)', () => {
      book('typst');
      const buffer = '---\nformat: html\n---\n\n# One\n';
      expect(modeOfBuffer('/project/ch1.qmd', buffer, () => true, () => true)).toEqual({ mode: 'react', format: 'q2-preview' });
    });

    it('the buffer wins over a different VFS copy', () => {
      book('typst');
      wasm.vfs_add_file('/project/ch1.qmd', '---\nformat: html\n---\n\n# One\n');
      expect(modeOfBuffer('/project/ch1.qmd', '# One\n', () => true, () => true)).toEqual({ mode: 'pdf', formatKey: 'typst-pdf' });
    });

    it('a project-level `format: revealjs` page previews as slides', () => {
      wasm.vfs_add_file('/project/_quarto.yml', 'project:\n  type: default\nformat: revealjs\n');
      wasm.vfs_add_file('/project/slides.qmd', '# Slide\n');
      expect(modeOfBuffer('/project/slides.qmd', '# Slide\n')).toEqual({ mode: 'react', format: 'revealjs' });
    });

    it('a project with no format: key previews as q2-preview', () => {
      wasm.vfs_add_file('/project/_quarto.yml', 'project:\n  type: default\n');
      wasm.vfs_add_file('/project/a.qmd', '# A\n');
      expect(modeOfBuffer('/project/a.qmd', '# A\n')).toEqual({ mode: 'react', format: 'q2-preview' });
    });
  });
});

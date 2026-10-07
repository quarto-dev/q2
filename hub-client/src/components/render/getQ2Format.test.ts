/**
 * The hub-client renderer-routing rule (bd-kltzdhle, D2 in
 * `claude-notes/plans/2026-09-09-hub-client-default-q2-preview.md`).
 *
 * `getQ2Format` reads the format `MetadataMergeStage` wrote into
 * `meta.format` and decides which preview branch `PreviewRouter`
 * mounts: a non-null string mounts `ReactPreview` with that format; `null`
 * mounts the full-DOM `Preview` (MorphIframe). The rule mirrors
 * `map_format_for_preview` in `crates/wasm-quarto-hub-client/src/lib.rs`
 * for the `html` arm, so hub-client's default is `q2 preview`'s default.
 */

import { describe, it, expect } from 'vitest';
import { getQ2Format, classifyPreviewMode } from './getQ2Format';

function astWithFormat(format: string | undefined): string {
  const meta =
    format === undefined ? {} : { format: { t: 'MetaString', c: format } };
  return JSON.stringify({ 'pandoc-api-version': [1, 23, 1], meta, blocks: [] });
}

describe('getQ2Format (hub-client renderer routing)', () => {
  it('routes html — the default when no format: key is set — to q2-preview', () => {
    // `detect_format_from_content` reports `html` for a missing key, a
    // bare `format: html`, and a `format: html: {…}` map, and the merge
    // stage writes that normalised string back; all three arrive here
    // as the single string `html`.
    expect(getQ2Format(astWithFormat('html'))).toBe('q2-preview');
  });

  it('routes q2-html-render to the full-DOM renderer (null)', () => {
    expect(getQ2Format(astWithFormat('q2-html-render'))).toBeNull();
  });

  it('passes the React-side formats through unchanged', () => {
    for (const f of ['q2-preview', 'q2-debug', 'q2-slides', 'q2-sandboxed-preview', 'revealjs']) {
      expect(getQ2Format(astWithFormat(f)), f).toBe(f);
    }
  });

  it('routes non-html formats to the full-DOM renderer (null)', () => {
    for (const f of ['pdf', 'docx', 'typst', 'gfm', 'acm-html']) {
      expect(getQ2Format(astWithFormat(f)), f).toBeNull();
    }
  });

  it('returns null when meta carries no format at all', () => {
    // Only reachable for ASTs that did not pass through the merge stage
    // (it always writes `format`); keep the defensive branch pinned.
    expect(getQ2Format(astWithFormat(undefined))).toBeNull();
  });

  it('returns null for unparseable AST JSON', () => {
    expect(getQ2Format('not json')).toBeNull();
  });
});

describe('classifyPreviewMode (three-class classifier, D8)', () => {
  const resolved = (key: string, cls: 'preview' | 'download' | 'neither') => () => ({
    success: true as const,
    source: 'document' as const,
    formats: [{ key, class: cls }],
  });
  const deps = (r: ReturnType<typeof resolved> | (() => null), canDownload = true) => ({ resolve: r, canDownload: () => canDownload });

  it('keeps the React side where getQ2Format decides it', () => {
    expect(classifyPreviewMode(astWithFormat('html'), 'a.qmd', deps(resolved('html', 'preview')))).toEqual({ mode: 'react', format: 'q2-preview' });
    expect(classifyPreviewMode(astWithFormat('revealjs'), 'a.qmd', deps(resolved('revealjs', 'preview')))).toEqual({ mode: 'react', format: 'revealjs' });
  });

  it('keeps the explicit full-DOM opt-out without consulting the resolver', () => {
    const resolve = () => {
      throw new Error('must not be called');
    };
    expect(classifyPreviewMode(astWithFormat('q2-html-render'), 'a.qmd', { resolve, canDownload: () => true })).toEqual({ mode: 'dom' });
  });

  it('a downloadable own format is the download mode, naming the first format key', () => {
    expect(classifyPreviewMode(astWithFormat('docx'), 'a.qmd', deps(resolved('docx', 'download')))).toEqual({ mode: 'download', formatKey: 'docx' });
  });

  it('a document whose own format is pdf (LaTeX) is neither: the browser cannot run it', () => {
    expect(classifyPreviewMode(astWithFormat('pdf'), 'a.qmd', { ...deps(resolved('pdf', 'neither')), canPreviewPdf: () => true })).toEqual({ mode: 'neither', formatKey: 'pdf' });
  });

  it('the resolved typst-pdf key (a typst document) is the pdf mode when the build compiles PDF in the browser, else the download mode', () => {
    const r = deps(resolved('typst-pdf', 'download'));
    expect(classifyPreviewMode(astWithFormat('pdf'), 'a.qmd', { ...r, canPreviewPdf: () => true })).toEqual({ mode: 'pdf', formatKey: 'typst-pdf' });
    expect(classifyPreviewMode(astWithFormat('pdf'), 'a.qmd', { ...r, canPreviewPdf: () => false })).toEqual({ mode: 'download', formatKey: 'typst-pdf' });
    expect(classifyPreviewMode(astWithFormat('pdf'), 'a.qmd', r)).toEqual({ mode: 'download', formatKey: 'typst-pdf' });
    // Download must be possible at all, and only pdf gets the viewer.
    expect(classifyPreviewMode(astWithFormat('pdf'), 'a.qmd', { ...deps(resolved('typst-pdf', 'download'), false), canPreviewPdf: () => true })).toEqual({ mode: 'dom' });
    expect(classifyPreviewMode(astWithFormat('docx'), 'a.qmd', { ...deps(resolved('docx', 'download')), canPreviewPdf: () => true })).toEqual({ mode: 'download', formatKey: 'docx' });
  });

  it('a downloadable format this build cannot produce keeps the full-DOM renderer', () => {
    expect(classifyPreviewMode(astWithFormat('docx'), 'a.qmd', deps(resolved('docx', 'download'), false))).toEqual({ mode: 'dom' });
  });

  it('a format that is neither mounts neither preview and names the format', () => {
    expect(classifyPreviewMode(astWithFormat('latex'), 'a.qmd', deps(resolved('latex', 'neither')))).toEqual({ mode: 'neither', formatKey: 'latex' });
  });

  it('falls back to the full-DOM renderer when the resolver fails or says preview', () => {
    expect(classifyPreviewMode(astWithFormat('x'), 'a.qmd', deps(() => null))).toEqual({ mode: 'dom' });
    expect(classifyPreviewMode(astWithFormat('x'), 'a.qmd', deps(() => ({ success: false as const, error: 'no' })))).toEqual({ mode: 'dom' });
    expect(classifyPreviewMode(astWithFormat('x'), 'a.qmd', deps(resolved('x', 'preview')))).toEqual({ mode: 'dom' });
    expect(classifyPreviewMode(astWithFormat('x'), undefined, deps(resolved('docx', 'download')))).toEqual({ mode: 'dom' });
  });
});

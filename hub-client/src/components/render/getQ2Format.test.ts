/**
 * The hub-client preview-mode rule. TypeScript decides nothing about the format: the Rust
 * resolver returns the document's `key`, `class` and `pseudoFormat`, and `classifyPreviewMode`
 * maps them to the pane `PreviewRouter` mounts (bd-kltzdhle's renderer routing, now driven by
 * the project-aware resolver, format-resolution F6).
 */

import { describe, it, expect } from 'vitest';
import { classifyPreviewMode } from './getQ2Format';

type Cls = 'preview' | 'download' | 'neither';

const resolved = (key: string, cls: Cls, pseudoFormat: string | null = null) => () => ({
  success: true as const,
  source: 'project' as const,
  key,
  class: cls,
  pseudoFormat,
  scope: 'chapter' as const,
  formats: [{ key, class: cls }],
});
const deps = (r: ReturnType<typeof resolved> | (() => null), canDownload = true) => ({ resolve: r, canDownload: () => canDownload });

describe('classifyPreviewMode', () => {
  it('mounts the React preview for the resolver pseudoFormat (html → q2-preview, revealjs, q2-*)', () => {
    expect(classifyPreviewMode('a.qmd', deps(resolved('html', 'preview', 'q2-preview')))).toEqual({ mode: 'react', format: 'q2-preview' });
    expect(classifyPreviewMode('a.qmd', deps(resolved('revealjs', 'preview', 'revealjs')))).toEqual({ mode: 'react', format: 'revealjs' });
    expect(classifyPreviewMode('a.qmd', deps(resolved('q2-slides', 'preview', 'q2-slides')))).toEqual({ mode: 'react', format: 'q2-slides' });
  });

  it('the explicit full-DOM opt-out is dom', () => {
    expect(classifyPreviewMode('a.qmd', deps(resolved('q2-html-render', 'preview')))).toEqual({ mode: 'dom' });
  });

  it('a downloadable own format is the download mode, naming the resolved key', () => {
    expect(classifyPreviewMode('a.qmd', deps(resolved('docx', 'download')))).toEqual({ mode: 'download', formatKey: 'docx' });
  });

  it('a document whose own format is pdf (LaTeX) is neither: the browser cannot run it', () => {
    expect(classifyPreviewMode('a.qmd', { ...deps(resolved('pdf', 'neither')), canPreviewPdf: () => true })).toEqual({ mode: 'neither', formatKey: 'pdf' });
  });

  it('the resolved typst-pdf key (a typst document, or a typst project) is the pdf mode when the build compiles PDF in the browser, else the download mode', () => {
    const r = deps(resolved('typst-pdf', 'download'));
    expect(classifyPreviewMode('a.qmd', { ...r, canPreviewPdf: () => true })).toEqual({ mode: 'pdf', formatKey: 'typst-pdf' });
    expect(classifyPreviewMode('a.qmd', { ...r, canPreviewPdf: () => false })).toEqual({ mode: 'download', formatKey: 'typst-pdf' });
    expect(classifyPreviewMode('a.qmd', r)).toEqual({ mode: 'download', formatKey: 'typst-pdf' });
    // Download must be possible at all, and only typst-pdf gets the viewer.
    expect(classifyPreviewMode('a.qmd', { ...deps(resolved('typst-pdf', 'download'), false), canPreviewPdf: () => true })).toEqual({ mode: 'dom' });
    expect(classifyPreviewMode('a.qmd', { ...deps(resolved('docx', 'download')), canPreviewPdf: () => true })).toEqual({ mode: 'download', formatKey: 'docx' });
  });

  it('reads the resolved key, never formats[0]', () => {
    const r = () => ({ ...resolved('typst-pdf', 'download')(), formats: [{ key: 'html', class: 'preview' as const }, { key: 'typst-pdf', class: 'download' as const }] });
    expect(classifyPreviewMode('a.qmd', { resolve: r, canDownload: () => true, canPreviewPdf: () => true })).toEqual({ mode: 'pdf', formatKey: 'typst-pdf' });
  });

  it('a downloadable format this build cannot produce keeps the full-DOM renderer', () => {
    expect(classifyPreviewMode('a.qmd', deps(resolved('docx', 'download'), false))).toEqual({ mode: 'dom' });
  });

  it('a format that is neither mounts neither preview and names the format', () => {
    expect(classifyPreviewMode('a.qmd', deps(resolved('latex', 'neither')))).toEqual({ mode: 'neither', formatKey: 'latex' });
  });

  it('falls back to the full-DOM renderer when the resolver fails, has no path, or says preview without a pseudo-format', () => {
    expect(classifyPreviewMode('a.qmd', deps(() => null))).toEqual({ mode: 'dom' });
    expect(classifyPreviewMode('a.qmd', deps(() => ({ success: false as const, error: 'no' })))).toEqual({ mode: 'dom' });
    expect(classifyPreviewMode('a.qmd', deps(resolved('acm-html', 'preview')))).toEqual({ mode: 'dom' });
    expect(classifyPreviewMode(undefined, deps(resolved('docx', 'download')))).toEqual({ mode: 'dom' });
  });
});

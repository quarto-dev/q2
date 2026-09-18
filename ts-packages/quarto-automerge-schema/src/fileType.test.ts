import { describe, it, expect } from 'vitest';
import { getFileExtension, inferMimeType, isBinaryExtension, isTextExtension } from './index';

/**
 * Extension-based file classification. The set of binary extensions
 * mirrors `BINARY_EXTENSIONS` in `crates/quarto-hub/src/resource.rs`
 * (the Rust side decides which project files sync as binary documents;
 * this side classifies uploads and displays). Keep the two in step.
 */
describe('file type detection', () => {
  it('extracts a lowercase extension without the dot', () => {
    expect(getFileExtension('figs/Plot.HEP')).toBe('hep');
    expect(getFileExtension('README')).toBe('');
    expect(getFileExtension('trailing.')).toBe('');
  });

  it('classifies images, fonts and documents as binary', () => {
    expect(isBinaryExtension('a.png')).toBe(true);
    expect(isBinaryExtension('dir/a.PDF')).toBe(true);
    expect(isBinaryExtension('fonts/a.woff2')).toBe(true);
    expect(isBinaryExtension('a.qmd')).toBe(false);
    expect(isBinaryExtension('a.yml')).toBe(false);
  });

  it('classifies hephaestus plot documents (.hep) as binary (bd-sxiv2tio)', () => {
    // The preview draws `.hep` plots in the browser from the synced
    // bytes; a text classification would corrupt them.
    expect(isBinaryExtension('figs/readings.hep')).toBe(true);
    expect(isBinaryExtension('figs/readings.HEP')).toBe(true);
    expect(isTextExtension('figs/readings.hep')).toBe(false);
    expect(inferMimeType('figs/readings.hep')).toBe('application/vnd.hephaestus.plot');
  });

  it('infers MIME types by extension and falls back to octet-stream', () => {
    expect(inferMimeType('a.png')).toBe('image/png');
    expect(inferMimeType('a.svg')).toBe('image/svg+xml');
    expect(inferMimeType('a.unknownext')).toBe('application/octet-stream');
  });
});

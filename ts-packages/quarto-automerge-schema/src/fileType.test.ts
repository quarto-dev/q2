import { describe, it, expect } from 'vitest';
import {
  getFileExtension,
  inferMimeType,
  isBinaryExtension,
  isImageExtension,
  isTextExtension,
} from './index';

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

  it('classifies import sources and EMF/WMF as binary with no viewer (I21)', () => {
    // Before, a stored .docx was neither binary nor text, so selecting it
    // opened an empty text editor. Binary-and-not-image makes the editor's
    // handleSelectFile a no-op, like pdf.
    for (const ext of ['docx', 'odt', 'rtf', 'epub', 'pptx', 'emf', 'wmf']) {
      expect(isBinaryExtension(`dir/a.${ext}`)).toBe(true);
      expect(isBinaryExtension(`dir/a.${ext.toUpperCase()}`)).toBe(true);
      expect(isImageExtension(`dir/a.${ext}`)).toBe(false);
      expect(isTextExtension(`dir/a.${ext}`)).toBe(false);
    }
  });

  it('infers MIME types for the import sources and EMF/WMF', () => {
    expect(inferMimeType('a.docx')).toBe(
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    );
    expect(inferMimeType('a.odt')).toBe('application/vnd.oasis.opendocument.text');
    expect(inferMimeType('a.rtf')).toBe('application/rtf');
    expect(inferMimeType('a.epub')).toBe('application/epub+zip');
    expect(inferMimeType('a.pptx')).toBe(
      'application/vnd.openxmlformats-officedocument.presentationml.presentation',
    );
    expect(inferMimeType('a.emf')).toBe('image/emf');
    expect(inferMimeType('a.wmf')).toBe('image/wmf');
  });

  it('infers MIME types by extension and falls back to octet-stream', () => {
    expect(inferMimeType('a.png')).toBe('image/png');
    expect(inferMimeType('a.svg')).toBe('image/svg+xml');
    expect(inferMimeType('a.unknownext')).toBe('application/octet-stream');
  });
});

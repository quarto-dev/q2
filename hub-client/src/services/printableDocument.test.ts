/**
 * Unit tests for the DOM-free logic of the printable-document service
 * (issue #315, bd-vhdknrvl). The self-contained inlining
 * (`makeSelfContainedHtml`) and reveal print-mode transform
 * (`forceRevealPrintMode`) are tested in `@quarto/preview-renderer`;
 * the render + `window.open` orchestration is covered end-to-end in a
 * browser. Here we pin the pure decision points.
 */

import { describe, it, expect } from 'vitest';
import {
  isPrintableSlidesFormat,
  buildPrintableHtml,
} from './printableDocument';

describe('isPrintableSlidesFormat', () => {
  it('is true for reveal deck formats', () => {
    expect(isPrintableSlidesFormat('revealjs')).toBe(true);
    expect(isPrintableSlidesFormat('q2-slides')).toBe(true);
  });

  it('is false for document / debug / null formats', () => {
    expect(isPrintableSlidesFormat('q2-preview')).toBe(false);
    expect(isPrintableSlidesFormat('html')).toBe(false);
    expect(isPrintableSlidesFormat('q2-debug')).toBe(false);
    expect(isPrintableSlidesFormat(null)).toBe(false);
  });
});

describe('buildPrintableHtml', () => {
  it('throws when the render produced no HTML', () => {
    const readers = { readText: () => null, readBinaryBase64: () => null };
    expect(() =>
      buildPrintableHtml(undefined, '/project/doc.qmd', 'q2-preview', readers),
    ).toThrow(/no HTML/i);
  });
});

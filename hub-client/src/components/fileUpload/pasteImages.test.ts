/**
 * Tests for the pure clipboard-paste decision module (bd-706b0ixu).
 *
 * The precedence matrix mirrors §2a of
 * claude-notes/plans/2026-08-27-paste-image-clipboard.md: take over iff
 * the payload's files are all accepted raster images (non-empty, none
 * zero-sized) and the text/plain item is empty or merely the filename
 * rider; everything else passes through to Monaco's own paste handling.
 */

import { describe, it, expect } from 'vitest';
import {
  classifyPastePayload,
  pastedImageFilename,
  sanitizeAltText,
  ACCEPTED_PASTE_IMAGE_TYPES,
} from './pasteImages';

const png = (name = 'image.png', size = 1024) => ({
  name,
  type: 'image/png',
  size,
});

describe('classifyPastePayload', () => {
  describe('take-over cases', () => {
    it('screenshot paste: single PNG file, no text', () => {
      expect(classifyPastePayload({ files: [png()], text: '' })).toBe(
        'take-over'
      );
    });

    it("Chrome 'Copy image': PNG file, empty text/plain (text/html ignored)", () => {
      // Chrome's copy-image payload carries text/html but no text/plain;
      // the classifier only sees the text/plain rendition.
      expect(classifyPastePayload({ files: [png()], text: '' })).toBe(
        'take-over'
      );
    });

    it('Finder file copy with filename rider as text', () => {
      expect(
        classifyPastePayload({
          files: [png('photo.png')],
          text: 'photo.png',
        })
      ).toBe('take-over');
    });

    it('filename rider with surrounding whitespace still counts as rider', () => {
      expect(
        classifyPastePayload({
          files: [png('photo.png')],
          text: '  photo.png\n',
        })
      ).toBe('take-over');
    });

    it('multi-file: all accepted raster types, no text', () => {
      expect(
        classifyPastePayload({
          files: [png('a.png'), { name: 'b.jpg', type: 'image/jpeg', size: 10 }],
          text: '',
        })
      ).toBe('take-over');
    });

    it('multi-file with newline-joined filename rider', () => {
      expect(
        classifyPastePayload({
          files: [png('a.png'), png('b.png')],
          text: 'a.png\nb.png',
        })
      ).toBe('take-over');
    });

    it.each([
      ['image/jpeg', 'photo.jpg'],
      ['image/gif', 'anim.gif'],
      ['image/webp', 'pic.webp'],
      ['image/avif', 'pic.avif'],
    ])('accepts raster type %s', (type, name) => {
      expect(
        classifyPastePayload({ files: [{ name, type, size: 10 }], text: '' })
      ).toBe('take-over');
    });
  });

  describe('pass-through cases', () => {
    it('Excel/Office mixed payload: image rendition plus meaningful text', () => {
      expect(
        classifyPastePayload({
          files: [png()],
          text: 'cell1\tcell2\ncell3\tcell4',
        })
      ).toBe('pass-through');
    });

    it('text differing from the filename is not a rider', () => {
      expect(
        classifyPastePayload({
          files: [png('photo.png')],
          text: 'photo of my cat',
        })
      ).toBe('pass-through');
    });

    it('SVG file (active content stays dialog-only, §3c S1)', () => {
      expect(
        classifyPastePayload({
          files: [{ name: 'diagram.svg', type: 'image/svg+xml', size: 10 }],
          text: '',
        })
      ).toBe('pass-through');
    });

    it('non-image file (PDF)', () => {
      expect(
        classifyPastePayload({
          files: [{ name: 'doc.pdf', type: 'application/pdf', size: 10 }],
          text: '',
        })
      ).toBe('pass-through');
    });

    it('unsupported raster type (image/tiff)', () => {
      expect(
        classifyPastePayload({
          files: [{ name: 'scan.tiff', type: 'image/tiff', size: 10 }],
          text: '',
        })
      ).toBe('pass-through');
    });

    it('file with empty MIME type', () => {
      expect(
        classifyPastePayload({
          files: [{ name: 'mystery', type: '', size: 10 }],
          text: '',
        })
      ).toBe('pass-through');
    });

    it('plain text paste (no files)', () => {
      expect(classifyPastePayload({ files: [], text: 'hello' })).toBe(
        'pass-through'
      );
    });

    it('empty payload', () => {
      expect(classifyPastePayload({ files: [], text: '' })).toBe(
        'pass-through'
      );
    });

    it('one accepted image plus one SVG', () => {
      expect(
        classifyPastePayload({
          files: [
            png(),
            { name: 'diagram.svg', type: 'image/svg+xml', size: 10 },
          ],
          text: '',
        })
      ).toBe('pass-through');
    });

    it('zero-size file (degrade to Monaco per §D5)', () => {
      expect(
        classifyPastePayload({ files: [png('image.png', 0)], text: '' })
      ).toBe('pass-through');
    });
  });
});

describe('pastedImageFilename', () => {
  const hash =
    'abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890';

  it('uses the pasted- prefix and an 8-char hash prefix', () => {
    expect(pastedImageFilename(hash, 'image/png')).toBe('pasted-abcdef12.png');
  });

  it.each([
    ['image/jpeg', 'pasted-abcdef12.jpg'],
    ['image/gif', 'pasted-abcdef12.gif'],
    ['image/webp', 'pasted-abcdef12.webp'],
    ['image/avif', 'pasted-abcdef12.avif'],
  ])('maps %s to the right extension', (mime, expected) => {
    expect(pastedImageFilename(hash, mime)).toBe(expected);
  });

  it('returns null for a MIME type outside the accepted set', () => {
    expect(pastedImageFilename(hash, 'image/svg+xml')).toBeNull();
    expect(pastedImageFilename(hash, 'application/pdf')).toBeNull();
  });

  it('accepted set and extension map agree', () => {
    for (const mime of ACCEPTED_PASTE_IMAGE_TYPES) {
      expect(pastedImageFilename(hash, mime)).not.toBeNull();
    }
  });
});

describe('sanitizeAltText', () => {
  it('passes plain text through', () => {
    expect(sanitizeAltText('a nice photo')).toBe('a nice photo');
  });

  it('collapses newlines and whitespace runs to single spaces', () => {
    expect(sanitizeAltText('line one\nline   two')).toBe('line one line two');
  });

  it('escapes square brackets', () => {
    expect(sanitizeAltText('see [fig 1]')).toBe('see \\[fig 1\\]');
  });

  it('trims leading and trailing whitespace', () => {
    expect(sanitizeAltText('  padded  ')).toBe('padded');
  });
});

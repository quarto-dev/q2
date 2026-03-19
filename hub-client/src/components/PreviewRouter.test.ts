import { describe, it, expect } from 'vitest';
import { getQ2Format } from './PreviewRouter';

/** Helper to build an AST JSON string with a MetaString format value */
function metaString(value: string): string {
  return JSON.stringify({ meta: { format: { t: 'MetaString', c: value } } });
}

/** Helper to build an AST JSON string with a MetaInlines format value */
function metaInlines(value: string): string {
  return JSON.stringify({
    meta: { format: { t: 'MetaInlines', c: [{ t: 'Str', c: value }] } },
  });
}

describe('getQ2Format', () => {
  // --- q2-* formats should be returned ---

  it('returns "q2-slides" from MetaString', () => {
    expect(getQ2Format(metaString('q2-slides'))).toBe('q2-slides');
  });

  it('returns "q2-slides" from MetaInlines', () => {
    expect(getQ2Format(metaInlines('q2-slides'))).toBe('q2-slides');
  });

  it('returns "q2-debug" from MetaString', () => {
    expect(getQ2Format(metaString('q2-debug'))).toBe('q2-debug');
  });

  it('returns "q2-debug" from MetaInlines', () => {
    expect(getQ2Format(metaInlines('q2-debug'))).toBe('q2-debug');
  });

  // --- non-q2 formats should return null ---

  it('returns null for "html" (MetaString)', () => {
    expect(getQ2Format(metaString('html'))).toBeNull();
  });

  it('returns null for "html" (MetaInlines)', () => {
    expect(getQ2Format(metaInlines('html'))).toBeNull();
  });

  it('returns null for "pdf"', () => {
    expect(getQ2Format(metaString('pdf'))).toBeNull();
  });

  it('returns null for "docx"', () => {
    expect(getQ2Format(metaString('docx'))).toBeNull();
  });

  it('returns null for "revealjs"', () => {
    expect(getQ2Format(metaString('revealjs'))).toBeNull();
  });

  it('returns null for "epub"', () => {
    expect(getQ2Format(metaString('epub'))).toBeNull();
  });

  // --- edge cases ---

  it('returns null when meta has no format key', () => {
    expect(getQ2Format(JSON.stringify({ meta: { title: { t: 'MetaString', c: 'Hello' } } }))).toBeNull();
  });

  it('returns null when meta is empty', () => {
    expect(getQ2Format(JSON.stringify({ meta: {} }))).toBeNull();
  });

  it('returns null when there is no meta', () => {
    expect(getQ2Format(JSON.stringify({}))).toBeNull();
  });

  it('returns null for invalid JSON', () => {
    expect(getQ2Format('not json')).toBeNull();
  });

  it('returns null for empty string', () => {
    expect(getQ2Format('')).toBeNull();
  });

  it('returns null for empty MetaInlines array', () => {
    expect(getQ2Format(JSON.stringify({
      meta: { format: { t: 'MetaInlines', c: [] } },
    }))).toBeNull();
  });

  it('returns null for unknown meta type', () => {
    expect(getQ2Format(JSON.stringify({
      meta: { format: { t: 'MetaMap', c: {} } },
    }))).toBeNull();
  });

  it('returns null for format value that is just "q2-" with no suffix', () => {
    expect(getQ2Format(metaString('q2-'))).toBe('q2-');
  });
});

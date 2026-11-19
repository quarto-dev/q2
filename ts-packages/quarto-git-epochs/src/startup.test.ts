import { describe, expect, it } from 'vitest';
import { readJson } from './fixture-io.js';
import { startupDecision } from './startup.js';

const rows: any[] = readJson('decision-tables/startup.json');

// The fixture rows name the overall case. The current blobs are derived from the
// record: when HEAD changed, the first file's blob differs and the others are the same.
function currentBlobs(r: any): Record<string, string> {
  const files = Object.keys(r.record?.blobs ?? { 'a.qmd': 1 });
  return Object.fromEntries(
    files.map((f, i) => [f, r.record && r.head !== r.record.head && i === 0 ? `${r.record.blobs[f]}-new` : (r.record?.blobs[f] ?? 'b1')]),
  );
}

describe('startupDecision: shared fixture rows', () => {
  for (const r of rows) {
    it(r.id, () => {
      expect(startupDecision({ record: r.record, head: r.head, blobs: currentBlobs(r) }).case).toBe(r.expected);
    });
  }
});

describe('startupDecision: per-file plan', () => {
  it('reconciles every file when HEAD is unchanged', () => {
    const p = startupDecision({ record: { head: 'h', blobs: { a: '1', b: '2' } }, head: 'h', blobs: { a: '1', b: '2' } });
    expect(p.files).toEqual({ a: 'reconcile', b: 'reconcile' });
  });
  it('arrives for the files whose blob differs, and for files the record has not seen', () => {
    const p = startupDecision({
      record: { head: 'h', blobs: { a: '1', b: '2' } },
      head: 'h2',
      blobs: { a: '1', b: '9', c: '3' },
    });
    expect(p.case).toBe('arrive-differing');
    expect(p.files).toEqual({ a: 'reconcile', b: 'arrive', c: 'arrive' });
  });
  it('arrives for every file without a record', () => {
    const p = startupDecision({ record: null, head: 'h', blobs: { a: '1', b: '2' } });
    expect(p.files).toEqual({ a: 'arrive', b: 'arrive' });
  });
  it('a detached path survives a restart when its blob is unchanged, and arrives when it changed', () => {
    const rec = { head: 'h', blobs: { a: '1', b: '2' }, detached: ['a', 'b'] };
    const same = startupDecision({ record: rec, head: 'h', blobs: { a: '1', b: '2' } });
    expect(same.files).toEqual({ a: 'stay-detached', b: 'stay-detached' });
    const moved = startupDecision({ record: rec, head: 'h2', blobs: { a: '1', b: '9' } });
    expect(moved.files).toEqual({ a: 'stay-detached', b: 'arrive' });
  });
  it('drops files that left the tree', () => {
    const p = startupDecision({ record: { head: 'h', blobs: { a: '1', gone: '2' } }, head: 'h2', blobs: { a: '1' } });
    expect(Object.keys(p.files)).toEqual(['a']);
  });
});

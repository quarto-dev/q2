import { describe, expect, it, vi } from 'vitest';
import { captureDocIdsFor, fetchChapterCaptures } from './captureFetch';

const bytes = (n: number) => new Uint8Array([n]);

describe('captureDocIdsFor', () => {
  const captures = { 'index.qmd': { captureDocId: 'a' }, 'one.qmd': { captureDocId: 'b' }, 'other.qmd': { captureDocId: 'c' } } as never;

  it('keeps the chapters that have a capture, keyed by chapter path', () => {
    expect(captureDocIdsFor(['index.qmd', 'one.qmd', 'two.qmd'], captures)).toEqual({ 'index.qmd': 'a', 'one.qmd': 'b' });
  });

  it('is empty without a sidecar', () => {
    expect(captureDocIdsFor(['one.qmd'], undefined)).toEqual({});
  });
});

describe('fetchChapterCaptures', () => {
  it('fetches every chapter in parallel and returns the bytes by path', async () => {
    const fetchDoc = vi.fn(async (id: string) => ({ content: bytes(id === 'a' ? 1 : 2) }));
    const out = await fetchChapterCaptures({ 'index.qmd': 'a', 'one.qmd': 'b' }, new AbortController().signal, fetchDoc as never);
    expect(out).toEqual({ byPath: { 'index.qmd': bytes(1), 'one.qmd': bytes(2) }, failed: [] });
    expect(fetchDoc).toHaveBeenCalledTimes(2);
  });

  it('a chapter whose fetch throws, or whose doc is missing or empty, is reported failed and the others still arrive', async () => {
    const fetchDoc = vi.fn(async (id: string) => {
      if (id === 'a') throw new Error('timeout');
      if (id === 'b') return undefined;
      if (id === 'c') return { content: undefined };
      return { content: bytes(9) };
    });
    const out = await fetchChapterCaptures({ 'a.qmd': 'a', 'b.qmd': 'b', 'c.qmd': 'c', 'd.qmd': 'd' }, new AbortController().signal, fetchDoc as never);
    expect(out.byPath).toEqual({ 'd.qmd': bytes(9) });
    expect(out.failed.sort()).toEqual(['a.qmd', 'b.qmd', 'c.qmd']);
  });

  it('an abort settles at once even though the underlying fetch is still pending (it cannot be cancelled)', async () => {
    const never = new Promise<never>(() => {});
    const ctl = new AbortController();
    const pending = fetchChapterCaptures({ 'a.qmd': 'a' }, ctl.signal, (() => never) as never);
    ctl.abort(new Error('cancelled'));
    await expect(pending).rejects.toThrow('cancelled');
  });

  it('an already-aborted signal rejects without fetching', async () => {
    const ctl = new AbortController();
    ctl.abort(new Error('superseded'));
    const fetchDoc = vi.fn();
    await expect(fetchChapterCaptures({ 'a.qmd': 'a' }, ctl.signal, fetchDoc as never)).rejects.toThrow('superseded');
    expect(fetchDoc).not.toHaveBeenCalled();
  });
});

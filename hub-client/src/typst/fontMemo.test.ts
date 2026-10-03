import { describe, expect, it, vi } from 'vitest';
import { createFontMemo, type FontListResult } from './fontMemo';

const OK = (families: string[] = ['A']): FontListResult => ({ ok: true, families, notices: [] });
const FAIL = (kind: 'superseded' | 'aborted' | 'typst-error' | 'crash'): FontListResult => ({ ok: false, kind, diagnostics: [], notices: [] });

function setup(initialKey = 'k1') {
  const state = { key: initialKey };
  const calls: { resolve: (r: FontListResult) => void; signal?: AbortSignal; onLoadProgress?: (p: never) => void }[] = [];
  const runner = {
    listFonts: vi.fn(
      (_fonts?: Uint8Array[], o?: { signal?: AbortSignal; onLoadProgress?: (p: never) => void }) =>
        new Promise<FontListResult>((resolve) => calls.push({ resolve, signal: o?.signal, onLoadProgress: o?.onLoadProgress })),
    ),
  };
  const fonts = vi.fn(() => [new Uint8Array([1])]);
  const memo = createFontMemo({ runner: runner as never, fonts, key: () => state.key });
  return { memo, calls, runner, fonts, state };
}

describe('createFontMemo', () => {
  it('hits on the same key and misses on a changed key', async () => {
    const { memo, calls, runner, state } = setup();
    const a = memo();
    calls[0].resolve(OK(['X']));
    expect(await a).toMatchObject({ families: ['X'] });
    expect(await memo()).toMatchObject({ families: ['X'] });
    expect(runner.listFonts).toHaveBeenCalledTimes(1);
    state.key = 'k2';
    const c = memo();
    calls[1].resolve(OK(['Y']));
    expect(await c).toMatchObject({ families: ['Y'] });
    expect(runner.listFonts).toHaveBeenCalledTimes(2);
  });

  it('evaluates the key before the font buffers are copied', async () => {
    const order: string[] = [];
    const memo = createFontMemo({
      runner: { listFonts: () => Promise.resolve(OK()) },
      fonts: () => (order.push('fonts'), []),
      key: () => (order.push('key'), 'k'),
    });
    await memo();
    expect(order.slice(0, 2)).toEqual(['key', 'fonts']);
    await memo(); // a hit never copies the buffers
    expect(order.filter((x) => x === 'fonts')).toHaveLength(1);
  });

  it('shares the in-flight promise between waiters', async () => {
    const { memo, calls, runner } = setup();
    const a = memo();
    const b = memo();
    expect(runner.listFonts).toHaveBeenCalledTimes(1);
    calls[0].resolve(OK());
    expect(await Promise.all([a, b])).toHaveLength(2);
  });

  it("an abort of the first waiter resolves it aborted and does not resolve the second's job; the job runs under its own signal", async () => {
    const { memo, calls } = setup();
    const ac = new AbortController();
    const a = memo({ signal: ac.signal });
    const b = memo();
    expect(calls[0].signal?.aborted).toBe(false);
    ac.abort();
    expect(await a).toMatchObject({ ok: false, kind: 'aborted' });
    expect(calls[0].signal?.aborted).toBe(false);
    calls[0].resolve(OK(['Z']));
    expect(await b).toMatchObject({ ok: true, families: ['Z'] });
  });

  it('does not cache a non-ok outcome', async () => {
    const { memo, calls, runner } = setup();
    const a = memo();
    calls[0].resolve(FAIL('typst-error'));
    expect(await a).toMatchObject({ ok: false, kind: 'typst-error' });
    const b = memo();
    calls[1].resolve(OK());
    expect((await b).ok).toBe(true);
    expect(runner.listFonts).toHaveBeenCalledTimes(2);
  });

  it('a waiter whose shared job was replaced (superseded) while its run is current asks again, once', async () => {
    const { memo, calls, runner } = setup();
    const a = memo();
    calls[0].resolve(FAIL('superseded'));
    await vi.waitFor(() => expect(runner.listFonts).toHaveBeenCalledTimes(2));
    calls[1].resolve(OK(['R']));
    expect(await a).toMatchObject({ ok: true, families: ['R'] });
    // A second replacement is not retried again.
    const { memo: m2, calls: c2, runner: r2 } = setup();
    const b = m2();
    c2[0].resolve(FAIL('superseded'));
    await vi.waitFor(() => expect(r2.listFonts).toHaveBeenCalledTimes(2));
    c2[1].resolve(FAIL('superseded'));
    expect(await b).toMatchObject({ ok: false, kind: 'superseded' });
    expect(r2.listFonts).toHaveBeenCalledTimes(2);
  });

  it('a waiter whose own signal aborted is not retried', async () => {
    const { memo, calls, runner } = setup();
    const ac = new AbortController();
    const a = memo({ signal: ac.signal });
    ac.abort();
    expect(await a).toMatchObject({ kind: 'aborted' });
    calls[0].resolve(FAIL('superseded'));
    await Promise.resolve();
    expect(runner.listFonts).toHaveBeenCalledTimes(1);
  });

  it('forwards load progress from the shared job to each waiter, until it detaches', async () => {
    const { memo, calls } = setup();
    const seenA: number[] = [];
    const seenB: number[] = [];
    const acA = new AbortController();
    void memo({ signal: acA.signal, onLoadProgress: (p) => seenA.push(p.loaded) });
    const b = memo({ onLoadProgress: (p) => seenB.push(p.loaded) });
    calls[0].onLoadProgress?.({ phase: 'download', loaded: 1, total: null } as never);
    acA.abort();
    calls[0].onLoadProgress?.({ phase: 'download', loaded: 2, total: null } as never);
    expect([seenA, seenB]).toEqual([[1], [1, 2]]);
    calls[0].resolve(OK());
    await b;
  });
});

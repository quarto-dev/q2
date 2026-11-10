/**
 * `renderPageForPreview` (the q2 preview SPA's render entry) goes through
 * the serial render queue (preview VFS contract D9): renders never
 * overlap, a burst coalesces latest-wins, and the source text the render
 * read is captured when the render STARTS, inside its queue slot, and
 * returned with the result. A snapshot taken by the caller before
 * enqueueing could describe content the render never saw (a torn render).
 */

import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest';

const { state, mkDeferred } = vi.hoisted(() => {
  interface Deferred<T> {
    promise: Promise<T>;
    resolve: (value: T) => void;
  }
  function mkDeferred<T>(): Deferred<T> {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((res) => {
      resolve = res;
    });
    return { promise, resolve };
  }
  const state = {
    pending: [] as Deferred<string>[],
    active: 0,
    maxActive: 0,
    /** What `vfs_read_file` returns for the active document. */
    content: 'v1',
  };
  return { state, mkDeferred };
});

import { _setWasmModuleForTest, renderPageForPreview, RenderSupersededError } from './wasmRenderer';

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

beforeAll(() => {
  _setWasmModuleForTest({
    render_page_for_preview: vi.fn(() => {
      state.active += 1;
      state.maxActive = Math.max(state.maxActive, state.active);
      const d = mkDeferred<string>();
      state.pending.push(d);
      return d.promise.finally(() => {
        state.active -= 1;
      });
    }),
    vfs_read_file: () => JSON.stringify({ success: true, content: state.content }),
    vfs_take_read_set: () => 'null',
  });
});

beforeEach(() => {
  state.pending.length = 0;
  state.active = 0;
  state.maxActive = 0;
  state.content = 'v1';
});

describe('renderPageForPreview', () => {
  it('serializes and coalesces latest-wins, and reports the source each render started from', async () => {
    const p1 = renderPageForPreview('a.qmd');
    await flush();
    expect(state.pending.length).toBe(1);

    // Two edits arrive while the first render is in flight.
    state.content = 'v2';
    const p2 = renderPageForPreview('a.qmd');
    state.content = 'v3';
    const p3 = renderPageForPreview('a.qmd');
    const p2Settled = Promise.allSettled([p2]);
    await flush();
    expect(state.pending.length).toBe(1);

    state.pending[0].resolve(JSON.stringify({ success: true, ast_json: '{"n":1}' }));
    await expect(p1).resolves.toMatchObject({ success: true, renderedSource: 'v1' });
    await flush();
    expect(state.pending.length).toBe(2);

    state.pending[1].resolve(JSON.stringify({ success: true, ast_json: '{"n":3}' }));
    await expect(p3).resolves.toMatchObject({
      success: true,
      ast_json: '{"n":3}',
      renderedSource: 'v3',
    });
    const [settled] = await p2Settled;
    expect(settled.status).toBe('rejected');
    expect((settled as PromiseRejectedResult).reason).toBeInstanceOf(RenderSupersededError);
    expect(state.maxActive).toBe(1);
  });
});

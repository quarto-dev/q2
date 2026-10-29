/**
 * Characterization tests for the serial render queue in wasmRenderer.ts
 * (`enqueueRender`), written for bd-k8ptcrd9 (the 2026-10-24 main-thread
 * render-path plan, Phase 0). These pin the CURRENT behavior that plan's
 * phases build on:
 *
 * - renders execute strictly one at a time, in call order;
 * - EVERY enqueued render executes — there is no coalescing of queued
 *   work (the plan's finding 2: a burst of N updates runs N full
 *   pipelines). Phase 1 (latest-wins coalescing) will deliberately
 *   change this pin; update it when that lands;
 * - a rejected render reaches its own caller as a `{ success: false }`
 *   result and does not jam the queue for later renders.
 */

import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest';

const { state, mkDeferred } = vi.hoisted(() => {
  interface Deferred<T> {
    promise: Promise<T>;
    resolve: (value: T) => void;
    reject: (reason?: unknown) => void;
  }
  function mkDeferred<T>(): Deferred<T> {
    let resolve!: (value: T) => void;
    let reject!: (reason?: unknown) => void;
    const promise = new Promise<T>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    return { promise, resolve, reject };
  }
  const state = {
    /** Deferreds for renders the mock has STARTED (the WASM call is in flight). */
    pending: [] as Deferred<string>[],
    active: 0,
    maxActive: 0,
  };
  return { state, mkDeferred };
});

import { _setWasmModuleForTest, renderToHtml } from './wasmRenderer';

/** Let the queued microtasks + a macrotask fire so the serial queue advances. */
const flush = () => new Promise<void>((r) => setTimeout(r, 0));

beforeAll(() => {
  // Install the stub directly rather than mocking the
  // 'wasm-quarto-hub-client' import: with the stub in place, initWasm()
  // early-returns and the dynamic import (whose glue file only exists
  // after a WASM build, absent in the CI TS-suites job) never fires.
  _setWasmModuleForTest({
    render_page_in_project_with_attribution: vi.fn(() => {
      state.active += 1;
      state.maxActive = Math.max(state.maxActive, state.active);
      const d = mkDeferred<string>();
      state.pending.push(d);
      return d.promise.finally(() => {
        state.active -= 1;
      });
    }),
    // The CSS-version read after every successful render: a miss keeps
    // the 'default' version suffix.
    vfs_read_file: () => JSON.stringify({ success: false }),
  });
});

beforeEach(() => {
  state.pending.length = 0;
  state.active = 0;
  state.maxActive = 0;
});

describe('render serial queue (enqueueRender) characterization', () => {
  it('runs renders one at a time, starting them in call order as slots free up', async () => {
    const p1 = renderToHtml({ documentPath: 'a.qmd' });
    const p2 = renderToHtml({ documentPath: 'a.qmd' });
    const p3 = renderToHtml({ documentPath: 'a.qmd' });
    await flush();

    // Only the first render has started; the other two are queued.
    expect(state.pending.length).toBe(1);

    state.pending[0].resolve(JSON.stringify({ success: true, html: '<p>1</p>' }));
    await flush();
    expect(state.pending.length).toBe(2);

    state.pending[1].resolve(JSON.stringify({ success: true, html: '<p>2</p>' }));
    await flush();
    expect(state.pending.length).toBe(3);

    state.pending[2].resolve(JSON.stringify({ success: true, html: '<p>3</p>' }));
    const results = await Promise.all([p1, p2, p3]);

    // Never concurrent...
    expect(state.maxActive).toBe(1);
    // ...and each caller got its own render's output, in order.
    expect(results.map((r) => r.html)).toEqual([
      '<p>1</p><!-- css-version: default -->',
      '<p>2</p><!-- css-version: default -->',
      '<p>3</p><!-- css-version: default -->',
    ]);
  });

  it('executes every enqueued render — no coalescing of queued work (finding 2 pin)', async () => {
    // A burst of N content updates with a render already in flight: today
    // every queued render still runs to completion. Phase 1 of the plan
    // changes this to latest-wins; this test is the baseline it moves.
    const promises = Array.from({ length: 5 }, () => renderToHtml({ documentPath: 'a.qmd' }));
    await flush();

    // Drain the queue one slot at a time until all five have run.
    for (let started = 0; started < 5; started++) {
      await flush();
      expect(state.pending.length).toBe(started + 1);
      state.pending[started].resolve(JSON.stringify({ success: true, html: `<p>${started}</p>` }));
    }
    await Promise.all(promises);
    expect(state.pending.length).toBe(5);
  });

  it('a rejected render reports failure to its own caller and does not jam the queue', async () => {
    const p1 = renderToHtml({ documentPath: 'a.qmd' });
    const p2 = renderToHtml({ documentPath: 'a.qmd' });
    await flush();

    state.pending[0].reject(new Error('boom'));
    // The failing caller sees a success:false result (renderToHtmlInner's
    // catch), not a rejection.
    await expect(p1).resolves.toMatchObject({ success: false, error: 'boom' });

    // The queue is unjammed: the next render starts and can succeed.
    await flush();
    expect(state.pending.length).toBe(2);
    state.pending[1].resolve(JSON.stringify({ success: true, html: '<p>after</p>' }));
    await expect(p2).resolves.toMatchObject({ success: true });
  });
});

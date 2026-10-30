/**
 * Characterization tests for the serial render queue in wasmRenderer.ts
 * (`enqueueRender`), written for bd-k8ptcrd9 (the 2026-10-24 main-thread
 * render-path plan, Phase 0) and updated for bd-r8iqmg2f (Phase 1,
 * latest-wins coalescing). These pin the queue's contract:
 *
 * - renders execute strictly one at a time, in call order;
 * - the render IN FLIGHT is never superseded, and the first render
 *   after an idle queue starts immediately (no debounce: typing feel
 *   depends on both);
 * - QUEUED (not yet started) renders from the same entry point coalesce
 *   latest-wins: a burst of N content updates executes at most the
 *   in-flight render plus the latest queued one. (Phase 0 pinned the
 *   opposite — every enqueued render executed, the plan's finding 2 —
 *   and Phase 1 deliberately flipped it.);
 * - different render entry points serialize but never supersede each
 *   other;
 * - `renderPrintable` SERIALIZES with everything else but never
 *   coalesces: each print is an explicit user action that must run, so
 *   every call enqueues under a fresh key no waiting slot can match
 *   (bd-gj6fk5hc — it previously bypassed the queue entirely and could
 *   overlap a preview render on the non-reentrant VFS state);
 * - a superseded render settles by rejecting with RenderSupersededError
 *   (preview callers swallow it: it neither jams the queue nor
 *   surfaces), while a render whose EXECUTION rejects reaches its own
 *   caller as a `{ success: false }` result and does not jam the queue.
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

import {
  _setWasmModuleForTest,
  renderToHtml,
  renderPageInProjectWithAttribution,
  renderPrintable,
  RenderSupersededError,
} from './wasmRenderer';

/** Let the queued microtasks + a macrotask fire so the serial queue advances. */
const flush = () => new Promise<void>((r) => setTimeout(r, 0));

/** Narrow a settled result to its rejection reason for assertions. */
function rejectionOf(result: PromiseSettledResult<unknown>): unknown {
  expect(result.status).toBe('rejected');
  return (result as PromiseRejectedResult).reason;
}

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
    render_printable: vi.fn(() => {
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
    // Mixed entry points: renderToHtml and the ReactPreview path
    // (renderPageInProjectWithAttribution) have different coalesce keys,
    // so none of these supersede — they serialize.
    const p1 = renderToHtml({ documentPath: 'a.qmd' });
    const p2 = renderPageInProjectWithAttribution('a.qmd', undefined, null);
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
    // (renderToHtml appends the css-version comment; the raw
    // renderPageInProjectWithAttribution does not.)
    expect(results.map((r) => r.html)).toEqual([
      '<p>1</p><!-- css-version: default -->',
      '<p>2</p>',
      '<p>3</p><!-- css-version: default -->',
    ]);
  });

  it('coalesces a burst of same-entry renders latest-wins: only the in-flight render and the latest queued one execute (finding 2)', async () => {
    // Five content updates fire while the first render is still in
    // flight. Before Phase 1 all five executed; now the middle three
    // are superseded before they start and only the latest queued
    // render runs.
    const p1 = renderToHtml({ documentPath: 'a.qmd' });
    const p2 = renderToHtml({ documentPath: 'a.qmd' });
    const p3 = renderToHtml({ documentPath: 'a.qmd' });
    const p4 = renderToHtml({ documentPath: 'a.qmd' });
    const p5 = renderToHtml({ documentPath: 'a.qmd' });
    // Attach handlers now (same task) so a supersede rejection — which
    // happens synchronously during the superseding enqueue — is never
    // unhandled; assert on the settled shapes at the end.
    const middleSettled = Promise.allSettled([p2, p3, p4]);
    await flush();

    // Only the first render has started; the other four share ONE
    // waiting slot (each newer enqueue superseded the previous).
    expect(state.pending.length).toBe(1);

    state.pending[0].resolve(JSON.stringify({ success: true, html: '<p>first</p>' }));
    await flush();

    // Exactly one additional render started: the latest one.
    expect(state.pending.length).toBe(2);

    state.pending[1].resolve(JSON.stringify({ success: true, html: '<p>latest</p>' }));

    // The in-flight render and the latest render get their own results;
    // the final preview reflects the last content.
    await expect(p1).resolves.toMatchObject({
      success: true,
      html: '<p>first</p><!-- css-version: default -->',
    });
    await expect(p5).resolves.toMatchObject({
      success: true,
      html: '<p>latest</p><!-- css-version: default -->',
    });

    // The superseded three settled by rejecting with the tagged error.
    for (const settled of await middleSettled) {
      expect(rejectionOf(settled)).toBeInstanceOf(RenderSupersededError);
    }

    // Still never concurrent, and exactly two renders ran in total.
    expect(state.maxActive).toBe(1);
    expect(state.pending.length).toBe(2);
  });

  it('coalesces the ReactPreview render entry (renderPageInProjectWithAttribution) the same way', async () => {
    const p1 = renderPageInProjectWithAttribution('a.qmd', undefined, null);
    const p2 = renderPageInProjectWithAttribution('a.qmd', undefined, null);
    const p3 = renderPageInProjectWithAttribution('a.qmd', undefined, null);
    const p2Settled = Promise.allSettled([p2]);
    await flush();
    expect(state.pending.length).toBe(1);

    state.pending[0].resolve(JSON.stringify({ success: true, html: '<p>first</p>' }));
    await flush();
    expect(state.pending.length).toBe(2);

    state.pending[1].resolve(JSON.stringify({ success: true, ast_json: '{}' }));
    await expect(p1).resolves.toMatchObject({ success: true, html: '<p>first</p>' });
    await expect(p3).resolves.toMatchObject({ success: true, ast_json: '{}' });
    expect(rejectionOf((await p2Settled)[0])).toBeInstanceOf(RenderSupersededError);
    expect(state.maxActive).toBe(1);
  });

  it('a superseded render rejects without jamming the queue, and the first render after an idle queue starts immediately', async () => {
    const p1 = renderToHtml({ documentPath: 'a.qmd' });
    const p2 = renderToHtml({ documentPath: 'a.qmd' });
    const p3 = renderToHtml({ documentPath: 'a.qmd' }); // supersedes p2
    const p2Settled = Promise.allSettled([p2]);
    await flush();
    expect(state.pending.length).toBe(1);

    state.pending[0].resolve(JSON.stringify({ success: true, html: '<p>1</p>' }));
    await flush();
    // The superseding render took over the waiting slot and runs next.
    expect(state.pending.length).toBe(2);

    state.pending[1].resolve(JSON.stringify({ success: true, html: '<p>3</p>' }));
    await expect(p1).resolves.toMatchObject({ success: true });
    await expect(p3).resolves.toMatchObject({
      success: true,
      html: '<p>3</p><!-- css-version: default -->',
    });
    expect(rejectionOf((await p2Settled)[0])).toBeInstanceOf(RenderSupersededError);

    // The queue is now idle: the next render starts on its own, with no
    // debounce and no second enqueue to nudge it.
    const p4 = renderToHtml({ documentPath: 'a.qmd' });
    await flush();
    expect(state.pending.length).toBe(3);
    state.pending[2].resolve(JSON.stringify({ success: true, html: '<p>4</p>' }));
    await expect(p4).resolves.toMatchObject({ success: true });
  });

  it('serializes renderPrintable with the other render entries — a print fired mid-render waits its turn (bd-gj6fk5hc)', async () => {
    const p1 = renderToHtml({ documentPath: 'a.qmd' });
    const p2 = renderPrintable('a.qmd');
    const p3 = renderPageInProjectWithAttribution('a.qmd', undefined, null);
    await flush();

    // Only the first render has started; the print and the page render wait.
    expect(state.pending.length).toBe(1);

    state.pending[0].resolve(JSON.stringify({ success: true, html: '<p>1</p>' }));
    await flush();

    // FIFO: the print started next, the page render still waits.
    expect(state.pending.length).toBe(2);
    state.pending[1].resolve(JSON.stringify({ success: true, html: '<p>print</p>' }));
    await expect(p2).resolves.toMatchObject({ success: true, html: '<p>print</p>' });
    await flush();

    expect(state.pending.length).toBe(3);
    state.pending[2].resolve(JSON.stringify({ success: true, ast_json: '{}' }));
    await expect(p1).resolves.toMatchObject({ success: true });
    await expect(p3).resolves.toMatchObject({ success: true });

    // Never concurrent — the print never overlapped either preview render.
    expect(state.maxActive).toBe(1);
  });

  it('never coalesces renderPrintable: every explicit print executes', async () => {
    const p1 = renderPrintable('a.qmd');
    const p2 = renderPrintable('a.qmd');
    const p3 = renderPrintable('a.qmd');
    await flush();

    // Only the first print has started; the other two WAIT (they do
    // not supersede each other — each is an explicit user action).
    expect(state.pending.length).toBe(1);

    for (let i = 0; i < 3; i++) {
      state.pending[i].resolve(JSON.stringify({ success: true, html: `<p>${i}</p>` }));
      await flush();
    }

    await expect(p1).resolves.toMatchObject({ success: true, html: '<p>0</p>' });
    await expect(p2).resolves.toMatchObject({ success: true, html: '<p>1</p>' });
    await expect(p3).resolves.toMatchObject({ success: true, html: '<p>2</p>' });
    expect(state.pending.length).toBe(3);
    expect(state.maxActive).toBe(1);
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

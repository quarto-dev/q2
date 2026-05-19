/**
 * Regression tests for the q2-debug / q2-preview iframe message
 * dispatcher.
 *
 * The bug this guards against: when `LOAD_CUSTOM_COMPONENTS` is in
 * flight and two `UPDATE_AST` messages arrive while components are
 * still loading, the previous setInterval(check, 50) polling pattern
 * could resolve the two waiters out of arrival order — each waiter
 * had its own setInterval phase, so depending on when
 * `componentsLoading` flipped, the second-arrived waiter could fire
 * first and the first-arrived waiter would overwrite it. In the
 * attribution-pipeline branch this manifested as the no-attribution
 * AST overwriting the with-attribution AST, so attribution colouring
 * never appeared by default for large files with `render-components:
 * - html.tsx`.
 *
 * The fix replaces the polling with a single shared promise that all
 * UPDATE_AST waiters `await`. Microtask continuations on one promise
 * resolve in FIFO insertion order, so message arrival order is
 * preserved deterministically.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  makeIframeMessageDispatcher,
  type IframeMessage,
} from './iframeMessageDispatch';

interface DeferredPromise<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}

function deferred<T>(): DeferredPromise<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

describe('makeIframeMessageDispatcher', () => {
  it('runs UPDATE_AST immediately when no LOAD_CUSTOM_COMPONENTS has fired', async () => {
    const updateAst = vi.fn();
    const dispatch = makeIframeMessageDispatcher({
      loadCustomComponents: vi.fn(),
      updateAst,
    });

    await dispatch({ type: 'UPDATE_AST', payload: { astJson: 'A' } });
    expect(updateAst).toHaveBeenCalledOnce();
    expect(updateAst).toHaveBeenCalledWith({ astJson: 'A' });
  });

  it('runs UPDATE_AST immediately after LOAD_CUSTOM_COMPONENTS has settled', async () => {
    const updateAst = vi.fn();
    const dispatch = makeIframeMessageDispatcher({
      loadCustomComponents: () => Promise.resolve(),
      updateAst,
    });

    await dispatch({
      type: 'LOAD_CUSTOM_COMPONENTS',
      componentsCode: { 'html.tsx': '' },
    });
    await dispatch({ type: 'UPDATE_AST', payload: { astJson: 'A' } });
    await dispatch({ type: 'UPDATE_AST', payload: { astJson: 'B' } });

    expect(updateAst.mock.calls.map((c) => c[0])).toEqual([
      { astJson: 'A' },
      { astJson: 'B' },
    ]);
  });

  it('defers UPDATE_AST while LOAD_CUSTOM_COMPONENTS is in flight', async () => {
    const updateAst = vi.fn();
    const load = deferred<void>();
    const dispatch = makeIframeMessageDispatcher({
      loadCustomComponents: () => load.promise,
      updateAst,
    });

    // Fire-and-forget the load (handler awaits internally).
    const loadHandler = dispatch({
      type: 'LOAD_CUSTOM_COMPONENTS',
      componentsCode: { 'html.tsx': '' },
    });
    // Let the LOAD handler enter its `await`.
    await Promise.resolve();

    const u1 = dispatch({ type: 'UPDATE_AST', payload: { astJson: 'A' } });
    const u2 = dispatch({ type: 'UPDATE_AST', payload: { astJson: 'B' } });
    // Let both UPDATE_AST handlers enter their `await`s.
    await Promise.resolve();

    // Neither should have called updateAst yet.
    expect(updateAst).not.toHaveBeenCalled();

    // Release the load. Both waiters wake; their `updateAst` calls
    // should run in arrival order (A then B), not in the order the
    // 50ms setInterval phases happened to fire.
    load.resolve();
    await Promise.all([loadHandler, u1, u2]);

    expect(updateAst.mock.calls.map((c) => c[0])).toEqual([
      { astJson: 'A' },
      { astJson: 'B' },
    ]);
  });

  it('preserves FIFO across three pending UPDATE_AST messages', async () => {
    // Three waiters exercise the chain harder than two — the
    // continuation-list bug would have surfaced as any permutation,
    // not strictly a swap.
    const updateAst = vi.fn();
    const load = deferred<void>();
    const dispatch = makeIframeMessageDispatcher({
      loadCustomComponents: () => load.promise,
      updateAst,
    });

    const loadHandler = dispatch({
      type: 'LOAD_CUSTOM_COMPONENTS',
      componentsCode: { 'html.tsx': '' },
    });
    await Promise.resolve();

    const handlers = [
      dispatch({ type: 'UPDATE_AST', payload: { astJson: 'A' } }),
      dispatch({ type: 'UPDATE_AST', payload: { astJson: 'B' } }),
      dispatch({ type: 'UPDATE_AST', payload: { astJson: 'C' } }),
    ];
    await Promise.resolve();

    load.resolve();
    await Promise.all([loadHandler, ...handlers]);

    expect(updateAst.mock.calls.map((c) => c[0])).toEqual([
      { astJson: 'A' },
      { astJson: 'B' },
      { astJson: 'C' },
    ]);
  });

  it('routes UPDATE_THEME to the applyTheme hook when provided', async () => {
    const applyTheme = vi.fn();
    const dispatch = makeIframeMessageDispatcher({
      loadCustomComponents: vi.fn(),
      updateAst: vi.fn(),
      applyTheme,
    });

    await dispatch({ type: 'UPDATE_THEME', cssUrl: 'blob:abc' });
    await dispatch({ type: 'UPDATE_THEME', cssUrl: null });

    expect(applyTheme.mock.calls.map((c) => c[0])).toEqual(['blob:abc', null]);
  });

  it('ignores UPDATE_THEME when applyTheme is not provided (q2-debug case)', async () => {
    const updateAst = vi.fn();
    const dispatch = makeIframeMessageDispatcher({
      loadCustomComponents: vi.fn(),
      updateAst,
    });

    // Should be a no-op, not a throw.
    await expect(
      dispatch({ type: 'UPDATE_THEME', cssUrl: 'blob:abc' } as IframeMessage),
    ).resolves.toBeUndefined();
    expect(updateAst).not.toHaveBeenCalled();
  });
});

/**
 * Sanity-check that the *old* setInterval(check, 50) polling pattern
 * can resolve UPDATE_AST waiters out of arrival order under
 * deterministic fake-timer scheduling — i.e. that the bug actually
 * existed and we're not chasing a non-issue. This test does NOT
 * exercise the production code; it inlines the old algorithm in
 * miniature.
 *
 * The chosen schedule (load at t=0, A queued at t=0, B queued at
 * t=60, componentsLoading flipped at t=105) reproduces the canonical
 * failure: A's setInterval fires at 50, 100 (both see
 * componentsLoading=true), then 150; B's setInterval fires at 110
 * (sees false, resolves) before A's 150 fire. The resulting order
 * is B-then-A, not A-then-B.
 *
 * Anchoring the regression with a deterministic reproduction of the
 * old behaviour means a future reader doesn't have to take the
 * commit message's word that the race was real.
 */
describe('legacy setInterval-polling pattern (documents the bug)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('resolves UPDATE_AST waiters out of arrival order under unlucky timing', async () => {
    const updateAstCalls: string[] = [];
    let componentsLoading = false;

    const oldDispatch = async (
      msg:
        | { type: 'LOAD_CUSTOM_COMPONENTS'; load: Promise<void> }
        | { type: 'UPDATE_AST'; payload: string },
    ) => {
      if (msg.type === 'LOAD_CUSTOM_COMPONENTS') {
        componentsLoading = true;
        await msg.load;
        componentsLoading = false;
      } else {
        if (componentsLoading) {
          await new Promise<void>((resolve) => {
            const check = setInterval(() => {
              if (!componentsLoading) {
                clearInterval(check);
                resolve();
              }
            }, 50);
          });
        }
        updateAstCalls.push(msg.payload);
      }
    };

    const load = deferred<void>();
    // t=0
    const loadHandler = oldDispatch({
      type: 'LOAD_CUSTOM_COMPONENTS',
      load: load.promise,
    });
    await Promise.resolve(); // let LOAD handler enter await

    // A's setInterval is registered at t=0.
    const uA = oldDispatch({ type: 'UPDATE_AST', payload: 'A' });
    await Promise.resolve();

    // Advance fake time to t=60, then register B's setInterval.
    await vi.advanceTimersByTimeAsync(60);
    const uB = oldDispatch({ type: 'UPDATE_AST', payload: 'B' });
    await Promise.resolve();

    // Advance to t=105 and flip componentsLoading to false.
    // (Phase: A's fires at 50, 100, 150; B's at 110, 160.)
    await vi.advanceTimersByTimeAsync(45);
    load.resolve();
    await Promise.resolve();
    await Promise.resolve();

    // Advance to t=160 so both intervals have had a chance to fire.
    await vi.advanceTimersByTimeAsync(55);

    await Promise.all([loadHandler, uA, uB]);

    // The bug: B resolves at 110, A at 150 → updateAst is called in
    // the wrong order. If this assertion ever flips (e.g. a future
    // browser/JS engine changes the setInterval scheduling
    // semantics), the comment block above also needs updating.
    expect(updateAstCalls).toEqual(['B', 'A']);
  });
});

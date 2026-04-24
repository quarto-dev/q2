/**
 * Visibility test helpers for jsdom.
 *
 * jsdom exposes `document.visibilityState` and `document.hidden` as read-only
 * getters on `Document.prototype`. We use `vi.spyOn` on those getters so the
 * spies are auto-restored by vitest's `vi.restoreAllMocks()` teardown — no
 * `Object.defineProperty` surgery that could leak across test files in the
 * same vitest worker.
 *
 * Always call `resetVisibility()` in `afterEach` of any file that calls
 * `setVisibility()` — paired with `vi.restoreAllMocks()` for belt-and-braces.
 */

import { vi } from 'vitest';

type GetterSpy = ReturnType<typeof vi.spyOn>;

let visibilitySpy: GetterSpy | null = null;
let hiddenSpy: GetterSpy | null = null;

export function setVisibility(state: 'visible' | 'hidden'): void {
  if (!visibilitySpy) {
    visibilitySpy = vi.spyOn(document, 'visibilityState', 'get');
    hiddenSpy = vi.spyOn(document, 'hidden', 'get');
  }
  (visibilitySpy as unknown as { mockReturnValue: (v: unknown) => void }).mockReturnValue(state);
  (hiddenSpy as unknown as { mockReturnValue: (v: unknown) => void }).mockReturnValue(state === 'hidden');
  document.dispatchEvent(new Event('visibilitychange'));
}

export function resetVisibility(): void {
  visibilitySpy?.mockRestore();
  hiddenSpy?.mockRestore();
  visibilitySpy = null;
  hiddenSpy = null;
}

export function fireWindowFocus(): void {
  window.dispatchEvent(new Event('focus'));
}

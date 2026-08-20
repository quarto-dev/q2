/**
 * Tests for the pwaPrompt store (GH #447, bd-axqunnx9): the tiny
 * module-level bridge between the DOM-free update flow in pwa.ts and
 * the UpdateAvailableToast React component. The pending flag matters
 * because setupSwUpdates runs before React mounts — a show() fired
 * before the toast subscribes must not be lost.
 */

import { describe, it, expect, vi } from 'vitest';
import { createPwaPromptStore } from './pwaPrompt';

describe('createPwaPromptStore', () => {
  it('starts with no pending prompt', () => {
    expect(createPwaPromptStore().isPending()).toBe(false);
  });

  it('marks the prompt pending and notifies subscribers on show()', () => {
    const store = createPwaPromptStore();
    const listener = vi.fn();
    store.subscribe(listener);
    store.show();
    expect(store.isPending()).toBe(true);
    expect(listener).toHaveBeenCalledOnce();
  });

  it('keeps a show() fired before anyone subscribed', () => {
    const store = createPwaPromptStore();
    store.show();
    const listener = vi.fn();
    store.subscribe(listener);
    expect(listener).not.toHaveBeenCalled();
    expect(store.isPending()).toBe(true);
  });

  it('is idempotent — a second show() does not re-notify', () => {
    const store = createPwaPromptStore();
    const listener = vi.fn();
    store.subscribe(listener);
    store.show();
    store.show();
    expect(listener).toHaveBeenCalledOnce();
  });

  it('stops notifying after unsubscribe', () => {
    const store = createPwaPromptStore();
    const listener = vi.fn();
    const unsubscribe = store.subscribe(listener);
    unsubscribe();
    store.show();
    expect(listener).not.toHaveBeenCalled();
  });
});

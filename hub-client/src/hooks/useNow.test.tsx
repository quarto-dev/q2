/**
 * Tests for useNow — the shared ticking clock on useSyncExternalStore.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { renderHook, act, cleanup } from '@testing-library/react';
import { useNow } from './useNow';

describe('useNow', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it('returns the current time on mount', () => {
    const { result } = renderHook(() => useNow(1000));
    expect(result.current).toBe(Date.now());
  });

  it('advances on each interval tick', () => {
    const { result } = renderHook(() => useNow(1000));
    const t0 = result.current;
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(result.current).toBe(t0 + 3000);
  });

  it('shares one ticking store across consumers of the same interval', () => {
    const first = renderHook(() => useNow(1000));
    const second = renderHook(() => useNow(1000));
    expect(vi.getTimerCount()).toBe(1);

    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(first.result.current).toBe(second.result.current);

    // The interval survives until the last consumer unsubscribes.
    first.unmount();
    expect(vi.getTimerCount()).toBe(1);
    second.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('refreshes the clock on resubscription after an idle spell', () => {
    const mounted = renderHook(() => useNow(1000));
    mounted.unmount();
    expect(vi.getTimerCount()).toBe(0);

    // Time passes with no subscribers; a fresh mount must not see the
    // stale cached timestamp.
    vi.advanceTimersByTime(60_000);
    const remounted = renderHook(() => useNow(1000));
    expect(remounted.result.current).toBe(Date.now());
  });
});

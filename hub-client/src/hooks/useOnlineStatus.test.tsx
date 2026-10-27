/**
 * Tests for useOnlineStatus — navigator.onLine as a subscription.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach, vi } from 'vitest';
import { renderHook, act, cleanup } from '@testing-library/react';
import { useOnlineStatus } from './useOnlineStatus';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('useOnlineStatus', () => {
  it('reflects navigator.onLine', () => {
    vi.spyOn(navigator, 'onLine', 'get').mockReturnValue(true);
    const { result } = renderHook(() => useOnlineStatus());
    expect(result.current).toBe(true);
  });

  it('re-renders on offline/online events', () => {
    const onLine = vi.spyOn(navigator, 'onLine', 'get').mockReturnValue(true);
    const { result } = renderHook(() => useOnlineStatus());

    onLine.mockReturnValue(false);
    act(() => {
      window.dispatchEvent(new Event('offline'));
    });
    expect(result.current).toBe(false);

    onLine.mockReturnValue(true);
    act(() => {
      window.dispatchEvent(new Event('online'));
    });
    expect(result.current).toBe(true);
  });
});

/**
 * Tests for useExecutionChannel (bd-sfet3264, Phase 2D).
 *
 * Verifies the lifecycle glue: while connected, an injected capability beacon
 * surfaces as a live executor; disconnecting tears the channel down.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

// A fake index DocHandle the mocked getIndexHandle returns.
const fake = (() => {
  const handlers = new Set<(p: { message: unknown }) => void>();
  return {
    handle: {
      broadcast: vi.fn(),
      on: (_e: string, h: (p: { message: unknown }) => void) => handlers.add(h),
      off: (_e: string, h: (p: { message: unknown }) => void) => handlers.delete(h),
    },
    inject: (message: unknown) => handlers.forEach((h) => h({ message })),
    handlerCount: () => handlers.size,
  };
})();

vi.mock('@quarto/preview-runtime', () => ({
  getIndexHandle: () => fake.handle,
}));

import { useExecutionChannel } from './useExecutionChannel';

describe('useExecutionChannel (Phase 2D)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    fake.handle.broadcast.mockClear();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns no executors when offline and does not subscribe', () => {
    const { result } = renderHook(() => useExecutionChannel(false, 'idx-1'));
    expect(result.current.executors).toEqual([]);
    expect(fake.handlerCount()).toBe(0);
  });

  it('surfaces a live executor from an injected beacon while connected', () => {
    const { result } = renderHook(() => useExecutionChannel(true, 'idx-1'));
    expect(fake.handlerCount()).toBe(1);

    act(() => {
      fake.inject({ kind: 'exec/beacon', actorId: 'exec-1', engines: ['knitr'], generation: 0 });
    });

    expect(result.current.executors).toHaveLength(1);
    expect(result.current.executors[0]).toMatchObject({ actorId: 'exec-1', engines: ['knitr'] });
  });

  it('requestExecution broadcasts an exec/request while connected', () => {
    const { result } = renderHook(() => useExecutionChannel(true, 'idx-1'));

    let requestId: string | null = null;
    act(() => {
      requestId = result.current.requestExecution('doc.qmd');
    });

    expect(requestId).toBeTruthy();
    expect(fake.handle.broadcast).toHaveBeenCalledWith(
      expect.objectContaining({ kind: 'exec/request', path: 'doc.qmd' }),
    );
  });

  it('requestExecution returns null when offline', () => {
    const { result } = renderHook(() => useExecutionChannel(false, 'idx-1'));
    expect(result.current.requestExecution('doc.qmd')).toBeNull();
    expect(fake.handle.broadcast).not.toHaveBeenCalled();
  });

  it('tears the channel down on unmount', () => {
    const { unmount } = renderHook(() => useExecutionChannel(true, 'idx-1'));
    expect(fake.handlerCount()).toBe(1);
    unmount();
    expect(fake.handlerCount()).toBe(0);
  });
});

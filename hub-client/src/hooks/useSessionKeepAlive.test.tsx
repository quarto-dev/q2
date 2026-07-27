/**
 * Unit tests for useSessionKeepAlive (bd-exk3hfxk, sliding sessions C6).
 *
 * With hub-minted sliding sessions the server re-issues the session
 * cookie on authenticated activity — but WebSocket traffic never
 * qualifies (validate-once at upgrade), so a WS-only client would idle
 * out despite being "active". This hook owns the keep-alive: a periodic
 * GET /auth/me while signed in with sync online. The slide requires no
 * IdP round-trip at all — these tests mock only fetchAuthMe. A definitive
 * rejection ends the session (`onAuthRejected`); the One-Tap silent-renewal
 * fallback was retired (bd-s042qcxj).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

vi.mock('../services/authService', () => ({
  fetchAuthMe: vi.fn(),
}));

import { useSessionKeepAlive, SESSION_KEEP_ALIVE_INTERVAL_MS } from './useSessionKeepAlive';
import { fetchAuthMe } from '../services/authService';

const mockFetchAuthMe = vi.mocked(fetchAuthMe);
const me = (expiresAt: number) => ({
  email: 'a@b.com',
  name: 'A',
  picture: null,
  expiresAt,
});

describe('useSessionKeepAlive', () => {
  let onAuthState: ReturnType<typeof vi.fn>;
  let onAuthRejected: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockFetchAuthMe.mockReset();
    onAuthState = vi.fn();
    onAuthRejected = vi.fn();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  const render = (enabled: boolean) =>
    renderHook(
      ({ on }: { on: boolean }) =>
        useSessionKeepAlive({ enabled: on, onAuthState, onAuthRejected }),
      { initialProps: { on: enabled } },
    );

  it('does not probe while disabled', async () => {
    render(false);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_KEEP_ALIVE_INTERVAL_MS * 2);
    });
    expect(mockFetchAuthMe).not.toHaveBeenCalled();
  });

  it('keeps the session sliding while enabled: periodic probes report the fresh expiry', async () => {
    // Server slides exp on each qualifying request; the client must
    // pick the new expiry up so its schedules follow the session.
    mockFetchAuthMe
      .mockResolvedValueOnce(me(1_000_000))
      .mockResolvedValueOnce(me(2_000_000));
    render(true);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(mockFetchAuthMe).toHaveBeenCalledTimes(1);
    expect(onAuthState).toHaveBeenLastCalledWith(me(1_000_000));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_KEEP_ALIVE_INTERVAL_MS);
    });
    expect(mockFetchAuthMe).toHaveBeenCalledTimes(2);
    expect(onAuthState).toHaveBeenLastCalledWith(me(2_000_000));
    // The whole slide happened without any IdP involvement.
    expect(onAuthRejected).not.toHaveBeenCalled();
  });

  it('ends the session on a definitive rejection', async () => {
    // e.g. the session was revoked via logout-everywhere on another
    // device, or hit the absolute cap: the session is over and the user
    // re-logs-in through the GIS button (no silent renewal any more).
    mockFetchAuthMe.mockResolvedValue(null);
    render(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(onAuthRejected).toHaveBeenCalledTimes(1);
    expect(onAuthState).not.toHaveBeenCalled();
  });

  it('takes no action on network errors (offline editing must survive)', async () => {
    mockFetchAuthMe.mockRejectedValue(new Error('offline'));
    render(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_KEEP_ALIVE_INTERVAL_MS);
    });
    expect(onAuthRejected).not.toHaveBeenCalled();
    expect(onAuthState).not.toHaveBeenCalled();
  });

  it('stops probing when disabled mid-flight (WS closed / signed out)', async () => {
    mockFetchAuthMe.mockResolvedValue(me(1_000_000));
    const { rerender } = render(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(mockFetchAuthMe).toHaveBeenCalledTimes(1);

    rerender({ on: false });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SESSION_KEEP_ALIVE_INTERVAL_MS * 2);
    });
    expect(mockFetchAuthMe).toHaveBeenCalledTimes(1);
  });
});

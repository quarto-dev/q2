/**
 * Unit tests for useAuthProbe (bd-3o8zmz46).
 *
 * The probe runs while sync is disconnected and decides, on evidence only,
 * whether the disconnect is an auth failure. Offline-mode invariant: network
 * errors never trigger any action.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

vi.mock('../services/authService', () => ({
  fetchAuthMe: vi.fn(),
}));

import { useAuthProbe, AUTH_PROBE_INTERVAL_MS } from './useAuthProbe';
import { fetchAuthMe } from '../services/authService';

const mockFetchAuthMe = vi.mocked(fetchAuthMe);
const user = { email: 'a@b.com', name: 'A', picture: null };

describe('useAuthProbe', () => {
  let triggerRefresh: ReturnType<typeof vi.fn>;
  let onAuthRejected: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockFetchAuthMe.mockReset();
    triggerRefresh = vi.fn();
    onAuthRejected = vi.fn();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  const render = (enabled: boolean) =>
    renderHook(
      ({ on }: { on: boolean }) =>
        useAuthProbe({ enabled: on, triggerRefresh, onAuthRejected }),
      { initialProps: { on: enabled } },
    );

  it('does not probe while disabled', async () => {
    render(false);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(AUTH_PROBE_INTERVAL_MS * 2);
    });
    expect(mockFetchAuthMe).not.toHaveBeenCalled();
  });

  it('probes immediately and then on an interval while enabled', async () => {
    mockFetchAuthMe.mockResolvedValue(user);
    render(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(mockFetchAuthMe).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(AUTH_PROBE_INTERVAL_MS);
    });
    expect(mockFetchAuthMe).toHaveBeenCalledTimes(2);
    expect(triggerRefresh).not.toHaveBeenCalled();
    expect(onAuthRejected).not.toHaveBeenCalled();
  });

  it('takes no action on network errors (offline mode)', async () => {
    mockFetchAuthMe.mockRejectedValue(new Error('network'));
    render(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(AUTH_PROBE_INTERVAL_MS * 3);
    });
    expect(mockFetchAuthMe).toHaveBeenCalled();
    expect(triggerRefresh).not.toHaveBeenCalled();
    expect(onAuthRejected).not.toHaveBeenCalled();
  });

  it('first rejection triggers renewal; second consecutive rejection rejects auth', async () => {
    mockFetchAuthMe.mockResolvedValue(null);
    render(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(triggerRefresh).toHaveBeenCalledTimes(1);
    expect(onAuthRejected).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(AUTH_PROBE_INTERVAL_MS);
    });
    expect(onAuthRejected).toHaveBeenCalledTimes(1);
  });

  it('a successful probe between rejections resets the strike counter', async () => {
    mockFetchAuthMe
      .mockResolvedValueOnce(null) // strike 1 → renewal
      .mockResolvedValueOnce(user) // renewal restored the session → reset
      .mockResolvedValueOnce(null); // strike 1 again → renewal, not rejection
    render(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(AUTH_PROBE_INTERVAL_MS);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(AUTH_PROBE_INTERVAL_MS);
    });
    expect(triggerRefresh).toHaveBeenCalledTimes(2);
    expect(onAuthRejected).not.toHaveBeenCalled();
  });

  it('stops probing when disabled (e.g. reconnected)', async () => {
    mockFetchAuthMe.mockResolvedValue(user);
    const { rerender } = render(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    const calls = mockFetchAuthMe.mock.calls.length;
    rerender({ on: false });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(AUTH_PROBE_INTERVAL_MS * 2);
    });
    expect(mockFetchAuthMe).toHaveBeenCalledTimes(calls);
  });
});

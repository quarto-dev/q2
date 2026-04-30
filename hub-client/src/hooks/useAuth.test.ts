/**
 * Unit Tests for useAuth hook
 *
 * Tests mount behavior, refresh scheduling, logout, and expiry logic.
 * Uses fake timers and mocked authService / Google OAuth.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor, cleanup } from '@testing-library/react';

// Track the One Tap callback so tests can invoke it manually.
let oneTapCallbacks: {
  onSuccess?: (response: { credential?: string }) => void;
  onError?: () => void;
  disabled?: boolean;
};

vi.mock('@react-oauth/google', () => ({
  useGoogleOneTapLogin: (opts: typeof oneTapCallbacks) => {
    oneTapCallbacks = opts;
  },
}));

vi.mock('../services/authService', () => ({
  fetchAuthMe: vi.fn(),
  logout: vi.fn(),
  refreshToken: vi.fn(),
}));

import { useAuth, REFRESH_BUFFER_MS } from './useAuth';
import {
  fetchAuthMe,
  logout as serverLogout,
  refreshToken,
} from '../services/authService';

const COOKIE_MAX_AGE_MS = 3600 * 1000;

const mockFetchAuthMe = vi.mocked(fetchAuthMe);
const mockServerLogout = vi.mocked(serverLogout);
const mockRefreshToken = vi.mocked(refreshToken);

describe('useAuth', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    oneTapCallbacks = {};
    mockServerLogout.mockResolvedValue();
  });

  // ── Mount behavior (real timers — waitFor needs them) ─────

  describe('mount', () => {
    it('starts in loading state', () => {
      mockFetchAuthMe.mockReturnValue(new Promise(() => {})); // never resolves
      const { result } = renderHook(() => useAuth());

      expect(result.current.loading).toBe(true);
      expect(result.current.auth).toBeNull();
    });

    it('sets auth on successful /auth/me', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth());
      await waitFor(() => expect(result.current.loading).toBe(false));

      expect(result.current.auth).toEqual(user);
    });

    it('sets auth to null on 401 (not authenticated)', async () => {
      mockFetchAuthMe.mockResolvedValue(null);

      const { result } = renderHook(() => useAuth());
      await waitFor(() => expect(result.current.loading).toBe(false));

      expect(result.current.auth).toBeNull();
    });

    it('sets auth to null on fetch error', async () => {
      mockFetchAuthMe.mockRejectedValue(new Error('network'));

      const { result } = renderHook(() => useAuth());
      await waitFor(() => expect(result.current.loading).toBe(false));

      expect(result.current.auth).toBeNull();
    });
  });

  // ── Logout (real timers) ──────────────────────────────────

  describe('logout', () => {
    it('clears auth state and calls server logout', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth());
      await waitFor(() => expect(result.current.auth).toEqual(user));

      act(() => {
        result.current.logout();
      });

      expect(result.current.auth).toBeNull();
      expect(mockServerLogout).toHaveBeenCalled();
    });

    it('clears auth even if server logout fails', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);
      mockServerLogout.mockRejectedValue(new Error('offline'));

      const { result } = renderHook(() => useAuth());
      await waitFor(() => expect(result.current.auth).toEqual(user));

      act(() => {
        result.current.logout();
      });

      expect(result.current.auth).toBeNull();
    });
  });

  // ── Refresh scheduling (fake timers) ──────────────────────

  describe('refresh scheduling', () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('starts with Google One Tap disabled', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      renderHook(() => useAuth());
      await vi.waitFor(() => expect(mockFetchAuthMe).toHaveBeenCalled());

      // Before any time has passed, One Tap should be disabled
      expect(oneTapCallbacks.disabled).toBe(true);
    });

    it('updates auth on successful One Tap refresh', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      const refreshedUser = {
        email: 'a@b.com',
        name: 'A Updated',
        picture: null,
      };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() =>
        expect(result.current.auth).toEqual(user),
      );

      // Simulate One Tap returning a credential
      mockRefreshToken.mockResolvedValue(refreshedUser);
      await act(async () => {
        oneTapCallbacks.onSuccess?.({ credential: 'fresh.jwt.token' });
      });

      await vi.waitFor(() =>
        expect(result.current.auth).toEqual(refreshedUser),
      );
      expect(mockRefreshToken).toHaveBeenCalledWith('fresh.jwt.token');
    });
  });

  // ── Visibility-triggered refresh (fake timers) ────────────

  describe('visibility-triggered refresh', () => {
    beforeEach(() => {
      // Unmount hooks from previous tests so their visibilitychange
      // listeners don't interfere with ours.
      cleanup();
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('attempts One Tap refresh when tab becomes visible with expired cookie', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      const refreshedUser = { email: 'a@b.com', name: 'Refreshed', picture: null };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Next /auth/me returns null (cookie expired)
      mockFetchAuthMe.mockResolvedValueOnce(null);
      mockRefreshToken.mockResolvedValue(refreshedUser);

      // Dispatch the event, then flush async handler + React updates separately.
      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // One Tap should be enabled for refresh
      expect(oneTapCallbacks.disabled).toBe(false);

      // Simulate One Tap returning a fresh credential
      await act(async () => {
        oneTapCallbacks.onSuccess?.({ credential: 'fresh.jwt' });
      });

      await vi.waitFor(() => expect(result.current.auth).toEqual(refreshedUser));
    });

    it('clears auth when One Tap fails after visibility-triggered refresh', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Jump Date.now() past cookie lifetime (simulates long idle)
      vi.setSystemTime(Date.now() + 3600 * 1000 + 1000);

      // Next /auth/me returns null (cookie expired)
      mockFetchAuthMe.mockResolvedValueOnce(null);

      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // Simulate One Tap failing (no active Google session)
      await act(async () => {
        oneTapCallbacks.onError?.();
      });

      await vi.waitFor(() => expect(result.current.auth).toBeNull());
    });

    it('clears auth when refreshToken returns null after visibility-triggered refresh', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Jump Date.now() past cookie lifetime (simulates long idle)
      vi.setSystemTime(Date.now() + 3600 * 1000 + 1000);

      // Next /auth/me returns null (cookie expired)
      mockFetchAuthMe.mockResolvedValueOnce(null);

      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // One Tap returns a credential, but server rejects it
      mockRefreshToken.mockResolvedValue(null);
      await act(async () => {
        oneTapCallbacks.onSuccess?.({ credential: 'rejected.jwt' });
      });

      await vi.waitFor(() => expect(result.current.auth).toBeNull());
    });
  });

  // ── Hard expiry (fake timers) ─────────────────────────────

  describe('hard expiry', () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('clears auth when cookie expires and no refresh in progress', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe
        .mockResolvedValueOnce(user) // mount check
        .mockResolvedValueOnce(null); // expiry re-check

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() =>
        expect(result.current.auth).toEqual(user),
      );

      // Advance past the refresh point. The refresh timer sets
      // isRefreshing=true and enables One Tap. Simulate One Tap failing
      // (no active Google session), which resets isRefreshing=false.
      await act(async () => {
        vi.advanceTimersByTime(COOKIE_MAX_AGE_MS - REFRESH_BUFFER_MS + 100);
      });
      await act(async () => {
        oneTapCallbacks.onError?.();
      });

      // Advance past cookie max-age (remaining buffer).
      await act(async () => {
        vi.advanceTimersByTime(REFRESH_BUFFER_MS + 100);
      });

      await vi.waitFor(() =>
        expect(result.current.auth).toBeNull(),
      );
    });

    it('keeps auth if server confirms valid cookie at expiry', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      const freshUser = {
        email: 'a@b.com',
        name: 'Still Valid',
        picture: null,
      };
      mockFetchAuthMe
        .mockResolvedValueOnce(user) // mount check
        .mockResolvedValueOnce(freshUser); // expiry re-check (refresh succeeded)

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() =>
        expect(result.current.auth).toEqual(user),
      );

      // Advance past cookie max-age
      await act(async () => {
        vi.advanceTimersByTime(3600 * 1000 + 100);
      });

      await vi.waitFor(() =>
        expect(result.current.auth).toEqual(freshUser),
      );
    });
  });

  // ── 401-triggered refresh (fake timers) ───────────────────

  describe('triggerRefresh', () => {
    beforeEach(() => {
      cleanup();
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('enables One Tap when called while auth is still considered valid', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Cookie freshly set on mount — One Tap initially disabled.
      expect(oneTapCallbacks.disabled).toBe(true);

      act(() => {
        result.current.triggerRefresh();
      });

      expect(oneTapCallbacks.disabled).toBe(false);
    });

    it('coalesces concurrent triggerRefresh() calls into a single One Tap attempt', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Multiple concurrent calls (e.g. parallel fetchActorId 401s).
      act(() => {
        result.current.triggerRefresh();
        result.current.triggerRefresh();
        result.current.triggerRefresh();
      });

      // One Tap is enabled exactly once; refreshEnabled stayed true.
      expect(oneTapCallbacks.disabled).toBe(false);

      // Resolve the One Tap success path; isRefreshing resets, One Tap disables.
      mockRefreshToken.mockResolvedValue(user);
      await act(async () => {
        oneTapCallbacks.onSuccess?.({ credential: 'fresh.jwt' });
      });
      await vi.waitFor(() => expect(oneTapCallbacks.disabled).toBe(true));

      // A subsequent triggerRefresh re-enables (proving the gate cleared).
      act(() => {
        result.current.triggerRefresh();
      });
      expect(oneTapCallbacks.disabled).toBe(false);
    });

    it('updates auth and refreshes cookieSetAt on successful One Tap', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      const refreshedUser = {
        email: 'a@b.com',
        name: 'A Refreshed',
        picture: null,
      };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Pretend cookie has aged most of the way to expiry. After successful
      // refresh, cookieSetAt should be reset to "now" so the next refresh
      // timer fires ~ (max-age - buffer) from this moment, not immediately.
      await act(async () => {
        vi.advanceTimersByTime(COOKIE_MAX_AGE_MS - REFRESH_BUFFER_MS - 60_000);
      });

      act(() => {
        result.current.triggerRefresh();
      });

      mockRefreshToken.mockResolvedValue(refreshedUser);
      await act(async () => {
        oneTapCallbacks.onSuccess?.({ credential: 'fresh.jwt' });
      });

      await vi.waitFor(() =>
        expect(result.current.auth).toEqual(refreshedUser),
      );

      // Drive time forward past the *old* expiry. If cookieSetAt was refreshed,
      // auth survives. If it wasn't, the expiry timer would have already cleared it.
      mockFetchAuthMe.mockResolvedValue(refreshedUser);
      await act(async () => {
        vi.advanceTimersByTime(60_000 + 100);
      });
      expect(result.current.auth).toEqual(refreshedUser);
    });

    it('does not clear auth on One Tap onError when the cookie is still valid', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth());
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Cookie was just set on mount, so cookieExpired() is false.
      act(() => {
        result.current.triggerRefresh();
      });

      await act(async () => {
        oneTapCallbacks.onError?.();
      });

      // Auth should be preserved — Google session may be gone but our cookie is still good.
      expect(result.current.auth).toEqual(user);
    });
  });
});

/**
 * Unit Tests for useAuth hook
 *
 * Tests mount behavior, refresh scheduling, logout, and expiry logic.
 * Uses fake timers and mocked authService / Google OAuth.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';

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

import { useAuth } from './useAuth';
import {
  fetchAuthMe,
  logout as serverLogout,
  refreshToken,
} from '../services/authService';

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

      // Advance past the refresh point (55 min). The refresh timer sets
      // isRefreshing=true and enables One Tap. Simulate One Tap failing
      // (no active Google session), which resets isRefreshing=false.
      await act(async () => {
        vi.advanceTimersByTime(55 * 60 * 1000 + 100);
      });
      await act(async () => {
        oneTapCallbacks.onError?.();
      });

      // Advance past cookie max-age (remaining ~5 min)
      await act(async () => {
        vi.advanceTimersByTime(5 * 60 * 1000 + 100);
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
});

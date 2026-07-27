/**
 * Unit Tests for useAuth hook
 *
 * Tests mount behavior, logout, and the definitive-session-end logic that
 * replaced One-Tap silent renewal (bd-s042qcxj). Session renewal is entirely
 * server-side now (sliding re-issue), so the hook only distinguishes:
 *   - a valid /auth/me (possibly with a slid `exp`) → stay signed in;
 *   - a definitive 401/403 from a reachable server → session ended;
 *   - a network error → offline, session preserved (bd-3o8zmz46).
 *
 * Uses fake timers and a MockAuthProvider in place of `@react-oauth/google`.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor, cleanup } from '@testing-library/react';
import type { ReactNode } from 'react';

import { AuthProviderRoot } from '../auth/AuthProvider';
import { createMockAuthProvider, type MockAuthProvider } from '../auth/MockAuthProvider';

vi.mock('../services/authService', () => ({
  fetchAuthMe: vi.fn(),
  logout: vi.fn(),
}));

import { useAuth } from './useAuth';
import { fetchAuthMe, logout as serverLogout } from '../services/authService';

const mockFetchAuthMe = vi.mocked(fetchAuthMe);
const mockServerLogout = vi.mocked(serverLogout);

let mockProvider: MockAuthProvider;

const wrapper = ({ children }: { children: ReactNode }) => (
  <AuthProviderRoot provider={mockProvider.provider}>{children}</AuthProviderRoot>
);

describe('useAuth', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockProvider = createMockAuthProvider();
    mockServerLogout.mockResolvedValue();
  });

  // ── Mount behavior (real timers — waitFor needs them) ─────

  describe('mount', () => {
    it('starts in loading state', () => {
      mockFetchAuthMe.mockReturnValue(new Promise(() => {})); // never resolves
      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.loading).toBe(true);
      expect(result.current.auth).toBeNull();
    });

    it('sets auth on successful /auth/me', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.loading).toBe(false));

      expect(result.current.auth).toEqual(user);
    });

    it('sets auth to null on 401 without flagging sessionExpired (not signed in)', async () => {
      mockFetchAuthMe.mockResolvedValue(null);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.loading).toBe(false));

      // A fresh visitor sees the sign-in prompt, not a "session expired" message.
      expect(result.current.auth).toBeNull();
      expect(result.current.sessionExpired).toBe(false);
    });

    it('sets auth to null on fetch error', async () => {
      mockFetchAuthMe.mockRejectedValue(new Error('network'));

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.loading).toBe(false));

      expect(result.current.auth).toBeNull();
    });

    it('does not expose a triggerRefresh method (One-Tap renewal retired)', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.auth).toEqual(user));

      expect(result.current).not.toHaveProperty('triggerRefresh');
    });
  });

  // ── Logout (real timers) ──────────────────────────────────

  describe('logout', () => {
    it('clears auth state and calls server logout', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.auth).toEqual(user));

      act(() => {
        result.current.logout();
      });

      expect(result.current.auth).toBeNull();
      expect(mockServerLogout).toHaveBeenCalled();
    });

    it('calls provider.signOut() during logout', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.auth).toEqual(user));

      act(() => {
        result.current.logout();
      });

      expect(mockProvider.signOutCalls).toBe(1);
    });

    it('clears auth even if server logout fails', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);
      mockServerLogout.mockRejectedValue(new Error('offline'));

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.auth).toEqual(user));

      act(() => {
        result.current.logout();
      });

      expect(result.current.auth).toBeNull();
    });

    it('does not flag sessionExpired on deliberate logout', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.auth).toEqual(user));

      act(() => {
        result.current.logout();
      });

      expect(result.current.auth).toBeNull();
      expect(result.current.sessionExpired).toBe(false);
    });
  });

  // ── Expiry-time re-check (fake timers) ────────────────────

  describe('expiry re-check', () => {
    beforeEach(() => {
      cleanup();
      mockFetchAuthMe.mockReset();
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('clears auth and flags sessionExpired on a definitive 401 at expiry', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe
        .mockResolvedValueOnce(user) // mount
        .mockResolvedValueOnce(null); // expiry re-check → 401

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Default 1 h lifetime (no server exp) → re-check just after +1 h.
      await act(async () => {
        vi.advanceTimersByTime(3600 * 1000 + 2000);
      });

      await vi.waitFor(() => expect(result.current.auth).toBeNull());
      expect(result.current.sessionExpired).toBe(true);
    });

    it('keeps auth when the server confirms a still-valid cookie at expiry', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      const freshUser = { email: 'a@b.com', name: 'Still Valid', picture: null };
      mockFetchAuthMe
        .mockResolvedValueOnce(user) // mount
        .mockResolvedValueOnce(freshUser); // expiry re-check → still valid

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      await act(async () => {
        vi.advanceTimersByTime(3600 * 1000 + 2000);
      });

      await vi.waitFor(() => expect(result.current.auth).toEqual(freshUser));
      expect(result.current.sessionExpired).toBe(false);
    });

    it('reschedules from a slid exp and logs out only at the new expiry', async () => {
      const start = Date.now();
      const user = { email: 'a@b.com', name: 'A', picture: null, expiresAt: start + 20 * 60 * 1000 };
      const slid = { email: 'a@b.com', name: 'A', picture: null, expiresAt: start + 40 * 60 * 1000 };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // First re-check at +20 min: the session has slid to a later exp.
      mockFetchAuthMe.mockResolvedValueOnce(slid);
      await act(async () => {
        vi.advanceTimersByTime(20 * 60 * 1000 + 2000);
      });
      await vi.waitFor(() => expect(result.current.auth).toEqual(slid));
      expect(result.current.sessionExpired).toBe(false);

      // A new timer was armed at the slid exp (+40 min); a 401 there logs out.
      mockFetchAuthMe.mockResolvedValueOnce(null);
      await act(async () => {
        vi.advanceTimersByTime(20 * 60 * 1000 + 2000);
      });
      await vi.waitFor(() => expect(result.current.auth).toBeNull());
      expect(result.current.sessionExpired).toBe(true);
    });

    it('keeps auth on an expiry-time network error, then logs out on a later 401', async () => {
      const start = Date.now();
      const user = { email: 'a@b.com', name: 'A', picture: null, expiresAt: start + 20 * 60 * 1000 };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Expiry re-check at +20 min hits a network error → stay logged in.
      mockFetchAuthMe.mockRejectedValueOnce(new Error('network'));
      await act(async () => {
        vi.advanceTimersByTime(20 * 60 * 1000 + 2000);
      });
      expect(result.current.auth).toEqual(user);
      expect(result.current.sessionExpired).toBe(false);

      // The next re-check (~60 s later) gets a definitive 401 → logout.
      mockFetchAuthMe.mockResolvedValueOnce(null);
      await act(async () => {
        vi.advanceTimersByTime(60 * 1000 + 1000);
      });
      await vi.waitFor(() => expect(result.current.auth).toBeNull());
      expect(result.current.sessionExpired).toBe(true);
    });
  });

  // ── Refocus (visibility) re-check (fake timers) ───────────

  describe('refocus re-check', () => {
    beforeEach(() => {
      cleanup();
      mockFetchAuthMe.mockReset();
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('clears auth and flags sessionExpired on a definitive 401 on refocus', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      mockFetchAuthMe.mockResolvedValueOnce(null); // refocus → 401
      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      await vi.waitFor(() => expect(result.current.auth).toBeNull());
      expect(result.current.sessionExpired).toBe(true);
    });

    it('keeps auth when refocus /auth/me fails with a network error (offline)', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      mockFetchAuthMe.mockRejectedValueOnce(new Error('network'));
      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // Offline must never log the user out.
      expect(result.current.auth).toEqual(user);
      expect(result.current.sessionExpired).toBe(false);
    });

    it('does not extend the expiry schedule on a refocus that returns the same exp', async () => {
      const start = Date.now();
      const expiresAt = start + 40 * 60 * 1000;
      const user = { email: 'a@b.com', name: 'A', picture: null, expiresAt };
      mockFetchAuthMe
        .mockResolvedValueOnce(user) // mount
        .mockResolvedValueOnce({ ...user }) // refocus: same expiry, fresh object
        .mockResolvedValueOnce(null); // expiry re-check: token now rejected

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Refocus at +10 min — server confirms the cookie, expiry unchanged.
      // This must NOT push the expiry schedule out past the real +40 min.
      await act(async () => {
        vi.advanceTimersByTime(10 * 60 * 1000);
      });
      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // At the REAL expiry (+40 min) the definitive 401 clears auth.
      await act(async () => {
        vi.advanceTimersByTime(30 * 60 * 1000 + 2000);
      });
      await vi.waitFor(() => expect(result.current.auth).toBeNull());
      expect(result.current.sessionExpired).toBe(true);
    });
  });
});

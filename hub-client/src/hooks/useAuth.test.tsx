/**
 * Unit Tests for useAuth hook
 *
 * Tests mount behavior, refresh scheduling, logout, and expiry logic.
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

    it('sets auth to null on 401 (not authenticated)', async () => {
      mockFetchAuthMe.mockResolvedValue(null);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.loading).toBe(false));

      expect(result.current.auth).toBeNull();
    });

    it('sets auth to null on fetch error', async () => {
      mockFetchAuthMe.mockRejectedValue(new Error('network'));

      const { result } = renderHook(() => useAuth(), { wrapper });
      await waitFor(() => expect(result.current.loading).toBe(false));

      expect(result.current.auth).toBeNull();
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
  });

  // ── Refresh scheduling (fake timers) ──────────────────────

  describe('refresh scheduling', () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('starts with silent renewal disabled', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(mockFetchAuthMe).toHaveBeenCalled());

      // Before any time has passed, silent renewal should be disabled.
      expect(mockProvider.lastSilentRenewalOpts?.enabled).toBe(false);
    });

    it('updates auth on successful silent renewal', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      const refreshedUser = {
        email: 'a@b.com',
        name: 'A Updated',
        picture: null,
      };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() =>
        expect(result.current.auth).toEqual(user),
      );

      // Simulate the provider delivering a fresh credential.
      mockRefreshToken.mockResolvedValue(refreshedUser);
      await act(async () => {
        mockProvider.lastSilentRenewalOpts?.onCredential('fresh.jwt.token');
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

    it('attempts silent renewal when tab becomes visible with expired cookie', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      const refreshedUser = { email: 'a@b.com', name: 'Refreshed', picture: null };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Next /auth/me returns null (cookie expired)
      mockFetchAuthMe.mockResolvedValueOnce(null);
      mockRefreshToken.mockResolvedValue(refreshedUser);

      // Dispatch the event, then flush async handler + React updates separately.
      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // Silent renewal should be enabled for refresh.
      expect(mockProvider.lastSilentRenewalOpts?.enabled).toBe(true);

      // Simulate the provider delivering a fresh credential.
      await act(async () => {
        mockProvider.lastSilentRenewalOpts?.onCredential('fresh.jwt');
      });

      await vi.waitFor(() => expect(result.current.auth).toEqual(refreshedUser));
    });

    it('clears auth when silent renewal fails after visibility-triggered refresh', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Jump Date.now() past cookie lifetime (simulates long idle)
      vi.setSystemTime(Date.now() + 3600 * 1000 + 1000);

      // Next /auth/me returns null (cookie expired)
      mockFetchAuthMe.mockResolvedValueOnce(null);

      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // Simulate silent renewal failing (no active IdP session).
      await act(async () => {
        mockProvider.lastSilentRenewalOpts?.onError();
      });

      await vi.waitFor(() => expect(result.current.auth).toBeNull());
    });

    it('clears auth when refreshToken returns null after visibility-triggered refresh', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Jump Date.now() past cookie lifetime (simulates long idle)
      vi.setSystemTime(Date.now() + 3600 * 1000 + 1000);

      // Next /auth/me returns null (cookie expired)
      mockFetchAuthMe.mockResolvedValueOnce(null);

      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // Silent renewal returns a credential, but server rejects it.
      mockRefreshToken.mockResolvedValue(null);
      await act(async () => {
        mockProvider.lastSilentRenewalOpts?.onCredential('rejected.jwt');
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

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() =>
        expect(result.current.auth).toEqual(user),
      );

      // Advance past the refresh point. The refresh timer sets
      // isRefreshing=true and enables silent renewal. Simulate the IdP
      // session being gone (onError fires), resetting isRefreshing=false.
      await act(async () => {
        vi.advanceTimersByTime(COOKIE_MAX_AGE_MS - REFRESH_BUFFER_MS + 100);
      });
      await act(async () => {
        mockProvider.lastSilentRenewalOpts?.onError();
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

      const { result } = renderHook(() => useAuth(), { wrapper });
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

  // ── Expiry from server-reported exp (bd-3o8zmz46) ─────────

  describe('expiry from server exp', () => {
    beforeEach(() => {
      cleanup();
      // Full reset (not just clear) so queued mockResolvedValueOnce values
      // from a failed sibling test can't leak into the next one.
      mockFetchAuthMe.mockReset();
      mockRefreshToken.mockReset();
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('schedules silent refresh from server-reported expiresAt, not a fixed lifetime', async () => {
      const user = {
        email: 'a@b.com',
        name: 'A',
        picture: null,
        expiresAt: Date.now() + 30 * 60 * 1000,
      };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));
      expect(mockProvider.lastSilentRenewalOpts?.enabled).toBe(false);

      // Refresh should fire at expiresAt − 15 min = +15 min, well before
      // the legacy fixed-1h schedule would (at +45 min).
      await act(async () => {
        vi.advanceTimersByTime(15 * 60 * 1000 + 1000);
      });
      expect(mockProvider.lastSilentRenewalOpts?.enabled).toBe(true);
    });

    it('does not extend assumed expiry on refocus without a fresh cookie (drift bug)', async () => {
      const expiresAt = Date.now() + 40 * 60 * 1000;
      const user = { email: 'a@b.com', name: 'A', picture: null, expiresAt };
      mockFetchAuthMe
        .mockResolvedValueOnce(user) // mount
        .mockResolvedValueOnce({ ...user }) // refocus: same expiry, fresh object
        .mockResolvedValueOnce(null); // expiry re-check: token now rejected

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Refocus at +10 min — server confirms the cookie but its expiry is
      // unchanged. This must NOT push the schedule out to +70 min.
      await act(async () => {
        vi.advanceTimersByTime(10 * 60 * 1000);
      });
      document.dispatchEvent(new Event('visibilitychange'));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // Refresh fires at expiresAt − 15 min (+25 min); renewal fails.
      await act(async () => {
        vi.advanceTimersByTime(15 * 60 * 1000 + 1000);
      });
      await act(async () => {
        mockProvider.lastSilentRenewalOpts?.onError();
      });

      // At the REAL expiry (+40 min) the server's 401 must clear auth.
      await act(async () => {
        vi.advanceTimersByTime(15 * 60 * 1000 + 2000);
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
    });

    it('keeps auth on expiry-time network error and re-checks later', async () => {
      const expiresAt = Date.now() + 20 * 60 * 1000;
      const user = { email: 'a@b.com', name: 'A', picture: null, expiresAt };
      mockFetchAuthMe.mockResolvedValueOnce(user); // mount

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Refresh fires at +5 min; renewal fails (session not lapsed → keep).
      await act(async () => {
        vi.advanceTimersByTime(5 * 60 * 1000 + 1000);
      });
      await act(async () => {
        mockProvider.lastSilentRenewalOpts?.onError();
      });

      // Expiry re-check at +20 min hits a network error → stay logged in.
      mockFetchAuthMe.mockRejectedValueOnce(new Error('network'));
      await act(async () => {
        vi.advanceTimersByTime(15 * 60 * 1000 + 2000);
      });
      expect(result.current.auth).toEqual(user);
      expect(result.current.sessionExpired).toBe(false);

      // The next re-check gets a definitive 401 → evidence-based logout.
      mockFetchAuthMe.mockResolvedValueOnce(null);
      await act(async () => {
        vi.advanceTimersByTime(60 * 1000 + 1000);
      });
      await vi.waitFor(() => expect(result.current.auth).toBeNull());
      expect(result.current.sessionExpired).toBe(true);
    });

    it('does not flag sessionExpired on deliberate logout', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      act(() => {
        result.current.logout();
      });

      expect(result.current.auth).toBeNull();
      expect(result.current.sessionExpired).toBe(false);
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

    it('enables silent renewal when called while auth is still considered valid', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Cookie freshly set on mount — renewal initially disabled.
      expect(mockProvider.lastSilentRenewalOpts?.enabled).toBe(false);

      act(() => {
        result.current.triggerRefresh();
      });

      expect(mockProvider.lastSilentRenewalOpts?.enabled).toBe(true);
    });

    it('coalesces concurrent triggerRefresh() calls into a single renewal attempt', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Multiple concurrent calls (e.g. parallel fetchActorId 401s).
      act(() => {
        result.current.triggerRefresh();
        result.current.triggerRefresh();
        result.current.triggerRefresh();
      });

      // Renewal enabled exactly once; refreshEnabled stayed true.
      expect(mockProvider.lastSilentRenewalOpts?.enabled).toBe(true);

      // Resolve the credential path; isRefreshing resets, renewal disables.
      mockRefreshToken.mockResolvedValue(user);
      await act(async () => {
        mockProvider.lastSilentRenewalOpts?.onCredential('fresh.jwt');
      });
      await vi.waitFor(() =>
        expect(mockProvider.lastSilentRenewalOpts?.enabled).toBe(false),
      );

      // A subsequent triggerRefresh re-enables (proving the gate cleared).
      act(() => {
        result.current.triggerRefresh();
      });
      expect(mockProvider.lastSilentRenewalOpts?.enabled).toBe(true);
    });

    it('updates auth and refreshes cookieSetAt on successful silent renewal', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      const refreshedUser = {
        email: 'a@b.com',
        name: 'A Refreshed',
        picture: null,
      };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
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
        mockProvider.lastSilentRenewalOpts?.onCredential('fresh.jwt');
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

    it('does not clear auth on renewal onError when the cookie is still valid', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      mockFetchAuthMe.mockResolvedValue(user);

      const { result } = renderHook(() => useAuth(), { wrapper });
      await vi.waitFor(() => expect(result.current.auth).toEqual(user));

      // Cookie was just set on mount, so cookieExpired() is false.
      act(() => {
        result.current.triggerRefresh();
      });

      await act(async () => {
        mockProvider.lastSilentRenewalOpts?.onError();
      });

      // Auth should be preserved — IdP session may be gone but our cookie is still good.
      expect(result.current.auth).toEqual(user);
    });
  });
});

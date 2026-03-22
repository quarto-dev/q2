/**
 * Unit Tests for authService
 *
 * Tests auth API helpers: fetchAuthMe, logout, refreshToken.
 * Uses mocked fetch and Google OAuth.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock @react-oauth/google before importing the module under test.
// vi.mock factories are hoisted above imports, so we cannot reference
// top-level variables. Instead, use vi.mocked() after import.
vi.mock('@react-oauth/google', () => ({
  googleLogout: vi.fn(),
}));

import { fetchAuthMe, fetchActorId, logout, refreshToken } from './authService';
import { googleLogout } from '@react-oauth/google';
const mockGoogleLogout = vi.mocked(googleLogout);

describe('authService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // ── fetchAuthMe ─────────────────────────────────────────────

  describe('fetchAuthMe', () => {
    it('returns user info on 200', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve(user),
      } as Response);

      const result = await fetchAuthMe();
      expect(result).toEqual({ email: 'a@b.com', name: 'A', picture: null });
      expect(fetch).toHaveBeenCalledWith('/auth/me', {
        credentials: 'same-origin',
      });
    });

    it('returns null on 401', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 401,
      } as Response);

      expect(await fetchAuthMe()).toBeNull();
    });

    it('throws on non-401 error status', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 500,
      } as Response);

      await expect(fetchAuthMe()).rejects.toThrow('/auth/me failed: 500');
    });
  });

  // ── logout ──────────────────────────────────────────────────

  describe('logout', () => {
    it('posts to /auth/logout with CSRF header and calls googleLogout', async () => {
      vi.mocked(fetch).mockResolvedValue({ ok: true } as Response);

      await logout();

      expect(fetch).toHaveBeenCalledWith('/auth/logout', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'X-Requested-With': 'XMLHttpRequest' },
      });
      expect(mockGoogleLogout).toHaveBeenCalled();
    });
  });

  // ── refreshToken ────────────────────────────────────────────

  describe('refreshToken', () => {
    it('sends credential and returns fresh user info on success', async () => {
      const user = { email: 'a@b.com', name: 'A', picture: null };

      // First call: POST /auth/refresh → 200
      // Second call: GET /auth/me → 200 with user
      vi.mocked(fetch)
        .mockResolvedValueOnce({ ok: true, status: 200 } as Response)
        .mockResolvedValueOnce({
          ok: true,
          status: 200,
          json: () => Promise.resolve(user),
        } as Response);

      const result = await refreshToken('jwt.token.here');

      expect(fetch).toHaveBeenNthCalledWith(1, '/auth/refresh', {
        method: 'POST',
        credentials: 'same-origin',
        headers: {
          'Content-Type': 'application/json',
          'X-Requested-With': 'XMLHttpRequest',
        },
        body: JSON.stringify({ credential: 'jwt.token.here' }),
      });
      expect(result).toEqual(user);
    });

    it('returns null on 401', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 401,
      } as Response);

      expect(await refreshToken('bad')).toBeNull();
    });

    it('returns null on 403', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 403,
      } as Response);

      expect(await refreshToken('wrong-domain')).toBeNull();
    });

    it('throws on unexpected server error', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 502,
      } as Response);

      await expect(refreshToken('cred')).rejects.toThrow(
        '/auth/refresh failed: 502',
      );
    });
  });

  // ── fetchActorId ─────────────────────────────────────────────

  describe('fetchActorId', () => {
    it('calls GET /auth/actor?project=<id> and returns actor_id', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ actor_id: 'abcd1234' }),
      } as Response);

      const result = await fetchActorId('automerge:abc123');

      expect(result).toBe('abcd1234');
      expect(fetch).toHaveBeenCalledWith(
        '/auth/actor?project=automerge%3Aabc123',
        { credentials: 'same-origin' },
      );
    });

    it('returns null on 401', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 401,
      } as Response);

      expect(await fetchActorId('automerge:abc')).toBeNull();
    });

    it('returns null on 403', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 403,
      } as Response);

      expect(await fetchActorId('automerge:abc')).toBeNull();
    });

    it('throws on non-OK, non-401/403 response', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 500,
      } as Response);

      await expect(fetchActorId('automerge:abc')).rejects.toThrow(
        '/auth/actor failed: 500',
      );
    });

    it('same request twice returns same actor_id (determinism via mock)', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ actor_id: 'deadbeef' }),
      } as Response);

      const id1 = await fetchActorId('automerge:proj1');
      const id2 = await fetchActorId('automerge:proj1');
      expect(id1).toBe(id2);
    });

    it('different project values produce different actor_ids via mock', async () => {
      vi.mocked(fetch)
        .mockResolvedValueOnce({
          ok: true,
          status: 200,
          json: () => Promise.resolve({ actor_id: 'aaaa' }),
        } as Response)
        .mockResolvedValueOnce({
          ok: true,
          status: 200,
          json: () => Promise.resolve({ actor_id: 'bbbb' }),
        } as Response);

      const id1 = await fetchActorId('automerge:proj1');
      const id2 = await fetchActorId('automerge:proj2');
      expect(id1).not.toBe(id2);
    });
  });
});

/**
 * Unit Tests for authService
 *
 * Tests auth API helpers: fetchAuthMe, logout, refreshToken, fetchActorId.
 * Uses mocked fetch. IdP-side signout is no longer authService's
 * concern (moved to the AuthProvider boundary); see Phase 6 of
 * `claude-notes/plans/2026-05-20-auth-provider-interface.md`.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import { fetchAuthMe, fetchActorId, resolveActorId, logout, refreshToken } from './authService';

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
    it('posts to /auth/logout with CSRF header', async () => {
      vi.mocked(fetch).mockResolvedValue({ ok: true } as Response);

      await logout();

      expect(fetch).toHaveBeenCalledWith('/auth/logout', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'X-Requested-With': 'XMLHttpRequest' },
      });
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

  // ── resolveActorId ───────────────────────────────────────────
  //
  // Three-valued contract the document-open callers depend on:
  //   string    → open with this actor ID
  //   undefined → auth disabled; open with no (random) actor ID
  //   null      → auth failure; abandon the open (callers guard `=== null`)

  describe('resolveActorId', () => {
    it('returns undefined and skips the network when auth is disabled', async () => {
      const onSessionExpired = vi.fn();
      const result = await resolveActorId('automerge:abc', false, onSessionExpired);

      expect(result).toBeUndefined();
      expect(fetch).not.toHaveBeenCalled();
      expect(onSessionExpired).not.toHaveBeenCalled();
    });

    it('returns the actor ID on success without triggering refresh', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ actor_id: 'abcd1234' }),
      } as Response);
      const onSessionExpired = vi.fn();

      const result = await resolveActorId('automerge:abc', true, onSessionExpired);

      expect(result).toBe('abcd1234');
      expect(onSessionExpired).not.toHaveBeenCalled();
    });

    it('returns null (abandon) and triggers refresh on 401', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 401,
      } as Response);
      const onSessionExpired = vi.fn();

      const result = await resolveActorId('automerge:abc', true, onSessionExpired);

      // null, NOT undefined: callers' `if (id === null) return` must fire so
      // the document open is abandoned while the refresh races in the background.
      expect(result).toBeNull();
      expect(onSessionExpired).toHaveBeenCalledTimes(1);
    });

    it('returns null (abandon) and triggers refresh on 403', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 403,
      } as Response);
      const onSessionExpired = vi.fn();

      const result = await resolveActorId('automerge:abc', true, onSessionExpired);

      expect(result).toBeNull();
      expect(onSessionExpired).toHaveBeenCalledTimes(1);
    });

    it('propagates non-401/403 errors without triggering refresh', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 500,
      } as Response);
      const onSessionExpired = vi.fn();

      await expect(
        resolveActorId('automerge:abc', true, onSessionExpired),
      ).rejects.toThrow('/auth/actor failed: 500');
      expect(onSessionExpired).not.toHaveBeenCalled();
    });
  });
});

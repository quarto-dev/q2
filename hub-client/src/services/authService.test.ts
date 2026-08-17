/**
 * Unit Tests for authService
 *
 * Tests auth API helpers: fetchAuthMe, logout, fetchActorId, resolveActorId.
 * Uses mocked fetch. IdP-side signout is no longer authService's
 * concern (moved to the AuthProvider boundary); see Phase 6 of
 * `claude-notes/plans/2026-05-20-auth-provider-interface.md`. Session
 * renewal is entirely server-side (sliding re-issue) — there is no client
 * renewal helper since One-Tap was retired (bd-s042qcxj).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import { fetchAuthMe, fetchActorId, resolveActorId, logout } from './authService';

describe('authService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllEnvs();
  });

  // ── hub base path (subpath mount) ───────────────────────────

  describe('VITE_HUB_BASE_PATH', () => {
    it('prefixes auth requests with the configured mount base', async () => {
      vi.stubEnv('VITE_HUB_BASE_PATH', '/subpath');
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ email: 'admin', name: 'admin', picture: null }),
      } as Response);

      await fetchAuthMe();
      expect(fetch).toHaveBeenCalledWith('/subpath/auth/me', {
        credentials: 'same-origin',
      });

      await fetchActorId('proj-1');
      expect(fetch).toHaveBeenCalledWith(
        '/subpath/auth/actor?project=proj-1',
        { credentials: 'same-origin' },
      );
    });

    it('leaves paths origin-absolute when unset (dev / standalone)', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ email: 'a@b.com', name: null, picture: null }),
      } as Response);

      await fetchAuthMe();
      expect(fetch).toHaveBeenCalledWith('/auth/me', {
        credentials: 'same-origin',
      });
    });
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

    it('maps exp and the credential discriminator through (bd-aw8f3sp8)', async () => {
      const now = Math.floor(Date.now() / 1000);
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({
          email: 'a@b.com',
          name: 'A',
          picture: null,
          exp: now + 600,
          credential: 'session',
        }),
      } as Response);

      const result = await fetchAuthMe();
      expect(result?.expiresAt).toBe((now + 600) * 1000);
      expect(result?.credential).toBe('session');
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

    it('returns the fallback actor id (not the network) when auth is disabled', async () => {
      // Auth-less deployments (local-prod) have no /auth/actor to call, but a
      // stable local actor id lets identity stamping still work. The fallback
      // is returned verbatim; the network is never touched.
      const onSessionExpired = vi.fn();
      const result = await resolveActorId(
        'automerge:abc',
        false,
        onSessionExpired,
        '6d914340d834489b934c58390f9b3301',
      );

      expect(result).toBe('6d914340d834489b934c58390f9b3301');
      expect(fetch).not.toHaveBeenCalled();
      expect(onSessionExpired).not.toHaveBeenCalled();
    });

    it('ignores the fallback when auth is enabled (server actor wins)', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ actor_id: 'serveractor' }),
      } as Response);
      const onSessionExpired = vi.fn();

      const result = await resolveActorId(
        'automerge:abc',
        true,
        onSessionExpired,
        '6d914340d834489b934c58390f9b3301',
      );

      expect(result).toBe('serveractor');
    });

    it('returns the actor ID on success without ending the session', async () => {
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

    it('returns null (abandon) and ends the session on 401', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 401,
      } as Response);
      const onSessionExpired = vi.fn();

      const result = await resolveActorId('automerge:abc', true, onSessionExpired);

      // null, NOT undefined: callers' `if (id === null) return` must fire so
      // the document open is abandoned; onSessionExpired ends the session so
      // the SPA shows the login screen (server-side renewal already slid a
      // live session — a 401 here means it is definitively over).
      expect(result).toBeNull();
      expect(onSessionExpired).toHaveBeenCalledTimes(1);
    });

    it('returns null (abandon) and ends the session on 403', async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 403,
      } as Response);
      const onSessionExpired = vi.fn();

      const result = await resolveActorId('automerge:abc', true, onSessionExpired);

      expect(result).toBeNull();
      expect(onSessionExpired).toHaveBeenCalledTimes(1);
    });

    it('propagates non-401/403 errors without ending the session', async () => {
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

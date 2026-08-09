/**
 * Unit tests for previewConfig.
 *
 * fetchPreviewSessionConfig decides whether the serving server is a
 * `q2 preview` session and, if so, whether edits persist to disk
 * (`allowEdit`). Null is the safe default: standalone hubs (no such
 * route), SPA-fallback HTML, malformed bodies, and network errors must
 * all yield null so the editor never shows the ephemeral-session
 * banner against a real hub.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import { fetchPreviewSessionConfig } from './previewConfig';

function jsonResponse(body: unknown, init?: { ok?: boolean; status?: number }): Response {
  return {
    ok: init?.ok ?? true,
    status: init?.status ?? 200,
    json: () => Promise.resolve(body),
  } as Response;
}

describe('fetchPreviewSessionConfig', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllEnvs();
  });

  it('returns the config when the server is a preview session', async () => {
    vi.mocked(fetch).mockResolvedValue(jsonResponse({ allowEdit: false }));

    await expect(fetchPreviewSessionConfig()).resolves.toEqual({ allowEdit: false });
    expect(fetch).toHaveBeenCalledWith('/api/preview/config', {
      credentials: 'same-origin',
    });
  });

  it('passes allowEdit: true through (persistent session)', async () => {
    vi.mocked(fetch).mockResolvedValue(jsonResponse({ allowEdit: true }));

    await expect(fetchPreviewSessionConfig()).resolves.toEqual({ allowEdit: true });
  });

  it('ignores extra fields such as editorBoot', async () => {
    vi.mocked(fetch).mockResolvedValue(
      jsonResponse({
        allowEdit: false,
        editorBoot: { indexDocId: 'doc', file: 'index.qmd', name: 'proj' },
      }),
    );

    await expect(fetchPreviewSessionConfig()).resolves.toEqual({ allowEdit: false });
  });

  it('returns null on 404 (a standalone hub has no such route)', async () => {
    vi.mocked(fetch).mockResolvedValue(jsonResponse({}, { ok: false, status: 404 }));

    await expect(fetchPreviewSessionConfig()).resolves.toBeNull();
  });

  it('returns null when the body is SPA-fallback HTML (json() throws)', async () => {
    vi.mocked(fetch).mockResolvedValue({
      ok: true,
      status: 200,
      json: () => Promise.reject(new SyntaxError('Unexpected token <')),
    } as unknown as Response);

    await expect(fetchPreviewSessionConfig()).resolves.toBeNull();
  });

  it.each([{}, { allowEdit: 'false' }, { allowEdit: 0 }, null, 'allowEdit'])(
    'returns null when allowEdit is missing or not a boolean: %j',
    async (body) => {
      vi.mocked(fetch).mockResolvedValue(jsonResponse(body));

      await expect(fetchPreviewSessionConfig()).resolves.toBeNull();
    },
  );

  it('returns null on network error', async () => {
    vi.mocked(fetch).mockRejectedValue(new TypeError('fetch failed'));

    await expect(fetchPreviewSessionConfig()).resolves.toBeNull();
  });

  it('prefixes the path with VITE_HUB_BASE_PATH when set', async () => {
    vi.stubEnv('VITE_HUB_BASE_PATH', '/subpath');
    vi.mocked(fetch).mockResolvedValue(jsonResponse({ allowEdit: true }));

    await fetchPreviewSessionConfig();
    expect(fetch).toHaveBeenCalledWith('/subpath/api/preview/config', {
      credentials: 'same-origin',
    });
  });
});

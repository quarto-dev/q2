/**
 * Guards the VITE_DISABLE_HUB_PROXY escape hatch in vite.config.ts.
 *
 * playwright.harness.config.ts boots `vite dev` with no hub server at
 * all; the /auth and /ws proxy entries would otherwise target a dead
 * port and turn the app's auth keepalive probes into hundreds of
 * ECONNREFUSED proxy errors per CI run (bd-a1cwdir9).
 */
import { describe, it, expect, afterEach, vi } from 'vitest';

const ENV_KEY = 'VITE_DISABLE_HUB_PROXY';
const ORIGINAL = process.env[ENV_KEY];

async function loadConfig(flag?: string) {
  if (flag === undefined) {
    delete process.env[ENV_KEY];
  } else {
    process.env[ENV_KEY] = flag;
  }
  // vite.config.ts reads env at module top level, so force re-evaluation.
  vi.resetModules();
  const mod = await import('../vite.config');
  return mod.default;
}

afterEach(() => {
  if (ORIGINAL === undefined) {
    delete process.env[ENV_KEY];
  } else {
    process.env[ENV_KEY] = ORIGINAL;
  }
});

describe('vite.config hub proxy', () => {
  it('proxies /auth and /ws to the hub target by default', async () => {
    const config = await loadConfig(undefined);
    const proxy = (config.server?.proxy ?? {}) as Record<string, { target?: string }>;
    expect(proxy).toHaveProperty('/auth');
    expect(proxy).toHaveProperty('/ws');
    expect(proxy['/auth'].target).toBe('http://localhost:3000');
    // Preview mirrors the dev-server proxy for the production-build E2E suite.
    expect(config.preview?.proxy).toHaveProperty('/auth');
  });

  it('omits the hub proxy when VITE_DISABLE_HUB_PROXY=1', async () => {
    const config = await loadConfig('1');
    const serverProxy = config.server?.proxy ?? {};
    const previewProxy = config.preview?.proxy ?? {};
    expect(serverProxy).not.toHaveProperty('/auth');
    expect(serverProxy).not.toHaveProperty('/ws');
    expect(previewProxy).not.toHaveProperty('/auth');
    expect(previewProxy).not.toHaveProperty('/ws');
  });
});

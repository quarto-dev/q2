/**
 * OAuth env-config sourcing. Both `QUARTO_HUB_MCP_CLIENT_ID` and
 * `QUARTO_HUB_MCP_CLIENT_SECRET` are required (the Desktop-app client
 * still needs the secret — Amendment 2026-05-28); a partial config
 * fails naming the missing var.
 */

import { describe, it, expect } from 'vitest';

import { loadOAuthConfigFromEnv, MissingOAuthConfigError } from './oauth-config.js';

function envOnly(values: Record<string, string | undefined>): NodeJS.ProcessEnv {
  const e: NodeJS.ProcessEnv = {};
  for (const [k, v] of Object.entries(values)) {
    if (v !== undefined) e[k] = v;
  }
  return e;
}

describe('loadOAuthConfigFromEnv', () => {
  it('returns both values when both are set', () => {
    const cfg = loadOAuthConfigFromEnv(
      envOnly({
        QUARTO_HUB_MCP_CLIENT_ID: 'cid.apps.googleusercontent.com',
        QUARTO_HUB_MCP_CLIENT_SECRET: 'GOCSPX-secret',
      }),
    );
    expect(cfg.clientId).toBe('cid.apps.googleusercontent.com');
    expect(cfg.clientSecret).toBe('GOCSPX-secret');
  });

  it('throws naming the secret when only the id is set', () => {
    expect(() =>
      loadOAuthConfigFromEnv(envOnly({ QUARTO_HUB_MCP_CLIENT_ID: 'cid' })),
    ).toThrowError(/QUARTO_HUB_MCP_CLIENT_SECRET/);
  });

  it('throws naming the id when only the secret is set', () => {
    expect(() =>
      loadOAuthConfigFromEnv(envOnly({ QUARTO_HUB_MCP_CLIENT_SECRET: 'sec' })),
    ).toThrowError(/QUARTO_HUB_MCP_CLIENT_ID/);
  });

  it('throws MissingOAuthConfigError naming both when neither is set', () => {
    let caught: unknown;
    try {
      loadOAuthConfigFromEnv(envOnly({}));
    } catch (err) {
      caught = err;
    }
    expect(caught).toBeInstanceOf(MissingOAuthConfigError);
    expect((caught as Error).message).toMatch(/QUARTO_HUB_MCP_CLIENT_ID/);
    expect((caught as Error).message).toMatch(/QUARTO_HUB_MCP_CLIENT_SECRET/);
  });

  it('treats an empty / whitespace value as unset', () => {
    expect(() =>
      loadOAuthConfigFromEnv(
        envOnly({ QUARTO_HUB_MCP_CLIENT_ID: '  ', QUARTO_HUB_MCP_CLIENT_SECRET: 'sec' }),
      ),
    ).toThrowError(/QUARTO_HUB_MCP_CLIENT_ID/);
  });
});

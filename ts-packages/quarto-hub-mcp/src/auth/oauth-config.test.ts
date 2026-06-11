/**
 * OAuth env-config sourcing. Both `QUARTO_HUB_MCP_CLIENT_ID` and
 * `QUARTO_HUB_MCP_CLIENT_SECRET` are required (the Desktop-app client
 * still needs the secret — Amendment 2026-05-28); a partial config
 * fails naming the missing var.
 */

import { describe, it, expect } from 'vitest';

import {
  GOOGLE_ISSUER,
  issuerAllowsInsecureRequests,
  loadOAuthConfigFromEnv,
  MissingOAuthConfigError,
  resolveIssuer,
} from './oauth-config.js';

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

describe('resolveIssuer', () => {
  it('defaults to Google when unset', () => {
    expect(resolveIssuer(envOnly({}))).toBe(GOOGLE_ISSUER);
  });

  it('accepts an https issuer override', () => {
    expect(resolveIssuer(envOnly({ QUARTO_HUB_MCP_ISSUER: 'https://idp.example.com' }))).toBe(
      'https://idp.example.com',
    );
  });

  it('treats empty / whitespace as unset', () => {
    expect(resolveIssuer(envOnly({ QUARTO_HUB_MCP_ISSUER: '  ' }))).toBe(GOOGLE_ISSUER);
  });

  it('accepts an http loopback issuer only with the insecure escape hatch', () => {
    expect(
      resolveIssuer(
        envOnly({
          QUARTO_HUB_MCP_ISSUER: 'http://127.0.0.1:9999',
          QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH: '1',
        }),
      ),
    ).toBe('http://127.0.0.1:9999');
  });

  it('rejects an http loopback issuer without the escape hatch, naming it', () => {
    expect(() =>
      resolveIssuer(envOnly({ QUARTO_HUB_MCP_ISSUER: 'http://127.0.0.1:9999' })),
    ).toThrowError(/QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH/);
  });

  it('rejects an http non-loopback issuer even with the escape hatch', () => {
    expect(() =>
      resolveIssuer(
        envOnly({
          QUARTO_HUB_MCP_ISSUER: 'http://idp.example.com',
          QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH: '1',
        }),
      ),
    ).toThrowError(/https/i);
  });

  it('rejects garbage and non-http(s) schemes', () => {
    expect(() => resolveIssuer(envOnly({ QUARTO_HUB_MCP_ISSUER: 'not a url' }))).toThrowError();
    expect(() =>
      resolveIssuer(envOnly({ QUARTO_HUB_MCP_ISSUER: 'ftp://idp.example.com' })),
    ).toThrowError();
  });
});

describe('issuerAllowsInsecureRequests', () => {
  it('is false for https issuers and true for http ones', () => {
    expect(issuerAllowsInsecureRequests('https://accounts.google.com')).toBe(false);
    expect(issuerAllowsInsecureRequests('http://127.0.0.1:9999')).toBe(true);
  });
});

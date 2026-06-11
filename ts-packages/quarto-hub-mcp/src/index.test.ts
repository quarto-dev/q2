/**
 * `--redirect-port` validation. The kernel-pick default is reached by
 * *omitting* the flag, so `0` and the rest of the privileged range are
 * rejected; a stable SSH-tunnel port must be non-privileged.
 *
 * `parseArgs` server-URL resolution: flag > env > canonical default
 * (wss://quarto-hub.com/ws — decided 2026-06-11, bd-81cfshmw plan
 * resolved question 3; the "easy path" for `q2 mcp` and npx users).
 */

import { describe, it, expect } from 'vitest';

import { DEFAULT_SERVER_URL, parseArgs, parseRedirectPort } from './index.js';

describe('parseRedirectPort', () => {
  it('accepts the bottom of the non-privileged range', () => {
    expect(parseRedirectPort('1024')).toBe(1024);
  });

  it('accepts the top of the valid range', () => {
    expect(parseRedirectPort('65535')).toBe(65535);
  });

  it('accepts an ephemeral-range port', () => {
    expect(parseRedirectPort('49152')).toBe(49152);
  });

  it('rejects 0 (kernel-pick is reached by omitting the flag)', () => {
    expect(() => parseRedirectPort('0')).toThrowError();
  });

  it('rejects privileged ports (<1024) naming the free range to use', () => {
    expect(() => parseRedirectPort('80')).toThrowError(/49152-65535|non-privileged/);
    expect(() => parseRedirectPort('1023')).toThrowError(/non-privileged/);
  });

  it('rejects out-of-range ports', () => {
    expect(() => parseRedirectPort('65536')).toThrowError();
    expect(() => parseRedirectPort('99999')).toThrowError();
    expect(() => parseRedirectPort('-1')).toThrowError();
  });

  it('rejects non-numeric input', () => {
    expect(() => parseRedirectPort('abc')).toThrowError(/integer/);
    expect(() => parseRedirectPort('80.5')).toThrowError(/integer/);
  });
});

describe('parseArgs server URL resolution', () => {
  const argv = (...rest: string[]) => ['node', 'index.js', ...rest];

  it('defaults to the canonical quarto-hub.com sync server', () => {
    const parsed = parseArgs(argv(), {});
    expect(parsed.serverUrl).toBe(DEFAULT_SERVER_URL);
    expect(DEFAULT_SERVER_URL).toBe('wss://quarto-hub.com/ws');
  });

  it('--server overrides the default', () => {
    const parsed = parseArgs(argv('--server', 'ws://127.0.0.1:3000/ws'), {});
    expect(parsed.serverUrl).toBe('ws://127.0.0.1:3000/ws');
  });

  it('QUARTO_HUB_SERVER overrides the default', () => {
    const parsed = parseArgs(argv(), { QUARTO_HUB_SERVER: 'wss://other.example/ws' });
    expect(parsed.serverUrl).toBe('wss://other.example/ws');
  });

  it('--server beats QUARTO_HUB_SERVER', () => {
    const parsed = parseArgs(argv('--server', 'wss://flag.example/ws'), {
      QUARTO_HUB_SERVER: 'wss://env.example/ws',
    });
    expect(parsed.serverUrl).toBe('wss://flag.example/ws');
  });
});

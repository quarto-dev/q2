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

import {
  DEFAULT_SERVER_URL,
  parseArgs,
  parseRedirectPort,
  resolveShutdownDrainMs,
} from './index.js';

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

/**
 * `QUARTO_MCP_SHUTDOWN_DRAIN_MS` — the shutdown drain budget's test
 * seam (bd-yw3mcdkg).
 *
 * The production default stays 3000 ms: it is the "exit promptly"
 * contract (bd-9jq2a060) that stdio-hygiene.test.ts asserts. The
 * override exists because exit-drain.test.ts has to push ~4 MB through
 * the drain to keep its assertion binding at all, and 4 MB does not
 * reliably clear 3000 ms on a 3-core CI runner. Overriding the budget
 * there removes a throughput race without touching the default that
 * real `q2 mcp` sessions get.
 *
 * A malformed value must fall back to the default rather than throw —
 * this is read during startup of a stdio server whose stdout belongs to
 * the protocol, and a typo in someone's shell must not brick the server.
 */
describe('resolveShutdownDrainMs', () => {
  it('defaults to 3000 ms when unset', () => {
    expect(resolveShutdownDrainMs(undefined)).toBe(3000);
  });

  it('defaults when set to the empty string', () => {
    expect(resolveShutdownDrainMs('')).toBe(3000);
  });

  it('accepts an explicit override', () => {
    expect(resolveShutdownDrainMs('30000')).toBe(30000);
  });

  it('accepts 0, which disables the drain', () => {
    // Deliberate: it is how the drain can be proven load-bearing
    // without editing source (see exit-drain.test.ts's payload notes).
    expect(resolveShutdownDrainMs('0')).toBe(0);
  });

  it('falls back to the default on a malformed value rather than throwing', () => {
    expect(resolveShutdownDrainMs('abc')).toBe(3000);
    expect(resolveShutdownDrainMs('3000.5')).toBe(3000);
    expect(resolveShutdownDrainMs('-1')).toBe(3000);
  });
});

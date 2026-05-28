/**
 * `--redirect-port` validation. The kernel-pick default is reached by
 * *omitting* the flag, so `0` and the rest of the privileged range are
 * rejected; a stable SSH-tunnel port must be non-privileged.
 */

import { describe, it, expect } from 'vitest';

import { parseRedirectPort } from './index.js';

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

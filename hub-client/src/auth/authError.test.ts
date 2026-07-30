/**
 * Tests for the `auth_error` redirect parameter: reading it, and mapping
 * it to user-facing copy.
 */

import { describe, it, expect } from 'vitest';

import { authErrorMessage, readAuthErrorReason } from './authError';

describe('readAuthErrorReason', () => {
  it('returns undefined when the parameter is absent', () => {
    expect(readAuthErrorReason('')).toBeUndefined();
    expect(readAuthErrorReason('?project=abc')).toBeUndefined();
  });

  it("returns '' for a bare ?auth_error — present, but with no value", () => {
    // What a pre-E1 hub, or a cached redirect, emits. `''` is falsy, so
    // presence has to be read separately from the value; a truthiness
    // check here makes the error message silently disappear.
    expect(readAuthErrorReason('?auth_error')).toBe('');
    expect(readAuthErrorReason('?auth_error=')).toBe('');
  });

  it('returns the reason when the parameter carries one', () => {
    expect(readAuthErrorReason('?auth_error=denied')).toBe('denied');
    expect(readAuthErrorReason('?project=abc&auth_error=stale_client')).toBe('stale_client');
  });
});

describe('authErrorMessage', () => {
  it('gives each hub reason its own copy', () => {
    expect(authErrorMessage('stale_client')).toMatch(/out of date/i);
    expect(authErrorMessage('restart')).toMatch(/didn't complete/i);
    expect(authErrorMessage('denied')).toMatch(/not authorized/i);
    expect(authErrorMessage('server')).toMatch(/went wrong on the hub/i);
  });

  it('falls back to the retry copy for an empty or unknown reason', () => {
    // Client/server skew, or a crafted URL. A false "try again" costs one
    // retry; a false "not authorized" sends the user to an administrator.
    expect(authErrorMessage('')).toBe(authErrorMessage('restart'));
    expect(authErrorMessage('nonsense')).toBe(authErrorMessage('restart'));
  });

  it('falls back for prototype keys too, and never echoes the reason', () => {
    // The reason is only ever a lookup key. An object-literal lookup of
    // `__proto__` would return something truthy that is not copy at all.
    expect(authErrorMessage('__proto__')).toBe(authErrorMessage('restart'));
    expect(authErrorMessage('constructor')).toBe(authErrorMessage('restart'));
    expect(authErrorMessage('<script>alert(1)</script>')).toBe(authErrorMessage('restart'));
  });
});

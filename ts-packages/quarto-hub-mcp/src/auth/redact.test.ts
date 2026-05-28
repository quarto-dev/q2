/**
 * Token redaction — known-answer cases lifted from the former
 * device-flow test suite when `redactTokens` moved to its own module.
 */

import { describe, it, expect } from 'vitest';

import { redactTokens } from './redact.js';

describe('redactTokens', () => {
  it('redacts Google access tokens (ya29.*)', () => {
    const s = 'token=ya29.aBcDeF_-1234567890XYZ end';
    expect(redactTokens(s)).not.toContain('ya29.aBcDeF');
    expect(redactTokens(s)).toContain('[redacted-token]');
  });

  it('redacts Google refresh tokens (1//*)', () => {
    const s = 'rt=1//0abcDEF-1234_xyz end';
    expect(redactTokens(s)).not.toContain('1//0abcDEF');
    expect(redactTokens(s)).toContain('[redacted-token]');
  });

  it('redacts JWT-shaped substrings (xxx.yyy.zzz)', () => {
    const jwt =
      'eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ4In0.signature_bytes_here_AB12';
    expect(redactTokens(`auth: ${jwt} end`)).not.toContain(jwt);
    expect(redactTokens(`auth: ${jwt} end`)).toContain('[redacted-token]');
  });

  it('passes through strings with no token shapes', () => {
    expect(redactTokens('hello world')).toBe('hello world');
  });
});

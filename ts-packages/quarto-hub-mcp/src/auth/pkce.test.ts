/**
 * PKCE + state primitives.
 *
 * Known-answer test against RFC 7636 §4.6 pins the S256 transform; the
 * round-trip test confirms our wrapper keeps verifier and challenge in
 * agreement.
 */

import { describe, it, expect } from 'vitest';
import * as oauth from 'oauth4webapi';

import { generatePkceParams } from './pkce.js';

// RFC 7636 §4.6 worked example.
const RFC_VERIFIER = 'dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk';
const RFC_CHALLENGE = 'E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM';

describe('PKCE', () => {
  it('computes the RFC 7636 §4.6 S256 challenge for the example verifier', async () => {
    const challenge = await oauth.calculatePKCECodeChallenge(RFC_VERIFIER);
    expect(challenge).toBe(RFC_CHALLENGE);
  });

  it('generatePkceParams returns a self-consistent S256 verifier/challenge', async () => {
    const p = await generatePkceParams();
    expect(p.codeChallengeMethod).toBe('S256');
    expect(p.codeVerifier.length).toBeGreaterThanOrEqual(43);
    const recomputed = await oauth.calculatePKCECodeChallenge(p.codeVerifier);
    expect(p.codeChallenge).toBe(recomputed);
  });

  it('generates a high-entropy state value', async () => {
    const a = await generatePkceParams();
    const b = await generatePkceParams();
    // base64url of ≥128 bits ⇒ ≥22 chars; and two draws must differ.
    expect(a.state.length).toBeGreaterThanOrEqual(22);
    expect(a.state).not.toBe(b.state);
  });
});

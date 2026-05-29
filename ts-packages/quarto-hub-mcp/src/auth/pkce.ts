/**
 * PKCE + CSRF-state primitives.
 *
 * Thin wrapper over `oauth4webapi` (already a project dep, used by every
 * other auth path) so the loopback flow has a small, stable, testable
 * surface. Rolling our own with Node `crypto` would diverge from the
 * established pattern for no gain.
 *
 *   - `code_verifier` / `code_challenge` (S256) bind the authorization
 *     code to this process: a leaked code is useless without the
 *     verifier held in memory here.
 *   - `state` (≥128 bits of entropy, base64url) binds the callback to
 *     the originating flow — the CSRF defence.
 */

import * as oauth from 'oauth4webapi';

export interface PkceParams {
  readonly codeVerifier: string;
  readonly codeChallenge: string;
  readonly codeChallengeMethod: 'S256';
  readonly state: string;
}

export async function generatePkceParams(): Promise<PkceParams> {
  const codeVerifier = oauth.generateRandomCodeVerifier();
  const codeChallenge = await oauth.calculatePKCECodeChallenge(codeVerifier);
  const state = oauth.generateRandomState();
  return { codeVerifier, codeChallenge, codeChallengeMethod: 'S256', state };
}

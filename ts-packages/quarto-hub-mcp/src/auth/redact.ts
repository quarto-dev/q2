/**
 * Token redaction for log call sites.
 *
 * Every log statement in the auth path funnels through {@link redactTokens}
 * so a Google-token-shaped substring never reaches stderr in the clear.
 * The pattern is a single alternation so the input is scanned once per call.
 *
 * Note: `code_verifier`, the authorization `code`, and `state` do **not**
 * match these shapes and must be filtered at the call site (i.e. simply
 * never passed to a logger). This module only handles token-shaped bytes.
 */

// Branches:
//   ya29\....            — Google access tokens
//   1\/\/...             — Google refresh tokens
//   eyJ....\....\....    — JWT-shaped (three base64url segments)
const TOKEN_PATTERN =
  /ya29\.[A-Za-z0-9_-]+|1\/\/[A-Za-z0-9_-]+|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g;

export function redactTokens(s: string): string {
  return s.replace(TOKEN_PATTERN, '[redacted-token]');
}

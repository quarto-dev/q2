/**
 * GoogleAuthProvider — AuthProvider implementation wrapping Google
 * Identity Services via `@react-oauth/google`.
 *
 * Requires `<GoogleOAuthProvider clientId={...}>` to be mounted above
 * any tree that uses this provider's `SignInButton`. The GIS provider
 * wrap stays in `main.tsx` (see Phase 3 of
 * `claude-notes/plans/2026-05-20-auth-provider-interface.md`); this
 * module does not produce its own provider scope.
 *
 * The login nonce (H2) is fetched here rather than passed down through
 * `SignInButtonProps`: a nonce is a GIS concern, and widening the
 * provider-agnostic interface with a Google-specific field would leak
 * this provider's mechanics into every other one.
 */

import { useEffect, useState } from 'react';
import { GoogleLogin, googleLogout } from '@react-oauth/google';

import { hubPath } from '../utils/routing';
import type { AuthProvider, SignInButtonProps } from './AuthProvider';

/**
 * Fetch a login nonce from the hub's pre-flight endpoint.
 *
 * The response body carries the nonce for GIS; the hub simultaneously
 * sets a sealed HttpOnly cookie holding the same value, which it
 * verifies when Google POSTs the credential back. Returns `null` on any
 * failure — the caller must not fall back to a nonce-less login, which
 * the hub would reject.
 */
async function fetchLoginNonce(): Promise<string | null> {
  try {
    const response = await fetch(hubPath('/auth/nonce'), {
      // The pre-flight's whole purpose is the cookie it sets.
      credentials: 'include',
    });
    if (!response.ok) return null;
    const body = await response.json();
    return typeof body?.nonce === 'string' && body.nonce.length > 0 ? body.nonce : null;
  } catch {
    return null;
  }
}

function SignInButton({ loginUri }: SignInButtonProps) {
  const [nonce, setNonce] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    fetchLoginNonce().then((value) => {
      if (cancelled) return;
      if (value) setNonce(value);
      else setFailed(true);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  if (failed) {
    return (
      <p role="alert" style={{ color: 'var(--posit-red)', fontSize: '14px', margin: 0 }}>
        Could not start sign-in. Please reload the page.
      </p>
    );
  }

  // Deliberately render nothing until the nonce is in hand.
  // `@react-oauth/google` forwards `nonce` into
  // `google.accounts.id.initialize` from an effect whose dependency list
  // does not include it, so a nonce arriving after the first render
  // would never reach GIS — and every login would then fail the hub's
  // check. Gating the mount is what guarantees the prop is applied.
  if (!nonce) {
    return null;
  }

  return (
    <GoogleLogin
      ux_mode="redirect"
      login_uri={loginUri}
      nonce={nonce}
      onSuccess={() => {
        // Not called in redirect mode — credential arrives via HttpOnly
        // cookie set by the server-side redirect callback.
      }}
    />
  );
}

export const googleAuthProvider: AuthProvider = {
  SignInButton,
  signOut: () => googleLogout(),
};

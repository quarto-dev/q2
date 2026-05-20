/**
 * GoogleAuthProvider — AuthProvider implementation wrapping Google
 * Identity Services via `@react-oauth/google`.
 *
 * Requires `<GoogleOAuthProvider clientId={...}>` to be mounted above
 * any tree that uses this provider's `SignInButton` or
 * `useSilentRenewal`. The GIS provider wrap stays in `main.tsx` (see
 * Phase 3 of `claude-notes/plans/2026-05-20-auth-provider-interface.md`);
 * this module does not produce its own provider scope.
 */

import {
  GoogleLogin,
  googleLogout,
  useGoogleOneTapLogin,
} from '@react-oauth/google';

import type {
  AuthProvider,
  SignInButtonProps,
  SilentRenewalOpts,
} from './AuthProvider';

function SignInButton({ loginUri }: SignInButtonProps) {
  return (
    <GoogleLogin
      ux_mode="redirect"
      login_uri={loginUri}
      onSuccess={() => {
        // Not called in redirect mode — credential arrives via HttpOnly
        // cookie set by the server-side redirect callback.
      }}
    />
  );
}

function useSilentRenewal(opts: SilentRenewalOpts) {
  useGoogleOneTapLogin({
    onSuccess: (response) => {
      if (response.credential) {
        opts.onCredential(response.credential);
      } else {
        // Success without a credential — semantically equivalent to a
        // failed renewal from the consumer's perspective.
        opts.onError();
      }
    },
    onError: () => {
      opts.onError();
    },
    auto_select: true,
    disabled: !opts.enabled,
  });
}

export const googleAuthProvider: AuthProvider = {
  SignInButton,
  useSilentRenewal,
  signOut: () => googleLogout(),
};

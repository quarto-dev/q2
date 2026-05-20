/**
 * Test double for the AuthProvider interface.
 *
 * Used by tests that want to drive consumers of AuthProvider without
 * loading the GIS SDK. Each created mock exposes:
 *
 * - `provider` — the AuthProvider object to pass into `AuthProviderRoot`.
 * - `lastSilentRenewalOpts` — the most-recent opts passed to
 *   `useSilentRenewal`, so tests can synchronously invoke
 *   `onCredential` / `onError` to simulate IdP responses (mirrors the
 *   `oneTapCallbacks` pattern in the legacy `useAuth.test.ts`).
 * - `signInButtonClicks` — count of times the SignInButton was clicked.
 * - `lastLoginUri` — the most recent `loginUri` prop passed to
 *   SignInButton, so tests can assert the wiring without needing DOM
 *   introspection.
 * - `signOutCalls` — count of `signOut()` invocations.
 *
 * Not for production use — the `Mock` prefix is the signal.
 */

import { useEffect } from 'react';

import type {
  AuthProvider,
  SignInButtonProps,
  SilentRenewalOpts,
} from './AuthProvider';

export interface MockAuthProvider {
  provider: AuthProvider;
  lastSilentRenewalOpts: SilentRenewalOpts | null;
  signInButtonClicks: number;
  lastLoginUri: string | null;
  signOutCalls: number;
  /** Reset all captured state. Useful in `beforeEach`. */
  reset(): void;
}

export function createMockAuthProvider(): MockAuthProvider {
  const state: MockAuthProvider = {
    provider: null as unknown as AuthProvider,
    lastSilentRenewalOpts: null,
    signInButtonClicks: 0,
    lastLoginUri: null,
    signOutCalls: 0,
    reset() {
      state.lastSilentRenewalOpts = null;
      state.signInButtonClicks = 0;
      state.lastLoginUri = null;
      state.signOutCalls = 0;
    },
  };

  const SignInButton = ({ loginUri }: SignInButtonProps) => {
    useEffect(() => {
      state.lastLoginUri = loginUri;
    }, [loginUri]);
    return (
      <button
        data-testid="auth-signin"
        onClick={() => {
          state.signInButtonClicks += 1;
        }}
      >
        Mock sign-in
      </button>
    );
  };

  state.provider = {
    SignInButton,
    useSilentRenewal: (opts: SilentRenewalOpts) => {
      state.lastSilentRenewalOpts = opts;
    },
    signOut: () => {
      state.signOutCalls += 1;
    },
  };

  return state;
}

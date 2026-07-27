/**
 * Test double for the AuthProvider interface.
 *
 * Used by tests that want to drive consumers of AuthProvider without
 * loading the GIS SDK. Each created mock exposes:
 *
 * - `provider` — the AuthProvider object to pass into `AuthProviderRoot`.
 * - `signInButtonClicks` — count of times the SignInButton was clicked.
 * - `lastLoginUri` — the most recent `loginUri` prop passed to
 *   SignInButton, so tests can assert the wiring without needing DOM
 *   introspection.
 * - `signOutCalls` — count of `signOut()` invocations.
 *
 * Not for production use — the `Mock` prefix is the signal.
 */

import { useEffect } from 'react';

import type { AuthProvider, SignInButtonProps } from './AuthProvider';

export interface MockAuthProvider {
  provider: AuthProvider;
  signInButtonClicks: number;
  lastLoginUri: string | null;
  signOutCalls: number;
  /** Reset all captured state. Useful in `beforeEach`. */
  reset(): void;
}

export function createMockAuthProvider(): MockAuthProvider {
  const state: MockAuthProvider = {
    provider: null as unknown as AuthProvider,
    signInButtonClicks: 0,
    lastLoginUri: null,
    signOutCalls: 0,
    reset() {
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
    signOut: () => {
      state.signOutCalls += 1;
    },
  };

  return state;
}

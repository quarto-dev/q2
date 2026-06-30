/**
 * Tests for LoginScreen.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import type { ReactNode } from 'react';

import { AuthProviderRoot } from '../../auth/AuthProvider';
import { createMockAuthProvider, type MockAuthProvider } from '../../auth/MockAuthProvider';
import { LoginScreen } from './LoginScreen';

let mock: MockAuthProvider;

beforeEach(() => {
  mock = createMockAuthProvider();
});

afterEach(() => {
  cleanup();
  vi.unstubAllEnvs();
});

function withProvider(children: ReactNode) {
  return (
    <AuthProviderRoot provider={mock.provider}>{children}</AuthProviderRoot>
  );
}

describe('LoginScreen', () => {
  it("renders the provider's SignInButton with loginUri = origin + /auth/callback", () => {
    render(withProvider(<LoginScreen />));

    // SignInButton mounted (mock renders a data-testid button).
    expect(screen.getByTestId('auth-signin')).toBeTruthy();

    // loginUri threaded through to the provider.
    expect(mock.lastLoginUri).not.toBeNull();
    expect(mock.lastLoginUri).toBe(window.location.origin + '/auth/callback');
  });

  it('prefixes the callback with the hub base path under a subpath mount', () => {
    vi.stubEnv('VITE_HUB_BASE_PATH', '/subpath');
    render(withProvider(<LoginScreen />));

    expect(mock.lastLoginUri).toBe(window.location.origin + '/subpath/auth/callback');
  });

  it('renders the default copy when error is false/absent', () => {
    render(withProvider(<LoginScreen />));
    expect(screen.getByText(/Sign in with Google to continue/i)).toBeTruthy();
    expect(screen.queryByText(/Sign-in failed/i)).toBeNull();
  });

  it('renders the error copy when error={true}', () => {
    render(withProvider(<LoginScreen error />));
    expect(screen.getByText(/Sign-in failed/i)).toBeTruthy();
    expect(screen.queryByText(/Sign in with Google to continue/i)).toBeNull();
  });

  it('renders a custom message (session expiry) instead of the default copy', () => {
    render(withProvider(<LoginScreen message="Your session expired — please sign in again." />));
    expect(screen.getByText(/session expired/i)).toBeTruthy();
    expect(screen.queryByText(/Sign in with Google to continue/i)).toBeNull();
  });

  it('error copy wins over a custom message', () => {
    render(withProvider(<LoginScreen error message="Your session expired — please sign in again." />));
    expect(screen.getByText(/Sign-in failed/i)).toBeTruthy();
    expect(screen.queryByText(/session expired/i)).toBeNull();
  });
});

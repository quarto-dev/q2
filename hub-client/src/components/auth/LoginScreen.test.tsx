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

  it('renders the default copy when no error reason is present', () => {
    render(withProvider(<LoginScreen />));
    expect(screen.getByText(/Sign in with Google to continue/i)).toBeTruthy();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
    expect(screen.queryByText(/didn't complete/i)).toBeNull();
  });

  // One case per user-facing message. Eleven distinct causes used to
  // collapse into the "not authorized" sentence, sending users who needed
  // a reload to an administrator instead.
  it('tells a stale client to reload', () => {
    render(withProvider(<LoginScreen errorReason="stale_client" />));
    expect(screen.getByText(/out of date.*reload the page/i)).toBeTruthy();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
  });

  it('tells a broken-down sign-in to try again', () => {
    render(withProvider(<LoginScreen errorReason="restart" />));
    expect(screen.getByText(/didn't complete.*try again/i)).toBeTruthy();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
  });

  it('tells a refused identity it is not authorized', () => {
    render(withProvider(<LoginScreen errorReason="denied" />));
    expect(screen.getByText(/not authorized to access this hub/i)).toBeTruthy();
  });

  it('reports a hub-side failure as a hub-side failure', () => {
    render(withProvider(<LoginScreen errorReason="server" />));
    expect(screen.getByText(/went wrong on the hub/i)).toBeTruthy();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
  });

  // A bare `/?auth_error` from a pre-E1 hub parses to `''`, which is
  // falsy — the error must still show, and as the retry copy rather than
  // the alarming one.
  it('renders the retry copy for an empty reason, not nothing', () => {
    render(withProvider(<LoginScreen errorReason="" />));
    expect(screen.getByText(/didn't complete.*try again/i)).toBeTruthy();
    expect(screen.queryByText(/Sign in with Google to continue/i)).toBeNull();
  });

  it('renders the retry copy for an unknown reason, never "not authorized"', () => {
    render(withProvider(<LoginScreen errorReason="something-we-do-not-know" />));
    expect(screen.getByText(/didn't complete.*try again/i)).toBeTruthy();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
  });

  it('never renders the reason string itself', () => {
    render(withProvider(<LoginScreen errorReason="<img src=x onerror=1>" />));
    expect(screen.queryByText(/onerror/i)).toBeNull();
    expect(screen.getByText(/didn't complete.*try again/i)).toBeTruthy();
  });

  it('renders a custom message (session expiry) instead of the default copy', () => {
    render(withProvider(<LoginScreen message="Your session expired — please sign in again." />));
    expect(screen.getByText(/session expired/i)).toBeTruthy();
    expect(screen.queryByText(/Sign in with Google to continue/i)).toBeNull();
  });

  it('error copy wins over a custom message', () => {
    render(withProvider(<LoginScreen errorReason="denied" message="Your session expired — please sign in again." />));
    expect(screen.getByText(/not authorized/i)).toBeTruthy();
    expect(screen.queryByText(/session expired/i)).toBeNull();
  });
});

/**
 * Tests for GoogleAuthProvider — the AuthProvider implementation that
 * wraps Google Identity Services.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup, screen, waitFor } from '@testing-library/react';

// Capture mock state at module scope so each test can inspect / drive it.
let lastGoogleLoginProps: {
  ux_mode?: string;
  login_uri?: string;
  nonce?: string;
} | null = null;

let googleLoginRenderCount = 0;

const mockGoogleLogout = vi.fn();

vi.mock('@react-oauth/google', () => ({
  GoogleLogin: (props: typeof lastGoogleLoginProps) => {
    lastGoogleLoginProps = props;
    googleLoginRenderCount += 1;
    return <div data-testid="gis-button" />;
  },
  googleLogout: () => mockGoogleLogout(),
}));

import { googleAuthProvider } from './GoogleAuthProvider';

/** Stub `fetch` with a successful `/auth/nonce` response. */
function stubNonce(nonce: string) {
  const fetchMock = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({ nonce }),
  });
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

beforeEach(() => {
  lastGoogleLoginProps = null;
  googleLoginRenderCount = 0;
  mockGoogleLogout.mockClear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('GoogleAuthProvider.SignInButton', () => {
  it('renders GoogleLogin in redirect mode with the given loginUri', async () => {
    stubNonce('nonce-abc');
    render(<googleAuthProvider.SignInButton loginUri="/auth/callback" />);

    await waitFor(() => expect(lastGoogleLoginProps).not.toBeNull());
    expect(lastGoogleLoginProps?.ux_mode).toBe('redirect');
    expect(lastGoogleLoginProps?.login_uri).toBe('/auth/callback');
  });

  it('fetches a nonce from the hub and passes it to GIS', async () => {
    // Server-verified nonce (H2): GIS puts this in the ID token, and the
    // hub's /auth/callback requires it to match the sealed cookie the
    // same pre-flight set.
    const fetchMock = stubNonce('nonce-xyz');
    render(<googleAuthProvider.SignInButton loginUri="/auth/callback" />);

    await waitFor(() => expect(lastGoogleLoginProps?.nonce).toBe('nonce-xyz'));
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][0]).toBe('/auth/nonce');
  });

  it('does not render GIS before the nonce arrives', async () => {
    // Load-bearing: @react-oauth/google forwards `nonce` into
    // `google.accounts.id.initialize` from an effect whose dependency
    // list does NOT include it. A nonce that arrives after the first
    // render would therefore never reach GIS, and every login would fail
    // the server check. Gating the render is what makes the prop stick.
    let resolveFetch: (value: unknown) => void = () => {};
    vi.stubGlobal(
      'fetch',
      vi.fn().mockReturnValue(new Promise((resolve) => {
        resolveFetch = resolve;
      })),
    );

    render(<googleAuthProvider.SignInButton loginUri="/auth/callback" />);
    expect(googleLoginRenderCount).toBe(0);
    expect(lastGoogleLoginProps).toBeNull();

    resolveFetch({ ok: true, json: async () => ({ nonce: 'late' }) });
    await waitFor(() => expect(lastGoogleLoginProps?.nonce).toBe('late'));
  });

  it('shows an error instead of a nonce-less button when the pre-flight fails', async () => {
    // Rendering GIS without a nonce would produce a login that always
    // fails at the callback — worse than saying so up front.
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')));
    render(<googleAuthProvider.SignInButton loginUri="/auth/callback" />);

    await waitFor(() => expect(screen.getByRole('alert')).toBeDefined());
    expect(googleLoginRenderCount).toBe(0);
  });

  it('treats a non-OK pre-flight response as a failure', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 500 }));
    render(<googleAuthProvider.SignInButton loginUri="/auth/callback" />);

    await waitFor(() => expect(screen.getByRole('alert')).toBeDefined());
    expect(googleLoginRenderCount).toBe(0);
  });
});

describe('GoogleAuthProvider', () => {
  it('exposes no silent-renewal hook (One-Tap renewal retired, bd-s042qcxj)', () => {
    expect('useSilentRenewal' in googleAuthProvider).toBe(false);
  });
});

describe('GoogleAuthProvider.signOut', () => {
  it('calls googleLogout()', () => {
    googleAuthProvider.signOut();
    expect(mockGoogleLogout).toHaveBeenCalledTimes(1);
  });
});

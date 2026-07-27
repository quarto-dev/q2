/**
 * Tests for GoogleAuthProvider — the AuthProvider implementation that
 * wraps Google Identity Services.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';

// Capture mock state at module scope so each test can inspect / drive it.
let lastGoogleLoginProps: {
  ux_mode?: string;
  login_uri?: string;
} | null = null;

const mockGoogleLogout = vi.fn();

vi.mock('@react-oauth/google', () => ({
  GoogleLogin: (props: typeof lastGoogleLoginProps) => {
    lastGoogleLoginProps = props;
    return null;
  },
  googleLogout: () => mockGoogleLogout(),
}));

import { googleAuthProvider } from './GoogleAuthProvider';

beforeEach(() => {
  lastGoogleLoginProps = null;
  mockGoogleLogout.mockClear();
});

afterEach(cleanup);

describe('GoogleAuthProvider.SignInButton', () => {
  it('renders GoogleLogin in redirect mode with the given loginUri', () => {
    render(<googleAuthProvider.SignInButton loginUri="/auth/callback" />);

    expect(lastGoogleLoginProps).not.toBeNull();
    expect(lastGoogleLoginProps?.ux_mode).toBe('redirect');
    expect(lastGoogleLoginProps?.login_uri).toBe('/auth/callback');
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

/**
 * Tests for GoogleAuthProvider — the AuthProvider implementation that
 * wraps Google Identity Services.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, render, cleanup } from '@testing-library/react';

import type { SilentRenewalOpts } from './AuthProvider';

// Capture mock state at module scope so each test can inspect / drive it.
let lastGoogleLoginProps: {
  ux_mode?: string;
  login_uri?: string;
} | null = null;

let lastOneTapOpts: {
  onSuccess?: (response: { credential?: string }) => void;
  onError?: () => void;
  disabled?: boolean;
  auto_select?: boolean;
} | null = null;

const mockGoogleLogout = vi.fn();

vi.mock('@react-oauth/google', () => ({
  GoogleLogin: (props: typeof lastGoogleLoginProps) => {
    lastGoogleLoginProps = props;
    return null;
  },
  useGoogleOneTapLogin: (opts: typeof lastOneTapOpts) => {
    lastOneTapOpts = opts;
  },
  googleLogout: () => mockGoogleLogout(),
}));

import { googleAuthProvider } from './GoogleAuthProvider';

beforeEach(() => {
  lastGoogleLoginProps = null;
  lastOneTapOpts = null;
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

describe('GoogleAuthProvider.useSilentRenewal', () => {
  function renderProviderHook(opts: SilentRenewalOpts) {
    return renderHook(() => googleAuthProvider.useSilentRenewal(opts));
  }

  it('calls useGoogleOneTapLogin with auto_select:true and disabled:false when enabled', () => {
    renderProviderHook({
      enabled: true,
      onCredential: vi.fn(),
      onError: vi.fn(),
    });

    expect(lastOneTapOpts).not.toBeNull();
    expect(lastOneTapOpts?.auto_select).toBe(true);
    expect(lastOneTapOpts?.disabled).toBe(false);
  });

  it('calls useGoogleOneTapLogin with disabled:true when not enabled', () => {
    renderProviderHook({
      enabled: false,
      onCredential: vi.fn(),
      onError: vi.fn(),
    });

    expect(lastOneTapOpts?.disabled).toBe(true);
  });

  it('forwards onCredential when one-tap success carries a credential', () => {
    const onCredential = vi.fn();
    const onError = vi.fn();
    renderProviderHook({ enabled: true, onCredential, onError });

    lastOneTapOpts?.onSuccess?.({ credential: 'jwt-token' });
    expect(onCredential).toHaveBeenCalledExactlyOnceWith('jwt-token');
    expect(onError).not.toHaveBeenCalled();
  });

  it('forwards onError when one-tap success carries no credential', () => {
    const onCredential = vi.fn();
    const onError = vi.fn();
    renderProviderHook({ enabled: true, onCredential, onError });

    lastOneTapOpts?.onSuccess?.({});
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onCredential).not.toHaveBeenCalled();
  });

  it('forwards onError on one-tap error', () => {
    const onCredential = vi.fn();
    const onError = vi.fn();
    renderProviderHook({ enabled: true, onCredential, onError });

    lastOneTapOpts?.onError?.();
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onCredential).not.toHaveBeenCalled();
  });
});

describe('GoogleAuthProvider.signOut', () => {
  it('calls googleLogout()', () => {
    googleAuthProvider.signOut();
    expect(mockGoogleLogout).toHaveBeenCalledTimes(1);
  });
});

/**
 * Tests for the AuthProvider React context plumbing.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach } from 'vitest';
import { renderHook, cleanup } from '@testing-library/react';
import type { ReactNode } from 'react';

import {
  AuthProviderRoot,
  noopAuthProvider,
  useAuthProvider,
  type AuthProvider,
} from './AuthProvider';

afterEach(cleanup);

function makeStubProvider(): AuthProvider {
  return {
    SignInButton: () => null,
    useSilentRenewal: () => {},
    signOut: () => {},
  };
}

describe('useAuthProvider', () => {
  it('returns noopAuthProvider when no AuthProviderRoot is mounted', () => {
    const { result } = renderHook(() => useAuthProvider());
    expect(result.current).toBe(noopAuthProvider);
  });

  it('returns the provided value inside AuthProviderRoot', () => {
    const provider = makeStubProvider();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AuthProviderRoot provider={provider}>{children}</AuthProviderRoot>
    );
    const { result } = renderHook(() => useAuthProvider(), { wrapper });
    expect(result.current).toBe(provider);
  });
});

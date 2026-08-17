/**
 * AuthProvider interface — mediates all IdP-specific calls.
 *
 * Implementations wrap a specific IdP integration (today: Google
 * Identity Services via `GoogleAuthProvider`). Consumers depend on
 * this interface via React context (`useAuthProvider`), never on a
 * concrete IdP SDK directly.
 *
 * `noopAuthProvider` is the no-op implementation used when auth is
 * disabled (no IdP configured). Consumers never need to branch — they
 * always receive a valid `AuthProvider`.
 *
 * Session *renewal* is not part of this interface: it is entirely
 * server-side (sliding re-issue, bd-ey6jg70f). The GIS One-Tap silent
 * renewal that this interface once exposed via `useSilentRenewal` was
 * retired in bd-s042qcxj; a session that hits a hard boundary
 * (revocation / absolute cap / idle) ends definitively and the user
 * re-logs-in through `SignInButton`.
 *
 * See `claude-notes/plans/2026-05-20-auth-provider-interface.md`
 * for the design rationale.
 */

import {
  createContext,
  useContext,
  type ComponentType,
  type ReactNode,
} from 'react';

export interface AuthProvider {
  /**
   * React component that renders the interactive sign-in affordance
   * (today: the GIS "Sign in with Google" button in redirect mode).
   *
   * The component owns the entire sign-in initiation — including the
   * redirect to the IdP. Success is observed by the SPA via the
   * existing `GET /auth/me` polling on the next page load.
   */
  readonly SignInButton: ComponentType<SignInButtonProps>;

  /**
   * Best-effort IdP-side sign-out. For Google this calls
   * `googleLogout()` which revokes the GIS session (no network round
   * trip). Synchronous on purpose — callers should not block UI on it.
   */
  signOut(): void;
}

export interface SignInButtonProps {
  /**
   * Server endpoint the IdP credential should land at. For GIS-redirect
   * mode this is wired into `<GoogleLogin login_uri={...} />`; for a
   * future Code+PKCE provider it would be the redirect URI the SPA
   * registers with the IdP.
   */
  loginUri: string;
}

/**
 * No-op provider used when auth is disabled (no `VITE_GOOGLE_CLIENT_ID`).
 * `SignInButton` renders nothing; `signOut` does nothing.
 */
export const noopAuthProvider: AuthProvider = {
  SignInButton: () => null,
  signOut: () => {},
};

const AuthProviderContext = createContext<AuthProvider>(noopAuthProvider);

export function AuthProviderRoot({
  provider,
  children,
}: {
  provider: AuthProvider;
  children: ReactNode;
}) {
  return (
    <AuthProviderContext.Provider value={provider}>
      {children}
    </AuthProviderContext.Provider>
  );
}

/** Returns the active provider, or `noopAuthProvider` when none is mounted. */
export function useAuthProvider(): AuthProvider {
  return useContext(AuthProviderContext);
}

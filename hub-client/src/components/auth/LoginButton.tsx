/**
 * Google Sign-In button wrapper.
 *
 * Uses Google Identity Services' "Sign In With Google" button in redirect
 * mode so the login flow stays within the same browser window (no popup).
 *
 * Flow:
 * 1. User clicks the button → browser navigates to Google (same tab)
 * 2. After authentication → Google POSTs the credential to login_uri
 * 3. The server at login_uri (Vite middleware in dev, hub server in
 *    production) extracts the credential and redirects back to the SPA
 *    with ?auth_credential=<jwt>
 * 4. useAuth() picks up the credential from the URL on mount
 */

import { GoogleLogin } from '@react-oauth/google';

export function LoginButton() {
  return (
    <GoogleLogin
      ux_mode="redirect"
      login_uri={window.location.origin + '/auth/callback'}
      onSuccess={() => {
        // Not called in redirect mode — credential arrives via URL parameter
        // after the server-side redirect callback.
      }}
      onError={() => console.error('Google login failed')}
    />
  );
}

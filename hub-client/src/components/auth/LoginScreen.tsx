/**
 * Login screen shown when authentication is required.
 *
 * Uses Google Identity Services' "Sign In With Google" button in redirect
 * mode so the login flow stays within the same browser window (no popup).
 *
 * Flow:
 * 1. User clicks the button → browser navigates to Google (same tab)
 * 2. After authentication → Google POSTs the credential to login_uri
 * 3. The server at login_uri validates the JWT, sets an HttpOnly cookie,
 *    and redirects to clean `/`
 * 4. useAuth() calls GET /auth/me on mount to populate auth state
 */

import { GoogleLogin } from '@react-oauth/google';

export function LoginScreen({ error }: { error?: boolean }) {
  return (
    <div className="project-selector" style={{ alignItems: 'center' }}>
      <div className="modal" style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', textAlign: 'center', padding: '48px 32px' }}>
        <img src="/quarto-icon.svg" alt="Quarto" style={{ width: '48px', height: '48px', marginBottom: '8px' }} />
        <h1 style={{ margin: 0 }}>Quarto Hub</h1>
        {error ? (
          <p style={{ color: 'var(--posit-red)', fontSize: '14px', margin: '0 0 16px' }}>
            Sign-in failed. Your account is not authorized to access this hub.
          </p>
        ) : (
          <p style={{ color: 'var(--text-secondary)', fontSize: '14px', margin: '0 0 16px' }}>
            Sign in with Google to continue
          </p>
        )}
        <GoogleLogin
          ux_mode="redirect"
          login_uri={window.location.origin + '/auth/callback'}
          onSuccess={() => {
            // Not called in redirect mode — credential arrives via HttpOnly
            // cookie set by the server-side redirect callback.
          }}
          onError={() => console.error('Google login failed')}
        />
      </div>
    </div>
  );
}

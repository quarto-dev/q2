/**
 * The landing page at quarto-hub.com, and the sign-in screen.
 *
 * One component serves both because App renders it whenever auth is
 * enabled and absent: a cold visitor, an expired session, and each of
 * the hub's auth-error reasons all land here. It therefore leads with
 * what Quarto Hub *is* (bd-g0uyp2v1) — a first-time visitor used to get
 * a logo and a Google button — and keeps that intro in the error states,
 * where "available by invite only" is the most useful line on the page.
 *
 * Renders the active AuthProvider's `SignInButton`. The flow shape
 * depends on the provider; for the default `googleAuthProvider`:
 *
 * 1. User clicks the GIS button → browser navigates to Google (same tab)
 * 2. After authentication → Google POSTs the credential to login_uri
 * 3. The server at login_uri validates the JWT, sets an HttpOnly cookie,
 *    and redirects to clean `/`
 * 4. useAuth() calls GET /auth/me on mount to populate auth state
 */

import './LoginScreen.css';
import { useAuthProvider } from '../../auth/AuthProvider';
import { authErrorMessage } from '../../auth/authError';
import { hubPath } from '../../utils/routing';
import { landing, links } from '../../strings';

/**
 * `errorReason` is the hub's coarse `auth_error` reason, not a flag —
 * `''` (a bare `/?auth_error`) is a *present* error, so presence is
 * tested with `!== undefined` rather than truthiness.
 */
export function LoginScreen({ errorReason, message }: { errorReason?: string; message?: string }) {
  const provider = useAuthProvider();

  return (
    <div className="ls-wrap">
      <div className="ls-card" data-testid="login-screen">
        <div className="ls-lockup">
          <img className="ls-logo" src="/quarto-icon.svg" alt="" />
          <span>{landing.product}</span>
        </div>

        {/* Two deliberate lines: the second sentence is the turn, and
            letting it wrap mid-phrase buried it. */}
        <h1 className="ls-tagline">
          <span className="ls-tagline-line">{landing.taglineLead}</span>
          <span className="ls-tagline-line">{landing.taglineFollow}</span>
        </h1>
        {/* Learn more runs on from the description rather than trailing
            the invite-only line: it belongs with what the product *is*,
            not with the caveat about who can get in. */}
        <p className="ls-what">
          {landing.what}{' '}
          {/* New tab: following it in place abandons the sign-in the
              visitor came here to complete. */}
          <a
            className="ls-learn-more"
            href={links.quartoHub}
            target="_blank"
            rel="noopener noreferrer"
          >
            {landing.learnMore}
          </a>
        </p>

        {/* Everything that qualifies the offer sits above the button, so
            nothing competes with it for last word. */}
        <p className="ls-footnote">{landing.inviteOnly}</p>

        {/* Nothing in the default state: the provider's button already
            says "Continue with Google", and a "Sign in with Google to
            continue" line right above it said so twice. The slot exists
            for the states that have something to report. */}
        {errorReason !== undefined ? (
          <p className="ls-error" role="alert">{authErrorMessage(errorReason)}</p>
        ) : message ? (
          <p className="ls-note">{message}</p>
        ) : null}

        <div className="ls-actions">
          <provider.SignInButton
            loginUri={window.location.origin + hubPath('/auth/callback')}
          />
        </div>
      </div>
    </div>
  );
}

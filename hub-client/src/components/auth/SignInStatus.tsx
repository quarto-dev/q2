/**
 * Sign-in status + sign-in / sign-out button.
 *
 * One line of status text beside the action that changes it:
 * - signed out → "Not signed in · changes stay on this device" (or the
 *   auth note: session expired, sign-in error) + the AuthProvider's
 *   sign-in button;
 * - signed in → "Signed in as <email>" + Sign out.
 *
 * Used in the projects-home top bar and in the editor's
 * ConnectionStatusDialog. The state arrives through context
 * (`SignInStatusProvider`, mounted by App) rather than props, because the
 * dialog sits several layers below anything that knows about auth.
 *
 * Renders nothing when no provider is mounted or its value is `null`
 * (auth not configured) — callers never need to branch.
 */

import { createContext, useContext, type ReactNode } from 'react';
import { useAuthProvider } from '../../auth/AuthProvider';
import { hubPath } from '../../utils/routing';
import { signInStatus as s } from '../../strings';
import './SignInStatus.css';

export interface SignInStatusValue {
  /** Signed-in user's email; `null` while signed out. */
  email: string | null;
  /** One-line auth note shown while signed out (session expired, sign-in error). */
  note?: string;
  onSignOut: () => void;
}

/** `null` = auth is not configured; the component renders nothing. */
const SignInStatusContext = createContext<SignInStatusValue | null>(null);

export function SignInStatusProvider({
  value,
  children,
}: {
  value: SignInStatusValue | null;
  children: ReactNode;
}) {
  return <SignInStatusContext.Provider value={value}>{children}</SignInStatusContext.Provider>;
}

/** The current sign-in status, or `null` when auth is not configured. */
export function useSignInStatus(): SignInStatusValue | null {
  return useContext(SignInStatusContext);
}

interface SignInStatusProps {
  className?: string;
  /**
   * `inline` (default): text and button side by side (top bar).
   * `stacked`: text above the button, end-aligned, for narrow hosts
   * (the connection status dialog).
   */
  layout?: 'inline' | 'stacked';
}

export default function SignInStatus({ className, layout = 'inline' }: SignInStatusProps) {
  const status = useSignInStatus();
  const provider = useAuthProvider();
  if (status === null) return null;

  const signedIn = status.email !== null;
  // Signed out with nothing else to report: the default line, whose
  // local-only half is emphasized as a warning.
  const showLocalOnly = signedIn === false && status.note === undefined;
  const text = signedIn
    ? s.signedInAs(status.email as string)
    : (status.note ?? `${s.signedOut} · ${s.localOnly}`);

  return (
    <div
      className={`sign-in-status ${layout}${signedIn ? '' : ' signed-out'}${className === undefined ? '' : ` ${className}`}`}
      data-testid="auth-status"
    >
      <span
        className={showLocalOnly ? 'sign-in-status-text with-warning' : 'sign-in-status-text'}
        title={text}
      >
        {showLocalOnly ? (
          <>
            <span>{s.signedOut}</span>
            <strong className="sign-in-status-warning">{s.localOnly}</strong>
          </>
        ) : (
          text
        )}
      </span>
      {signedIn ? (
        <button className="qh-btn outline" onClick={status.onSignOut}>
          {s.signOut}
        </button>
      ) : (
        <provider.SignInButton loginUri={window.location.origin + hubPath('/auth/callback')} />
      )}
    </div>
  );
}

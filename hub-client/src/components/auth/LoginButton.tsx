/**
 * Google Sign-In button wrapper.
 *
 * Uses Google Identity Services' "Sign In With Google" button which
 * returns an ID token directly — no separate userinfo API call needed.
 */

import { GoogleLogin } from '@react-oauth/google';

export function LoginButton({
  onCredential,
}: {
  onCredential: (credential: string) => void;
}) {
  return (
    <GoogleLogin
      onSuccess={(response) => {
        if (response.credential) onCredential(response.credential);
      }}
      onError={() => console.error('Google login failed')}
    />
  );
}

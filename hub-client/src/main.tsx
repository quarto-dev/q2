import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { GoogleOAuthProvider } from '@react-oauth/google'
import './theme.css'
import './index.css'
import App from './App.tsx'
import { savePreAuthHash, restorePreAuthHash } from './utils/routing'
import { AuthProviderRoot, noopAuthProvider } from './auth/AuthProvider'
import { googleAuthProvider } from './auth/GoogleAuthProvider'

// Pre-auth hash preservation for the Google OAuth redirect flow.
// On first visit: save the hash (e.g., #/share/...) before React clears it.
// On return from auth: restore it so the share route is processed normally.
// Order matters: restore first (consumes saved value), then save current.
restorePreAuthHash() || savePreAuthHash();

// E2E test hooks. Tree-shaken out unless VITE_E2E=1 was set at build time.
// We expose the dynamic-import promise on `window.__quartoTestReady` so
// `page.evaluate` callbacks can `await` it. Top-level await here would
// not help: it blocks React mount but the browser's `load` event (which
// playwright `goto` waits for) fires before module top-level awaits
// resolve, so a naive `window.__quartoTest` access can still race.
if (import.meta.env.VITE_E2E === '1') {
  (window as Window & { __quartoTestReady?: Promise<unknown> }).__quartoTestReady =
    import('./test-hooks');
}

const GOOGLE_CLIENT_ID = import.meta.env.VITE_GOOGLE_CLIENT_ID;

const authProvider = GOOGLE_CLIENT_ID ? googleAuthProvider : noopAuthProvider;

const root = (
  <StrictMode>
    <App />
  </StrictMode>
);

createRoot(document.getElementById('root')!).render(
  <GoogleOAuthProvider clientId={GOOGLE_CLIENT_ID || 'disabled'}>
    <AuthProviderRoot provider={authProvider}>
      {root}
    </AuthProviderRoot>
  </GoogleOAuthProvider>,
)

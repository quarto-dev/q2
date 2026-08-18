import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { GoogleOAuthProvider } from '@react-oauth/google'
import './theme.css'
import './index.css'
import './ui.css'
import App from './App.tsx'
import { savePreAuthHash, restorePreAuthHash } from './utils/routing'
import { AuthProviderRoot, noopAuthProvider } from './auth/AuthProvider'
import { googleAuthProvider } from './auth/GoogleAuthProvider'
import { ThemeProvider } from './components/ThemeContext'
import { isEphemeralStorage } from './services/ephemeralStorage'

// Ephemeral storage mode (bd-91mdd056): keep the WASM bridge's
// quarto-cache (SASS/theme artifacts) in memory too. The flag is read
// by ts-packages/wasm-js-bridge/src/cache.js in this realm; it must be
// set before the WASM first touches the cache, i.e. before any render.
if (isEphemeralStorage()) {
  (globalThis as Record<string, unknown>).__Q2_EPHEMERAL_STORAGE__ = true
}

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
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </StrictMode>
);

createRoot(document.getElementById('root')!).render(
  <GoogleOAuthProvider clientId={GOOGLE_CLIENT_ID || 'disabled'}>
    <AuthProviderRoot provider={authProvider}>
      {root}
    </AuthProviderRoot>
  </GoogleOAuthProvider>,
)

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { GoogleOAuthProvider } from '@react-oauth/google'
import './index.css'
import App from './App.tsx'
import { savePreAuthHash, restorePreAuthHash } from './utils/routing'

// Pre-auth hash preservation for the Google OAuth redirect flow.
// On first visit: save the hash (e.g., #/share/...) before React clears it.
// On return from auth: restore it so the share route is processed normally.
// Order matters: restore first (consumes saved value), then save current.
restorePreAuthHash() || savePreAuthHash();

const GOOGLE_CLIENT_ID = import.meta.env.VITE_GOOGLE_CLIENT_ID;

const root = (
  <StrictMode>
    <App />
  </StrictMode>
);

createRoot(document.getElementById('root')!).render(
  <GoogleOAuthProvider clientId={GOOGLE_CLIENT_ID || 'disabled'}>
    {root}
  </GoogleOAuthProvider>,
)

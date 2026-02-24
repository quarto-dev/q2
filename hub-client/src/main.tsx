import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { GoogleOAuthProvider } from '@react-oauth/google'
import './index.css'
import App from './App.tsx'

const GOOGLE_CLIENT_ID = import.meta.env.VITE_GOOGLE_CLIENT_ID;

const root = (
  <StrictMode>
    <App />
  </StrictMode>
);

createRoot(document.getElementById('root')!).render(
  GOOGLE_CLIENT_ID
    ? <GoogleOAuthProvider clientId={GOOGLE_CLIENT_ID}>{root}</GoogleOAuthProvider>
    : root,
)

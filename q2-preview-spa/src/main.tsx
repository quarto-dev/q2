/**
 * Entry point for the q2-preview SPA.
 *
 * Phase A.3 (bd-o5wd) — replaces the bd-hfjj Phase 6 placeholder with
 * a real boot through PreviewApp.
 */

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import PreviewApp from './PreviewApp';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PreviewApp />
  </StrictMode>,
);

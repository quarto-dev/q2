/**
 * Entry point for the q2-preview SPA.
 *
 * Phase A.3 (bd-o5wd) — replaces the bd-hfjj Phase 6 placeholder with
 * a real boot through PreviewApp.
 */

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import PreviewApp from './PreviewApp';

// The viewer only ever runs inside `q2 preview`, whose sessions are
// ephemeral (a fresh origin per session): keep the WASM bridge's
// quarto-cache in memory so stale IndexedDB databases don't accumulate
// across sessions (bd-91mdd056). Matches the hardcoded
// `storage: 'memory'` for the automerge repo in PreviewApp.tsx. Read by
// ts-packages/wasm-js-bridge/src/cache.js; must be set before the WASM
// first touches the cache.
(globalThis as Record<string, unknown>).__Q2_EPHEMERAL_STORAGE__ = true;

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PreviewApp />
  </StrictMode>,
);

/**
 * Entry point for the q2-preview SPA.
 *
 * Phase A.3 (bd-o5wd) — replaces the bd-hfjj Phase 6 placeholder with
 * a real boot through PreviewApp.
 */

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { startWorkerEngine } from '@quarto/preview-runtime/worker/engineClient';
import PreviewApp from './PreviewApp';
import { serverUrl } from './serverUrl';

// The viewer only ever runs inside `q2 preview`, whose sessions are
// ephemeral (a fresh origin per session): keep the WASM bridge's
// quarto-cache in memory so stale IndexedDB databases don't accumulate
// across sessions (bd-91mdd056). Matches the hardcoded
// `storage: 'memory'` for the automerge repo in PreviewApp.tsx. Read by
// ts-packages/wasm-js-bridge/src/cache.js; must be set before the WASM
// first touches the cache.
(globalThis as Record<string, unknown>).__Q2_EPHEMERAL_STORAGE__ = true;

// Preview VFS contract D3: the render engine runs in a dedicated worker.
// The worker receives the flag above in its init message. Installed before
// PreviewApp calls `initWasm`, which then starts the worker's engine. On a
// VFS miss the worker asks the server's miss routes (contract D2).
const engine = startWorkerEngine({ missBaseUrl: serverUrl('/api/preview/') });
// Test hook (like `__renderTicks`): the render worker's miss-route calls,
// keyed `<arm>.<status>` (preview VFS contract D2).
(window as unknown as { __q2MissStats?: () => Promise<Record<string, number>> }).__q2MissStats =
  () => engine.missStats();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PreviewApp />
  </StrictMode>,
);

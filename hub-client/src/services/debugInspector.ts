/**
 * Mount/unmount service for the in-context live inspector
 * (`quartoDebug.openInspector()` / `closeInspector()`).
 *
 * The panel renders into a SECOND React root on a body-appended div —
 * deliberately outside the App tree, so the inspector works even when
 * the app tree is in a broken state (it's a debugging surface). The
 * panel component, the reused /debug.html displays, and their CSS load
 * as a lazy chunk via dynamic import; none of it is in the main
 * bundle.
 *
 * Tracking: bd-lb1cxprv. Plan:
 * `claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md`.
 */

import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { getRepo } from '@quarto/preview-runtime';
import type { QuartoDebugAutomergeApi } from './debugAutomerge';

const CONTAINER_ID = 'quarto-debug-inspector-container';

let mounted: { root: Root; container: HTMLElement } | null = null;
let opening = false;

/**
 * Open the inspector over the live sync-client Repo. Throws when no
 * project is connected (there would be nothing to inspect). No-op if
 * already open.
 */
export async function openInspector(am: QuartoDebugAutomergeApi): Promise<void> {
  if (mounted || opening) return;
  const repo = getRepo();
  if (!repo) {
    throw new Error(
      'quartoDebug.openInspector: no project connected — open a project first',
    );
  }
  opening = true;
  try {
    const { DebugInspectorPanel } = await import(
      '../components/debug-inspector/DebugInspectorPanel'
    );
    const container = document.createElement('div');
    container.id = CONTAINER_ID;
    document.body.appendChild(container);
    const root = createRoot(container);
    root.render(
      createElement(DebugInspectorPanel, {
        repo,
        am,
        onClose: closeInspector,
      }),
    );
    mounted = { root, container };
  } finally {
    opening = false;
  }
}

/** Close the inspector if open. Idempotent. */
export function closeInspector(): void {
  if (!mounted) return;
  const { root, container } = mounted;
  mounted = null;
  root.unmount();
  container.remove();
}

export function isInspectorOpen(): boolean {
  return mounted !== null;
}

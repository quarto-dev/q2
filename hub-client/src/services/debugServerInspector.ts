/**
 * /debug.html iframe embed (`quartoDebug.openServerInspector()`).
 *
 * Injects the standalone Automerge debugger in an overlay iframe,
 * seeded with the current project's index doc via the `#doc=` hash
 * that `DebugApp` already understands. The iframe deliberately keeps
 * debug.html's own SERVER-connected ephemeral Repo — that is the
 * point: side by side with the live inspector (`openInspector()`) or
 * `quartoDebug.am`, it shows the sync server's view of the same
 * documents, so live-vs-server head divergence is directly visible.
 *
 * Plain DOM (header + close button + iframe) — no React, no lazy
 * chunk; the payload is the iframe itself.
 *
 * Tracking: bd-09aja9gl. Plan:
 * `claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md`.
 */

const CONTAINER_ID = 'quarto-debug-server-inspector-container';

let container: HTMLElement | null = null;

/**
 * Open the embedded server-view debugger seeded with the given index
 * doc id (bare or `automerge:`-prefixed). Throws when null (no
 * project connected). No-op if already open.
 */
export function openServerInspector(indexDocId: string | null): void {
  if (container) return;
  if (!indexDocId) {
    throw new Error(
      'quartoDebug.openServerInspector: no project connected — open a project first',
    );
  }
  const bare = indexDocId.startsWith('automerge:')
    ? indexDocId.slice('automerge:'.length)
    : indexDocId;
  // Resolved against the document base, matching how the SPA's own
  // relative assets load (vite `base: './'`).
  const src = new URL(`debug.html#doc=automerge:${bare}`, document.baseURI).href;

  const el = document.createElement('div');
  el.id = CONTAINER_ID;
  el.style.cssText = [
    'position: fixed',
    'inset: auto 0 0 0',
    'height: 60vh',
    'z-index: 10001',
    'display: flex',
    'flex-direction: column',
    'background: #181825',
    'border-top: 2px solid #f9e2af',
    'box-shadow: 0 -4px 24px rgba(0, 0, 0, 0.45)',
  ].join(';');

  const header = document.createElement('div');
  header.style.cssText = [
    'display: flex',
    'align-items: center',
    'gap: 12px',
    'padding: 4px 12px',
    'color: #f9e2af',
    "font: 600 12px 'Source Code Pro', ui-monospace, monospace",
  ].join(';');

  const title = document.createElement('span');
  title.textContent =
    'Server-view debugger (own connection — compare against the live inspector)';
  title.style.flex = '1 1 auto';

  const close = document.createElement('button');
  close.textContent = '×';
  close.setAttribute('aria-label', 'Close server inspector');
  close.title = 'Close server inspector';
  close.style.cssText =
    'background:#313244;color:#cdd6f4;border:1px solid #45475a;border-radius:4px;cursor:pointer;padding:0 8px;font:inherit';
  close.addEventListener('click', closeServerInspector);

  const iframe = document.createElement('iframe');
  iframe.src = src;
  iframe.title = 'Quarto Hub — Automerge Debugger (server view)';
  iframe.style.cssText = 'flex:1 1 auto;border:0;width:100%;background:#fff';

  header.append(title, close);
  el.append(header, iframe);
  document.body.appendChild(el);
  container = el;
}

/** Close the embedded debugger if open. Idempotent. */
export function closeServerInspector(): void {
  if (!container) return;
  container.remove();
  container = null;
}

export function isServerInspectorOpen(): boolean {
  return container !== null;
}

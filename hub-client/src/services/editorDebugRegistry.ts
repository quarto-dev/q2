/**
 * Registry connecting the live Monaco editor to the debug API without
 * coupling `debugAutomerge.ts` to the Editor component tree.
 *
 * `useAutomergeSync` registers a provider on mount; `quartoDebug.am
 * .doctor()` reads it to compare the Monaco model text against the
 * Automerge document (the classic divergence bug). Single-slot by
 * design: hub-client opens at most one editor at a time (see the
 * matching note in useAutomergeSync.ts).
 *
 * Tracking: bd-6ogrov5r. Plan:
 * `claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md`.
 */

export interface EditorTextProvider {
  /** Path of the file the editor currently shows, or null. */
  getPath(): string | null;
  /** Current Monaco model text, or null when no editor/model exists. */
  getText(): string | null;
}

let provider: EditorTextProvider | null = null;

/**
 * Register the live editor's text provider. Returns an unregister
 * function; unregistering only clears the slot if it still holds this
 * provider (a later registration wins).
 */
export function registerEditorTextProvider(p: EditorTextProvider): () => void {
  provider = p;
  return () => {
    if (provider === p) provider = null;
  };
}

/** The registered provider, or null when no editor is mounted. */
export function getEditorTextProvider(): EditorTextProvider | null {
  return provider;
}

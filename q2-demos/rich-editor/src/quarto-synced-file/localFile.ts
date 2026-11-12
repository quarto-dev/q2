/**
 * A qmd file held in memory, with the same interface as a synced one: for
 * running an editor with no hub.
 */

import type { EditorContentChange } from '@quarto/quarto-sync-client'
import { parseQmdContent } from './wasm.ts'
import type { SyncedFile, SyncedFileSnapshot } from './syncedFile.ts'

/** Must call initWasm() first. */
export function createLocalFile(initialText: string): SyncedFile {
  const listeners = new Set<() => void>()
  let snapshot: SyncedFileSnapshot = {
    text: null,
    ast: null,
    parseError: false,
    connected: true,
    connecting: false,
    error: null,
  }

  function setText(text: string) {
    const parsed = parseQmdContent(text)
    snapshot = { ...snapshot, text, ast: parsed ?? snapshot.ast, parseError: parsed === null }
    for (const listener of listeners) listener()
  }
  setText(initialText)

  return {
    subscribe(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    getSnapshot: () => snapshot,
    applyEdits(changes: EditorContentChange[]) {
      let text = snapshot.text ?? ''
      for (const c of changes) {
        text = text.slice(0, c.rangeOffset) + c.text + text.slice(c.rangeOffset + c.rangeLength)
      }
      if (text !== snapshot.text) setText(text)
    },
    dispose() {
      listeners.clear()
    },
  }
}

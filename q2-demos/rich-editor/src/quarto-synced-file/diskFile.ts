/**
 * A qmd file on disk, with the same interface as a synced one: for running
 * the editor on a local file with no sync server (the desktop app).
 *
 * The text lives in an automerge document that never touches a network.
 * Edits go into it and are saved, one save at a time and always the latest
 * text. A change made to the file by something else (`applyDiskText`) is
 * a change against what was last saved, so it's made at the document's
 * heads as of that save and merged: edits typed since, not saved yet, stay.
 *
 * Because it's automerge, connecting the document to a sync server too
 * would add live collaboration without changing any of this.
 */

import { change, changeAt, from, getHeads, splice, type Doc, type Heads } from '@automerge/automerge'
import type { EditorContentChange } from '@quarto/quarto-sync-client'
import { diffToChange } from './diff.ts'
import { parseQmdContent } from './wasm.ts'
import type { SyncedFile, SyncedFileSnapshot } from './syncedFile.ts'

type TextDoc = { text: string }

export interface DiskFile extends SyncedFile {
  /** The file's text as it now is on disk, changed by something else. */
  applyDiskText(text: string): void
}

/**
 * `save` writes a text to the file; the next save waits for it to finish.
 * Must call initWasm() first.
 */
export function createDiskFile(initialText: string, save: (text: string) => Promise<void>): DiskFile {
  const listeners = new Set<() => void>()
  let disposed = false
  let doc: Doc<TextDoc> = from({ text: initialText })
  // What the file holds, as far as we know: the last text saved (or
  // being saved), or read from disk, and the document's heads for it.
  let savedText = initialText
  let savedHeads: Heads = getHeads(doc)
  let saving = false

  let snapshot: SyncedFileSnapshot = {
    text: null,
    ast: null,
    parseError: false,
    connected: true,
    connecting: false,
    error: null,
  }

  function publish() {
    const text = doc.text
    if (text === snapshot.text) return
    const parsed = parseQmdContent(text)
    snapshot = { ...snapshot, text, ast: parsed ?? snapshot.ast, parseError: parsed === null }
    for (const listener of listeners) listener()
  }
  publish()

  function saveLatest() {
    if (saving || disposed || doc.text === savedText) return
    saving = true
    savedText = doc.text
    savedHeads = getHeads(doc)
    save(savedText).then(
      () => {
        saving = false
        saveLatest()
      },
      (e) => {
        saving = false
        snapshot = { ...snapshot, error: `Couldn't save the file: ${e instanceof Error ? e.message : String(e)}` }
        for (const listener of listeners) listener()
      },
    )
  }

  return {
    subscribe(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    getSnapshot: () => snapshot,
    applyEdits(changes: EditorContentChange[]) {
      if (disposed || changes.length === 0) return
      doc = change(doc, (d) => {
        for (const c of changes) splice(d, ['text'], c.rangeOffset, c.rangeLength, c.text)
      })
      publish()
      saveLatest()
    },
    applyDiskText(text: string) {
      if (disposed) return
      const edit = diffToChange(savedText, text)
      if (!edit) return
      const { newDoc, newHeads } = changeAt(doc, savedHeads, (d) => {
        splice(d, ['text'], edit.rangeOffset, edit.rangeLength, edit.text)
      })
      doc = newDoc
      savedText = text
      if (newHeads) savedHeads = newHeads
      publish()
      saveLatest()
    },
    dispose() {
      disposed = true
      listeners.clear()
    },
  }
}

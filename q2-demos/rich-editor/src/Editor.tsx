/**
 * The editor. Receives the file's source text and its parsed AST, and reports
 * edits as positional splices into the text (`onEdit` is null while
 * disconnected).
 *
 * Placeholder UI: a plain textarea, to prove the round trip.
 */

import type { ChangeEvent } from 'react'
import { diffToChange, type EditorContentChange } from './quarto-synced-file/index.ts'

interface EditorProps {
  text: string
  parseError: boolean
  onEdit: ((changes: EditorContentChange[]) => void) | null
}

export function Editor({ text, parseError, onEdit }: EditorProps) {
  function handleChange(e: ChangeEvent<HTMLTextAreaElement>) {
    if (!onEdit) return
    const change = diffToChange(text, e.target.value)
    if (change) onEdit([change])
  }

  return (
    <div className="editor">
      <textarea
        className="editor-source"
        value={text}
        onChange={handleChange}
        readOnly={!onEdit}
        spellCheck={false}
      />
      {parseError && <p className="status">Parse failed; showing the last good AST.</p>}
    </div>
  )
}

/**
 * The editor. Receives the file's source text and its parsed AST, and reports
 * edits as positional splices into the text (`onEdit` is null while
 * disconnected).
 *
 * Placeholder UI: a plain textarea, to prove the round trip, plus the AST's
 * top-level block types.
 */

import type { ChangeEvent } from 'react'
import { diffToChange, type EditorContentChange } from './quarto-synced-file/index.ts'
import type { RustQmdJson } from '@quarto/pandoc-types'

interface EditorProps {
  text: string
  ast: RustQmdJson | null
  parseError: boolean
  onEdit: ((changes: EditorContentChange[]) => void) | null
}

export function Editor({ text, ast, parseError, onEdit }: EditorProps) {
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
      <p className="status">
        {parseError && 'Parse failed; showing the last good AST. '}
        {ast
          ? `${ast.blocks.length} top-level blocks: ${ast.blocks.map((b) => b.t).join(', ')}`
          : 'No AST yet.'}
      </p>
    </div>
  )
}

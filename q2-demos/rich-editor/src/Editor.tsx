/**
 * The editor. Receives the file's source text and its parsed AST, and reports
 * edits as positional splices into the text (`onEdit` is null while
 * disconnected).
 *
 * Placeholder UI: a plain textarea, to prove the round trip, plus the AST's
 * top-level block types.
 */

import type { ChangeEvent } from 'react'
import type { EditorContentChange } from '@quarto/quarto-sync-client'
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

/** The single replaced range that turns `before` into `after`, or null if equal. */
function diffToChange(before: string, after: string): EditorContentChange | null {
  if (before === after) return null
  let start = 0
  const minLen = Math.min(before.length, after.length)
  while (start < minLen && before[start] === after[start]) start++
  let end = 0
  while (
    end < minLen - start &&
    before[before.length - 1 - end] === after[after.length - 1 - end]
  ) end++
  return {
    rangeOffset: start,
    rangeLength: before.length - start - end,
    text: after.slice(start, after.length - end),
  }
}

/**
 * A qmd file's metadata text fields, and its body, as the document an
 * editor edits (`FieldSource`): the title block's title, subtitle and
 * abstract, and the body as one multi-line field (see `body.ts`).
 *
 * Reading: a field's text is the plain text of its metadata value, as the
 * title block shows it; an abstract has one line per paragraph.
 *
 * Writing: the new text is put into the metadata as plain markdown text (a
 * typed `*` stays a `*`), the document is written back with the
 * incremental writer, and the difference from the current source is sent
 * as one splice. The incremental writer rewrites only the changed keys,
 * so nothing else in the source moves. An empty field is written as no
 * key at all, and spaces that qmd doesn't keep (at the ends of a line, or
 * more than one in a row) aren't written. In the body, only the
 * paragraphs whose text changed are written.
 */

import type { RustQmdJson } from '@quarto/pandoc-types'
import type { EditorContentChange } from '@quarto/quarto-sync-client'
import type { Fields } from '../engine/model.ts'
import type { FieldChanges, FieldSource } from '../engine/source.ts'
import { diffToChange } from '../quarto-synced-file/diff.ts'
import { incrementalWriteQmd } from '../quarto-synced-file/wasm.ts'
import { bodyText, editBody, kept as keptLine, words } from './body.ts'
import { metaText, type Meta } from './meta.ts'

/** The part of a synced (or local) file this needs. */
export interface QmdFile {
  subscribe(listener: () => void): () => void
  getSnapshot(): { text: string | null; ast: RustQmdJson | null; parseError: boolean; connected: boolean }
  /** On return, the snapshot has the edited text and its parse. */
  applyEdits(changes: EditorContentChange[]): void
}

export interface MetaFieldSourceOptions {
  /** The metadata keys to edit. */
  keys: readonly string[]
  /** Keys whose text has one paragraph per line. */
  multiline?: readonly string[]
  /** The name of the field that is the document's body, if it's edited. */
  body?: string
}

export function createMetaFieldSource(
  file: QmdFile,
  { keys, multiline = [], body }: MetaFieldSourceOptions,
): FieldSource {
  let readFrom: RustQmdJson | null | undefined
  let fields: Fields = {}

  const get = (): Fields => {
    const { ast } = file.getSnapshot()
    if (ast !== readFrom) {
      readFrom = ast
      const next = Object.fromEntries(
        keys.flatMap((key) => {
          const text = metaText(ast?.meta[key])
          return text ? [[key, text]] : []
        }),
      )
      const text = body === undefined || !ast ? '' : bodyText(ast.blocks as Node[])
      if (body !== undefined && text) next[body] = text
      // The same object while no field changes, however else the file does.
      if ([...keys, ...(body === undefined ? [] : [body])].some((key) => next[key] !== fields[key])) fields = next
    }
    return fields
  }

  const writable = (): boolean => {
    const { text, ast, parseError, connected } = file.getSnapshot()
    return connected && text !== null && ast !== null && !parseError
  }

  const set = (changes: FieldChanges): void => {
    const { text, ast } = file.getSnapshot()
    if (!writable() || text === null || ast === null) return
    const current = get()
    const meta: Meta = { ...ast.meta }
    let blocks = ast.blocks
    let changed = false
    for (const [key, value] of Object.entries(changes)) {
      if (key === body) {
        const edited = editBody(ast.blocks as Node[], value ?? '')
        if (!edited || (edited.length === blocks.length && edited.every((block, i) => block === blocks[i]))) continue
        changed = true
        blocks = edited as typeof blocks
        continue
      }
      const lines = kept(value ?? '', multiline.includes(key))
      // Nothing to write when the document already reads this way.
      if (lines.join('\n') === (current[key] ?? '')) continue
      changed = true
      if (lines.length === 0) delete meta[key]
      else if (multiline.includes(key)) meta[key] = paragraphs(lines) as Meta[string]
      else meta[key] = { t: 'MetaInlines', c: words(lines[0]) } as Meta[string]
    }
    if (!changed) return
    const change = diffToChange(text, incrementalWriteQmd(text, { ...ast, meta, blocks }))
    if (change) file.applyEdits([change])
  }

  return { get, writable, set, subscribe: (listener) => file.subscribe(listener) }
}

type Node = { t: string; c?: unknown }

/**
 * The lines of `text` that qmd keeps: each without the spaces at its ends
 * and with runs of spaces as one, and no empty lines. One line at most
 * unless `multiline`.
 */
function kept(text: string, multiline: boolean): string[] {
  const lines = multiline ? text.split('\n') : [text]
  return lines.map(keptLine).filter((line) => line !== '')
}

/** A text value with a paragraph per line. */
function paragraphs(lines: string[]): Node {
  return { t: 'MetaBlocks', c: lines.map((line) => ({ t: 'Para', c: words(line) })) }
}

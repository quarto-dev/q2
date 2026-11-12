/**
 * React bindings: `useFieldEditor` turns an element into the editing root
 * and owns every edit made in it, and `Field` renders one field's text
 * inside it.
 *
 * The browser moves the caret and selection (arrows, Shift-select,
 * dragging, the goal column across lines). Every edit is intercepted
 * before the browser makes it (`beforeinput`, paste, cut, drop), turned
 * into a model edit, and rendered back, with the selection restored from
 * the model. The one exception is IME composition, which can't be
 * cancelled: the browser edits the field's text itself, and the result is
 * read back when the composition ends.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import { deleteBackward, deleteForward, enter, insertText, select, selectAllStep } from './commands.ts'
import { markIslands, readSelection, selectionInFields, targetRange, writeSelection } from './dom.ts'
import { apply, createEditor, redo, undo, type ApplyOptions, type Editor } from './editor.ts'
import { selectedText, type Fields, type Schema, type State } from './model.ts'

export interface FieldProps {
  name: string
  /** The field's text; the field isn't rendered when it's undefined. */
  text: string | undefined
  /** Shown, faintly, while the field is empty (style `[data-placeholder]`). */
  placeholder?: string
  className?: string
}

/**
 * One field. React never renders its children: its text is written to the
 * DOM only when it differs from the model, so the browser's own text
 * (during IME composition) is left alone.
 */
export function Field({ name, text, placeholder, className }: FieldProps) {
  const ref = useRef<HTMLSpanElement | null>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el || text === undefined) return
    if (el.textContent !== text) el.textContent = text
    // An emptied element can keep a stray <br>.
    if (text === '' && el.firstChild) el.replaceChildren()
  }, [text])
  if (text === undefined) return null
  return <span ref={ref} className={className} data-field={name} data-placeholder={placeholder} />
}

export interface FieldEditor {
  state: State
  editor: Editor
}

/**
 * Edit `schema`'s fields inside `root`, starting from `initial`. A new
 * `resetKey` starts over from `initial`, clearing the history.
 */
export function useFieldEditor(
  root: HTMLElement | null,
  schema: Schema,
  initial: () => Fields,
  resetKey: unknown,
): FieldEditor {
  const [key, setKey] = useState(resetKey)
  const [editor, setEditor] = useState(() => createEditor(initial()))
  if (key !== resetKey) {
    setKey(resetKey)
    setEditor(createEditor(initial()))
  }

  // Events can arrive faster than React renders; they read and write this.
  const latest = useRef(editor)
  latest.current = editor
  // Whether the next render puts the page's selection back from the model.
  const restore = useRef(false)

  const commit = useCallback((next: Editor, restoreSelection: boolean) => {
    if (next === latest.current) return
    latest.current = next
    if (restoreSelection) restore.current = true
    setEditor(next)
  }, [])

  useLayoutEffect(() => {
    if (!root) return
    root.contentEditable = 'true'
    root.spellcheck = false
    return () => {
      root.removeAttribute('contenteditable')
    }
  }, [root])

  // After every render: keep the markup around the fields non-editable and
  // put the selection back after our own edits. (Runs after `Field`s have
  // written their text.)
  useLayoutEffect(() => {
    if (!root) return
    markIslands(root)
    if (!restore.current) return
    restore.current = false
    const selection = editor.present.selection
    if (selection) writeSelection(root, selection)
  })

  useEffect(() => {
    if (!root) return
    const doc = root.ownerDocument
    let composing = false

    /** Apply a command to the state with the page's current selection. */
    const edit = (command: (state: State) => State, options: Omit<ApplyOptions, 'before'> = {}) => {
      const h = latest.current
      const before = readSelection(root) ?? h.present.selection
      const at = select(schema, h.present, before)
      commit(apply(h, command(at), { ...options, before }), true)
    }

    const onBeforeInput = (e: InputEvent) => {
      if (composing || e.inputType === 'insertCompositionText') return
      e.preventDefault()
      const type = e.inputType
      const target = () => targetRange(root, e)
      if (type === 'historyUndo') commit(undo(latest.current), true)
      else if (type === 'historyRedo') commit(redo(latest.current), true)
      else if (type === 'insertText') edit((s) => insertText(schema, s, e.data ?? ''), { inputType: type })
      else if (type === 'insertReplacementText') {
        // Autocorrect or a spelling suggestion: replace the target range.
        const range = target()
        const text = e.dataTransfer?.getData('text/plain') ?? e.data ?? ''
        edit(
          (s) => insertText(schema, range ? select(schema, s, { anchor: range.from, focus: range.to }) : s, text),
          { inputType: type },
        )
      } else if (type === 'insertParagraph') edit((s) => enter(schema, s), { inputType: type })
      else if (type.startsWith('delete') && type.endsWith('Backward')) {
        edit((s) => deleteBackward(schema, s, target()), { inputType: type })
      } else if (type.startsWith('delete') && type.endsWith('Forward')) {
        edit((s) => deleteForward(schema, s, target()), { inputType: type })
      }
      // Everything else (line breaks, formatting, drag and drop) is ignored.
    }

    const onCompositionStart = () => {
      composing = true
    }
    const onCompositionEnd = () => {
      composing = false
      // The browser has already changed the field's text; read it back.
      const h = latest.current
      const selection = readSelection(root)
      const field = selection?.focus.field
      const el = field && root.querySelector<HTMLElement>(`[data-field="${field}"]`)
      if (!selection || !field || !el) return
      const text = el.textContent ?? ''
      if (text === h.present.fields[field]) return
      commit(
        apply(h, { fields: { ...h.present.fields, [field]: text }, selection }, { inputType: 'insertText' }),
        true,
      )
    }

    const onPaste = (e: ClipboardEvent) => {
      e.preventDefault()
      const text = (e.clipboardData?.getData('text/plain') ?? '').replace(/\s*[\r\n]+\s*/g, ' ')
      edit((s) => insertText(schema, s, text), { inputType: 'insertFromPaste' })
    }

    const copySelection = (e: ClipboardEvent) => {
      const { present } = latest.current
      const selection = readSelection(root) ?? present.selection
      if (!selection) return false
      e.preventDefault()
      e.clipboardData?.setData('text/plain', selectedText(schema, present.fields, selection))
      return true
    }
    const onCopy = (e: ClipboardEvent) => {
      copySelection(e)
    }
    const onCut = (e: ClipboardEvent) => {
      if (copySelection(e)) edit((s) => insertText(schema, s, ''), { inputType: 'deleteByCut' })
    }
    const onDrop = (e: DragEvent) => e.preventDefault()

    const onKeyDown = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey
      const key = e.key.toLowerCase()
      if (mod && key === 'z') {
        e.preventDefault()
        commit(e.shiftKey ? redo(latest.current) : undo(latest.current), true)
      } else if (e.ctrlKey && !e.metaKey && key === 'y') {
        e.preventDefault()
        commit(redo(latest.current), true)
      } else if (mod && key === 'a' && doc.activeElement === root) {
        e.preventDefault()
        edit((s) => selectAllStep(schema, s))
      }
    }

    // Reads the page's selection into the model. `selectionchange` arrives
    // up to a frame late, so `keyup` and `mouseup`, which follow the
    // browser's caret move, sync it as soon as it has happened.
    const syncSelection = () => {
      if (composing || doc.activeElement !== root) return
      const h = latest.current
      const selection = readSelection(root, h.present.selection)
      if (!selection) return
      // Rendered at once, like an edit: an empty field the caret just left
      // disappears in the same frame.
      flushSync(() => commit(apply(h, select(schema, h.present, selection)), false))
      // The browser put the caret outside the fields (past the last one,
      // say); put it where the model has it.
      if (!selectionInFields(root)) writeSelection(root, selection)
    }
    const onBlur = () => {
      const h = latest.current
      commit(apply(h, select(schema, h.present, null)), false)
    }

    root.addEventListener('beforeinput', onBeforeInput)
    root.addEventListener('compositionstart', onCompositionStart)
    root.addEventListener('compositionend', onCompositionEnd)
    root.addEventListener('paste', onPaste)
    root.addEventListener('copy', onCopy)
    root.addEventListener('cut', onCut)
    root.addEventListener('drop', onDrop)
    root.addEventListener('blur', onBlur)
    root.addEventListener('keyup', syncSelection)
    root.addEventListener('mouseup', syncSelection)
    doc.addEventListener('keydown', onKeyDown)
    doc.addEventListener('selectionchange', syncSelection)
    return () => {
      root.removeEventListener('beforeinput', onBeforeInput)
      root.removeEventListener('compositionstart', onCompositionStart)
      root.removeEventListener('compositionend', onCompositionEnd)
      root.removeEventListener('paste', onPaste)
      root.removeEventListener('copy', onCopy)
      root.removeEventListener('cut', onCut)
      root.removeEventListener('drop', onDrop)
      root.removeEventListener('blur', onBlur)
      root.removeEventListener('keyup', syncSelection)
      root.removeEventListener('mouseup', syncSelection)
      doc.removeEventListener('keydown', onKeyDown)
      doc.removeEventListener('selectionchange', syncSelection)
    }
  }, [root, schema, commit])

  return { state: editor.present, editor }
}

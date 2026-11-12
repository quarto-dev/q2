/**
 * The visual editor pane. Renders the document as `q2 render` would show
 * it, inside an iframe so the theme's Bootstrap CSS can't leak into the
 * rest of the app (hub-client's preview does the same).
 *
 * So far it renders only the title block, with hub-client's own
 * `PreviewTitleBlock`, from metadata derived on demand by
 * `deriveTitleBlockMeta` instead of a full render.
 *
 * Editing is local for now: edits change a local copy of the metadata,
 * with its own undo history, and never reach the source text. A change to
 * the source text (from the source pane or a collaborator) replaces the
 * local copy and clears the history.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { RustQmdJson } from '@quarto/pandoc-types'
import { PreviewTitleBlock, type TitleBlockField } from '@quarto/preview-renderer/title-block'
import { compileDefaultThemeCss, deriveTitleBlockMeta } from './quarto-synced-file/index.ts'
import { EditableText, type ArrowDirection } from './editing/EditableText.tsx'
import {
  createHistory,
  endGroup,
  record,
  redo,
  replacePresent,
  undo,
  type History,
} from './editing/history.ts'
import { placeCaretOnLine } from './editing/lines.ts'
import { setMetaText, type Meta } from './editing/meta.ts'
import { undoGroup } from './editing/undoGroup.ts'
import { setTextRange, type TextRange } from './editing/textRange.ts'

interface VisualEditorProps {
  ast: RustQmdJson | null
}

/** One undoable state: the metadata, and where the caret was. */
interface EditState {
  meta: Meta
  caret: { field: TitleBlockField; range: TextRange } | null
}

// The page structure `q2 render` emits for an article without a TOC.
const SRCDOC = `<!doctype html>
<html><head><meta charset="utf-8"></head>
<body class="fullcontent quarto-light">
<div id="quarto-content" class="quarto-container page-columns page-rows-contents page-layout-article">
<main class="content" id="quarto-document-content"></main>
</div>
</body></html>`

// Editable fields render exactly like the text they replace: no focus
// ring, no hover state. `min-height` keeps an emptied field one line tall;
// the placeholder is positioned out of the flow so the caret sits at the
// start of the line, not after the placeholder text.
const EDITING_CSS = `
.qe-field { display: block; position: relative; min-height: 1lh; outline: none; }
.qe-field:empty::before {
  content: attr(data-placeholder);
  position: absolute;
  pointer-events: none;
  color: #adb5bd;
}`

/** The editable fields in reading order: Enter and the arrows move along it. */
const FIELD_ORDER: TitleBlockField[] = ['title', 'subtitle']

const PLACEHOLDERS: Record<TitleBlockField, string> = {
  title: 'Title',
  subtitle: 'Subtitle',
}

const START = { start: 0, end: 0 }

const noop = () => {}

export function VisualEditor({ ast }: VisualEditorProps) {
  const [mount, setMount] = useState<HTMLElement | null>(null)
  const [frameDoc, setFrameDoc] = useState<Document | null>(null)
  const themeCss = useThemeCss()

  useEffect(() => {
    if (!frameDoc || themeCss === null) return
    const style = frameDoc.createElement('style')
    // dart-sass starts compressed output that contains non-ASCII text with
    // a byte-order mark. A <link>ed file treats it as an encoding marker,
    // but in a <style> element it becomes part of the first selector
    // (`﻿:root`), which then matches nothing and drops all of
    // Bootstrap's `:root` variables.
    style.textContent = themeCss.replace(/^﻿/, '')
    frameDoc.head.appendChild(style)
    return () => style.remove()
  }, [frameDoc, themeCss])

  useEffect(() => {
    if (!frameDoc) return
    const style = frameDoc.createElement('style')
    style.textContent = EDITING_CSS
    frameDoc.head.appendChild(style)
    return () => style.remove()
  }, [frameDoc])

  const { history, edit, leave, step, addEmpty } = useEditHistory(ast)
  const fields = useRef(new Map<TitleBlockField, HTMLElement>())

  // After an undo or redo, put the caret back where it was.
  const pendingCaret = useRef<EditState['caret']>(null)
  useLayoutEffect(() => {
    const caret = pendingCaret.current
    pendingCaret.current = null
    const el = caret && fields.current.get(caret.field)
    if (!caret || !el) return
    el.focus()
    setTextRange(el, caret.range)
  }, [history])

  const doUndo = useCallback(() => {
    pendingCaret.current = step('undo')
  }, [step])
  const doRedo = useCallback(() => {
    pendingCaret.current = step('redo')
  }, [step])

  /**
   * Enter: move the caret to the start of the next field, never splitting
   * the text. A missing subtitle is added as an empty line, which goes away
   * again if it's left empty.
   */
  const enter = useCallback(
    (field: TitleBlockField) => {
      const next = FIELD_ORDER[FIELD_ORDER.indexOf(field) + 1]
      if (!next) return
      const el = fields.current.get(next)
      if (el) {
        el.focus()
        setTextRange(el, START)
      } else if (next === 'subtitle') {
        pendingCaret.current = { field: next, range: START }
        addEmpty(next)
      }
    },
    [addEmpty],
  )

  /**
   * Arrows past a field's edge go to the nearest field that way: Up / Down
   * keep the caret's horizontal position, Left lands at the end of the
   * previous field and Right at the start of the next, as between
   * paragraphs in a word processor.
   */
  const arrowOut = useCallback((field: TitleBlockField, direction: ArrowDirection, x: number) => {
    const i = FIELD_ORDER.indexOf(field)
    const forward = direction === 'down' || direction === 'right'
    const order = forward ? FIELD_ORDER.slice(i + 1) : FIELD_ORDER.slice(0, i).reverse()
    const target = order.map((f) => fields.current.get(f)).find((el) => el !== undefined)
    if (!target) return false
    if (direction === 'up' || direction === 'down') {
      placeCaretOnLine(target, x, forward ? 'first' : 'last')
    } else {
      target.focus()
      const at = forward ? 0 : (target.textContent ?? '').length
      setTextRange(target, { start: at, end: at })
    }
    return true
  }, [])

  // Undo and redo with the caret outside any field.
  useEffect(() => {
    if (!frameDoc) return
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.target as Element | null)?.closest?.('.qe-field')) return
      const key = e.key.toLowerCase()
      if ((e.metaKey || e.ctrlKey) && key === 'z') {
        e.preventDefault()
        if (e.shiftKey) doRedo()
        else doUndo()
      } else if (e.ctrlKey && !e.metaKey && key === 'y') {
        e.preventDefault()
        doRedo()
      }
    }
    frameDoc.addEventListener('keydown', onKeyDown)
    return () => frameDoc.removeEventListener('keydown', onKeyDown)
  }, [frameDoc, doUndo, doRedo])

  const derived = useMemo(() => {
    if (!ast || !history) return null
    const start = performance.now()
    const { meta, issues } = deriveTitleBlockMeta({ ...ast, meta: history.present.meta })
    return { ast: { ...ast, meta }, issues, ms: performance.now() - start }
  }, [ast, history])

  const renderField = useCallback(
    (field: TitleBlockField, text: string | undefined) =>
      text === undefined ? null : (
        <EditableText
          value={text}
          placeholder={PLACEHOLDERS[field]}
          onEdit={(next, inputType, before, after) => edit(field, next, inputType, before, after)}
          onUndo={doUndo}
          onRedo={doRedo}
          onEnter={() => enter(field)}
          onArrowOut={(direction, x) => arrowOut(field, direction, x)}
          onLeave={() => leave(field)}
          elementRef={(el) => {
            if (el) fields.current.set(field, el)
            else fields.current.delete(field)
          }}
        />
      ),
    [edit, leave, enter, arrowOut, doUndo, doRedo],
  )

  return (
    <div className="visual-editor">
      <iframe
        className="visual-frame"
        title="Visual editor"
        srcDoc={SRCDOC}
        onLoad={(e) => {
          const doc = e.currentTarget.contentDocument
          setFrameDoc(doc)
          setMount(doc?.getElementById('quarto-document-content') ?? null)
        }}
      />
      <p className="status">
        {themeCss === null ? 'Compiling theme CSS… ' : ''}
        {derived && `Title block derived in ${derived.ms.toFixed(2)} ms. `}
        {history && history.past.length > 0 && 'Local edits only; not saved to the source. '}
        {derived && derived.issues.length > 0 && ` Issues: ${derived.issues.join('; ')}`}
      </p>
      {mount && derived &&
        createPortal(
          // PreviewTitleBlock is typed against preview-renderer's own AST
          // type; the JSON shape is the same.
          <PreviewTitleBlock ast={derived.ast as never} setAst={noop} renderField={renderField} />,
          mount,
        )}
    </div>
  )
}

/**
 * The local edit history, seeded from the source AST's metadata and reset
 * whenever the source AST changes.
 */
function useEditHistory(ast: RustQmdJson | null) {
  const [source, setSource] = useState(ast)
  const [history, setHistory] = useState<History<EditState> | null>(() =>
    ast ? createHistory({ meta: ast.meta, caret: null }) : null,
  )
  if (ast !== source) {
    setSource(ast)
    setHistory(ast ? createHistory({ meta: ast.meta, caret: null }) : null)
  }

  // Read by `step`, which must report the caret it moved to synchronously.
  const latest = useRef(history)
  latest.current = history

  const edit = useCallback(
    (
      field: TitleBlockField,
      text: string,
      inputType: string,
      before: TextRange | null,
      after: TextRange | null,
    ) => {
      setHistory((h) => {
        if (!h) return h
        const caretAt = (range: TextRange | null) => (range ? { field, range } : null)
        const last = h.present.caret
        const lastAfter = last?.field === field ? last.range : null
        return record(
          h,
          { meta: setMetaText(h.present.meta, field, text), caret: caretAt(after) },
          {
            group: undoGroup(field, inputType, before, lastAfter),
            before: { ...h.present, caret: caretAt(before) },
          },
        )
      })
    },
    [],
  )

  /**
   * End the field's undo step. An emptied field is removed, without an undo
   * step of its own: undo brings back the text that was deleted.
   */
  const leave = useCallback((field: TitleBlockField) => {
    setHistory((h) => {
      if (!h) return h
      const value = h.present.meta[field] as { t: string; c: unknown } | undefined
      if (value?.t === 'MetaString' && value.c === '') {
        h = replacePresent(h, { ...h.present, meta: setMetaText(h.present.meta, field, undefined) })
      }
      return endGroup(h)
    })
  }, [])

  /** Add an empty field, without an undo step (it's removed if left empty). */
  const addEmpty = useCallback((field: TitleBlockField) => {
    setHistory((h) => h && replacePresent(h, { ...h.present, meta: setMetaText(h.present.meta, field, '') }))
  }, [])

  /** Undo or redo; returns the caret position of the state moved to. */
  const step = useCallback((direction: 'undo' | 'redo'): EditState['caret'] => {
    const h = latest.current
    if (!h) return null
    const next = direction === 'undo' ? undo(h) : redo(h)
    if (next === h) return null
    latest.current = next
    setHistory(next)
    return next.present.caret
  }, [])

  return { history, edit, leave, step, addEmpty }
}

/** The default theme's CSS, compiled once per page load. */
let themeCssPromise: Promise<string> | null = null

function useThemeCss(): string | null {
  const [css, setCss] = useState<string | null>(null)
  useEffect(() => {
    themeCssPromise ??= compileDefaultThemeCss()
    let cancelled = false
    themeCssPromise.then(
      (result) => !cancelled && setCss(result),
      (err) => console.error(err),
    )
    return () => {
      cancelled = true
    }
  }, [])
  return css
}

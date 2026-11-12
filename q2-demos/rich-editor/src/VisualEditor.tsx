/**
 * The visual editor pane. Renders the document as `q2 render` would show
 * it, inside an iframe so the theme's Bootstrap CSS can't leak into the
 * rest of the app (hub-client's preview does the same).
 *
 * The title block is hub-client's own `PreviewTitleBlock`, from metadata
 * derived on demand by `deriveTitleBlockMeta` instead of a full render.
 * The body is under it, block by block, from the parsed source.
 *
 * The title, subtitle and abstract are edited in place, and so are the
 * body's paragraphs of plain text; the rest of the body (headings,
 * formatted text, lists) is shown and can't be edited yet. A document has
 * only the title-block fields its author adds, with the "+" row above the
 * title block (`TitleBlockWidget`); an added field stays, empty, until
 * Backspace in it removes it. Every edit is
 * written to the file's source text as it's made, and a change to the
 * source from elsewhere (the source pane or a collaborator) shows up here
 * without disturbing the caret, the selection or the undo history. The
 * editing itself is the generic engine in `engine/`; `editing/` connects
 * it to the qmd file. Nothing can be edited while the source doesn't
 * parse.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useState, useSyncExternalStore } from 'react'
import { createPortal } from 'react-dom'
import { PreviewTitleBlock, type TitleBlockField } from '@quarto/preview-renderer/title-block'
import { compileDefaultThemeCss, deriveTitleBlockMeta } from './quarto-synced-file/index.ts'
import { Field, Lines, useFieldEditor, type LeaveStart } from './engine/react.tsx'
import type { Fields, Schema } from './engine/model.ts'
import { readOnlyBlocks, type Node } from './editing/body.ts'
import { createMetaFieldSource, type QmdFile } from './editing/fieldSource.ts'
import { setMetaText, type Meta } from './editing/meta.ts'
import { ReadOnlyBlock } from './ReadOnlyBlock.tsx'
import { TitleBlockWidget, type WidgetFocus } from './TitleBlockWidget.tsx'

interface VisualEditorProps {
  file: QmdFile
}

// The page structure `q2 render` emits for an article without a TOC.
const SRCDOC = `<!doctype html>
<html><head><meta charset="utf-8"></head>
<body class="fullcontent quarto-light">
<div id="quarto-content" class="quarto-container page-columns page-rows-contents page-layout-article">
<main class="content" id="quarto-document-content"></main>
</div>
</body></html>`

// The editing root and its fields render exactly like the text they
// replace: no focus ring, no hover state. `pre-wrap` keeps a trailing
// space a caret position, as in the source text. `min-height` keeps an
// emptied field one line tall; the placeholder is positioned out of the flow so
// the caret sits at the start of the line, not after the placeholder text. (An
// empty field holds a <br>, so it's marked `data-empty` rather than `:empty`.)
const EDITING_CSS = `
#quarto-document-content[contenteditable] { outline: none; }
.qe-unknown { margin-bottom: 1rem; padding: 0.5rem 0.75rem; border: 1px dashed #ced4da; color: #6c757d; font-size: 0.875em; }
.qe-field { display: block; position: relative; min-height: 1lh; outline: none; white-space: pre-wrap; }
.qe-field[data-empty][data-placeholder]::before {
  content: attr(data-placeholder);
  position: absolute;
  pointer-events: none;
  color: #adb5bd;
}`

/** The title block's editable fields, in reading order. */
const TITLE_BLOCK = ['title', 'subtitle', 'abstract'] satisfies TitleBlockField[]

/**
 * The editable fields, in reading order: Enter, Backspace at a field's
 * start, and selections across fields follow it. Enter goes to the next
 * field there is, and from the last one-line field of the title block to
 * the body, starting one if there's none; it never adds a title-block
 * field (the "+" row does). The abstract and the body take line breaks
 * (one per paragraph). The title-block fields stay when empty.
 */
const SCHEMA: Schema = {
  order: [...TITLE_BLOCK, 'body'],
  addable: ['body'],
  multiline: ['abstract', 'body'],
  keepEmpty: TITLE_BLOCK,
}

const PLACEHOLDERS: Record<TitleBlockField, string> = {
  title: 'Title',
  subtitle: 'Subtitle',
  abstract: 'Abstract',
}

/** The fields the "+" row adds, in reading order. */
const ADDABLE = TITLE_BLOCK.map((name) => ({ name, label: PLACEHOLDERS[name] }))

const noop = () => {}

export function VisualEditor({ file }: VisualEditorProps) {
  const { ast, parseError } = useSyncExternalStore(file.subscribe, file.getSnapshot)
  const source = useMemo(
    () =>
      createMetaFieldSource(file, { keys: TITLE_BLOCK, multiline: ['abstract'], body: 'body', keepEmpty: TITLE_BLOCK }),
    [file],
  )
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

  // The whole document is one editing surface, once it has a field to
  // edit; the engine keeps the caret inside the fields.
  const [root, setRoot] = useState<HTMLElement | null>(null)
  useLayoutEffect(() => {
    const ready = mount?.querySelector('[data-field]') ? mount : null
    if (ready !== root) setRoot(ready)
  })
  // Where the focus goes in the "+" row when the caret leaves the fields for it.
  const [widgetFocus, setWidgetFocus] = useState<WidgetFocus | null>(null)
  const onLeaveStart = useCallback((how: LeaveStart) => setWidgetFocus({ field: how.removed, x: how.x }), [])
  const { state, writable, add, focusStart } = useFieldEditor(root, SCHEMA, source, { autoFocus: true, onLeaveStart })
  const present = useMemo(() => new Set(TITLE_BLOCK.filter((f) => state.fields[f] !== undefined)), [state.fields])
  const caretInTitleBlock = TITLE_BLOCK.some((f) => state.selection?.focus.field === f)
  const empty = present.size === 0 && !state.fields.body

  const derived = useMemo(() => {
    if (!ast) return null
    const start = performance.now()
    const { meta, issues } = deriveTitleBlockMeta({ ...ast, meta: metaWithFields(ast.meta, state.fields) })
    return { ast: { ...ast, meta }, issues, ms: performance.now() - start }
  }, [ast, state.fields])

  // The blocks the body's read-only lines stand for, in order.
  const readOnly = useMemo(() => (ast ? readOnlyBlocks(ast.blocks as Node[]) : []), [ast])
  const renderAtom = useCallback(
    (index: number) => (readOnly[index] ? <ReadOnlyBlock block={readOnly[index]} /> : null),
    [readOnly],
  )

  const renderField = useCallback(
    (field: TitleBlockField) =>
      state.fields[field] === undefined ? null : (
        <Field name={field} text={state.fields[field]} placeholder={PLACEHOLDERS[field]} className="qe-field" />
      ),
    [state.fields],
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
        {parseError
          ? "Read-only: the source doesn't parse. "
          : !writable && 'Read-only: not connected. '}
        {derived && derived.issues.length > 0 && ` Issues: ${derived.issues.join('; ')}`}
      </p>
      {mount && frameDoc && writable && (
        <TitleBlockWidget
          doc={frameDoc}
          content={mount}
          fields={ADDABLE}
          present={present}
          caretInTitleBlock={caretInTitleBlock}
          empty={empty}
          focus={widgetFocus}
          onFocused={() => setWidgetFocus(null)}
          onAdd={add}
          onExit={focusStart}
        />
      )}
      {mount && derived &&
        createPortal(
          <>
            {/* PreviewTitleBlock is typed against preview-renderer's own AST
                type; the JSON shape is the same. */}
            <PreviewTitleBlock ast={derived.ast as never} setAst={noop} renderField={renderField} />
            <Lines name="body" text={state.fields.body} className="qe-field" renderAtom={renderAtom} />
          </>,
          mount,
        )}
    </div>
  )
}

/** The source metadata with the edited fields written over it. */
function metaWithFields(meta: Meta, fields: Fields): Meta {
  return TITLE_BLOCK.reduce((m, f) => setMetaText(m, f, fields[f]), meta)
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

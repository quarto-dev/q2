/**
 * The visual editor pane. Renders the document as `q2 render` would show
 * it, inside an iframe so the theme's Bootstrap CSS can't leak into the
 * rest of the app (hub-client's preview does the same).
 *
 * So far it renders only the title block, with hub-client's own
 * `PreviewTitleBlock`, from metadata derived on demand by
 * `deriveTitleBlockMeta` instead of a full render.
 *
 * Editing is local for now: edits change a local copy of the title-block
 * fields, with their own undo history, and never reach the source text. A
 * change to the source text (from the source pane or a collaborator)
 * replaces the local copy and clears the history. The editing itself is
 * the generic engine in `engine/`.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import type { RustQmdJson } from '@quarto/pandoc-types'
import { PreviewTitleBlock, type TitleBlockField } from '@quarto/preview-renderer/title-block'
import { compileDefaultThemeCss, deriveTitleBlockMeta } from './quarto-synced-file/index.ts'
import { Field, useFieldEditor } from './engine/react.tsx'
import type { Fields, Schema } from './engine/model.ts'
import { metaText, setMetaText, type Meta } from './editing/meta.ts'

interface VisualEditorProps {
  ast: RustQmdJson | null
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
// the caret sits at the start of the line, not after the placeholder text.
const EDITING_CSS = `
#title-block-header[contenteditable] { outline: none; }
.qe-field { display: block; position: relative; min-height: 1lh; outline: none; white-space: pre-wrap; }
.qe-field:empty::before {
  content: attr(data-placeholder);
  position: absolute;
  pointer-events: none;
  color: #adb5bd;
}`

/**
 * The title block's editable fields, in reading order: Enter, Backspace at
 * a field's start, and selections across fields follow it.
 */
const SCHEMA: Schema = { order: ['title', 'subtitle'] satisfies TitleBlockField[] }

const PLACEHOLDERS: Record<TitleBlockField, string> = {
  title: 'Title',
  subtitle: 'Subtitle',
}

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

  // The whole title block is one editing surface; the engine keeps the
  // caret inside the fields.
  const [root, setRoot] = useState<HTMLElement | null>(null)
  useLayoutEffect(() => {
    const header = mount?.querySelector<HTMLElement>('#title-block-header') ?? null
    if (header !== root) setRoot(header)
  })
  const { state, editor } = useFieldEditor(root, SCHEMA, () => fieldsFromMeta(ast?.meta), ast)

  const derived = useMemo(() => {
    if (!ast) return null
    const start = performance.now()
    const { meta, issues } = deriveTitleBlockMeta({ ...ast, meta: metaWithFields(ast.meta, state.fields) })
    return { ast: { ...ast, meta }, issues, ms: performance.now() - start }
  }, [ast, state.fields])

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
        {editor.past.length > 0 && 'Local edits only; not saved to the source. '}
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

function fieldsFromMeta(meta: Meta | undefined): Fields {
  return Object.fromEntries(SCHEMA.order.map((f) => [f, metaText(meta?.[f])]))
}

/** The source metadata with the edited fields written over it. */
function metaWithFields(meta: Meta, fields: Fields): Meta {
  return SCHEMA.order.reduce((m, f) => setMetaText(m, f, fields[f]), meta)
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

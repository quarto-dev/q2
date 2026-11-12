/**
 * The visual editor pane. Renders the document as `q2 render` would show
 * it, inside an iframe so the theme's Bootstrap CSS can't leak into the
 * rest of the app (hub-client's preview does the same).
 *
 * So far it renders only the title block, with hub-client's own
 * `PreviewTitleBlock`, from metadata derived on demand by
 * `deriveTitleBlockMeta` instead of a full render.
 */

import { useEffect, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import type { RustQmdJson } from '@quarto/pandoc-types'
import { PreviewTitleBlock } from '@quarto/preview-renderer/title-block'
import { compileDefaultThemeCss, deriveTitleBlockMeta } from './quarto-synced-file/index.ts'

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

  const derived = useMemo(() => {
    if (!ast) return null
    const start = performance.now()
    const { meta, issues } = deriveTitleBlockMeta(ast)
    return { ast: { ...ast, meta }, issues, ms: performance.now() - start }
  }, [ast])

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
        {derived && `Title block derived in ${derived.ms.toFixed(2)} ms.`}
        {derived && derived.issues.length > 0 && ` Issues: ${derived.issues.join('; ')}`}
      </p>
      {mount && derived &&
        createPortal(
          // PreviewTitleBlock is typed against preview-renderer's own AST
          // type; the JSON shape is the same.
          <PreviewTitleBlock ast={derived.ast as never} setAst={noop} />,
          mount,
        )}
    </div>
  )
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

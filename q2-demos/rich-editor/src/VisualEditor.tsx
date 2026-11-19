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
 * body's paragraphs, headings, one-paragraph quotes and tight bullet and
 * numbered lists of text, formatted or not, and its horizontal rules can
 * be made and removed; the rest of the body (text with math, notes or
 * images, other lists, code) is shown and can't be edited yet. Lists
 * in items are edited as the lists they're in, and Tab and Shift+Tab (or
 * the toolbar's indent buttons) indent and outdent items. A
 * document has only the title-block fields its author adds, with the "+"
 * row above the title block (`TitleBlockWidget`); an added field stays,
 * empty, until Backspace in it removes it. Every edit is
 * written to the file's source text as it's made, and a change to the
 * source from elsewhere (the source pane or a collaborator) shows up here
 * without disturbing the caret, the selection or the undo history. The
 * editing itself is the generic engine in `engine/`; `editing/` connects
 * it to the qmd file. Nothing can be edited while the source doesn't
 * parse. Above the document, a toolbar (`Toolbar`) shows and changes the
 * style of the caret's line and the formatting of the selection. The
 * body's text can be formatted (bold, italic, underline, strikethrough,
 * highlight, code, links); a link's popover and the form that makes links
 * are `LinkPopover`.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useState, useSyncExternalStore, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { PreviewTitleBlock, type TitleBlockField } from '@quarto/preview-renderer/title-block'
import { compileDefaultThemeCss, deriveTitleBlockMeta } from './quarto-synced-file/index.ts'
import { Field, Lines, useFieldEditor, type LeaveStart } from './engine/react.tsx'
import { isCollapsed, ordered, selectedText, type Fields, type Schema } from './engine/model.ts'
import { activeMarks, linkAround } from './engine/commands.ts'
import type { MarkView } from './engine/dom.ts'
import { CODE, EMPH, formatElement, HIGHLIGHT, inlineRule, LEAF, STRIKEOUT, STRONG, TWO_SIDED, UNDERLINE } from './editing/inlines.ts'
import { LinkPopover, type LinkForm } from './LinkPopover.tsx'
import {
  BULLET,
  blocksOfLines,
  bodyInputRule,
  figureImage,
  headingAttr,
  headingLevel,
  isFigure,
  isItem,
  isNumbered,
  isQuote,
  itemDepth,
  listStart,
  NUMBERED,
  outdentPrefix,
  prefixMarkdown,
  readOnlyBlocks,
  type Node,
} from './editing/body.ts'
import { createMetaFieldSource, type QmdFile } from './editing/fieldSource.ts'
import { canChoose, canIndent, canList, canQuote, caretLine, chooseStyle, indent, toggleList, toggleQuote } from './editing/lineStyles.ts'
import type { LineParent } from './engine/react.tsx'
import { setMetaText, type Meta } from './editing/meta.ts'
import { FigureFrame, hasOwnAlt, ReadOnlyBlock } from './ReadOnlyBlock.tsx'
import { withoutMarks } from './engine/marks.ts'
import { TitleBlockWidget, type WidgetFocus } from './TitleBlockWidget.tsx'
import { Toolbar } from './Toolbar.tsx'
import { matches, SHORTCUTS, styleShortcut } from './shortcuts.ts'

interface VisualEditorProps {
  file: QmdFile
  /** The toolbar's Show source button (absent: no source pane to show). */
  devTools?: { open: boolean; toggle: () => void }
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
li.qe-field { display: list-item; }
li.qe-nest { list-style: none; }
.qe-field[data-empty][data-placeholder]::before {
  content: attr(data-placeholder);
  position: absolute;
  pointer-events: none;
  color: #adb5bd;
}
.qe-field a { cursor: text; }
.qe-bubble {
  position: absolute; z-index: 10; display: flex; align-items: center; gap: 2px;
  padding: 4px 6px; background: #fff; border: 1px solid #dee2e6; border-radius: 7px;
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.12); font: 13px system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
  white-space: nowrap; color: #212529;
}
.qe-bubble button {
  font: inherit; border: 1px solid transparent; background: none; padding: 4px 9px; border-radius: 5px;
  cursor: pointer; color: #343a40;
}
.qe-bubble button:hover { background: #f1f3f5; }
.qe-bubble a:focus, .qe-bubble button:focus { outline: 2px solid #2780e3; outline-offset: -1px; border-radius: 5px; }
.qe-bubble .qe-url { color: #2780e3; max-width: 260px; overflow: hidden; text-overflow: ellipsis; padding: 0 6px; }
.qe-bubble .qe-none { color: #adb5bd; padding: 0 6px; }
.qe-bubble .qe-bsep { width: 1px; height: 18px; background: #dee2e6; margin: 0 4px; }
.qe-linkform form { display: flex; flex-direction: column; gap: 6px; width: 300px; padding: 6px 4px; white-space: normal; }
.qe-linkform label { display: grid; grid-template-columns: 38px 1fr; align-items: center; color: #6c757d; font-size: 12px; }
.qe-linkform input { font: 13px system-ui, sans-serif; padding: 4px 7px; border: 1px solid #dee2e6; border-radius: 4px; outline: none; }
.qe-linkform input:focus { border-color: #2780e3; }
.qe-linkform .qe-row { display: flex; justify-content: flex-end; gap: 4px; }
.qe-linkform .qe-apply { background: #2780e3; color: #fff; }
.qe-linkform .qe-apply:hover { background: #1f6ab8; }`

/** How the body's formatting is shown: as `q2 render` writes it. */
const MARK_VIEW: MarkView = { element: formatElement, twoSided: TWO_SIDED, leaf: LEAF }

/**
 * The formatting shortcuts (see `shortcuts.ts`): ⌘B bold, ⌘I italic, ⌘U
 * underline, ⇧⌘X strikethrough, ⌘E code and ⇧⌘H highlight. (⌘K, the link
 * form, is handled on its own.)
 */
const FORMAT_SHORTCUTS = [
  { shortcut: SHORTCUTS.bold, format: STRONG },
  { shortcut: SHORTCUTS.italic, format: EMPH },
  { shortcut: SHORTCUTS.underline, format: UNDERLINE },
  { shortcut: SHORTCUTS.strikeout, format: STRIKEOUT },
  { shortcut: SHORTCUTS.code, format: CODE },
  { shortcut: SHORTCUTS.highlight, format: HIGHLIGHT },
]

/** The line styles' shortcuts: ⌥⌘0 Paragraph, ⌥⌘1 to ⌥⌘4 the headings. */
const STYLE_LEVELS = [0, 1, 2, 3, 4]

/** The title block's editable fields, in reading order. */
const TITLE_BLOCK = ['title', 'subtitle', 'abstract'] satisfies TitleBlockField[]

/**
 * The editable fields, in reading order: Enter, Backspace at a field's
 * start, and selections across fields follow it. Enter goes to the next
 * field there is, and from the last one-line field of the title block to
 * the body, starting one if there's none; it never adds a title-block
 * field (the "+" row does). Backspace at the start of the body stays there,
 * unless the body is empty. The abstract and the body take line breaks
 * (one per paragraph). The title-block fields stay when empty.
 * In the body, `## ` at the start of a paragraph makes it a heading,
 * `> ` a quote, `- ` a list item and `1. ` a numbered one, and they're
 * copied with their `#`s, `>`, `-` and `1.`. Enter in an item starts
 * another, and on an empty one ends the list, or, nested, moves out a
 * level, as Backspace at the start of its text does. `---` there makes a horizontal rule, which is copied as `---`.
 * A figure's caption is a line too, which holds its image (see
 * `editing/body.ts`): Enter in its middle moves the rest of it to a
 * paragraph, and it's copied as its text alone. A figure with no caption
 * is its image alone, which can be removed but not edited, as a rule can.
 * The body's lines can be formatted (bold, italic, underline,
 * strikethrough, highlight, code and links: see `editing/inlines.ts`), and
 * markdown typed out and closed there becomes formatting. Code has a
 * caret position inside and one outside at each edge.
 */
const SCHEMA: Schema = {
  order: [...TITLE_BLOCK, 'body'],
  addable: ['body'],
  multiline: ['abstract', 'body'],
  inputRule: (field, line) => (field === 'body' ? bodyInputRule(line) : null),
  prefixText: prefixMarkdown,
  continued: isItem,
  splitPrefix: (prefix) => (isFigure(prefix) ? '' : prefix),
  voidWhenEmpty: isFigure,
  outdent: outdentPrefix,
  voidText: '---',
  keepEmpty: TITLE_BLOCK,
  always: ['body'],
  backspaceStops: ['body'],
  marks: { fields: ['body'], twoSided: TWO_SIDED, leaf: LEAF },
  inlineRule: (field, line, caret) => (field === 'body' ? inlineRule(line, caret) : null),
}

const PLACEHOLDERS: Record<TitleBlockField, string> = {
  title: 'Title',
  subtitle: 'Subtitle',
  abstract: 'Abstract',
}

/** The fields the "+" row adds, in reading order. */
const ADDABLE = TITLE_BLOCK.map((name) => ({ name, label: PLACEHOLDERS[name] }))

const noop = () => {}

const renderRule = () => <hr />

/** The `li` a nested list is in, after its parent item's line. */
const NESTED: LineParent = { as: 'li', className: 'qe-nest', shared: true }

export function VisualEditor({ file, devTools }: VisualEditorProps) {
  const { ast, parseError, connected } = useSyncExternalStore(file.subscribe, file.getSnapshot)
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
  const { state, editor, writable, add, focusStart, setPrefixes, setFieldPrefixes, undo, redo, toggleMark, setLink, focusAt } = useFieldEditor(root, SCHEMA, source, {
    autoFocus: true,
    onLeaveStart,
  })
  const marks = writable ? activeMarks(SCHEMA, state) : null
  const caretLink = linkAround(SCHEMA, state)
  const [linkForm, setLinkForm] = useState<LinkForm | null>(null)

  // ⌘K and the toolbar's link button: the form for the caret's link, or for
  // making the selected text (in one line) a link.
  const openLinkForm = useCallback(() => {
    const selection = state.selection
    if (!selection || !writable || marks === null) return
    if (caretLink) {
      const text = selectedText(SCHEMA, state.fields, { anchor: caretLink.from, focus: caretLink.to })
      setLinkForm({ from: caretLink.from, to: caretLink.to, text, url: caretLink.url, selection })
      return
    }
    const { from, to } = ordered(SCHEMA, selection)
    const text = isCollapsed(selection) ? '' : selectedText(SCHEMA, state.fields, selection)
    if (from.field !== to.field || text.includes('\n')) return
    setLinkForm({ from, to, text, url: '', selection })
  }, [state, writable, marks, caretLink])

  // The caret's line in the body, for the line styles' shortcuts.
  const line = caretLine(state, writable)

  // The formatting and line style shortcuts, while the caret is in the
  // document. A line style's shortcut does what the toolbar's button or
  // menu item does, and nothing where that's disabled.
  useEffect(() => {
    if (!frameDoc || !root) return
    const onKey = (e: KeyboardEvent) => {
      if (frameDoc.activeElement !== root) return
      const format = FORMAT_SHORTCUTS.find((f) => matches(e, f.shortcut))
      const level = STYLE_LEVELS.find((l) => matches(e, styleShortcut(l)))
      if (format) {
        e.preventDefault()
        toggleMark(format.format)
      } else if (matches(e, SHORTCUTS.link)) {
        e.preventDefault()
        openLinkForm()
      } else if (matches(e, SHORTCUTS.bulletList) || matches(e, SHORTCUTS.numberedList)) {
        e.preventDefault()
        const kind = matches(e, SHORTCUTS.bulletList) ? BULLET : NUMBERED
        if (line && canList(line)) setPrefixes(toggleList(line, kind))
      } else if (matches(e, SHORTCUTS.indent) || matches(e, SHORTCUTS.outdent)) {
        e.preventDefault()
        const by = matches(e, SHORTCUTS.indent) ? 1 : -1
        if (line && canIndent(state, by)) setFieldPrefixes(indent(by))
      } else if (e.key === 'Tab' && !e.metaKey && !e.ctrlKey && !e.altKey && (line?.bulleted || line?.numbered) && !caretLink) {
        // Tab in a list's item (but in a link, where it goes to the link's
        // popover) indents it, and Shift+Tab outdents it; where it can't,
        // it does nothing, rather than take the focus out of the document.
        e.preventDefault()
        const by = e.shiftKey ? -1 : 1
        if (canIndent(state, by)) setFieldPrefixes(indent(by))
      } else if (matches(e, SHORTCUTS.quote)) {
        e.preventDefault()
        if (line && canQuote(line)) setPrefixes(toggleQuote(line))
      } else if (level !== undefined) {
        e.preventDefault()
        if (line && canChoose(line, level)) setPrefixes(chooseStyle(level))
      }
    }
    frameDoc.addEventListener('keydown', onKey)
    return () => frameDoc.removeEventListener('keydown', onKey)
  }, [frameDoc, root, toggleMark, openLinkForm, line, setPrefixes, setFieldPrefixes, state, caretLink])
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

  // A heading line is an `h1`-`h6`, with its block's id and classes, a
  // quote line a paragraph in a `blockquote`, the lines of a list's
  // items `li`s in one `ul`, or in one `ol` from the list's start number,
  // and a caption line a `figcaption` under its image, as `q2 render`
  // shows them. A nested list is in an `li` of the list it's in, after its
  // parent item's (whose own `li` is the parent's line, so it can't hold
  // another line's).
  const lineBlocks = useMemo(
    () => (ast && state.fields.body !== undefined ? blocksOfLines(ast.blocks as Node[], state.fields.body) : []),
    [ast, state.fields.body],
  )
  const bodyLines = useMemo(() => state.fields.body?.split('\n') ?? [], [state.fields.body])
  // For each line of an item, the lines of the items it's nested in.
  const itemAncestors = useMemo(() => {
    const ancestors: number[][] = []
    const open: number[] = []
    ;(state.fields.body ?? '').split('\n').forEach((line, i) => {
      const depth = itemDepth(line)
      // An item's at most a level deeper than the one before it.
      open.length = depth < 0 ? 0 : Math.min(depth, open.length)
      ancestors.push([...open])
      if (depth >= 0) open.push(i)
    })
    return ancestors
  }, [state.fields.body])
  const listParent = useCallback(
    (index: number): LineParent => {
      if (!isNumbered(state.fields.body?.split('\n')[index] ?? '')) return { as: 'ul', shared: true }
      const start = listStart(lineBlocks[index])
      const attrs: Record<string, string> = start === 1 ? { type: '1' } : { start: String(start), type: '1' }
      return { as: 'ol', shared: true, attrs }
    },
    [lineBlocks, state.fields.body],
  )
  const lineElement = useCallback(
    (prefix: string, index: number) => {
      const image = figureImage(prefix)
      if (image) {
        const [id, classes] = image.attr
        const caption = withoutMarks(bodyLines[index]?.slice(prefix.length) ?? '')
        return {
          as: 'figcaption',
          attrs: hasOwnAlt(image.attr) ? undefined : { 'aria-hidden': 'true' },
          // A figure with no caption is its image alone, with no line to type in.
          textless: caption === '',
          wrap: (line: ReactNode, attrs: Record<string, string>) => (
            <FigureFrame id={id} classes={[]} src={image.src} imageAttr={['', classes, image.attr[2]]} caption={caption} attrs={attrs}>
              {line}
            </FigureFrame>
          ),
        }
      }
      if (isQuote(prefix)) return { as: 'p', parent: { as: 'blockquote' } }
      if (isItem(prefix)) {
        const outer = (itemAncestors[index] ?? []).flatMap((a) => [listParent(a), NESTED])
        return { as: 'li', parents: [...outer, listParent(index)] }
      }
      const level = headingLevel(prefix)
      const { id, classes } = headingAttr(lineBlocks[index])
      return { as: `h${level || 6}`, className: classes.length > 0 ? classes.join(' ') : undefined, id }
    },
    [lineBlocks, bodyLines, itemAncestors, listParent],
  )

  const renderField = useCallback(
    (field: TitleBlockField) =>
      state.fields[field] === undefined ? null : (
        <Field name={field} text={state.fields[field]} placeholder={PLACEHOLDERS[field]} className="qe-field" />
      ),
    [state.fields],
  )

  return (
    // `data-ready` once the title block is worked out and the theme compiled;
    // `data-offline` while the connection to the sync server is down.
    <div
      className="visual-editor"
      data-ready={derived && themeCss !== null ? '' : undefined}
      data-offline={connected ? undefined : ''}
    >
      <div className="visual-box">
        <Toolbar
          state={state}
          writable={writable}
          frameDoc={frameDoc}
          setPrefixes={setPrefixes}
          setFieldPrefixes={setFieldPrefixes}
          canUndo={editor.past.length > 0}
          canRedo={editor.future.length > 0}
          undo={undo}
          redo={redo}
          marks={marks}
          toggleMark={toggleMark}
          inLink={caretLink !== null}
          onLink={openLinkForm}
          devTools={devTools}
        />
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
      </div>
      <p className="status">
        {themeCss === null ? 'Compiling theme CSS… ' : ''}
        {parseError && "Read-only: the source doesn't parse."}
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
            <Lines
              name="body"
              text={state.fields.body}
              className="qe-field"
              lineElement={lineElement}
              renderAtom={renderAtom}
              renderVoid={renderRule}
              view={MARK_VIEW}
              placeholder="Start writing…"
              caret={state.selection && isCollapsed(state.selection) && state.selection.focus.field === 'body' ? state.selection.focus.offset : undefined}
            />
          </>,
          mount,
        )}
      {root && frameDoc && (
        <LinkPopover
          doc={frameDoc}
          root={root}
          schema={SCHEMA}
          state={state}
          writable={writable}
          form={linkForm}
          setForm={setLinkForm}
          setLink={setLink}
          focusAt={focusAt}
        />
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

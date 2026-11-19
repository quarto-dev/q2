/**
 * A block of the body that the visual editor shows and can't edit yet,
 * rendered roughly as `q2 render` would: enough to read the document
 * around the paragraphs being edited. Blocks it doesn't know are shown as
 * a labelled box.
 */

import { createElement, type ReactNode } from 'react'

type Node = { t: string; c?: unknown }
export type Attr = [string, string[], [string, string][]]

export function ReadOnlyBlock({ block }: { block: Node }) {
  return renderBlock(block)
}

function attrProps([id, classes]: Attr): { id?: string; className?: string } {
  return { id: id || undefined, className: classes.length > 0 ? classes.join(' ') : undefined }
}

function renderBlocks(blocks: Node[]): ReactNode {
  return blocks.map((block, i) => <ReadOnlyBlock key={i} block={block} />)
}

function renderBlock(block: Node): ReactNode {
  const c = block.c as never
  switch (block.t) {
    case 'Para':
      return <p>{renderInlines(c)}</p>
    case 'Plain':
      return <>{renderInlines(c)}</>
    case 'Header': {
      const [level, attr, inlines] = c as [number, Attr, Node[]]
      return createElement(`h${Math.min(Math.max(level, 1), 6)}`, attrProps(attr), renderInlines(inlines))
    }
    case 'CodeBlock': {
      const [attr, text] = c as [Attr, string]
      return (
        <pre {...attrProps(attr)}>
          <code>{text}</code>
        </pre>
      )
    }
    case 'BlockQuote':
      return <blockquote>{renderBlocks(c)}</blockquote>
    case 'BulletList':
      return <ul>{(c as Node[][]).map((item, i) => <li key={i}>{renderBlocks(item)}</li>)}</ul>
    case 'OrderedList': {
      const [[start], items] = c as [[number], Node[][]]
      return <ol start={start}>{items.map((item, i) => <li key={i}>{renderBlocks(item)}</li>)}</ol>
    }
    case 'Div': {
      const [attr, blocks] = c as [Attr, Node[]]
      return <div {...attrProps(attr)}>{renderBlocks(blocks)}</div>
    }
    case 'HorizontalRule':
      return <hr />
    case 'Figure': {
      const figure = renderFigure(c)
      if (figure) return figure
      return <div className="qe-unknown">{block.t}</div>
    }
    default:
      return <div className="qe-unknown">{block.t}</div>
  }
}

/**
 * A figure of one image, as `q2 render` writes one without a `#fig-`
 * label: the image, with its caption under it. The caption is the image's
 * alt text too, unless it has its own (`fig-alt`), and is then hidden
 * from screen readers, which have read it already. A labelled figure is
 * shown the same way, without its number. Anything else in a figure
 * isn't shown (null).
 */
function renderFigure([[id, classes], [, caption], content]: [Attr, [unknown, Node[]], Node[]]): ReactNode {
  const image = content.length === 1 && content[0].t === 'Plain' ? (content[0].c as Node[]) : []
  if (image.length !== 1 || image[0].t !== 'Image') return null
  const [attr, alt, [src]] = image[0].c as [Attr, Node[], [string, string]]
  return (
    <FigureFrame id={id} classes={classes} src={src} imageAttr={attr} caption={plainText(alt)}>
      <figcaption aria-hidden={hasOwnAlt(attr) ? undefined : 'true'}>{renderBlocks(caption)}</figcaption>
    </FigureFrame>
  )
}

/** Whether an image has alt text of its own (`fig-alt`), rather than its caption. */
export function hasOwnAlt([, , keyvals]: Attr): boolean {
  return keyvals.some(([key]) => key === 'fig-alt')
}

/**
 * A figure, as `q2 render` writes one, around its caption (`children`):
 * the figure's id and classes, and its image, whose alt text is its own
 * (`fig-alt`) or else the caption's plain text.
 */
export function FigureFrame({
  id,
  classes,
  src,
  imageAttr: [, imageClasses, keyvals],
  caption,
  attrs,
  children,
}: {
  id: string
  classes: string[]
  src: string
  imageAttr: Attr
  caption: string
  /** More attributes for its outermost element (the editor's). */
  attrs?: Record<string, string>
  children: ReactNode
}) {
  const figAlt = keyvals.find(([key]) => key === 'fig-alt')?.[1]
  // The rest are the image's attributes (`width=50%`). Not `style`, which
  // React takes only as an object.
  const attributes = Object.fromEntries(keyvals.filter(([key]) => key !== 'fig-alt' && key !== 'style'))
  return (
    <div
      {...attrs}
      id={id || undefined}
      className={['quarto-figure', 'quarto-figure-center', ...classes, ...(id ? ['anchored'] : [])].join(' ')}
    >
      <figure>
        <img src={src} alt={figAlt ?? caption} className={[...imageClasses, 'img-fluid'].join(' ')} {...attributes} />
        {children}
      </figure>
    </div>
  )
}

/** Inlines as plain text, as an image's alt text has them. */
function plainText(inlines: Node[]): string {
  return inlines
    .map((inline) => {
      switch (inline.t) {
        case 'Str':
          return inline.c as string
        case 'Space':
        case 'SoftBreak':
        case 'LineBreak':
          return ' '
        case 'Code':
        case 'Math':
          return (inline.c as [unknown, string])[1]
        case 'Link':
        case 'Image':
        case 'Span':
        case 'Quoted':
        case 'Cite':
          return plainText((inline.c as [unknown, Node[]])[1])
        default:
          // Formatting (Emph, Strong, …) holds its inlines; a note is left out.
          return inline.t in WRAPPERS || inline.t === 'SmallCaps' ? plainText(inline.c as Node[]) : ''
      }
    })
    .join('')
}

function renderInlines(inlines: Node[]): ReactNode {
  return inlines.map((inline, i) => <Inline key={i} inline={inline} />)
}

const WRAPPERS: Record<string, string> = {
  Emph: 'em',
  Strong: 'strong',
  Underline: 'u',
  Strikeout: 's',
  Superscript: 'sup',
  Subscript: 'sub',
}

function Inline({ inline }: { inline: Node }): ReactNode {
  const c = inline.c as never
  if (inline.t in WRAPPERS) return createElement(WRAPPERS[inline.t], null, renderInlines(c))
  switch (inline.t) {
    case 'Str':
      return c
    case 'Space':
    case 'SoftBreak':
      return ' '
    case 'LineBreak':
      return <br />
    case 'SmallCaps':
      return <span className="smallcaps">{renderInlines(c)}</span>
    case 'Code':
      return <code>{(c as [Attr, string])[1]}</code>
    case 'Math':
      return <span className="math">{(c as [unknown, string])[1]}</span>
    case 'Quoted': {
      const [kind, inner] = c as [{ t: string }, Node[]]
      const [open, close] = kind.t === 'SingleQuote' ? ['‘', '’'] : ['“', '”']
      return (
        <>
          {open}
          {renderInlines(inner)}
          {close}
        </>
      )
    }
    case 'Link': {
      const [attr, inner, [href]] = c as [Attr, Node[], [string, string]]
      return (
        <a {...attrProps(attr)} href={href}>
          {renderInlines(inner)}
        </a>
      )
    }
    case 'Image': {
      const [attr, inner, [src]] = c as [Attr, Node[], [string, string]]
      return <img {...attrProps(attr)} src={src} alt={inner.map((n) => (typeof n.c === 'string' ? n.c : ' ')).join('')} />
    }
    case 'Span': {
      const [attr, inner] = c as [Attr, Node[]]
      return <span {...attrProps(attr)}>{renderInlines(inner)}</span>
    }
    case 'Cite':
      return <>{renderInlines((c as [unknown, Node[]])[1])}</>
    default:
      return null
  }
}

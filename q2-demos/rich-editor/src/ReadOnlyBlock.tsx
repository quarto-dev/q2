/**
 * A block of the body that the visual editor shows and can't edit yet,
 * rendered roughly as `q2 render` would: enough to read the document
 * around the paragraphs being edited. Blocks it doesn't know are shown as
 * a labelled box.
 */

import { createElement, type ReactNode } from 'react'

type Node = { t: string; c?: unknown }
type Attr = [string, string[], [string, string][]]

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
      return <blockquote className="blockquote">{renderBlocks(c)}</blockquote>
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
    default:
      return <div className="qe-unknown">{block.t}</div>
  }
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

/**
 * Simple React-based Pandoc AST renderer
 *
 * This is a lightweight alternative to the full Rust HTML writer that renders
 * Pandoc AST directly to React elements. It handles common block and inline types
 * but is not as feature-complete as the Rust implementation.
 */

import type { ReactNode } from 'react';

// ============================================================================
// Type Definitions (subset of Pandoc AST)
// ============================================================================

interface PandocAst {
  'pandoc-api-version': number[];
  meta: Record<string, any>;
  blocks: Block[];
}

type Block =
  | { t: 'Para'; c: Inline[] }
  | { t: 'Plain'; c: Inline[] }
  | { t: 'Header'; c: [number, Attr, Inline[]] }
  | { t: 'CodeBlock'; c: [Attr, string] }
  | { t: 'BulletList'; c: Block[][] }
  | { t: 'OrderedList'; c: [ListAttributes, Block[][]] }
  | { t: 'BlockQuote'; c: Block[] }
  | { t: 'Div'; c: [Attr, Block[]] }
  | { t: 'HorizontalRule' }
  | { t: 'RawBlock'; c: [string, string] };

type Inline =
  | { t: 'Str'; c: string }
  | { t: 'Space' }
  | { t: 'SoftBreak' }
  | { t: 'LineBreak' }
  | { t: 'Emph'; c: Inline[] }
  | { t: 'Strong'; c: Inline[] }
  | { t: 'Underline'; c: Inline[] }
  | { t: 'Strikeout'; c: Inline[] }
  | { t: 'Superscript'; c: Inline[] }
  | { t: 'Subscript'; c: Inline[] }
  | { t: 'Code'; c: [Attr, string] }
  | { t: 'Link'; c: [Attr, Inline[], Target] }
  | { t: 'Image'; c: [Attr, Inline[], Target] }
  | { t: 'Span'; c: [Attr, Inline[]] }
  | { t: 'RawInline'; c: [string, string] };

type Attr = [string, string[], Array<[string, string]>];
type Target = [string, string]; // [url, title]
type ListAttributes = [number, { t: string; c?: any }, { t: string; c?: any }];

// ============================================================================
// Inline Rendering
// ============================================================================

function renderInline(inline: Inline, key: number): ReactNode {
  switch (inline.t) {
    case 'Str':
      return inline.c;
    case 'Space':
      return ' ';
    case 'SoftBreak':
      return '\n';
    case 'LineBreak':
      return <br key={key} />;

    case 'Emph':
      return <em key={key}>{renderInlines(inline.c)}</em>;
    case 'Strong':
      return <strong key={key}>{renderInlines(inline.c)}</strong>;
    case 'Underline':
      return <u key={key}>{renderInlines(inline.c)}</u>;
    case 'Strikeout':
      return <del key={key}>{renderInlines(inline.c)}</del>;
    case 'Superscript':
      return <sup key={key}>{renderInlines(inline.c)}</sup>;
    case 'Subscript':
      return <sub key={key}>{renderInlines(inline.c)}</sub>;

    case 'Code': {
      const [attr, text] = inline.c;
      const props = attrToProps(attr);
      return (
        <code key={key} {...props}>
          {text}
        </code>
      );
    }

    case 'Link': {
      const [attr, content, target] = inline.c;
      const [url, title] = target;
      const props = attrToProps(attr);
      return (
        <a key={key} href={url} title={title || undefined} {...props}>
          {renderInlines(content)}
        </a>
      );
    }

    case 'Image': {
      const [attr, content, target] = inline.c;
      const [url, title] = target;
      const props = attrToProps(attr);
      const alt = extractPlainText(content);
      return <img key={key} src={url} alt={alt} title={title || undefined} {...props} />;
    }

    case 'Span': {
      const [attr, content] = inline.c;
      const props = attrToProps(attr);
      return (
        <span key={key} {...props}>
          {renderInlines(content)}
        </span>
      );
    }

    case 'RawInline': {
      const [format, text] = inline.c;
      if (format === 'html') {
        return <span key={key} dangerouslySetInnerHTML={{ __html: text }} />;
      }
      return null;
    }

    default:
      return null;
  }
}

function renderInlines(inlines: Inline[]): ReactNode[] {
  return inlines.map((inline, i) => renderInline(inline, i));
}

// ============================================================================
// Block Rendering
// ============================================================================

function renderBlock(block: Block, key: number): ReactNode {
  switch (block.t) {
    case 'Plain':
      return <div key={key}>{renderInlines(block.c)}</div>;

    case 'Para':
      return <p key={key}>{renderInlines(block.c)}</p>;

    case 'Header': {
      const [level, attr, content] = block.c;
      const props = attrToProps(attr);
      const Tag = `h${level}` as 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6';
      return (
        <Tag key={key} {...props}>
          {renderInlines(content)}
        </Tag>
      );
    }

    case 'CodeBlock': {
      const [attr, text] = block.c;
      const props = attrToProps(attr);
      return (
        <pre key={key} {...props}>
          <code>{text}</code>
        </pre>
      );
    }

    case 'BulletList':
      return (
        <ul key={key}>
          {block.c.map((item, i) => (
            <li key={i}>{renderBlocks(item)}</li>
          ))}
        </ul>
      );

    case 'OrderedList': {
      const [_listAttrs, items] = block.c;
      return (
        <ol key={key}>
          {items.map((item, i) => (
            <li key={i}>{renderBlocks(item)}</li>
          ))}
        </ol>
      );
    }

    case 'BlockQuote':
      return <blockquote key={key}>{renderBlocks(block.c)}</blockquote>;

    case 'Div': {
      const [attr, content] = block.c;
      const props = attrToProps(attr);
      return (
        <div key={key} {...props}>
          {renderBlocks(content)}
        </div>
      );
    }

    case 'HorizontalRule':
      return <hr key={key} />;

    case 'RawBlock': {
      const [format, text] = block.c;
      if (format === 'html') {
        return <div key={key} dangerouslySetInnerHTML={{ __html: text }} />;
      }
      return null;
    }

    default:
      console.warn('Unsupported block type:', (block as any).t);
      return null;
  }
}

function renderBlocks(blocks: Block[]): ReactNode[] {
  return blocks.map((block, i) => renderBlock(block, i));
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Convert Pandoc Attr to React props
 */
function attrToProps(attr: Attr): Record<string, any> {
  const [id, classes, keyvals] = attr;
  const props: Record<string, any> = {};

  if (id) {
    props.id = id;
  }

  if (classes.length > 0) {
    props.className = classes.join(' ');
  }

  for (const [key, value] of keyvals) {
    // Convert attribute names to React-friendly format
    if (key === 'style') {
      // Parse inline styles if needed
      props.style = value;
    } else {
      // Add data- prefix for custom attributes
      props[`data-${key}`] = value;
    }
  }

  return props;
}

/**
 * Extract plain text from inline elements (for alt text, etc.)
 */
function extractPlainText(inlines: Inline[]): string {
  return inlines
    .map((inline) => {
      if (inline.t === 'Str') return inline.c;
      if (inline.t === 'Space') return ' ';
      if ('c' in inline && Array.isArray(inline.c)) {
        const nested = inline.c.find((item: any) => Array.isArray(item));
        if (nested) return extractPlainText(nested);
      }
      return '';
    })
    .join('');
}

// ============================================================================
// Main Component
// ============================================================================

interface ReactAstRendererProps {
  ast: PandocAst | string;
  className?: string;
}

/**
 * Render a Pandoc AST to React elements
 *
 * @param ast - Pandoc AST object or JSON string
 * @param className - Optional CSS class for the container
 */
export function ReactAstRenderer({ ast, className }: ReactAstRendererProps) {
  if (typeof ast === 'string' && ast.trim().length === 0) return <div></div>
  const parsedAst = typeof ast === 'string' ? JSON.parse(ast) : ast;

  return <div className={className}>{renderBlocks(parsedAst.blocks)}</div>;
}

export default ReactAstRenderer;

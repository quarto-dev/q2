import React from 'react';

/**
 * Simplified Pandoc AST types for rendering
 */
interface PandocAST {
  'pandoc-api-version': [number, number, number];
  meta: Record<string, unknown>;
  blocks: Block[];
}

type ParaBlock = { t: 'Para'; c: Inline[] };
type PlainBlock = { t: 'Plain'; c: Inline[] };
type HeaderBlock = { t: 'Header'; c: [number, [string, string[], [string, string][]], Inline[]] };
type CodeBlock = { t: 'CodeBlock'; c: [[string, string[], [string, string][]], string] };
type BulletListBlock = { t: 'BulletList'; c: Block[][] };
type OrderedListBlock = { t: 'OrderedList'; c: [[number, { t: string }, { t: string }], Block[][]] };
type BlockQuoteBlock = { t: 'BlockQuote'; c: Block[] };
type DivBlock = { t: 'Div'; c: [[string, string[], [string, string][]], Block[]] };
type HorizontalRuleBlock = { t: 'HorizontalRule' };
type RawBlock = { t: 'RawBlock'; c: [string, string] };
type FigureBlock = { t: 'Figure'; c: [[string, string[], [string, string][]], [Inline[] | null, Block[]], Block[]] };
type UnknownBlock = { t: string; c?: unknown };

type Block =
  | ParaBlock
  | PlainBlock
  | HeaderBlock
  | CodeBlock
  | BulletListBlock
  | OrderedListBlock
  | BlockQuoteBlock
  | DivBlock
  | HorizontalRuleBlock
  | RawBlock
  | FigureBlock
  | UnknownBlock;

type StrInline = { t: 'Str'; c: string };
type SpaceInline = { t: 'Space' };
type SoftBreakInline = { t: 'SoftBreak' };
type LineBreakInline = { t: 'LineBreak' };
type EmphInline = { t: 'Emph'; c: Inline[] };
type StrongInline = { t: 'Strong'; c: Inline[] };
type CodeInline = { t: 'Code'; c: [[string, string[], [string, string][]], string] };
type LinkInline = { t: 'Link'; c: [[string, string[], [string, string][]], Inline[], [string, string]] };
type ImageInline = { t: 'Image'; c: [[string, string[], [string, string][]], Inline[], [string, string]] };
type SpanInline = { t: 'Span'; c: [[string, string[], [string, string][]], Inline[]] };
type UnknownInline = { t: string; c?: unknown };

type Inline =
  | StrInline
  | SpaceInline
  | SoftBreakInline
  | LineBreakInline
  | EmphInline
  | StrongInline
  | CodeInline
  | LinkInline
  | ImageInline
  | SpanInline
  | UnknownInline;

interface PandocAstRendererProps {
  astJson: string;
  onNavigateToDocument?: (path: string, anchor: string | null) => void;
}

/**
 * Component that renders Pandoc AST as React elements
 */
export function Ast({ astJson, onNavigateToDocument }: PandocAstRendererProps) {
  let ast: PandocAST;

  try {
    ast = JSON.parse(astJson);
  } catch (err) {
    return (
      <div className="error" style={{ padding: '20px', color: 'red' }}>
        Failed to parse AST: {err instanceof Error ? err.message : String(err)}
      </div>
    );
  }

  return (
    <div className="pandoc-content" style={{ padding: '20px', maxWidth: '800px', margin: '0 auto' }}>
      {ast.blocks.map((block, i) => renderBlock(block, i, onNavigateToDocument))}
    </div>
  );
}

// ============================================================================
// Block Rendering
// ============================================================================

function renderBlock(
  block: Block,
  key: number,
  onNavigateToDocument?: (path: string, anchor: string | null) => void
): React.ReactNode {
  switch (block.t) {
    case 'Para': {
      const paraBlock = block as ParaBlock;
      return <p key={key}>{renderInlines(paraBlock.c, onNavigateToDocument)}</p>;
    }

    case 'Plain': {
      const plainBlock = block as PlainBlock;
      return <div key={key}>{renderInlines(plainBlock.c, onNavigateToDocument)}</div>;
    }

    case 'Header': {
      const headerBlock = block as HeaderBlock;
      const [level, [id, classes, attrs], inlines] = headerBlock.c;
      const Tag = `h${level}` as 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6';
      const className = classes.join(' ');
      const attrObj = Object.fromEntries(attrs);
      return (
        <Tag key={key} id={id} className={className} {...attrObj}>
          {renderInlines(inlines, onNavigateToDocument)}
        </Tag>
      );
    }

    case 'CodeBlock': {
      const codeBlock = block as CodeBlock;
      const [[id, classes, attrs], code] = codeBlock.c;
      const className = classes.join(' ');
      const attrObj = Object.fromEntries(attrs);
      return (
        <pre key={key} id={id} className={className} {...attrObj}>
          <code>{code}</code>
        </pre>
      );
    }

    case 'BulletList': {
      const bulletList = block as BulletListBlock;
      return (
        <ul key={key}>
          {bulletList.c.map((item, i) => (
            <li key={i}>
              {item.map((b, j) => renderBlock(b, j, onNavigateToDocument))}
            </li>
          ))}
        </ul>
      );
    }

    case 'OrderedList': {
      const orderedList = block as OrderedListBlock;
      const [[start, _style, _delim], items] = orderedList.c;
      return (
        <ol key={key} start={start}>
          {items.map((item, i) => (
            <li key={i}>
              {item.map((b, j) => renderBlock(b, j, onNavigateToDocument))}
            </li>
          ))}
        </ol>
      );
    }

    case 'BlockQuote': {
      const blockQuote = block as BlockQuoteBlock;
      return (
        <blockquote key={key}>
          {blockQuote.c.map((b, i) => renderBlock(b, i, onNavigateToDocument))}
        </blockquote>
      );
    }

    case 'Div': {
      const divBlock = block as DivBlock;
      const [[id, classes, attrs], blocks] = divBlock.c;
      const className = classes.join(' ');
      const attrObj = Object.fromEntries(attrs);
      return (
        <div key={key} id={id} className={className} {...attrObj}>
          {blocks.map((b, i) => renderBlock(b, i, onNavigateToDocument))}
        </div>
      );
    }

    case 'HorizontalRule':
      return <hr key={key} />;

    case 'RawBlock': {
      const rawBlock = block as RawBlock;
      const [format, content] = rawBlock.c;
      if (format === 'html') {
        return <div key={key} dangerouslySetInnerHTML={{ __html: content }} />;
      }
      return null;
    }

    case 'Figure': {
      const figureBlock = block as FigureBlock;
      const [[id, classes, attrs], [caption, _blocks], content] = figureBlock.c;
      const className = classes.join(' ');
      const attrObj = Object.fromEntries(attrs);

      return (
        <figure key={key} id={id} className={className} {...attrObj}>
          {content.map((b, i) => renderBlock(b, i, onNavigateToDocument))}
          {caption && caption.length > 0 && (
            <figcaption>{renderInlines(caption, onNavigateToDocument)}</figcaption>
          )}
        </figure>
      );
    }

    default:
      console.warn('Unhandled block type:', block.t);
      return <div key={key} style={{ color: 'gray', fontSize: '0.9em' }}>[{block.t}]</div>;
  }
}

// ============================================================================
// Inline Rendering
// ============================================================================

function renderInlines(
  inlines: Inline[],
  onNavigateToDocument?: (path: string, anchor: string | null) => void
): React.ReactNode[] {
  return inlines.map((inline, i) => renderInline(inline, i, onNavigateToDocument));
}

function renderInline(
  inline: Inline,
  key: number,
  onNavigateToDocument?: (path: string, anchor: string | null) => void
): React.ReactNode {
  switch (inline.t) {
    case 'Str': {
      const strInline = inline as StrInline;
      return strInline.c;
    }

    case 'Space':
      return ' ';

    case 'SoftBreak':
      return ' ';

    case 'LineBreak':
      return <br key={key} />;

    case 'Emph': {
      const emphInline = inline as EmphInline;
      return <em key={key}>{renderInlines(emphInline.c, onNavigateToDocument)}</em>;
    }

    case 'Strong': {
      const strongInline = inline as StrongInline;
      return <strong key={key}>{renderInlines(strongInline.c, onNavigateToDocument)}</strong>;
    }

    case 'Code': {
      const codeInline = inline as CodeInline;
      const [[id, classes, attrs], code] = codeInline.c;
      const className = classes.join(' ');
      const attrObj = Object.fromEntries(attrs);
      return (
        <code key={key} id={id} className={className} {...attrObj}>
          {code}
        </code>
      );
    }

    case 'Link': {
      const linkInline = inline as LinkInline;
      const [[id, classes, attrs], inlines, [url, title]] = linkInline.c;
      const className = classes.join(' ');
      const attrObj = Object.fromEntries(attrs);

      // Handle .qmd links
      if (url.endsWith('.qmd') && onNavigateToDocument) {
        const [path, anchor] = url.split('#');
        return (
          <a
            key={key}
            id={id}
            className={className}
            {...attrObj}
            href={url}
            title={title}
            onClick={(e) => {
              e.preventDefault();
              onNavigateToDocument(path, anchor || null);
            }}
          >
            {renderInlines(inlines, onNavigateToDocument)}
          </a>
        );
      }

      return (
        <a key={key} id={id} className={className} {...attrObj} href={url} title={title}>
          {renderInlines(inlines, onNavigateToDocument)}
        </a>
      );
    }

    case 'Image': {
      const imageInline = inline as ImageInline;
      const [[id, classes, attrs], inlines, [url, title]] = imageInline.c;
      const className = classes.join(' ');
      const attrObj = Object.fromEntries(attrs);
      const alt = inlines.map(i => {
        if ('c' in i && typeof i.c === 'string') return i.c;
        return '';
      }).join('');

      return (
        <img key={key} id={id} className={className} {...attrObj} src={url} alt={alt} title={title} />
      );
    }

    case 'Span': {
      const spanInline = inline as SpanInline;
      const [[id, classes, attrs], inlines] = spanInline.c;
      const className = classes.join(' ');
      const attrObj = Object.fromEntries(attrs);
      return (
        <span key={key} id={id} className={className} {...attrObj}>
          {renderInlines(inlines, onNavigateToDocument)}
        </span>
      );
    }

    default:
      console.warn('Unhandled inline type:', inline.t);
      return <span key={key} style={{ color: 'gray', fontSize: '0.9em' }}>[{inline.t}]</span>;
  }
}

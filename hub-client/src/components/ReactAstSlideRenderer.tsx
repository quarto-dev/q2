import React, { useState, useEffect } from 'react';
import { AspectRatioScaler } from './AspectRatioScaler';
import katex from 'katex';
import 'katex/dist/katex.min.css';
import { vfsReadFile, vfsReadBinaryFile } from '../services/wasmRenderer';

/**
 * Simplified Pandoc AST types for rendering
 * Exported for use in thumbnail generation.
 */
export interface PandocAST {
  'pandoc-api-version': [number, number, number];
  meta: Record<string, unknown>;
  blocks: Block[];
}

/**
 * Represents a single slide with its content
 * Exported for use in thumbnail generation.
 */
export interface Slide {
  type: 'title' | 'content';
  title?: string;
  author?: string;
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
type MathInline = { t: 'Math'; c: [{ t: string }, string] };
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
  | MathInline
  | UnknownInline;

interface PandocAstSlideRendererProps {
  astJson: string;
  /** Current file path for resolving relative image paths */
  currentFilePath: string;
  onNavigateToDocument?: (path: string, anchor: string | null) => void;
  /** Optional controlled current slide index. If provided, component uses this instead of internal state. */
  currentSlide?: number;
  /** Callback when current slide changes (for controlled mode). */
  onSlideChange?: (slideIndex: number) => void;
}

/**
 * Component that renders Pandoc AST as React elements for slides
 */
export function SlideAst({ astJson, currentFilePath, onNavigateToDocument, currentSlide: controlledSlide, onSlideChange }: PandocAstSlideRendererProps) {
  const [internalSlide, setInternalSlide] = useState(0);

  // Use controlled slide if provided, otherwise use internal state
  const currentSlide = controlledSlide !== undefined ? controlledSlide : internalSlide;
  const setCurrentSlide = (value: number | ((prev: number) => number)) => {
    const newValue = typeof value === 'function' ? value(currentSlide) : value;
    if (controlledSlide !== undefined) {
      // Controlled mode - notify parent
      onSlideChange?.(newValue);
    } else {
      // Uncontrolled mode - update internal state
      setInternalSlide(newValue);
    }
  };

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

  // Parse blocks into slides
  const slides = parseSlides(ast);

  // Clamp slide index if it's out of bounds (e.g., slides were removed during editing)
  useEffect(() => {
    if (currentSlide >= slides.length && slides.length > 0) {
      setCurrentSlide(slides.length - 1);
    }
  }, [slides.length, currentSlide]);

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'ArrowLeft') {
        setCurrentSlide(prev => Math.max(0, prev - 1));
      } else if (e.key === 'ArrowRight') {
        setCurrentSlide(prev => Math.min(slides.length - 1, prev + 1));
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [slides.length, setCurrentSlide]);

  const goToPrevSlide = () => setCurrentSlide(prev => Math.max(0, prev - 1));
  const goToNextSlide = () => setCurrentSlide(prev => Math.min(slides.length - 1, prev + 1));

  return (
    <div
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        background: '#191919'
      }}
    >
      {/* AspectRatioScaler handles sizing and scaling */}
      <AspectRatioScaler width={1050} height={700} backgroundColor="#191919">
        <div
          style={{
            width: '1050px',
            height: '700px',
            background: '#fff',
            boxShadow: '0 0 30px rgba(0,0,0,0.5)',
            position: 'relative',
            overflow: 'hidden'
          }}
        >
          {renderSlide(slides[currentSlide], currentFilePath, onNavigateToDocument)}
        </div>
      </AspectRatioScaler>

      {/* Navigation buttons */}
      <div style={{
        position: 'absolute',
        bottom: '20px',
        right: '20px',
        display: 'flex',
        gap: '10px',
        zIndex: 100
      }}>
        <button
          onClick={goToPrevSlide}
          disabled={currentSlide === 0}
          style={{
            padding: '10px 15px',
            fontSize: '18px',
            background: '#2a76dd',
            color: '#fff',
            border: 'none',
            borderRadius: '5px',
            cursor: currentSlide === 0 ? 'not-allowed' : 'pointer',
            opacity: currentSlide === 0 ? 0.5 : 1
          }}
        >
          ←
        </button>
        <button
          onClick={goToNextSlide}
          disabled={currentSlide === slides.length - 1}
          style={{
            padding: '10px 15px',
            fontSize: '18px',
            background: '#2a76dd',
            color: '#fff',
            border: 'none',
            borderRadius: '5px',
            cursor: currentSlide === slides.length - 1 ? 'not-allowed' : 'pointer',
            opacity: currentSlide === slides.length - 1 ? 0.5 : 1
          }}
        >
          →
        </button>
      </div>

      {/* Slide counter */}
      <div style={{
        position: 'absolute',
        bottom: '20px',
        left: '20px',
        color: '#fff',
        fontSize: '14px',
        fontFamily: 'sans-serif'
      }}>
        {currentSlide + 1} / {slides.length}
      </div>
    </div>
  );
}

// ============================================================================
// Slide Parsing (exported for thumbnail generation)
// ============================================================================

/**
 * Parse AST blocks into slides.
 * Strategy:
 * 1. Check if blocks are section Divs (Pandoc wraps sections in Divs)
 * 2. If so, each section Div becomes a slide
 * 3. Otherwise, split on h1/h2 headers
 *
 * Exported for use in thumbnail generation.
 */
export function parseSlides(ast: PandocAST): Slide[] {
  const slides: Slide[] = [];

  // Extract title and author from metadata
  const title = extractMetaString(ast.meta.title);
  const author = extractMetaString(ast.meta.author);

  // Add title slide if we have title or author
  if (title || author) {
    slides.push({
      type: 'title',
      title,
      author,
      blocks: []
    });
  }

  // Check if blocks are section Divs (look for Divs with headers as first block)
  const sections = extractSections(ast.blocks);

  if (sections.length > 0) {
    // Use section-based splitting
    for (const section of sections) {
      slides.push({
        type: 'content',
        blocks: section
      });
    }
  } else {
    // Fall back to header-based splitting
    const flattenedBlocks = flattenBlocks(ast.blocks);
    const contentSlides = splitByHeaders(flattenedBlocks);
    slides.push(...contentSlides);
  }

  return slides;
}

/**
 * Extract sections from blocks. Each section Div becomes a slide.
 * Returns empty array if blocks don't follow section pattern.
 */
function extractSections(blocks: Block[]): Block[][] {
  const sections: Block[][] = [];

  for (const block of blocks) {
    if (block.t === 'Div') {
      const divBlock = block as DivBlock;
      const [[, classes], innerBlocks] = divBlock.c;

      // Check if this Div looks like a section
      // (has "section" class OR first block is a header)
      const isSection = classes.includes('section') ||
                       (innerBlocks.length > 0 && innerBlocks[0].t === 'Header');

      if (isSection) {
        sections.push(innerBlocks);
      }
    }
  }

  // Only return sections if ALL top-level blocks are section Divs
  // Otherwise return empty to trigger header-based splitting
  return sections.length === blocks.length ? sections : [];
}

/**
 * Split blocks into slides based on h1/h2 headers
 */
function splitByHeaders(blocks: Block[]): Slide[] {
  const slides: Slide[] = [];
  let currentSlideBlocks: Block[] = [];

  for (const block of blocks) {
    if (block.t === 'Header') {
      const headerBlock = block as HeaderBlock;
      const [level] = headerBlock.c;

      if (level === 1 || level === 2) {
        // Save previous slide if it has content
        if (currentSlideBlocks.length > 0) {
          slides.push({
            type: 'content',
            blocks: currentSlideBlocks
          });
        }

        // Start new slide with this heading
        currentSlideBlocks = [block];
      } else {
        // h3, h4, etc. - add to current slide
        currentSlideBlocks.push(block);
      }
    } else {
      // Non-heading block - add to current slide
      currentSlideBlocks.push(block);
    }
  }

  // Add final slide if it has content
  if (currentSlideBlocks.length > 0) {
    slides.push({
      type: 'content',
      blocks: currentSlideBlocks
    });
  }

  return slides;
}

/**
 * Flatten block structure by extracting blocks from Divs
 * This handles the case where sections are wrapped in Div containers
 */
function flattenBlocks(blocks: Block[]): Block[] {
  const result: Block[] = [];

  for (const block of blocks) {
    if (block.t === 'Div') {
      const divBlock = block as DivBlock;
      const [, innerBlocks] = divBlock.c;
      // Recursively flatten inner blocks
      result.push(...flattenBlocks(innerBlocks));
    } else {
      result.push(block);
    }
  }

  return result;
}

/**
 * Extract a string value from Pandoc metadata
 */
function extractMetaString(meta: unknown): string | undefined {
  if (!meta) return undefined;

  // Handle MetaInlines (most common for title/author)
  if (typeof meta === 'object' && meta !== null && 't' in meta) {
    const metaObj = meta as { t: string; c?: unknown };
    if (metaObj.t === 'MetaInlines' && Array.isArray(metaObj.c)) {
      return metaObj.c
        .map((inline: any) => {
          if (inline.t === 'Str') return inline.c;
          if (inline.t === 'Space') return ' ';
          return '';
        })
        .join('');
    }
    if (metaObj.t === 'MetaString' && typeof metaObj.c === 'string') {
      return metaObj.c;
    }
  }

  return undefined;
}

/**
 * Render a single slide
 * Exported for use in thumbnail generation.
 */
export function renderSlide(
  slide: Slide,
  currentFilePath: string,
  onNavigateToDocument?: (path: string, anchor: string | null) => void
): React.ReactNode {
  if (slide.type === 'title') {
    return (
      <div style={{
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'center',
        alignItems: 'center',
        width: '100%',
        height: '100%',
        padding: '80px',
        textAlign: 'center',
        boxSizing: 'border-box'
      }}>
        {slide.title && (
          <h1 style={{
            fontSize: '72px',
            margin: '0 0 40px 0',
            color: '#1a1a1a',
            fontWeight: 'bold'
          }}>
            {slide.title}
          </h1>
        )}
        {slide.author && (
          <p style={{
            fontSize: '36px',
            margin: 0,
            color: '#666'
          }}>
            {slide.author}
          </p>
        )}
      </div>
    );
  }

  // Content slide
  return (
    <div style={{
      width: '100%',
      height: '100%',
      padding: '80px',
      overflow: 'auto',
      fontSize: '28px',
      boxSizing: 'border-box'
    }}>
      {slide.blocks.map((block, i) => renderBlock(block, i, currentFilePath, onNavigateToDocument))}
    </div>
  );
}

// ============================================================================
// Attribute Helpers
// ============================================================================

/**
 * Convert Pandoc attributes to React-compatible props.
 * Handles the special case where 'style' might be a string and needs to be parsed.
 */
function attributesToProps(
  id: string,
  classes: string[],
  attrs: [string, string][],
  additionalStyle?: React.CSSProperties
): {
  id?: string;
  className?: string;
  style?: React.CSSProperties;
  [key: string]: any;
} {
  const className = classes.join(' ');
  const attrObj: { [key: string]: any } = {};
  let styleString: string | undefined;

  // Separate style from other attributes
  for (const [key, value] of attrs) {
    if (key === 'style') {
      styleString = value;
    } else {
      attrObj[key] = value;
    }
  }

  // Parse style string into style object
  let parsedStyle: React.CSSProperties = {};
  if (styleString) {
    parsedStyle = parseStyleString(styleString);
  }

  // Merge with additional styles
  const style = { ...parsedStyle, ...additionalStyle };

  return {
    ...(id ? { id } : {}),
    ...(className ? { className } : {}),
    ...(Object.keys(style).length > 0 ? { style } : {}),
    ...attrObj
  };
}

/**
 * Parse a CSS style string (e.g., "color: red; font-size: 14px") into a React style object.
 */
function parseStyleString(styleString: string): React.CSSProperties {
  const style: React.CSSProperties = {};

  // Split by semicolon and process each declaration
  const declarations = styleString.split(';').map(s => s.trim()).filter(Boolean);

  for (const declaration of declarations) {
    const colonIndex = declaration.indexOf(':');
    if (colonIndex === -1) continue;

    const property = declaration.slice(0, colonIndex).trim();
    const value = declaration.slice(colonIndex + 1).trim();

    // Convert CSS property names to camelCase (e.g., "font-size" -> "fontSize")
    const camelCaseProperty = property.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());

    // @ts-ignore - Dynamic property assignment
    style[camelCaseProperty] = value;
  }

  return style;
}

// ============================================================================
// Block Rendering
// ============================================================================

function renderBlock(
  block: Block,
  key: number,
  currentFilePath: string,
  onNavigateToDocument?: (path: string, anchor: string | null) => void
): React.ReactNode {
  switch (block.t) {
    case 'Para': {
      const paraBlock = block as ParaBlock;
      return (
        <p key={key} style={{ marginTop: '0.5em', marginBottom: '0.5em', lineHeight: '1.4' }}>
          {renderInlines(paraBlock.c, currentFilePath, onNavigateToDocument)}
        </p>
      );
    }

    case 'Plain': {
      const plainBlock = block as PlainBlock;
      return (
        <div key={key} style={{ marginTop: '0.5em', marginBottom: '0.5em', lineHeight: '1.4' }}>
          {renderInlines(plainBlock.c, currentFilePath, onNavigateToDocument)}
        </div>
      );
    }

    case 'Header': {
      const headerBlock = block as HeaderBlock;
      const [level, [id, classes, attrs], inlines] = headerBlock.c;
      const Tag = `h${level}` as 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6';

      // Slide-appropriate header styles
      const headerStyles: React.CSSProperties = {
        marginTop: level <= 2 ? '0' : '0.5em',
        marginBottom: '0.5em',
        color: '#1a1a1a',
        fontWeight: 'bold'
      };

      if (level === 1) {
        headerStyles.fontSize = '64px';
      } else if (level === 2) {
        headerStyles.fontSize = '52px';
      } else if (level === 3) {
        headerStyles.fontSize = '40px';
      }

      const props = attributesToProps(id, classes, attrs, headerStyles);

      return (
        <Tag key={key} {...props}>
          {renderInlines(inlines, currentFilePath, onNavigateToDocument)}
        </Tag>
      );
    }

    case 'CodeBlock': {
      const codeBlock = block as CodeBlock;
      const [[id, classes, attrs], code] = codeBlock.c;
      const codeBlockStyle: React.CSSProperties = {
        background: '#f5f5f5',
        padding: '20px',
        borderRadius: '8px',
        overflow: 'auto',
        fontSize: '20px',
        marginTop: '0.5em',
        marginBottom: '0.5em'
      };
      const props = attributesToProps(id, classes, attrs, codeBlockStyle);
      return (
        <pre key={key} {...props}>
          <code>{code}</code>
        </pre>
      );
    }

    case 'BulletList': {
      const bulletList = block as BulletListBlock;
      return (
        <ul key={key} style={{ marginTop: '0.5em', marginBottom: '0.5em', lineHeight: '1.6' }}>
          {bulletList.c.map((item, i) => (
            <li key={i} style={{ marginTop: '0.3em', marginBottom: '0.3em' }}>
              {item.map((b, j) => renderBlock(b, j, currentFilePath, onNavigateToDocument))}
            </li>
          ))}
        </ul>
      );
    }

    case 'OrderedList': {
      const orderedList = block as OrderedListBlock;
      const [[start, _style, _delim], items] = orderedList.c;
      return (
        <ol key={key} start={start} style={{ marginTop: '0.5em', marginBottom: '0.5em', lineHeight: '1.6' }}>
          {items.map((item, i) => (
            <li key={i} style={{ marginTop: '0.3em', marginBottom: '0.3em' }}>
              {item.map((b, j) => renderBlock(b, j, currentFilePath, onNavigateToDocument))}
            </li>
          ))}
        </ol>
      );
    }

    case 'BlockQuote': {
      const blockQuote = block as BlockQuoteBlock;
      return (
        <blockquote
          key={key}
          style={{
            borderLeft: '5px solid #2a76dd',
            paddingLeft: '20px',
            marginLeft: '0',
            marginTop: '0.5em',
            marginBottom: '0.5em',
            color: '#555',
            fontStyle: 'italic'
          }}
        >
          {blockQuote.c.map((b, i) => renderBlock(b, i, currentFilePath, onNavigateToDocument))}
        </blockquote>
      );
    }

    case 'Div': {
      const divBlock = block as DivBlock;
      const [[id, classes, attrs], blocks] = divBlock.c;
      const props = attributesToProps(id, classes, attrs);
      return (
        <div key={key} {...props}>
          {blocks.map((b, i) => renderBlock(b, i, currentFilePath, onNavigateToDocument))}
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
      const figureStyle: React.CSSProperties = {
        margin: 0,
      };
      const props = attributesToProps(id, classes, attrs, figureStyle);

      return (
        <figure key={key} {...props}>
          {content.map((b, i) => renderBlock(b, i, currentFilePath, onNavigateToDocument))}
          {caption && caption.length > 0 && (
            <figcaption>{renderInlines(caption, currentFilePath, onNavigateToDocument)}</figcaption>
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
  currentFilePath: string,
  onNavigateToDocument?: (path: string, anchor: string | null) => void
): React.ReactNode[] {
  return inlines.map((inline, i) => renderInline(inline, i, currentFilePath, onNavigateToDocument));
}

function renderInline(
  inline: Inline,
  key: number,
  currentFilePath: string,
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
      return <em key={key}>{renderInlines(emphInline.c, currentFilePath, onNavigateToDocument)}</em>;
    }

    case 'Strong': {
      const strongInline = inline as StrongInline;
      return <strong key={key}>{renderInlines(strongInline.c, currentFilePath, onNavigateToDocument)}</strong>;
    }

    case 'Code': {
      const codeInline = inline as CodeInline;
      const [[id, classes, attrs], code] = codeInline.c;
      const props = attributesToProps(id, classes, attrs);
      return (
        <code key={key} {...props}>
          {code}
        </code>
      );
    }

    case 'Link': {
      const linkInline = inline as LinkInline;
      const [[id, classes, attrs], inlines, [url, title]] = linkInline.c;
      const props = attributesToProps(id, classes, attrs);

      // Handle .qmd links
      if (url.endsWith('.qmd') && onNavigateToDocument) {
        const [path, anchor] = url.split('#');
        return (
          <a
            key={key}
            {...props}
            href={url}
            title={title}
            onClick={(e) => {
              e.preventDefault();
              onNavigateToDocument(path, anchor || null);
            }}
          >
            {renderInlines(inlines, currentFilePath, onNavigateToDocument)}
          </a>
        );
      }

      return (
        <a key={key} {...props} href={url} title={title}>
          {renderInlines(inlines, currentFilePath, onNavigateToDocument)}
        </a>
      );
    }

    case 'Image': {
      const imageInline = inline as ImageInline;
      const [[id, classes, attrs], inlines, [url, title]] = imageInline.c;
      const props = attributesToProps(id, classes, attrs);
      const alt = inlines.map(i => {
        if ('c' in i && typeof i.c === 'string') return i.c;
        return '';
      }).join('');

      // Resolve image source from VFS
      let resolvedSrc = url;

      // Skip external URLs and data URIs
      if (!url.startsWith('http://') && !url.startsWith('https://') && !url.startsWith('data:')) {
        // Handle /.quarto/ paths (built-in resources)
        if (url.startsWith('/.quarto/')) {
          const result = vfsReadFile(url);
          if (result.success && result.content) {
            const mimeType = guessMimeType(url);
            resolvedSrc = `data:${mimeType};base64,${result.content}`;
          }
        } else {
          // Handle project-relative paths (images uploaded to project)
          const resolvedPath = resolveRelativePath(currentFilePath, url);
          // Remove leading slash for VFS path (VFS stores as "images/foo.png" not "/images/foo.png")
          const vfsPath = resolvedPath.startsWith('/') ? resolvedPath.slice(1) : resolvedPath;

          const result = vfsReadBinaryFile(vfsPath);
          if (result.success && result.content) {
            const mimeType = guessMimeType(url);
            // vfsReadBinaryFile returns base64-encoded content
            resolvedSrc = `data:${mimeType};base64,${result.content}`;
          }
        }
      }

      // Add styles to constrain image size
      const imageStyle: React.CSSProperties = {
        maxWidth: '100%',
        maxHeight: '500px',
        objectFit: 'contain',
        display: 'block',
        margin: '0 auto',
      };

      return (
        <img
          key={key}
          {...props}
          src={resolvedSrc}
          alt={alt}
          title={title}
          style={{ ...imageStyle, ...(props.style || {}) }}
        />
      );
    }

    case 'Span': {
      const spanInline = inline as SpanInline;
      const [[id, classes, attrs], inlines] = spanInline.c;
      const props = attributesToProps(id, classes, attrs);
      return (
        <span key={key} {...props}>
          {renderInlines(inlines, currentFilePath, onNavigateToDocument)}
        </span>
      );
    }

    case 'Math': {
      const mathInline = inline as MathInline;
      const [mathType, latex] = mathInline.c;
      const isDisplayMath = mathType.t === 'DisplayMath';

      try {
        const html = katex.renderToString(latex, {
          displayMode: isDisplayMath,
          throwOnError: false,
          output: 'html'
        });

        return (
          <span
            key={key}
            dangerouslySetInnerHTML={{ __html: html }}
          />
        );
      } catch (err) {
        console.error('KaTeX rendering error:', err);
        return (
          <span key={key} style={{ color: 'red', fontSize: '0.9em' }}>
            [Math Error: {latex}]
          </span>
        );
      }
    }

    default:
      console.warn('Unhandled inline type:', inline.t);
      return <span key={key} style={{ color: 'gray', fontSize: '0.9em' }}>[{inline.t}]</span>;
  }
}

// ============================================================================
// Helper Functions
// ============================================================================

/** Resolve a relative path against the current file's directory */
function resolveRelativePath(
  currentFile: string,
  relativePath: string
): string {
  if (relativePath.startsWith('/')) {
    return relativePath; // Already absolute
  }
  // Get directory of current file
  const lastSlash = currentFile.lastIndexOf('/');
  const currentDir =
    lastSlash >= 0 ? currentFile.substring(0, lastSlash + 1) : '/';
  return normalizePath(currentDir + relativePath);
}

function normalizePath(path: string): string {
  const parts = path.split('/').filter((p) => p !== '.');
  const result: string[] = [];
  for (const part of parts) {
    if (part === '..') {
      result.pop();
    } else if (part) {
      result.push(part);
    }
  }
  return '/' + result.join('/');
}

function guessMimeType(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase();
  const mimeTypes: Record<string, string> = {
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    gif: 'image/gif',
    svg: 'image/svg+xml',
    webp: 'image/webp',
  };
  return mimeTypes[ext || ''] || 'application/octet-stream';
}

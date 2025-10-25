/**
 * @quarto/annotated-qmd
 *
 * Converts quarto-markdown-pandoc JSON output to AnnotatedParse structures
 * compatible with quarto-cli's YAML validation infrastructure.
 */

// Re-export types and functions from @quarto/mapped-string
export type { MappedString } from '@quarto/mapped-string';
export { asMappedString, mappedSubstring } from '@quarto/mapped-string';

// Re-export our types
export type {
  AnnotatedParse,
  JSONValue,
  JsonMetaValue,
  MetaMapEntry,
  RustQmdJson
} from './types.js';

export type {
  SerializableSourceInfo,
  SourceContext,
  SourceInfoErrorHandler
} from './source-map.js';

// Re-export Pandoc AST types (base types)
export type {
  // Supporting types
  Attr,
  Target,
  MathType,
  QuoteType,
  ListNumberStyle,
  ListNumberDelim,
  ListAttributes,
  Citation,
  CitationMode,
  Alignment,
  ColWidth,
  ColSpec,
  Row,
  Cell,
  TableHead,
  TableBody,
  TableFoot,
  Caption,

  // Base Inline types
  Inline,
  Inline_Str,
  Inline_Space,
  Inline_SoftBreak,
  Inline_LineBreak,
  Inline_Emph,
  Inline_Strong,
  Inline_Strikeout,
  Inline_Superscript,
  Inline_Subscript,
  Inline_SmallCaps,
  Inline_Underline,
  Inline_Quoted,
  Inline_Code,
  Inline_Math,
  Inline_RawInline,
  Inline_Link,
  Inline_Image,
  Inline_Span,
  Inline_Cite,
  Inline_Note,

  // Base Block types
  Block,
  Block_Plain,
  Block_Para,
  Block_Header,
  Block_CodeBlock,
  Block_RawBlock,
  Block_BlockQuote,
  Block_BulletList,
  Block_OrderedList,
  Block_DefinitionList,
  Block_Div,
  Block_HorizontalRule,
  Block_Null,
  Block_Table,
  Block_Figure,

  // Base Meta types
  MetaValue,
  MetaValue_Map,
  MetaValue_List,
  MetaValue_Bool,
  MetaValue_String,
  MetaValue_Inlines,
  MetaValue_Blocks,

  // Base Document
  PandocDocument,

  // Annotated Inline types
  Annotated_Inline,
  Annotated_Inline_Str,
  Annotated_Inline_Space,
  Annotated_Inline_SoftBreak,
  Annotated_Inline_LineBreak,
  Annotated_Inline_Emph,
  Annotated_Inline_Strong,
  Annotated_Inline_Strikeout,
  Annotated_Inline_Superscript,
  Annotated_Inline_Subscript,
  Annotated_Inline_SmallCaps,
  Annotated_Inline_Underline,
  Annotated_Inline_Quoted,
  Annotated_Inline_Code,
  Annotated_Inline_Math,
  Annotated_Inline_RawInline,
  Annotated_Inline_Link,
  Annotated_Inline_Image,
  Annotated_Inline_Span,
  Annotated_Inline_Cite,
  Annotated_Inline_Note,

  // Annotated Block types
  Annotated_Block,
  Annotated_Block_Plain,
  Annotated_Block_Para,
  Annotated_Block_Header,
  Annotated_Block_CodeBlock,
  Annotated_Block_RawBlock,
  Annotated_Block_BlockQuote,
  Annotated_Block_BulletList,
  Annotated_Block_OrderedList,
  Annotated_Block_DefinitionList,
  Annotated_Block_Div,
  Annotated_Block_HorizontalRule,
  Annotated_Block_Null,
  Annotated_Block_Table,
  Annotated_Block_Figure,

  // Annotated Meta types
  Annotated_MetaValue,
  Annotated_MetaValue_Map,
  Annotated_MetaValue_List,
  Annotated_MetaValue_Bool,
  Annotated_MetaValue_String,
  Annotated_MetaValue_Inlines,
  Annotated_MetaValue_Blocks,

  // QMD Document
  QmdPandocDocument,
} from './pandoc-types.js';

export {
  isQmdPandocDocument,
  isInline,
  isBlock,
} from './pandoc-types.js';

// Re-export classes
export { SourceInfoReconstructor } from './source-map.js';
export { MetadataConverter } from './meta-converter.js';
export { InlineConverter } from './inline-converter.js';
export { BlockConverter } from './block-converter.js';
export { DocumentConverter } from './document-converter.js';

// Import for main functions
import { SourceInfoReconstructor } from './source-map.js';
import { MetadataConverter } from './meta-converter.js';
import { DocumentConverter, type AnnotatedPandocDocument } from './document-converter.js';
import { BlockConverter } from './block-converter.js';
import { InlineConverter } from './inline-converter.js';
import type { RustQmdJson, AnnotatedParse } from './types.js';
import type { SourceInfoErrorHandler } from './source-map.js';
import type { Annotated_Block, Annotated_Inline } from './pandoc-types.js';

/**
 * Convert quarto-markdown-pandoc JSON output to AnnotatedParse
 *
 * @param json - The JSON output from quarto-markdown-pandoc
 * @param errorHandler - Optional error handler for SourceInfo reconstruction errors
 * @returns AnnotatedParse structure compatible with quarto-cli
 *
 * @example
 * ```typescript
 * import { parseRustQmdMetadata } from '@quarto/annotated-qmd';
 *
 * const json = {
 *   meta: {
 *     title: { t: 'MetaString', c: 'Hello', s: 0 }
 *   },
 *   blocks: [],
 *   astContext: {
 *     sourceInfoPool: [
 *       { r: [11, 16], t: 0, d: 0 }
 *     ],
 *     files: [
 *       { name: 'test.qmd', content: '---\ntitle: Hello\n---' }
 *     ]
 *   },
 *   'pandoc-api-version': [1, 23, 1]
 * };
 *
 * const metadata = parseRustQmdMetadata(json);
 * console.log(metadata.result); // { title: 'Hello' }
 * ```
 */
export function parseRustQmdMetadata(
  json: RustQmdJson,
  errorHandler?: SourceInfoErrorHandler
): AnnotatedParse {
  // Normalize the JSON structure to internal format
  const sourceContext = {
    files: json.astContext.files.map((f, idx) => ({
      id: idx,
      path: f.name,
      content: f.content || ''
    }))
  };

  // 1. Create SourceInfoReconstructor with pool and context
  const sourceReconstructor = new SourceInfoReconstructor(
    json.astContext.sourceInfoPool,
    sourceContext,
    errorHandler
  );

  // 2. Create MetadataConverter with metaTopLevelKeySources
  const converter = new MetadataConverter(
    sourceReconstructor,
    json.astContext.metaTopLevelKeySources
  );

  // 3. Convert metadata to AnnotatedParse
  return converter.convertMeta(json.meta);
}

/**
 * Convert a complete quarto-markdown-pandoc document to AnnotatedParse
 *
 * @param json - The JSON output from quarto-markdown-pandoc (full document)
 * @param errorHandler - Optional error handler for SourceInfo reconstruction errors
 * @returns AnnotatedParse structure for the entire document
 *
 * @example
 * ```typescript
 * import { parseRustQmdDocument } from '@quarto/annotated-qmd';
 *
 * const json = {
 *   meta: { title: { t: 'MetaString', c: 'Hello', s: 0 } },
 *   blocks: [
 *     { t: 'Para', c: [{ t: 'Str', c: 'World', s: 1 }], s: 2 }
 *   ],
 *   astContext: { ... },
 *   'pandoc-api-version': [1, 23, 1]
 * };
 *
 * const doc = parseRustQmdDocument(json);
 * // doc.components includes metadata and all blocks
 * ```
 */
export function parseRustQmdDocument(
  json: RustQmdJson,
  errorHandler?: SourceInfoErrorHandler
): AnnotatedParse {
  // Normalize the JSON structure to internal format
  const sourceContext = {
    files: json.astContext.files.map((f, idx) => ({
      id: idx,
      path: f.name,
      content: f.content || ''
    }))
  };

  // Create SourceInfoReconstructor
  const sourceReconstructor = new SourceInfoReconstructor(
    json.astContext.sourceInfoPool,
    sourceContext,
    errorHandler
  );

  // Create DocumentConverter
  const converter = new DocumentConverter(
    sourceReconstructor,
    json.astContext.metaTopLevelKeySources
  );

  // Convert document (cast to AnnotatedPandocDocument since RustQmdJson extends it)
  return converter.convertDocument(json as unknown as AnnotatedPandocDocument);
}

/**
 * Convert an array of blocks to AnnotatedParse structures
 *
 * @param blocks - Array of annotated blocks from quarto-markdown-pandoc
 * @param json - The full JSON for source context (needed for sourceInfoPool)
 * @param errorHandler - Optional error handler for SourceInfo reconstruction errors
 * @returns Array of AnnotatedParse structures, one per block
 *
 * @example
 * ```typescript
 * import { parseRustQmdBlocks } from '@quarto/annotated-qmd';
 *
 * const blocks = parseRustQmdBlocks(json.blocks, json);
 * ```
 */
export function parseRustQmdBlocks(
  blocks: Annotated_Block[],
  json: RustQmdJson,
  errorHandler?: SourceInfoErrorHandler
): AnnotatedParse[] {
  const sourceContext = {
    files: json.astContext.files.map((f, idx) => ({
      id: idx,
      path: f.name,
      content: f.content || ''
    }))
  };

  const sourceReconstructor = new SourceInfoReconstructor(
    json.astContext.sourceInfoPool,
    sourceContext,
    errorHandler
  );

  const converter = new DocumentConverter(sourceReconstructor);
  return converter.convertBlocks(blocks);
}

/**
 * Convert a single block to AnnotatedParse
 *
 * @param block - A single annotated block from quarto-markdown-pandoc
 * @param json - The full JSON for source context (needed for sourceInfoPool)
 * @param errorHandler - Optional error handler for SourceInfo reconstruction errors
 * @returns AnnotatedParse structure for the block
 *
 * @example
 * ```typescript
 * import { parseRustQmdBlock } from '@quarto/annotated-qmd';
 *
 * const block = parseRustQmdBlock(json.blocks[0], json);
 * ```
 */
export function parseRustQmdBlock(
  block: Annotated_Block,
  json: RustQmdJson,
  errorHandler?: SourceInfoErrorHandler
): AnnotatedParse {
  const sourceContext = {
    files: json.astContext.files.map((f, idx) => ({
      id: idx,
      path: f.name,
      content: f.content || ''
    }))
  };

  const sourceReconstructor = new SourceInfoReconstructor(
    json.astContext.sourceInfoPool,
    sourceContext,
    errorHandler
  );

  const converter = new DocumentConverter(sourceReconstructor);
  return converter.convertBlock(block);
}

/**
 * Convert a single inline to AnnotatedParse
 *
 * @param inline - A single annotated inline from quarto-markdown-pandoc
 * @param json - The full JSON for source context (needed for sourceInfoPool)
 * @param errorHandler - Optional error handler for SourceInfo reconstruction errors
 * @returns AnnotatedParse structure for the inline
 *
 * @example
 * ```typescript
 * import { parseRustQmdInline } from '@quarto/annotated-qmd';
 *
 * const inline = parseRustQmdInline(someInline, json);
 * ```
 */
export function parseRustQmdInline(
  inline: Annotated_Inline,
  json: RustQmdJson,
  errorHandler?: SourceInfoErrorHandler
): AnnotatedParse {
  const sourceContext = {
    files: json.astContext.files.map((f, idx) => ({
      id: idx,
      path: f.name,
      content: f.content || ''
    }))
  };

  const sourceReconstructor = new SourceInfoReconstructor(
    json.astContext.sourceInfoPool,
    sourceContext,
    errorHandler
  );

  const converter = new DocumentConverter(sourceReconstructor);
  return converter.convertInline(inline);
}

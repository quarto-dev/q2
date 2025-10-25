/**
 * Type definitions for Pandoc JSON AST and quarto-markdown-pandoc extensions
 *
 * This file defines types for the standard Pandoc JSON format, plus extensions
 * for quarto-markdown-pandoc's source location information.
 *
 * STRUCTURAL COMPATIBILITY:
 * Annotated types are structurally compatible with base Pandoc types. This means
 * documents with source annotations can be serialized and consumed by standard
 * Pandoc tools - Pandoc will simply ignore the additional `s` (source info) and
 * `*S` (source tracking) fields. For example:
 *
 * - Annotated_Inline extends Inline by adding `s: number` field
 * - Annotated_Block extends Block by adding `s: number` field
 * - Elements with attributes add `attrS: AttrSourceInfo` for attribute source tracking
 * - Elements with targets add `targetS: TargetSourceInfo` for target source tracking
 *
 * This design ensures that quarto-markdown-pandoc JSON output is valid Pandoc JSON
 * and can be processed by the standard Pandoc toolchain.
 *
 * The types are based on observation of Pandoc's JSON output since there is no
 * official JSON schema documentation.
 */

import type { MappedString } from '@quarto/mapped-string';

// =============================================================================
// 1. Supporting Types (used throughout Pandoc AST)
// =============================================================================

/**
 * Attributes structure: [id, classes, key-value pairs]
 */
export type Attr = [string, string[], [string, string][]];

/**
 * Target for links and images: [url, title]
 */
export type Target = [string, string];

/**
 * Math type discriminator
 */
export type MathType =
  | { t: "InlineMath" }
  | { t: "DisplayMath" };

/**
 * Quote type discriminator
 */
export type QuoteType =
  | { t: "SingleQuote" }
  | { t: "DoubleQuote" };

/**
 * List number style
 */
export type ListNumberStyle =
  | { t: "DefaultStyle" }
  | { t: "Example" }
  | { t: "Decimal" }
  | { t: "LowerRoman" }
  | { t: "UpperRoman" }
  | { t: "LowerAlpha" }
  | { t: "UpperAlpha" };

/**
 * List number delimiter
 */
export type ListNumberDelim =
  | { t: "DefaultDelim" }
  | { t: "Period" }
  | { t: "OneParen" }
  | { t: "TwoParens" };

/**
 * List attributes for ordered lists: [start_number, style, delimiter]
 */
export type ListAttributes = [number, ListNumberStyle, ListNumberDelim];

/**
 * Citation mode
 */
export type CitationMode =
  | { t: "AuthorInText" }
  | { t: "SuppressAuthor" }
  | { t: "NormalCitation" };

/**
 * Table column alignment
 */
export type Alignment =
  | { t: "AlignLeft" }
  | { t: "AlignRight" }
  | { t: "AlignCenter" }
  | { t: "AlignDefault" };

/**
 * Table column width specification
 */
export type ColWidth =
  | { t: "ColWidth"; c: number }
  | { t: "ColWidthDefault" };

/**
 * Column specification: [alignment, width]
 */
export type ColSpec = [Alignment, ColWidth];

// =============================================================================
// 2. Base Pandoc Inline Types (standard, no source info)
// =============================================================================

export type Inline =
  | Inline_Str
  | Inline_Space
  | Inline_SoftBreak
  | Inline_LineBreak
  | Inline_Emph
  | Inline_Strong
  | Inline_Strikeout
  | Inline_Superscript
  | Inline_Subscript
  | Inline_SmallCaps
  | Inline_Underline
  | Inline_Quoted
  | Inline_Code
  | Inline_Math
  | Inline_RawInline
  | Inline_Link
  | Inline_Image
  | Inline_Span
  | Inline_Cite
  | Inline_Note;

// Simple text
export type Inline_Str = { t: "Str"; c: string };
export type Inline_Space = { t: "Space" };
export type Inline_SoftBreak = { t: "SoftBreak" };
export type Inline_LineBreak = { t: "LineBreak" };

// Formatting
export type Inline_Emph = { t: "Emph"; c: Inline[] };
export type Inline_Strong = { t: "Strong"; c: Inline[] };
export type Inline_Strikeout = { t: "Strikeout"; c: Inline[] };
export type Inline_Superscript = { t: "Superscript"; c: Inline[] };
export type Inline_Subscript = { t: "Subscript"; c: Inline[] };
export type Inline_SmallCaps = { t: "SmallCaps"; c: Inline[] };
export type Inline_Underline = { t: "Underline"; c: Inline[] };

// Quotes
export type Inline_Quoted = { t: "Quoted"; c: [QuoteType, Inline[]] };

// Code and math
export type Inline_Code = { t: "Code"; c: [Attr, string] };
export type Inline_Math = { t: "Math"; c: [MathType, string] };
export type Inline_RawInline = { t: "RawInline"; c: [string, string] };  // [format, content]

// Links and images
export type Inline_Link = { t: "Link"; c: [Attr, Inline[], Target] };
export type Inline_Image = { t: "Image"; c: [Attr, Inline[], Target] };

// Span (generic container with attributes)
export type Inline_Span = { t: "Span"; c: [Attr, Inline[]] };

// Citations
export interface Citation {
  citationId: string;
  citationPrefix: Inline[];
  citationSuffix: Inline[];
  citationMode: CitationMode;
  citationNoteNum: number;
  citationHash: number;
}
export type Inline_Cite = { t: "Cite"; c: [Citation[], Inline[]] };

// Footnote
export type Inline_Note = { t: "Note"; c: Block[] };

// =============================================================================
// 3. Base Pandoc Block Types (standard, no source info)
// =============================================================================

export type Block =
  | Block_Plain
  | Block_Para
  | Block_Header
  | Block_CodeBlock
  | Block_RawBlock
  | Block_BlockQuote
  | Block_BulletList
  | Block_OrderedList
  | Block_DefinitionList
  | Block_Div
  | Block_HorizontalRule
  | Block_Null
  | Block_Table
  | Block_Figure;

// Simple blocks with inline content
export type Block_Plain = { t: "Plain"; c: Inline[] };
export type Block_Para = { t: "Para"; c: Inline[] };

// Headers: [level, attr, content]
export type Block_Header = { t: "Header"; c: [number, Attr, Inline[]] };

// Code blocks
export type Block_CodeBlock = { t: "CodeBlock"; c: [Attr, string] };
export type Block_RawBlock = { t: "RawBlock"; c: [string, string] };  // [format, content]

// Block quotes
export type Block_BlockQuote = { t: "BlockQuote"; c: Block[] };

// Lists
export type Block_BulletList = { t: "BulletList"; c: Block[][] };  // List of items
export type Block_OrderedList = { t: "OrderedList"; c: [ListAttributes, Block[][]] };
export type Block_DefinitionList = { t: "DefinitionList"; c: [Inline[], Block[][]][] };  // [(term, definitions)]

// Structural
export type Block_Div = { t: "Div"; c: [Attr, Block[]] };
export type Block_HorizontalRule = { t: "HorizontalRule" };
export type Block_Null = { t: "Null" };

// Table types (Pandoc array format)
export type Row = [Attr, Cell[]];
export type Cell = [Attr, Alignment, number, number, Block[]]; // [attr, alignment, rowSpan, colSpan, content]
export type TableHead = [Attr, Row[]];
export type TableBody = [Attr, number, Row[], Row[]]; // [attr, rowHeadColumns, head, body]
export type TableFoot = [Attr, Row[]];
export type Caption = [Inline[] | null, Block[]]; // [short, long]

export type Block_Table = {
  t: "Table";
  c: [Attr, Caption, ColSpec[], TableHead, TableBody[], TableFoot];
};

// Figures (Pandoc 3.0+)
export type Block_Figure = { t: "Figure"; c: [Attr, Caption, Block[]] };

// =============================================================================
// 4. Base Pandoc Meta Types (standard, no source info)
// =============================================================================

export type MetaValue =
  | MetaValue_Map
  | MetaValue_List
  | MetaValue_Bool
  | MetaValue_String
  | MetaValue_Inlines
  | MetaValue_Blocks;

export type MetaValue_Map = { t: "MetaMap"; c: Record<string, MetaValue> };
export type MetaValue_List = { t: "MetaList"; c: MetaValue[] };
export type MetaValue_Bool = { t: "MetaBool"; c: boolean };
export type MetaValue_String = { t: "MetaString"; c: string };
export type MetaValue_Inlines = { t: "MetaInlines"; c: Inline[] };
export type MetaValue_Blocks = { t: "MetaBlocks"; c: Block[] };

// =============================================================================
// 5. Base Pandoc Document (standard)
// =============================================================================

export interface PandocDocument {
  "pandoc-api-version": [number, number, number];
  meta: Record<string, MetaValue>;
  blocks: Block[];
}

// =============================================================================
// 6. Source Info Types (for tracking source locations)
// =============================================================================

/**
 * Source information for Attr tuple: [id, classes, key-value pairs]
 * Mirrors the structure with source IDs (or null if empty/missing)
 *
 * Example for attr ["my-id", ["class1", "class2"], [["key1", "value1"]]]
 * attrS would be: {id: 1, classes: [2, 3], kvs: [[4, 5]]}
 */
export interface AttrSourceInfo {
  id: number | null;                     // Source ID for id string (null if "")
  classes: (number | null)[];             // Source IDs for each class
  kvs: [number | null, number | null][]; // Source IDs for each [key, value] pair
}

/**
 * Source information for Target tuple: [url, title]
 * Mirrors the structure with source IDs
 *
 * Example for target ["https://example.com", "Example"]
 * targetS would be: [10, 11]
 */
export type TargetSourceInfo = [
  number | null,  // Source ID for URL
  number | null   // Source ID for title
];

/**
 * Source info for table structural elements
 */
export interface CellSourceInfo {
  s: number;
  attrS: AttrSourceInfo;
}

export interface RowSourceInfo {
  s: number;
  attrS: AttrSourceInfo;
  cellsS: CellSourceInfo[];
}

export interface TableHeadSourceInfo {
  s: number;
  attrS: AttrSourceInfo;
  rowsS: RowSourceInfo[];
}

export interface TableBodySourceInfo {
  s: number;
  attrS: AttrSourceInfo;
  headS: RowSourceInfo[];
  bodyS: RowSourceInfo[];
}

export interface TableFootSourceInfo {
  s: number;
  attrS: AttrSourceInfo;
  rowsS: RowSourceInfo[];
}

/**
 * Serialized source information from Rust
 */
export interface SerializableSourceInfo {
  r: [number, number];  // Range [start, end]
  t: number;            // File/target ID
  d: unknown;           // Additional data (varies by type)
}

/**
 * Source context for reconstructing MappedStrings
 */
export interface SourceContext {
  files: Array<{
    id: number;
    path: string;
    content: string;
  }>;
}

/**
 * Error handler for source info reconstruction
 */
export type SourceInfoErrorHandler = (msg: string, id?: number) => void;

// =============================================================================
// 7. Annotated Pandoc Inline Types (base types + source info)
// =============================================================================

export type Annotated_Inline =
  | Annotated_Inline_Str
  | Annotated_Inline_Space
  | Annotated_Inline_SoftBreak
  | Annotated_Inline_LineBreak
  | Annotated_Inline_Emph
  | Annotated_Inline_Strong
  | Annotated_Inline_Strikeout
  | Annotated_Inline_Superscript
  | Annotated_Inline_Subscript
  | Annotated_Inline_SmallCaps
  | Annotated_Inline_Underline
  | Annotated_Inline_Quoted
  | Annotated_Inline_Code
  | Annotated_Inline_Math
  | Annotated_Inline_RawInline
  | Annotated_Inline_Link
  | Annotated_Inline_Image
  | Annotated_Inline_Span
  | Annotated_Inline_Cite
  | Annotated_Inline_Note;

// Simple text (leaf nodes - no nested children)
export interface Annotated_Inline_Str {
  t: "Str";
  c: string;
  s: number;
}

export interface Annotated_Inline_Space {
  t: "Space";
  s: number;
}

export interface Annotated_Inline_SoftBreak {
  t: "SoftBreak";
  s: number;
}

export interface Annotated_Inline_LineBreak {
  t: "LineBreak";
  s: number;
}

// Formatting (contain Annotated_Inline[] not Inline[])
export interface Annotated_Inline_Emph {
  t: "Emph";
  c: Annotated_Inline[];
  s: number;
}

export interface Annotated_Inline_Strong {
  t: "Strong";
  c: Annotated_Inline[];
  s: number;
}

export interface Annotated_Inline_Strikeout {
  t: "Strikeout";
  c: Annotated_Inline[];
  s: number;
}

export interface Annotated_Inline_Superscript {
  t: "Superscript";
  c: Annotated_Inline[];
  s: number;
}

export interface Annotated_Inline_Subscript {
  t: "Subscript";
  c: Annotated_Inline[];
  s: number;
}

export interface Annotated_Inline_SmallCaps {
  t: "SmallCaps";
  c: Annotated_Inline[];
  s: number;
}

export interface Annotated_Inline_Underline {
  t: "Underline";
  c: Annotated_Inline[];
  s: number;
}

// Quoted
export interface Annotated_Inline_Quoted {
  t: "Quoted";
  c: [QuoteType, Annotated_Inline[]];
  s: number;
}

// Code and math (leaf nodes with attributes)
export interface Annotated_Inline_Code {
  t: "Code";
  c: [Attr, string];
  s: number;
  attrS: AttrSourceInfo;
}

export interface Annotated_Inline_Math {
  t: "Math";
  c: [MathType, string];
  s: number;
}

export interface Annotated_Inline_RawInline {
  t: "RawInline";
  c: [string, string];  // [format, content]
  s: number;
}

// Links and images (with attrS and targetS)
export interface Annotated_Inline_Link {
  t: "Link";
  c: [Attr, Annotated_Inline[], Target];
  s: number;
  attrS: AttrSourceInfo;
  targetS: TargetSourceInfo;
}

export interface Annotated_Inline_Image {
  t: "Image";
  c: [Attr, Annotated_Inline[], Target];
  s: number;
  attrS: AttrSourceInfo;
  targetS: TargetSourceInfo;
}

// Span (with attrS)
export interface Annotated_Inline_Span {
  t: "Span";
  c: [Attr, Annotated_Inline[]];
  s: number;
  attrS: AttrSourceInfo;
}

// Citations (with annotated Citation and citationIdS)
export interface Annotated_Citation {
  citationId: string;
  citationPrefix: Annotated_Inline[];
  citationSuffix: Annotated_Inline[];
  citationMode: CitationMode;
  citationNoteNum: number;
  citationHash: number;
  citationIdS: number | null;
}

export interface Annotated_Inline_Cite {
  t: "Cite";
  c: [Annotated_Citation[], Annotated_Inline[]];
  s: number;
}

// Footnote (cross-reference to Annotated_Block)
export interface Annotated_Inline_Note {
  t: "Note";
  c: Annotated_Block[];
  s: number;
}

// =============================================================================
// 8. Annotated Pandoc Block Types (base types + source info)
// =============================================================================

export type Annotated_Block =
  | Annotated_Block_Plain
  | Annotated_Block_Para
  | Annotated_Block_Header
  | Annotated_Block_CodeBlock
  | Annotated_Block_RawBlock
  | Annotated_Block_BlockQuote
  | Annotated_Block_BulletList
  | Annotated_Block_OrderedList
  | Annotated_Block_DefinitionList
  | Annotated_Block_Div
  | Annotated_Block_HorizontalRule
  | Annotated_Block_Null
  | Annotated_Block_Table
  | Annotated_Block_Figure;

// Simple blocks with inline content
export interface Annotated_Block_Plain {
  t: "Plain";
  c: Annotated_Inline[];
  s: number;
}

export interface Annotated_Block_Para {
  t: "Para";
  c: Annotated_Inline[];
  s: number;
}

// Headers (with attrS)
export interface Annotated_Block_Header {
  t: "Header";
  c: [number, Attr, Annotated_Inline[]];
  s: number;
  attrS: AttrSourceInfo;
}

// Code blocks (with attrS)
export interface Annotated_Block_CodeBlock {
  t: "CodeBlock";
  c: [Attr, string];
  s: number;
  attrS: AttrSourceInfo;
}

export interface Annotated_Block_RawBlock {
  t: "RawBlock";
  c: [string, string];  // [format, content]
  s: number;
}

// Block quotes
export interface Annotated_Block_BlockQuote {
  t: "BlockQuote";
  c: Annotated_Block[];
  s: number;
}

// Lists
export interface Annotated_Block_BulletList {
  t: "BulletList";
  c: Annotated_Block[][];  // List of items
  s: number;
}

export interface Annotated_Block_OrderedList {
  t: "OrderedList";
  c: [ListAttributes, Annotated_Block[][]];
  s: number;
}

export interface Annotated_Block_DefinitionList {
  t: "DefinitionList";
  c: [Annotated_Inline[], Annotated_Block[][]][];  // [(term, definitions)]
  s: number;
}

// Structural (with attrS)
export interface Annotated_Block_Div {
  t: "Div";
  c: [Attr, Annotated_Block[]];
  s: number;
  attrS: AttrSourceInfo;
}

export interface Annotated_Block_HorizontalRule {
  t: "HorizontalRule";
  s: number;
}

export interface Annotated_Block_Null {
  t: "Null";
  s: number;
}

// Annotated table array types
export type Annotated_Row = [Attr, Annotated_Cell[]];
export type Annotated_Cell = [Attr, Alignment, number, number, Annotated_Block[]];
export type Annotated_TableHead_Array = [Attr, Annotated_Row[]];
export type Annotated_TableBody_Array = [Attr, number, Annotated_Row[], Annotated_Row[]];
export type Annotated_TableFoot_Array = [Attr, Annotated_Row[]];
export type Annotated_CaptionArray = [Annotated_Inline[] | null, Annotated_Block[]];

// Helper type for Caption with annotated content
export interface Annotated_Caption {
  shortCaption: Annotated_Inline[] | null;
  longCaption: Annotated_Block[];
}

// Table block with parallel source tracking
export interface Annotated_Block_Table {
  t: "Table";
  c: [Attr, Annotated_CaptionArray, ColSpec[], Annotated_TableHead_Array, Annotated_TableBody_Array[], Annotated_TableFoot_Array];
  s: number;
  attrS: AttrSourceInfo;
  captionS: number; // Source info ref for caption
  headS: TableHeadSourceInfo;
  bodiesS: TableBodySourceInfo[];
  footS: TableFootSourceInfo;
}

// Figures (with attrS)
export interface Annotated_Block_Figure {
  t: "Figure";
  c: [Attr, Annotated_CaptionArray, Annotated_Block[]];
  s: number;
  attrS: AttrSourceInfo;
}

// =============================================================================
// 9. Annotated Pandoc Meta Types (base types + source info)
// =============================================================================

export type Annotated_MetaValue =
  | Annotated_MetaValue_Map
  | Annotated_MetaValue_List
  | Annotated_MetaValue_Bool
  | Annotated_MetaValue_String
  | Annotated_MetaValue_Inlines
  | Annotated_MetaValue_Blocks;

export interface Annotated_MetaValue_Map {
  t: "MetaMap";
  c: Record<string, Annotated_MetaValue>;
  s: number;
}

export interface Annotated_MetaValue_List {
  t: "MetaList";
  c: Annotated_MetaValue[];
  s: number;
}

export interface Annotated_MetaValue_Bool {
  t: "MetaBool";
  c: boolean;
  s: number;
}

export interface Annotated_MetaValue_String {
  t: "MetaString";
  c: string;
  s: number;
}

export interface Annotated_MetaValue_Inlines {
  t: "MetaInlines";
  c: Annotated_Inline[];
  s: number;
}

export interface Annotated_MetaValue_Blocks {
  t: "MetaBlocks";
  c: Annotated_Block[];
  s: number;
}

// =============================================================================
// 10. Package I/O Types
// =============================================================================

/**
 * JSON value type (matching quarto-cli's JSONValue)
 */
export type JSONValue =
  | string
  | number
  | boolean
  | null
  | JSONValue[]
  | { [key: string]: JSONValue };

/**
 * AnnotatedParse structure (matching quarto-cli's interface)
 * This is the output format from all conversion functions.
 */
export interface AnnotatedParse {
  start: number;
  end: number;
  result: JSONValue;
  kind: string;
  source: MappedString;
  components: AnnotatedParse[];
}

/**
 * MetaValue from quarto-markdown-pandoc JSON (raw format before conversion)
 */
export interface JsonMetaValue {
  t: string;      // Type: "MetaString", "MetaBool", "MetaInlines", "MetaBlocks", "MetaList", "MetaMap"
  c?: unknown;    // Content (varies by type)
  s: number;      // SourceInfo ID
}

/**
 * MetaMap entry structure
 */
export interface MetaMapEntry {
  key: string;
  key_source: number;  // SourceInfo ID for key
  value: JsonMetaValue;
}

/**
 * File information from Rust JSON output
 */
export interface RustFileInfo {
  name: string;              // File path/name
  line_breaks?: number[];    // Byte offsets of newlines
  total_length?: number;     // Total file length in bytes
  content?: string;          // File content (populated by consumer)
}

/**
 * Complete JSON output from quarto-markdown-pandoc (Rust parser).
 * This is the input format for all parse functions.
 *
 * Use the parse functions to convert to AnnotatedParse:
 * - `parseRustQmdDocument(json)` - convert entire document
 * - `parseRustQmdBlocks(json.blocks, json)` - convert just blocks
 * - `parseRustQmdMetadata(json)` - convert just metadata
 * - `parseRustQmdBlock(json.blocks[i], json)` - convert single block
 * - `parseRustQmdInline(inline, json)` - convert single inline
 */
export interface RustQmdJson {
  "pandoc-api-version": [number, number, number];

  /** Metadata with source info (JsonMetaValue includes source ID) */
  meta: Record<string, JsonMetaValue>;

  /** Blocks with source info (Annotated_Block[] at runtime) */
  blocks: Annotated_Block[];

  /** Source location tracking data */
  astContext: {
    sourceInfoPool: SerializableSourceInfo[];
    files: RustFileInfo[];
    metaTopLevelKeySources?: Record<string, number>;
  };
}

// =============================================================================
// 11. Type Guards
// =============================================================================

export function isInline(node: unknown): node is Inline {
  return (
    typeof node === 'object' &&
    node !== null &&
    't' in node &&
    typeof (node as { t: unknown }).t === 'string'
  );
}

export function isBlock(node: unknown): node is Block {
  return (
    typeof node === 'object' &&
    node !== null &&
    't' in node &&
    typeof (node as { t: unknown }).t === 'string'
  );
}

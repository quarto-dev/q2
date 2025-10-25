/**
 * TypeScript type declarations for Pandoc JSON AST
 *
 * This file defines types for the standard Pandoc JSON format, plus
 * extensions for quarto-markdown-pandoc's source location information.
 *
 * Structure:
 * 1. Base Pandoc types (standard, no extensions)
 * 2. Annotated types (base types + source info via intersection)
 *
 * The types are based on observation of Pandoc's JSON output since
 * there is no official JSON schema documentation.
 */

// =============================================================================
// Supporting types used throughout Pandoc AST
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
// Base Pandoc Inline types (standard, no source info)
// =============================================================================

// Forward declarations for recursive types
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
// Base Pandoc Block types (standard, no source info)
// =============================================================================

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

// Tables
// Table structural types - now matching Pandoc's array format
export type Row = [Attr, Cell[]];
export type Annotated_Row = [Attr, Annotated_Cell[]];

export type Cell = [Attr, Alignment, number, number, Block[]]; // [attr, alignment, rowSpan, colSpan, content]
export type Annotated_Cell = [Attr, Alignment, number, number, Annotated_Block[]]; // annotated version

export type TableHead = [Attr, Row[]];
export type Annotated_TableHead_Array = [Attr, Annotated_Row[]];

export type TableBody = [Attr, number, Row[], Row[]]; // [attr, rowHeadColumns, head, body]
export type Annotated_TableBody_Array = [Attr, number, Annotated_Row[], Annotated_Row[]];

export type TableFoot = [Attr, Row[]];
export type Annotated_TableFoot_Array = [Attr, Annotated_Row[]];

// Caption types
export type Caption = [Inline[] | null, Block[]]; // [short, long] - base Pandoc format
export type Annotated_CaptionArray = [Annotated_Inline[] | null, Annotated_Block[]]; // [short, long] - with annotations

export type Block_Table = {
  t: "Table";
  c: [Attr, Caption, ColSpec[], TableHead, TableBody[], TableFoot];
};

// Figures (Pandoc 3.0+)
export type Block_Figure = { t: "Figure"; c: [Attr, Caption, Block[]] };

// =============================================================================
// Base Pandoc Meta types (standard, no source info)
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
// Base Pandoc Document (standard)
// =============================================================================

export interface PandocDocument {
  "pandoc-api-version": [number, number, number];
  meta: Record<string, MetaValue>;
  blocks: Block[];
}

// =============================================================================
// Sideloaded Source Info Types (for tuple-based structures)
// =============================================================================

/**
 * Source information for Attr tuple: [id, classes, key-value pairs]
 * Mirrors the structure with source IDs (or null if empty/missing)
 *
 * Example for attr [id, ["class1", "class2"], [["key1", "value1"]]]
 * attrS would be:  {id: 1, classes: [2, 3], kvs: [[4, 5]]}
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
 * targetS would be:  [10, 11]
 */
export type TargetSourceInfo = [
  number | null,  // Source ID for URL
  number | null   // Source ID for title
];

// =============================================================================
// Annotated types (full parallel hierarchy with source info)
// =============================================================================

// Forward declarations for recursive annotated types
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

// -----------------------------------------------------------------------------
// Annotated Inline types (with proper nested references)
// -----------------------------------------------------------------------------

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

// -----------------------------------------------------------------------------
// Annotated Block types (with proper nested references)
// -----------------------------------------------------------------------------

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

// Tables (with annotated table components)
// Annotated table types - arrays in 'c' field, source info in parallel fields

// Source info for Cell
export interface CellSourceInfo {
  s: number;
  attrS: AttrSourceInfo;
}

// Source info for Row
export interface RowSourceInfo {
  s: number;
  attrS: AttrSourceInfo;
  cellsS: CellSourceInfo[];
}

// Source info for TableHead
export interface TableHeadSourceInfo {
  s: number;
  attrS: AttrSourceInfo;
  rowsS: RowSourceInfo[];
}

// Source info for TableBody
export interface TableBodySourceInfo {
  s: number;
  attrS: AttrSourceInfo;
  headS: RowSourceInfo[];
  bodyS: RowSourceInfo[];
}

// Source info for TableFoot
export interface TableFootSourceInfo {
  s: number;
  attrS: AttrSourceInfo;
  rowsS: RowSourceInfo[];
}

// Helper type for Caption with annotated content
// Caption is [short | null, long] in Pandoc format
export interface Annotated_Caption {
  shortCaption: Annotated_Inline[] | null;
  longCaption: Annotated_Block[];
}

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

// -----------------------------------------------------------------------------
// Annotated Meta types (with proper nested references)
// -----------------------------------------------------------------------------

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

export type Annotated_MetaValue =
  | Annotated_MetaValue_Map
  | Annotated_MetaValue_List
  | Annotated_MetaValue_Bool
  | Annotated_MetaValue_String
  | Annotated_MetaValue_Inlines
  | Annotated_MetaValue_Blocks;

// =============================================================================
// QMD Extended Document (with astContext)
// =============================================================================

export interface QmdPandocDocument extends PandocDocument {
  astContext: {
    sourceInfoPool: Array<{
      r: [number, number];
      t: number;
      d: unknown;
    }>;
    files: Array<{
      name: string;
      line_breaks?: number[];
      total_length?: number;
      content?: string;
    }>;
    metaTopLevelKeySources?: Record<string, number>;
  };
}

// =============================================================================
// Type guards
// =============================================================================

export function isQmdPandocDocument(doc: PandocDocument): doc is QmdPandocDocument {
  return 'astContext' in doc;
}

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

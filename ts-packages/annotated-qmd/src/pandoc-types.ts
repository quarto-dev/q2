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
export interface Row {
  attr: Attr;
  cells: Cell[];
}

export interface Cell {
  attr: Attr;
  alignment: Alignment;
  rowSpan: number;
  colSpan: number;
  content: Block[];
}

export interface TableHead {
  attr: Attr;
  rows: Row[];
}

export interface TableBody {
  attr: Attr;
  rowHeadColumns: number;
  head: Row[];
  body: Row[];
}

export interface TableFoot {
  attr: Attr;
  rows: Row[];
}

export interface Caption {
  shortCaption: Inline[] | null;
  longCaption: Block[];
}

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
// Annotated types (base + source info)
// =============================================================================

// Annotated Inline types
export type Annotated_Inline_Str = Inline_Str & { s: number };
export type Annotated_Inline_Space = Inline_Space & { s: number };
export type Annotated_Inline_SoftBreak = Inline_SoftBreak & { s: number };
export type Annotated_Inline_LineBreak = Inline_LineBreak & { s: number };
export type Annotated_Inline_Emph = Inline_Emph & { s: number };
export type Annotated_Inline_Strong = Inline_Strong & { s: number };
export type Annotated_Inline_Strikeout = Inline_Strikeout & { s: number };
export type Annotated_Inline_Superscript = Inline_Superscript & { s: number };
export type Annotated_Inline_Subscript = Inline_Subscript & { s: number };
export type Annotated_Inline_SmallCaps = Inline_SmallCaps & { s: number };
export type Annotated_Inline_Underline = Inline_Underline & { s: number };
export type Annotated_Inline_Quoted = Inline_Quoted & { s: number };
export type Annotated_Inline_Code = Inline_Code & { s: number };
export type Annotated_Inline_Math = Inline_Math & { s: number };
export type Annotated_Inline_RawInline = Inline_RawInline & { s: number };
export type Annotated_Inline_Link = Inline_Link & { s: number };
export type Annotated_Inline_Image = Inline_Image & { s: number };
export type Annotated_Inline_Span = Inline_Span & { s: number };
export type Annotated_Inline_Cite = Inline_Cite & { s: number };
export type Annotated_Inline_Note = Inline_Note & { s: number };

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

// Annotated Block types
export type Annotated_Block_Plain = Block_Plain & { s: number };
export type Annotated_Block_Para = Block_Para & { s: number };
export type Annotated_Block_Header = Block_Header & { s: number };
export type Annotated_Block_CodeBlock = Block_CodeBlock & { s: number };
export type Annotated_Block_RawBlock = Block_RawBlock & { s: number };
export type Annotated_Block_BlockQuote = Block_BlockQuote & { s: number };
export type Annotated_Block_BulletList = Block_BulletList & { s: number };
export type Annotated_Block_OrderedList = Block_OrderedList & { s: number };
export type Annotated_Block_DefinitionList = Block_DefinitionList & { s: number };
export type Annotated_Block_Div = Block_Div & { s: number };
export type Annotated_Block_HorizontalRule = Block_HorizontalRule & { s: number };
export type Annotated_Block_Null = Block_Null & { s: number };
export type Annotated_Block_Table = Block_Table & { s: number };
export type Annotated_Block_Figure = Block_Figure & { s: number };

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

// Annotated Meta types
export type Annotated_MetaValue_Map = MetaValue_Map & { s: number };
export type Annotated_MetaValue_List = MetaValue_List & { s: number };
export type Annotated_MetaValue_Bool = MetaValue_Bool & { s: number };
export type Annotated_MetaValue_String = MetaValue_String & { s: number };
export type Annotated_MetaValue_Inlines = MetaValue_Inlines & { s: number };
export type Annotated_MetaValue_Blocks = MetaValue_Blocks & { s: number };

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

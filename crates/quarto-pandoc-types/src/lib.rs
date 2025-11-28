/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pandoc AST type definitions for Quarto.
 *
 * This crate provides pure data type definitions for the Pandoc AST,
 * mirroring the types from pandoc-types in Haskell. It has minimal
 * dependencies (serde, quarto-source-map, hashlink) and can be used
 * by any crate that needs to work with Pandoc AST structures.
 */

pub mod attr;
pub mod block;
pub mod caption;
pub mod inline;
pub mod list;
pub mod meta;
pub mod pandoc;
pub mod shortcode;
pub mod table;

// Re-export commonly used types at the crate root
pub use attr::{Attr, AttrSourceInfo, TargetSourceInfo, empty_attr, is_empty_attr};
pub use block::{
    Block, BlockQuote, Blocks, BulletList, CaptionBlock, CodeBlock, DefinitionList, Div, Figure,
    Header, HorizontalRule, LineBlock, MetaBlock, NoteDefinitionFencedBlock, NoteDefinitionPara,
    OrderedList, Paragraph, Plain, RawBlock,
};
pub use caption::Caption;
pub use inline::{
    AsInline, Citation, CitationMode, Cite, Code, Delete, EditComment, Emph, Highlight, Image,
    Inline, Inlines, Insert, LineBreak, Link, Math, MathType, Note, NoteReference, QuoteType,
    Quoted, RawInline, SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong, Subscript,
    Superscript, Target, Underline, is_empty_target, make_cite_inline, make_span_inline,
};
pub use list::{ListAttributes, ListNumberDelim, ListNumberStyle};
pub use meta::{
    Meta, MetaMapEntry, MetaValue, MetaValueWithSourceInfo, meta_from_legacy,
    meta_value_from_legacy,
};
pub use pandoc::Pandoc;
pub use shortcode::{Shortcode, ShortcodeArg};
pub use table::{Alignment, Cell, ColSpec, ColWidth, Row, Table, TableBody, TableFoot, TableHead};

/*
 * mod.rs
 * Copyright (c) 2025 Posit, PBC
 */

pub mod attr;
pub mod block;
pub mod caption;
pub mod inline;
pub mod list;
pub mod location;
pub mod meta;
pub mod pandoc;
pub mod shortcode;
pub mod table;
pub mod treesitter;

pub use crate::pandoc::attr::Attr;
pub use crate::pandoc::block::{
    Block, BlockQuote, Blocks, BulletList, CodeBlock, DefinitionList, Div, Figure, Header,
    HorizontalRule, LineBlock, OrderedList, Paragraph, Plain, RawBlock,
};
pub use crate::pandoc::caption::Caption;
pub use crate::pandoc::inline::{
    Citation, CitationMode, Cite, Code, Delete, EditComment, Emph, Highlight, Image, Inline,
    Inlines, Insert, LineBreak, Link, Math, MathType, Note, NoteReference, QuoteType, Quoted,
    RawInline, SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong, Subscript, Superscript,
    Underline,
};
pub use crate::pandoc::list::{ListAttributes, ListNumberDelim, ListNumberStyle};
pub use crate::pandoc::pandoc::Pandoc;
pub use crate::pandoc::shortcode::Shortcode;
pub use crate::pandoc::table::{
    Alignment, Cell, ColWidth, Row, Table, TableBody, TableFoot, TableHead,
};

pub use crate::pandoc::meta::{Meta, MetaValue, rawblock_to_meta};
pub use crate::pandoc::treesitter::treesitter_to_pandoc;

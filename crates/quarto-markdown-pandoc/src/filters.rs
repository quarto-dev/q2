/*
 * filters.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::MetaValue;
use crate::pandoc::block::MetaBlock;
use crate::pandoc::inline::AsInline;
use crate::pandoc::meta::Meta;
use crate::pandoc::{self, Block, Blocks, Inline, Inlines};

// filters are destructive and take ownership of the input

pub enum FilterReturn<T, U> {
    Unchanged(T),
    FilterResult(U, bool), // (new content, should recurse)
}

type InlineFilterFn<'a, T> = Box<dyn FnMut(T) -> FilterReturn<T, Inlines> + 'a>;
type BlockFilterFn<'a, T> = Box<dyn FnMut(T) -> FilterReturn<T, Blocks> + 'a>;
type MetaFilterFn<'a> = Box<dyn FnMut(Meta) -> FilterReturn<Meta, Meta> + 'a>;
type InlineFilterField<'a, T> = Option<InlineFilterFn<'a, T>>;
type BlockFilterField<'a, T> = Option<BlockFilterFn<'a, T>>;
type MetaFilterField<'a> = Option<MetaFilterFn<'a>>;

pub struct Filter<'a> {
    pub inlines: InlineFilterField<'a, Inlines>,
    pub blocks: BlockFilterField<'a, Blocks>,

    pub inline: InlineFilterField<'a, Inline>,
    pub block: BlockFilterField<'a, Block>,

    pub str: InlineFilterField<'a, pandoc::Str>,
    pub emph: InlineFilterField<'a, pandoc::Emph>,
    pub underline: InlineFilterField<'a, pandoc::Underline>,
    pub strong: InlineFilterField<'a, pandoc::Strong>,
    pub strikeout: InlineFilterField<'a, pandoc::Strikeout>,
    pub superscript: InlineFilterField<'a, pandoc::Superscript>,
    pub subscript: InlineFilterField<'a, pandoc::Subscript>,
    pub small_caps: InlineFilterField<'a, pandoc::SmallCaps>,
    pub quoted: InlineFilterField<'a, pandoc::Quoted>,
    pub cite: InlineFilterField<'a, pandoc::Cite>,
    pub code: InlineFilterField<'a, pandoc::Code>,
    pub space: InlineFilterField<'a, pandoc::Space>,
    pub soft_break: InlineFilterField<'a, pandoc::SoftBreak>,
    pub line_break: InlineFilterField<'a, pandoc::LineBreak>,
    pub math: InlineFilterField<'a, pandoc::Math>,
    pub raw_inline: InlineFilterField<'a, pandoc::RawInline>,
    pub link: InlineFilterField<'a, pandoc::Link>,
    pub image: InlineFilterField<'a, pandoc::Image>,
    pub note: InlineFilterField<'a, pandoc::Note>,
    pub span: InlineFilterField<'a, pandoc::Span>,
    pub shortcode: InlineFilterField<'a, pandoc::Shortcode>,
    pub note_reference: InlineFilterField<'a, pandoc::NoteReference>,
    pub attr: InlineFilterField<'a, pandoc::Attr>,
    pub insert: InlineFilterField<'a, pandoc::Insert>,
    pub delete: InlineFilterField<'a, pandoc::Delete>,
    pub highlight: InlineFilterField<'a, pandoc::Highlight>,
    pub edit_comment: InlineFilterField<'a, pandoc::EditComment>,

    pub paragraph: BlockFilterField<'a, pandoc::Paragraph>,
    pub plain: BlockFilterField<'a, pandoc::Plain>,
    pub code_block: BlockFilterField<'a, pandoc::CodeBlock>,
    pub raw_block: BlockFilterField<'a, pandoc::RawBlock>,
    pub bullet_list: BlockFilterField<'a, pandoc::BulletList>,
    pub ordered_list: BlockFilterField<'a, pandoc::OrderedList>,
    pub block_quote: BlockFilterField<'a, pandoc::BlockQuote>,
    pub div: BlockFilterField<'a, pandoc::Div>,
    pub figure: BlockFilterField<'a, pandoc::Figure>,
    pub line_block: BlockFilterField<'a, pandoc::LineBlock>,
    pub definition_list: BlockFilterField<'a, pandoc::DefinitionList>,
    pub header: BlockFilterField<'a, pandoc::Header>,
    pub table: BlockFilterField<'a, pandoc::Table>,
    pub horizontal_rule: BlockFilterField<'a, pandoc::HorizontalRule>,

    pub meta: MetaFilterField<'a>,
}

impl Default for Filter<'static> {
    fn default() -> Filter<'static> {
        Filter {
            inlines: None,
            blocks: None,
            inline: None,
            block: None,

            str: None,
            emph: None,
            underline: None,
            strong: None,
            strikeout: None,
            superscript: None,
            subscript: None,
            small_caps: None,
            quoted: None,
            cite: None,
            code: None,
            space: None,
            soft_break: None,
            line_break: None,
            math: None,
            raw_inline: None,
            link: None,
            image: None,
            note: None,
            span: None,
            shortcode: None,
            note_reference: None,

            paragraph: None,
            plain: None,
            code_block: None,
            raw_block: None,
            bullet_list: None,
            ordered_list: None,
            block_quote: None,
            div: None,
            figure: None,
            line_block: None,
            definition_list: None,
            header: None,
            table: None,
            horizontal_rule: None,
            attr: None,

            insert: None,
            delete: None,
            highlight: None,
            edit_comment: None,

            meta: None,
        }
    }
}

impl Filter<'static> {
    pub fn new() -> Filter<'static> {
        Self::default()
    }
}

impl<'a> Filter<'a> {
    pub fn with_inlines<F>(mut self, f: F) -> Filter<'a>
    where
        F: FnMut(Inlines) -> FilterReturn<Inlines, Inlines> + 'a,
    {
        self.inlines = Some(Box::new(f));
        self
    }

    pub fn with_blocks<F>(mut self, f: F) -> Filter<'a>
    where
        F: FnMut(Blocks) -> FilterReturn<Blocks, Blocks> + 'a,
    {
        self.blocks = Some(Box::new(f));
        self
    }

    pub fn with_meta<F>(mut self, f: F) -> Filter<'a>
    where
        F: FnMut(Meta) -> FilterReturn<Meta, Meta> + 'a,
    {
        self.meta = Some(Box::new(f));
        self
    }
}

macro_rules! define_filter_with_methods {
    ($return:ident, $($field:ident),* $(,)?) => {
        impl<'a> Filter<'a> {

            $(
                paste::paste! {
                    pub fn [<with_ $field>]<F>(mut self, filter: F) -> Filter<'a>
                    where
                        F: FnMut(pandoc::[<$field:camel>]) -> FilterReturn<pandoc::[<$field:camel>], $return> + 'a,
                    {
                        self.$field = Some(Box::new(filter));
                        self
                    }
                }
            )*
        }
    };
}

define_filter_with_methods!(
    Inlines,
    str,
    emph,
    underline,
    strong,
    strikeout,
    superscript,
    subscript,
    small_caps,
    quoted,
    cite,
    code,
    space,
    soft_break,
    line_break,
    math,
    raw_inline,
    link,
    image,
    note,
    span,
    shortcode,
    note_reference,
    attr,
    insert,
    delete,
    highlight,
    edit_comment
);

define_filter_with_methods!(
    Blocks,
    plain,
    paragraph,
    line_block,
    code_block,
    raw_block,
    block_quote,
    ordered_list,
    bullet_list,
    definition_list,
    header,
    horizontal_rule,
    table,
    figure,
    div
);

// Macro to generate repetitive match arms
// Macro to reduce repetition in filter logic
macro_rules! handle_inline_filter {
    ($variant:ident, $value:ident, $filter_field:ident, $filter:expr) => {
        if let Some(f) = &mut $filter.$filter_field {
            return inlines_apply_and_maybe_recurse!($value, f, $filter);
        } else if let Some(f) = &mut $filter.inline {
            return inlines_apply_and_maybe_recurse!($value.as_inline(), f, $filter);
        } else {
            vec![traverse_inline_structure(Inline::$variant($value), $filter)]
        }
    };
}

macro_rules! handle_block_filter {
    ($variant:ident, $value:ident, $filter_field:ident, $filter:expr) => {
        if let Some(f) = &mut $filter.$filter_field {
            return blocks_apply_and_maybe_recurse!($value, f, $filter);
        } else if let Some(f) = &mut $filter.block {
            return blocks_apply_and_maybe_recurse!(Block::$variant($value), f, $filter);
        } else {
            vec![traverse_block_structure(Block::$variant($value), $filter)]
        }
    };
}

trait InlineFilterableStructure {
    fn filter_structure(self, filter: &mut Filter) -> Inline;
}

macro_rules! impl_inline_filterable_terminal {
    ($($variant:ident),*) => {
        $(
            impl InlineFilterableStructure for pandoc::$variant {
                fn filter_structure(self, _: &mut Filter) -> Inline {
                    Inline::$variant(self)
                }
            }
        )*
    };
}
impl_inline_filterable_terminal!(
    Str,
    Code,
    Space,
    SoftBreak,
    LineBreak,
    Math,
    RawInline,
    Shortcode,
    NoteReference,
    Attr
);

macro_rules! impl_inline_filterable_simple {
    ($($variant:ident),*) => {
        $(
            impl InlineFilterableStructure for pandoc::$variant {
                fn filter_structure(self, filter: &mut Filter) -> Inline {
                    Inline::$variant(pandoc::$variant {
                        content: topdown_traverse_inlines(self.content, filter),
                        ..self
                    })
                }
            }
        )*
    };
}

impl_inline_filterable_simple!(
    Emph,
    Underline,
    Strong,
    Strikeout,
    Superscript,
    Subscript,
    SmallCaps,
    Quoted,
    Link,
    Image,
    Span,
    Insert,
    Delete,
    Highlight,
    EditComment
);

impl InlineFilterableStructure for pandoc::Note {
    fn filter_structure(self, filter: &mut Filter) -> Inline {
        Inline::Note(pandoc::Note {
            content: topdown_traverse_blocks(self.content, filter),
            source_info: self.source_info,
        })
    }
}

impl InlineFilterableStructure for pandoc::Cite {
    fn filter_structure(self, filter: &mut Filter) -> Inline {
        Inline::Cite(pandoc::Cite {
            citations: self
                .citations
                .into_iter()
                .map(|cit| pandoc::Citation {
                    id: cit.id,
                    prefix: topdown_traverse_inlines(cit.prefix, filter),
                    suffix: topdown_traverse_inlines(cit.suffix, filter),
                    mode: cit.mode,
                    note_num: cit.note_num,
                    hash: cit.hash,
                })
                .collect(),
            content: topdown_traverse_inlines(self.content, filter),
            source_info: self.source_info,
        })
    }
}

impl InlineFilterableStructure for Inline {
    fn filter_structure(self, filter: &mut Filter) -> Inline {
        traverse_inline_structure(self, filter)
    }
}
trait BlockFilterableStructure {
    fn filter_structure(self, filter: &mut Filter) -> Block;
}

macro_rules! impl_block_filterable_terminal {
    ($($variant:ident),*) => {
        $(
            impl BlockFilterableStructure for pandoc::$variant {
                fn filter_structure(self, _: &mut Filter) -> Block {
                    Block::$variant(self)
                }
            }
        )*
    };
}
impl_block_filterable_terminal!(CodeBlock, RawBlock, HorizontalRule);

macro_rules! impl_block_filterable_simple {
    ($($variant:ident),*) => {
        $(
            impl BlockFilterableStructure for pandoc::$variant {
                fn filter_structure(self, filter: &mut Filter) -> Block {
                    Block::$variant(pandoc::$variant {
                        content: topdown_traverse_blocks(self.content, filter),
                        ..self
                    })
                }
            }
        )*
    };
}
impl_block_filterable_simple!(BlockQuote, Div);

impl BlockFilterableStructure for pandoc::Paragraph {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        Block::Paragraph(pandoc::Paragraph {
            content: topdown_traverse_inlines(self.content, filter),
            ..self
        })
    }
}

impl BlockFilterableStructure for pandoc::Plain {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        Block::Plain(pandoc::Plain {
            content: topdown_traverse_inlines(self.content, filter),
            ..self
        })
    }
}

impl BlockFilterableStructure for pandoc::LineBlock {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        Block::LineBlock(pandoc::LineBlock {
            content: self
                .content
                .into_iter()
                .map(|inlines| topdown_traverse_inlines(inlines, filter))
                .collect(),
            ..self
        })
    }
}

impl BlockFilterableStructure for pandoc::OrderedList {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        Block::OrderedList(pandoc::OrderedList {
            content: self
                .content
                .into_iter()
                .map(|blocks| topdown_traverse_blocks(blocks, filter))
                .collect(),
            ..self
        })
    }
}

impl BlockFilterableStructure for pandoc::BulletList {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        Block::BulletList(pandoc::BulletList {
            content: self
                .content
                .into_iter()
                .map(|blocks| topdown_traverse_blocks(blocks, filter))
                .collect(),
            ..self
        })
    }
}

impl BlockFilterableStructure for pandoc::DefinitionList {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        Block::DefinitionList(pandoc::DefinitionList {
            content: self
                .content
                .into_iter()
                .map(|(term, def)| {
                    (
                        topdown_traverse_inlines(term, filter),
                        def.into_iter()
                            .map(|blocks| topdown_traverse_blocks(blocks, filter))
                            .collect(),
                    )
                })
                .collect(),
            ..self
        })
    }
}

impl BlockFilterableStructure for pandoc::Header {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        Block::Header(pandoc::Header {
            content: topdown_traverse_inlines(self.content, filter),
            ..self
        })
    }
}

impl BlockFilterableStructure for pandoc::Table {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        Block::Table(pandoc::Table {
            caption: traverse_caption(self.caption, filter),
            head: pandoc::TableHead {
                rows: self
                    .head
                    .rows
                    .into_iter()
                    .map(|row| traverse_row(row, filter))
                    .collect(),
                ..self.head
            },
            bodies: self
                .bodies
                .into_iter()
                .map(|body| pandoc::TableBody {
                    head: body
                        .head
                        .into_iter()
                        .map(|row| traverse_row(row, filter))
                        .collect(),
                    body: body
                        .body
                        .into_iter()
                        .map(|row| traverse_row(row, filter))
                        .collect(),
                    ..body
                })
                .collect(),
            foot: pandoc::TableFoot {
                rows: self
                    .foot
                    .rows
                    .into_iter()
                    .map(|row| traverse_row(row, filter))
                    .collect(),
                ..self.foot
            },
            ..self
        })
    }
}

impl BlockFilterableStructure for pandoc::Figure {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        Block::Figure(pandoc::Figure {
            caption: traverse_caption(self.caption, filter),
            content: topdown_traverse_blocks(self.content, filter),
            ..self
        })
    }
}

impl BlockFilterableStructure for Block {
    fn filter_structure(self, filter: &mut Filter) -> Block {
        traverse_block_structure(self, filter)
    }
}

macro_rules! inlines_apply_and_maybe_recurse {
    ($item:expr, $filter_fn:expr, $filter:expr) => {
        match $filter_fn($item) {
            FilterReturn::Unchanged(inline) => vec![inline.filter_structure($filter)],
            FilterReturn::FilterResult(new_content, recurse) => {
                if !recurse {
                    new_content
                } else {
                    topdown_traverse_inlines(new_content, $filter)
                }
            }
        }
    };
}

macro_rules! blocks_apply_and_maybe_recurse {
    ($item:expr, $filter_fn:expr, $filter:expr) => {
        match $filter_fn($item) {
            FilterReturn::Unchanged(block) => vec![block.filter_structure($filter)],
            FilterReturn::FilterResult(new_content, recurse) => {
                if !recurse {
                    new_content
                } else {
                    topdown_traverse_blocks(new_content, $filter)
                }
            }
        }
    };
}

pub fn topdown_traverse_inline(inline: Inline, filter: &mut Filter) -> Inlines {
    match inline {
        Inline::Str(s) => {
            handle_inline_filter!(Str, s, str, filter)
        }
        Inline::Emph(e) => {
            handle_inline_filter!(Emph, e, emph, filter)
        }
        Inline::Underline(u) => {
            handle_inline_filter!(Underline, u, underline, filter)
        }
        Inline::Strong(sg) => {
            handle_inline_filter!(Strong, sg, strong, filter)
        }
        Inline::Strikeout(st) => {
            handle_inline_filter!(Strikeout, st, strikeout, filter)
        }
        Inline::Superscript(sp) => {
            handle_inline_filter!(Superscript, sp, superscript, filter)
        }
        Inline::Subscript(sb) => {
            handle_inline_filter!(Subscript, sb, subscript, filter)
        }
        Inline::SmallCaps(sc) => {
            handle_inline_filter!(SmallCaps, sc, small_caps, filter)
        }
        Inline::Quoted(q) => {
            handle_inline_filter!(Quoted, q, quoted, filter)
        }
        Inline::Cite(c) => {
            handle_inline_filter!(Cite, c, cite, filter)
        }
        Inline::Code(co) => {
            handle_inline_filter!(Code, co, code, filter)
        }
        Inline::Space(sp) => {
            handle_inline_filter!(Space, sp, space, filter)
        }
        Inline::SoftBreak(sb) => {
            handle_inline_filter!(SoftBreak, sb, soft_break, filter)
        }
        Inline::LineBreak(lb) => {
            handle_inline_filter!(LineBreak, lb, line_break, filter)
        }
        Inline::Math(m) => {
            handle_inline_filter!(Math, m, math, filter)
        }
        Inline::RawInline(ri) => {
            handle_inline_filter!(RawInline, ri, raw_inline, filter)
        }
        Inline::Link(l) => {
            handle_inline_filter!(Link, l, link, filter)
        }
        Inline::Image(i) => {
            handle_inline_filter!(Image, i, image, filter)
        }
        Inline::Note(note) => {
            handle_inline_filter!(Note, note, note, filter)
        }
        Inline::Span(span) => {
            handle_inline_filter!(Span, span, span, filter)
        }
        // quarto extensions
        Inline::Shortcode(shortcode) => {
            handle_inline_filter!(Shortcode, shortcode, shortcode, filter)
        }
        Inline::NoteReference(note_ref) => {
            handle_inline_filter!(NoteReference, note_ref, note_reference, filter)
        }
        Inline::Attr(attr) => {
            handle_inline_filter!(Attr, attr, attr, filter)
        }
        Inline::Insert(ins) => {
            handle_inline_filter!(Insert, ins, insert, filter)
        }
        Inline::Delete(del) => {
            handle_inline_filter!(Delete, del, delete, filter)
        }
        Inline::Highlight(hl) => {
            handle_inline_filter!(Highlight, hl, highlight, filter)
        }
        Inline::EditComment(ec) => {
            handle_inline_filter!(EditComment, ec, edit_comment, filter)
        }
    }
}

pub fn topdown_traverse_block(block: Block, filter: &mut Filter) -> Blocks {
    match block {
        Block::Paragraph(para) => {
            handle_block_filter!(Paragraph, para, paragraph, filter)
        }
        Block::CodeBlock(code) => {
            handle_block_filter!(CodeBlock, code, code_block, filter)
        }
        Block::RawBlock(raw) => {
            handle_block_filter!(RawBlock, raw, raw_block, filter)
        }
        Block::BulletList(list) => {
            handle_block_filter!(BulletList, list, bullet_list, filter)
        }
        Block::OrderedList(list) => {
            handle_block_filter!(OrderedList, list, ordered_list, filter)
        }
        Block::BlockQuote(quote) => {
            handle_block_filter!(BlockQuote, quote, block_quote, filter)
        }
        Block::Div(div) => {
            handle_block_filter!(Div, div, div, filter)
        }
        Block::Figure(figure) => {
            handle_block_filter!(Figure, figure, figure, filter)
        }
        Block::Plain(plain) => {
            handle_block_filter!(Plain, plain, plain, filter)
        }
        Block::LineBlock(line_block) => {
            handle_block_filter!(LineBlock, line_block, line_block, filter)
        }
        Block::DefinitionList(def_list) => {
            handle_block_filter!(DefinitionList, def_list, definition_list, filter)
        }
        Block::Header(header) => {
            handle_block_filter!(Header, header, header, filter)
        }
        Block::Table(table) => {
            handle_block_filter!(Table, table, table, filter)
        }
        Block::HorizontalRule(hr) => {
            handle_block_filter!(HorizontalRule, hr, horizontal_rule, filter)
        }
        // quarto extensions
        Block::BlockMetadata(meta) => {
            if let Some(f) = &mut filter.meta {
                return match f(meta.meta) {
                    FilterReturn::Unchanged(m) => vec![Block::BlockMetadata(MetaBlock {
                        meta: m,
                        source_info: meta.source_info.clone(),
                    })],
                    FilterReturn::FilterResult(new_meta, recurse) => {
                        if !recurse {
                            vec![Block::BlockMetadata(MetaBlock {
                                meta: new_meta,
                                source_info: meta.source_info.clone(),
                            })]
                        } else {
                            vec![Block::BlockMetadata(MetaBlock {
                                meta: topdown_traverse_meta(new_meta, filter),
                                source_info: meta.source_info.clone(),
                            })]
                        }
                    }
                };
            }
            vec![Block::BlockMetadata(meta)]
        }
        Block::NoteDefinitionPara(refdef) => {
            // Process the inline content of the reference definition
            let content = topdown_traverse_inlines(refdef.content, filter);
            vec![Block::NoteDefinitionPara(
                crate::pandoc::block::NoteDefinitionPara {
                    id: refdef.id,
                    content,
                    source_info: refdef.source_info,
                },
            )]
        }
    }
}

pub fn topdown_traverse_inlines(vec: Inlines, filter: &mut Filter) -> Inlines {
    fn walk_vec(vec: Inlines, filter: &mut Filter) -> Inlines {
        let mut result = vec![];
        for inline in vec {
            result.extend(topdown_traverse_inline(inline, filter));
        }
        result
    }
    match &mut filter.inlines {
        None => walk_vec(vec, filter),
        Some(f) => match f(vec) {
            FilterReturn::Unchanged(inlines) => walk_vec(inlines, filter),
            FilterReturn::FilterResult(new_content, recurse) => {
                if !recurse {
                    return new_content;
                }
                walk_vec(new_content, filter)
            }
        },
    }
}

fn traverse_inline_nonterminal(inline: Inline, filter: &mut Filter) -> Inline {
    match inline {
        Inline::Emph(e) => Inline::Emph(crate::pandoc::Emph {
            content: topdown_traverse_inlines(e.content, filter),
            source_info: e.source_info,
        }),
        Inline::Underline(u) => Inline::Underline(crate::pandoc::Underline {
            content: topdown_traverse_inlines(u.content, filter),
            source_info: u.source_info,
        }),
        Inline::Strong(sg) => Inline::Strong(crate::pandoc::Strong {
            content: topdown_traverse_inlines(sg.content, filter),
            source_info: sg.source_info,
        }),
        Inline::Strikeout(st) => Inline::Strikeout(crate::pandoc::Strikeout {
            content: topdown_traverse_inlines(st.content, filter),
            source_info: st.source_info,
        }),
        Inline::Superscript(sp) => Inline::Superscript(crate::pandoc::Superscript {
            content: topdown_traverse_inlines(sp.content, filter),
            source_info: sp.source_info,
        }),
        Inline::Subscript(sb) => Inline::Subscript(crate::pandoc::Subscript {
            content: topdown_traverse_inlines(sb.content, filter),
            source_info: sb.source_info,
        }),
        Inline::SmallCaps(sc) => Inline::SmallCaps(crate::pandoc::SmallCaps {
            content: topdown_traverse_inlines(sc.content, filter),
            source_info: sc.source_info,
        }),
        Inline::Quoted(q) => Inline::Quoted(crate::pandoc::Quoted {
            quote_type: q.quote_type,
            content: topdown_traverse_inlines(q.content, filter),
            source_info: q.source_info,
        }),
        Inline::Cite(c) => Inline::Cite(crate::pandoc::Cite {
            citations: c
                .citations
                .into_iter()
                .map(|cit| crate::pandoc::Citation {
                    id: cit.id,
                    prefix: topdown_traverse_inlines(cit.prefix, filter),
                    suffix: topdown_traverse_inlines(cit.suffix, filter),
                    mode: cit.mode,
                    note_num: cit.note_num,
                    hash: cit.hash,
                })
                .collect(),
            content: topdown_traverse_inlines(c.content, filter),
            source_info: c.source_info,
        }),
        Inline::Link(l) => Inline::Link(crate::pandoc::Link {
            attr: l.attr,
            target: l.target,
            content: topdown_traverse_inlines(l.content, filter),
            source_info: l.source_info,
        }),
        Inline::Image(i) => Inline::Image(crate::pandoc::Image {
            attr: i.attr,
            target: i.target,
            content: topdown_traverse_inlines(i.content, filter),
            source_info: i.source_info,
        }),
        Inline::Note(note) => Inline::Note(crate::pandoc::Note {
            content: topdown_traverse_blocks(note.content, filter),
            source_info: note.source_info,
        }),
        Inline::Span(span) => Inline::Span(crate::pandoc::Span {
            attr: span.attr,
            content: topdown_traverse_inlines(span.content, filter),
            source_info: span.source_info,
        }),
        _ => panic!("Unsupported inline type: {:?}", inline),
    }
}

pub fn traverse_inline_structure(inline: Inline, filter: &mut Filter) -> Inline {
    match &inline {
        // terminal inline types
        Inline::Str(_) => inline,
        Inline::Code(_) => inline,
        Inline::Space(_) => inline,
        Inline::SoftBreak(_) => inline,
        Inline::LineBreak(_) => inline,
        Inline::Math(_) => inline,
        Inline::RawInline(_) => inline,
        // extensions
        Inline::Shortcode(_) => inline,
        Inline::NoteReference(_) => inline,
        Inline::Attr(_) => inline,
        _ => traverse_inline_nonterminal(inline, filter),
    }
}

fn traverse_blocks_vec_nonterminal(blocks_vec: Vec<Blocks>, filter: &mut Filter) -> Vec<Blocks> {
    blocks_vec
        .into_iter()
        .map(|blocks| topdown_traverse_blocks(blocks, filter))
        .collect()
}

fn traverse_caption(
    caption: crate::pandoc::Caption,
    filter: &mut Filter,
) -> crate::pandoc::Caption {
    crate::pandoc::Caption {
        short: caption
            .short
            .map(|short| topdown_traverse_inlines(short, filter)),
        long: caption
            .long
            .map(|long| topdown_traverse_blocks(long, filter)),
    }
}

fn traverse_row(row: crate::pandoc::Row, filter: &mut Filter) -> crate::pandoc::Row {
    crate::pandoc::Row {
        cells: row
            .cells
            .into_iter()
            .map(|cell| crate::pandoc::Cell {
                content: topdown_traverse_blocks(cell.content, filter),
                ..cell
            })
            .collect(),
        ..row
    }
}

fn traverse_rows(rows: Vec<crate::pandoc::Row>, filter: &mut Filter) -> Vec<crate::pandoc::Row> {
    rows.into_iter()
        .map(|row| traverse_row(row, filter))
        .collect()
}

fn traverse_block_nonterminal(block: Block, filter: &mut Filter) -> Block {
    match block {
        Block::Plain(plain) => Block::Plain(crate::pandoc::Plain {
            content: topdown_traverse_inlines(plain.content, filter),
            ..plain
        }),
        Block::Paragraph(para) => Block::Paragraph(crate::pandoc::Paragraph {
            content: topdown_traverse_inlines(para.content, filter),
            ..para
        }),
        Block::LineBlock(line_block) => Block::LineBlock(crate::pandoc::LineBlock {
            content: line_block
                .content
                .into_iter()
                .map(|line| topdown_traverse_inlines(line, filter))
                .collect(),
            ..line_block
        }),
        Block::BlockQuote(quote) => Block::BlockQuote(crate::pandoc::BlockQuote {
            content: topdown_traverse_blocks(quote.content, filter),
            ..quote
        }),
        Block::OrderedList(list) => Block::OrderedList(crate::pandoc::OrderedList {
            content: traverse_blocks_vec_nonterminal(list.content, filter),
            ..list
        }),
        Block::BulletList(list) => Block::BulletList(crate::pandoc::BulletList {
            content: traverse_blocks_vec_nonterminal(list.content, filter),
            ..list
        }),
        Block::DefinitionList(list) => Block::DefinitionList(crate::pandoc::DefinitionList {
            content: list
                .content
                .into_iter()
                .map(|(term, def)| {
                    (
                        topdown_traverse_inlines(term, filter),
                        traverse_blocks_vec_nonterminal(def, filter),
                    )
                })
                .collect(),
            ..list
        }),
        Block::Header(header) => Block::Header(crate::pandoc::Header {
            content: topdown_traverse_inlines(header.content, filter),
            ..header
        }),
        Block::Table(table) => Block::Table(crate::pandoc::Table {
            caption: traverse_caption(table.caption, filter),
            head: crate::pandoc::TableHead {
                rows: traverse_rows(table.head.rows, filter),
                ..table.head
            },
            bodies: table
                .bodies
                .into_iter()
                .map(|table_body| crate::pandoc::TableBody {
                    head: traverse_rows(table_body.head, filter),
                    body: traverse_rows(table_body.body, filter),
                    ..table_body
                })
                .collect(),
            foot: crate::pandoc::TableFoot {
                rows: traverse_rows(table.foot.rows, filter),
                ..table.foot
            },
            ..table
        }),
        Block::Figure(figure) => Block::Figure(crate::pandoc::Figure {
            caption: traverse_caption(figure.caption, filter),
            content: topdown_traverse_blocks(figure.content, filter),
            ..figure
        }),
        Block::Div(div) => Block::Div(crate::pandoc::Div {
            content: topdown_traverse_blocks(div.content, filter),
            ..div
        }),
        _ => {
            panic!("Unsupported block type: {:?}", block);
        }
    }
}
pub fn traverse_block_structure(block: Block, filter: &mut Filter) -> Block {
    match &block {
        // terminal block types
        Block::CodeBlock(_) => block,
        Block::RawBlock(_) => block,
        Block::HorizontalRule(_) => block,
        _ => traverse_block_nonterminal(block, filter),
    }
}

pub fn topdown_traverse_blocks(vec: Blocks, filter: &mut Filter) -> Blocks {
    fn walk_vec(vec: Blocks, filter: &mut Filter) -> Blocks {
        let mut result = vec![];
        for block in vec {
            result.extend(topdown_traverse_block(block, filter));
        }
        result
    }
    match &mut filter.blocks {
        None => walk_vec(vec, filter),
        Some(f) => match f(vec) {
            FilterReturn::Unchanged(blocks) => walk_vec(blocks, filter),
            FilterReturn::FilterResult(new_content, recurse) => {
                if !recurse {
                    return new_content;
                }
                walk_vec(new_content, filter)
            }
        },
    }
}

pub fn topdown_traverse_meta_value(value: MetaValue, filter: &mut Filter) -> MetaValue {
    match value {
        MetaValue::MetaMap(m) => MetaValue::MetaMap(
            m.into_iter()
                .map(|(k, v)| (k, topdown_traverse_meta_value(v, filter)))
                .collect(),
        ),
        MetaValue::MetaList(l) => MetaValue::MetaList(
            l.into_iter()
                .map(|mv| topdown_traverse_meta_value(mv, filter))
                .collect(),
        ),
        MetaValue::MetaBlocks(b) => MetaValue::MetaBlocks(topdown_traverse_blocks(b, filter)),
        MetaValue::MetaInlines(i) => MetaValue::MetaInlines(topdown_traverse_inlines(i, filter)),
        value => value,
    }
}

pub fn topdown_traverse_meta(meta: Meta, filter: &mut Filter) -> Meta {
    if let Some(f) = &mut filter.meta {
        return match f(meta) {
            FilterReturn::FilterResult(new_meta, recurse) => {
                if !recurse {
                    return new_meta;
                }
                topdown_traverse_meta(new_meta, filter)
            }
            FilterReturn::Unchanged(m) => {
                let meta_value = MetaValue::MetaMap(m);
                match topdown_traverse_meta_value(meta_value, filter) {
                    MetaValue::MetaMap(m) => m,
                    _ => panic!("Expected MetaMap after filtering meta"),
                }
            }
        };
    } else {
        return meta
            .into_iter()
            .map(|(k, v)| (k, topdown_traverse_meta_value(v, filter)))
            .collect();
    }
}

pub fn topdown_traverse(doc: pandoc::Pandoc, filter: &mut Filter) -> pandoc::Pandoc {
    pandoc::Pandoc {
        meta: topdown_traverse_meta(doc.meta, filter),
        blocks: topdown_traverse_blocks(doc.blocks, filter),
    }
}

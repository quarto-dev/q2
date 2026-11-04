/*
 * dashboard/structure.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Build the dashboard's typed structure from a sectioned document.
//!
//! The port of Quarto 1's dashboard Lua (`quarto-post/dashboard.lua`):
//!
//! - **Cards** (Q1 pass 1, [`build_cards`]): code cells and `.card` divs
//!   become `DashboardCard` nodes ([`make_card`] = Q1 `card.lua`
//!   `makeCard` / `resolveCardBodies`).
//! - **Layout** (Q1 pass 2, [`build_layout`]): the sections
//!   `SectionizeTransform` produced become pages (level 1) and row/column
//!   containers (deeper levels). Loose content is gathered into flow cards
//!   (Q1's `organizer`), and content before the first section stays
//!   outside the grid ("leading content").
//!
//! One deliberate change (dashboard plan D5-Q3): a container's orientation
//! is the flip of its parent container's, not a global toggle flipped on
//! every change of heading level.

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Blocks, Div, Header, Plain};
use quarto_pandoc_types::inline::{Inline, Inlines, Str};
use quarto_source_map::SourceInfo;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

use super::model::{
    BodyData, CARD_TYPE, CONTAINER_TYPE, Card, CardData, Container, ContainerData, Layout,
    Orientation, Page, PageData, as_dashboard_node,
};

/// Document-level options (`format: dashboard:` keys, flattened).
#[derive(Debug, Clone)]
pub struct DashboardOptions {
    pub orientation: Orientation,
    pub scrolling: bool,
    pub expandable: bool,
}

impl Default for DashboardOptions {
    fn default() -> Self {
        DashboardOptions {
            orientation: Orientation::Rows,
            scrolling: false,
            expandable: true,
        }
    }
}

/// Classes the organizer leaves where they are (Q1 `kIgnoreWhenOrganizingClz`).
const IGNORE_WHEN_ORGANIZING: &[&str] = &["section", "hidden"];

fn generated() -> SourceInfo {
    SourceInfo::generated(quarto_source_map::By::unknown())
}

fn has_class(attr: &Attr, class: &str) -> bool {
    attr.1.iter().any(|c| c == class)
}

fn div(attr: Attr, content: Blocks) -> Block {
    Block::Div(Div {
        attr,
        content,
        source_info: generated(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn class_attr(classes: &[&str]) -> Attr {
    (
        String::new(),
        classes.iter().map(|c| c.to_string()).collect(),
        LinkedHashMap::new(),
    )
}

fn as_div(block: &Block) -> Option<&Div> {
    match block {
        Block::Div(d) => Some(d),
        _ => None,
    }
}

fn is_card_div(block: &Block) -> bool {
    as_div(block).is_some_and(|d| has_class(&d.attr, "card"))
}

fn is_card_node(block: &Block) -> bool {
    as_dashboard_node(block, CARD_TYPE).is_some()
}

fn is_container_node(block: &Block) -> bool {
    as_dashboard_node(block, CONTAINER_TYPE).is_some()
}

/// Q1 `isLayoutContainer`: what the organizer does not wrap in a loose card.
/// Value boxes, sidebars, toolbars and card inputs join with Phases 5–6.
fn is_layout_container(block: &Block) -> bool {
    if is_card_node(block) || is_container_node(block) || is_card_div(block) {
        return true;
    }
    as_div(block).is_some_and(|d| {
        [
            "rows",
            "columns",
            "valuebox",
            "sidebar",
            "toolbar",
            "tabset",
            "card-toolbar",
            "card-sidebar",
        ]
        .iter()
        .any(|c| has_class(&d.attr, c))
    })
}

fn is_ignored_when_organizing(block: &Block) -> bool {
    let attr = match block {
        Block::Div(d) => &d.attr,
        Block::Header(h) => &h.attr,
        _ => return false,
    };
    IGNORE_WHEN_ORGANIZING.iter().any(|c| has_class(attr, c))
}

// ---------------------------------------------------------------------------
// Cards (Q1 pass 1)
// ---------------------------------------------------------------------------

/// Q1 `card.readOptions`: options from a cell or `.card` div's classes and
/// attributes, plus the user classes (`expandable` removed).
#[derive(Debug, Clone, Default)]
pub(crate) struct CardOptions {
    pub title: Option<(String, SourceInfo)>,
    pub padding: Option<String>,
    pub height: Option<String>,
    pub width: Option<String>,
    pub min_height: Option<String>,
    pub max_height: Option<String>,
    pub expandable: Option<String>,
    pub fill: Option<bool>,
}

pub(crate) fn read_card_options(attr: &Attr, source: &SourceInfo) -> (CardOptions, Vec<String>) {
    let mut o = CardOptions::default();
    if has_class(attr, "expandable") {
        o.expandable = Some("true".to_string());
    }
    if has_class(attr, "flow") {
        o.fill = Some(false);
    } else if has_class(attr, "fill") {
        o.fill = Some(true);
    }
    let get = |k: &str| attr.2.get(k).cloned();
    if let Some(t) = get("title") {
        o.title = Some((t, source.clone()));
    }
    o.padding = get("padding");
    o.height = get("height");
    o.width = get("width");
    o.min_height = get("min-height");
    o.max_height = get("max-height");
    if let Some(e) = get("expandable") {
        o.expandable = Some(e);
    }
    if let Some(f) = get("fill") {
        o.fill = Some(f != "false");
    }
    let classes = attr
        .1
        .iter()
        .filter(|c| c.as_str() != "expandable")
        .cloned()
        .collect();
    (o, classes)
}

/// Q1 `hasRealLookingContent`: something other than raw blocks and knitr
/// `preserve<hex>` placeholders (or nothing at all).
fn has_real_looking_content(blocks: &[Block]) -> bool {
    if blocks.is_empty() {
        return true;
    }
    blocks.iter().any(|b| match b {
        Block::RawBlock(_) => false,
        Block::Paragraph(p) => !is_preserve_placeholder(&p.content),
        _ => true,
    })
}

fn is_preserve_placeholder(inlines: &[Inline]) -> bool {
    let text = crate::editorial_marks::blocks_to_inlines(&[Block::Plain(Plain {
        content: inlines.to_vec(),
        source_info: generated(),
    })]);
    let s: String = text
        .iter()
        .map(|i| match i {
            Inline::Str(s) => s.text.as_str(),
            _ => "",
        })
        .collect();
    s.len() >= 24 && s.starts_with("preserve") && s[8..24].chars().all(|c| c.is_ascii_hexdigit())
}

/// Q1 `processCardBodyContent`: a paragraph holding a single inline becomes
/// `Plain` (so an image sits directly in its output div), recursively; a
/// heading becomes a `Div.h<N>` with its level offset.
fn process_body_content(block: Block, heading_offset: usize) -> Block {
    match block {
        Block::Header(h) => header_div(h, heading_offset),
        other => unwrap_single_inline_paras(other),
    }
}

fn header_div(h: Header, heading_offset: usize) -> Block {
    let level = (h.level + heading_offset).min(6);
    div(
        class_attr(&[&format!("h{level}")]),
        vec![Block::Plain(Plain {
            content: h.content,
            source_info: h.source_info,
        })],
    )
}

fn unwrap_single_inline_paras(block: Block) -> Block {
    match block {
        Block::Paragraph(p) if p.content.len() == 1 => Block::Plain(Plain {
            content: p.content,
            source_info: p.source_info,
        }),
        Block::Div(mut d) => {
            d.content = d
                .content
                .into_iter()
                .map(unwrap_single_inline_paras)
                .collect();
            Block::Div(d)
        }
        Block::BlockQuote(mut q) => {
            q.content = q
                .content
                .into_iter()
                .map(unwrap_single_inline_paras)
                .collect();
            Block::BlockQuote(q)
        }
        other => other,
    }
}

fn min_heading_level(blocks: &[Block]) -> Option<usize> {
    let mut min: Option<usize> = None;
    fn visit(blocks: &[Block], min: &mut Option<usize>) {
        for b in blocks {
            match b {
                Block::Header(h) => *min = Some(min.map_or(h.level, |m| m.min(h.level))),
                Block::Div(d) => visit(&d.content, min),
                Block::BlockQuote(q) => visit(&q.content, min),
                _ => {}
            }
        }
    }
    visit(blocks, &mut min);
    min
}

const BODY_ATTRIBUTES: &[&str] = &["title", "height", "min-height", "max-height"];

/// Move Q1's body options (`title`, heights) out of a Div's attributes.
fn take_body_options(attr: &mut Attr, body: &mut BodyData) {
    for key in BODY_ATTRIBUTES {
        if let Some(v) = attr.2.remove(*key) {
            match *key {
                "title" => body.title = Some(v),
                "height" => body.height = Some(v),
                "min-height" => body.min_height = Some(v),
                _ => body.max_height = Some(v),
            }
        }
    }
}

struct ResolvedBodies {
    bodies: Vec<(BodyData, Blocks)>,
    header: Blocks,
    footer: Blocks,
}

/// Q1 `resolveCardBodies`.
fn resolve_card_bodies(contents: Blocks) -> ResolvedBodies {
    let offset = min_heading_level(&contents).map_or(0, |base| 4usize.saturating_sub(base));
    let mut out = ResolvedBodies {
        bodies: Vec::new(),
        header: Vec::new(),
        footer: Vec::new(),
    };
    let mut collected: Blocks = Vec::new();

    let flush = |collected: &mut Blocks, out: &mut ResolvedBodies| {
        if collected.is_empty() {
            return;
        }
        let mut body = BodyData {
            classes: vec!["card-body".to_string()],
            ..Default::default()
        };
        if let Some(Block::Div(first)) = collected.first_mut() {
            take_body_options(&mut first.attr, &mut body);
        }
        if has_real_looking_content(collected) {
            out.bodies.push((body, std::mem::take(collected)));
        } else if let Some((_, prev)) = out.bodies.last_mut() {
            prev.append(collected);
        }
    };

    for block in contents {
        match block {
            Block::Div(d) if has_class(&d.attr, "card") => {
                flush(&mut collected, &mut out);
                let mut inner = d.content;
                let mut title = None;
                if let Some(Block::Div(h)) = inner.first()
                    && has_class(&h.attr, "card-header")
                {
                    title = Some(stringify_blocks(&h.content));
                    inner.remove(0);
                }
                let mut nested = resolve_card_bodies(inner);
                if let (Some(t), Some((first, _))) = (title, nested.bodies.first_mut()) {
                    first.title = Some(t);
                }
                out.bodies.append(&mut nested.bodies);
                out.footer.append(&mut nested.footer);
                out.header.append(&mut nested.header);
            }
            Block::Div(mut d) if has_class(&d.attr, "card-body") || has_class(&d.attr, "cell") => {
                flush(&mut collected, &mut out);
                let mut body = BodyData::default();
                take_body_options(&mut d.attr, &mut body);
                let mut classes = d.attr.1.clone();
                if !classes.iter().any(|c| c == "card-body") {
                    classes.push("card-body".to_string());
                }
                body.classes = classes;
                body.id = d.attr.0.clone();
                body.attributes = d
                    .attr
                    .2
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let content = d
                    .content
                    .into_iter()
                    .map(|b| process_body_content(b, offset))
                    .collect();
                out.bodies.push((body, content));
            }
            Block::BlockQuote(q) => out.footer.extend(q.content),
            Block::Div(d) if has_class(&d.attr, "card-footer") => out.footer.extend(d.content),
            Block::Div(d) if has_class(&d.attr, "card-header") => out.header.extend(d.content),
            other => collected.push(process_body_content(other, offset)),
        }
    }
    flush(&mut collected, &mut out);
    out
}

fn stringify_blocks(blocks: &[Block]) -> String {
    let inlines = crate::editorial_marks::blocks_to_inlines(blocks);
    stringify_inlines(&inlines)
}

fn stringify_inlines(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Str(t) => s.push_str(&t.text),
            Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_) => s.push(' '),
            Inline::Emph(e) => s.push_str(&stringify_inlines(&e.content)),
            Inline::Strong(e) => s.push_str(&stringify_inlines(&e.content)),
            Inline::Code(c) => s.push_str(&c.text),
            Inline::Span(sp) => s.push_str(&stringify_inlines(&sp.content)),
            Inline::Link(l) => s.push_str(&stringify_inlines(&l.content)),
            _ => {}
        }
    }
    s
}

/// The parsing context a card title string needs (Q1
/// `string_to_quarto_ast_inlines`).
fn title_inlines(title: &str, source: &SourceInfo) -> Inlines {
    let mut diagnostics = Vec::new();
    match pampa::pandoc::meta::parse_config_string_as_markdown(title, source, &mut diagnostics) {
        quarto_pandoc_types::ConfigValueKind::PandocInlines(inlines) => inlines,
        quarto_pandoc_types::ConfigValueKind::PandocBlocks(blocks) => {
            crate::editorial_marks::blocks_to_inlines(&blocks)
        }
        _ => vec![Inline::Str(Str {
            text: title.to_string(),
            source_info: source.clone(),
        })],
    }
}

/// Q1 `card.makeCard`. `None` when the content is only raw output.
pub(crate) fn make_card(
    contents: Blocks,
    classes: Vec<String>,
    options: CardOptions,
    doc: &DashboardOptions,
    source_info: SourceInfo,
) -> Option<Block> {
    if !has_real_looking_content(&contents) {
        return None;
    }
    let resolved = resolve_card_bodies(contents);
    let mut card_classes = vec!["card".to_string()];
    for c in classes {
        if !card_classes.contains(&c) {
            card_classes.push(c);
        }
    }
    let is_valuebox = card_classes.iter().any(|c| c == "valuebox");
    // Q1's default (`format-dashboard-card.ts`, the `data-expandable`
    // handler): off for value boxes, else the document's `expandable`.
    let expandable = match options.expandable.as_deref() {
        Some(v) => v != "false",
        None => !is_valuebox && doc.expandable,
    };
    let (mut bodies_data, bodies): (Vec<BodyData>, Vec<Blocks>) =
        resolved.bodies.into_iter().unzip();
    for (data, content) in bodies_data.iter_mut().zip(&bodies) {
        data.flow = data.fill == Some(false) || shiny_inputs_only(content);
    }
    let card = Card {
        data: CardData {
            classes: card_classes,
            padding: options.padding,
            height: options.height,
            width: options.width,
            min_height: options.min_height,
            max_height: options.max_height,
            fill: options.fill,
            expandable,
            bodies: bodies_data,
            ..Default::default()
        },
        title: options.title.map(|(t, si)| title_inlines(&t, &si)),
        header: resolved.header,
        bodies,
        footer: resolved.footer,
    };
    Some(card.to_block(source_info))
}

/// Q1 `shinyInputs` (`format-dashboard-card.ts`): every element child of
/// the body is a `.cell-output` whose first element child is a
/// `.shiny-input-container`. Vacuously true for a body without element
/// children — a text-only body (a `Plain` of words) — which Q1 therefore
/// lays out as flow.
fn shiny_inputs_only(content: &[Block]) -> bool {
    content.iter().all(|block| match block {
        Block::Plain(p) => p.content.iter().all(|i| {
            matches!(
                i,
                Inline::Str(_) | Inline::Space(_) | Inline::SoftBreak(_)
            )
        }),
        Block::Div(d) => {
            has_class(&d.attr, "cell-output")
                && matches!(d.content.first(), Some(Block::Div(f)) if has_class(&f.attr, "shiny-input-container"))
        }
        _ => false,
    })
}

/// Q1 pass 1: turn code cells and `.card` divs into cards, everywhere.
pub fn build_cards(
    blocks: Blocks,
    doc: &DashboardOptions,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Blocks {
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        let Block::Div(mut d) = block else {
            out.push(block);
            continue;
        };
        if has_class(&d.attr, "cell") && has_class(&d.attr, "markdown") {
            // Notebook markdown cells unwrap (Q1 #9056).
            out.extend(build_cards(d.content, doc, diagnostics));
        } else if d.attr.2.get("output").is_some_and(|v| v == "asis") {
            d.content = build_cards(d.content, doc, diagnostics);
            out.push(Block::Div(d));
        } else if has_class(&d.attr, "card") {
            if is_literal_card(&d) {
                out.push(literal_card(d, doc));
                continue;
            }
            let (options, classes) = read_card_options(&d.attr, &d.source_info);
            let source = d.source_info.clone();
            match make_card(d.content.clone(), classes, options, doc, source) {
                Some(card) => out.push(card),
                None => out.push(Block::Div(d)),
            }
        } else if is_value_box(&d.attr) {
            out.push(make_value_box(d, doc, diagnostics));
        } else if has_class(&d.attr, "cell") {
            out.extend(cell_to_card(d, doc));
        } else {
            d.content = build_cards(d.content, doc, diagnostics);
            out.push(Block::Div(d));
        }
    }
    out
}

/// Q1 `isValueBox`: the `.valuebox` class, or `content` / `component` set
/// to `valuebox` (a value-box cell).
fn is_value_box(attr: &Attr) -> bool {
    has_class(attr, "valuebox") || is_value_box_content(attr)
}

fn is_value_box_content(attr: &Attr) -> bool {
    ["content", "component"]
        .iter()
        .any(|k| attr.2.get(*k).is_some_and(|v| v == "valuebox"))
}

fn plain(inlines: Inlines) -> Block {
    Block::Plain(Plain {
        content: inlines,
        source_info: generated(),
    })
}

fn text_inlines(text: &str) -> Inlines {
    vec![Inline::Str(Str {
        text: text.to_string(),
        source_info: generated(),
    })]
}

/// Q1 `valuebox.makeValueBox` (see [`super::valuebox`]).
fn make_value_box(
    el: Div,
    doc: &DashboardOptions,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Block {
    let mut title: Option<Blocks> = el
        .attr
        .2
        .get("title")
        .map(|t| vec![plain(title_inlines(t, &el.source_info))]);
    let mut options: LinkedHashMap<String, String> = LinkedHashMap::new();
    let value: Blocks;
    let mut extra: Blocks = Vec::new();

    if is_value_box_content(&el.attr) {
        let mut printed = None;
        find_printed_output(&el.content, &mut printed);
        match printed {
            Some(text) => match super::valuebox::parse_printed(&text) {
                Ok(p) => {
                    if let Some(t) = p.options.get("title") {
                        title = Some(vec![plain(text_inlines(t))]);
                    }
                    value = match p.value {
                        Some(v) => vec![plain(text_inlines(&v))],
                        None => el.content.clone(),
                    };
                    options = p.options;
                }
                Err(problem) => {
                    diagnostics.push(
                        DiagnosticMessageBuilder::warning(
                            "value box output could not be read",
                        )
                        .with_code("Q-27-1")
                        .with_location(el.source_info.clone())
                        .problem(format!(
                            "This `content: valuebox` cell printed something that looks like a \
                             Python dict or an R list, but it could not be read: {problem}."
                        ))
                        .add_hint(
                            "Print a dict such as `dict(value = 42, icon = \"chat\")` (Python) or \
                             a list such as `list(value = 42, icon = \"chat\")` (R).",
                        )
                        .build(),
                    );
                    value = vec![plain(text_inlines(&text))];
                }
            },
            None => value = el.content.clone(),
        }
    } else {
        let mut pending = el.content.clone();
        if title.is_none() && pending.len() > 1 {
            title = Some(vec![pending.remove(0)]);
        }
        value = if pending.is_empty() {
            Vec::new()
        } else {
            vec![pending.remove(0)]
        };
        extra = pending;
    }

    let mut vb_attr: Attr = (String::new(), Vec::new(), LinkedHashMap::new());
    for key in ["color", "bg-color", "fg-color"] {
        if let Some(v) = el.attr.2.get(key) {
            vb_attr.2.insert(format!("data-{key}"), v.clone());
        }
    }
    for (k, v) in &options {
        vb_attr.2.insert(format!("data-{k}"), v.clone());
    }
    let mut vb_content = Vec::new();
    let mut showcase: LinkedHashMap<String, String> = LinkedHashMap::new();
    // D5-Q11/13: `showcase` is accepted as an alias of Q1's
    // `showcase-position`, which Q1's own fixtures use but Q1 never reads.
    for (key, out) in [
        ("icon", "data-icon"),
        ("showcase-position", "data-showcase-position"),
        ("showcase", "data-showcase-position"),
    ] {
        if let Some(v) = el.attr.2.get(key).or_else(|| options.get(key))
            && !showcase.contains_key(out)
        {
            showcase.insert(out.to_string(), v.clone());
        }
    }
    if !showcase.is_empty() {
        vb_content.push(div(
            (
                String::new(),
                vec!["value-box-showcase".to_string()],
                showcase,
            ),
            Vec::new(),
        ));
    }
    let title_blocks = title.unwrap_or_else(|| vec![plain(text_inlines("\u{a0}"))]);
    let mut area = vec![
        div(class_attr(&["value-box-title"]), title_blocks),
        div(class_attr(&["value-box-value"]), value),
    ];
    area.extend(extra);
    vb_content.push(div(class_attr(&["value-box-area"]), area));
    let vb = div(vb_attr, vb_content);

    let mut classes = vec!["valuebox".to_string()];
    classes.extend(el.attr.1.iter().cloned());
    make_card(
        vec![vb],
        classes,
        CardOptions::default(),
        doc,
        el.source_info,
    )
    .unwrap_or_else(|| {
        Block::Div(Div {
            attr: class_attr(&["valuebox"]),
            content: Vec::new(),
            source_info: generated(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Q1 `valueboxContent`: the text of a value-box cell's printed output (a
/// code block in a `cell-output-stdout` or `cell-output-display`; the last
/// one wins, as Q1's walk overwrites it).
fn find_printed_output(blocks: &[Block], found: &mut Option<String>) {
    for b in blocks {
        if let Block::Div(d) = b {
            if (has_class(&d.attr, "cell-output-stdout")
                || has_class(&d.attr, "cell-output-display"))
                && let Some(cb) = first_code_block(&d.content)
            {
                *found = Some(cb.text.clone());
            }
            find_printed_output(&d.content, found);
        }
    }
}

/// Q1 `isLiteralCard`: a `.card` whose first two children are an explicit
/// `.card-header` and `.card-body`, left exactly as written.
fn is_literal_card(d: &Div) -> bool {
    let first = d.content.first().and_then(as_div);
    let second = d.content.get(1).and_then(as_div);
    first.is_some_and(|h| has_class(&h.attr, "card-header"))
        && second.is_some_and(|b| has_class(&b.attr, "card-body"))
}

/// A literal card as a card node. Q1's Lua leaves it untouched (no body
/// regrouping, no paragraph unwrapping), but its DOM pass still makes it a
/// bslib card, so it lowers like any other: header from `.card-header`,
/// one body per `.card-body`, footer from `.card-footer`. Anything else
/// joins the body before it.
fn literal_card(d: Div, doc: &DashboardOptions) -> Block {
    let source = d.source_info.clone();
    let classes = d.attr.1.clone();
    let mut header = Vec::new();
    let mut bodies: Vec<(BodyData, Blocks)> = Vec::new();
    let mut footer = Vec::new();
    for child in d.content {
        match child {
            Block::Div(c) if has_class(&c.attr, "card-header") => header.extend(c.content),
            Block::Div(c) if has_class(&c.attr, "card-footer") => footer.extend(c.content),
            Block::Div(mut c) if has_class(&c.attr, "card-body") => {
                let mut body = BodyData::default();
                take_body_options(&mut c.attr, &mut body);
                body.id = c.attr.0.clone();
                body.classes = c.attr.1.clone();
                body.attributes = c
                    .attr
                    .2
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                body.flow = shiny_inputs_only(&c.content);
                bodies.push((body, c.content));
            }
            other => match bodies.last_mut() {
                Some((_, content)) => content.push(other),
                None => header.push(other),
            },
        }
    }
    let (bodies_data, bodies): (Vec<BodyData>, Vec<Blocks>) = bodies.into_iter().unzip();
    let mut card_classes = vec!["card".to_string()];
    for c in classes {
        if !card_classes.contains(&c) {
            card_classes.push(c);
        }
    }
    Card {
        data: CardData {
            classes: card_classes,
            expandable: doc.expandable,
            bodies: bodies_data,
            ..Default::default()
        },
        title: None,
        header,
        bodies,
        footer,
    }
    .to_block(source)
}

/// A code cell's card (Q1 pass 1, the `cell` branch).
fn cell_to_card(mut cell: Div, doc: &DashboardOptions) -> Vec<Block> {
    // An output-less cell makes nothing (Q1 #9796; observed in the
    // corpus goldens).
    if cell.content.is_empty() {
        return Vec::new();
    }
    // Q1's hidden check walks the cell itself as well as its descendants
    // (`_quarto.ast.walk` applies the filter to the root element too).
    let mut hidden = has_class(&cell.attr, "hidden");
    let mut markdown_output = false;
    let mut lifted: Blocks = Vec::new();
    lift_and_inspect(
        &mut cell.content,
        &mut hidden,
        &mut markdown_output,
        &mut lifted,
    );

    if !lifted.is_empty() {
        // Engine-emitted bslib components pass through raw; the rest of the
        // cell still becomes a card (Q1 #15023).
        let mut result = lifted;
        if !cell.content.is_empty() {
            let (options, classes) = read_card_options(&cell.attr, &cell.source_info);
            let source = cell.source_info.clone();
            if let Some(card) = make_card(cell.content, classes, options, doc, source) {
                result.push(card);
            }
        }
        return result;
    }
    if hidden {
        if !has_class(&cell.attr, "hidden") {
            cell.attr.1.push("hidden".to_string());
        }
        return vec![Block::Div(cell)];
    }

    let (mut options, classes) = read_card_options(&cell.attr, &cell.source_info);
    if markdown_output && options.fill.is_none() {
        options.fill = Some(false);
    }
    let mut content = cell.content;
    // Dynamic title: a first stdout output that starts with `title=`.
    if content.len() > 1
        && let Some(Block::Div(first)) = content.first()
        && has_class(&first.attr, "cell-output-stdout")
        && let Some(cb) = first_code_block(&first.content)
        && cb.text.len() > "title=".len()
        && let Some(rest) = cb.text.strip_prefix("title=")
    {
        options.title = Some((rest.trim().to_string(), cb.source_info.clone()));
        content.remove(0);
    }
    let source = cell.source_info.clone();
    make_card(content, classes, options, doc, source)
        .into_iter()
        .collect()
}

/// The first code block of an output, looking through the wrapper Divs
/// later transforms add (the code-copy scaffold).
fn first_code_block(blocks: &[Block]) -> Option<&quarto_pandoc_types::block::CodeBlock> {
    match blocks.first()? {
        Block::CodeBlock(cb) => Some(cb),
        Block::Div(d) => first_code_block(&d.content),
        _ => None,
    }
}

/// Walk a cell's Divs: note hidden descendants and markdown outputs, and
/// lift `cell-output-display` raw HTML that carries bslib components.
fn lift_and_inspect(
    blocks: &mut Blocks,
    hidden: &mut bool,
    markdown: &mut bool,
    lifted: &mut Blocks,
) {
    let mut i = 0;
    while i < blocks.len() {
        if let Block::Div(d) = &mut blocks[i] {
            *hidden |= has_class(&d.attr, "hidden");
            if has_class(&d.attr, "cell-output-display") {
                *markdown |= has_class(&d.attr, "cell-output-markdown");
                if let [Block::RawBlock(raw)] = d.content.as_slice()
                    && raw.format == "html"
                    && raw.text.contains("bslib-")
                {
                    lifted.push(Block::RawBlock(raw.clone()));
                    blocks.remove(i);
                    continue;
                }
            }
            lift_and_inspect(&mut d.content, hidden, markdown, lifted);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// Layout (Q1 pass 2)
// ---------------------------------------------------------------------------

/// What the layout pass learned about pages, for the navbar.
#[derive(Debug, Default)]
pub struct LayoutInfo {
    pub pages: Vec<PageData>,
}

/// Q1 `organizer.ensureInLayoutContainers`: wrap runs of loose content in
/// flow cards.
fn organize(contents: Blocks, doc: &DashboardOptions) -> Blocks {
    let mut out = Vec::new();
    let mut loose: Blocks = Vec::new();
    let flush = |loose: &mut Blocks, out: &mut Blocks| {
        if loose.is_empty() {
            return;
        }
        let first = loose.first().cloned();
        let (mut options, _) = match &first {
            Some(Block::Div(d)) => read_card_options(&d.attr, &d.source_info),
            _ => (CardOptions::default(), Vec::new()),
        };
        options.fill = Some(false);
        let taken = std::mem::take(loose);
        match make_card(taken.clone(), Vec::new(), options, doc, generated()) {
            Some(card) => out.push(card),
            None => out.extend(taken),
        }
    };
    for block in contents {
        if !is_layout_container(&block) && !is_ignored_when_organizing(&block) {
            loose.push(block);
        } else {
            flush(&mut loose, &mut out);
            out.push(block);
        }
    }
    flush(&mut loose, &mut out);
    out
}

fn is_section(block: &Block) -> bool {
    as_div(block).is_some_and(|d| has_class(&d.attr, "section"))
}

/// Orientation forced by a direct child sidebar or toolbar (Q1
/// `suggestOrientation`).
fn suggested_orientation(children: &[Block]) -> Option<Orientation> {
    let header_has = |b: &Block, class: &str| {
        as_div(b).is_some_and(|d| {
            has_class(&d.attr, class)
                || matches!(d.content.first(), Some(Block::Header(h)) if has_class(&h.attr, class))
        })
    };
    if children.iter().any(|b| header_has(b, "sidebar")) {
        Some(Orientation::Columns)
    } else if children.iter().any(|b| header_has(b, "toolbar")) {
        Some(Orientation::Rows)
    } else {
        None
    }
}

/// Q1 pass 2 over the whole document. Returns the new top-level blocks
/// (leading content, then the root container) and the page list.
pub fn build_layout(blocks: Blocks, doc: &DashboardOptions) -> (Blocks, LayoutInfo) {
    let mut leading = Vec::new();
    let mut rest = Vec::new();
    let mut seen = false;
    for block in blocks {
        if seen || is_section(&block) || is_card_node(&block) {
            seen = true;
            rest.push(block);
        } else {
            leading.push(block);
        }
    }
    let (mut out, layout_els) = if rest.is_empty() {
        (Vec::new(), leading)
    } else {
        (leading, rest)
    };

    let mut info = LayoutInfo::default();
    let orientation = suggested_orientation(&layout_els).unwrap_or(doc.orientation);
    let children = organize(layout_els, doc);
    let children = children
        .into_iter()
        .flat_map(|b| convert_section(b, orientation, doc, &mut info))
        .collect();
    let root = Container {
        data: ContainerData {
            orientation,
            id: String::new(),
            classes: Vec::new(),
            layout: Some(if doc.scrolling {
                Layout::Flow
            } else {
                Layout::Fill
            }),
            height: None,
            width: None,
            root: true,
            grid_template: String::new(),
        },
        children,
    };
    out.push(root.to_block());
    (out, info)
}

/// Convert one block of a container's children: a section becomes a page or
/// container (recursively); anything else is kept.
fn convert_section(
    block: Block,
    parent: Orientation,
    doc: &DashboardOptions,
    info: &mut LayoutInfo,
) -> Vec<Block> {
    let Block::Div(section) = block else {
        return vec![block];
    };
    if let Some(orientation) = explicit_orientation(&section.attr) {
        return vec![explicit_container(section, orientation, doc, info)];
    }
    if !has_class(&section.attr, "section") {
        return vec![Block::Div(section)];
    }
    let mut content = section.content;
    let Some(Block::Header(header)) = content.first().cloned() else {
        return vec![div(section.attr, content)];
    };
    content.remove(0);

    if has_class(&header.attr, "sidebar") || has_class(&header.attr, "toolbar") {
        // Sidebars and toolbars arrive with Phase 6; until then the
        // section's content stays in place, without its heading.
        return vec![div(section.attr, content)];
    }

    if header.level == 1 {
        let orientation = page_orientation(&header.attr, doc);
        let orientation = suggested_orientation(&content).unwrap_or(orientation);
        let children = organize(content, doc)
            .into_iter()
            .flat_map(|b| convert_section(b, orientation, doc, info))
            .collect();
        let index = info.pages.len() + 1;
        let id = if section.attr.0.is_empty() {
            format!("dashboard-page-{index}")
        } else {
            section.attr.0.clone()
        };
        let data = PageData {
            id,
            title: stringify_inlines(&header.content),
            orientation,
            scrolling: header.attr.2.get("scrolling").cloned(),
            index,
        };
        info.pages.push(data.clone());
        let container = Container {
            data: ContainerData {
                orientation,
                id: String::new(),
                classes: Vec::new(),
                layout: None,
                height: None,
                width: None,
                root: false,
                grid_template: String::new(),
            },
            children,
        };
        return vec![
            Page {
                data,
                content: vec![container.to_block()],
            }
            .to_block(),
        ];
    }

    if has_class(&header.attr, "tabset") {
        return make_tabset(header, content, parent, doc, info);
    }

    let orientation = suggested_orientation(&content).unwrap_or(parent.flip());
    let children = organize(content, doc)
        .into_iter()
        .flat_map(|b| convert_section(b, orientation, doc, info))
        .collect();
    let classes: Vec<String> = header
        .attr
        .1
        .iter()
        .filter(|c| c.as_str() != "rows" && c.as_str() != "columns")
        .cloned()
        .collect();
    let layout = if has_class(&header.attr, "fill") {
        Some(Layout::Fill)
    } else if has_class(&header.attr, "flow") {
        Some(Layout::Flow)
    } else {
        None
    };
    vec![
        Container {
            data: ContainerData {
                orientation,
                id: String::new(),
                classes,
                layout,
                height: header.attr.2.get("height").cloned(),
                width: header.attr.2.get("width").cloned(),
                root: false,
                grid_template: String::new(),
            },
            children,
        }
        .to_block(),
    ]
}

/// Q1 `tabset.makeTabset` / `resolveTabs`: a `.tabset` heading becomes a
/// tabset card built from the section's raw contents (not organized into
/// loose cards). Each card becomes a tab titled by its title, holding all
/// its body contents (its footer joins the tabset's); each subsection a
/// tab titled by its heading (`title=` attribute first), laid out as a
/// container; anything else a tab of its own. Hidden cells follow the card.
///
/// Subsections flip the orientation of the container holding the tabset
/// (the tabset itself is a card, not a container).
fn make_tabset(
    header: Header,
    contents: Blocks,
    parent: Orientation,
    doc: &DashboardOptions,
    info: &mut LayoutInfo,
) -> Vec<Block> {
    let mut tabs: Vec<(BodyData, Blocks)> = Vec::new();
    let mut footer: Blocks = Vec::new();
    let mut hidden: Blocks = Vec::new();
    for v in contents {
        let mut body = BodyData {
            classes: vec!["card-body".to_string()],
            ..Default::default()
        };
        let content =
            if let Some(card) = as_dashboard_node(&v, CARD_TYPE).and_then(Card::from_custom) {
                body.title = card.title.as_deref().map(stringify_inlines);
                footer.extend(card.footer);
                card.bodies.into_iter().flatten().collect()
            } else if as_div(&v).is_some_and(|d| has_class(&d.attr, "hidden")) {
                hidden.push(v);
                continue;
            } else if let Some(Block::Header(h)) = as_div(&v)
                .filter(|d| d.content.len() > 1)
                .and_then(|d| d.content.first())
            {
                body.title = Some(
                    h.attr
                        .2
                        .get("title")
                        .cloned()
                        .unwrap_or_else(|| stringify_inlines(&h.content)),
                );
                convert_section(v, parent, doc, info)
            } else {
                vec![v]
            };
        body.flow = shiny_inputs_only(&content);
        tabs.push((body, content));
    }
    let (bodies_data, bodies): (Vec<BodyData>, Vec<Blocks>) = tabs.into_iter().unzip();
    let mut classes = vec!["card".to_string(), "tabset".to_string()];
    for c in header.attr.1.iter().filter(|c| c.as_str() != "tabset") {
        if !classes.contains(c) {
            classes.push(c.clone());
        }
    }
    let title = header
        .attr
        .2
        .get("title")
        .map(|t| title_inlines(t, &header.source_info));
    let card = Card {
        data: CardData {
            classes,
            expandable: doc.expandable,
            bodies: bodies_data,
            tabset: true,
            ..Default::default()
        },
        title,
        header: Vec::new(),
        bodies,
        footer,
    };
    let mut out = vec![card.to_block(header.source_info.clone())];
    out.extend(hidden);
    out
}

/// An explicit `::: {.rows}` / `::: {.columns}` div: Q1 treats any div
/// with those classes as a layout container (`layout.isRowOrColumnContainer`)
/// and grid-sizes it like a heading container. Its orientation is its
/// class, and its children are not organized into loose cards.
fn explicit_orientation(attr: &Attr) -> Option<Orientation> {
    if has_class(attr, "rows") {
        Some(Orientation::Rows)
    } else if has_class(attr, "columns") {
        Some(Orientation::Columns)
    } else {
        None
    }
}

fn explicit_container(
    d: Div,
    orientation: Orientation,
    doc: &DashboardOptions,
    info: &mut LayoutInfo,
) -> Block {
    let children = d
        .content
        .into_iter()
        .flat_map(|b| convert_section(b, orientation, doc, info))
        .collect();
    Container {
        data: ContainerData {
            orientation,
            id: d.attr.0.clone(),
            classes: d
                .attr
                .1
                .iter()
                .filter(|c| c.as_str() != "rows" && c.as_str() != "columns")
                .cloned()
                .collect(),
            layout: None,
            height: None,
            width: None,
            root: false,
            grid_template: String::new(),
        },
        children,
    }
    .to_block()
}

/// Q1 `page.readOptions`: `orientation=` or a `.rows` / `.columns` class,
/// else the document's.
fn page_orientation(attr: &Attr, doc: &DashboardOptions) -> Orientation {
    let mut o = attr
        .2
        .get("orientation")
        .and_then(|v| Orientation::parse(v))
        .unwrap_or(doc.orientation);
    if has_class(attr, "columns") {
        o = Orientation::Columns;
    } else if has_class(attr, "rows") {
        o = Orientation::Rows;
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::model::{CONTAINER_TYPE, Card, Container, PAGE_TYPE, Page};

    fn parse(body: &str) -> Blocks {
        let mut sink = Vec::new();
        let (ast, _, _) =
            pampa::readers::qmd::read(body.as_bytes(), false, "t.qmd", &mut sink, true, None)
                .expect("parses");
        ast.blocks
    }

    fn build(body: &str, doc: &DashboardOptions) -> (Blocks, LayoutInfo) {
        let blocks = build_cards(parse(body), doc, &mut Vec::new());
        build_layout(pampa::transforms::sectionize_blocks(blocks), doc)
    }

    fn root(blocks: &[Block]) -> Container {
        let node = blocks
            .iter()
            .find_map(|b| as_dashboard_node(b, CONTAINER_TYPE))
            .expect("a root container");
        Container::from_custom(node).unwrap()
    }

    fn containers(c: &Container) -> Vec<Container> {
        c.children
            .iter()
            .filter_map(|b| as_dashboard_node(b, CONTAINER_TYPE))
            .filter_map(Container::from_custom)
            .collect()
    }

    fn cards(blocks: &[Block]) -> Vec<Card> {
        blocks
            .iter()
            .filter_map(|b| as_dashboard_node(b, CARD_TYPE))
            .filter_map(Card::from_custom)
            .collect()
    }

    const CELL: &str = "::: {.cell}\n\n::: {.cell-output-display}\n\nout\n\n:::\n\n:::\n\n";

    /// D5-Q3: a container flips its parent's orientation, whatever the
    /// heading levels skipped or jumped back over.
    #[test]
    fn orientation_follows_nesting_not_heading_levels() {
        let body = format!("## A\n\n#### B\n\n{CELL}## C\n\n{CELL}");
        let (blocks, _) = build(&body, &DashboardOptions::default());
        let root = root(&blocks);
        assert_eq!(root.data.orientation, Orientation::Rows);
        let level2 = containers(&root);
        assert_eq!(level2.len(), 2);
        for c in &level2 {
            assert_eq!(
                c.data.orientation,
                Orientation::Columns,
                "every ## is a row"
            );
        }
        assert_eq!(
            containers(&level2[0])[0].data.orientation,
            Orientation::Rows
        );
    }

    #[test]
    fn pages_take_their_own_orientation() {
        let body = format!("# P1\n\n{CELL}# P2 {{orientation=\"columns\"}}\n\n## X\n\n{CELL}");
        let (blocks, info) = build(&body, &DashboardOptions::default());
        assert_eq!(info.pages.len(), 2);
        assert_eq!(info.pages[1].orientation, Orientation::Columns);
        let pages: Vec<Page> = root(&blocks)
            .children
            .iter()
            .filter_map(|b| as_dashboard_node(b, PAGE_TYPE))
            .filter_map(Page::from_custom)
            .collect();
        let inner = Container::from_custom(
            as_dashboard_node(&pages[1].content[0], CONTAINER_TYPE).unwrap(),
        )
        .unwrap();
        assert_eq!(inner.data.orientation, Orientation::Columns);
        assert_eq!(containers(&inner)[0].data.orientation, Orientation::Rows);
    }

    /// Q1 #9796: an output-less cell makes no card.
    #[test]
    fn empty_cell_makes_nothing() {
        let blocks = build_cards(
            parse("::: {.cell}\n:::\n"),
            &DashboardOptions::default(),
            &mut Vec::new(),
        );
        assert!(blocks.is_empty(), "{blocks:?}");
    }

    /// Q1's `shinyInputs` test holds vacuously for a text-only body, so it
    /// flows; a body holding a paragraph element does not.
    #[test]
    fn text_only_bodies_flow() {
        let blocks = build_cards(
            parse("::: {.card}\nWord.\n:::\n\n::: {.card}\nTwo words.\n:::\n"),
            &DashboardOptions::default(),
            &mut Vec::new(),
        );
        let cards = cards(&blocks);
        assert!(
            cards[0].data.bodies[0].flow,
            "single inline → Plain → text only"
        );
        assert!(
            !cards[1].data.bodies[0].flow,
            "a paragraph is an element child"
        );
    }

    #[test]
    fn explicit_rows_and_columns_divs_are_containers() {
        let body = format!("## R\n\n::: {{#mine .columns}}\n\n{CELL}{CELL}:::\n");
        let (blocks, _) = build(&body, &DashboardOptions::default());
        let row = &containers(&root(&blocks))[0];
        let explicit = &containers(row)[0];
        assert_eq!(explicit.data.orientation, Orientation::Columns);
        assert_eq!(explicit.data.id, "mine");
        assert_eq!(cards(&explicit.children).len(), 2);
    }

    #[test]
    fn leading_content_stays_outside_the_grid() {
        let body = format!("Lead paragraph here.\n\n## R\n\n{CELL}");
        let (blocks, _) = build(&body, &DashboardOptions::default());
        assert!(matches!(blocks[0], Block::Paragraph(_)), "{:?}", blocks[0]);
        assert!(as_dashboard_node(&blocks[1], CONTAINER_TYPE).is_some());
    }

    #[test]
    fn tabset_tabs_from_cards_and_subsections() {
        let body = format!(
            "## T {{.tabset title=\"Tabs\"}}\n\n::: {{.card title=\"First\"}}\nOne two.\n\n> foot\n:::\n\n::: {{.cell .hidden}}\nx\n:::\n\n### Second\n\n{CELL}"
        );
        let (blocks, _) = build(&body, &DashboardOptions::default());
        let root = root(&blocks);
        let tabset = cards(&root.children).remove(0);
        assert!(tabset.data.tabset);
        assert_eq!(tabset.data.classes, ["card", "tabset"]);
        let titles: Vec<_> = tabset.data.bodies.iter().map(|b| b.title.clone()).collect();
        assert_eq!(
            titles,
            [Some("First".to_string()), Some("Second".to_string())]
        );
        assert_eq!(
            tabset.footer.len(),
            1,
            "the card footer joins the tabset footer"
        );
        // The subsection is laid out as a container that flips the root.
        let inner = Container::from_custom(
            as_dashboard_node(&tabset.bodies[1][0], CONTAINER_TYPE).expect("a container"),
        )
        .unwrap();
        assert_eq!(inner.data.orientation, Orientation::Columns);
        // The hidden cell follows the tabset card.
        assert!(matches!(&root.children[1], Block::Div(d) if has_class(&d.attr, "hidden")));
    }

    #[test]
    fn expandable_defaults() {
        let doc = DashboardOptions {
            expandable: false,
            ..Default::default()
        };
        let blocks = build_cards(
            parse("::: {.card}\nA b.\n:::\n\n::: {.card expandable=\"true\"}\nA b.\n:::\n"),
            &doc,
            &mut Vec::new(),
        );
        let cards = cards(&blocks);
        assert!(!cards[0].data.expandable);
        assert!(cards[1].data.expandable, "an explicit option wins");
    }
}

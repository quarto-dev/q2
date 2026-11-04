/*
 * dashboard/resolve.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Lower dashboard nodes to bslib markup: the port of Quarto 1's DOM
//! postprocessor (`format-dashboard.ts` and `format-dashboard-{layout,card,
//! page}.ts`), applied to the AST instead of the finished HTML.
//!
//! Fill classes follow Q1's `shouldApplyClasses`. Q1 counts text nodes as
//! "fill children", and the HTML writer separates blocks with newlines, so
//! in practice every `<div>` without a never-fill class gets both
//! `html-fill-item` and `html-fill-container` wherever Q1 recurses: the
//! root container's top level (item only), each page, each card body.

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Blocks, Div, Plain, RawBlock};
use quarto_pandoc_types::inline::{Inline, Span};
use quarto_source_map::SourceInfo;

use super::model::{CARD_TYPE, CONTAINER_TYPE, Card, Container, Orientation, PAGE_TYPE, Page};

/// Q1 `kNeverFillClasses`, plus `card-title`, which Q1 creates only after
/// its fill passes have run.
const NEVER_FILL: &[&str] = &[
    "value-box-grid",
    "value-box-area",
    "value-box-title",
    "value-box-value",
    "nav-tabs",
    "card-header",
    "card-footer",
    "callout",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "input-panel",
    "toolbar",
    "flow",
    "card-title",
];

/// Q1 `kFillContentElements.classes`: non-`div` elements that still fill.
const FILL_CLASSES: &[&str] = &[
    "card",
    "card-body",
    "tab-pane",
    "tab-content",
    "tabset",
    "bslib-grid",
    "bslib-grid-item",
    "sidebar-content",
    "main",
    "bslib-sidebar-layout",
    "cell-output-display",
    "quarto-float",
];

/// Q1 `kFillDontRecurseInsideClasses`.
const DONT_RECURSE: &[&str] = &["sidebar", "toolbar"];

const EXPAND_BUTTON: &str = r#"<bslib-tooltip placement="auto" bsoptions="[]" data-require-bs-version="5" data-require-bs-caller="tooltip()">
    <template>Expand</template>
    <span class="bslib-full-screen-enter badge rounded-pill">
        <svg xmlns="http://www.w3.org/2000/svg" viewbox="0 0 24 24" style="height:1em;width:1em;" aria-hidden="true" role="img"><path d="M20 5C20 4.4 19.6 4 19 4H13C12.4 4 12 3.6 12 3C12 2.4 12.4 2 13 2H21C21.6 2 22 2.4 22 3V11C22 11.6 21.6 12 21 12C20.4 12 20 11.6 20 11V5ZM4 19C4 19.6 4.4 20 5 20H11C11.6 20 12 20.4 12 21C12 21.6 11.6 22 11 22H3C2.4 22 2 21.6 2 21V13C2 12.4 2.4 12 3 12C3.6 12 4 12.4 4 13V19Z"></path></svg>
    </span>
</bslib-tooltip>"#;

const CARD_INIT_SCRIPT: &str =
    r#"<script data-bslib-card-init="">bslib.Card.initializeAllCards();</script>"#;

fn generated() -> SourceInfo {
    SourceInfo::generated(quarto_source_map::By::unknown())
}

fn raw_html(text: &str) -> Block {
    Block::RawBlock(RawBlock {
        format: "html".to_string(),
        text: text.to_string(),
        source_info: generated(),
    })
}

fn div(attr: Attr, content: Blocks) -> Block {
    Block::Div(Div {
        attr,
        content,
        source_info: generated(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn add_class(attr: &mut Attr, class: &str) {
    if !attr.1.iter().any(|c| c == class) {
        attr.1.push(class.to_string());
    }
}

fn has_class(attr: &Attr, class: &str) -> bool {
    attr.1.iter().any(|c| c == class)
}

/// Q1 `ensureCssUnits`: a bare number is pixels.
fn css_units(v: &str) -> String {
    if v.chars().last().is_some_and(|c| c.is_ascii_digit()) {
        format!("{v}px")
    } else {
        v.to_string()
    }
}

fn append_style(attr: &mut Attr, decl: String) {
    match attr.2.get_mut("style") {
        Some(existing) if !existing.is_empty() => {
            existing.push(' ');
            existing.push_str(&decl);
        }
        _ => {
            attr.2.insert("style".to_string(), decl);
        }
    }
}

// ---------------------------------------------------------------------------
// Fill classes
// ---------------------------------------------------------------------------

fn never_fill(attr: &Attr) -> bool {
    NEVER_FILL.iter().any(|c| has_class(attr, c))
}

fn fill_by_class(attr: &Attr) -> bool {
    FILL_CLASSES.iter().any(|c| has_class(attr, c))
}

/// Q1 `applyFillItemClasses` for one element.
fn apply_fill_item(block: &mut Block) {
    match block {
        Block::Div(d) if !never_fill(&d.attr) => add_class(&mut d.attr, "html-fill-item"),
        Block::Figure(f) if !never_fill(&f.attr) && fill_by_class(&f.attr) => {
            add_class(&mut f.attr, "html-fill-item")
        }
        _ => {}
    }
}

/// Q1 `recursiveApplyFillClasses`.
pub fn apply_fill_recursive(block: &mut Block) {
    match block {
        Block::Div(d) => {
            if !never_fill(&d.attr) {
                add_class(&mut d.attr, "html-fill-item");
                add_class(&mut d.attr, "html-fill-container");
            }
            if !DONT_RECURSE.iter().any(|c| has_class(&d.attr, c)) {
                for child in d.content.iter_mut() {
                    apply_fill_recursive(child);
                }
            }
        }
        Block::Figure(f) => {
            if !never_fill(&f.attr) && fill_by_class(&f.attr) {
                add_class(&mut f.attr, "html-fill-item");
                add_class(&mut f.attr, "html-fill-container");
            }
            for child in f.content.iter_mut() {
                apply_fill_recursive(child);
            }
        }
        Block::BlockQuote(q) => {
            for child in q.content.iter_mut() {
                apply_fill_recursive(child);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/// Q1's image fill (`format-dashboard.ts`): images directly in a
/// `cell-output-display`, or in its `quarto-figure > quarto-float`, get
/// `quarto-dashboard-img-contain` and lose their width and height.
fn contain_images(blocks: &mut Blocks) {
    for block in blocks.iter_mut() {
        let Block::Div(d) = block else { continue };
        if has_class(&d.attr, "cell-output-display") {
            for child in d.content.iter_mut() {
                match child {
                    Block::Plain(p) => {
                        for inline in p.content.iter_mut() {
                            contain_image(inline);
                        }
                    }
                    Block::Div(fig) if has_class(&fig.attr, "quarto-figure") => {
                        for b in fig.content.iter_mut() {
                            if let Block::Figure(f) = b
                                && has_class(&f.attr, "quarto-float")
                            {
                                crate::ast_walk::for_each_inline_mut(&mut f.content, &mut |i| {
                                    contain_image(i)
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        contain_images(&mut d.content);
    }
}

fn contain_image(inline: &mut Inline) {
    if let Inline::Image(img) = inline {
        add_class(&mut img.attr, "quarto-dashboard-img-contain");
        img.attr.2.remove("width");
        img.attr.2.remove("height");
    }
}

// ---------------------------------------------------------------------------
// Lowering
// ---------------------------------------------------------------------------

/// Lower every dashboard node under `blocks` (the document's top level) to
/// bslib markup.
pub fn lower_document(blocks: Blocks) -> Blocks {
    let has_pages = contains_page(&blocks);
    blocks
        .into_iter()
        .map(|block| {
            let is_root = matches!(&block, Block::Custom(n) if n.type_name == CONTAINER_TYPE);
            let mut lowered = lower(block);
            if is_root
                && has_pages
                && let Block::Div(d) = &mut lowered
            {
                add_class(&mut d.attr, "tab-content");
            }
            // Q1: every element at the content area's top level is a grid
            // item (bar paragraphs and scripts), with the fill-item class.
            match &mut lowered {
                Block::Div(d) => add_class(&mut d.attr, "bslib-grid-item"),
                Block::Header(h) => add_class(&mut h.attr, "bslib-grid-item"),
                _ => {}
            }
            apply_fill_item(&mut lowered);
            lowered
        })
        .collect()
}

fn contains_page(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| match b {
        Block::Custom(n) if n.type_name == PAGE_TYPE => true,
        Block::Custom(n) => n.slots.values().any(|s| match s {
            quarto_pandoc_types::custom::Slot::Blocks(b) => contains_page(b),
            _ => false,
        }),
        _ => false,
    })
}

fn lower_all(blocks: Blocks) -> Blocks {
    blocks.into_iter().map(lower).collect()
}

fn lower(block: Block) -> Block {
    let Block::Custom(node) = &block else {
        return match block {
            Block::Div(mut d) => {
                d.content = lower_all(d.content);
                Block::Div(d)
            }
            other => other,
        };
    };
    match node.type_name.as_str() {
        CONTAINER_TYPE => Container::from_custom(node).map(lower_container),
        PAGE_TYPE => Page::from_custom(node).map(lower_page),
        CARD_TYPE => Card::from_custom(node).map(lower_card),
        _ => None,
    }
    .unwrap_or(block)
}

fn lower_container(c: Container) -> Block {
    let mut attr: Attr = (
        c.data.id.clone(),
        vec!["bslib-grid".to_string()],
        LinkedHashMap::new(),
    );
    for class in &c.data.classes {
        add_class(&mut attr, class);
    }
    if let Some(layout) = c.data.layout {
        attr.2
            .insert("data-layout".to_string(), layout.as_str().to_string());
    }
    let style = match c.data.orientation {
        Orientation::Rows => format!(
            "display: grid; grid-template-rows: {}; grid-auto-columns: minmax(0, 1fr);",
            c.data.grid_template
        ),
        Orientation::Columns => format!(
            "display: grid; grid-template-columns: {};\ngrid-auto-rows: minmax(0, 1fr);",
            c.data.grid_template
        ),
    };
    attr.2.insert("style".to_string(), style);
    div(attr, lower_all(c.children))
}

fn lower_page(p: Page) -> Block {
    let mut attr: Attr = (
        p.data.id.clone(),
        vec!["dashboard-page".to_string(), "tab-pane".to_string()],
        LinkedHashMap::new(),
    );
    if p.data.index == 1 {
        add_class(&mut attr, "show");
        add_class(&mut attr, "active");
    } else {
        add_class(&mut attr, "grid-skip");
    }
    attr.2.insert(
        "data-orientation".to_string(),
        p.data.orientation.as_str().to_string(),
    );
    if let Some(s) = &p.data.scrolling {
        attr.2.insert("data-scrolling".to_string(), s.clone());
    }
    attr.2
        .insert("aria-labelledby".to_string(), format!("tab-{}", p.data.id));
    let mut page = div(attr, lower_all(p.content));
    apply_fill_recursive(&mut page);
    page
}

/// Split a header's loose text (`Plain` blocks whose inlines are text,
/// emphasis, strong, code or inline math, which HTML renders directly into
/// the header element) from the rest.
fn take_loose_header_text(
    header: Blocks,
) -> (Option<quarto_pandoc_types::inline::Inlines>, Blocks) {
    let loose = |p: &Plain| {
        !p.content.is_empty()
            && p.content.iter().all(|i| {
                matches!(
                    i,
                    Inline::Str(_)
                        | Inline::Space(_)
                        | Inline::SoftBreak(_)
                        | Inline::Emph(_)
                        | Inline::Strong(_)
                        | Inline::Code(_)
                        | Inline::Math(_)
                )
            })
    };
    let mut title = Vec::new();
    let mut rest = Vec::new();
    for block in header {
        match block {
            Block::Plain(p) if loose(&p) => {
                if !title.is_empty() {
                    title.push(Inline::Space(quarto_pandoc_types::inline::Space {
                        source_info: generated(),
                    }));
                }
                title.extend(p.content);
            }
            other => rest.push(other),
        }
    }
    ((!title.is_empty()).then_some(title), rest)
}

fn lower_card(card: Card) -> Block {
    let data = card.data;
    let mut attr: Attr = (String::new(), data.classes.clone(), LinkedHashMap::new());
    add_class(&mut attr, "bslib-card");
    add_class(&mut attr, "html-fill-container");
    attr.2
        .insert("data-bslib-card-init".to_string(), String::new());
    attr.2
        .insert("data-require-bs-caller".to_string(), "card()".to_string());
    if data.expandable {
        attr.2
            .insert("data-full-screen".to_string(), "false".to_string());
    }
    for (prop, value) in [
        ("max-height", &data.max_height),
        ("min-height", &data.min_height),
        ("height", &data.height),
    ] {
        if let Some(v) = value {
            append_style(&mut attr, format!("{prop}: {};", css_units(v)));
        }
    }

    // Q1 `processCards`: without an explicit title, the header's loose
    // text (words, emphasis, strong, code, inline math) becomes it.
    let (title, header) = match card.title {
        Some(t) => (Some(t), card.header),
        None => take_loose_header_text(card.header),
    };
    let mut content: Blocks = Vec::new();
    if title.is_some() || !header.is_empty() {
        let mut header_attr: Attr = (
            String::new(),
            vec!["card-header".to_string()],
            LinkedHashMap::new(),
        );
        let mut header_content = Vec::new();
        match title {
            Some(title) if !title.is_empty() => {
                let span = Inline::Span(Span {
                    attr: (
                        String::new(),
                        Vec::new(),
                        LinkedHashMap::from_iter([(
                            "style".to_string(),
                            "display: inline".to_string(),
                        )]),
                    ),
                    content: title,
                    source_info: generated(),
                    attr_source: AttrSourceInfo::empty(),
                });
                header_content.push(div(
                    (
                        String::new(),
                        vec!["card-title".to_string()],
                        LinkedHashMap::new(),
                    ),
                    vec![Block::Plain(Plain {
                        content: vec![span],
                        source_info: generated(),
                    })],
                ));
            }
            _ => add_class(&mut header_attr, "dashboard-card-no-title"),
        }
        header_content.extend(lower_all(header));
        content.push(div(header_attr, header_content));
    }

    for (body_data, body) in data.bodies.iter().zip(card.bodies) {
        let mut body_attr: Attr = (
            body_data.id.clone(),
            body_data.classes.clone(),
            body_data.attributes.iter().cloned().collect(),
        );
        add_class(&mut body_attr, "card-body");
        if body_data.flow {
            add_class(&mut body_attr, "flow");
        }
        for (prop, value) in [
            ("max-height", &body_data.max_height),
            ("min-height", &body_data.min_height),
            ("height", &body_data.height),
        ] {
            if let Some(v) = value {
                append_style(&mut body_attr, format!("{prop}: {};", css_units(v)));
            }
        }
        if let Some(p) = &data.padding {
            append_style(&mut body_attr, format!("padding: {};", css_units(p)));
        }
        let mut body_content = lower_all(body);
        contain_images(&mut body_content);
        let mut body_div = div(body_attr, body_content);
        apply_fill_recursive(&mut body_div);
        content.push(body_div);
    }

    if !card.footer.is_empty() {
        content.push(div(
            (
                String::new(),
                vec!["card-footer".to_string()],
                LinkedHashMap::new(),
            ),
            lower_all(card.footer),
        ));
    }
    if data.expandable {
        content.push(raw_html(EXPAND_BUTTON));
    }
    content.push(raw_html(CARD_INIT_SCRIPT));
    div(attr, content)
}

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

/// Q1 `ensureCssUnits`: a bare number is pixels, except `0`.
fn css_units(v: &str) -> String {
    if v != "0" && v.chars().last().is_some_and(|c| c.is_ascii_digit()) {
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
    let mut cx = Lower::default();
    blocks
        .into_iter()
        .map(|block| {
            let is_root = matches!(&block, Block::Custom(n) if n.type_name == CONTAINER_TYPE);
            let mut lowered = lower(block, &mut cx);
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

/// State threaded through one document's lowering: Q1 numbers tabsets by
/// the card's position among all `.card` elements in document order
/// (`card-tabset-<n>`).
#[derive(Default)]
struct Lower {
    cards: usize,
    /// The orientation of the container whose children are being lowered
    /// (`None` inside pages, card bodies and other non-grid content).
    parent: Option<Orientation>,
}

fn lower_all(blocks: Blocks, cx: &mut Lower) -> Blocks {
    blocks.into_iter().map(|b| lower(b, cx)).collect()
}

fn lower(block: Block, cx: &mut Lower) -> Block {
    let Block::Custom(node) = &block else {
        return match block {
            Block::Div(d) if explicit_grid(&d.attr).is_some() => lower_explicit_grid(d, cx),
            Block::Div(mut d) => {
                let parent = cx.parent.take();
                d.content = lower_all(d.content, cx);
                cx.parent = parent;
                Block::Div(d)
            }
            other => other,
        };
    };
    match node.type_name.as_str() {
        CONTAINER_TYPE => Container::from_custom(node).map(|c| lower_container(c, cx)),
        PAGE_TYPE => Page::from_custom(node).map(|p| lower_page(p, cx)),
        CARD_TYPE => Card::from_custom(node).map(|c| lower_card(c, cx)),
        _ => None,
    }
    .unwrap_or(block)
}

fn lower_container(c: Container, cx: &mut Lower) -> Block {
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
    let parent = cx.parent.replace(c.data.orientation);
    let children = lower_all(c.children, cx);
    cx.parent = parent;
    div(attr, children)
}

/// Q1 grid-sizes every `div.rows` / `div.columns` in the page
/// (`processRows` / `processColumns` query the whole document), including
/// ones the structure pass did not turn into containers, e.g. inside a
/// card body.
fn explicit_grid(attr: &Attr) -> Option<Orientation> {
    if has_class(attr, "rows") {
        Some(Orientation::Rows)
    } else if has_class(attr, "columns") {
        Some(Orientation::Columns)
    } else {
        None
    }
}

fn lower_explicit_grid(d: Div, cx: &mut Lower) -> Block {
    let orientation = explicit_grid(&d.attr).expect("checked by the caller");
    let mut container = Container {
        data: super::model::ContainerData {
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
        children: d.content,
    };
    container.data.grid_template = super::sizing::grid_template(&container);
    lower_container(container, cx)
}

fn lower_page(p: Page, cx: &mut Lower) -> Block {
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
    let parent = cx.parent.take();
    let mut page = div(attr, lower_all(p.content, cx));
    cx.parent = parent;
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

/// Q1 `processValueBoxes` (`format-dashboard-valuebox.ts`), run after the
/// body's fill pass as in Q1 (so the value-box grid keeps its fill
/// classes): bslib classes, the color (`data-color`, else `data-bg-color` /
/// `data-fg-color`, else `bg-secondary`; a `#` color is a style), the
/// showcase icon and position, and the title and value as paragraphs.
///
/// The title and value become `<p class=…>` around their inlines (raw tags
/// around a `Plain`), never a paragraph inside a paragraph: Q1 nests the
/// title's `<p>` in another, which browsers split, emptying the title slot
/// (dashboard plan D5-Q14).
fn finish_value_box(card_attr: &mut Attr, body: &mut Block) {
    add_class(card_attr, "bslib-value-box");
    let Block::Div(body) = body else { return };
    let Some(Block::Div(vb)) = body.content.iter_mut().find(|b| matches!(b, Block::Div(_))) else {
        return;
    };
    add_class(&mut vb.attr, "value-box-grid");

    let mut colored = false;
    if let Some(c) = vb.attr.2.remove("data-color") {
        color(card_attr, &c, true);
        colored = true;
    }
    if !colored {
        if let Some(c) = vb.attr.2.remove("data-bg-color") {
            color(card_attr, &c, true);
            colored = true;
        }
        if let Some(c) = vb.attr.2.remove("data-fg-color") {
            color(card_attr, &c, false);
            colored = true;
        }
    }
    vb.attr.2.remove("data-bg-color");
    vb.attr.2.remove("data-fg-color");
    if !colored {
        color(card_attr, "secondary", true);
    }

    for block in vb.content.iter_mut() {
        let Block::Div(d) = block else { continue };
        if has_class(&d.attr, "value-box-showcase") {
            if let Some(icon) = d.attr.2.remove("data-icon") {
                d.content.push(raw_html(&format!(
                    r#"<i class="bi bi-{}"></i>"#,
                    html_escape(&icon)
                )));
            }
            let position = d
                .attr
                .2
                .remove("data-showcase-position")
                .unwrap_or_else(|| "left-center".to_string());
            add_class(card_attr, &format!("showcase-{position}"));
        } else if has_class(&d.attr, "value-box-area") {
            d.content = std::mem::take(&mut d.content)
                .into_iter()
                .flat_map(paragraph_for_value_part)
                .collect();
        }
    }
    vb.attr.2.retain(|k, _| !k.starts_with("data-"));
}

/// Q1 `colorEl`: a `#` color is an inline style, a name a `bg-` / `text-`
/// class.
fn color(attr: &mut Attr, color: &str, background: bool) {
    if color.starts_with('#') {
        let prop = if background { "background" } else { "color" };
        append_style(attr, format!("{prop}: {color};"));
    } else {
        let prefix = if background { "bg-" } else { "text-" };
        add_class(attr, &format!("{prefix}{color}"));
    }
}

/// A `.value-box-title` / `.value-box-value` Div as `<p class=…>` around
/// its inlines. Content that is not inline (a cell's display output used
/// as the value) stays a Div.
fn paragraph_for_value_part(block: Block) -> Vec<Block> {
    let Block::Div(d) = &block else {
        return vec![block];
    };
    let Some(class) = ["value-box-title", "value-box-value"]
        .into_iter()
        .find(|c| has_class(&d.attr, c))
    else {
        return vec![block];
    };
    let mut inlines = Vec::new();
    for b in &d.content {
        match b {
            Block::Plain(p) => inlines.extend(p.content.iter().cloned()),
            Block::Paragraph(p) => inlines.extend(p.content.iter().cloned()),
            // A heading as the title (`### Title` first in a `.valuebox`):
            // its text, rather than Q1's heading inside the paragraph.
            Block::Header(h) => inlines.extend(h.content.iter().cloned()),
            _ => return vec![block],
        }
    }
    vec![
        raw_html(&format!(r#"<p class="{class}">"#)),
        Block::Plain(Plain {
            content: inlines,
            source_info: generated(),
        }),
        raw_html("</p>"),
    ]
}

/// Q1 `convertToTabsetHeader`: the tab list in a tabset card's header.
fn tab_nav_html(id: &str, bodies: &[super::model::BodyData]) -> String {
    let mut html = format!(
        r#"<ul class="nav nav-tabs card-header-tabs" role="tablist" data-tabsetid="{id}">"#
    );
    for (i, body) in bodies.iter().enumerate() {
        let n = i + 1;
        let title = body.title.clone().unwrap_or_else(|| format!("Tab {n}"));
        let title = html_escape(&title);
        let (active, selected) = if n == 1 {
            (" active", "true")
        } else {
            ("", "false")
        };
        html.push_str(&format!(
            r##"<li class="nav-item" role="presentation"><a href="#{id}-{n}" role="tab" data-toggle="tab" data-bs-toggle="tab" data-value="{title}" aria-selected="{selected}" class="nav-link{active}">{title}</a></li>"##
        ));
    }
    html.push_str("</ul>");
    html
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn lower_card(card: Card, cx: &mut Lower) -> Block {
    cx.cards += 1;
    // A card's height sizes its row when its parent is a rows grid, and Q1
    // then drops it (`computeRowLayouts` removes `data-height` before the
    // card pass would turn it into a style).
    let height_is_style = cx.parent != Some(Orientation::Rows);
    let parent = cx.parent.take();
    let tabset_id = card
        .data
        .tabset
        .then(|| format!("card-tabset-{}", cx.cards));
    let data = card.data;
    let is_valuebox = data.classes.iter().any(|c| c == "valuebox");
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
        if let Some(v) = value
            && (prop != "height" || height_is_style)
        {
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
    if title.is_some() || !header.is_empty() || tabset_id.is_some() {
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
        header_content.extend(lower_all(header, cx));
        if let Some(id) = &tabset_id {
            add_class(&mut header_attr, "bslib-navs-card-title");
            header_content.push(raw_html(&tab_nav_html(id, &data.bodies)));
        }
        content.push(div(header_attr, header_content));
    }

    let mut panes = Vec::new();
    for (index, (body_data, body)) in data.bodies.iter().zip(card.bodies).enumerate() {
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
        let mut body_content = lower_all(body, cx);
        contain_images(&mut body_content);
        let mut body_div = div(body_attr, body_content);
        match &tabset_id {
            // Q1 `convertToTabs`: each body in its own tab pane.
            Some(id) => {
                let mut pane_attr: Attr = (
                    format!("{id}-{}", index + 1),
                    vec!["tab-pane".to_string()],
                    LinkedHashMap::from_iter([("role".to_string(), "tabpanel".to_string())]),
                );
                if index == 0 {
                    add_class(&mut pane_attr, "active");
                    add_class(&mut pane_attr, "show");
                }
                panes.push(div(pane_attr, vec![body_div]));
            }
            None => {
                apply_fill_recursive(&mut body_div);
                if is_valuebox && index == 0 {
                    finish_value_box(&mut attr, &mut body_div);
                }
                content.push(body_div);
            }
        }
    }
    if let Some(id) = &tabset_id {
        let mut tab_content = div(
            (
                String::new(),
                vec!["tab-content".to_string()],
                LinkedHashMap::from_iter([("data-tabset-id".to_string(), id.clone())]),
            ),
            panes,
        );
        apply_fill_recursive(&mut tab_content);
        content.push(tab_content);
    }

    if !card.footer.is_empty() {
        content.push(div(
            (
                String::new(),
                vec!["card-footer".to_string()],
                LinkedHashMap::new(),
            ),
            lower_all(card.footer, cx),
        ));
    }
    if data.expandable {
        content.push(raw_html(EXPAND_BUTTON));
    }
    content.push(raw_html(CARD_INIT_SCRIPT));
    cx.parent = parent;
    div(attr, content)
}

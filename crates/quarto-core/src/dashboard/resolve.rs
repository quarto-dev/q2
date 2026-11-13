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

use super::model::{
    CARD_TYPE, CONTAINER_TYPE, Card, Container, Orientation, PAGE_TYPE, PANEL_TYPE, Page, Panel,
    PanelKind,
};

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
const DONT_RECURSE: &[&str] = &["sidebar", "toolbar", FILL_STOP];

/// Marks a sidebar\'s Div while the lowering runs. Q1\'s fill passes stop
/// at `aside.sidebar` (`sidebar` in [`DONT_RECURSE`]); q2\'s `<aside>` is
/// raw HTML around the Div, so later passes (the page\'s) would otherwise
/// fill the sidebar\'s content and stretch it down the sidebar.
/// [`lower_document`] removes the class.
const FILL_STOP: &str = "dashboard-fill-stop";

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
    Block::RawBlock(Box::new(RawBlock {
        format: "html".to_string(),
        text: text.to_string(),
        source_info: generated(),
    }))
}

fn div(attr: Attr, content: Blocks) -> Block {
    Block::Div(Box::new(Div {
        attr,
        content,
        source_info: generated(),
        attr_source: AttrSourceInfo::empty(),
    }))
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

/// A card body\'s computed `flow` class (Q1 `cardBodyLayout`) while the
/// lowering runs; [`lower_document`] renames it to `flow` at the end. Q1
/// adds that class in `processCards`, after `processPages` has filled every
/// page, so the page pass fills such a body while the card\'s own passes
/// (and the later sidebar and toolbar passes) treat it as `flow`.
const FLOW_PENDING: &str = "dashboard-flow-pending";

fn never_fill(attr: &Attr) -> bool {
    NEVER_FILL.iter().any(|c| has_class(attr, c)) || has_class(attr, FLOW_PENDING)
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

/// Q1 `recursiveApplyFillClasses`, as the passes that run before Q1 builds
/// card titles see the page (pages, tabs, card bodies): a `.card-title`
/// (which q2 has already built) is skipped.
pub fn apply_fill_recursive(block: &mut Block) {
    apply_fill(block, false);
}

/// Q1 `processPages`\' fill pass: it runs before card bodies get their
/// computed `flow` class, so [`FLOW_PENDING`] bodies are filled.
fn apply_fill_page(block: &mut Block) {
    rename_class(block, FLOW_PENDING, "dashboard-flow-pending-page");
    apply_fill(block, false);
    rename_class(block, "dashboard-flow-pending-page", FLOW_PENDING);
}

/// Rename `from` to `to` on every Div under `block`; an empty `to` removes
/// the class.
fn rename_class(block: &mut Block, from: &str, to: &str) {
    match block {
        Block::Div(d) => {
            for c in d.attr.1.iter_mut() {
                if c == from {
                    *c = to.to_string();
                }
            }
            d.attr.1.retain(|c| !c.is_empty());
            for child in d.content.iter_mut() {
                rename_class(child, from, to);
            }
        }
        Block::Figure(f) => {
            for child in f.content.iter_mut() {
                rename_class(child, from, to);
            }
        }
        Block::BlockQuote(q) => {
            for child in q.content.iter_mut() {
                rename_class(child, from, to);
            }
        }
        _ => {}
    }
}

/// Q1 `recursiveApplyFillClasses`. `titles`: whether card titles exist yet
/// in Q1's ordering. The sidebar and toolbar passes run after the card
/// pass, so their card titles get fill classes; earlier passes do not see
/// titles at all.
fn apply_fill(block: &mut Block, titles: bool) {
    match block {
        Block::Div(d) if !titles && has_class(&d.attr, "card-title") => {}
        Block::Div(d) => {
            if !never_fill(&d.attr) {
                add_class(&mut d.attr, "html-fill-item");
                add_class(&mut d.attr, "html-fill-container");
            }
            if !DONT_RECURSE.iter().any(|c| has_class(&d.attr, c)) {
                for child in d.content.iter_mut() {
                    apply_fill(child, titles);
                }
            }
        }
        Block::Figure(f) => {
            if !never_fill(&f.attr) && fill_by_class(&f.attr) {
                add_class(&mut f.attr, "html-fill-item");
                add_class(&mut f.attr, "html-fill-container");
            }
            for child in f.content.iter_mut() {
                apply_fill(child, titles);
            }
        }
        Block::BlockQuote(q) => {
            for child in q.content.iter_mut() {
                apply_fill(child, titles);
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

/// Classes the template adds for what the lowering found at the top level
/// (Q1 `processSidebars` / `processToolbars`).
#[derive(Debug, Default)]
pub struct TopLevel {
    /// For `<body>`: `dashboard-sidebar`, `dashboard-toolbar`.
    pub body_classes: Vec<String>,
    /// For the content area: `dashboard-sidebar-container`.
    pub content_classes: Vec<String>,
}

/// Lower every dashboard node under `blocks` (the document's top level) to
/// bslib markup. `preview`: finish with [`super::view::to_views`] for the
/// React preview.
pub fn lower_document(blocks: Blocks, preview: bool) -> (Blocks, TopLevel) {
    let mut cx = Lower {
        preview,
        ..Default::default()
    };
    let mut top = TopLevel::default();
    let mut lowered_all: Blocks = blocks
        .into_iter()
        .map(|block| {
            let panel = match &block {
                Block::Custom(n) if n.type_name == PANEL_TYPE => {
                    Panel::from_custom(n).map(|p| p.data.kind)
                }
                _ => None,
            };
            let mut lowered = lower(block, &mut cx);
            // A panel replaces its container after Q1's top-level pass, so
            // it is not a grid item.
            if let Some(kind) = panel {
                match kind {
                    PanelKind::Sidebar => {
                        add_unique(&mut top.content_classes, "dashboard-sidebar-container")
                    }
                    PanelKind::Toolbar => {
                        if let Block::Div(d) = &mut lowered
                            && !top.body_classes.iter().any(|c| c == "dashboard-toolbar")
                        {
                            add_class(&mut d.attr, "toolbar-toplevel");
                            add_unique(&mut top.body_classes, "dashboard-toolbar");
                        }
                    }
                }
                return lowered;
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
        .collect();
    for b in lowered_all.iter_mut() {
        rename_class(b, FLOW_PENDING, "flow");
        rename_class(b, FILL_STOP, "");
    }
    if mark_top_level_sidebar(&mut lowered_all) {
        add_unique(&mut top.body_classes, "dashboard-sidebar");
    }
    if preview {
        lowered_all = super::view::to_views(lowered_all);
    }
    (lowered_all, top)
}

fn add_unique(list: &mut Vec<String>, class: &str) {
    if !list.iter().any(|c| c == class) {
        list.push(class.to_string());
    }
}

/// Q1: the first sidebar layout that is a direct child of the content area
/// or of a page has no border (`data-bslib-sidebar-border[-radius]=false`),
/// and the body gets `dashboard-sidebar`.
fn mark_top_level_sidebar(blocks: &mut Blocks) -> bool {
    fn is_layout(b: &Block) -> bool {
        matches!(b, Block::Div(d) if has_class(&d.attr, "bslib-sidebar-layout"))
    }
    fn mark(b: &mut Block) {
        if let Block::Div(d) = b {
            d.attr
                .2
                .insert("data-bslib-sidebar-border".to_string(), "false".to_string());
            d.attr.2.insert(
                "data-bslib-sidebar-border-radius".to_string(),
                "false".to_string(),
            );
        }
    }
    fn search(blocks: &mut Blocks, top: bool) -> bool {
        for b in blocks.iter_mut() {
            if top && is_layout(b) {
                mark(b);
                return true;
            }
            if let Block::Div(d) = b {
                let page = has_class(&d.attr, "dashboard-page");
                if page && let Some(first) = d.content.iter_mut().find(|c| is_layout(c)) {
                    mark(first);
                    return true;
                }
                if search(&mut d.content, false) {
                    return true;
                }
            }
        }
        false
    }
    search(blocks, true)
}

/// State threaded through one document's lowering: Q1 numbers tabsets by
/// the card's position among all `.card` elements in document order
/// (`card-tabset-<n>`).
#[derive(Default)]
struct Lower {
    cards: usize,
    /// Page sidebars lowered so far (`bslib-sidebar-<n>`).
    sidebars: usize,
    /// Lowering for the React preview (the tab nav is a view node).
    preview: bool,
    /// The orientation of the container whose children are being lowered
    /// (`None` inside pages, card bodies and other non-grid content).
    parent: Option<Orientation>,
}

fn lower_all(blocks: Blocks, cx: &mut Lower) -> Blocks {
    blocks.into_iter().map(|b| lower(b, cx)).collect()
}

/// [`lower_all`] for a grid's or page's children; Q1 gives the parent of a
/// page sidebar `dashboard-sidebar-container`.
fn lower_children(blocks: Blocks, cx: &mut Lower, parent: &mut Attr) -> Blocks {
    let has_sidebar = blocks.iter().any(|b| {
        matches!(b, Block::Custom(n) if n.type_name == PANEL_TYPE
            && Panel::from_custom(n).is_some_and(|p| p.data.kind == PanelKind::Sidebar))
    });
    if has_sidebar {
        add_class(parent, "dashboard-sidebar-container");
    }
    lower_all(blocks, cx)
}

fn lower(block: Block, cx: &mut Lower) -> Block {
    let Block::Custom(node) = &block else {
        return match block {
            Block::Div(d) if explicit_grid(&d.attr).is_some() => lower_explicit_grid(*d, cx),
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
        PANEL_TYPE => Panel::from_custom(node).map(|p| lower_panel(p, cx)),
        _ => None,
    }
    .unwrap_or(block)
}

const SIDEBAR_TOGGLE: &str = r#"<button class="collapse-toggle" type="button" title="Toggle sidebar" aria-expanded="true" aria-controls="{id}">
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" class="bi bi-chevron-left collapse-icon" style="fill:currentColor;" aria-hidden="true" role="img">
      <path fill-rule="evenodd" d="M11.354 1.646a.5.5 0 0 1 0 .708L5.707 8l5.647 5.646a.5.5 0 0 1-.708.708l-6-6a.5.5 0 0 1 0-.708l6-6a.5.5 0 0 1 .708 0z"></path>
    </svg>
</button>
<script data-bslib-sidebar-init>
bslib.Sidebar.initCollapsibleAll()
</script>"#;

/// Q1 `asCssSize`: a bare number is pixels.
fn css_size(v: &str) -> String {
    if v.chars().last().is_some_and(|c| c.is_ascii_digit()) {
        format!("{v}px")
    } else {
        v.to_string()
    }
}

/// Q1 `makeSidebar` (`format-dashboard-sidebar.ts`): the bslib sidebar
/// layout around `main` content and an `aside` holding the sidebar Div.
/// `titles_fill`: whether the fill pass runs after card titles exist.
fn sidebar_layout(
    id: &str,
    main: Blocks,
    sidebar: Block,
    at_end: bool,
    width: Option<&str>,
    titles_fill: bool,
) -> Block {
    let mut attr: Attr = (
        String::new(),
        vec![
            "bslib-sidebar-layout".to_string(),
            "html-fill-item".to_string(),
        ],
        LinkedHashMap::from_iter([
            ("data-bslib-sidebar-open".to_string(), "desktop".to_string()),
            ("data-bslib-sidebar-init".to_string(), "true".to_string()),
        ]),
    )
        .into();
    if at_end {
        add_class(&mut attr, "sidebar-right");
    }
    if let Some(w) = width {
        attr.2.insert(
            "style".to_string(),
            format!(" --bslib-sidebar-width: {}", css_size(w)),
        );
    }
    let main = div(
        (
            String::new(),
            vec!["main".to_string(), "html-fill-container".to_string()],
            LinkedHashMap::new(),
        )
            .into(),
        main,
    );
    let mut layout = div(attr, vec![main]);
    apply_fill(&mut layout, titles_fill);
    let mut sidebar = sidebar;
    if let Block::Div(d) = &mut sidebar {
        add_class(&mut d.attr, "html-fill-container");
        add_class(&mut d.attr, "html-fill-item");
        add_class(&mut d.attr, FILL_STOP);
    }
    let aside = raw_html(&format!(
        r#"<aside id="{id}" class="sidebar html-fill-container html-fill-item">"#
    ));
    if let Block::Div(d) = &mut layout {
        d.content.push(aside);
        d.content.push(sidebar);
        d.content.push(raw_html("</aside>"));
        d.content
            .push(raw_html(&SIDEBAR_TOGGLE.replace("{id}", id)));
    }
    layout
}

/// A page sidebar or toolbar (Q1 `processSidebars` / `processToolbars`).
/// Both run after the card pass, so their fill classes reach card titles.
fn lower_panel(p: Panel, cx: &mut Lower) -> Block {
    let parent = cx.parent.take();
    let content = lower_all(p.content, cx);
    cx.parent = parent;
    let panel_div = div(
        (String::new(), Vec::new(), LinkedHashMap::new()).into(),
        lower_all(p.panel, cx),
    );
    match p.data.kind {
        PanelKind::Sidebar => {
            cx.sidebars += 1;
            let id = format!("bslib-sidebar-{}", cx.sidebars);
            let main = vec![div(
                (
                    String::new(),
                    vec!["sidebar-content".to_string()],
                    LinkedHashMap::new(),
                )
                    .into(),
                content,
            )];
            sidebar_layout(
                &id,
                main,
                panel_div,
                p.data.at_end,
                p.data.width.as_deref(),
                true,
            )
        }
        PanelKind::Toolbar => {
            let Block::Div(mut toolbar) = panel_div else {
                unreachable!()
            };
            toolbar.attr.1 = vec!["toolbar".to_string(), "html-fill-container".to_string()];
            if p.data.at_end {
                add_class(&mut toolbar.attr, "toolbar-bottom");
            }
            // D5-Q1: a toolbar's `height` sizes it (Q1 never read it).
            if let Some(h) = &p.data.height {
                append_style(&mut toolbar.attr, format!("height: {};", css_size(h)));
            }
            let toolbar_content = div(
                (
                    String::new(),
                    vec!["toolbar-content".to_string()],
                    LinkedHashMap::new(),
                )
                    .into(),
                content,
            );
            let mut container = div(
                (
                    String::new(),
                    vec![
                        "html-fill-item".to_string(),
                        "html-fill-container".to_string(),
                    ],
                    LinkedHashMap::new(),
                )
                    .into(),
                vec![Block::Div(toolbar), toolbar_content],
            );
            apply_fill(&mut container, true);
            if let Block::Div(d) = &mut container {
                add_class(&mut d.attr, "dashboard-toolbar-container");
            }
            container
        }
    }
}

fn lower_container(c: Container, cx: &mut Lower) -> Block {
    let mut attr: Attr = (
        c.data.id.clone(),
        vec!["bslib-grid".to_string()],
        LinkedHashMap::new(),
    )
        .into();
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
    // Q1 `processPages`: the pages' parent holds them as tab panes.
    if c.children
        .iter()
        .any(|b| matches!(b, Block::Custom(n) if n.type_name == PAGE_TYPE))
    {
        add_class(&mut attr, "tab-content");
    }
    let parent = cx.parent.replace(c.data.orientation);
    let children = lower_children(c.children, cx, &mut attr);
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
    )
        .into();
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
    let content = lower_children(p.content, cx, &mut attr);
    let mut page = div(attr, content);
    cx.parent = parent;
    apply_fill_page(&mut page);
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

/// Each tab\'s title (Q1: a body\'s title, else `Tab <n>`).
fn tab_titles(bodies: &[super::model::BodyData]) -> Vec<String> {
    bodies
        .iter()
        .enumerate()
        .map(|(i, b)| b.title.clone().unwrap_or_else(|| format!("Tab {}", i + 1)))
        .collect()
}

/// Q1 `convertToTabsetHeader`: the tab list in a tabset card's header.
fn tab_nav_html(id: &str, bodies: &[super::model::BodyData]) -> String {
    let mut html = format!(
        r#"<ul class="nav nav-tabs card-header-tabs" role="tablist" data-tabsetid="{id}">"#
    );
    for (i, title) in tab_titles(bodies).iter().enumerate() {
        let n = i + 1;
        let title = html_escape(title);
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
    let card_number = cx.cards;
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
    let mut attr: Attr = (String::new(), data.classes.clone(), LinkedHashMap::new()).into();
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
        )
            .into();
        let mut header_content = lower_all(header, cx);
        let mut title_div = None;
        match title {
            Some(title) if !title.is_empty() => {
                let span = Inline::Span(Box::new(Span {
                    attr: (
                        String::new(),
                        Vec::new(),
                        LinkedHashMap::from_iter([(
                            "style".to_string(),
                            "display: inline".to_string(),
                        )]),
                    )
                        .into(),
                    content: title,
                    source_info: generated(),
                    attr_source: AttrSourceInfo::empty(),
                }));
                title_div = Some(div(
                    (
                        String::new(),
                        vec!["card-title".to_string()],
                        LinkedHashMap::new(),
                    )
                        .into(),
                    vec![Block::Plain(Plain {
                        content: vec![span],
                        source_info: generated(),
                    })],
                ));
            }
            _ => add_class(&mut header_attr, "dashboard-card-no-title"),
        }
        if let Some(title_div) = title_div {
            // Q1 `processCards`: the title goes first in the header's card
            // toolbar when it has one, else first in the header.
            match header_content.iter_mut().find_map(|b| match b {
                Block::Div(d) if has_class(&d.attr, "card-toolbar") => Some(d),
                _ => None,
            }) {
                Some(toolbar) => toolbar.content.insert(0, title_div),
                None => header_content.insert(0, title_div),
            }
        }
        if let Some(id) = &tabset_id {
            add_class(&mut header_attr, "bslib-navs-card-title");
            header_content.push(if cx.preview {
                super::view::tab_nav(id, &tab_titles(&data.bodies))
            } else {
                raw_html(&tab_nav_html(id, &data.bodies))
            });
        }
        content.push(div(header_attr, header_content));
    }

    let card_sidebar = data.sidebar.clone().map(|sb| {
        let mut panel = lower_all(card.sidebar, cx);
        let block = if panel.len() == 1 {
            panel.remove(0)
        } else {
            div(
                (String::new(), Vec::new(), LinkedHashMap::new()).into(),
                panel,
            )
        };
        (sb, block)
    });
    let mut bodies_out = Vec::new();
    let mut panes = Vec::new();
    for (index, (body_data, body)) in data.bodies.iter().zip(card.bodies).enumerate() {
        let mut body_attr: Attr = (
            body_data.id.clone(),
            body_data.classes.clone(),
            body_data.attributes.iter().cloned().collect(),
        )
            .into();
        add_class(&mut body_attr, "card-body");
        if body_data.flow {
            add_class(&mut body_attr, FLOW_PENDING);
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
                )
                    .into();
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
                bodies_out.push(body_div);
            }
        }
    }
    if let Some(id) = &tabset_id {
        let mut tab_content = div(
            (
                String::new(),
                vec!["tab-content".to_string()],
                LinkedHashMap::from_iter([("data-tabset-id".to_string(), id.clone())]),
            )
                .into(),
            panes,
        );
        apply_fill_recursive(&mut tab_content);
        match card_sidebar.clone() {
            // Q1 `convertToTabs`: a card sidebar wraps the tab content, ahead
            // of the footer.
            Some((sb, panel)) => content.push(sidebar_layout(
                &format!("{id}-card-sidebar"),
                vec![tab_content],
                panel,
                sb.position == "right",
                sb.width.as_deref(),
                false,
            )),
            None => content.push(tab_content),
        }
    }
    let bodies_layout = match (&card_sidebar, tabset_id.is_some()) {
        (Some(_), false) => Some(std::mem::take(&mut bodies_out)),
        _ => {
            content.append(&mut bodies_out);
            None
        }
    };

    if !card.footer.is_empty() {
        content.push(div(
            (
                String::new(),
                vec!["card-footer".to_string()],
                LinkedHashMap::new(),
            )
                .into(),
            lower_all(card.footer, cx),
        ));
    }
    // Q1 `processCards` appends a (non-tabset) card's sidebar layout, with
    // the bodies in its main area, after the footer.
    if let (Some(bodies), Some((sb, panel))) = (bodies_layout, card_sidebar) {
        content.push(sidebar_layout(
            &format!("card-{}-card-sidebar", card_number),
            bodies,
            panel,
            sb.position == "right",
            sb.width.as_deref(),
            false,
        ));
    }
    if data.expandable {
        content.push(raw_html(EXPAND_BUTTON));
    }
    content.push(raw_html(CARD_INIT_SCRIPT));
    cx.parent = parent;
    div(attr, content)
}

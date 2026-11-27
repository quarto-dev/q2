/*
 * layout_panel.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Transform that lays the children of a `layout-ncol` / `layout-nrow` /
 * `layout` Div out in rows and cells.
 */

//! Layout panels for the HTML formats.
//!
//! A Div (or a code cell, or a figure div) with `layout-ncol`, `layout-nrow`
//! or `layout` holds a list of blocks to arrange in a grid:
//!
//! ```markdown
//! ::: {layout-ncol=2}
//! ![One](a.png)
//!
//! ![Two](b.png)
//! :::
//! ```
//!
//! This is a port of Quarto 1's layout engine (`filters/layout/layout.lua`,
//! `width.lua`) and of its HTML renderer (`filters/layout/html.lua`). The
//! DOM is the one the float taxonomy design pins
//! (`claude-notes/designs/float-layout-class-taxonomy.md`, shape 3):
//!
//! ```text
//! Div (original attr + .quarto-layout-panel)
//!   Div (.quarto-layout-row [.quarto-layout-valign-{top|center|bottom}])
//!     Div (.quarto-layout-cell, style="flex-basis: 50.0%;justify-content: flex-start;")
//!       [the cell's blocks]
//! ```
//!
//! ## The engine
//!
//! - **Cells.** Each child block is one cell. A child Div is the cell
//!   itself; anything else is wrapped in a new Div. A heading attaches to
//!   the cell after it. A `.cell-output-display` Div with no figure float in
//!   it is unrolled, one cell per block. Code (`.cell-code`), stderr output
//!   and annotations are not cells: they move in front of the panel.
//! - **Rows.** `layout-ncol` fills rows of that many cells. `layout-nrow`
//!   derives the column count. `layout` is a JSON array of rows of widths
//!   (`[[1,1],[1]]`), where a negative width is an empty spacer. With none of
//!   the three the panel has one column.
//! - **Widths.** Percentages, to one decimal place, written as `flex-basis`.
//! - **Alignment.** `layout-align` sets `justify-content` on every cell. With
//!   no `layout-align`, a cell holding a bare image or a table is centered and
//!   any other cell starts at the left. `layout-valign` becomes a class on
//!   each row.
//!
//! ## Where it runs
//!
//! `Finalization`, right after `crossref-render`, for `format: html` and
//! `format: revealjs` renders. By then a figure div is its rendered
//! `Div > Figure > Div` shape, so a figure with layout attributes gets its
//! rows inside the figure's content wrapper and keeps its caption.
//!
//! ## What it does not do
//!
//! Subfloat identity. The children of a cross-referenced panel are still
//! numbered as figures of their own ("Figure 1", not "(a)"), and the cells
//! carry no `quarto-layout-cell-subref` class or `data-ref-parent`
//! attribute. That is the subfloat work (bd-plcqhfcn).

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div, Paragraph};
use quarto_pandoc_types::inline::{Image, Inline, Str, Strong};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::ast_walk::for_each_block_list_mut;
use crate::format::PipelineProfile;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

const LAYOUT_NCOL: &str = "layout-ncol";
const LAYOUT_NROW: &str = "layout-nrow";
const LAYOUT: &str = "layout";
const LAYOUT_ALIGN: &str = "layout-align";
const LAYOUT_VALIGN: &str = "layout-valign";

const PANEL_CLASS: &str = "quarto-layout-panel";
const ROW_CLASS: &str = "quarto-layout-row";
const CELL_CLASS: &str = "quarto-layout-cell";
const SPACER_CLASS: &str = "quarto-figure-spacer";

/// Divs that are the rendered form of one piece of content (a figure, a
/// callout, a tabset, a column set, another panel). In Quarto 1 these are
/// custom nodes when the layout runs, so each is wrapped in a cell. Any other
/// Div is used as the cell itself.
const WRAPPED_DIV_CLASSES: [&str; 6] = [
    "quarto-figure",
    "quarto-float",
    PANEL_CLASS,
    "callout",
    "panel-tabset",
    "columns",
];

/// Transform that builds layout panels.
pub struct LayoutPanelTransform;

impl LayoutPanelTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LayoutPanelTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for LayoutPanelTransform {
    fn name(&self) -> &str {
        "layout-panel"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let reveal = match ctx.pipeline_profile {
            PipelineProfile::HtmlRender => false,
            PipelineProfile::RevealjsRender => true,
            _ => return Ok(()),
        };
        let mut diagnostics = Vec::new();
        // Post-order: an inner panel is built before the Div around it is
        // looked at.
        for_each_block_list_mut(&mut ast.blocks, &mut |list| {
            layout_block_list(list, reveal, &mut diagnostics);
        });
        ctx.diagnostics.extend(diagnostics);
        Ok(())
    }
}

/// Replace every panel Div in `list` with its preamble and its panel.
fn layout_block_list(
    list: &mut Vec<Block>,
    reveal: bool,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    let mut i = 0;
    while i < list.len() {
        if !matches!(&list[i], Block::Div(div) if requires_panel_layout(div)) {
            i += 1;
            continue;
        }
        let Block::Div(div) = list.remove(i) else {
            unreachable!("checked above");
        };
        let replacement = build_panel(*div, reveal, diagnostics);
        let count = replacement.len();
        list.splice(i..i, replacement);
        i += count;
    }
}

fn has_class(div: &Div, class: &str) -> bool {
    div.attr.1.iter().any(|c| c == class)
}

/// A Div with a layout attribute, unless a panel was already built inside
/// it. Quarto 1 leaves such a Div alone ("the layout has been handled by the
/// child").
fn requires_panel_layout(div: &Div) -> bool {
    [LAYOUT_NCOL, LAYOUT_NROW, LAYOUT]
        .iter()
        .any(|key| div.attr.2.contains_key(*key))
        && !has_class(div, PANEL_CLASS)
        && !any_div(&div.content, &|d| has_class(d, PANEL_CLASS))
}

/// Whether any Div under `blocks` (through Divs, figures and block quotes)
/// satisfies `pred`.
fn any_div(blocks: &[Block], pred: &dyn Fn(&Div) -> bool) -> bool {
    blocks.iter().any(|block| match block {
        Block::Div(d) => pred(d) || any_div(&d.content, pred),
        Block::Figure(f) => any_div(&f.content, pred),
        Block::BlockQuote(b) => any_div(&b.content, pred),
        _ => false,
    })
}

fn contains_table(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Table(_) => true,
        Block::Div(d) => contains_table(&d.content),
        Block::Figure(f) => contains_table(&f.content),
        Block::BlockQuote(b) => contains_table(&b.content),
        _ => false,
    })
}

fn generated() -> SourceInfo {
    SourceInfo::generated(By::raw("layout-panel", serde_json::Value::Null))
}

fn new_div(classes: Vec<String>, content: Vec<Block>) -> Div {
    Div {
        attr: (String::new(), classes, hashlink::LinkedHashMap::new()).into(),
        content,
        source_info: generated(),
        attr_source: AttrSourceInfo::empty(),
    }
}

// ── The panel ──────────────────────────────────────────────────────────────

/// The layout attributes of one panel.
struct LayoutSpec {
    ncol: Option<usize>,
    nrow: Option<usize>,
    layout: Option<String>,
    /// `layout-align`, when valid: `left`, `center` or `right`.
    align: Option<String>,
    /// The row class for `layout-valign`.
    valign_class: Option<&'static str>,
}

impl LayoutSpec {
    fn from_div(div: &Div, diagnostics: &mut Vec<DiagnosticMessage>) -> Self {
        let kvs = &div.attr.2;
        // Lua `tonumber`: anything that is not a number counts as absent.
        let count = |key: &str| {
            kvs.get(key)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|n| *n >= 1.0)
                .map(|n| n.floor() as usize)
        };
        let mut warn = |what: &str, value: &str| {
            diagnostics.push(
                DiagnosticMessageBuilder::warning(format!("Invalid {what} attribute value"))
                    .problem(format!(
                        "`{value}` is not a {what} this layout understands."
                    ))
                    .with_location(div.source_info.clone())
                    .build(),
            );
        };
        let align = match kvs.get(LAYOUT_ALIGN).map(String::as_str) {
            None | Some("default") => None,
            Some(v @ ("left" | "center" | "right")) => Some(v.to_string()),
            Some(other) => {
                warn("horizontal alignment", other);
                None
            }
        };
        let valign_class = match kvs.get(LAYOUT_VALIGN).map(String::as_str) {
            // Quarto 1 has no class for `top-baseline`.
            None | Some("top-baseline") => None,
            Some("top" | "default") => Some("quarto-layout-valign-top"),
            Some("center") => Some("quarto-layout-valign-center"),
            Some("bottom") => Some("quarto-layout-valign-bottom"),
            Some(other) => {
                warn("vertical alignment", other);
                Some("quarto-layout-valign-top")
            }
        };
        Self {
            ncol: count(LAYOUT_NCOL),
            nrow: count(LAYOUT_NROW),
            layout: kvs.get(LAYOUT).cloned(),
            align,
            valign_class,
        }
    }
}

/// Build the panel for `div`. Returns the blocks that replace it: the
/// preamble (code, stderr, annotations), then the panel.
fn build_panel(mut div: Div, reveal: bool, diagnostics: &mut Vec<DiagnosticMessage>) -> Vec<Block> {
    let spec = LayoutSpec::from_div(&div, diagnostics);
    let location = div.source_info.clone();
    let mut preamble = Vec::new();

    if let Some(wrapper) = float_content_wrapper(&mut div) {
        // A rendered figure div: the rows go where its content was, and the
        // caption stays. Quarto 1 then replaces the outer attr with the id
        // and `quarto-layout-panel` (`layout/html.lua`).
        let content = std::mem::take(&mut wrapper.content);
        let cells = partition_cells(content, reveal, &mut preamble);
        wrapper.content = render_rows(arrange(cells, &spec, &location, diagnostics), &spec);

        let margin_caption = has_class(&div, "margin-caption");
        let attr = &mut *div.attr;
        attr.1 = vec![PANEL_CLASS.to_string()];
        if margin_caption {
            attr.1.push("margin-caption".to_string());
        }
        for key in [
            LAYOUT_NCOL,
            LAYOUT_NROW,
            LAYOUT,
            LAYOUT_ALIGN,
            LAYOUT_VALIGN,
        ] {
            attr.2.remove(key);
        }
    } else {
        let content = std::mem::take(&mut div.content);
        let cells = partition_cells(content, reveal, &mut preamble);
        div.content = render_rows(arrange(cells, &spec, &location, diagnostics), &spec);

        let attr = &mut *div.attr;
        attr.1.push(PANEL_CLASS.to_string());
        attr.2.remove(LAYOUT_VALIGN);
    }

    preamble.push(Block::Div(Box::new(div)));
    preamble
}

/// For a figure div as `crossref-render` leaves it
/// (`Div.quarto-float > Figure > Div[aria-describedby]`), the innermost Div,
/// which holds the figure's content.
fn float_content_wrapper(div: &mut Div) -> Option<&mut Div> {
    if !has_class(div, "quarto-float") {
        return None;
    }
    let [Block::Figure(figure)] = div.content.as_mut_slice() else {
        return None;
    };
    let [Block::Div(wrapper)] = figure.content.as_mut_slice() else {
        return None;
    };
    wrapper
        .attr
        .2
        .contains_key("aria-describedby")
        .then_some(&mut **wrapper)
}

// ── Cells ──────────────────────────────────────────────────────────────────

/// One cell of a panel.
struct Cell {
    div: Div,
    /// The width the cell asks for (an image's `width`, a Div's `width`, or
    /// a width from `layout`).
    width: Option<String>,
}

impl Cell {
    /// A child Div is the cell itself, unless it is the rendered form of a
    /// figure, callout and so on (see [`WRAPPED_DIV_CLASSES`]).
    fn from_block(block: Block) -> Self {
        match block {
            Block::Div(div) if !WRAPPED_DIV_CLASSES.iter().any(|c| has_class(&div, c)) => {
                let mut div = *div;
                let width = if div.attr.2.contains_key("width") {
                    div.attr.2.remove("width")
                } else {
                    None
                };
                Cell { div, width }
            }
            other => Cell {
                div: new_div(Vec::new(), vec![other]),
                width: None,
            },
        }
    }

    fn spacer(width: String) -> Self {
        let nbsp = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "\u{a0}".to_string(),
                source_info: generated(),
            })],
            source_info: generated(),
        });
        Cell {
            div: new_div(vec![SPACER_CLASS.to_string()], vec![nbsp]),
            width: Some(width),
        }
    }
}

/// Quarto 1 `is_preamble_block`: what a code cell shows that is not a panel
/// cell.
fn is_preamble_block(block: &Block) -> bool {
    match block {
        Block::CodeBlock(code) => code.attr.1.iter().any(|c| c == "cell-code"),
        Block::Div(div) => {
            has_class(div, "cell-output-stderr") || has_class(div, "cell-annotation")
        }
        _ => false,
    }
}

/// Add a block to the preamble. Two code blocks in a row with the same
/// classes become one, as in Quarto 1 (`handle_preamble_codeblock`).
fn push_preamble(preamble: &mut Vec<Block>, block: Block) {
    if let (Some(Block::CodeBlock(last)), Block::CodeBlock(next)) = (preamble.last_mut(), &block)
        && last.attr.1 == next.attr.1
    {
        last.text.push('\n');
        last.text.push_str(&next.text);
        return;
    }
    preamble.push(block);
}

/// Move the `.cell-code` blocks found anywhere under a cell Div into the
/// preamble (quarto-cli #4370).
fn lift_code_blocks(blocks: &mut Vec<Block>, preamble: &mut Vec<Block>) {
    let mut i = 0;
    while i < blocks.len() {
        if matches!(&blocks[i], Block::CodeBlock(_)) && is_preamble_block(&blocks[i]) {
            push_preamble(preamble, blocks.remove(i));
            continue;
        }
        if let Block::Div(div) = &mut blocks[i] {
            lift_code_blocks(&mut div.content, preamble);
        }
        i += 1;
    }
}

/// The image of a cell block that is a bare image: a paragraph holding one
/// image, or one link around one image.
fn lone_image_mut(block: &mut Block) -> Option<&mut Image> {
    let content = match block {
        Block::Paragraph(p) => &mut p.content,
        Block::Plain(p) => &mut p.content,
        _ => return None,
    };
    match content.as_mut_slice() {
        [Inline::Image(image)] => Some(&mut **image),
        [Inline::Link(link)] => match link.content.as_mut_slice() {
            [Inline::Image(image)] => Some(&mut **image),
            _ => None,
        },
        _ => None,
    }
}

fn has_lone_image(blocks: &mut [Block]) -> bool {
    blocks.iter_mut().any(|b| lone_image_mut(b).is_some())
}

/// `50%` and `33.3%` are percentages (Lua `^([%d%.]+)%%$`).
fn percent_value(size: &str) -> Option<f64> {
    let number = size.strip_suffix('%')?;
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    number.parse().ok()
}

/// Quarto 1 `transferImageWidthToCell`: a bare image's width is the width
/// its cell asks for. A percentage leaves the image, which then fills the
/// cell. The height goes, so the image keeps its proportions.
fn transfer_image_width(cell: &mut Cell) {
    let Some(image) = cell.div.content.iter_mut().find_map(lone_image_mut) else {
        return;
    };
    cell.width = image.attr.2.get("width").cloned();
    let percent = cell.width.as_deref().and_then(percent_value).is_some();
    if percent {
        image.attr.2.remove("width");
    }
    if image.attr.2.contains_key("height") {
        image.attr.2.remove("height");
    }
}

/// Quarto 1 `partition_cells`: split a panel's blocks into cells, moving the
/// preamble blocks out.
fn partition_cells(content: Vec<Block>, reveal: bool, preamble: &mut Vec<Block>) -> Vec<Cell> {
    let mut cells = Vec::new();
    let mut heading: Option<Block> = None;
    for block in content {
        if is_preamble_block(&block) {
            push_preamble(preamble, block);
            continue;
        }
        if let Block::Header(header) = block {
            // A deck would read a heading as a slide title, so there it
            // becomes a bold paragraph.
            heading = Some(if reveal {
                let header = *header;
                Block::Paragraph(Paragraph {
                    content: vec![Inline::Strong(Strong {
                        content: header.content,
                        source_info: header.source_info.clone(),
                    })],
                    source_info: header.source_info,
                })
            } else {
                Block::Header(header)
            });
            continue;
        }

        let mut cell = Cell::from_block(block);
        lift_code_blocks(&mut cell.div.content, preamble);
        transfer_image_width(&mut cell);
        if let Some(heading) = heading.take() {
            cell.div.content.insert(0, heading);
        }

        // An output Div that is not one figure float holds independent
        // blocks: one cell each.
        if has_class(&cell.div, "cell-output-display")
            && !any_div(&cell.div.content, &|d| has_class(d, "quarto-float"))
        {
            cells.extend(cell.div.content.into_iter().map(Cell::from_block));
        } else {
            cells.push(cell);
        }
    }
    cells
}

// ── Rows and widths ────────────────────────────────────────────────────────

/// Lua `round(num, places)`: half up.
fn round_to(value: f64, places: i32) -> f64 {
    let mult = 10f64.powi(places);
    (value * mult + 0.5).floor() / mult
}

/// A percentage to one decimal place, as Lua prints a float: `50.0%`.
fn percent_string(value: f64) -> String {
    format!("{:.1}%", round_to(value, 1))
}

/// The number a width starts with (Lua `^(-?[%d%.]+)`), or 0.
fn leading_number(width: &str) -> f64 {
    let unsigned = width.strip_prefix('-').unwrap_or(width);
    let digits = unsigned
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map_or(unsigned, |end| &unsigned[..end]);
    let end = width.len() - unsigned.len() + digits.len();
    width[..end].parse().unwrap_or(0.0)
}

/// Quarto 1 `layout_cells`: arrange the cells in rows and give each a width.
fn arrange(
    cells: Vec<Cell>,
    spec: &LayoutSpec,
    location: &SourceInfo,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<Vec<Cell>> {
    // `layout-ncol` wins over `layout-nrow`, and both over `layout`.
    let mut cols = spec
        .ncol
        .or_else(|| spec.nrow.map(|nrow| cells.len().div_ceil(nrow).max(1)));
    let mut layout = None;
    if cols.is_none() {
        match spec
            .layout
            .as_deref()
            .map(|text| parse_layout(text, cells.len()))
        {
            Some(Ok(rows)) => layout = Some(rows),
            Some(Err(problem)) => {
                diagnostics.push(
                    DiagnosticMessageBuilder::warning("Invalid figure layout specification")
                        .problem(format!(
                            "{problem} The `layout` attribute is ignored and the panel has one column."
                        ))
                        .with_location(location.clone())
                        .build(),
                );
                cols = Some(1);
            }
            None => cols = Some(1),
        }
    }

    let mut rows: Vec<Vec<Cell>> = Vec::new();
    if let Some(cols) = cols {
        for (i, cell) in cells.into_iter().enumerate() {
            if i % cols == 0 {
                rows.push(Vec::new());
            }
            rows.last_mut().expect("a row was just pushed").push(cell);
        }
        for row in &mut rows {
            widths_to_percent(row, cols);
        }
    } else if let Some(layout) = layout {
        let mut cells = cells.into_iter().peekable();
        for row_widths in layout {
            if cells.peek().is_none() {
                break;
            }
            let mut row = Vec::new();
            for width in row_widths {
                match width.strip_prefix('-') {
                    Some(spacer) => row.push(Cell::spacer(spacer.to_string())),
                    None => {
                        if let Some(mut cell) = cells.next() {
                            cell.width = Some(width);
                            row.push(cell);
                        }
                    }
                }
            }
            rows.push(row);
        }
    }

    // Percentages are written to one decimal place.
    for cell in rows.iter_mut().flatten() {
        if let Some(percent) = cell.width.as_deref().and_then(percent_value) {
            cell.width = Some(percent_string(percent));
        }
    }
    rows
}

/// Quarto 1 `widthsToPercent`: share a row's width by the widths its cells
/// ask for. A cell that asks for none takes the first width asked for in the
/// row, and a short last row keeps the columns of a full one.
fn widths_to_percent(row: &mut [Cell], cols: usize) {
    let mut widths: Vec<f64> = row
        .iter()
        .map(|cell| cell.width.as_deref().map_or(0.0, leading_number))
        .collect();
    // 42 is Quarto 1's own placeholder: any value works when no cell asks
    // for a width.
    let default = widths.iter().copied().find(|w| *w > 0.0).unwrap_or(42.0);
    widths.resize(widths.len().max(cols), 0.0);
    for width in widths.iter_mut().take(cols) {
        if *width == 0.0 {
            *width = default;
        }
    }
    let total: f64 = widths.iter().take(cols).sum();
    for (cell, width) in row.iter_mut().zip(&widths) {
        cell.width = Some(percent_string(width / total * 100.0));
    }
}

/// Quarto 1 `parseLayoutWidths`: the `layout` attribute as rows of width
/// strings. A spacer keeps its leading `-`. `cell_count` is how many cells
/// there are to place: when the layout has too few slots, its last row
/// repeats.
fn parse_layout(text: &str, cell_count: usize) -> std::result::Result<Vec<Vec<String>>, String> {
    use serde_json::Value;

    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) else {
        return Err(format!("`{text}` is not a JSON array."));
    };
    // A flat list is one row.
    let row_values: Vec<Vec<Value>> = if items.iter().any(Value::is_array) {
        let mut rows = Vec::new();
        for item in items {
            let Value::Array(row) = item else {
                return Err(format!("`{text}` mixes rows and widths at the top level."));
            };
            rows.push(row);
        }
        rows
    } else {
        vec![items]
    };

    let mut slots = 0usize;
    let mut spacers = 0usize;
    let mut rows: Vec<Vec<String>> = Vec::new();
    for row in row_values {
        // Numbers in an all-number row are shares of that row.
        let numbers: Option<Vec<f64>> = row.iter().map(Value::as_f64).collect();
        let total: f64 = numbers
            .as_ref()
            .map_or(0.0, |ns| ns.iter().map(|n| n.abs()).sum());
        let mut widths = Vec::new();
        for value in &row {
            let width = match value {
                Value::Number(n) => {
                    let n = n.as_f64().unwrap_or(0.0);
                    let percent = if total != 0.0 {
                        round_to(n / total * 100.0, 2)
                    } else if n <= 1.0 {
                        round_to(n * 100.0, 2)
                    } else {
                        n
                    };
                    format!("{percent}%")
                }
                Value::String(s) => s.clone(),
                other => return Err(format!("`{other}` is not a width.")),
            };
            slots += 1;
            if width.starts_with('-') {
                spacers += 1;
            }
            widths.push(width);
        }
        rows.push(widths);
    }

    // Not enough slots: repeat the last row.
    let needed = cell_count + spacers;
    if needed > slots
        && let Some(last) = rows.last().cloned()
        && !last.is_empty()
    {
        for _ in 0..(needed - slots).div_ceil(last.len()) {
            rows.push(last.clone());
        }
    }
    Ok(rows)
}

// ── Rendering ──────────────────────────────────────────────────────────────

/// Quarto 1 `layoutCellAlignment` and `flexAlign`.
fn justify_content(cell: &mut Cell, align: Option<&str>) -> &'static str {
    let align = align.unwrap_or_else(|| {
        if has_lone_image(&mut cell.div.content) || contains_table(&cell.div.content) {
            "center"
        } else {
            "left"
        }
    });
    match align {
        "center" => "center",
        "right" => "flex-end",
        _ => "flex-start",
    }
}

/// The row Divs of a panel (`layout/html.lua`).
fn render_rows(rows: Vec<Vec<Cell>>, spec: &LayoutSpec) -> Vec<Block> {
    rows.into_iter()
        .map(|row| {
            let cells = row
                .into_iter()
                .map(|mut cell| {
                    let justify = justify_content(&mut cell, spec.align.as_deref());
                    let mut style = String::new();
                    if let Some(width) = &cell.width {
                        style.push_str(&format!("flex-basis: {width};"));
                    }
                    style.push_str(&format!("justify-content: {justify};"));
                    let attr = &mut *cell.div.attr;
                    attr.1.push(CELL_CLASS.to_string());
                    attr.2.remove(LAYOUT_ALIGN);
                    attr.2.insert("style".to_string(), style);
                    Block::Div(Box::new(cell.div))
                })
                .collect();
            let mut classes = vec![ROW_CLASS.to_string()];
            if let Some(valign) = spec.valign_class {
                classes.push(valign.to_string());
            }
            Block::Div(Box::new(new_div(classes, cells)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: si(),
            })],
            source_info: si(),
        })
    }

    fn panel(kvs: &[(&str, &str)], content: Vec<Block>) -> Div {
        let mut map = hashlink::LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert(k.to_string(), v.to_string());
        }
        Div {
            attr: (String::new(), Vec::new(), map).into(),
            content,
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }
    }

    fn paras(n: usize) -> Vec<Block> {
        (1..=n).map(|i| para(&format!("c{i}"))).collect()
    }

    /// Build the panel and describe it: one string per row, the cells'
    /// `flex-basis` values joined by spaces (a spacer is marked `~`).
    fn widths(kvs: &[(&str, &str)], content: Vec<Block>) -> Vec<String> {
        let mut diagnostics = Vec::new();
        let blocks = build_panel(panel(kvs, content), false, &mut diagnostics);
        let Some(Block::Div(outer)) = blocks.last() else {
            panic!("expected the panel Div last");
        };
        assert!(has_class(outer, PANEL_CLASS));
        outer
            .content
            .iter()
            .map(|row| {
                let Block::Div(row) = row else {
                    panic!("row Div")
                };
                assert!(has_class(row, ROW_CLASS));
                row.content
                    .iter()
                    .map(|cell| {
                        let Block::Div(cell) = cell else {
                            panic!("cell Div")
                        };
                        assert!(has_class(cell, CELL_CLASS));
                        let style = cell.attr.2.get("style").expect("cell style");
                        let basis = style
                            .strip_prefix("flex-basis: ")
                            .and_then(|s| s.split(';').next())
                            .expect("flex-basis first");
                        if has_class(cell, SPACER_CLASS) {
                            format!("~{basis}")
                        } else {
                            basis.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    #[test]
    fn ncol_fills_rows_and_keeps_the_columns_of_a_short_row() {
        assert_eq!(widths(&[("layout-ncol", "2")], paras(2)), ["50.0% 50.0%"]);
        assert_eq!(
            widths(&[("layout-ncol", "3")], paras(5)),
            ["33.3% 33.3% 33.3%", "33.3% 33.3%"]
        );
    }

    #[test]
    fn nrow_derives_the_column_count() {
        assert_eq!(
            widths(&[("layout-nrow", "2")], paras(4)),
            ["50.0% 50.0%", "50.0% 50.0%"]
        );
        assert_eq!(
            widths(&[("layout-nrow", "2")], paras(5)),
            ["33.3% 33.3% 33.3%", "33.3% 33.3%"]
        );
    }

    #[test]
    fn ncol_wins_over_nrow_and_layout() {
        assert_eq!(
            widths(
                &[
                    ("layout-ncol", "1"),
                    ("layout-nrow", "1"),
                    ("layout", "[[1,1]]")
                ],
                paras(2)
            ),
            ["100.0%", "100.0%"]
        );
    }

    #[test]
    fn layout_rows_are_shares_of_each_row() {
        assert_eq!(
            widths(&[("layout", "[[1,1],[1]]")], paras(3)),
            ["50.0% 50.0%", "100.0%"]
        );
        assert_eq!(widths(&[("layout", "[30,70]")], paras(2)), ["30.0% 70.0%"]);
        assert_eq!(widths(&[("layout", "[[1,2]]")], paras(2)), ["33.3% 66.7%"]);
    }

    #[test]
    fn negative_width_is_a_spacer() {
        assert_eq!(
            widths(&[("layout", "[[40,-20,40],[100]]")], paras(3)),
            ["40.0% ~20.0% 40.0%", "100.0%"]
        );
    }

    #[test]
    fn layout_repeats_its_last_row_for_extra_cells() {
        assert_eq!(
            widths(&[("layout", "[[1],[1,1]]")], paras(5)),
            ["100.0%", "50.0% 50.0%", "50.0% 50.0%"]
        );
    }

    #[test]
    fn layout_stops_when_the_cells_run_out() {
        assert_eq!(
            widths(&[("layout", "[[1,1],[1,1],[1,1]]")], paras(3)),
            ["50.0% 50.0%", "50.0%"]
        );
    }

    #[test]
    fn layout_strings_pass_through() {
        assert_eq!(
            widths(&[("layout", r#"[["100px","45%"]]"#)], paras(2)),
            ["100px 45.0%"]
        );
    }

    #[test]
    fn bad_layout_warns_and_falls_back_to_one_column() {
        let mut diagnostics = Vec::new();
        let blocks = build_panel(
            panel(&[("layout", "[[1,1],2]")], paras(2)),
            false,
            &mut diagnostics,
        );
        assert_eq!(diagnostics.len(), 1);
        let Some(Block::Div(outer)) = blocks.last() else {
            panic!("panel Div");
        };
        assert_eq!(outer.content.len(), 2, "one cell per row");
    }

    #[test]
    fn alignment_default_depends_on_the_cell() {
        let image = Block::Paragraph(Paragraph {
            content: vec![Inline::Image(Box::new(Image {
                attr: quarto_pandoc_types::attr::empty_attr(),
                content: Vec::new(),
                target: ("x.png".to_string(), String::new()),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
                target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
            }))],
            source_info: si(),
        });
        let justify = |kvs: &[(&str, &str)], content: Vec<Block>| -> Vec<String> {
            let mut diagnostics = Vec::new();
            let blocks = build_panel(panel(kvs, content), false, &mut diagnostics);
            let Some(Block::Div(outer)) = blocks.last() else {
                panic!("panel Div");
            };
            let Block::Div(row) = &outer.content[0] else {
                panic!("row Div");
            };
            row.content
                .iter()
                .map(|cell| {
                    let Block::Div(cell) = cell else {
                        panic!("cell Div")
                    };
                    let style = cell.attr.2.get("style").unwrap();
                    style
                        .split("justify-content: ")
                        .nth(1)
                        .unwrap()
                        .trim_end_matches(';')
                        .to_string()
                })
                .collect()
        };
        assert_eq!(
            justify(&[("layout-ncol", "2")], vec![image.clone(), para("text")]),
            ["center", "flex-start"]
        );
        assert_eq!(
            justify(
                &[("layout-ncol", "2"), ("layout-align", "right")],
                vec![image, para("text")]
            ),
            ["flex-end", "flex-end"]
        );
    }

    #[test]
    fn valign_becomes_a_row_class_and_leaves_the_panel() {
        let mut diagnostics = Vec::new();
        let blocks = build_panel(
            panel(
                &[("layout-ncol", "2"), ("layout-valign", "bottom")],
                paras(2),
            ),
            false,
            &mut diagnostics,
        );
        let Some(Block::Div(outer)) = blocks.last() else {
            panic!("panel Div");
        };
        assert!(!outer.attr.2.contains_key(LAYOUT_VALIGN));
        assert_eq!(outer.attr.2.get(LAYOUT_NCOL).map(String::as_str), Some("2"));
        let Block::Div(row) = &outer.content[0] else {
            panic!("row Div");
        };
        assert!(has_class(row, "quarto-layout-valign-bottom"));
    }

    #[test]
    fn child_div_is_the_cell_and_a_figure_wrapper_is_wrapped() {
        let user = Block::Div(Box::new(Div {
            attr: (
                String::new(),
                vec!["mine".to_string()],
                hashlink::LinkedHashMap::new(),
            )
                .into(),
            content: vec![para("a")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }));
        let figure = Block::Div(Box::new(Div {
            attr: (
                String::new(),
                vec!["quarto-figure".to_string()],
                hashlink::LinkedHashMap::new(),
            )
                .into(),
            content: vec![para("b")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }));
        let mut diagnostics = Vec::new();
        let blocks = build_panel(
            panel(&[("layout-ncol", "2")], vec![user, figure]),
            false,
            &mut diagnostics,
        );
        let Some(Block::Div(outer)) = blocks.last() else {
            panic!("panel Div");
        };
        let Block::Div(row) = &outer.content[0] else {
            panic!("row Div");
        };
        let Block::Div(first) = &row.content[0] else {
            panic!("cell Div");
        };
        assert_eq!(first.attr.1, ["mine", CELL_CLASS]);
        let Block::Div(second) = &row.content[1] else {
            panic!("cell Div");
        };
        assert_eq!(second.attr.1, [CELL_CLASS]);
        assert!(matches!(&second.content[0], Block::Div(d) if has_class(d, "quarto-figure")));
    }

    #[test]
    fn a_div_holding_a_panel_is_left_alone() {
        let inner = panel(&[("layout-ncol", "2")], paras(2));
        let mut list = vec![Block::Div(Box::new(panel(
            &[("layout-ncol", "1")],
            vec![Block::Div(Box::new(inner))],
        )))];
        let mut diagnostics = Vec::new();
        for_each_block_list_mut(&mut list, &mut |l| {
            layout_block_list(l, false, &mut diagnostics);
        });
        let Block::Div(outer) = &list[0] else {
            panic!("outer Div");
        };
        assert!(!has_class(outer, PANEL_CLASS));
        assert!(matches!(&outer.content[0], Block::Div(d) if has_class(d, PANEL_CLASS)));
    }
}

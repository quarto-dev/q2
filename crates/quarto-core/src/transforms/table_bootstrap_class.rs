/*
 * transforms/table_bootstrap_class.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Bootstrap classes on every `Block::Table` in the AST.
//!
//! Mirrors three Quarto 1 filters:
//!
//! - `quarto-bootstrap-table.lua`: every table gets `table` and
//!   `caption-top`. Quarto's default HTML output ships a Bootstrap
//!   stylesheet whose table styling is keyed off `.table`; the caption
//!   position is keyed off `.caption-top`. Pandoc's HTML writer emits a
//!   bare `<table>` by default, so without this enrichment the rendered
//!   table inherits no Bootstrap styling (bd-2c8rg; see
//!   `claude-notes/plans/2026-05-20-table-default-rendering-parity.md`, D2).
//! - `quarto-pre/table-classes.lua`: the Bootstrap shorthands an author
//!   writes in a caption's attribute block (`: cap {.striped .hover}`) are
//!   prefixed to `table-striped table-hover`, `table-sm` and `small` pull
//!   each other in, and a `tbl` float's classes are forwarded onto the
//!   tables it holds (bd-hfghrqvh).
//! - `quarto-post/responsive.lua` `responsive_table()`: a
//!   `.responsive(-*)` table is wrapped in `div.table-responsive(-*)`.
//!
//! The float forwarding is a separate, earlier transform
//! ([`TableFloatClassForwardTransform`]) because the float's own classes
//! are only distinguishable before `crossref-render`, which folds them
//! into the `quarto-float` wrapper Div. The forwarded copies are left
//! unnormalized; [`TableBootstrapClassTransform`] normalizes every table's
//! classes in one place. The float keeps its own unprefixed copies, as in
//! Quarto 1, so CSS written against the float div keeps working.
//!
//! Pipeline placement: both in the **FINALIZATION PHASE** of the HTML
//! pipeline. Q1 normalizes for every format in its pre stage; q2 does it
//! HTML-only, a reviewed divergence (bd-7zw93czl tracks the all-format
//! version). Running late keeps the AST clean for upstream transforms /
//! user filters that might inspect table classes for their own reasons,
//! and the responsive wrapper must not exist yet when `crossref-render`
//! looks for a float's bare top-level `Table`.

use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div};
use quarto_pandoc_types::custom::CustomNode;
use quarto_pandoc_types::custom::Slot;
use quarto_pandoc_types::inline::Inlines;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::ast_walk::{
    ListVisitor, block_children_mut, for_each_block_list_mut, inline_children_mut,
};
use crate::crossref::FLOAT_REF_TARGET;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Bootstrap class added to every `<table>` so the Bootstrap stylesheet
/// can theme it. Matches Quarto 1.
const TABLE_CLASS: &str = "table";
/// Class that flips the Pandoc default caption position from `bottom`
/// (browser default for `<table>`) to `top`. Matches Quarto 1.
const CAPTION_TOP_CLASS: &str = "caption-top";

/// Shorthands that get a `table-` prefix, verbatim from Quarto 1's
/// `table_bootstrap_nm` (`quarto-pre/table-classes.lua`).
const BOOTSTRAP_SHORTHANDS: &[&str] = &[
    "primary",
    "secondary",
    "success",
    "danger",
    "warning",
    "info",
    "light",
    "dark",
    "striped",
    "hover",
    "active",
    "bordered",
    "borderless",
    "sm",
    "responsive",
    "responsive-sm",
    "responsive-md",
    "responsive-lg",
    "responsive-xl",
    "responsive-xxl",
];

/// Classes that move off the table onto a wrapper Div (Quarto 1's
/// `responsive_table()`). Matched after prefixing, so an authored
/// `.responsive-md` and an explicit `.table-responsive-md` both land here.
const RESPONSIVE_CLASSES: &[&str] = &[
    "table-responsive",
    "table-responsive-sm",
    "table-responsive-md",
    "table-responsive-lg",
    "table-responsive-xl",
    "table-responsive-xxl",
];

pub struct TableBootstrapClassTransform;

impl TableBootstrapClassTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TableBootstrapClassTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for TableBootstrapClassTransform {
    fn name(&self) -> &str {
        "table-bootstrap-class"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        apply_to_blocks(&mut ast.blocks);
        Ok(())
    }
}

/// Forward a `tbl` float's classes onto the tables it holds (Quarto 1's
/// `table_classes()` `FloatRefTarget` handler). Runs just before
/// `crossref-render`; see the module doc.
pub struct TableFloatClassForwardTransform;

impl TableFloatClassForwardTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TableFloatClassForwardTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for TableFloatClassForwardTransform {
    fn name(&self) -> &str {
        "table-float-class-forward"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        forward_float_classes(&mut ast.blocks);
        Ok(())
    }
}

/// Apply the class enrichment to every `Block::Table` reachable from
/// `blocks`: nested in divs, blockquotes, figures, lists, other tables'
/// cells, and custom nodes' slots (q2-preview keeps `FloatRefTarget` as a
/// custom node, since it skips `crossref-render`).
fn apply_to_blocks(blocks: &mut Vec<Block>) {
    for_each_block_list_mut(blocks, &mut |list| {
        for block in list.iter_mut() {
            if let Block::Table(table) = &mut *block {
                let classes = &mut table.attr.1;
                normalize_classes(classes);
                ensure_class(classes, CAPTION_TOP_CLASS);
                ensure_class(classes, TABLE_CLASS);
                if let Some(pos) = classes
                    .iter()
                    .position(|c| RESPONSIVE_CLASSES.contains(&c.as_str()))
                {
                    let wrapper_class = classes.remove(pos);
                    wrap_in_div(block, wrapper_class);
                }
            }
        }
    });
}

/// Prefix the Bootstrap shorthands in place and pair `table-sm` with
/// `small` (Q1 adds `small` for `table-sm`, else `table-sm` for `small`).
fn normalize_classes(classes: &mut Vec<String>) {
    for class in classes.iter_mut() {
        if BOOTSTRAP_SHORTHANDS.contains(&class.as_str()) {
            *class = format!("table-{class}");
        }
    }
    if classes.iter().any(|c| c == "table-sm") {
        ensure_class(classes, "small");
    } else if classes.iter().any(|c| c == "small") {
        ensure_class(classes, "table-sm");
    }
}

/// Replace `block` with `Div(.class) > block`.
fn wrap_in_div(block: &mut Block, class: String) {
    let source_info = block.source_info().clone();
    let inner = std::mem::replace(
        block,
        Block::Div(Box::new(Div {
            attr: (String::new(), vec![class], hashlink::LinkedHashMap::new()).into(),
            content: Vec::new(),
            source_info,
            attr_source: AttrSourceInfo::empty(),
        })),
    );
    if let Block::Div(div) = block {
        div.content.push(inner);
    }
}

/// Forward every `tbl` float's classes onto the tables in its `content`
/// slot, at any depth but never into a nested float (a sub-float's tables
/// take only the sub-float's classes).
fn forward_float_classes(blocks: &mut Vec<Block>) {
    for_each_block_list_mut(blocks, &mut |list| {
        for block in list.iter_mut() {
            let Block::Custom(node) = block else { continue };
            if !is_table_float(node) || node.attr.1.is_empty() {
                continue;
            }
            let classes = node.attr.1.clone();
            if let Some(Slot::Blocks(content)) = node.slots.get_mut("content") {
                ForwardClasses { classes: &classes }.blocks(content);
            }
        }
    });
}

fn is_table_float(node: &CustomNode) -> bool {
    node.type_name == FLOAT_REF_TARGET
        && node.plain_data.get("ref_type").and_then(|v| v.as_str()) == Some("tbl")
}

/// Top-down walk that adds `classes` to every table it meets and stops at
/// nested floats.
struct ForwardClasses<'a> {
    classes: &'a [String],
}

impl ListVisitor for ForwardClasses<'_> {
    fn inlines(&mut self, list: &mut Inlines) {
        for inline in list.iter_mut() {
            inline_children_mut(inline, self);
        }
    }

    fn blocks(&mut self, list: &mut Vec<Block>) {
        for block in list.iter_mut() {
            match &mut *block {
                Block::Custom(node) if node.type_name == FLOAT_REF_TARGET => continue,
                Block::Table(table) => {
                    for class in self.classes {
                        ensure_class(&mut table.attr.1, class);
                    }
                }
                _ => {}
            }
            block_children_mut(block, self);
        }
    }
}

/// Push `class` onto `classes` if it isn't already present. Preserves
/// insertion order, so first-runs are appended after existing user
/// classes (idempotent on second run).
fn ensure_class(classes: &mut Vec<String>, class: &str) {
    if !classes.iter().any(|c| c == class) {
        classes.push(class.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::{Block, Div, Plain};
    use quarto_pandoc_types::caption::Caption;
    use quarto_pandoc_types::custom::{CustomNode, Slot};
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_pandoc_types::table::{
        Alignment, Cell, Row, Table, TableBody, TableFoot, TableHead,
    };
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    fn empty_attr() -> (String, Vec<String>, LinkedHashMap<String, String>) {
        (String::new(), Vec::new(), LinkedHashMap::new())
    }

    fn table_with_classes(classes: Vec<&str>) -> Block {
        let attr = (
            String::new(),
            classes.into_iter().map(|s| s.to_string()).collect(),
            LinkedHashMap::new(),
        );
        Block::Table(Box::new(Table {
            attr: attr.into(),
            caption: Caption {
                short: None,
                long: None,
                source_info: dummy_source_info(),
            },
            colspec: vec![],
            head: TableHead {
                attr: empty_attr().into(),
                rows: vec![],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            },
            bodies: vec![TableBody {
                attr: empty_attr().into(),
                rowhead_columns: 0,
                head: vec![],
                body: vec![Row {
                    attr: empty_attr().into(),
                    cells: vec![Cell {
                        attr: empty_attr().into(),
                        alignment: Alignment::Default,
                        row_span: 1,
                        col_span: 1,
                        content: vec![Block::Plain(Plain {
                            content: vec![Inline::Str(Str {
                                text: "x".into(),
                                source_info: dummy_source_info(),
                            })],
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                        attr_source: AttrSourceInfo::empty(),
                    }],
                    source_info: dummy_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                }],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            }],
            foot: TableFoot {
                attr: empty_attr().into(),
                rows: vec![],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            },
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn ast_with(blocks: Vec<Block>) -> Pandoc {
        Pandoc {
            meta: ConfigValue::default(),
            blocks,
        }
    }

    fn table_classes(block: &Block) -> &[String] {
        match block {
            Block::Table(t) => &t.attr.1,
            _ => panic!("not a table"),
        }
    }

    #[test]
    fn bare_table_gets_both_classes() {
        let mut ast = ast_with(vec![table_with_classes(vec![])]);
        apply_to_blocks(&mut ast.blocks);
        assert_eq!(
            table_classes(&ast.blocks[0]),
            &["caption-top".to_string(), "table".to_string()]
        );
    }

    #[test]
    fn existing_user_class_is_preserved() {
        let mut ast = ast_with(vec![table_with_classes(vec!["my-class"])]);
        apply_to_blocks(&mut ast.blocks);
        assert_eq!(
            table_classes(&ast.blocks[0]),
            &[
                "my-class".to_string(),
                "caption-top".to_string(),
                "table".to_string()
            ]
        );
    }

    #[test]
    fn idempotent_on_second_run() {
        let mut ast = ast_with(vec![table_with_classes(vec![])]);
        apply_to_blocks(&mut ast.blocks);
        let first = table_classes(&ast.blocks[0]).to_vec();
        apply_to_blocks(&mut ast.blocks);
        let second = table_classes(&ast.blocks[0]).to_vec();
        assert_eq!(first, second, "running twice must not duplicate classes");
    }

    #[test]
    fn already_classed_table_unchanged() {
        let mut ast = ast_with(vec![table_with_classes(vec!["caption-top", "table"])]);
        apply_to_blocks(&mut ast.blocks);
        assert_eq!(
            table_classes(&ast.blocks[0]),
            &["caption-top".to_string(), "table".to_string()]
        );
    }

    #[test]
    fn one_of_two_classes_present_only_other_added() {
        let mut ast = ast_with(vec![table_with_classes(vec!["table"])]);
        apply_to_blocks(&mut ast.blocks);
        assert_eq!(
            table_classes(&ast.blocks[0]),
            &["table".to_string(), "caption-top".to_string()]
        );
    }

    #[test]
    fn table_nested_in_div_is_reached() {
        let div = Block::Div(Box::new(Div {
            attr: empty_attr().into(),
            content: vec![table_with_classes(vec![])],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        }));
        let mut ast = ast_with(vec![div]);
        apply_to_blocks(&mut ast.blocks);
        let Block::Div(d) = &ast.blocks[0] else {
            panic!("not a div");
        };
        assert_eq!(
            table_classes(&d.content[0]),
            &["caption-top".to_string(), "table".to_string()]
        );
    }

    fn strings(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    fn float_node(ref_type: &str, classes: Vec<&str>, content: Vec<Block>) -> Block {
        let attr = (
            format!("{ref_type}-x"),
            strings(&classes),
            LinkedHashMap::new(),
        );
        Block::Custom(Box::new(
            CustomNode::new(FLOAT_REF_TARGET, attr.into(), dummy_source_info())
                .with_data(serde_json::json!({ "ref_type": ref_type }))
                .with_slot("content", Slot::Blocks(content)),
        ))
    }

    fn float_content(block: &Block) -> &[Block] {
        let Block::Custom(node) = block else {
            panic!("not a custom node");
        };
        let Some(Slot::Blocks(content)) = node.slots.get("content") else {
            panic!("no content slot");
        };
        content
    }

    #[test]
    fn every_shorthand_is_prefixed_and_other_classes_pass_through() {
        let mut classes: Vec<&str> = BOOTSTRAP_SHORTHANDS
            .iter()
            .copied()
            .filter(|c| !c.starts_with("responsive"))
            .collect();
        classes.push("my-class");
        let mut ast = ast_with(vec![table_with_classes(classes.clone())]);
        apply_to_blocks(&mut ast.blocks);
        let mut expected: Vec<String> = classes
            .iter()
            .map(|c| {
                if *c == "my-class" {
                    c.to_string()
                } else {
                    format!("table-{c}")
                }
            })
            .collect();
        // `.sm` pulls in `small`.
        expected.extend(strings(&["small", "caption-top", "table"]));
        assert_eq!(table_classes(&ast.blocks[0]), expected.as_slice());
    }

    #[test]
    fn caption_shorthands_match_q1_order() {
        let mut ast = ast_with(vec![table_with_classes(vec![
            "striped", "hover", "my-class",
        ])]);
        apply_to_blocks(&mut ast.blocks);
        assert_eq!(
            table_classes(&ast.blocks[0]),
            strings(&[
                "table-striped",
                "table-hover",
                "my-class",
                "caption-top",
                "table"
            ])
        );
    }

    #[test]
    fn sm_adds_small_and_small_adds_table_sm() {
        let mut ast = ast_with(vec![
            table_with_classes(vec!["bordered", "sm"]),
            table_with_classes(vec!["small"]),
        ]);
        apply_to_blocks(&mut ast.blocks);
        assert_eq!(
            table_classes(&ast.blocks[0]),
            strings(&[
                "table-bordered",
                "table-sm",
                "small",
                "caption-top",
                "table"
            ])
        );
        assert_eq!(
            table_classes(&ast.blocks[1]),
            strings(&["small", "table-sm", "caption-top", "table"])
        );
    }

    #[test]
    fn responsive_table_is_wrapped_and_loses_the_class() {
        for (authored, wrapper) in [
            ("responsive-md", "table-responsive-md"),
            ("responsive", "table-responsive"),
            ("table-responsive-xl", "table-responsive-xl"),
        ] {
            let mut ast = ast_with(vec![table_with_classes(vec![authored, "primary"])]);
            apply_to_blocks(&mut ast.blocks);
            let Block::Div(d) = &ast.blocks[0] else {
                panic!("{authored}: table was not wrapped");
            };
            assert_eq!(d.attr.1, strings(&[wrapper]), "{authored}");
            assert_eq!(d.content.len(), 1);
            assert_eq!(
                table_classes(&d.content[0]),
                strings(&["table-primary", "caption-top", "table"]),
                "{authored}"
            );
        }
    }

    #[test]
    fn responsive_wrap_is_idempotent() {
        let mut ast = ast_with(vec![table_with_classes(vec!["responsive", "sm"])]);
        apply_to_blocks(&mut ast.blocks);
        let first = ast.blocks.clone();
        apply_to_blocks(&mut ast.blocks);
        assert_eq!(first, ast.blocks, "a second run must not re-wrap or re-add");
    }

    #[test]
    fn table_inside_custom_node_slot_is_reached() {
        let mut ast = ast_with(vec![float_node(
            "tbl",
            vec![],
            vec![table_with_classes(vec!["hover"])],
        )]);
        apply_to_blocks(&mut ast.blocks);
        assert_eq!(
            table_classes(&float_content(&ast.blocks[0])[0]),
            strings(&["table-hover", "caption-top", "table"])
        );
    }

    #[test]
    fn tbl_float_classes_are_forwarded_to_its_table_and_kept_on_the_float() {
        let mut ast = ast_with(vec![float_node(
            "tbl",
            vec!["bordered", "my-class"],
            vec![table_with_classes(vec!["sm"])],
        )]);
        forward_float_classes(&mut ast.blocks);
        assert_eq!(
            table_classes(&float_content(&ast.blocks[0])[0]),
            strings(&["sm", "bordered", "my-class"])
        );
        let Block::Custom(node) = &ast.blocks[0] else {
            unreachable!()
        };
        assert_eq!(node.attr.1, strings(&["bordered", "my-class"]));
    }

    #[test]
    fn every_table_in_a_multi_table_float_gets_the_classes() {
        let layout = Block::Div(Box::new(Div {
            attr: empty_attr().into(),
            content: vec![table_with_classes(vec![])],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        }));
        let mut ast = ast_with(vec![float_node(
            "tbl",
            vec!["hover"],
            vec![table_with_classes(vec![]), layout],
        )]);
        forward_float_classes(&mut ast.blocks);
        let content = float_content(&ast.blocks[0]);
        assert_eq!(table_classes(&content[0]), strings(&["hover"]));
        let Block::Div(d) = &content[1] else {
            panic!("not a div")
        };
        assert_eq!(table_classes(&d.content[0]), strings(&["hover"]));
    }

    #[test]
    fn sub_float_tables_take_only_the_sub_float_classes() {
        let sub = float_node("tbl", vec!["dark"], vec![table_with_classes(vec![])]);
        let mut ast = ast_with(vec![float_node(
            "tbl",
            vec!["hover"],
            vec![table_with_classes(vec![]), sub],
        )]);
        forward_float_classes(&mut ast.blocks);
        let content = float_content(&ast.blocks[0]);
        assert_eq!(table_classes(&content[0]), strings(&["hover"]));
        assert_eq!(
            table_classes(&float_content(&content[1])[0]),
            strings(&["dark"])
        );
    }

    #[test]
    fn non_table_floats_are_not_forwarded() {
        let mut ast = ast_with(vec![float_node(
            "fig",
            vec!["striped"],
            vec![table_with_classes(vec![])],
        )]);
        forward_float_classes(&mut ast.blocks);
        assert!(table_classes(&float_content(&ast.blocks[0])[0]).is_empty());
    }

    #[test]
    fn forwarding_is_idempotent() {
        let mut ast = ast_with(vec![float_node(
            "tbl",
            vec!["striped"],
            vec![table_with_classes(vec!["striped"])],
        )]);
        forward_float_classes(&mut ast.blocks);
        forward_float_classes(&mut ast.blocks);
        assert_eq!(
            table_classes(&float_content(&ast.blocks[0])[0]),
            strings(&["striped"])
        );
    }
}

/*
 * callout_resolve.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that resolves Callout CustomNodes to standard Pandoc AST.
 */

//! Callout resolution transform.
//!
//! This transform converts Callout CustomNodes back to standard Pandoc AST
//! (Div blocks with appropriate structure). This separation allows:
//!
//! 1. The HTML writer to remain generic (no knowledge of callout semantics)
//! 2. Different resolve transforms to produce different HTML structures
//! 3. A single source of HTML-writing behavior in the codebase
//!
//! ## Pipeline Order
//!
//! This transform should run AFTER `CalloutTransform`:
//! 1. `CalloutTransform`: Div with `.callout-*` → CustomNode("Callout")
//! 2. `CalloutResolveTransform`: CustomNode("Callout") → Div with HTML structure
//!
//! ## Output Structure
//!
//! The transform produces Div structures matching what TS Quarto's
//! `src/resources/filters/modules/callouts.lua` (`render_to_bootstrap_div`)
//! emits, so the bundled Bootstrap SCSS in
//! `resources/scss/bootstrap/_bootstrap-rules.scss` applies cleanly.
//!
//! **Titled callout** (user title OR appearance=default + injected display name):
//!
//! ```text
//! Div.callout.callout-style-{appearance}.callout-{type}.callout-titled[.no-icon][.callout-empty-content]
//!   Div.callout-header.d-flex.align-content-center[.collapsed]
//!     Div.callout-icon-container      (when icon=true)
//!       Plain[RawInline(html, "<i class=\"callout-icon\"></i>")]
//!     Div.callout-title-container.flex-fill
//!       Plain[title inlines...]
//!     Plain[<div class="callout-btn-toggle ...">...]   (when collapse=true|false)
//!   Div.callout-body-container.callout-body            (when no collapse)
//!     [content blocks...]
//!   OR
//!   Div.callout-collapse.collapse[.show]               (when collapse=true|false)
//!     Div.callout-body-container.callout-body
//!       [content blocks...]
//! ```
//!
//! **Untitled callout** (appearance!=default + no user title):
//!
//! ```text
//! Div.callout.callout-style-{appearance}.callout-{type}[.no-icon][.callout-empty-content]
//!   Div.callout-body.d-flex
//!     Div.callout-icon-container      (when icon=true)
//!       Plain[RawInline(html, "<i class=\"callout-icon\"></i>")]
//!     Div.callout-body-container
//!       [content blocks...]
//! ```

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Div, Plain};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{Inline, RawInline, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;
use serde_json::Value;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that resolves Callout CustomNodes to standard Pandoc Div structure.
///
/// This enables the HTML writer to remain generic while still producing
/// the expected callout HTML structure.
pub struct CalloutResolveTransform;

impl CalloutResolveTransform {
    /// Create a new callout resolve transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CalloutResolveTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CalloutResolveTransform {
    fn name(&self) -> &str {
        "callout-resolve"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // Document-scoped counter for generating unique `callout-N-contents`
        // ids on collapsible callouts. Starts at 1 to match TS Quarto's
        // `calloutidx` (`src/resources/filters/modules/callouts.lua`).
        let mut counter = 1u32;
        resolve_blocks(&mut ast.blocks, &mut counter);
        Ok(())
    }
}

/// Resolve CustomNodes in a vector of blocks.
fn resolve_blocks(blocks: &mut Vec<Block>, counter: &mut u32) {
    for block in blocks.iter_mut() {
        resolve_block(block, counter);
    }
}

/// Resolve a single block, potentially converting CustomNode to Div.
fn resolve_block(block: &mut Block, counter: &mut u32) {
    // First, recursively resolve any nested blocks
    match block {
        Block::BlockQuote(bq) => {
            resolve_blocks(&mut bq.content, counter);
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                resolve_blocks(item, counter);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                resolve_blocks(item, counter);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    resolve_blocks(def, counter);
                }
            }
        }
        Block::Figure(fig) => {
            resolve_blocks(&mut fig.content, counter);
        }
        Block::Div(div) => {
            resolve_blocks(&mut div.content, counter);
        }
        Block::Table(table) => {
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        resolve_blocks(&mut cell.content, counter);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content, counter);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content, counter);
                }
            }
        }
        Block::Custom(custom) => {
            // First resolve any nested blocks in slots
            for (_name, slot) in &mut custom.slots {
                match slot {
                    Slot::Block(b) => resolve_block(b, counter),
                    Slot::Blocks(bs) => resolve_blocks(bs, counter),
                    _ => {}
                }
            }

            // Then check if this is a Callout that should be resolved
            if custom.type_name == "Callout" {
                let resolved_div = resolve_callout(custom, counter);
                *block = Block::Div(resolved_div);
            }
        }
        // Other block types don't contain nested blocks
        _ => {}
    }
}

/// Resolve a Callout CustomNode to a Div matching the TS Quarto /
/// Bootstrap HTML structure (see module doc-comment).
///
/// `counter` is bumped by 1 whenever a collapsible callout consumes
/// it for the `callout-N-contents` id.
fn resolve_callout(custom: &mut CustomNode, counter: &mut u32) -> Div {
    // Extract callout properties from plain_data. Appearance is
    // already normalized at CalloutTransform time (minimal → simple +
    // icon=false), so the resolver only sees `default` or `simple`.
    // Owned copies up-front so the `&custom.plain_data` borrow is
    // released before `extract_content_blocks` takes `&mut custom`
    // below (NLL would handle this for a single short-lived borrow,
    // but `callout_type` / `appearance` are used after the mutating
    // call, so we clone now).
    let callout_type = extract_string(&custom.plain_data, "type")
        .unwrap_or("note")
        .to_string();
    let raw_appearance = extract_string(&custom.plain_data, "appearance")
        .unwrap_or("default")
        .to_string();
    let collapse = extract_bool(&custom.plain_data, "collapse").unwrap_or(false);
    let collapse_starts_collapsed =
        extract_bool(&custom.plain_data, "collapse_starts_collapsed").unwrap_or(false);
    let raw_icon = extract_bool(&custom.plain_data, "icon").unwrap_or(true);

    // Defense-in-depth normalization (`appearance="minimal"` →
    // `simple` + `icon=false`). CalloutTransform already does this
    // upstream, but the resolver also accepts CustomNodes synthesized
    // by other code (tests, eventual filter authors), so we
    // re-apply it here. Matches TS Quarto's `nameForCalloutStyle`.
    let (appearance, icon) = if raw_appearance == "minimal" {
        ("simple".to_string(), false)
    } else {
        (raw_appearance, raw_icon)
    };

    let source_info = custom.source_info.clone();

    // Pull title and content out of the custom node.
    let user_title = extract_user_title(custom);
    let content_blocks = extract_content_blocks(custom);
    let has_content = !content_blocks.is_empty();

    // Default-title injection (per `callouts.lua:224-227`):
    // `appearance="default"` + empty title → inject the type's display name.
    // `appearance="simple"` keeps the empty title, taking the untitled path.
    let title_inlines: Option<Vec<Inline>> = match user_title {
        Some(t) if !t.is_empty() => Some(t),
        _ if appearance == "default" => Some(vec![Inline::Str(Str {
            text: capitalize(&callout_type),
            source_info: SourceInfo::default(),
        })]),
        _ => None,
    };
    let is_titled = title_inlines.is_some();

    // Outer div classes.
    let mut classes = vec![
        "callout".to_string(),
        format!("callout-style-{}", appearance),
        format!("callout-{}", callout_type),
    ];
    if !icon {
        classes.push("no-icon".to_string());
    }
    if is_titled {
        classes.push("callout-titled".to_string());
    }
    if !has_content {
        classes.push("callout-empty-content".to_string());
    }

    // Preserve original non-callout classes from the source attr.
    let (orig_id, orig_classes, orig_attrs) = &custom.attr;
    for cls in orig_classes {
        if !cls.starts_with("callout") {
            classes.push(cls.clone());
        }
    }
    let outer_attr: Attr = (orig_id.clone(), classes, orig_attrs.clone());

    // Reserve a unique id for the collapse wrapper up-front so the
    // header's `bs-target` / `aria-controls` match what we apply to
    // the wrapper. Only consumed if collapse is enabled.
    let collapse_id = if collapse {
        let id = format!("callout-{}-contents", *counter);
        *counter += 1;
        Some(id)
    } else {
        None
    };

    let body_inner_div = Div {
        attr: make_attr(&["callout-body-container"]),
        content: content_blocks,
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    };

    let outer_content = if is_titled {
        build_titled_content(
            &source_info,
            title_inlines.expect("is_titled => title_inlines is Some"),
            icon,
            collapse,
            collapse_starts_collapsed,
            collapse_id.as_deref(),
            body_inner_div,
        )
    } else {
        build_untitled_content(&source_info, icon, body_inner_div)
    };

    Div {
        attr: outer_attr,
        content: outer_content,
        source_info,
        attr_source: AttrSourceInfo::empty(),
    }
}

/// Construct the children of the outer Div for the titled-callout path.
fn build_titled_content(
    source_info: &SourceInfo,
    title_inlines: Vec<Inline>,
    icon: bool,
    collapse: bool,
    collapse_starts_collapsed: bool,
    collapse_id: Option<&str>,
    body_inner_div: Div,
) -> Vec<Block> {
    let mut header_content = Vec::new();

    if icon {
        header_content.push(Block::Div(icon_container_div(source_info)));
    }

    // Title container.
    header_content.push(Block::Div(Div {
        attr: make_attr(&["callout-title-container", "flex-fill"]),
        content: vec![Block::Plain(Plain {
            content: title_inlines,
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    }));

    // Header: classes + (when collapsible) Bootstrap toggle attrs.
    let mut header_classes: Vec<String> = vec![
        "callout-header".to_string(),
        "d-flex".to_string(),
        "align-content-center".to_string(),
    ];
    let mut header_attrs = LinkedHashMap::new();
    if collapse {
        let collapse_id = collapse_id.expect("collapse=true => collapse_id is Some");
        if collapse_starts_collapsed {
            header_classes.push("collapsed".to_string());
        }
        header_attrs.insert("bs-toggle".to_string(), "collapse".to_string());
        header_attrs.insert("bs-target".to_string(), format!(".{}", collapse_id));
        header_attrs.insert("aria-controls".to_string(), collapse_id.to_string());
        header_attrs.insert(
            "aria-expanded".to_string(),
            if collapse_starts_collapsed {
                "false"
            } else {
                "true"
            }
            .to_string(),
        );
        header_attrs.insert("aria-label".to_string(), "Toggle callout".to_string());

        // Trailing toggle button.
        header_content.push(Block::Plain(Plain {
            content: vec![Inline::RawInline(RawInline {
                format: "html".to_string(),
                text: "<div class=\"callout-btn-toggle d-inline-block border-0 py-1 ps-1 pe-0 \
                       float-end\"><i class=\"callout-toggle\"></i></div>"
                    .to_string(),
                source_info: source_info.clone(),
            })],
            source_info: source_info.clone(),
        }));
    }
    let header_div = Block::Div(Div {
        attr: (String::new(), header_classes, header_attrs),
        content: header_content,
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });

    // Body. With collapse, wrap the body-container in a
    // `.callout-collapse.collapse[.show]` div; without collapse, the
    // body-container itself takes the `callout-body` class.
    let body_block = if collapse {
        let collapse_id = collapse_id.expect("collapse=true => collapse_id is Some");
        let mut collapse_classes = vec![
            collapse_id.to_string(),
            "callout-collapse".to_string(),
            "collapse".to_string(),
        ];
        if !collapse_starts_collapsed {
            collapse_classes.push("show".to_string());
        }
        // Body-container inside the collapse wrapper carries
        // `callout-body-container callout-body` (matches TS Quarto's
        // titled+collapse output).
        let mut body_with_class = body_inner_div;
        body_with_class.attr.1.push("callout-body".to_string());
        Block::Div(Div {
            attr: (
                collapse_id.to_string(),
                collapse_classes,
                LinkedHashMap::new(),
            ),
            content: vec![Block::Div(body_with_class)],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        })
    } else {
        let mut body_with_class = body_inner_div;
        body_with_class.attr.1.push("callout-body".to_string());
        Block::Div(body_with_class)
    };

    vec![header_div, body_block]
}

/// Construct the children of the outer Div for the untitled-callout path.
///
/// Untitled callouts are flat: a single `<div class="callout-body d-flex">`
/// wrapping the icon container and the body-container.
fn build_untitled_content(source_info: &SourceInfo, icon: bool, body_inner_div: Div) -> Vec<Block> {
    let mut body_content = Vec::new();
    if icon {
        body_content.push(Block::Div(icon_container_div(source_info)));
    }
    body_content.push(Block::Div(body_inner_div));

    let body_outer = Div {
        attr: make_attr(&["callout-body", "d-flex"]),
        content: body_content,
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    };
    vec![Block::Div(body_outer)]
}

fn icon_container_div(source_info: &SourceInfo) -> Div {
    Div {
        attr: make_attr(&["callout-icon-container"]),
        content: vec![Block::Plain(Plain {
            content: vec![Inline::RawInline(RawInline {
                format: "html".to_string(),
                text: "<i class=\"callout-icon\"></i>".to_string(),
                source_info: source_info.clone(),
            })],
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    }
}

/// Pull the user-supplied title inlines out of the CustomNode, if any.
/// Returns None when the title slot is absent or empty — that's the
/// signal for the resolver to decide between default-title injection
/// (appearance=default) and the untitled path (appearance=simple).
fn extract_user_title(custom: &CustomNode) -> Option<Vec<Inline>> {
    match custom.get_slot("title")? {
        Slot::Inlines(inlines) if !inlines.is_empty() => Some(inlines.clone()),
        Slot::Inline(inline) => Some(vec![inline.as_ref().clone()]),
        _ => None,
    }
}

/// Extract content blocks from the CustomNode.
fn extract_content_blocks(custom: &mut CustomNode) -> Vec<Block> {
    if let Some(content_slot) = custom.slots.remove("content") {
        match content_slot {
            Slot::Blocks(blocks) => blocks,
            Slot::Block(block) => vec![*block],
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    }
}

/// Create an Attr with the given classes.
fn make_attr(classes: &[&str]) -> Attr {
    (
        String::new(),
        classes.iter().map(|s| (*s).to_string()).collect(),
        LinkedHashMap::new(),
    )
}

/// Extract a string value from JSON.
fn extract_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

/// Extract a bool value from JSON.
fn extract_bool(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|v| v.as_bool())
}

/// Capitalize the first letter of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::empty_attr;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_source_map::{FileId, Location, Range};
    use serde_json::json;

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;

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

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
        }
    }

    #[tokio::test]
    async fn test_transform_name() {
        let transform = CalloutResolveTransform::new();
        assert_eq!(transform.name(), "callout-resolve");
    }

    #[tokio::test]
    async fn test_resolve_simple_callout() {
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "warning"});
        custom.set_slot(
            "content",
            Slot::Blocks(vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Warning content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })]),
        );

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Custom(custom)],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutResolveTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Verify the CustomNode was converted to a Div
        assert_eq!(ast.blocks.len(), 1);
        match &ast.blocks[0] {
            Block::Div(div) => {
                let (_, classes, _) = &div.attr;
                assert!(classes.contains(&"callout".to_string()));
                assert!(classes.contains(&"callout-warning".to_string()));

                // Should have header and body divs
                assert_eq!(div.content.len(), 2);

                // Check header structure
                match &div.content[0] {
                    Block::Div(header) => {
                        let (_, classes, _) = &header.attr;
                        assert!(classes.contains(&"callout-header".to_string()));
                    }
                    _ => panic!("Expected header Div"),
                }

                // Check body structure
                match &div.content[1] {
                    Block::Div(body) => {
                        let (_, classes, _) = &body.attr;
                        assert!(classes.contains(&"callout-body-container".to_string()));
                        assert!(classes.contains(&"callout-body".to_string()));
                    }
                    _ => panic!("Expected body Div"),
                }
            }
            _ => panic!("Expected Div block"),
        }
    }

    #[tokio::test]
    async fn test_resolve_callout_with_title() {
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "tip"});
        custom.set_slot(
            "title",
            Slot::Inlines(vec![Inline::Str(Str {
                text: "Pro Tip".to_string(),
                source_info: dummy_source_info(),
            })]),
        );
        custom.set_slot(
            "content",
            Slot::Blocks(vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Tip content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })]),
        );

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Custom(custom)],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutResolveTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Verify structure
        match &ast.blocks[0] {
            Block::Div(div) => {
                let (_, classes, _) = &div.attr;
                assert!(classes.contains(&"callout-tip".to_string()));
            }
            _ => panic!("Expected Div"),
        }
    }

    #[tokio::test]
    async fn test_resolve_callout_default_title() {
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "note"});
        // No title slot - should use default

        let resolved = resolve_callout(&mut custom, &mut 1);

        // Find the title container and check it has "Note"
        let header = &resolved.content[0];
        if let Block::Div(header_div) = header {
            // Find title container (second child if icon is present)
            for block in &header_div.content {
                if let Block::Div(div) = block {
                    let (_, classes, _) = &div.attr;
                    if classes.contains(&"callout-title-container".to_string())
                        && let Block::Plain(plain) = &div.content[0]
                        && let Inline::Str(s) = &plain.content[0]
                    {
                        assert_eq!(s.text, "Note");
                        return;
                    }
                }
            }
        }
        panic!("Could not find default title");
    }

    #[tokio::test]
    async fn test_resolve_callout_no_icon() {
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "warning", "icon": false});

        let resolved = resolve_callout(&mut custom, &mut 1);

        // Header should only have title container, no icon
        if let Block::Div(header_div) = &resolved.content[0] {
            // With icon=false, header should have only 1 child (title container)
            assert_eq!(header_div.content.len(), 1);
            if let Block::Div(title_div) = &header_div.content[0] {
                let (_, classes, _) = &title_div.attr;
                assert!(classes.contains(&"callout-title-container".to_string()));
            }
        }
    }

    #[tokio::test]
    async fn test_resolve_nested_callout() {
        // Callout inside a blockquote
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "note"});

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::BlockQuote(quarto_pandoc_types::block::BlockQuote {
                content: vec![Block::Custom(custom)],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutResolveTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Verify the nested callout was resolved
        match &ast.blocks[0] {
            Block::BlockQuote(bq) => match &bq.content[0] {
                Block::Div(div) => {
                    let (_, classes, _) = &div.attr;
                    assert!(classes.contains(&"callout-note".to_string()));
                }
                _ => panic!("Expected Div inside BlockQuote"),
            },
            _ => panic!("Expected BlockQuote"),
        }
    }

    #[tokio::test]
    async fn test_capitalize() {
        assert_eq!(capitalize("note"), "Note");
        assert_eq!(capitalize("warning"), "Warning");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("TIP"), "TIP");
    }

    // ====================================================================
    // Canonical-class tests (TS Quarto / Bootstrap scheme).
    //
    // These tests assert the class vocabulary that the bundled Bootstrap
    // SCSS (`resources/scss/bootstrap/_bootstrap-rules.scss`) keys off
    // of, matching what `src/resources/filters/modules/callouts.lua` in
    // TS Quarto emits (`render_to_bootstrap_div`). They are EXPECTED TO
    // FAIL against the pre-Phase-2 resolver — they encode the target
    // behaviour for the rewrite. See
    // `claude-notes/plans/2026-05-22-callout-class-vocabulary-fix.md`.
    // ====================================================================

    fn str_inline(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![str_inline(text)],
            source_info: dummy_source_info(),
        })
    }

    /// Construct a Callout CustomNode. Passing `None` for `title` leaves
    /// the title slot unset (= user supplied no `## Title` header).
    ///
    /// `collapse=Some(true)` models the user writing `collapse="true"`
    /// (starts collapsed); `Some(false)` models `collapse="false"`
    /// (collapsible but starts expanded); `None` means no `collapse`
    /// attribute at all (not collapsible). This mirrors what
    /// `CalloutTransform` writes into `plain_data` for a real qmd
    /// callout.
    fn callout_node(
        callout_type: &str,
        appearance: Option<&str>,
        icon: Option<bool>,
        collapse: Option<bool>,
        title: Option<Vec<Inline>>,
        content: Vec<Block>,
    ) -> CustomNode {
        let mut data = serde_json::Map::new();
        data.insert("type".into(), json!(callout_type));
        if let Some(a) = appearance {
            data.insert("appearance".into(), json!(a));
        }
        if let Some(i) = icon {
            data.insert("icon".into(), json!(i));
        }
        if let Some(starts_collapsed) = collapse {
            data.insert("collapse".into(), json!(true));
            data.insert("collapse_starts_collapsed".into(), json!(starts_collapsed));
        }
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = Value::Object(data);
        if let Some(t) = title {
            custom.set_slot("title", Slot::Inlines(t));
        }
        custom.set_slot("content", Slot::Blocks(content));
        custom
    }

    fn outer_classes(div: &Div) -> Vec<String> {
        div.attr.1.clone()
    }

    fn assert_has_class(classes: &[String], expected: &str) {
        assert!(
            classes.iter().any(|c| c == expected),
            "expected class `{}` to be present; got {:?}",
            expected,
            classes
        );
    }

    fn assert_no_class(classes: &[String], unexpected: &str) {
        assert!(
            classes.iter().all(|c| c != unexpected),
            "expected class `{}` to be absent; got {:?}",
            unexpected,
            classes
        );
    }

    /// Walk the resolved Div tree and return true if any descendant Div
    /// (including the root) carries `class`.
    fn contains_class_anywhere(div: &Div, class: &str) -> bool {
        if div.attr.1.iter().any(|c| c == class) {
            return true;
        }
        for block in &div.content {
            if let Block::Div(d) = block
                && contains_class_anywhere(d, class)
            {
                return true;
            }
        }
        false
    }

    /// Pull the user-visible title text out of a titled-path resolved
    /// Div. Returns None for untitled callouts.
    fn extract_title_text(resolved: &Div) -> Option<String> {
        let header = match resolved.content.first()? {
            Block::Div(d) if d.attr.1.iter().any(|c| c == "callout-header") => d,
            _ => return None,
        };
        for block in &header.content {
            if let Block::Div(d) = block
                && d.attr.1.iter().any(|c| c == "callout-title-container")
                && let Some(Block::Plain(p)) = d.content.first()
            {
                let mut text = String::new();
                for inline in &p.content {
                    if let Inline::Str(s) = inline {
                        text.push_str(&s.text);
                    }
                }
                return Some(text);
            }
        }
        None
    }

    #[tokio::test]
    async fn test_canonical_default_with_user_title() {
        let mut node = callout_node(
            "warning",
            Some("default"),
            None,
            None,
            Some(vec![str_inline("Watch Out")]),
            vec![para("Body")],
        );
        let resolved = resolve_callout(&mut node, &mut 1);
        let classes = outer_classes(&resolved);
        assert_has_class(&classes, "callout");
        assert_has_class(&classes, "callout-style-default");
        assert_has_class(&classes, "callout-warning");
        assert_has_class(&classes, "callout-titled");
        assert_no_class(&classes, "no-icon");
        assert_no_class(&classes, "callout-empty-content");
        assert_no_class(&classes, "callout-appearance-default");
        assert_no_class(&classes, "callout-appearance-simple");
    }

    #[tokio::test]
    async fn test_canonical_default_no_title_injects_default() {
        let mut node = callout_node("tip", Some("default"), None, None, None, vec![para("Body")]);
        let resolved = resolve_callout(&mut node, &mut 1);
        let classes = outer_classes(&resolved);
        assert_has_class(&classes, "callout-style-default");
        assert_has_class(&classes, "callout-titled");
        assert_eq!(
            extract_title_text(&resolved).as_deref(),
            Some("Tip"),
            "appearance=default with no user title should inject the type's display name"
        );
    }

    #[tokio::test]
    async fn test_canonical_simple_no_title_stays_untitled() {
        let mut node = callout_node("note", Some("simple"), None, None, None, vec![para("Body")]);
        let resolved = resolve_callout(&mut node, &mut 1);
        let classes = outer_classes(&resolved);
        assert_has_class(&classes, "callout-style-simple");
        assert_no_class(&classes, "callout-titled");

        // Untitled path: outer has a single child, the body Div with
        // classes `callout-body d-flex` containing the icon container
        // and then the body-container.
        assert_eq!(
            resolved.content.len(),
            1,
            "untitled callout should have a single body child, no header div"
        );
        let body = match &resolved.content[0] {
            Block::Div(d) => d,
            other => panic!("expected body Div, got {:?}", other),
        };
        let body_classes = outer_classes(body);
        assert_has_class(&body_classes, "callout-body");
        assert_has_class(&body_classes, "d-flex");
        assert_no_class(&body_classes, "callout-header");

        // First child = icon container.
        let icon = match &body.content[0] {
            Block::Div(d) => d,
            other => panic!("expected icon container Div, got {:?}", other),
        };
        assert_has_class(&outer_classes(icon), "callout-icon-container");

        // Second child = body-container (NOT also `callout-body`).
        let container = match &body.content[1] {
            Block::Div(d) => d,
            other => panic!("expected body-container Div, got {:?}", other),
        };
        let container_classes = outer_classes(container);
        assert_has_class(&container_classes, "callout-body-container");
        assert_no_class(&container_classes, "callout-body");
    }

    #[tokio::test]
    async fn test_canonical_simple_with_user_title() {
        let mut node = callout_node(
            "important",
            Some("simple"),
            None,
            None,
            Some(vec![str_inline("Please Read")]),
            vec![para("Body")],
        );
        let resolved = resolve_callout(&mut node, &mut 1);
        let classes = outer_classes(&resolved);
        assert_has_class(&classes, "callout-style-simple");
        assert_has_class(&classes, "callout-titled");
        let header = match &resolved.content[0] {
            Block::Div(d) => d,
            other => panic!("expected header Div, got {:?}", other),
        };
        assert_has_class(&outer_classes(header), "callout-header");
    }

    #[tokio::test]
    async fn test_canonical_minimal_normalizes_to_simple_no_icon() {
        let mut node = callout_node(
            "caution",
            Some("minimal"),
            None,
            None,
            None,
            vec![para("Body")],
        );
        let resolved = resolve_callout(&mut node, &mut 1);
        let classes = outer_classes(&resolved);
        assert_has_class(&classes, "callout-style-simple");
        assert_no_class(&classes, "callout-style-minimal");
        assert_has_class(&classes, "no-icon");
        assert!(
            !contains_class_anywhere(&resolved, "callout-icon-container"),
            "minimal appearance must omit the icon container"
        );
    }

    #[tokio::test]
    async fn test_canonical_icon_false_emits_no_icon() {
        let mut node = callout_node(
            "warning",
            Some("default"),
            Some(false),
            None,
            Some(vec![str_inline("Heads Up")]),
            vec![para("Body")],
        );
        let resolved = resolve_callout(&mut node, &mut 1);
        assert_has_class(&outer_classes(&resolved), "no-icon");
        assert!(
            !contains_class_anywhere(&resolved, "callout-icon-container"),
            "icon=false must omit the icon container"
        );
    }

    #[tokio::test]
    async fn test_canonical_empty_content_class() {
        let mut node = callout_node(
            "note",
            Some("default"),
            None,
            None,
            Some(vec![str_inline("Heads Up")]),
            vec![],
        );
        let resolved = resolve_callout(&mut node, &mut 1);
        assert_has_class(&outer_classes(&resolved), "callout-empty-content");
    }

    #[tokio::test]
    async fn test_canonical_titled_header_has_utility_classes() {
        let mut node = callout_node(
            "tip",
            Some("default"),
            None,
            None,
            Some(vec![str_inline("Title")]),
            vec![para("Body")],
        );
        let resolved = resolve_callout(&mut node, &mut 1);
        let header = match &resolved.content[0] {
            Block::Div(d) => d,
            other => panic!("expected header Div, got {:?}", other),
        };
        let header_classes = outer_classes(header);
        assert_has_class(&header_classes, "callout-header");
        assert_has_class(&header_classes, "d-flex");
        assert_has_class(&header_classes, "align-content-center");
    }

    #[tokio::test]
    async fn test_canonical_collapse_true_emits_wrapper() {
        let mut node = callout_node(
            "note",
            Some("default"),
            None,
            Some(true),
            Some(vec![str_inline("T")]),
            vec![para("Body")],
        );
        let resolved = resolve_callout(&mut node, &mut 1);
        // Expected structure: outer > [header, collapse-wrapper];
        // collapse-wrapper > body.
        assert_eq!(
            resolved.content.len(),
            2,
            "outer should have header + collapse wrapper as siblings"
        );
        let header = match &resolved.content[0] {
            Block::Div(d) => d,
            other => panic!("expected header Div, got {:?}", other),
        };
        let header_classes = outer_classes(header);
        assert_has_class(&header_classes, "callout-header");
        assert_has_class(&header_classes, "collapsed");
        let header_attrs = &header.attr.2;
        assert_eq!(
            header_attrs.get("aria-expanded").map(String::as_str),
            Some("false"),
            "collapsed callout header must have aria-expanded=\"false\""
        );
        let collapse = match &resolved.content[1] {
            Block::Div(d) => d,
            other => panic!("expected collapse wrapper Div, got {:?}", other),
        };
        let collapse_classes = outer_classes(collapse);
        assert_has_class(&collapse_classes, "callout-collapse");
        assert_has_class(&collapse_classes, "collapse");
        assert_no_class(&collapse_classes, "show");
    }

    #[tokio::test]
    async fn test_canonical_collapse_false_emits_show_class() {
        let mut node = callout_node(
            "note",
            Some("default"),
            None,
            Some(false),
            Some(vec![str_inline("T")]),
            vec![para("Body")],
        );
        let resolved = resolve_callout(&mut node, &mut 1);
        let header = match &resolved.content[0] {
            Block::Div(d) => d,
            other => panic!("expected header Div, got {:?}", other),
        };
        let header_attrs = &header.attr.2;
        assert_eq!(
            header_attrs.get("aria-expanded").map(String::as_str),
            Some("true"),
            "open collapse callout must have aria-expanded=\"true\""
        );
        assert_no_class(&outer_classes(header), "collapsed");
        let collapse = match &resolved.content[1] {
            Block::Div(d) => d,
            other => panic!("expected collapse wrapper Div, got {:?}", other),
        };
        let collapse_classes = outer_classes(collapse);
        assert_has_class(&collapse_classes, "callout-collapse");
        assert_has_class(&collapse_classes, "collapse");
        assert_has_class(&collapse_classes, "show");
    }

    #[tokio::test]
    async fn test_canonical_no_legacy_appearance_class() {
        for appearance in &["default", "simple"] {
            let mut node = callout_node(
                "note",
                Some(appearance),
                None,
                None,
                Some(vec![str_inline("T")]),
                vec![para("Body")],
            );
            let resolved = resolve_callout(&mut node, &mut 1);
            let classes = outer_classes(&resolved);
            assert_no_class(&classes, "callout-appearance-default");
            assert_no_class(&classes, "callout-appearance-simple");
            assert_no_class(&classes, "callout-appearance-minimal");
        }
    }

    #[tokio::test]
    async fn test_canonical_user_id_preserved() {
        let mut node = callout_node(
            "tip",
            Some("default"),
            None,
            None,
            Some(vec![str_inline("T")]),
            vec![para("Body")],
        );
        node.attr.0 = "mywarn".to_string();
        let resolved = resolve_callout(&mut node, &mut 1);
        assert_eq!(
            resolved.attr.0, "mywarn",
            "user-supplied id must survive resolution"
        );
    }

    #[tokio::test]
    async fn test_canonical_all_types_emit_type_class() {
        for t in &["note", "warning", "tip", "important", "caution"] {
            let mut node = callout_node(
                t,
                Some("default"),
                None,
                None,
                Some(vec![str_inline("T")]),
                vec![para("Body")],
            );
            let resolved = resolve_callout(&mut node, &mut 1);
            assert_has_class(&outer_classes(&resolved), &format!("callout-{}", t));
        }
    }
}

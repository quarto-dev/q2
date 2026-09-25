/*
 * transforms/cross_chapter_crossref_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P5: project-wide crossref registry resolution.
 */

//! Resolve cross-chapter crossref references from the project-wide
//! registry.
//!
//! Multi-file books pause each chapter immediately after the Navigation
//! phase, aggregate every chapter's crossref inventory into one
//! [`ProjectCrossrefIndex`](crate::crossref::project_index::ProjectCrossrefIndex),
//! then resume each chapter into Finalization with that registry attached
//! to the context. This Finalization-phase transform consults the registry
//! for `CustomNode("CrossrefResolvedRef")` nodes that
//! `CrossrefResolveTransform` left unresolved — ids the *referencing*
//! chapter doesn't define but a sibling chapter does — and patches them
//! with the owning chapter's number and a working cross-document target.
//!
//! ## Local wins by construction
//!
//! The transform only ever touches nodes still marked unresolved, so a
//! chapter's own correct local resolution cannot be clobbered — there is
//! no precedence check to get wrong.
//!
//! ## True no-op without a registry
//!
//! `cross_chapter_crossref_registry` is `None` for every non-book render,
//! so registering the transform unconditionally in the shared
//! `build_transform_pipeline` (immediately before
//! `CrossrefRenderTransform`, which renders the patched nodes) is safe:
//! the walk doesn't even run.
//!
//! ## What gets patched
//!
//! Additive `plain_data` keys on the unresolved node, all read by
//! `render_resolved_ref` (in `crossref_render.rs`) when present:
//!
//! - `resolved` → `true` (drops the `quarto-unresolved-ref` class and the
//!   `?id?` text),
//! - `resolved_number` → the owning chapter's composed display number
//!   (e.g. `"2.1"`, `"A.1"`) — composed at aggregation time from the
//!   owning chapter's raw `Order` and seed, so the *rendering* chapter's
//!   seed cannot renumber it,
//! - `target_href` → `page_url_for(owning_chapter_href)` + `#identifier`
//!   (page-relative, exactly like `LinkRewriteTransform`'s output),
//! - `order` / `in_appendix` → the *owning* chapter's raw values, so the
//!   `sec` kind swap ("Chapter N" / "Appendix A") in `sec_ref_text`
//!   classifies by the target's own chapter, not the referencing one.
//!
//! Ids absent from the registry are left untouched — the P4 unresolved
//! placeholder contract (`?id?` + `quarto-unresolved-ref`) still applies
//! to genuinely unknown ids.
//!
//! The walk is [`for_each_inline_mut`] — the same canonical walker
//! `equation_number` and `math_ml` use — so refs inside list items,
//! table cells, captions, custom-node slots, and `Inline::Note` content
//! are all reached.

use crate::ast_walk::for_each_inline_mut;
use crate::crossref::CROSSREF_RESOLVED_REF;
use crate::crossref::project_index::ProjectCrossrefEntry;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use quarto_pandoc_types::custom::CustomNode;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;

/// Project-wide crossref resolver (book-projects P5). See module docs.
pub struct CrossChapterCrossrefResolveTransform;

impl CrossChapterCrossrefResolveTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CrossChapterCrossrefResolveTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CrossChapterCrossrefResolveTransform {
    fn name(&self) -> &str {
        "cross-chapter-crossref-resolve"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> crate::Result<()> {
        // True no-op without a registry: the walk doesn't run, so every
        // non-book render is untouched by construction.
        let Some(registry) = ctx.cross_chapter_crossref_registry.as_ref() else {
            return Ok(());
        };
        let resolver = ctx.resource_resolver.as_ref();
        let target_base = move |href: &str| -> String {
            match resolver {
                Some(rr) => rr.page_url_for(href),
                None => href.to_string(),
            }
        };
        for_each_inline_mut(&mut ast.blocks, &mut |inline| {
            let Inline::Custom(node) = inline else {
                return;
            };
            if node.type_name != CROSSREF_RESOLVED_REF {
                return;
            }
            // Only still-unresolved nodes are patched, so a chapter's own
            // local resolution always wins by construction.
            if node
                .plain_data
                .get("resolved")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                return;
            }
            let Some(entry) = node
                .plain_data
                .get("identifier")
                .and_then(|v| v.as_str())
                .and_then(|id| registry.entries.get(id))
            else {
                // Unknown id: left untouched — the P4 `?id?` placeholder
                // contract still applies.
                return;
            };
            resolve_node(node, entry, &target_base);
        });
        Ok(())
    }
}

/// Patch one unresolved `CrossrefResolvedRef` node in place from a registry
/// hit. `target_base` maps the owning chapter's site-root-relative output
/// href to the URL form the current page should embed (`page_url_for`).
fn resolve_node(
    node: &mut CustomNode,
    entry: &ProjectCrossrefEntry,
    target_base: &dyn Fn(&str) -> String,
) {
    let Some(identifier) = node
        .plain_data
        .get("identifier")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        return;
    };
    let Some(obj) = node.plain_data.as_object_mut() else {
        return;
    };
    obj.insert("resolved".into(), serde_json::json!(true));
    obj.insert(
        "resolved_number".into(),
        serde_json::json!(entry.resolved_number),
    );
    obj.insert(
        "target_href".into(),
        serde_json::json!(format!(
            "{}#{identifier}",
            target_base(&entry.owning_chapter_href)
        )),
    );
    // The owning chapter's raw order + appendix flag, so display code that
    // reads section paths (the `sec` kind swap) classifies by the target's
    // own chapter, not the referencing one.
    obj.insert(
        "order".into(),
        serde_json::json!({"section": entry.order.section, "order": entry.order.order}),
    );
    obj.insert("in_appendix".into(), serde_json::json!(entry.in_appendix));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::index::{CrossrefEntry, CrossrefIndex, Order};
    use crate::crossref::project_index::{
        ChapterCrossrefInventory, ProjectCrossrefIndex, aggregate_chapter_inventories,
    };
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::{BinaryDependencies, ChapterSeed, RenderContext};
    use crate::resource_resolver::ResourceResolverContext;
    use quarto_pandoc_types::attr::Attr;
    use quarto_pandoc_types::block::{Block, Paragraph};
    use quarto_source_map::{FileId, SourceInfo};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn seed(number: u32, is_appendix: bool) -> ChapterSeed {
        ChapterSeed {
            chapter_number: number,
            is_appendix,
        }
    }

    /// entries: (identifier, ref_type, order counter, in_appendix)
    fn inventory(
        href: &str,
        chapter_seed: ChapterSeed,
        entries: Vec<(&str, &str, u32, bool)>,
    ) -> ChapterCrossrefInventory {
        let mut index = CrossrefIndex::new(FileId(0));
        index.max_heading = 1;
        for (id, ref_type, order, in_appendix) in entries {
            index.insert(CrossrefEntry {
                identifier: id.to_string(),
                ref_type: ref_type.to_string(),
                parent: None,
                order: Order {
                    section: vec![order],
                    order,
                },
                caption: None,
                in_appendix,
                source_info: si(),
            });
        }
        ChapterCrossrefInventory {
            index,
            chapter_seed: Some(chapter_seed),
            output_href: href.to_string(),
        }
    }

    fn registry_for(chapters: Vec<ChapterCrossrefInventory>) -> Arc<ProjectCrossrefIndex> {
        let (index, _) = aggregate_chapter_inventories(&chapters);
        Arc::new(index)
    }

    /// An unresolved `CrossrefResolvedRef` custom node exactly as
    /// `CrossrefResolveTransform` leaves it for an id this document does
    /// not define: `resolved: false`, and no `order`/`in_appendix` keys.
    fn unresolved_ref(identifier: &str, ref_type: &str) -> Inline {
        let mut node = CustomNode::new(CROSSREF_RESOLVED_REF, Attr::default(), si());
        node.plain_data = serde_json::json!({
            "identifier": identifier,
            "ref_type": ref_type,
            "kind": "Figure",
            "resolved": false,
            "label_upper": false,
        });
        Inline::Custom(node)
    }

    fn para(content: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: si(),
        })
    }

    fn ast_with(content: Vec<Inline>) -> Pandoc {
        Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![para(content)],
        }
    }

    fn find_custom(blocks: &[Block]) -> &CustomNode {
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected paragraph");
        };
        let Inline::Custom(node) = &p.content[0] else {
            panic!("expected custom node, got {:?}", p.content[0]);
        };
        node
    }

    async fn run_transform(ast: &mut Pandoc, registry: Option<Arc<ProjectCrossrefIndex>>) {
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            output_dir: PathBuf::from("/project/_book"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/ch1.qmd");
        let format = Format::from_format_string("html").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.resource_resolver = Some(ResourceResolverContext::website(
            "/project/_book",
            "/project/_book/ch1.html",
            "site_libs",
            "ch1",
        ));
        ctx.cross_chapter_crossref_registry = registry;
        CrossChapterCrossrefResolveTransform::new()
            .transform(ast, &mut ctx)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cross_chapter_ref_is_patched_from_the_registry() {
        let registry = registry_for(vec![
            inventory("ch1.html", seed(1, false), vec![]),
            inventory(
                "ch2.html",
                seed(2, false),
                vec![("fig-two", "fig", 1, false)],
            ),
        ]);

        let mut ast = ast_with(vec![unresolved_ref("fig-two", "fig")]);
        run_transform(&mut ast, Some(registry)).await;

        let node = find_custom(&ast.blocks);
        assert_eq!(
            node.plain_data["resolved"],
            serde_json::json!(true),
            "the node must be marked resolved: {:?}",
            node.plain_data
        );
        assert_eq!(
            node.plain_data["resolved_number"],
            serde_json::json!("2.1"),
            "the owning chapter's composed number must be patched: {:?}",
            node.plain_data
        );
        assert_eq!(
            node.plain_data["target_href"],
            serde_json::json!("ch2.html#fig-two"),
            "the target must point into the owning chapter's file: {:?}",
            node.plain_data
        );
        assert_eq!(
            node.plain_data["order"],
            serde_json::json!({"section": [1], "order": 1}),
            "the owning chapter's raw order must be patched: {:?}",
            node.plain_data
        );
    }

    #[tokio::test]
    async fn sec_ref_gets_owning_chapter_appendix_flag() {
        let registry = registry_for(vec![inventory(
            "app-a.html",
            seed(1, true),
            vec![("sec-methods", "sec", 1, true)],
        )]);

        let mut ast = ast_with(vec![unresolved_ref("sec-methods", "sec")]);
        run_transform(&mut ast, Some(registry)).await;

        let node = find_custom(&ast.blocks);
        assert_eq!(node.plain_data["resolved"], serde_json::json!(true));
        // The owning chapter's appendix flag rides along so the display-side
        // kind swap ("Appendix A") classifies by the target's own chapter.
        assert_eq!(
            node.plain_data["in_appendix"],
            serde_json::json!(true),
            "the owning chapter's appendix flag must be patched: {:?}",
            node.plain_data
        );
        // The aggregated number for an appendix sec entry is letter-form.
        assert_eq!(
            node.plain_data["resolved_number"],
            serde_json::json!("A"),
            "appendix sec entries get letter numbers: {:?}",
            node.plain_data
        );
    }

    #[tokio::test]
    async fn unknown_id_is_left_unresolved() {
        let registry = registry_for(vec![inventory(
            "ch1.html",
            seed(1, false),
            vec![("fig-one", "fig", 1, false)],
        )]);

        let mut ast = ast_with(vec![unresolved_ref("fig-nowhere", "fig")]);
        run_transform(&mut ast, Some(registry)).await;

        let node = find_custom(&ast.blocks);
        assert_eq!(
            node.plain_data["resolved"],
            serde_json::json!(false),
            "an id the registry doesn't know must stay unresolved: {:?}",
            node.plain_data
        );
        assert!(
            node.plain_data.get("resolved_number").is_none(),
            "no number may be patched for an unknown id: {:?}",
            node.plain_data
        );
        assert!(
            node.plain_data.get("target_href").is_none(),
            "no target may be patched for an unknown id: {:?}",
            node.plain_data
        );
        assert!(
            node.plain_data.get("order").is_none(),
            "no order may be patched for an unknown id: {:?}",
            node.plain_data
        );
    }

    #[tokio::test]
    async fn already_resolved_local_node_is_untouched() {
        // A node this document resolved locally — the transform must not
        // clobber it even when the registry also knows the id.
        let registry = registry_for(vec![inventory(
            "ch1.html",
            seed(1, false),
            vec![("fig-one", "fig", 1, false)],
        )]);

        let mut ast = ast_with(vec![unresolved_ref("fig-one", "fig")]);
        if let Inline::Custom(node) = {
            let Block::Paragraph(p) = &mut ast.blocks[0] else {
                panic!()
            };
            &mut p.content[0]
        } {
            node.plain_data["resolved"] = serde_json::json!(true);
            node.plain_data["resolved_number"] = serde_json::json!("1.1");
        }
        run_transform(&mut ast, Some(registry)).await;

        let node = find_custom(&ast.blocks);
        assert_eq!(
            node.plain_data["resolved_number"],
            serde_json::json!("1.1"),
            "a locally resolved node must keep its own number: {:?}",
            node.plain_data
        );
        assert!(
            node.plain_data.get("target_href").is_none(),
            "a locally resolved node must not gain a cross-chapter target: {:?}",
            node.plain_data
        );
    }

    #[tokio::test]
    async fn no_registry_leaves_the_ast_untouched() {
        let mut ast = ast_with(vec![unresolved_ref("fig-two", "fig")]);
        let before = find_custom(&ast.blocks).plain_data.clone();
        run_transform(&mut ast, None).await;
        let node = find_custom(&ast.blocks);
        assert_eq!(
            node.plain_data, before,
            "no registry → the walk must not run at all"
        );
        assert_eq!(node.plain_data["resolved"], serde_json::json!(false));
    }
}

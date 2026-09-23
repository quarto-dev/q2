/*
 * tests/integration/pandoc_meta_mapping.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * P7 Task 6 — the `Meta`-block mapping for docx/pptx. T6.1 confirms the
 * carriage P2 Task 5 already built (a normalized `title`/`author`/`date`
 * profile reaches Pandoc `Meta` as `MetaInlines`); T6.5 confirms the new
 * `MetaBlocks`->`MetaInlines` coercion (`pandoc_filters::meta_coerce`)
 * survives the real wire serializer, matching exactly the sequence
 * `PandocWriteStage` runs (coerce, then serialize).
 */

use pampa::pandoc::ASTContext;
use pampa::writers::json::{JsonConfig, write_with_config};
use quarto_core::pandoc_filters::meta_coerce::coerce_meta_blocks_to_inlines;
use quarto_pandoc_types::block::{Block, Paragraph};
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_pandoc_types::inline::{Inline, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
use quarto_source_map::SourceInfo;

fn str_inline(s: &str) -> Inline {
    Inline::Str(Str {
        text: s.to_string(),
        source_info: SourceInfo::for_test(),
    })
}

/// A normalized profile the way real front-matter parsing produces it:
/// `title`/`author`/`date` are plain single-line YAML scalars, which
/// Quarto's config parser stores as `PandocInlines`, not `Scalar` (see
/// the `metadata-as-str` lint rule's rationale in the root `CLAUDE.md`).
fn normalized_meta() -> ConfigValue {
    let entries = [
        ("title", "My Title"),
        ("author", "Alice"),
        ("date", "2026-01-02"),
    ]
    .into_iter()
    .map(|(key, text)| ConfigMapEntry {
        key: key.to_string(),
        key_source: SourceInfo::for_test(),
        value: ConfigValue::new_inlines(vec![str_inline(text)], SourceInfo::for_test()),
    })
    .collect();
    ConfigValue::new_map(entries, SourceInfo::for_test())
}

fn meta_json(meta: ConfigValue) -> serde_json::Value {
    let ast = Pandoc {
        meta,
        blocks: vec![],
    };
    let ctx = ASTContext::new();
    let mut buf = Vec::new();
    write_with_config(
        &ast,
        &ctx,
        &mut buf,
        &JsonConfig {
            raw: false,
            ..Default::default()
        },
    )
    .expect("serialization should succeed");
    serde_json::from_slice(&buf).expect("writer output should be valid JSON")
}

/// T6.1: a normalized `title`/`author`/`date` profile reaches `Meta` as
/// `MetaInlines` for all three keys.
#[test]
fn normalized_title_author_date_are_meta_inlines() {
    let json = meta_json(normalized_meta());
    for key in ["title", "author", "date"] {
        assert_eq!(
            json["meta"][key]["t"], "MetaInlines",
            "expected {key} to be MetaInlines, got {:#?}",
            json["meta"][key]
        );
    }
}

/// T6.5: the coercion survives the real wire serializer — a block-valued
/// `title` (as a multi-line YAML scalar produces) is `MetaInlines`, not
/// `MetaBlocks`, in the JSON `PandocWriteStage` actually hands to pandoc.
#[test]
fn block_valued_title_serializes_as_meta_inlines() {
    let blocks = vec![Block::Paragraph(Paragraph {
        content: vec![str_inline("My Title")],
        source_info: SourceInfo::for_test(),
    })];
    let mut meta = ConfigValue::new_map(
        vec![ConfigMapEntry {
            key: "title".to_string(),
            key_source: SourceInfo::for_test(),
            value: ConfigValue {
                value: ConfigValueKind::PandocBlocks(blocks),
                source_info: SourceInfo::for_test(),
                merge_op: Default::default(),
            },
        }],
        SourceInfo::for_test(),
    );

    // Exactly `PandocWriteStage`'s sequence: coerce, then serialize.
    coerce_meta_blocks_to_inlines(&mut meta);
    let json = meta_json(meta);

    assert_eq!(
        json["meta"]["title"]["t"], "MetaInlines",
        "expected MetaInlines after coercion, got {:#?}",
        json["meta"]["title"]
    );
}

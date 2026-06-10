//! Phase 5 — q2-debug JSON wire shape for attribution.
//!
//! Mirrors the contract pinned in
//! `claude-notes/plans/2026-05-06-attribution-pipeline.md` § Phase 4a:
//! when `JsonConfig.attribution_by_node` is populated, the streaming
//! writer emits `astContext.attribution` (sparse array of
//! `{s, actor, time}`) and `astContext.attributionActors` (actor →
//! `{name, color}` table). When `None`, both keys are absent (the
//! off-path JSON is byte-identical to today's output — the
//! structural backing for the Phase 0 byte-identicality invariant
//! that Phase 3b's WASM `parse_qmd_to_ast_with_attribution(content,
//! None)` rests on).

use std::collections::HashMap;
use std::sync::Arc;

use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{Block, Inline, Pandoc, Paragraph, Str};
use pampa::writers::json::{
    JsonAttributionIdentity, JsonAttributionRecord, JsonConfig, write_with_config,
};
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceInfo;

fn empty_meta() -> ConfigValue {
    ConfigValue::new_map(Vec::new(), SourceInfo::for_test())
}

/// Build a one-paragraph Pandoc AST with two `Str` inlines: "hello"
/// (alice) and "world" (bob). Returns the AST plus the source_info
/// pointers for the two inlines (used as attribution_by_node keys).
fn two_str_ast() -> (Pandoc, usize, usize) {
    let s_hello = Inline::Str(Str {
        text: "hello".to_string(),
        source_info: SourceInfo::Original {
            file_id: quarto_source_map::FileId(0),
            start_offset: 0,
            end_offset: 5,
        },
    });
    let s_world = Inline::Str(Str {
        text: "world".to_string(),
        source_info: SourceInfo::Original {
            file_id: quarto_source_map::FileId(0),
            start_offset: 6,
            end_offset: 11,
        },
    });
    let para = Block::Paragraph(Paragraph {
        content: vec![s_hello, s_world],
        source_info: SourceInfo::Original {
            file_id: quarto_source_map::FileId(0),
            start_offset: 0,
            end_offset: 11,
        },
    });
    let pandoc = Pandoc {
        blocks: vec![para],
        meta: empty_meta(),
    };

    // Extract the source_info pointers AFTER constructing pandoc so
    // they're keyed against the field addresses the writer will see.
    let hello_ptr = match &pandoc.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => &s.source_info as *const SourceInfo as usize,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let world_ptr = match &pandoc.blocks[0] {
        Block::Paragraph(p) => match &p.content[1] {
            Inline::Str(s) => &s.source_info as *const SourceInfo as usize,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };

    (pandoc, hello_ptr, world_ptr)
}

#[test]
fn attribution_off_path_omits_both_astcontext_keys() {
    let (pandoc, _, _) = two_str_ast();
    let context = ASTContext::anonymous();

    let mut buf = Vec::new();
    let config = JsonConfig::default();
    write_with_config(&pandoc, &context, &mut buf, &config).expect("write");

    let json: serde_json::Value = serde_json::from_slice(&buf).expect("valid JSON");
    let ast_context = &json["astContext"];

    assert!(
        ast_context.get("attribution").is_none(),
        "off-path: astContext.attribution must be absent — got: {}",
        ast_context
    );
    assert!(
        ast_context.get("attributionActors").is_none(),
        "off-path: astContext.attributionActors must be absent — got: {}",
        ast_context
    );
}

#[test]
fn attribution_on_path_emits_records_and_actors_table() {
    let (pandoc, hello_ptr, world_ptr) = two_str_ast();
    let context = ASTContext::anonymous();

    // Build attribution_by_node keyed by source_info pointer (mirrors
    // what `AttributionRenderTransform::visit_inline` populates).
    let alice: Arc<str> = Arc::from("alice");
    let bob: Arc<str> = Arc::from("bob");
    let mut by_node: HashMap<usize, JsonAttributionRecord> = HashMap::new();
    by_node.insert(
        hello_ptr,
        JsonAttributionRecord {
            actor: Arc::clone(&alice),
            time: 1000,
        },
    );
    by_node.insert(
        world_ptr,
        JsonAttributionRecord {
            actor: Arc::clone(&bob),
            time: 2000,
        },
    );

    let mut actors: HashMap<Arc<str>, JsonAttributionIdentity> = HashMap::new();
    actors.insert(
        Arc::clone(&alice),
        JsonAttributionIdentity {
            display_name: "Alice".to_string(),
            color: "#ff0000".to_string(),
        },
    );
    actors.insert(
        Arc::clone(&bob),
        JsonAttributionIdentity {
            display_name: "Bob".to_string(),
            color: "#00ff00".to_string(),
        },
    );

    let config = JsonConfig {
        include_inline_locations: false,
        attribution_by_node: Some(Arc::new(by_node)),
        attribution_actors: Some(Arc::new(actors)),
    };

    let mut buf = Vec::new();
    write_with_config(&pandoc, &context, &mut buf, &config).expect("write");

    let json: serde_json::Value = serde_json::from_slice(&buf).expect("valid JSON");
    let ast_context = &json["astContext"];

    // attribution array: one record per node, three fields each.
    let attribution = ast_context["attribution"]
        .as_array()
        .expect("astContext.attribution is an array");
    assert_eq!(
        attribution.len(),
        2,
        "two Str inlines → two records; got: {:#?}",
        attribution
    );
    for rec in attribution {
        let obj = rec.as_object().expect("record is an object");
        assert!(obj.contains_key("s"), "record has 's' field: {:?}", rec);
        assert!(
            obj.contains_key("actor"),
            "record has 'actor' field: {:?}",
            rec
        );
        assert!(
            obj.contains_key("time"),
            "record has 'time' field: {:?}",
            rec
        );
    }
    // Names should match alice/bob in walk order.
    let actors_seen: Vec<&str> = attribution
        .iter()
        .map(|r| r["actor"].as_str().unwrap())
        .collect();
    assert_eq!(actors_seen, vec!["alice", "bob"]);

    // attributionActors table: entries for both alice and bob, sorted
    // by key (deterministic emission).
    let actors_obj = ast_context["attributionActors"]
        .as_object()
        .expect("astContext.attributionActors is an object");
    let alice_entry = actors_obj.get("alice").expect("alice in actors table");
    assert_eq!(alice_entry["name"], "Alice");
    assert_eq!(alice_entry["color"], "#ff0000");
    let bob_entry = actors_obj.get("bob").expect("bob in actors table");
    assert_eq!(bob_entry["name"], "Bob");
    assert_eq!(bob_entry["color"], "#00ff00");
}

/// Off-path byte-identicality structural guard: when attribution is
/// off, the JSON serialization equals exactly what it produces with
/// `JsonConfig::default()` (no attribution fields). Backs the Phase
/// 3b WASM byte-identicality invariant: every q2-debug call with
/// `attribution_provider = None` produces output identical to the
/// pre-Phase-5 baseline.
#[test]
fn attribution_off_path_is_byte_identical_to_no_attribution_default() {
    let (pandoc, _, _) = two_str_ast();
    let context = ASTContext::anonymous();

    let mut buf_default = Vec::new();
    write_with_config(&pandoc, &context, &mut buf_default, &JsonConfig::default())
        .expect("write default");

    // Same default, but with both attribution fields explicitly None —
    // simulates the WASM forwarding path when no provider is installed.
    let config_explicit_none = JsonConfig {
        include_inline_locations: false,
        attribution_by_node: None,
        attribution_actors: None,
    };
    let mut buf_explicit = Vec::new();
    write_with_config(&pandoc, &context, &mut buf_explicit, &config_explicit_none)
        .expect("write explicit-none");

    assert_eq!(
        buf_default, buf_explicit,
        "off-path JSON must be byte-identical regardless of how `None` was supplied"
    );
}

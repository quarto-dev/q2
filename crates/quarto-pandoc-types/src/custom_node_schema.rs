//! Loader for the canonical custom-node wire-format schema.
//!
//! `resources/custom-node-schema.json` is the single source of truth for
//! Q2's `__quarto_custom_node` wire format: for each of Q2's real
//! `CustomNode` types, its Pandoc bridge `route` (L/R/N), its `slots`, and
//! its `plain_data` field set (including which fields are conditional and
//! why). It is compiled into the binary via `include_str!` — there is no
//! filesystem read at runtime, so `load()` cannot fail on missing-file
//! grounds; it only reports malformed JSON or a shape the deserializer
//! rejects.
//!
//! See `claude-notes/plans/2026-08-20-pandoc-hybrid-P2-wire-schema.md` for
//! the design and `claude-notes/plans/2026-09-18-pandoc-hybrid-P2-implementation.md`
//! Task 1 for the artifact's derivation.

use hashlink::LinkedHashMap;
use serde::Deserialize;

const SCHEMA_JSON: &str = include_str!("../resources/custom-node-schema.json");

/// The parsed contents of `custom-node-schema.json`.
#[derive(Debug, Deserialize)]
pub struct Schema {
    #[serde(rename = "$comment")]
    pub comment: String,
    pub version: u32,
    pub types: LinkedHashMap<String, TypeEntry>,
}

/// One `CustomNode` type's entry: its Pandoc bridge route, its slots, and
/// its `plain_data` field set.
#[derive(Debug, Deserialize)]
pub struct TypeEntry {
    pub route: Route,
    pub slots: LinkedHashMap<String, SlotKind>,
    pub plain_data: LinkedHashMap<String, PlainDataField>,
}

/// The Pandoc bridge route a `CustomNode` type takes, per design §3's
/// frozen table.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    L,
    R,
    N,
}

/// The wire envelope's `data-custom-slots` tag for one slot.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Block,
    Blocks,
    Inline,
    Inlines,
}

/// One `plain_data` field's presence contract.
#[derive(Debug, Deserialize)]
pub struct PlainDataField {
    pub required: bool,
    #[serde(default)]
    pub when: Option<String>,
    #[serde(default)]
    pub producer: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
}

/// Parse the compiled-in `custom-node-schema.json`.
pub fn load() -> Result<Schema, serde_json::Error> {
    serde_json::from_str(SCHEMA_JSON)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const EXPECTED_TYPE_NAMES: &[&str] = &[
        "Callout",
        "Tabset",
        "FloatRefTarget",
        "Theorem",
        "Proof",
        "Equation",
        "CrossrefResolvedRef",
    ];

    // T1.1: the committed artifact parses, is version 1, and its `types`
    // key set is *exactly* the 7-name set — two-way equality, not
    // `contains` — so both a missing type (revert: delete `"Tabset"`) and
    // an extra/renamed type are caught. `ExampleEmbed` must be absent.
    #[test]
    fn load_returns_exactly_the_seven_expected_types() {
        let schema = load().expect("schema must parse");
        assert_eq!(schema.version, 1);

        let actual: BTreeSet<&str> = schema.types.keys().map(String::as_str).collect();
        let expected: BTreeSet<&str> = EXPECTED_TYPE_NAMES.iter().copied().collect();
        assert_eq!(actual, expected);
        assert!(!schema.types.contains_key("ExampleEmbed"));
    }

    // T1.2: the per-type route map matches design §3's frozen table
    // exactly. This is the discriminator for route *assignment* (as
    // opposed to T1.3's shape-only membership check) — flipping any one
    // type's route (e.g. `Equation` to `"R"`) must turn this red.
    #[test]
    fn per_type_route_map_matches_design_frozen_table() {
        let schema = load().expect("schema must parse");
        let expected: &[(&str, Route)] = &[
            ("Callout", Route::R),
            ("Tabset", Route::R),
            ("FloatRefTarget", Route::R),
            ("Theorem", Route::R),
            ("Proof", Route::R),
            ("Equation", Route::N),
            ("CrossrefResolvedRef", Route::N),
        ];
        for (name, expected_route) in expected {
            let entry = schema
                .types
                .get(*name)
                .unwrap_or_else(|| panic!("missing type entry: {name}"));
            assert_eq!(
                entry.route, *expected_route,
                "route mismatch for {name}: expected {expected_route:?}, got {:?}",
                entry.route
            );
        }
    }

    // T1.5: the deliberate ExampleEmbed omission is documented, not
    // silent — the top-level `$comment` must exist and name ExampleEmbed.
    #[test]
    fn top_level_comment_documents_the_example_embed_omission() {
        let schema = load().expect("schema must parse");
        assert!(!schema.comment.is_empty());
        assert!(schema.comment.contains("ExampleEmbed"));
    }

    // T1.3: the `Route` / `SlotKind` enum deserializers reject malformed
    // literals. Membership/shape validity is reserved for this test
    // (deliberately malformed literals only); the per-type discriminator
    // lives in `per_type_route_map_matches_design_frozen_table` above, so
    // a `Route::L | R | N` membership check here can't mask a mislabelled
    // real type.
    #[test]
    fn malformed_route_literal_fails_to_deserialize() {
        let bad_route = r#"{
            "route": "Q",
            "slots": {},
            "plain_data": {}
        }"#;
        let result: Result<TypeEntry, _> = serde_json::from_str(bad_route);
        assert!(result.is_err(), "expected Err for invalid route tag \"Q\"");
    }

    #[test]
    fn malformed_slot_kind_literal_fails_to_deserialize() {
        let bad_slot_kind = r#"{
            "route": "R",
            "slots": { "x": "Chunk" },
            "plain_data": {}
        }"#;
        let result: Result<TypeEntry, _> = serde_json::from_str(bad_slot_kind);
        assert!(
            result.is_err(),
            "expected Err for invalid slot kind tag \"Chunk\""
        );
    }

    // T1.4: `Proof` deliberately carries no `order` key (it is
    // unnumbered — proof.rs never writes `ref_type`, and
    // crossref_index.rs early-returns without one). Every `order` entry
    // that *does* exist elsewhere in the schema is conditional
    // (`required: false`) and names its producer.
    #[test]
    fn proof_has_no_order_field_and_every_order_field_is_conditional_with_a_producer() {
        let schema = load().expect("schema must parse");

        let proof = schema.types.get("Proof").expect("Proof must exist");
        assert!(
            !proof.plain_data.contains_key("order"),
            "Proof is unnumbered and must not carry an `order` plain_data entry"
        );

        for (type_name, entry) in &schema.types {
            if let Some(order) = entry.plain_data.get("order") {
                assert!(
                    !order.required,
                    "{type_name}'s `order` field must be `required: false`"
                );
                assert!(
                    order.producer.is_some(),
                    "{type_name}'s `order` field must carry a `producer`"
                );
            }
        }
    }

    // Every `plain_data` field with `required: false` must carry a
    // `when` explanation (acceptance criterion 2).
    #[test]
    fn every_optional_plain_data_field_carries_a_when() {
        let schema = load().expect("schema must parse");
        for (type_name, entry) in &schema.types {
            for (field_name, field) in &entry.plain_data {
                if !field.required {
                    assert!(
                        field.when.is_some(),
                        "{type_name}.{field_name} is optional but has no `when`"
                    );
                }
            }
        }
    }
}

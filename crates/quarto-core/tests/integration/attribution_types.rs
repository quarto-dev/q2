//! Phase 0 tests #1 and #2.
//!
//! - **#1**: WASM-transport JSON round-trip with interning preservation.
//!   The transport-only mirror types serde-round-trip in three
//!   configurations (runs-only, identities-only, both populated), and
//!   the canonical form produced by `PreBuiltAttributionProvider`
//!   restores the `Arc<str>` interning invariant.
//! - **#2**: `AttributionMap::query_byte_range` — mirrors the TS
//!   `attribution-runs.test.ts` invariants on `feat/node-attribution`.

use std::collections::HashMap;
use std::sync::Arc;

use quarto_core::Format;
use quarto_core::attribution::{
    AttributionData, AttributionDataBuilder, AttributionMap, AttributionSource,
    AttributionSourceProvider, Identity, PreBuiltAttributionProvider, TransportAttributionData,
    TransportAttributionRun,
};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};

// ===========================================================================
// Phase 0 test #1 — Transport JSON round-trip + interning restoration
// ===========================================================================

/// Construct a small but representative transport payload and serde
/// round-trip it, verifying the wire shape preserves all three field
/// configurations (runs-only, identities-only, both).
#[test]
fn transport_json_round_trip_runs_only() {
    let original = TransportAttributionData {
        runs: vec![TransportAttributionRun {
            start: 0,
            end: 5,
            actor: "alice@example.com".to_string(),
            time: 1_700_000_000_000,
        }],
        identities: HashMap::new(),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    // identities is empty → key should be omitted via skip_serializing_if
    assert!(
        !json.contains("\"identities\""),
        "runs-only payload should omit empty identities; got: {json}"
    );
    let decoded: TransportAttributionData = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded.runs.len(), 1);
    assert_eq!(decoded.runs[0].actor, "alice@example.com");
    assert_eq!(decoded.runs[0].start, 0);
    assert_eq!(decoded.runs[0].end, 5);
    assert_eq!(decoded.runs[0].time, 1_700_000_000_000);
    assert!(decoded.identities.is_empty());
}

#[test]
fn transport_json_round_trip_identities_only() {
    let mut identities = HashMap::new();
    identities.insert(
        "alice@example.com".to_string(),
        Identity {
            display_name: "Alice".to_string(),
            color: "#ff0000".to_string(),
        },
    );
    let original = TransportAttributionData {
        runs: Vec::new(),
        identities,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    assert!(
        !json.contains("\"runs\""),
        "identities-only payload should omit empty runs; got: {json}"
    );
    let decoded: TransportAttributionData = serde_json::from_str(&json).expect("deserialize");
    assert!(decoded.runs.is_empty());
    assert_eq!(decoded.identities.len(), 1);
    let id = decoded.identities.get("alice@example.com").expect("alice");
    assert_eq!(id.display_name, "Alice");
    assert_eq!(id.color, "#ff0000");
    // Wire shape uses `name`, not `display_name`.
    assert!(
        json.contains("\"name\":\"Alice\""),
        "Identity should serialize as `name`, not `display_name`; got: {json}"
    );
}

#[test]
fn transport_json_round_trip_both_populated() {
    let mut identities = HashMap::new();
    identities.insert(
        "alice@example.com".to_string(),
        Identity {
            display_name: "Alice".to_string(),
            color: "#ff0000".to_string(),
        },
    );
    let original = TransportAttributionData {
        runs: vec![TransportAttributionRun {
            start: 0,
            end: 5,
            actor: "alice@example.com".to_string(),
            time: 1_700_000_000_000,
        }],
        identities,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let decoded: TransportAttributionData = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded.runs.len(), 1);
    assert_eq!(decoded.identities.len(), 1);
}

/// Stronger assertion (the load-bearing half of test #1):
/// `PreBuiltAttributionProvider` takes a transport JSON string,
/// decodes it via the transport types, feeds the result through
/// `AttributionDataBuilder`, and the resulting canonical
/// `AttributionData` satisfies `Arc::ptr_eq(run.actor,
/// identities.get_key_value(actor))` for every actor that appears in
/// both runs and identities.
///
/// This is the round-trip *interning restoration* contract. Each
/// `Arc::from(s)` during deserialize would otherwise allocate
/// per-occurrence; the builder re-interns so the writer-side
/// invariant is preserved through the wire.
#[test]
fn transport_round_trip_restores_arc_interning_via_prebuilt_provider() {
    let mut identities = HashMap::new();
    identities.insert(
        "alice@example.com".to_string(),
        Identity {
            display_name: "Alice".to_string(),
            color: "#ff0000".to_string(),
        },
    );
    let original = TransportAttributionData {
        runs: vec![
            TransportAttributionRun {
                start: 0,
                end: 5,
                actor: "alice@example.com".to_string(),
                time: 1,
            },
            // Second run by the same actor — interning should mean both
            // `run.actor` Arcs are pointer-equal AND pointer-equal to
            // the identities map key.
            TransportAttributionRun {
                start: 5,
                end: 10,
                actor: "alice@example.com".to_string(),
                time: 2,
            },
        ],
        identities,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let provider = PreBuiltAttributionProvider::new(json);

    // We need a RenderContext to call `build`. Construct a minimal
    // single-doc context — the prebuilt provider doesn't actually
    // consult any of the context fields; the ctx arg is just trait
    // conformance.
    let project_dir = std::env::temp_dir().join("attribution-test-#1");
    let project = ProjectContext {
        dir: project_dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(project_dir.join("input.qmd"))],
        output_dir: project_dir.clone(),

        ..Default::default()
    };
    let doc = DocumentInfo::from_path(project_dir.join("input.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let data: AttributionData = provider.build(&ctx).expect("build");

    // Run-actor Arcs must be pointer-equal across runs by the same author.
    assert_eq!(data.runs.len(), 2, "two runs survive the round-trip");
    let r0 = &data.runs.as_slice()[0];
    let r1 = &data.runs.as_slice()[1];
    assert!(
        Arc::ptr_eq(&r0.actor, &r1.actor),
        "interning invariant: same-author runs share the same Arc<str>"
    );

    // Run-actor Arc must be pointer-equal to the identities map key
    // for that actor — this is the interning invariant the writer-
    // side `attribution_lookup` relies on.
    let (key, _id) = data
        .identities
        .get_key_value(r0.actor.as_ref())
        .expect("identity entry for alice");
    assert!(
        Arc::ptr_eq(key, &r0.actor),
        "interning invariant: identities key Arc<str> is ptr-equal to run.actor"
    );
}

// ===========================================================================
// Phase 0 test #2 — AttributionMap::query_byte_range invariants
// ===========================================================================

fn make_map(runs: Vec<(usize, usize, &str, i64)>) -> AttributionMap {
    let mut b = AttributionDataBuilder::new();
    for (start, end, actor, time) in runs {
        b.push_run(start, end, actor, time);
    }
    b.build().runs
}

#[test]
fn query_byte_range_empty_runs_returns_none() {
    let map = AttributionMap::new();
    assert!(map.query_byte_range(0, 10).is_none());
}

#[test]
fn query_byte_range_single_run_hit_within_bounds() {
    let map = make_map(vec![(0, 10, "alice@x", 100)]);
    let hit = map.query_byte_range(2, 5).expect("hit");
    assert_eq!(hit.actor.as_ref(), "alice@x");
    assert_eq!(hit.time, 100);
}

#[test]
fn query_byte_range_non_overlapping_query_returns_none() {
    let map = make_map(vec![(0, 5, "alice@x", 100)]);
    assert!(map.query_byte_range(10, 20).is_none());
}

#[test]
fn query_byte_range_overlapping_two_actors_picks_most_recent() {
    let map = make_map(vec![(0, 5, "alice@x", 100), (5, 10, "bob@x", 200)]);
    let hit = map.query_byte_range(0, 10).expect("hit");
    assert_eq!(hit.actor.as_ref(), "bob@x");
    assert_eq!(hit.time, 200);
}

#[test]
fn query_byte_range_at_run_boundary() {
    // Query [0, 5) sits exactly inside the first run; the second run
    // starts at 5 (exclusive boundary). The first author wins.
    let map = make_map(vec![(0, 5, "alice@x", 100), (5, 10, "bob@x", 200)]);
    let hit = map.query_byte_range(0, 5).expect("hit");
    assert_eq!(hit.actor.as_ref(), "alice@x");
    assert_eq!(hit.time, 100);
}

#[test]
fn query_byte_range_inverted_or_empty_query_returns_none() {
    let map = make_map(vec![(0, 10, "alice@x", 100)]);
    assert!(map.query_byte_range(5, 5).is_none(), "empty range");
    assert!(map.query_byte_range(7, 3).is_none(), "inverted range");
}

//! Phase 0 tests #4 and #5 — `AttributionGenerateTransform`.
//!
//! - **#4**: happy path. Given a fixture provider, the transform
//!   populates `ctx.attribution_data`; the run-actor Arcs are
//!   pointer-equal to the corresponding key in `identities`.
//! - **#5**: skip conditions (no provider, feature disabled, format
//!   doesn't consume the lookup) plus the identities-only YAML
//!   override merge (provider wins on collision for the Arc key, user
//!   wins on identity value, non-colliding user keys are dropped).

use std::sync::Arc;

use quarto_core::Format;
use quarto_core::Result;
use quarto_core::attribution::{
    AttributionData, AttributionDataBuilder, AttributionHit, AttributionMap, AttributionSource,
    AttributionSourceProvider, Identity,
};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::transform::AstTransform;
use quarto_core::transforms::AttributionGenerateTransform;
use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_project(dir: &std::path::Path) -> ProjectContext {
    ProjectContext {
        dir: dir.to_path_buf(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(dir.join("test.qmd"))],
        output_dir: dir.to_path_buf(),

        ..Default::default()
    }
}

fn make_doc(dir: &std::path::Path) -> DocumentInfo {
    DocumentInfo::from_path(dir.join("test.qmd"))
}

fn empty_meta() -> ConfigValue {
    ConfigValue::new_map(Vec::new(), SourceInfo::for_test())
}

fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
    let info = SourceInfo::for_test();
    let map_entries: Vec<ConfigMapEntry> = entries
        .into_iter()
        .map(|(k, v)| ConfigMapEntry {
            key: k.to_string(),
            key_source: info.clone(),
            value: v,
        })
        .collect();
    ConfigValue::new_map(map_entries, info)
}

fn s(x: &str) -> ConfigValue {
    ConfigValue::new_string(x, SourceInfo::for_test())
}

fn b(x: bool) -> ConfigValue {
    ConfigValue::new_bool(x, SourceInfo::for_test())
}

fn pandoc_with_meta(meta: ConfigValue) -> Pandoc {
    Pandoc {
        blocks: Vec::new(),
        meta,
    }
}

/// Query helper that imports the trait inline so the test code reads cleanly.
fn query(map: &AttributionMap, start: usize, end: usize) -> Option<AttributionHit> {
    map.query_byte_range(start, end)
}

/// Fixture provider that hands back a fixed `AttributionData`.
struct FixtureProvider {
    data: AttributionData,
}

impl AttributionSourceProvider for FixtureProvider {
    fn build(&self, _ctx: &RenderContext) -> Result<AttributionData> {
        Ok(self.data.clone())
    }
}

// ===========================================================================
// Phase 0 test #4 — happy path
// ===========================================================================

#[tokio::test]
async fn generate_happy_path_populates_sidecar_and_preserves_arc_interning() {
    let dir = std::env::temp_dir().join("attribution-test-#4");
    let project = make_project(&dir);
    let doc = make_doc(&dir);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let mut bld = AttributionDataBuilder::new();
    bld.set_identity(
        "alice",
        Identity {
            display_name: "Alice".to_string(),
            color: "#ff0000".to_string(),
        },
    );
    bld.push_run(0, 5, "alice", 1);
    bld.push_run(5, 10, "bob", 2);
    let data = bld.build();

    ctx.attribution_provider = Some(Arc::new(FixtureProvider { data }));

    let mut ast = pandoc_with_meta(empty_meta());
    AttributionGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    let sidecar = ctx
        .attribution_data
        .as_ref()
        .expect("attribution_data populated");

    let hit = query(&sidecar.runs, 0, 10).expect("hit on full range");
    assert_eq!(hit.actor.as_ref(), "bob");
    assert_eq!(hit.time, 2);

    let (alice_key, alice_identity) = sidecar
        .identities
        .iter()
        .find(|(k, _)| k.as_ref() == "alice")
        .expect("alice identity present");
    assert_eq!(alice_identity.display_name, "Alice");
    assert_eq!(alice_identity.color, "#ff0000");

    let alice_run = sidecar
        .runs
        .as_slice()
        .iter()
        .find(|r| r.actor.as_ref() == "alice")
        .expect("alice run");
    assert!(
        Arc::ptr_eq(alice_key, &alice_run.actor),
        "interning invariant: identities key Arc<str> is ptr-equal to AttributionRun.actor"
    );
}

#[tokio::test]
async fn generate_with_empty_provider_identities_leaves_sidecar_identities_empty() {
    let dir = std::env::temp_dir().join("attribution-test-#4-empty-id");
    let project = make_project(&dir);
    let doc = make_doc(&dir);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let mut bld = AttributionDataBuilder::new();
    bld.push_run(0, 5, "alice", 1);
    ctx.attribution_provider = Some(Arc::new(FixtureProvider { data: bld.build() }));

    let mut ast = pandoc_with_meta(empty_meta());
    AttributionGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    let sidecar = ctx.attribution_data.as_ref().expect("sidecar populated");
    assert!(
        sidecar.identities.is_empty(),
        "provider returned no identities; merge produces an empty map"
    );
}

// ===========================================================================
// Phase 0 test #5 — skip conditions and identities-only YAML override
// ===========================================================================

#[tokio::test]
async fn generate_no_provider_skips_silently() {
    let dir = std::env::temp_dir().join("attribution-test-#5-no-provider");
    let project = make_project(&dir);
    let doc = make_doc(&dir);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let mut ast = pandoc_with_meta(empty_meta());
    AttributionGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    assert!(
        ctx.attribution_data.is_none(),
        "no provider → sidecar untouched"
    );
    assert!(
        ctx.diagnostics.is_empty(),
        "no provider → no diagnostic emitted"
    );
}

#[tokio::test]
async fn generate_feature_disabled_skips() {
    let dir = std::env::temp_dir().join("attribution-test-#5-feature-disabled");
    let project = make_project(&dir);
    let doc = make_doc(&dir);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let mut bld = AttributionDataBuilder::new();
    bld.push_run(0, 5, "alice", 1);
    ctx.attribution_provider = Some(Arc::new(FixtureProvider { data: bld.build() }));

    let meta = map(vec![("attribution", b(false))]);
    let mut ast = pandoc_with_meta(meta);

    AttributionGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    assert!(
        ctx.attribution_data.is_none(),
        "feature disabled → sidecar untouched"
    );
}

#[tokio::test]
async fn generate_non_consuming_format_skips_before_calling_provider() {
    let dir = std::env::temp_dir().join("attribution-test-#5-non-consuming");
    let project = make_project(&dir);
    let doc = make_doc(&dir);
    // Any non-HTML format works to exercise the skip ladder's first
    // rule. `pdf` is a real format; `native` was a Phase 0 placeholder
    // that doesn't exist in `FormatIdentifier`.
    let format = Format::from_format_string("pdf").expect("pdf format");
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    /// Provider that would panic if `build` were called. The skip
    /// ladder must bail before reaching it.
    struct PanicProvider;
    impl AttributionSourceProvider for PanicProvider {
        fn build(&self, _ctx: &RenderContext) -> Result<AttributionData> {
            panic!("provider must NOT be called for non-consuming formats");
        }
    }
    ctx.attribution_provider = Some(Arc::new(PanicProvider));

    let mut ast = pandoc_with_meta(empty_meta());
    AttributionGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    assert!(ctx.attribution_data.is_none());
}

/// Identities-only YAML override merge (positive case, not a skip).
///
/// Three sub-assertions:
/// - **(a)** Key present in both YAML and provider → user identity
///   wins; the merged map's key for that actor is `Arc::ptr_eq` to the
///   provider's `Arc<str>` (preserving the interning invariant).
/// - **(b)** Key present only in the provider → unchanged.
/// - **(c)** Key present only in user YAML → dropped (not unioned).
#[tokio::test]
async fn generate_identities_only_yaml_override_merges_correctly() {
    let dir = std::env::temp_dir().join("attribution-test-#5-yaml-merge");
    let project = make_project(&dir);
    let doc = make_doc(&dir);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let mut bld = AttributionDataBuilder::new();
    bld.set_identity(
        "alice",
        Identity {
            display_name: "Alice from provider".to_string(),
            color: "#000001".to_string(),
        },
    );
    bld.set_identity(
        "bob",
        Identity {
            display_name: "Bob from provider".to_string(),
            color: "#000002".to_string(),
        },
    );
    bld.push_run(0, 5, "alice", 1);
    bld.push_run(5, 10, "bob", 2);
    ctx.attribution_provider = Some(Arc::new(FixtureProvider { data: bld.build() }));

    // meta.attribution.identities = { alice: <override>, carol: <no-collision> }
    let alice_id = map(vec![
        ("name", s("Alice from YAML")),
        ("color", s("#ffaaaa")),
    ]);
    let carol_id = map(vec![("name", s("Carol")), ("color", s("#ccccff"))]);
    let identities = map(vec![("alice", alice_id), ("carol", carol_id)]);
    let attribution_node = map(vec![("identities", identities)]);
    let meta = map(vec![("attribution", attribution_node)]);
    let mut ast = pandoc_with_meta(meta);

    AttributionGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    let sidecar = ctx.attribution_data.as_ref().expect("sidecar populated");

    // (a) alice: user value wins, but Arc key is provider's.
    let (alice_key, alice_id_merged) = sidecar
        .identities
        .iter()
        .find(|(k, _)| k.as_ref() == "alice")
        .expect("alice identity present");
    assert_eq!(
        alice_id_merged.display_name, "Alice from YAML",
        "(a) user identity wins on collision"
    );
    assert_eq!(alice_id_merged.color, "#ffaaaa");
    let alice_run = sidecar
        .runs
        .as_slice()
        .iter()
        .find(|r| r.actor.as_ref() == "alice")
        .expect("alice run");
    assert!(
        Arc::ptr_eq(alice_key, &alice_run.actor),
        "(a) interning invariant preserved through merge"
    );

    // (b) bob: provider-only, unchanged.
    let bob_id = sidecar
        .identities
        .iter()
        .find(|(k, _)| k.as_ref() == "bob")
        .map(|(_, id)| id)
        .expect("bob identity present");
    assert_eq!(
        bob_id.display_name, "Bob from provider",
        "(b) provider unchanged"
    );

    // (c) carol: YAML-only, dropped.
    let carol = sidecar
        .identities
        .iter()
        .find(|(k, _)| k.as_ref() == "carol");
    assert!(
        carol.is_none(),
        "(c) non-colliding user-only YAML identity is dropped (no runs for that actor)"
    );
}

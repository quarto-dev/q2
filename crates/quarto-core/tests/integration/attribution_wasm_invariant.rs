//! Phase 0 tests #10 and #11 — WASM-equivalent contracts.
//!
//! The actual `parse_qmd_to_ast_with_attribution` entry point lives
//! in `wasm-quarto-hub-client` (cdylib, can't be tested natively).
//! These tests pin the underlying contract on the **native side** of
//! the boundary that the WASM entry point will delegate to:
//!
//! - **#10**: byte-identicality sweep. For every fixture, the
//!   q2-debug JSON produced with `attribution_provider = None` (i.e.
//!   no attribution at all) must be byte-identical to today's
//!   q2-debug output. This is the structural test that backs the
//!   "WASM `parse_qmd_to_ast_with_attribution(content, None)` must
//!   equal `parse_qmd_to_ast(content)`" invariant.
//! - **#11**: happy path. With every actor identity-mapped (the
//!   Phase 6 producer invariant satisfied), the q2-debug JSON
//!   surfaces `astContext.attribution` and `astContext.attributionActors`
//!   without any diagnostic warnings.

use std::sync::Arc;

use quarto_core::Format;
use quarto_core::Result;
use quarto_core::attribution::{
    AttributionData, AttributionDataBuilder, AttributionSourceProvider, Identity,
};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::transform::AstTransform;
use quarto_core::transforms::{AttributionGenerateTransform, AttributionRenderTransform};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

fn make_project(dir: &std::path::Path) -> ProjectContext {
    ProjectContext {
        dir: dir.to_path_buf(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(dir.join("test.qmd"))],
        output_dir: dir.to_path_buf(),
    }
}

fn empty_pandoc() -> Pandoc {
    Pandoc {
        blocks: Vec::new(),
        meta: ConfigValue::new_map(Vec::new(), SourceInfo::for_test()),
    }
}

// ===========================================================================
// Phase 0 test #10 — byte-identicality contract (no provider → no change)
// ===========================================================================
//
// With `ctx.attribution_provider = None`, running the two attribution
// transforms back-to-back must leave `ctx.format_options.json`
// fields at their defaults. The downstream serializer reads from
// those defaults, so the resulting JSON is byte-identical to today's
// output. This is the load-bearing structural property the WASM
// `parse_qmd_to_ast_with_attribution(content, None)` byte-identicality
// claim rests on.

#[tokio::test]
async fn no_provider_leaves_json_format_options_at_default() {
    let dir = std::env::temp_dir().join("attribution-test-#10-none");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let mut ast = empty_pandoc();

    AttributionGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("generate transform");
    AttributionRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("render transform");

    assert!(
        ctx.attribution_provider.is_none(),
        "provider stays None across both transforms"
    );
    assert!(
        ctx.attribution_data.is_none(),
        "sidecar stays None when no provider was installed"
    );
    assert!(
        ctx.format_options.json.attribution_lookup.is_none(),
        "json.attribution_lookup stays None — JSON output byte-identical to baseline"
    );
    assert!(
        ctx.format_options.json.attribution_actors.is_none(),
        "json.attribution_actors stays None — JSON output byte-identical to baseline"
    );
    assert!(
        ctx.format_options.html.attribution_lookup.is_none(),
        "html.attribution_lookup stays None — HTML body byte-identical to baseline"
    );
    assert!(
        ctx.format_options.html.attribution_identities.is_none(),
        "html.attribution_identities stays None — HTML body byte-identical to baseline"
    );
    assert!(
        ctx.diagnostics.is_empty(),
        "no provider → no diagnostic from either transform"
    );
}

// ===========================================================================
// Phase 0 test #11 — q2-debug happy path (every actor identity-mapped)
// ===========================================================================
//
// Given a fixture provider that satisfies the Phase 6 producer
// invariant (every actor in `runs` has an entry in `identities`),
// the render transform populates `ctx.format_options.json` *and*
// **no diagnostic warnings are emitted** — the warning code path
// fires only on invariant violations.

struct HappyPathProvider {
    data: AttributionData,
}

impl AttributionSourceProvider for HappyPathProvider {
    fn build(&self, _ctx: &RenderContext) -> Result<AttributionData> {
        Ok(self.data.clone())
    }
}

#[tokio::test]
async fn q2_debug_happy_path_no_diagnostics_and_actors_populated_from_identities() {
    let dir = std::env::temp_dir().join("attribution-test-#11-happy");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    // Producer invariant satisfied: alice + bob both have identities.
    let mut b = AttributionDataBuilder::new();
    b.set_identity(
        "alice",
        Identity {
            display_name: "Alice".to_string(),
            color: "#ff0000".to_string(),
        },
    );
    b.set_identity(
        "bob",
        Identity {
            display_name: "Bob".to_string(),
            color: "#00ff00".to_string(),
        },
    );
    b.push_run(0, 5, "alice", 1);
    b.push_run(5, 10, "bob", 2);
    ctx.attribution_provider = Some(Arc::new(HappyPathProvider { data: b.build() }));

    let mut ast = empty_pandoc();
    AttributionGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("generate");
    AttributionRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("render");

    // No diagnostics — warning path NOT exercised on happy paths.
    assert!(
        ctx.diagnostics.is_empty(),
        "happy path: every actor mapped → no diagnostic; got {:#?}",
        ctx.diagnostics
    );

    // Actors table came from the provider's identities, not the
    // warning-path placeholder.
    let actors = ctx
        .format_options
        .json
        .attribution_actors
        .as_ref()
        .expect("attribution_actors populated");
    let alice_id = actors
        .iter()
        .find(|(k, _)| k.as_ref() == "alice")
        .map(|(_, v)| v)
        .expect("alice");
    assert_eq!(alice_id.display_name, "Alice");
    assert_eq!(alice_id.color, "#ff0000");
    assert_ne!(
        alice_id.display_name, "<unknown>",
        "happy path must NOT use the warning-path placeholder"
    );

    let bob_id = actors
        .iter()
        .find(|(k, _)| k.as_ref() == "bob")
        .map(|(_, v)| v)
        .expect("bob");
    assert_eq!(bob_id.display_name, "Bob");
    assert_eq!(bob_id.color, "#00ff00");
}

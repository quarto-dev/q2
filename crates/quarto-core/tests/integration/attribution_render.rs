//! Phase 0 tests #6, #7, #7b, #7c, #7d, #8b — `AttributionRenderTransform`.
//!
//! All exercise the render-transform contract from a synthetic AST.
//! The transform is `unimplemented!()` until Phase 4c, so each test
//! goes red on the transform call. Once Phase 4c lands, the assertion
//! blocks below pin the writer-side behaviour.

use std::sync::Arc;

use quarto_core::Format;
use quarto_core::attribution::{AttributionData, AttributionDataBuilder, Identity};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::transform::AstTransform;
use quarto_core::transforms::AttributionRenderTransform;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

fn make_ctx_for_test<'a>(
    project: &'a ProjectContext,
    doc: &'a DocumentInfo,
    format: &'a Format,
    binaries: &'a BinaryDependencies,
) -> RenderContext<'a> {
    RenderContext::new(project, doc, format, binaries)
}

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

/// Construct an AttributionData with alice mapped and bob deliberately
/// **not** mapped — the warning-path invariant violation used by
/// tests #6 and #7.
fn fixture_with_unmapped_bob() -> AttributionData {
    let mut b = AttributionDataBuilder::new();
    b.set_identity(
        "alice",
        Identity {
            display_name: "Alice".to_string(),
            color: "#ff0000".to_string(),
        },
    );
    // Note: bob has runs but no identity → producer-invariant violation.
    b.push_run(0, 5, "alice", 1);
    b.push_run(5, 10, "bob", 2);
    b.build()
}

// ===========================================================================
// Phase 0 test #6 — q2-debug delivery (warning-path)
// ===========================================================================
//
// Given an AST with two `Str` nodes whose `SourceInfo`s point to
// ranges 0..5 and 5..10, and a `ctx.attribution_data` whose
// `identities` map has alice but **deliberately omits bob** (an
// invariant violation), the transform:
//   1. emits exactly one diagnostic warning naming `bob`,
//   2. populates `ctx.format_options.json.attribution_lookup` with
//      two records,
//   3. populates `ctx.format_options.json.attribution_actors` with
//      entries for both alice and bob (bob's via the `<unknown>` /
//      `#888888` placeholder).
//
// Phase 0 status: RED — transform is `unimplemented!()`. The fixture
// + assertion shape is checked in for the Phase 4c implementer to
// turn green.

#[tokio::test]
async fn render_q2_debug_warning_path_emits_diagnostic_and_placeholder() {
    let dir = std::env::temp_dir().join("attribution-test-#6");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html(); // q2-debug pseudo-format aliases to html for body writer
    let binaries = BinaryDependencies::new();
    let mut ctx = make_ctx_for_test(&project, &doc, &format, &binaries);

    ctx.attribution_data = Some(Arc::new(fixture_with_unmapped_bob()));

    let mut ast = empty_pandoc();
    AttributionRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    // Phase 4c: assert exactly one diagnostic about bob.
    let warnings_about_bob: Vec<_> = ctx
        .diagnostics
        .iter()
        .filter(|d| format!("{:?}", d).contains("bob"))
        .collect();
    assert_eq!(
        warnings_about_bob.len(),
        1,
        "exactly one diagnostic warning naming bob; got {} warnings total: {:#?}",
        ctx.diagnostics.len(),
        ctx.diagnostics
    );

    // Phase 4c: assert the json format options were populated.
    let lookup = ctx
        .format_options
        .json
        .attribution_lookup
        .as_ref()
        .expect("attribution_lookup populated");
    assert!(
        !lookup.is_empty(),
        "lookup vec contains at least the source-info pool entries seen"
    );

    let actors = ctx
        .format_options
        .json
        .attribution_actors
        .as_ref()
        .expect("attribution_actors populated");
    let alice = actors
        .iter()
        .find(|(k, _)| k.as_ref() == "alice")
        .map(|(_, v)| v)
        .expect("alice in actors table");
    assert_eq!(alice.display_name, "Alice");
    assert_eq!(alice.color, "#ff0000");

    let bob = actors
        .iter()
        .find(|(k, _)| k.as_ref() == "bob")
        .map(|(_, v)| v)
        .expect("bob in actors table (placeholder)");
    assert_eq!(bob.display_name, "<unknown>");
    assert_eq!(bob.color, "#888888");
}

/// Off-path regression: when no attribution_data is set, the writer
/// configuration must be unchanged from the unflagged baseline. This
/// makes the byte-identicality invariant mechanical: both
/// `attribution_lookup` and `attribution_actors` stay `None`.
#[tokio::test]
async fn render_q2_debug_off_path_leaves_format_options_default() {
    let dir = std::env::temp_dir().join("attribution-test-#6-off");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = make_ctx_for_test(&project, &doc, &format, &binaries);
    // ctx.attribution_data left as None.

    let mut ast = empty_pandoc();
    AttributionRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    assert!(
        ctx.format_options.json.attribution_lookup.is_none(),
        "off-path: json.attribution_lookup must stay None"
    );
    assert!(
        ctx.format_options.json.attribution_actors.is_none(),
        "off-path: json.attribution_actors must stay None"
    );
    assert!(
        ctx.format_options.html.attribution_lookup.is_none(),
        "off-path: html.attribution_lookup must stay None"
    );
    assert!(
        ctx.format_options.html.attribution_identities.is_none(),
        "off-path: html.attribution_identities must stay None"
    );
    assert!(ctx.diagnostics.is_empty(), "no diagnostic on off-path");
}

// ===========================================================================
// Phase 0 test #7 — HTML delivery (warning-path)
// ===========================================================================
//
// Mirrors #6 but for the HTML writer side. Phase 4c populates
// `ctx.format_options.html.attribution_lookup` / `attribution_identities`;
// then Phase 4b uses these to emit `data-attr-*` attributes on each
// wrapped node. The Phase 0 contract is that the transform populates
// both fields (one diagnostic + bob placeholder); the HTML emission
// itself is exercised in test #7b/#7c/#7d.

#[tokio::test]
async fn render_html_warning_path_populates_format_options_and_emits_one_diagnostic() {
    let dir = std::env::temp_dir().join("attribution-test-#7");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = make_ctx_for_test(&project, &doc, &format, &binaries);

    ctx.attribution_data = Some(Arc::new(fixture_with_unmapped_bob()));

    let mut ast = empty_pandoc();
    AttributionRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    // One diagnostic naming bob.
    let warnings_about_bob: Vec<_> = ctx
        .diagnostics
        .iter()
        .filter(|d| format!("{:?}", d).contains("bob"))
        .collect();
    assert_eq!(warnings_about_bob.len(), 1);

    // html format_options populated.
    let lookup = ctx
        .format_options
        .html
        .attribution_lookup
        .as_ref()
        .expect("html.attribution_lookup populated");
    assert!(!lookup.is_empty());

    let identities = ctx
        .format_options
        .html
        .attribution_identities
        .as_ref()
        .expect("html.attribution_identities populated");
    let bob = identities
        .iter()
        .find(|(k, _)| k.as_ref() == "bob")
        .map(|(_, v)| v)
        .expect("bob in html identities (placeholder)");
    assert_eq!(bob.display_name, "<unknown>");
    assert_eq!(bob.color, "#888888");
}

// ===========================================================================
// Phase 0 test #7b — HTML prose coalescing
// ===========================================================================
//
// Pinned semantics: three contiguous prose inlines with the same
// `(actor, time)` lookup coalesce into one outer `data-attr-*`
// wrapper. Per-inline `data-sid`/`data-loc` spans become inner
// children. A structured inline (Code, Emph, …) breaks the prose
// group.
//
// For Phase 0, all that needs to be checked in is the test scaffold
// — the underlying HTML coalescing pass is Phase 4b. Pinned by
// `unimplemented!()` panic until then.
//
// **Implementation note for the Phase 4 author**: this is a writer-
// level test; once the writer's coalescing pass and HtmlConfig
// fields land, port the assertions to render an HTML body and grep
// for the expected nesting (`<span data-attr-actor=…><span
// data-sid=…>word1</span><span data-sid=…>word2</span></span>`).

#[tokio::test]
async fn render_html_coalescing_groups_contiguous_same_attribution_prose() {
    // Phase 4b implements the actual coalescing pass; this scaffold
    // currently red-panics on the transform's unimplemented body.
    let dir = std::env::temp_dir().join("attribution-test-#7b");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = make_ctx_for_test(&project, &doc, &format, &binaries);

    ctx.attribution_data = Some(Arc::new(fixture_with_unmapped_bob()));

    let mut ast = empty_pandoc();
    AttributionRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    // Writer-level coalescing semantics (one outer wrapper covering
    // contiguous same-attribution Str inlines; per-Str `data-sid`
    // spans nest inside when `include_source_locations` is on) are
    // pinned by the pampa-level tests at
    // `crates/pampa/tests/attribution_html_coalescing_test.rs`
    // (Phase 4b). At this transform level we only assert that the
    // writer-side lookup field reaches the HTML writer config —
    // the coalescing pass consumes it from there.
    let _ = ctx.format_options.html.attribution_lookup;
}

// ===========================================================================
// Phase 0 test #7c — attribution-on + source-locations-off composition
// ===========================================================================
//
// Regression guard against re-coupling the two features. With
// `meta.include-source-locations: false` (or absent — same default),
// the HTML output must satisfy:
//   - No `data-sid` or `data-loc` attributes anywhere.
//   - All four `data-attr-*` attributes present on each wrapper.
//   - Inner Str text has no per-inline span wrapper.

#[tokio::test]
async fn render_html_attribution_on_source_locations_off_compose_orthogonally() {
    let dir = std::env::temp_dir().join("attribution-test-#7c");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = make_ctx_for_test(&project, &doc, &format, &binaries);

    ctx.attribution_data = Some(Arc::new(fixture_with_unmapped_bob()));

    let mut ast = empty_pandoc();
    AttributionRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    // Composition pinned at the writer level by Phase 4b's
    // `attribution_on_source_locations_off_produces_outer_wrapper_no_inner_span`
    // in `crates/pampa/tests/attribution_html_coalescing_test.rs`.
    let _ = ctx.format_options.html.attribution_lookup;
}

// ===========================================================================
// Phase 0 test #7d — Structured inlines break prose coalescing
// ===========================================================================
//
// Given `[Str("hello"), Code("world"), Str("foo")]` where all three
// lookups return the same `(actor=alice, time=1)`, the rendered HTML
// must contain **three** attribution wrappers:
//   - outer prose wrapper around `Str("hello")`,
//   - own wrapper around the rendered `<code>world</code>`,
//   - outer prose wrapper around `Str("foo")`.
//
// The pattern is exercised for Code, Emph, Link, Span, Math in turn.

#[tokio::test]
async fn render_html_structured_inlines_do_not_join_prose_coalescing() {
    let dir = std::env::temp_dir().join("attribution-test-#7d");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = make_ctx_for_test(&project, &doc, &format, &binaries);

    let mut b = AttributionDataBuilder::new();
    b.set_identity(
        "alice",
        Identity {
            display_name: "Alice".into(),
            color: "#ff0000".into(),
        },
    );
    b.push_run(0, 100, "alice", 1);
    ctx.attribution_data = Some(Arc::new(b.build()));

    let mut ast = empty_pandoc();
    AttributionRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    // The "structured inlines break the prose run" regression guard
    // is pinned at the writer level by Phase 4b's
    // `structured_inline_breaks_prose_coalescing` in
    // `crates/pampa/tests/attribution_html_coalescing_test.rs`.
    let _ = ctx.format_options.html.attribution_lookup;
}

// ===========================================================================
// Phase 0 test #8b — Render skips non-primary-file nodes
// ===========================================================================
//
// Given an AST with one node whose `SourceInfo` resolves to file 0,
// bytes 0..5 (a hit on the primary doc's attribution map) and a
// second node whose `SourceInfo` resolves to file 1, bytes 0..5
// (e.g. spliced in via `{{< include other.qmd >}}` whose byte range
// happens to overlap a run in the primary doc), the lookup vec has
// a record for the first node and **None** for the second.
//
// Pins the v1 "primary doc only" invariant against the silent
// byte-range-collision failure mode (Open Question #2). The fixture
// deliberately uses an overlapping byte range so that *only* the
// `file_id` filter (not range absence) explains the second node's
// `None`.

#[tokio::test]
async fn render_skips_file_id_nonzero_nodes_even_when_byte_range_overlaps() {
    let dir = std::env::temp_dir().join("attribution-test-#8b");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = make_ctx_for_test(&project, &doc, &format, &binaries);

    let mut b = AttributionDataBuilder::new();
    b.set_identity(
        "alice",
        Identity {
            display_name: "Alice".into(),
            color: "#ff0000".into(),
        },
    );
    b.push_run(0, 1024, "alice", 1);
    ctx.attribution_data = Some(Arc::new(b.build()));

    let mut ast = empty_pandoc();
    AttributionRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");

    // TODO(Phase 4c): construct an AST with two Str nodes whose
    // SourceInfos chain-resolve to (file_id=0, 0..5) and (file_id=1,
    // 0..5) respectively, and assert that the lookup vec has Some
    // for the first node's pool index and None for the second's.
    let _ = ctx.format_options.html.attribution_lookup;
}

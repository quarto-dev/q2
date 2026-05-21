/*
 * tests/idempotence.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 3 — q2-preview pipeline idempotence gate.
 *
 * Each fixture is driven through the q2-preview pipeline twice in
 * each drive mode (`SingleFile` and `ProjectOrchestrator`) and the
 * resulting `blocks` and `meta` (excluding `rendered.*`) hashes must
 * compare equal across the two runs.
 *
 * See:
 *   claude-notes/plans/2026-05-04-q2-preview-plan-3-builtin-filter-idempotence.md
 *
 * The plan documents the long-lived-integration-branch policy: a
 * fixture that surfaces real non-determinism stays failing here, and
 * a beads issue (filled in from the panic message's
 * `DivergencePoint`) is filed against the offending transform/stage.
 * Do not `#[ignore]` a failing fixture without explicit user approval.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use pampa::pandoc::ASTContext;
use quarto_ast_reconcile::{
    compute_blocks_hash_fresh, compute_meta_hash_fresh_excluding_rendered, find_first_divergence,
};
use quarto_core::format::Format;
use quarto_core::pipeline::{build_q2_preview_pipeline_stages, run_pipeline};
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
use quarto_core::project::pass2_renderer::{RenderToPreviewAstRenderer, WasmPassTwoOutput};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::stage::DocumentAst;
use quarto_pandoc_types::Pandoc;
use quarto_source_map::SourceContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

// ─── Helpers (copied verbatim from render_page_in_project.rs) ─────
//
// Each `tests/*.rs` file is its own test binary, so sharing helpers
// between integration tests requires a `tests/common/` module that
// every test then explicitly imports. The plan rules dedup of that
// shape out of scope for Plan 3, so for now we copy these tiny
// utilities. If/when a second consumer wants them, this pair plus
// the `render_active_page_preview` body below is the natural
// extraction point.

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn write_bytes(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

// ─── Drive modes ──────────────────────────────────────────────────

/// How a fixture is driven through the pipeline. Every fixture runs
/// once per mode; the two runs within a mode must hash equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DriveMode {
    /// `run_pipeline` directly with `build_q2_preview_pipeline_stages`.
    /// Mirrors `render_qmd_to_preview_ast` — the lowest-level entry
    /// point used by the WASM preview.
    SingleFile,
    /// Drives `ProjectPipeline<RenderToPreviewAstRenderer>` with
    /// `RenderMode::ActivePage(active)`. Reuses the same orchestrator
    /// path real `q2 preview` / hub-client takes.
    ProjectOrchestrator,
}

const BOTH_MODES: &[DriveMode] = &[DriveMode::SingleFile, DriveMode::ProjectOrchestrator];
#[allow(dead_code)] // Used by website / orchestrator-only fixtures in Phase 4.
const ORCHESTRATOR_ONLY: &[DriveMode] = &[DriveMode::ProjectOrchestrator];

// ─── Fixture struct ───────────────────────────────────────────────

/// A single Plan-3 fixture. Each fixture owns its own `TempDir` per
/// run; `setup` writes the project contents into that root.
struct Fixture {
    name: &'static str,
    /// Idempotent setup callback. Receives the freshly-created
    /// project root (a canonicalized `TempDir` path) and writes the
    /// page contents — at minimum `<root>/<active>`, plus any
    /// `_quarto.yml` or sibling files the fixture needs.
    setup: Box<dyn Fn(&Path)>,
    /// The active page, relative to the project root.
    active: PathBuf,
    /// Which drive modes this fixture is meaningful in. Document-only
    /// fixtures run in both modes; website-chrome fixtures are
    /// orchestrator-only (chrome transforms need a populated
    /// ProjectIndex).
    modes: &'static [DriveMode],
    /// Optional transport-shape attribution JSON. When set, both
    /// `DriveMode`s install a
    /// `PreBuiltAttributionProvider(json.to_string())` on the
    /// `RenderContext` (single-file) or pass it to the renderer
    /// via `with_attribution` (orchestrator). `None` = no provider.
    attribution_json: Option<&'static str>,
}

impl Fixture {
    fn run_in_each_mode(&self) {
        for &mode in self.modes {
            run_fixture(self, mode);
        }
    }
}

// ─── Test driver ──────────────────────────────────────────────────

fn run_fixture(fixture: &Fixture, mode: DriveMode) {
    let doc_1 = run_q2_preview(fixture, mode);
    let doc_2 = run_q2_preview(fixture, mode);

    let blocks_a = compute_blocks_hash_fresh(&doc_1.ast.blocks);
    let blocks_b = compute_blocks_hash_fresh(&doc_2.ast.blocks);
    let meta_a = compute_meta_hash_fresh_excluding_rendered(&doc_1.ast.meta);
    let meta_b = compute_meta_hash_fresh_excluding_rendered(&doc_2.ast.meta);

    if blocks_a != blocks_b || meta_a != meta_b {
        let point = find_first_divergence(
            &doc_1.ast.blocks,
            &doc_1.ast.meta,
            &doc_2.ast.blocks,
            &doc_2.ast.meta,
        );
        panic!(
            "fixture {} ({:?}): non-idempotent\n  \
             blocks: {:016x} vs {:016x}\n  \
             meta:   {:016x} vs {:016x}\n  \
             first divergence: {:?}",
            fixture.name, mode, blocks_a, blocks_b, meta_a, meta_b, point,
        );
    }
}

fn run_q2_preview(fixture: &Fixture, mode: DriveMode) -> DocumentAst {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    (fixture.setup)(&project_dir);
    let active = canonical(&project_dir.join(&fixture.active));

    let doc = match mode {
        DriveMode::SingleFile => run_single_file(&project_dir, &active, fixture.attribution_json),
        DriveMode::ProjectOrchestrator => {
            run_orchestrator(&project_dir, &active, fixture.attribution_json)
        }
    };
    drop(temp);
    doc
}

// ─── SingleFile mode ──────────────────────────────────────────────

fn run_single_file(
    _project_dir: &Path,
    active: &Path,
    attribution_json: Option<&'static str>,
) -> DocumentAst {
    pollster::block_on(async {
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        // Mirror `render_active_page_preview`'s discovery dance so
        // a fixture that writes a `_quarto.yml` ends up with a
        // populated `project.files` rather than a single-file
        // synthetic project.
        let mut project = ProjectContext::discover(active, runtime.as_ref()).unwrap();
        if !project.is_single_file {
            project = ProjectContext::discover(&project.dir, runtime.as_ref()).unwrap();
        }

        let doc_info = project
            .files
            .iter()
            .find(|d| d.input == active)
            .expect("active file present in discovered project")
            .clone();

        let format = Format::from_format_string("q2-preview")
            .expect("q2-preview is a recognized pseudo-format");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc_info, &format, &binaries);
        if let Some(json) = attribution_json {
            ctx.attribution_provider = Some(Arc::new(
                quarto_core::attribution::PreBuiltAttributionProvider::new(json.to_string()),
            ));
        }

        let content = std::fs::read(active).unwrap();
        let stages = build_q2_preview_pipeline_stages(None, Vec::new());
        let (output, _diagnostics) = run_pipeline(
            &content,
            &active.to_string_lossy(),
            &mut ctx,
            runtime,
            stages,
        )
        .await
        .expect("q2-preview pipeline run (SingleFile mode)");

        output
            .into_document_ast()
            .expect("q2-preview pipeline produces DocumentAst at its tail")
    })
}

// ─── ProjectOrchestrator mode ─────────────────────────────────────

fn run_orchestrator(
    _project_dir: &Path,
    active: &Path,
    attribution_json: Option<&'static str>,
) -> DocumentAst {
    let output = render_active_page_preview(active, attribution_json);
    let ast_json = output
        .payload
        .as_ast_json()
        .expect("orchestrator must emit Pass2Payload::AstJson");
    let mut bytes = ast_json.as_bytes();
    let (pandoc, ast_context) =
        pampa::readers::json::read(&mut bytes).expect("re-parse AST JSON from orchestrator");
    pandoc_to_document_ast(pandoc, ast_context, active.to_path_buf())
}

/// Lifted from `crates/quarto-core/tests/render_page_in_project.rs:660`.
/// Each `tests/*.rs` is its own binary, so the helper has to be
/// duplicated rather than imported. The plan flags this as
/// acceptable for now.
fn render_active_page_preview(
    active: &Path,
    attribution_json: Option<&'static str>,
) -> WasmPassTwoOutput {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(active, runtime.as_ref()).unwrap();
    if !project.is_single_file {
        project = ProjectContext::discover(&project.dir, runtime.as_ref()).unwrap();
    }

    let project_type = project_type_for(&project);
    let vfs_root = project.dir.join(".quarto/project-artifacts");
    // bd-rz2we: override the URL root so rendered AST link/asset
    // URLs use the synthetic VFS prefix instead of the host's
    // tempdir path. Disk writes still land under `vfs_root` (the
    // real tempdir) via `runtime.file_write`, but the URLs
    // embedded in the AST stay byte-identical across runs in
    // different tempdirs — which is what this idempotence gate
    // is asserting.
    let mut renderer =
        RenderToPreviewAstRenderer::new(&vfs_root).with_url_root("/.quarto/project-artifacts");
    if let Some(json) = attribution_json {
        renderer = renderer.with_attribution(json.to_string());
    }

    let format =
        Format::from_format_string("q2-preview").expect("q2-preview is a recognized pseudo-format");

    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        format,
        "q2-preview",
        runtime.clone(),
        renderer,
    )
    .with_mode(RenderMode::ActivePage(active.to_path_buf()));

    let summary = pollster::block_on(pipeline.run()).expect("q2-preview pipeline run");
    assert!(
        summary.pass1_failures.is_empty(),
        "unexpected pass-1 failures: {:?}",
        summary.pass1_failures,
    );
    assert!(
        summary.pass2_failures.is_empty(),
        "unexpected pass-2 failures: {:?}",
        summary.pass2_failures,
    );
    assert_eq!(
        summary.outputs.len(),
        1,
        "ActivePage mode should produce exactly one output",
    );
    summary.outputs.into_iter().next().unwrap()
}

/// Shuffle a re-parsed `Pandoc` + `ASTContext` into the `DocumentAst`
/// shape the hashing helpers expect. The hash only reads
/// `ast.blocks` and `ast.meta`; the other `DocumentAst` fields are
/// defaulted because they're outside the contract this gate defends.
fn pandoc_to_document_ast(pandoc: Pandoc, ast_context: ASTContext, path: PathBuf) -> DocumentAst {
    DocumentAst {
        path,
        ast: pandoc,
        ast_context,
        source_context: SourceContext::new(),
        warnings: Vec::new(),
        recorded_includes: Vec::new(),
    }
}

// ─── Convenience constructors ─────────────────────────────────────

/// Single-file fixture: writes `content` to `<root>/index.qmd`,
/// runs in both `SingleFile` and `ProjectOrchestrator` modes.
fn doc_fixture(name: &'static str, content: &'static str) -> Fixture {
    Fixture {
        name,
        setup: Box::new(move |root: &Path| {
            write(&root.join("index.qmd"), content);
        }),
        active: PathBuf::from("index.qmd"),
        modes: BOTH_MODES,
        attribution_json: None,
    }
}

// =====================================================================
// Phase-2 smoke fixture
// =====================================================================
//
// One minimal fixture proves the harness works end-to-end before
// Phases 3-4 (existing-fixture carry-forward, gap-closure fixtures)
// land. The fixture body is intentionally trivial — a single
// paragraph — so any failure points unambiguously at the harness,
// not at a transform.

#[test]
fn smoke_plain_paragraph() {
    doc_fixture("smoke-plain-paragraph", "hello\n").run_in_each_mode();
}

// =====================================================================
// Phase 3 — carry-forward fixtures (one per transform / feature)
// =====================================================================
//
// Each `#[test]` calls `run_in_each_mode`, which loops through
// `SingleFile` and `ProjectOrchestrator`. Failures are *expected* on
// first run for some of these — that's the whole point of the gate.
// Per Phase 5 / §"CI failure policy", leave failing fixtures failing
// and file a beads issue using the sub-agent investigation prompt
// the panic message fills in. Do NOT `#[ignore]` without explicit
// user approval.

// ─── shortcode-resolve, metadata-normalize ────────────────────────

#[test]
fn meta_single() {
    doc_fixture("meta-single", "---\nfoo: hello\n---\n\n{{< meta foo >}}\n").run_in_each_mode();
}

#[test]
fn meta_markdown() {
    doc_fixture(
        "meta-markdown",
        "---\nfoo: '**Bold** title'\n---\n\n{{< meta foo >}}\n",
    )
    .run_in_each_mode();
}

// ─── include-expansion + shortcode-resolve ────────────────────────

#[test]
fn include_trivial() {
    let fixture = Fixture {
        name: "include-trivial",
        setup: Box::new(|root: &Path| {
            write(&root.join("child.qmd"), "Child content\n");
            write(&root.join("index.qmd"), "{{< include child.qmd >}}\n");
        }),
        active: PathBuf::from("index.qmd"),
        modes: BOTH_MODES,
        attribution_json: None,
    };
    fixture.run_in_each_mode();
}

// ─── callout (callout-resolve is excluded from q2-preview) ────────

#[test]
fn callout_warning() {
    doc_fixture(
        "callout-warning",
        "::: {.callout-warning}\nBody of the callout.\n:::\n",
    )
    .run_in_each_mode();
}

// ─── theorem-sugar ────────────────────────────────────────────────

#[test]
fn theorem() {
    doc_fixture(
        "theorem",
        "::: {#thm-foo .theorem}\nThere is a theorem here.\n:::\n",
    )
    .run_in_each_mode();
}

// ─── float-ref-target-sugar ───────────────────────────────────────

#[test]
fn figure_ref_target() {
    // Image file is not actually opened by AST transforms; absence
    // is fine for AST-level hashing. If a downstream transform
    // grows a path-resolution side effect, add a tiny stub here.
    doc_fixture(
        "figure-ref-target",
        ":::: {#fig-foo}\n![cap](img.png)\n::::\n",
    )
    .run_in_each_mode();
}

// ─── crossref-index + crossref-resolve ────────────────────────────

#[test]
fn crossref_to_theorem() {
    doc_fixture(
        "crossref-to-theorem",
        "::: {#thm-foo .theorem}\nThere is a theorem here.\n:::\n\nSee @thm-foo for the proof.\n",
    )
    .run_in_each_mode();
}

// ─── sectionize ───────────────────────────────────────────────────

#[test]
fn sectionize_multi() {
    doc_fixture(
        "sectionize-multi",
        "## A\n\nBody A.\n\n### B\n\nBody B.\n\n## C\n\nBody C.\n",
    )
    .run_in_each_mode();
}

// ─── footnotes ────────────────────────────────────────────────────

#[test]
fn footnotes_mixed() {
    doc_fixture(
        "footnotes-mixed",
        "Text with inline^[an inline footnote] and reference[^foo].\n\n[^foo]: footnote body\n",
    )
    .run_in_each_mode();
}

// ─── appendix-structure (with license meta + footnotes interaction)

#[test]
fn appendix_license() {
    doc_fixture(
        "appendix-license",
        "---\nlicense: CC BY\ncopyright: 2026 ACME\n---\n\nBody paragraph.\n\n::: {.appendix}\nAppendix content.\n:::\n\nReference[^a]\n\n[^a]: footnote\n",
    )
    .run_in_each_mode();
}

// ─── combined: sectionize + callouts + shortcodes ────────────────

#[test]
fn combined_stress() {
    doc_fixture(
        "combined-stress",
        "---\ntitle: '**Bold** title'\n---\n\n## A\n\n::: {.callout-warning}\nWarning: {{< meta title >}}\n:::\n\n### B\n\nMore body text.\n",
    )
    .run_in_each_mode();
}

// =====================================================================
// Phase 4a — gap-closure fixtures (single-file, no extra scaffolding)
// =====================================================================

// ─── code-block-generate, code-block-render, code-highlight ───────

#[test]
fn code_block_fenced() {
    doc_fixture(
        "code-block-fenced",
        "Some text.\n\n```python\nprint(\"hello\")\n```\n",
    )
    .run_in_each_mode();
}

// ─── shortcode-resolve via Lua-loaded handler ─────────────────────
//
// `{{< version >}}` returns `quarto.version` joined by dots. Lua
// state is constructed fresh per pipeline run (see plan §"Design
// decisions"), so two runs over the same input must agree.

#[test]
fn lua_shortcode_version() {
    doc_fixture("lua-shortcode-version", "version: {{< version >}}\n").run_in_each_mode();
}

// `{{< lipsum 3 >}}` (no `random=` kwarg) — `math.randomseed` runs
// at module load but `math.random` is never reached on this code
// path, so the output is deterministically the first 3 paragraphs
// of the canned text. The `random=true` variant is intentionally
// non-deterministic and out of scope (plan §"Noted, not actively
// tested").

#[test]
fn lua_shortcode_lipsum_fixed() {
    doc_fixture(
        "lua-shortcode-lipsum-fixed",
        // The comment in-document survives as part of the markdown
        // (it's an HTML comment in the parsed AST), so the seed
        // observation is recorded in the fixture itself rather than
        // only in this Rust source.
        "<!-- lipsum.lua calls math.randomseed at module load; the\n     fixed (non-`random=true`) code path never reaches math.random,\n     so this fixture is deterministic. -->\n\n{{< lipsum 3 >}}\n",
    )
    .run_in_each_mode();
}

// ─── proof-sugar ──────────────────────────────────────────────────

#[test]
fn proof() {
    doc_fixture(
        "proof",
        "::: {.proof}\nThe proof is left as an exercise.\n:::\n",
    )
    .run_in_each_mode();
}

// ─── equation-label + crossref-resolve (equation branch) ──────────

#[test]
fn equation_labeled() {
    doc_fixture(
        "equation-labeled",
        "$$ E = mc^2 $$ {#eq-mass}\n\nSee @eq-mass for the relation.\n",
    )
    .run_in_each_mode();
}

// ─── toc-generate, toc-render ─────────────────────────────────────

#[test]
fn toc_on() {
    doc_fixture(
        "toc-on",
        "---\ntoc: true\n---\n\n## A\n\nBody A.\n\n## B\n\nBody B.\n\n## C\n\nBody C.\n",
    )
    .run_in_each_mode();
}

// ─── built-in Lua filter (video) ──────────────────────────────────
//
// `resources/extensions/quarto/video/` is embedded at compile time
// via `include_dir!` (see `crates/quarto-core/src/extension/mod.rs`)
// and auto-discovered for every `StageContext::new` call, so the
// fixture needs no scaffolding beyond declaring the filter.

#[test]
fn video_filter_header() {
    doc_fixture(
        "video-filter-header",
        "---\nfilters:\n  - video\n---\n\n# Title {background-video=\"https://www.youtube.com/embed/abc\"}\n",
    )
    .run_in_each_mode();
}

// ─── table-bootstrap-class ────────────────────────────────────────

#[test]
fn table_bootstrap_class() {
    doc_fixture("table-bootstrap-class", "| col |\n| --- |\n| val |\n").run_in_each_mode();
}

// ─── compile-theme-css stage ──────────────────────────────────────
//
// Default theme. The `theme:` key isn't required to opt the stage
// in; `compile-theme-css` runs in the q2-preview stage list for
// HTML-shaped formats unconditionally. Hash excludes `rendered.*`,
// so the compiled CSS (which lands under `meta.rendered.*` and may
// vary in trivial whitespace) doesn't participate.

#[test]
fn theme_bootstrap() {
    doc_fixture("theme-bootstrap", "---\ntheme: cosmo\n---\n\nBody.\n").run_in_each_mode();
}

// =====================================================================
// Phase 4b — gap-closure fixtures (multi-file)
// =====================================================================

// ─── include-resolve stage ────────────────────────────────────────

#[test]
fn include_in_header() {
    let fixture = Fixture {
        name: "include-in-header",
        setup: Box::new(|root: &Path| {
            write(
                &root.join("header.html"),
                "<style>.idempotence-marker{display:none}</style>\n",
            );
            write(
                &root.join("index.qmd"),
                "---\ninclude-in-header: header.html\n---\n\nBody paragraph.\n",
            );
        }),
        active: PathBuf::from("index.qmd"),
        modes: BOTH_MODES,
        attribution_json: None,
    };
    fixture.run_in_each_mode();
}

// ─── resource-collector ───────────────────────────────────────────
//
// 67-byte minimal valid PNG (1×1 transparent pixel). The AST-level
// transforms only record the path, not the bytes, but providing a
// real file means resource-collector can resolve relative to the
// fixture root rather than warning about a missing local resource.
// Per the fixtures README, paths must resolve relative to the
// project root (no absolute process paths).

const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

#[test]
fn resource_image() {
    let fixture = Fixture {
        name: "resource-image",
        setup: Box::new(|root: &Path| {
            write_bytes(&root.join("local.png"), TINY_PNG);
            write(&root.join("index.qmd"), "![alt text](./local.png)\n");
        }),
        active: PathBuf::from("index.qmd"),
        modes: BOTH_MODES,
        attribution_json: None,
    };
    fixture.run_in_each_mode();
}

// =====================================================================
// Phase 4c — website-project fixtures (orchestrator-only)
// =====================================================================
//
// Chrome transforms (navbar / sidebar / page-nav / footer / favicon /
// bootstrap-icons / canonical-url) require a populated `ProjectIndex`,
// which the orchestrator pass-1 builds and pass-2 consumes. Driving
// a website fixture through `SingleFile` mode would test a partial
// pipeline that doesn't exist in production — so these run in
// `ProjectOrchestrator` only.

// ─── website-chrome: navbar, sidebar, page-nav, footer, favicon,
// bootstrap-icons, canonical-url, title-prefix ────────────────────

#[test]
fn website_chrome() {
    let fixture = Fixture {
        name: "website-chrome",
        setup: Box::new(|root: &Path| {
            write(
                &root.join("_quarto.yml"),
                concat!(
                    "project:\n",
                    "  type: website\n",
                    "\n",
                    "website:\n",
                    "  title: Idempotence Chrome\n",
                    "  site-url: https://example.com/\n",
                    "  favicon: favicon.ico\n",
                    "  navbar:\n",
                    "    title: Idempotence Chrome\n",
                    "    background: primary\n",
                    "    left:\n",
                    "      - index.qmd\n",
                    "      - other.qmd\n",
                    "  sidebar:\n",
                    "    contents:\n",
                    "      - index.qmd\n",
                    "      - other.qmd\n",
                    "  page-navigation: true\n",
                    "  page-footer: \"Copyright 2026\"\n",
                ),
            );
            write(
                &root.join("index.qmd"),
                "---\ntitle: Home\n---\n\nHome body.\n",
            );
            write(
                &root.join("other.qmd"),
                "---\ntitle: Other\n---\n\nOther body.\n",
            );
            // favicon.ico — a tiny stub so any path-resolution side
            // effect resolves. Per the fixtures README rule.
            write_bytes(&root.join("favicon.ico"), &[0u8; 4]);
        }),
        active: PathBuf::from("index.qmd"),
        modes: ORCHESTRATOR_ONLY,
        attribution_json: None,
    };
    fixture.run_in_each_mode();
}

// ─── website-links: cross-page .qmd body links → link-rewrite,
// link-resolution stage ───────────────────────────────────────────

#[test]
fn website_links() {
    let fixture = Fixture {
        name: "website-links",
        setup: Box::new(|root: &Path| {
            write(
                &root.join("_quarto.yml"),
                concat!(
                    "project:\n",
                    "  type: website\n",
                    "\n",
                    "website:\n",
                    "  title: Idempotence Links\n",
                ),
            );
            write(
                &root.join("index.qmd"),
                "---\ntitle: Home\n---\n\nSee [the other page](other.qmd) for more.\n",
            );
            write(
                &root.join("other.qmd"),
                "---\ntitle: Other\n---\n\nReturn to [home](index.qmd).\n",
            );
        }),
        active: PathBuf::from("index.qmd"),
        modes: ORCHESTRATOR_ONLY,
        attribution_json: None,
    };
    fixture.run_in_each_mode();
}

// ─── website-listing: listing-generate, listing-render,
// categories-sidebar, listing-feed-link, listing-feed-stage,
// listing-item-info stage ─────────────────────────────────────────

#[test]
fn website_listing() {
    let fixture = Fixture {
        name: "website-listing",
        setup: Box::new(|root: &Path| {
            write(
                &root.join("_quarto.yml"),
                concat!(
                    "project:\n",
                    "  type: website\n",
                    "\n",
                    "website:\n",
                    "  title: Idempotence Listing\n",
                    "  site-url: https://example.com/\n",
                ),
            );
            write(
                &root.join("index.qmd"),
                concat!(
                    "---\n",
                    "title: Posts\n",
                    "listing:\n",
                    "  contents: posts\n",
                    "  type: default\n",
                    "  categories: true\n",
                    "  feed: true\n",
                    "---\n",
                    "\n",
                    "Listing index.\n",
                ),
            );
            write(
                &root.join("posts/first.qmd"),
                concat!(
                    "---\n",
                    "title: First Post\n",
                    "categories: [alpha, beta]\n",
                    "date: 2026-05-01\n",
                    "---\n",
                    "\n",
                    "First body.\n",
                ),
            );
            write(
                &root.join("posts/second.qmd"),
                concat!(
                    "---\n",
                    "title: Second Post\n",
                    "categories: [beta]\n",
                    "date: 2026-05-15\n",
                    "---\n",
                    "\n",
                    "Second body.\n",
                ),
            );
        }),
        active: PathBuf::from("index.qmd"),
        modes: ORCHESTRATOR_ONLY,
        attribution_json: None,
    };
    fixture.run_in_each_mode();
}

// =====================================================================
// Phase 4d — attribution fixture
// =====================================================================
//
// Deterministic stub. `PreBuiltAttributionProvider` reads a static
// JSON payload; the writer-side machinery (`AttributionGenerateStage`
// + `AttributionRenderTransform`) then populates `format_options.json`
// and writes per-node attribution records into the AST. Using
// `GitBlameProvider` here would be flaky — depends on actual git
// history; the prebuilt path is the same shape the WASM client
// drives in production.

/// Tiny transport-shape attribution JSON: one actor, one run
/// covering bytes 0..1024 so it overlaps anything the fixture
/// document might contain.
const STUB_ATTRIBUTION_JSON: &str = concat!(
    "{",
    "\"runs\":[{\"start\":0,\"end\":1024,\"actor\":\"alice\",\"time\":1700000000}],",
    "\"identities\":{\"alice\":{\"name\":\"Alice\",\"color\":\"#ff0000\"}}",
    "}"
);

#[test]
fn attribution_basic() {
    let fixture = Fixture {
        name: "attribution-basic",
        setup: Box::new(|root: &Path| {
            // Plenty of bytes for the attribution run to overlap.
            write(
                &root.join("index.qmd"),
                "---\ntitle: Attributed\n---\n\n## Section\n\nA paragraph attributed to alice for the whole byte range.\n",
            );
        }),
        active: PathBuf::from("index.qmd"),
        modes: BOTH_MODES,
        attribution_json: Some(STUB_ATTRIBUTION_JSON),
    };
    fixture.run_in_each_mode();
}

# Plan 3 — Built-in transform and filter idempotence verification (CI-time)

**Date:** 2026-05-04 (revised 2026-05-21)
**Branch:** feature/provenance (long-lived integration branch — see
§"Phase 5 — Failure triage" and §"Long-lived branch policy" below)
**Status:** Development plan (work items below)
**Milestone:** M2 verification gate (no new milestone — locks in property
on what's already shipped)

## Long-lived branch policy

`feature/provenance` is **not** intended to merge to `main` while any
fixture in this plan is red. The integration branch is the *home* of
the failing-test queue; each red fixture has a beads issue, and the
queue is drained before merge. This is by design — the plan ships a
verification gate, and the gate has to be allowed to be red while it
discovers what's actually non-deterministic in the pipeline today.
See §"Phase 5 — Failure triage" for the operational rules, and
§"CI failure policy & sub-agent prompt template" (under §"Decisions")
for the rationale.

## Epic context

Part of the **provenance epic** (Plans 3–8). Plan 3 is the
verification-gate piece: it locks in the idempotence + structural-hash-
stability contract the rest of the epic (typed provenance, incremental
writer, soft-drop) rests on. The file name keeps its q2-preview-plan-N
form for continuity with the earlier discussion notes.

## Goal

Verify and lock in the **idempotence + structural-hash-stability**
contract for the q2-preview pipeline. Every Rust transform in the
q2-preview transform list **and** every built-in Lua filter shipped
under `resources/extensions/` must produce the same structural AST when
run twice on the same input. Without this, the incremental writer's
reconciliation (Plan 7) cannot reliably preserve untouched regions.

This plan ships:

- A canonical fixture set covering each transform and built-in Lua
  filter in scope.
- A test that runs each fixture through the q2-preview pipeline twice
  and asserts the resulting `blocks` and `meta` (excluding
  `rendered.*`) hash equal.
- A `compute_meta_hash_fresh` helper in `quarto-ast-reconcile`
  parallel to the existing `compute_blocks_hash_fresh`.
- Documentation of the idempotence contract for future transform/filter
  authors.

When this plan lands on `main` (after Phase 5's failure queue is
drained), the q2-preview round-trip story (Plans 4-8) rests on a
**CI-enforced** stable foundation: every push to `main` runs the
idempotence suite and fails the build on regression. *Until* the
plan lands on `main`, the integration branch
(`feature/provenance`) carries the suite in a possibly-red state
as the queue of discovered non-determinism issues is worked
through — that's the design, not a process gap. See §"Long-lived
branch policy" and §"Phase 5 — Failure triage."

## Scope

### What "built-in" covers — the universe under test

Two distinct classes, both shipped with Quarto and both in scope:

**Rust transforms** — the source of truth is
`build_q2_preview_transform_pipeline` in
`crates/quarto-core/src/pipeline.rs:1237`, which is
`build_transform_pipeline` minus the four names in
`Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1198`). As of this
revision, the q2-preview pipeline runs **37 transforms** across four
phases:

- **Normalization**: callout, shortcode-resolve, metadata-normalize,
  code-block-generate, website-title-prefix, website-favicon,
  website-bootstrap-icons, website-canonical-url, sectionize,
  footnotes, theorem-sugar, proof-sugar, float-ref-target-sugar,
  equation-label.
- **Crossref**: crossref-index, crossref-resolve.
- **Navigation**: toc-generate, navbar-generate, sidebar-generate,
  page-nav-generate, footer-generate, listing-generate, listing-render,
  categories-sidebar, listing-feed-stage (native only),
  listing-feed-link, toc-render, navbar-render, sidebar-render,
  page-nav-render, footer-render.
- **Finalization**: link-rewrite, appendix-structure, code-block-render,
  resource-collector, table-bootstrap-class, attribution-render.

Excluded by `Q2_PREVIEW_TRANSFORM_EXCLUDED` (out of scope for Plan 3
because they don't run): callout-resolve, attribution-viewer,
title-block, crossref-render.

**Stage-level work** in `build_q2_preview_pipeline_stages`
(`pipeline.rs:380`) also runs around `AstTransformsStage` and can
introduce non-determinism: parse-document, metadata-merge,
include-expansion, include-resolve, listing-item-info, document-profile,
link-resolution, unwrap-profile, pre-engine-sugaring, capture-splice,
engine-execution, compile-theme-css, attribution-generate,
user-filters-pre/post, resource-report, code-highlight. These are
exercised implicitly by every fixture (most are no-ops absent specific
metadata).

`Q2_PREVIEW_STAGE_EXCLUDED` (`pipeline.rs:356`) currently excludes
three stages by name: `math-js`, `render-html-body`, and
`apply-template`. `MathJsStage`'s exclusion means `meta.math` never
appears under this pipeline and contributes nothing to the meta
hash; `RenderHtmlBodyStage` and `ApplyTemplateStage` produce
HTML/text side outputs that wouldn't reach the AST anyway, so their
exclusion is also AST-neutral. `BootstrapJsStage` and
`ClipboardJsStage` are *not* excluded — they run on native q2-preview
but write only to `ctx.artifacts`, not to `doc.ast.meta` or
`doc.ast.blocks`, so they don't affect the hash. (Whether they
should be in `Q2_PREVIEW_STAGE_EXCLUDED` at all is a separate
question, filed as **bd-2ag1c** — see §"Open questions for
implementation" for ordering relative to Plan 3.)

**Lua filters under `resources/extensions/`** — there is exactly **one**
today: `resources/extensions/quarto/video/video-filter.lua`. It rewrites
Header attributes when `background-video` is set on a slide-shaped
header. (The other Lua files in `resources/extensions/` — kbd, video,
lipsum, version, placeholder — are *shortcodes*, not filters, and run
through `shortcode-resolve` rather than `UserFiltersStage`. They're
exercised via shortcode fixtures.)

### In scope

- **Canonical fixture set**: small `.qmd` files exercising each
  transform / filter in the universe above. Existing fixtures + new
  ones from the gap audit below. Detailed listing in §"Coverage gaps to
  address during implementation."

- **`compute_meta_hash_fresh` helper** in
  `crates/quarto-ast-reconcile/src/hash.rs`. Walks `ConfigValue`
  source-info-agnostically:
  - hashes scalars by their `Yaml` payload;
  - recurses into `PandocInlines` / `PandocBlocks` via the existing
    inline / block hashers;
  - hashes `Array` entries in order (matches `Vec<ConfigValue>` shape);
  - hashes `Map` entries as `(key_string, recurse(value))` pairs **in
    insertion order — no sort**. Insertion-order hashing is the right
    choice for an idempotence test: it catches HashMap-iteration-order
    bugs in transforms that stuff results into a meta `Map`. Sorting
    would silently mask exactly the class of non-determinism we want to
    detect. `ConfigValue::Map` is already a `Vec<ConfigMapEntry>` that
    preserves YAML document order, so hashing insertion order is also
    the simplest implementation;
  - **includes `merge_op`** in the hash (every `ConfigValue` has
    `value: ConfigValueKind`, `source_info: SourceInfo`, and
    `merge_op: MergeOp` — `merge_op` participates so we catch
    transforms that change merge semantics non-deterministically).
    `MergeOp::default()` is `Concat`
    (`crates/quarto-pandoc-types/src/config_value.rs:75`, derived
    `#[default]`) — a stable compile-time constant with no env or
    runtime dependence, so transforms that leave `merge_op` at its
    default contribute a deterministic value to the hash;
  - skips `source_info` and `key_source` (Plan 4's churn must not break
    the contract).

  Tests for the helper land alongside it (mirroring the existing
  `test_same_content_same_hash` style at `hash.rs:767`). Include a test
  proving the helper diverges when `Map` insertion order changes — this
  is the regression guard for the no-sort choice.

- **Idempotence test runner**: takes a fixture, runs the q2-preview
  pipeline twice (once per `DriveMode` — see §"What gets tested
  concretely"), hashes `doc.ast.blocks` via `compute_blocks_hash_fresh`
  and `doc.ast.meta` via `compute_meta_hash_fresh_excluding_rendered`
  (everything under `rendered.*` is HTML/text side output — see §"Out
  of scope"). Asserts hash equality across the two runs *within a
  mode*. One assertion per (fixture, mode) pair; failures name the
  fixture, the mode, and which hash diverged.

- **Divergence-localization helper** in
  `crates/quarto-ast-reconcile/src/hash.rs`, alongside the hash fns.
  When the (blocks, meta) hashes diverge, the test driver calls
  `find_first_divergence(&doc_1, &doc_2) -> DivergencePoint` to
  surface a useful location in the failure message. Returns one of:
  - `DivergencePoint::Block { index, hash_a, hash_b }` — first block
    index whose `compute_block_hash_fresh` differs;
  - `DivergencePoint::MetaKey { path, hash_a, hash_b }` — first meta
    key path (e.g. `["listings", "foo", "items"]`) whose recursive
    hash differs, walking the `ConfigValue` tree in insertion order
    and excluding `rendered.*`;
  - `DivergencePoint::None` — hashes equal at top but a sub-component
    differs (would indicate a bug in the hasher itself; vanishingly
    unlikely with FxHasher).

  The test driver embeds the returned `DivergencePoint` in the panic
  message, so the sub-agent investigation prompt arrives with a
  concrete starting point ("block index 7" / "meta.listings.foo
  diverged") rather than just "hash diverged." Saves agent triage
  time and makes the sub-agent prompt template (§"Open questions for
  implementation") fillable from the panic message alone.

- **Documentation** in `claude-notes/instructions/`: a short note on the
  idempotence contract for transform and filter authors, including the
  meta-hash-excludes-`rendered.*` rule and how to add a fixture when
  introducing a new transform.

### Out of scope

- **Round-trip non-idempotence**
  (`pipeline(write(pipeline(x))) ≠ pipeline(x)`). Plan 7a's runtime
  check handles this. Plan 3 deliberately tests only pipeline
  non-determinism — see §"Pipeline-determinism only" below.
- **User-supplied filters**. Per-document, per-user; Plan 7a covers
  these at runtime with an `idempotent: false` opt-out.
- **Rust-vs-React rendering parity**. Different contract; later plan.
- **Performance / debouncing**. Idempotence verification doesn't
  measure runtime.
- **Engine execution non-determinism**. CI doesn't run jupyter / knitr;
  fixtures must contain only fenced code blocks (AST-level), not
  executable code cells. The `engine-execution` stage is a no-op on
  fixtures with no engine cells; the `capture-splice` stage is a
  pass-through when no capture is supplied. See §"No executable engine
  cells" below.
- **Chrome HTML-string canonicalization**. Meta hash skips
  `rendered.*` because those are HTML strings populated by
  navbar-render / sidebar-render / etc.; semantically-equal but
  textually-different HTML would fail a strict comparison. Structural
  non-determinism in chrome transforms shows up elsewhere (e.g., a
  navbar transform that emits attributes in non-canonical order
  inside its HTML still produces a stable hash *of the meta key
  containing the HTML* across runs, because both runs go through
  the same code path — what we're missing is HTML-shape determinism,
  which is a separate concern best tested with HTML snapshots).
- **`meta.rendered.includes.*` HTML/text strings**. Written by
  `IncludeResolveStage` (user-supplied `include-in-header` /
  `before-body` / `after-body` files), `WebsiteFaviconTransform`
  (favicon `<link>`), `attribution_viewer` (CLI-only — q2-preview
  excludes it), and Bootstrap/clipboard injection on the HTML path.
  These all sit under `rendered.*` and are skipped by
  `compute_meta_hash_fresh_excluding_rendered`. If we ever want to
  cover the includes subtree separately (catch a transform that
  shuffles include-file ordering, say), the right shape is a separate
  helper, not a partial inclusion of the rendered subtree.

### No executable engine cells

CI does not execute engine cells. Fixtures must:

- Use only fenced code blocks (`` ```python ``, ` ```r `, etc.) — AST
  nodes, not executed.
- NOT use `{python}` / `{r}` / `{julia}` style executable cells.

If a fixture happens to include an executable cell, the
`engine-execution` stage will either fail (no kernel available) or
fall through to the markdown passthrough. Either way the test is
unreliable. The fixture-format documentation enforces this.

## Pipeline-determinism only — round-trip is Plan 7a's job

Two distinct properties get loosely called "non-idempotence":

1. **Pipeline non-determinism**: `pipeline(x)` produces different
   output on repeat calls. Caused by time / RNG / mutable global state
   / undefined-order iteration. **This is what Plan 3 tests.**

2. **Round-trip non-idempotence**:
   `pipeline(write(pipeline(x))) ≠ pipeline(x)`. The pipeline doesn't
   re-parse its own output today; this becomes a concern only when
   Plan 7's incremental writer lands. Plan 7a covers (2) at runtime
   for **user-supplied** Lua filters, with per-filter attribution and
   an `idempotent: false` opt-out. **Built-in** filter round-trip is
   not covered by any plan in the epic (see Plan 7a's §"Notes" for
   the accepted-gap reasoning).

Plan 3 deliberately scopes to (1) because:

- (2) isn't exercised by today's pipeline.
- (2)'s test conflates writer-lossiness with filter-non-idempotence;
  Plan 7's writer-lossless baseline test (planned for Plan 7's first
  commit) and Plan 7a's per-filter isolation disambiguate the user
  filter case.
- For built-ins, the universe is small (one Lua filter +
  ~36 Rust transforms, all under our control); if (2) bites us in
  production after Plan 7 ships, the fix is to extend Plan 7a's
  runtime check to also fire on `FilterSource::Extension` filters —
  a small follow-up tracked in 7a's §"Out of scope."

See Plan 7a's §"Two flavors of non-idempotence" for the full
treatment.

## Design decisions (settled in conversation)

- **The hash is source-info-agnostic** (verified). `compute_block_hash_fresh`
  excludes `source_info`; the new `compute_meta_hash_fresh` will do the
  same for `ConfigValue::source_info` and `ConfigMapEntry::key_source`.
  Test asserting this lives at `hash.rs:767` for blocks; equivalent
  test lands for meta.
- **`merge_op` participates; map keys hashed in insertion order, no
  sort.** See the helper spec in §"In scope" for the full reasoning.
  In one line: an idempotence test wants to *catch* the kind of
  non-determinism a sort would hide.
- **Hash covers blocks and meta-minus-`rendered.*`**. Meta inclusion
  catches non-determinism in metadata-normalize, listing data,
  shortcode-resolved meta values, attribution metadata, etc. The
  `rendered.*` keys are HTML strings populated by chrome-render
  transforms; their canonicalization is a separate concern.
- **Filter mutation provenance stays Original** (post-Plan 4 unified
  `Generated { by: By::filter(...), anchors: [] }` shape). Idempotence
  test sees consistent shape across runs.
- **Each pipeline run uses fresh Lua state.** Two construction sites,
  both verified fresh per pipeline invocation:
  - **User filters**: `apply_lua_filter` (singular, at
    `crates/pampa/src/lua/filter.rs:158`) constructs a fresh
    `Lua::new()` per filter. The outer `apply_lua_filters` (plural, at
    line 270) loops over `filter_paths` and calls the singular form
    once per filter, so every filter in every run starts from a clean
    Lua state.
  - **Shortcodes**: `LuaShortcodeEngine::new`
    (`crates/pampa/src/lua/shortcode.rs:68`) is constructed on the
    stack inside `ShortcodeResolveTransform::transform()` at
    `crates/quarto-core/src/transforms/shortcode_resolve.rs:513`, so
    each pipeline run also gets a fresh shortcode-side `Lua::new()`.

  No cross-run state accumulation on either side. This matches
  production (hub-client builds a new pipeline per render) and
  resolves the prior "second-run pipeline starts fresh?" open
  question.
- **Built-in scope = Rust transforms + ship-with-Quarto Lua filters**.
  User filters are out of scope here (Plan 7a covers them).

## What gets tested concretely

Every fixture runs through **two pipeline-driver modes**, both compared
against themselves:

1. **Single-file mode** — `run_pipeline` directly with
   `build_q2_preview_pipeline_stages`. Mirrors the lowest-level entry
   point used by `render_qmd_to_preview_ast` (`pipeline.rs:859`).
2. **Project-orchestrator mode** — calls the existing
   `render_active_page_preview` helper at
   `crates/quarto-core/tests/render_page_in_project.rs:660`. That
   helper already drives
   `ProjectPipeline<RenderToPreviewAstRenderer>` end-to-end (project
   discovery, multi-file re-discovery guard, format setup, `ActivePage`
   mode), returns `WasmPassTwoOutput`, and panics on pass-1 / pass-2
   failures. It is exactly the path the real `q2 preview` and
   hub-client renders take. We use it as-is; no fresh orchestrator
   wiring is required.

Why both: single-file mode catches stage / transform non-determinism;
project mode additionally exercises any non-determinism introduced by
the orchestrator itself (project discovery, ProjectIndex assembly,
file-iteration order, pass-1 → pass-2 hand-off).

```rust
use quarto_ast_reconcile::{compute_blocks_hash_fresh, compute_meta_hash_fresh_excluding_rendered};
use quarto_core::format::Format;
use quarto_core::pipeline::{build_q2_preview_pipeline_stages, run_pipeline};
use quarto_core::project::pass2_renderer::WasmPassTwoOutput;
use quarto_core::stage::{DocumentAst, PipelineData};
use quarto_pandoc_types::Pandoc;
use quarto_system_runtime::NativeRuntime;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

/// How a fixture is driven through the pipeline. Every fixture runs
/// once per mode; both modes hash equal across two runs.
#[derive(Clone, Copy, Debug)]
enum DriveMode {
    /// `run_pipeline` directly with `build_q2_preview_pipeline_stages`.
    SingleFile,
    /// Reuses the existing `render_active_page_preview` helper at
    /// `crates/quarto-core/tests/render_page_in_project.rs:660`.
    ProjectOrchestrator,
}

/// A test fixture. The whole project lives in a `TempDir` that the
/// fixture owns; the `_quarto.yml` (if any) plus the page contents
/// are written by `setup()`. Document-only fixtures still create a
/// temp dir + minimal `index.qmd` so the orchestrator mode has
/// something to discover.
struct Fixture {
    name: &'static str,
    /// Idempotent setup callback. Receives the project root.
    /// Must write at minimum `<root>/<active>` (the page being rendered),
    /// optionally `<root>/_quarto.yml` and sibling files.
    setup: Box<dyn Fn(&Path)>,
    /// The active page, relative to the project root. Defaults to
    /// `index.qmd`.
    active: PathBuf,
}

fn run_fixture(fixture: &Fixture, mode: DriveMode) {
    let doc_1 = run_q2_preview(fixture, mode);
    let doc_2 = run_q2_preview(fixture, mode);

    let blocks_a = compute_blocks_hash_fresh(&doc_1.blocks);
    let blocks_b = compute_blocks_hash_fresh(&doc_2.blocks);
    let meta_a = compute_meta_hash_fresh_excluding_rendered(&doc_1.meta);
    let meta_b = compute_meta_hash_fresh_excluding_rendered(&doc_2.meta);

    if blocks_a != blocks_b || meta_a != meta_b {
        // Localize before panicking so the failure message gives the
        // sub-agent prompt a concrete starting point.
        let point = find_first_divergence(&doc_1, &doc_2);
        panic!(
            "fixture {} ({mode:?}): non-idempotent\n  \
             blocks: {blocks_a:016x} vs {blocks_b:016x}\n  \
             meta:   {meta_a:016x} vs {meta_b:016x}\n  \
             first divergence: {point:?}",
            fixture.name,
        );
    }
}

fn run_q2_preview(fixture: &Fixture, mode: DriveMode) -> DocumentAst {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    (fixture.setup)(&project_dir);
    let active = project_dir.join(&fixture.active).canonicalize().unwrap();

    match mode {
        DriveMode::SingleFile => run_single_file(&project_dir, &active),
        DriveMode::ProjectOrchestrator => run_orchestrator(&project_dir, &active),
    }
}

fn run_single_file(project_dir: &Path, active: &Path) -> DocumentAst {
    // `run_pipeline` is async; the existing tests (e.g.
    // render_page_in_project.rs) drive it via `pollster::block_on`.
    pollster::block_on(async {
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(NativeRuntime::new());
        let mut project = quarto_core::project::ProjectContext::discover(
            active,
            runtime.as_ref(),
        )
        .unwrap();
        if !project.is_single_file {
            project = quarto_core::project::ProjectContext::discover(
                &project.dir,
                runtime.as_ref(),
            )
            .unwrap();
        }
        let doc = project
            .documents
            .iter()
            .find(|d| d.path == active)
            .expect("active doc in project")
            .clone();
        let format = Format::from_format_string("q2-preview")
            .expect("q2-preview is a recognized pseudo-format");
        let binaries = quarto_core::render::BinaryDependencies::new();
        let mut ctx = quarto_core::render::RenderContext::new(
            &project, &doc, &format, &binaries,
        );

        let content = std::fs::read(active).unwrap();
        let stages = build_q2_preview_pipeline_stages(None, None);
        let (output, _diagnostics) = run_pipeline(
            &content,
            &active.to_string_lossy(),
            &mut ctx,
            runtime,
            stages,
        )
        .await
        .expect("pipeline run");

        match output {
            PipelineData::DocumentAst(ast) => ast,
            other => panic!("expected DocumentAst, got {:?}", other.kind()),
        }
    })
}

fn run_orchestrator(project_dir: &Path, active: &Path) -> DocumentAst {
    // Delegates to the existing helper. It already drives
    // ProjectContext::discover + ProjectPipeline + ActivePage mode,
    // and panics if pass-1 / pass-2 surface failures. We just lift
    // the AST JSON out of `Pass2Payload::AstJson` and re-parse it
    // back into a typed Pandoc via pampa's JSON reader — the source_info
    // round-trips, but the hash explicitly excludes source_info, so
    // the parse is a clean conversion for hashing purposes.
    let output: WasmPassTwoOutput =
        render_active_page_preview(project_dir, active);
    let ast_json = output
        .payload
        .as_ast_json()
        .expect("orchestrator must emit Pass2Payload::AstJson");
    let mut bytes = ast_json.as_bytes();
    let (pandoc, _ast_ctx) = pampa::readers::json::read(&mut bytes)
        .expect("re-parse AST JSON");
    pandoc_to_document_ast(pandoc)
}

// `pandoc_to_document_ast` converts the re-parsed `Pandoc` into the
// `DocumentAst` shape the hash helpers want. Pandoc carries blocks
// + the document meta; the helpers take `&[Block]` and `&ConfigValue`
// respectively, so this is mostly a field shuffle. Exact body
// determined during implementation once the DocumentAst struct is
// inspected next to Pandoc.
```

Notes on the helpers:

- `run_pipeline` (`pipeline.rs:627`) is the existing entry point for
  the single-file mode; no new driver is needed.
- The q2-preview pipeline ends at `CodeHighlightStage`, so its output
  is `PipelineData::DocumentAst`.
- Each call constructs fresh `StageContext` (inside `run_pipeline` or
  inside the orchestrator's per-page renderer setup) and fresh Lua
  engines per filter / shortcode invocation — natural per-run
  isolation.
- The orchestrator path's `Pass2Payload::as_ast_json()` accessor
  (`crates/quarto-core/src/project/pass2_renderer.rs:272`) already
  exists. `pampa::readers::json::read` (`crates/pampa/src/readers/json.rs:1063`)
  parses the JSON back into a typed `Pandoc`. The source_info that
  the JSON writer emits with `include_inline_locations: true`
  (`crates/quarto-core/src/pipeline.rs:910` area) round-trips through
  the reader, but **the hash explicitly excludes source_info** — so
  no stripping pass is required and no production plumbing change is
  needed. See §"Decisions" /
  §"Orchestrator-mode `DocumentAst` extraction" for why option (a)
  beats the typed-plumbing alternative.

### Fixture-to-mode mapping

Not every fixture is meaningful in every mode:

| Fixture class | Single-file | Project-orchestrator |
|---|---|---|
| Plain document (`callout-warning`, `theorem`, `code-block-fenced`, …) | ✓ | ✓ (one-page project) |
| Website chrome (`website-chrome`, `website-links`, `website-listing`) | n/a (chrome stages need ProjectContext) | ✓ |
| Attribution (`attribution-basic`) | ✓ (provider on RenderContext) | ✓ |

Document fixtures run in both modes against the *same* fixture content
(the orchestrator wraps the document in a tiny synthetic project).
Website fixtures run orchestrator-only because the chrome transforms
require a populated ProjectIndex; running them through single-file
mode would test a partial pipeline that doesn't exist in production.

### Failure modes the test catches

- A filter that's truly non-idempotent (e.g., `Str.text + "!"` →
  growing text on each run).
- A transform that emits non-deterministic attributes or `plain_data`
  (e.g., HashMap iteration order in a sloppy implementation).
- A transform that mutates inputs differently across runs (probably a
  bug).
- A metadata transform that synthesizes meta keys non-deterministically
  (e.g., listing-item-info that gets file-mtime in racy ways).

### Failure modes the test does NOT catch

- A transform that's idempotent but produces *wrong* output (wrong-but-
  consistent — needs other testing).
- A filter that's idempotent for one input but non-idempotent for
  another (need representative fixtures).
- Round-trip non-idempotence — see §"Pipeline-determinism only" above
  and Plan 7a.
- HTML-shape non-determinism inside `meta.rendered.*` (excluded from
  the hash).

## Coverage gaps to address during implementation

Each fixture below covers one or more transforms. Existing fixtures are
marked; new ones are unchecked.

**Existing fixtures (carry forward from prior plan draft):**

- [ ] `meta-single` — `{{< meta foo >}}` with single-string foo →
  shortcode-resolve, metadata-normalize.
- [ ] `meta-markdown` — `{{< meta foo >}}` with `**Bold** title` →
  shortcode-resolve (PandocInlines branch).
- [ ] `include-trivial` — `{{< include child.qmd >}}` →
  include-expansion stage, shortcode-resolve.
- [ ] `callout-warning` — `::: {.callout-warning} Body :::` → callout.
  (callout-resolve is excluded; CustomNode survives.)
- [ ] `theorem` — `::: {.theorem #thm-foo} Math here :::` →
  theorem-sugar.
- [ ] `figure-ref-target` — `:::: {#fig-foo} ![cap](img.png) ::::` →
  float-ref-target-sugar.
- [ ] `crossref-to-theorem` — `See @thm-foo` paired with the theorem
  above → crossref-index, crossref-resolve.
- [ ] `sectionize-multi` — `## A` / `### B` / `## C` with body →
  sectionize.
- [ ] `footnotes-mixed` — inline `^[...]` + reference `[^foo]` →
  footnotes.
- [ ] `appendix-license` — `license:` / `copyright:` meta +
  `:::{.appendix}` user block + footnotes → appendix-structure
  (+ footnotes interaction).
- [ ] `combined-stress` — sectionize + callouts + shortcodes
  interacting.

**New fixtures (gap audit):**

- [ ] `code-block-fenced` — fenced ``` ```python ``` block with content
  → code-block-generate, code-block-render, code-highlight stage.
- [ ] `lua-shortcode-version` — `{{< version >}}` → shortcode-resolve
  (Lua-loaded handler path; simplest deterministic case — returns
  `quarto.version` joined by dots).
- [ ] `lua-shortcode-lipsum-fixed` — `{{< lipsum 3 >}}` (no `random=`
  kwarg) → shortcode-resolve via lipsum's Lua handler. The
  `math.randomseed` in `lipsum.lua:5` runs but `math.random` is never
  called on this code path, so the output is the first three
  paragraphs of the canned data deterministically. The `random=true`
  variant is intentionally non-deterministic and out of scope.
- [ ] `proof` — `::: {.proof} ... :::` → proof-sugar.
- [ ] `equation-labeled` — `$$ E=mc^2 $$ {#eq-mass}` paired with
  `@eq-mass` → equation-label, crossref-resolve (equation branch).
- [ ] `toc-on` — `toc: true` + multiple sections → toc-generate,
  toc-render.
- [ ] `video-filter-header` — exercises
  `resources/extensions/quarto/video/video-filter.lua` (the only
  built-in Lua filter under `resources/extensions/`). The `quarto/video`
  extension is **embedded at compile time** (`include_dir!` of
  `resources/extensions/` in
  `crates/quarto-core/src/extension/mod.rs:33`) and auto-discovered for
  every `StageContext::new()` call (`stage/context.rs:221`), so the
  fixture needs no scaffolding beyond declaring the filter. Minimal
  shape:

  ```yaml
  ---
  filters:
    - video
  ---

  # Title {background-video="https://www.youtube.com/embed/abc"}
  ```

  The filter rewrites `background-video` → `background-iframe` on
  Headers whose URL matches one of three video hosts. Pattern matches
  the smoke-test at
  `crates/quarto/tests/smoke-all/extensions/filter-extension/test.qmd`.
- [ ] `include-in-header` — `include-in-header: foo.html` in meta with
  trivial `foo.html` → include-resolve stage.
- [ ] `theme-bootstrap` — `theme: cosmo` (or default) in meta →
  compile-theme-css stage.
- [ ] `table-bootstrap-class` — a simple pipe table (`| col |
  --- | val |`) → `TableBootstrapClassTransform`. The transform
  attaches Bootstrap CSS classes (`table`, etc.) to `Table` nodes;
  the assertion that the same classes appear in the same order on
  both runs is the idempotence check. Minimal shape: one
  two-column, two-row pipe table; no extra config needed.

**Website-project fixtures** (each needs a `ProjectContext` wired to a
`_quarto.yml` with `project.type: website` + the relevant config; one
combined fixture can cover most chrome transforms):

- [ ] `website-chrome` — minimal website with navbar, sidebar, page
  navigation, footer, favicon, bootstrap icons → website-title-prefix,
  website-favicon, website-bootstrap-icons, website-canonical-url,
  navbar-generate/render, sidebar-generate/render, page-nav-generate/render,
  footer-generate/render, link-resolution stage.
- [ ] `website-links` — internal `.qmd` body links between two project
  pages → link-rewrite + link-resolution.
- [ ] `website-listing` — minimal listing with two items, one with
  categories, one with `feed:` config → listing-generate, listing-render,
  categories-sidebar, listing-feed-link, listing-feed-stage (native only),
  listing-item-info stage.

**Attribution fixture** (the test helper installs an
`AttributionSourceProvider` on `RenderContext.attribution_provider`;
`run_pipeline` forwards it to `StageContext.attribution_provider` at
`pipeline.rs:664`):

- [ ] `attribution-basic` — document with an installed git-based
  attribution provider → attribution-generate stage, attribution-render
  transform.

**Resource fixture:**

- [ ] `resource-image` — `![alt](./local.png)` with the image file
  present → resource-collector.

If a fixture in this list discovers non-idempotence on first run,
**leave the test failing** and file a beads issue using the sub-agent
investigation prompt template in §"CI failure policy & sub-agent
prompt template."
The fix lands against the appropriate transform's crate (per §"What
happens when a fixture fails"). Do not silently drop the fixture, and
do not `#[ignore]` it without explicit user approval — failing tests
are the triage backlog.

## Decisions (was: open questions)

- **Test crate location** — settled. The test lives at
  `crates/quarto-core/tests/idempotence.rs` as a workspace-level
  integration test (matches the existing pattern in
  `crates/quarto-core/tests/` — `sidebar_pipeline.rs`,
  `navbar_footer_pipeline.rs`, `render_page_in_project.rs`, etc.).
  Invoke with `cargo nextest run -p quarto-core --test idempotence`.
- **Fixture location** — settled. Files in
  `crates/quarto-core/tests/fixtures/idempotence/`, one subdirectory
  per non-trivial fixture (for the website/multi-file ones); in-source
  literals for the trivial single-page cases written by the fixture's
  `setup` closure into a `TempDir`. Pattern matches
  `crates/quarto-core/tests/fixtures/websites/hub-smoke/` and
  `phase5-website-baseline/`.
- **`ProjectContext` setup for website fixtures** — resolved by
  reuse. There is **no need** to write a `make_website_project_ctx`
  helper. The existing pattern across `crates/quarto-core/tests/`
  (used by `render_page_in_project.rs`, `sidebar_pipeline.rs`,
  `navbar_footer_pipeline.rs`, `page_navigation_pipeline.rs`,
  `listing_pipeline.rs`, `navigation_e2e.rs`, `link_rewriting_pipeline.rs`,
  `website_post_render.rs`) is: write `_quarto.yml` + page contents
  into a `TempDir`, then let `ProjectContext::discover` do the rest.
  Each Plan 3 fixture's `setup` closure does exactly this in 5-30
  lines. The chrome transforms read their config from the discovered
  project — they don't need a parameterized builder. For the website
  fixtures (`website-chrome`, `website-links`, `website-listing`)
  we can either inline the YAML in `setup` or, for the larger ones,
  use `copy_fixture(...)` (see `render_page_in_project.rs:616`) to
  pull a pre-built fixture directory out of `tests/fixtures/idempotence/`.
- **Fixture-authoring rules for path-recording transforms** —
  settled. Fixtures that exercise `resource-collector`,
  `include-resolve`, `BUILTIN_EXTENSIONS` (any built-in extension
  lookup), or other transforms that record absolute paths into meta
  MUST use only paths that resolve relative to the fixture root,
  never absolute process paths. Reason: the built-in extensions
  resource bundle extracts to a `temp_dir()`'d location whose
  absolute path differs across processes (stable within a single
  process — fine for Plan 3's two-runs-compare contract, but a
  latent issue for any future stored-snapshot variant). The
  fixtures README must spell this out. Two practical rules:
  (1) use relative URLs in fixture body content (`./local.png`,
  not `/private/var/.../local.png`);
  (2) when a transform's output includes a path, the assertion must
  hash the value through `compute_meta_hash_fresh_excluding_rendered`
  (which we already do) so test-process-specific paths under
  `rendered.*` are excluded by construction.
- **Orchestrator-mode `DocumentAst` extraction** — settled on
  option (a). The orchestrator path emits the AST as a JSON string
  via `Pass2Payload::AstJson`; `Pass2Payload::as_ast_json()`
  (`crates/quarto-core/src/project/pass2_renderer.rs:272`) is
  already in the API. `pampa::readers::json::read`
  (`crates/pampa/src/readers/json.rs:1063`) parses it back into a
  typed `(Pandoc, ASTContext)`. The JSON writer emits source_info
  triples (`include_inline_locations: true`), and those round-trip
  through the reader — but **the hash explicitly excludes
  source_info** (`compute_blocks_hash_fresh` /
  `compute_meta_hash_fresh` both skip it), so no stripping pass is
  needed. Cost: one JSON-string parse per orchestrator-mode
  assertion, no production plumbing change. The earlier draft of
  this section preferred option (b) (forward typed `DocumentAst`
  through `PreviewAstOutput` / `WasmPassTwoOutput`) — abandoned
  because (a) needs *no* type changes and the source_info concern
  doesn't actually bite the hash.
- **bd-2ag1c ordering** — Plan 3 lands first; bd-2ag1c (whether
  `BootstrapJsStage` / `ClipboardJsStage` belong in
  `Q2_PREVIEW_STAGE_EXCLUDED`) waits for Plan 3's coverage. The
  rationale: Plan 3 is what *measures* whether those stages
  contribute non-determinism to the q2-preview AST; if they don't
  (they currently only write to `ctx.artifacts`, not to `meta` or
  `blocks`), bd-2ag1c can be closed without changes. If they do,
  bd-2ag1c picks up the cleanup with measurements in hand.

### CI failure policy & sub-agent prompt template

The test fails noisily if any transform / filter is non-idempotent
— that's the point. Failing fixtures stay **failing** (no
auto-`#[ignore]`). For each failure, file a beads issue whose
description doubles as a self-contained sub-agent investigation
prompt: the fixture path, the two hash values, the diverging key
path (block vs meta), and the suspected stage / transform / filter
to focus on. `#[ignore]` is only applied when the user explicitly
says so. **The integration branch (`feature/provenance`) is allowed
to be red while the queue is being drained** — see §"Long-lived
branch policy" at the top of this plan and §"Phase 5 — Failure
triage" for the operational mechanics.

Sub-agent prompt template (filled in per failure when filing the
beads issue — the test driver's panic message provides the
fixture, mode, hashes, and `DivergencePoint`, so the agent already
has a concrete starting point):

> Investigate non-idempotence in q2-preview fixture
> `<fixture-name>` (`<DriveMode>` mode). Two consecutive pipeline
> runs over the same input diverge at
> `<DivergencePoint from panic message — e.g. "Block { index: 7 }"
> or "MetaKey { path: ["listings", "foo"] }">`. Hashes: blocks
> `<a>` vs `<b>`, meta `<a>` vs `<b>`. Read
> `claude-notes/plans/<this-plan>.md` §"Failure modes the test
> catches" for category guidance. Reproduce with `cargo nextest
> run -p quarto-core --test idempotence <fixture-name>`.
> Suspected source likely lives in `<transform-or-stage>` based on
> the divergence location — start there. Verdict: deterministic
> source (HashMap iteration, time, RNG) → propose a fix;
> non-deterministic but semantically equivalent (e.g. attribute
> ordering inside an HTML chrome payload) → propose either
> canonicalization at the source or a targeted hash exclusion. Do
> not `#[ignore]` the test.

## References

Line numbers below are accurate as of `feature/provenance` HEAD on
2026-05-21. Plan 4's source_info churn or any pipeline reorganization
may shift them — when in doubt, grep by symbol name. The plan's
factual content survives line-number drift; the references are a
convenience for navigating, not a contract.

- `crates/quarto-core/src/pipeline.rs:1237`
  `build_q2_preview_transform_pipeline` — q2-preview transform list,
  source of truth.
- `crates/quarto-core/src/pipeline.rs:1198`
  `Q2_PREVIEW_TRANSFORM_EXCLUDED` — the four transforms that don't run.
- `crates/quarto-core/src/pipeline.rs:380`
  `build_q2_preview_pipeline_stages` — stage-level pipeline.
- `crates/quarto-core/src/pipeline.rs:356`
  `Q2_PREVIEW_STAGE_EXCLUDED` — three excluded stages
  (`math-js`, `render-html-body`, `apply-template`).
- `crates/quarto-core/src/pipeline.rs:627`
  `run_pipeline` — pipeline execution entry point used by the test
  runner.
- `crates/quarto-core/src/pipeline.rs:859`
  `render_qmd_to_preview_ast` — production entry point that combines
  `build_q2_preview_pipeline_stages` + `run_pipeline`; mirrors the
  `DriveMode::SingleFile` helper.
- `crates/quarto-core/src/pipeline.rs:168`
  `PreviewAstOutput` — currently carries only `ast_json: String`
  (no typed `DocumentAst`).
- `crates/quarto-core/src/transforms/` — the Rust transform crate root.
  Each transform's `name()` matches the kebab-case strings listed in
  §"What 'built-in' covers."
- `crates/quarto-core/src/transforms/code_highlight.rs:126`
  `CodeHighlightStage`'s native user-grammar disk scan
  (`ctx.project.dir.join("_quarto").join("grammars")`). OS-order-
  dependent if a grammar directory is present; not exercised by
  Plan 3 fixtures (see §"Noted, not actively tested").
- `crates/quarto-core/src/transforms/shortcode_resolve.rs:513`
  `ShortcodeResolveTransform::transform` — site of the per-pipeline
  fresh `LuaShortcodeEngine::new` construction.
- `crates/quarto-ast-reconcile/src/hash.rs:115`
  `compute_blocks_hash_fresh` — the existing blocks hasher (slice).
- `crates/quarto-ast-reconcile/src/hash.rs:102`
  `compute_block_hash_fresh` — the singular per-block hasher used
  by `find_first_divergence` for index-keyed comparison.
- `crates/quarto-ast-reconcile/src/hash.rs:767`
  `test_same_content_same_hash` — confirms blocks hash excludes
  source_info.
- `crates/pampa/src/lua/filter.rs:158`
  `apply_lua_filter` — per-filter Lua engine creation point (singular).
  Driven by `apply_lua_filters` (plural, line 270), which loops over
  `filter_paths` and calls the singular form once per filter.
- `crates/pampa/src/lua/shortcode.rs:68`
  `LuaShortcodeEngine::new` — per-pipeline Lua engine for shortcodes
  (constructed on the stack inside
  `ShortcodeResolveTransform::transform`).
- `crates/pampa/src/readers/json.rs:1063`
  `pampa::readers::json::read` — re-parses AST JSON back into a
  typed `(Pandoc, ASTContext)`; used by `DriveMode::ProjectOrchestrator`
  to recover a typed AST for hashing.
- `crates/quarto-core/src/project/pass2_renderer.rs:272`
  `Pass2Payload::as_ast_json` — accessor used by both the existing
  test in `render_page_in_project.rs` and Plan 3's orchestrator-mode
  helper.
- `crates/quarto-core/src/project/pass2_renderer.rs:254`
  `Pass2Payload::AstJson` — variant currently carries only
  `ast_json: String`.
- `crates/quarto-core/src/stage/context.rs:221`
  `StageContext::new` — calls `discover_extensions` with the embedded
  built-in extensions path, so the `quarto/video` filter extension is
  always discoverable without per-fixture scaffolding.
- `crates/quarto-core/src/extension/mod.rs:33`
  `BUILTIN_EXTENSIONS_DIR` — compile-time
  `include_dir!(resources/extensions)` ensures the video/lipsum/version/
  kbd/placeholder extensions are baked into the binary.
- `crates/quarto-core/tests/render_page_in_project.rs:660`
  `render_active_page_preview` — the **existing**
  `DriveMode::ProjectOrchestrator` helper. Reused verbatim by Plan 3,
  not reimplemented.
- `crates/quarto-core/tests/render_page_in_project.rs:64`
  `render_active_page` — sibling HTML helper; useful prior art for
  the project-discovery pattern even though Plan 3 doesn't use it
  directly.
- `crates/quarto-core/tests/render_page_in_project.rs:616`
  `copy_fixture` — utility for copying a pre-built fixture directory
  out of `tests/fixtures/` into a `TempDir`. Available for the
  heavier website fixtures.
- `crates/quarto-core/tests/fixtures/websites/hub-smoke/`,
  `crates/quarto-core/tests/fixtures/phase5-website-baseline/` —
  example website fixture directories with `_quarto.yml` + multi-page
  layouts. Demonstrates the shape Plan 3's website fixtures take.
- `resources/extensions/quarto/video/video-filter.lua` — the one
  built-in Lua filter today.
- `claude-notes/plans/lua-filter-pipeline/00-index.md` — Carlos's
  2025-12-21 analysis of **TypeScript Quarto**'s `run_as_extended_ast()`
  Lua filter pipeline (~78 stages classified by side-effect category).
  This is porting reference material for the broader epic, **not** the
  inventory Plan 3 tests. Plan 3's universe is enumerated in §"What
  'built-in' covers." Useful when porting an additional TS filter into
  Rust and wondering whether the source-side analysis flagged it as
  pure / file-reading / network / subprocess.

## Work items

### Phase 1 — Hashing infrastructure

- [x] Add `compute_meta_hash_fresh` in
  `crates/quarto-ast-reconcile/src/hash.rs`, parallel to
  `compute_blocks_hash_fresh`. Walks `ConfigValue` tree
  source-info-agnostically. Hashes scalars by `Yaml` payload, recurses
  into `PandocInlines` / `PandocBlocks` via the existing inline / block
  hashers, hashes `Array` in order, hashes `Map` entries
  `(key_string, recurse(value))` **in insertion order** (no sort),
  **includes `merge_op`**, skips `source_info` and `key_source`. (See
  §"In scope" for the full spec.)
- [x] Add `compute_meta_hash_fresh_excluding_rendered` variant that
  skips the `rendered` top-level key (HTML-string side outputs from
  chrome transforms + `IncludeResolveStage` + Bootstrap/clipboard
  injection).
- [x] Add unit tests for both:
  - same content → same hash;
  - different content → different hash;
  - different `source_info` / `key_source` → same hash;
  - same content with `rendered.foo` key only differing → same hash
    for the excluding variant;
  - **same content with Map keys in different insertion order →
    different hash** (regression guard for the no-sort choice);
  - different `merge_op` → different hash (regression guard for the
    `merge_op`-participates choice).
- [x] Add `find_first_divergence(blocks_a, meta_a, blocks_b, meta_b)
  -> DivergencePoint` alongside the hashers. The plan-sketch signature
  took `&DocumentAst`, but `DocumentAst` lives in `quarto-core` and
  `quarto-ast-reconcile` cannot depend on it; the helper takes the
  underlying `&[Block]` + `&ConfigValue` instead, and the test driver
  in `quarto-core/tests/idempotence.rs` will project from its
  `DocumentAst`. Reuses `compute_block_hash_fresh` for the block walk
  and a recursive insertion-order traversal for the meta walk; both
  walks short-circuit on the first divergence.
- [x] Unit tests for `find_first_divergence`:
  - identical docs → `DivergencePoint::None`;
  - one block differs at index N → `Block { index: N, ... }`;
  - one meta key path differs → `MetaKey { path: [...], ... }`;
  - divergence under a `rendered.*` path → not reported (skipped to
    match `compute_meta_hash_fresh_excluding_rendered`).

### Phase 2 — Test crate scaffolding

- [x] Create `crates/quarto-core/tests/idempotence.rs`.
- [x] Implement the `Fixture` struct + `run_fixture(fixture, mode)`
  helper that loops `DriveMode::{SingleFile, ProjectOrchestrator}`
  (see §"What gets tested concretely" for the body).
- [x] Implement `run_single_file(project_dir, active) -> DocumentAst`
  using `ProjectContext::discover` + `build_q2_preview_pipeline_stages`
  + `run_pipeline`. (~50 lines; the only genuinely new driver.)
- [x] Implement `run_orchestrator(project_dir, active) -> DocumentAst`
  by delegating to the existing `render_active_page_preview` helper
  at `crates/quarto-core/tests/render_page_in_project.rs:660` and
  re-parsing `Pass2Payload::as_ast_json()` via
  `pampa::readers::json::read`. (Helper copied inline since each
  `tests/*.rs` is its own binary; the plan flags this as acceptable.)
  No new orchestrator wiring is written; no production plumbing
  change is needed.
- [x] Implement `pandoc_to_document_ast(pandoc) -> DocumentAst` — the
  small field-shuffle between the re-parsed `Pandoc` and the
  hashing helpers' expected shape. Land inline in `idempotence.rs`;
  do not promote to library code until a second caller appears.
- [x] Create `crates/quarto-core/tests/fixtures/idempotence/`
  directory with a README listing the fixture-format rules:
  - no executable engine cells (fenced `` ```python `` blocks only);
  - **no absolute process paths** in fixture content — see §"Decisions"
    / "Fixture-authoring rules for path-recording transforms";
  - per-fixture mode mapping (document fixtures run in both modes;
    website fixtures orchestrator-only).
- [x] Borrow `write` / `canonical` (and `snippet` / `copy_fixture` when
  needed by Phase 4 fixtures) from `render_page_in_project.rs` —
  copied into `idempotence.rs` for now; pulling them into a shared
  `tests/common/` module is out of scope for Plan 3.
- [x] **Phase-2 smoke fixture** (`smoke_plain_paragraph`) drives both
  modes on a single-paragraph document. Passing this confirms the
  harness is wired correctly before Phases 3-4 add the real fixtures.

### Phase 3 — Existing-fixture coverage (carry-forward)

- [x] Add fixtures: `meta-single`, `meta-markdown`, `include-trivial`,
  `callout-warning`, `theorem`, `figure-ref-target`,
  `crossref-to-theorem`, `sectionize-multi`, `footnotes-mixed`,
  `appendix-license`, `combined-stress`.
- [x] Wire one assertion per (fixture, mode) pair — these are all
  document fixtures, so each runs in both `SingleFile` and
  `ProjectOrchestrator` mode. (For now each `#[test]` calls
  `run_in_each_mode` which loops over both modes; if a fixture
  later goes red in only one mode, the panic message names the
  mode, and we can split into two `#[test]` functions for finer
  reporting at that point.)

All 11 carry-forward fixtures pass on first run, in both modes.
No queue entries.

### Phase 4 — New-fixture coverage (gap closure)

- [x] Add document-level fixtures (run in **both** modes), batch 4a
  (no extra scaffolding): `code-block-fenced`, `lua-shortcode-version`,
  `lua-shortcode-lipsum-fixed` (with module-load `randomseed` comment
  in the `.qmd` per §"Noted, not actively tested"), `proof`,
  `equation-labeled`, `toc-on`, `video-filter-header`,
  `theme-bootstrap`, `table-bootstrap-class`. **9/10 pass on first run.**
  `lua_shortcode_lipsum_fixed` fails in `ProjectOrchestrator` mode
  only — not a hash mismatch but a `MalformedSourceInfoPool` when
  re-parsing the orchestrator's AST JSON. JSON writer/reader
  round-trip bug specific to lipsum-shortcode-generated inlines.
  Filed as **bd-3odjm**; root-caused 2026-05-21 to the type-code-3
  mismatch between writer (`FilterProvenance` payload
  `[filter_path, line]`) and reader (still decodes code 3 as legacy
  `Transformed` `[parent_id, ...]`). Fix is owned by
  **[Plan 5](2026-05-04-q2-preview-plan-5-wire-format.md)** —
  §Goal calls this exact bug out and Plan 5's reader change handles
  both shapes. Per the long-lived-branch policy below, this stays
  red on `feature/provenance` until Plan 5 lands; do not patch
  locally. `SingleFile` mode passes — the pipeline itself is
  idempotent.
- [x] Add document-level fixtures, batch 4b (multi-file):
  `include-in-header` (writes a small HTML stub),
  `resource-image` (writes a 67-byte minimal PNG). Both pass on
  first run in both modes.
- [x] Add website-project fixtures (orchestrator-mode only):
  `website-chrome`, `website-links`, `website-listing`. **2/3 pass
  on first run.** `website-chrome` (navbar + sidebar + page-nav +
  footer + favicon + bootstrap-icons + canonical-url) is clean.
  `website-listing` (listing with categories + feed) is clean.
  `website-links` (cross-page `.qmd` body links) is non-idempotent:
  block 0 hash diverges between runs while meta hash is stable.
  Filed as **bd-rz2we** — link-rewrite or link-resolution captures
  something path-dependent into the AST when it should produce a
  path-independent relative URL. Each `run_q2_preview` call gets a
  fresh `TempDir`, so the two runs see different absolute project
  roots; structural AST should not depend on that.
- [ ] Add attribution fixture: `attribution-basic` (both modes; the
  helper installs an `AttributionSourceProvider` on
  `RenderContext.attribution_provider` — needs a small `Fixture`
  extension).

### Phase 5 — Failure triage

`feature/provenance` is a **long-lived integration branch** that
holds failing fixtures *on purpose* until the queue is drained.
The plan does not merge to `main` while any fixture in this gate is
red. See §"Long-lived branch policy" at the top of this plan for
the rationale; what follows is the operational loop.

- [ ] Run the full test suite. For each failing fixture, classify the
  cause (filter non-idempotence, transform non-determinism,
  metadata-merge issue, etc.).
- [ ] For each failure: either fix in-place (if scope is contained and
  obvious) or **file a beads issue using the sub-agent investigation
  prompt template** from §"CI failure policy & sub-agent prompt
  template." Failing tests **stay failing** — no auto-`#[ignore]`.
  Only ignore when the user explicitly says so.
- [ ] Keep the (still-failing) tests on the integration branch so each
  beads issue has a live reproduction. The integration branch may
  stay red for an extended period; the merge to `main` happens only
  after the queue is drained (every red fixture either fixed or
  explicitly `#[ignore]`-d with a permanent rationale signed off
  by the user). The failing tests *are* the triage backlog.

### Phase 6 — Documentation

- [ ] Add `claude-notes/instructions/idempotence-contract.md` covering:
  what the contract requires of new transforms, the meta-hash
  `rendered.*` exclusion, how to add a fixture when introducing a new
  transform, the engine-cells-forbidden rule.
- [ ] Cross-link from the README of the fixtures directory.
- [ ] Cross-link from Plan 7a (so authors looking at runtime user-filter
  idempotence find the CI contract too).

### Phase 7 — Verification

- [ ] `cargo nextest run --workspace` passes.
- [ ] `cargo xtask verify --skip-hub-build` passes.
- [ ] Document the end-to-end test invocation in the commit message
  (per project CLAUDE.md's "End-to-end verification before declaring
  success").

## Dependencies

- Depends on: Plan 1 (`build_q2_preview_pipeline_stages` exists and
  runs).
- Blocks: implicitly Plans 4-8 (round-trip work assumes this contract
  holds — but for pipeline non-determinism only; round-trip itself is
  7a's concern).
- Related to Plan 7a (runtime user-filter idempotence check). Plan 3
  is the **CI-time** half for built-ins (transforms + ship-with-Quarto
  Lua filters); Plan 7a is the **runtime** half for user-supplied
  filters. The two share `compute_blocks_hash_fresh` /
  `compute_meta_hash_fresh` and the same flavor-1-vs-flavor-2
  distinction. See Plan 7a's §"Two flavors of non-idempotence" for the
  shared vocabulary.

### What happens when a fixture fails

Plan 3 reports failures; the *fix* lands wherever the offending
transform / filter lives. Failure modes and where their fixes go:

- **Non-idempotent built-in Lua filter**. Edit the filter's Lua
  source. Lands in `resources/extensions/quarto/<ext>/`. Plan 3
  surfaces the test.
- **Non-deterministic transform attribute / `plain_data` ordering**.
  HashMap iteration or similar. Lands in the transform's `.rs` file
  under `crates/quarto-core/src/transforms/`.
- **Non-deterministic metadata transform**. Lands in
  `metadata_normalize.rs` or wherever the offending merge/normalize
  step lives.
- **Source-info-related instability**. Should NOT happen because the
  hashers exclude source_info / key_source. If somehow it does,
  Plan 4's type changes are the place to investigate.

If a fixture fails on first run, **leave the test failing** and file
a beads issue (with the sub-agent investigation prompt from §"CI
failure policy & sub-agent prompt template"). The failing test stays
red until the issue is resolved — `#[ignore]` only when the user
explicitly says so. Do not silently disable.

## Risk areas

- **A transform or filter might fail the test on first run**. Triaged
  per Phase 5; **leave failing + file a sub-agent investigation prompt**
  (see §"CI failure policy & sub-agent prompt template"). `#[ignore]`
  only when the user explicitly says so.
- **Hash stability across binary versions**: `FxHasher`'s output is
  stable within a Rust process but not across versions. Tests compare
  hashes computed in the same process, not stored as constants. This is
  the natural shape of "run pipeline twice and compare" anyway.
- **Pipeline construction non-determinism**: if extension discovery
  picks up paths in OS-dependent order, attributes could differ on
  different machines. Mitigated by fixture isolation — fixtures don't
  reference real OS paths unless explicitly testing a path-aware
  feature. The attribution fixture is the main case to watch.
- **Website-project fixture complexity**: assembling a valid
  `ProjectContext` is non-trivial. Risk: time spent on test
  scaffolding rather than transform coverage. Mitigation: reuse the
  existing pattern (write `_quarto.yml` + page contents into a
  `TempDir`, call `ProjectContext::discover`) — the same recipe
  used by ~10 sibling tests in `crates/quarto-core/tests/`. No
  parameterized builder is needed. See §"Decisions" /
  "ProjectContext setup for website fixtures."

### Noted, not actively tested

Two latent determinism surfaces surfaced during the source review. The
test suite isn't expected to flake on either; they're recorded here so
the next person who *does* hit a hash divergence in their neighborhood
has a head start:

- **`CodeHighlightStage`'s native disk scan for user grammars**
  (`crates/quarto-core/src/transforms/code_highlight.rs:126-129`).
  On native, when no `user_grammar_provider` is supplied (CLI
  default), the stage falls back to scanning
  `ctx.project.dir.join("_quarto").join("grammars")` for user
  grammars. If that scan returns paths in OS-dependent order,
  attribute output could differ across machines. Fixtures here
  don't supply user grammars, so the directory is absent and the
  early-return at the top of the function makes the scan a no-op
  in practice. Not tested today; flag if a future fixture
  introduces a grammar dependency.
- **Lipsum module-load `randomseed`**
  (`resources/extensions/quarto/lipsum/lipsum.lua:5`). The Lua module
  calls `math.randomseed(os.time())` at load time, which runs once per
  fresh `LuaShortcodeEngine`. On the non-random code path (`{{< lipsum
  3 >}}` — what `lua-shortcode-lipsum-fixed` exercises) `math.random`
  is never reached, so the seed has no observable effect. If a future
  variant routes through `math.random` (random shortcode-resolution
  paths, random shortcode arg parsing) the test would start flaking
  noticeably across runs. The fixture should carry a comment naming
  this.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `compute_meta_hash_fresh` + excluding-rendered variant + tests | ~140 |
| `find_first_divergence` + `DivergencePoint` + tests | ~80 |
| Test crate scaffolding — `Fixture` struct, `run_single_file`, `run_orchestrator` (thin wrapper over existing helper), `pandoc_to_document_ast` shuffle | ~100 |
| Per-fixture `.qmd` files / inline literals (~25 fixtures, 5-30 lines each) | ~280 |
| Per-fixture (fixture, mode) test assertions (mostly one-liners; ~25 fixtures × 1-2 modes ≈ 40 pairs) | ~120 |
| `idempotence-contract.md` + fixtures README | ~80 |
| **Total** | **~800** |

The scaffolding line item dropped from an earlier estimate of ~260
to ~100 after pinning the orchestrator path on the existing
`render_active_page_preview` helper and choosing option (a) for
`DocumentAst` extraction — neither requires a new orchestrator
driver, a `make_website_project_ctx` builder, or production
plumbing changes. `PreviewAstOutput::ast` plumbing is no longer
needed (was ~20 lines in the earlier draft).

**Inventory note**: an earlier draft estimated "~10-20 built-in filters"
in `resources/extensions/`. That was wrong — `resources/extensions/`
contains one Lua filter (`video-filter.lua`) plus five shortcodes
(kbd, video, lipsum, version, placeholder). The bulk of the universe
under test is the **37 Rust transforms** in
`build_q2_preview_transform_pipeline`, plus the stage-level work in
`build_q2_preview_pipeline_stages`.

Realistic shape: 2-3 focused sessions — one for hashing
infrastructure + scaffolding + carry-forward fixtures, one for
gap-closure fixtures (particularly the website-project ones), and
a third for Phase 5 triage if the first run surfaces multiple red
fixtures (which is the expected case, not a surprise).

## Notes

The user said: "Yes, idempotency and stable structural hash have to be
the base contract — so we have to work that out as part of this complex
of plans. Everything existing must be verified to have those
properties." This plan encodes that contract as a CI-enforced test.

The hash function excluding source_info means that future plans (4-8)
that change source_info don't risk breaking idempotence — even if a
transform produces different source_info on different runs (e.g., a
Sectionize that generates synthetic source_info from current
timestamps; not what we do, but illustrative), the hash stays stable.

Round-trip non-idempotence — the property
`pipeline(write(pipeline(x))) ≠ pipeline(x)` — is deliberately not
tested here. The pipeline doesn't re-parse its own output today, so
there's nothing to break. When Plan 7's incremental writer lands,
the property becomes load-bearing for blocks the writer rewrites.
Plan 7a's runtime check is the natural home for round-trip detection
**on user-supplied filters**: per-document, with per-filter attribution
and an `idempotent: false` opt-out, none of which a CI fixture gate
can provide. Round-trip on the built-in side (transforms + one Lua
filter) is consciously left unverified — see Plan 7a's §"Notes" for
the v1 acceptance reasoning.

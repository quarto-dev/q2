# Plan 3 — Built-in transform and filter idempotence verification (CI-time)

**Date:** 2026-05-04 (revised 2026-05-20)
**Branch:** feature/q2-preview
**Status:** Development plan (work items below)
**Milestone:** M2 verification gate (no new milestone — locks in property
on what's already shipped)

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

When this plan lands, we have CI-enforced confidence that the q2-preview
round-trip story (Plans 4-8) rests on a stable foundation.

## Scope

### What "built-in" covers — the universe under test

Two distinct classes, both shipped with Quarto and both in scope:

**Rust transforms** — the source of truth is
`build_q2_preview_transform_pipeline` in
`crates/quarto-core/src/pipeline.rs:1220`, which is
`build_transform_pipeline` minus the four names in
`Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1181`). As of this
revision, the q2-preview pipeline runs **36 transforms** across four
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
  resource-collector, attribution-render.

Excluded by `Q2_PREVIEW_TRANSFORM_EXCLUDED` (out of scope for Plan 3
because they don't run): callout-resolve, attribution-viewer,
title-block, crossref-render.

**Stage-level work** in `build_q2_preview_pipeline_stages`
(`pipeline.rs:379`) also runs around `AstTransformsStage` and can
introduce non-determinism: parse-document, metadata-merge,
include-expansion, include-resolve, listing-item-info, document-profile,
link-resolution, unwrap-profile, pre-engine-sugaring, capture-splice,
engine-execution, compile-theme-css, attribution-generate,
user-filters-pre/post, resource-report, code-highlight. These are
exercised implicitly by every fixture (most are no-ops absent specific
metadata).

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
  source-info-agnostically: hashes scalars by their `Yaml` payload,
  recurses into `PandocInlines` / `PandocBlocks` via the existing inline
  / block hashers, hashes `Map` entries as `(key_string, recurse(value))`
  pairs in key-sorted order, skips `source_info` and `key_source`. Tests
  for the helper land alongside it (mirroring the existing
  `test_same_content_same_hash` style at `hash.rs:767`).

- **Idempotence test runner**: takes a fixture, runs the q2-preview
  pipeline twice via `run_pipeline` (`pipeline.rs:626`), hashes
  `doc.ast.blocks` and `doc.ast.meta` (minus `rendered.*`; keep
  `meta.includes.*` in initially — see "Out of scope" note), asserts
  equality. One test per fixture; failures name the fixture and which
  hash diverged.

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
- **`meta.includes.*` HTML strings**. Try including these in the meta
  hash initially. If a fixture surfaces non-determinism that's
  rendered-shape rather than structural, move the key to the
  exclusion list and document why. Not vital — `IncludeResolveStage`
  copies user-supplied include files verbatim, so non-determinism
  here would be surprising.

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
- **Hash covers blocks and meta-minus-`rendered.*`**. Meta inclusion
  catches non-determinism in metadata-normalize, listing data,
  shortcode-resolved meta values, attribution metadata, etc. The
  `rendered.*` keys are HTML strings populated by chrome-render
  transforms; their canonicalization is a separate concern.
- **Filter mutation provenance stays Original** (post-Plan 4 unified
  `Generated { by: By::filter(...), anchors: [] }` shape). Idempotence
  test sees consistent shape across runs.
- **Each pipeline run uses fresh Lua state**. Confirmed at
  `crates/pampa/src/lua/filter.rs:158`: `apply_lua_filters` constructs
  a fresh `Lua::new()` per invocation. No cross-run state
  accumulation. This matches production (hub-client builds a new
  pipeline per render) and resolves the prior "second-run pipeline
  starts fresh?" open question.
- **Built-in scope = Rust transforms + ship-with-Quarto Lua filters**.
  User filters are out of scope here (Plan 7a covers them).

## What gets tested concretely

```rust
use quarto_ast_reconcile::{compute_blocks_hash_fresh, compute_meta_hash_fresh_excluding_rendered};
use quarto_core::pipeline::{build_q2_preview_pipeline_stages, run_pipeline};
use quarto_core::stage::PipelineData;
use quarto_system_runtime::NativeRuntime;
use std::sync::Arc;

async fn assert_pipeline_deterministic(name: &str, content: &[u8]) {
    let doc_1 = run_q2_preview_pipeline(content, name).await;
    let doc_2 = run_q2_preview_pipeline(content, name).await;

    assert_eq!(
        compute_blocks_hash_fresh(&doc_1.ast.blocks),
        compute_blocks_hash_fresh(&doc_2.ast.blocks),
        "fixture {name}: blocks hash diverged across runs",
    );
    assert_eq!(
        compute_meta_hash_fresh_excluding_rendered(&doc_1.ast.meta),
        compute_meta_hash_fresh_excluding_rendered(&doc_2.ast.meta),
        "fixture {name}: meta hash diverged across runs",
    );
}

async fn run_q2_preview_pipeline(content: &[u8], source_name: &str) -> DocumentAst {
    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(NativeRuntime::new());
    let project = test_project_context();
    let doc = test_document_info(source_name);
    let format = quarto_core::format::Format::q2_preview();
    let binaries = quarto_core::render::BinaryDependencies::new();
    let mut ctx = quarto_core::render::RenderContext::new(&project, &doc, &format, &binaries);

    let stages = build_q2_preview_pipeline_stages(None, None);
    let (output, _diagnostics) =
        run_pipeline(content, source_name, &mut ctx, runtime, stages)
            .await
            .expect("pipeline run");

    match output {
        PipelineData::DocumentAst(ast) => ast,
        other => panic!("expected DocumentAst, got {:?}", other.kind()),
    }
}
```

Notes on the helper:

- `run_pipeline` (`pipeline.rs:626`) is the existing entry point; no new
  driver is needed.
- The q2-preview pipeline ends at `CodeHighlightStage`, so its output is
  `PipelineData::DocumentAst`.
- Each call constructs fresh `StageContext` (inside `run_pipeline`) and
  fresh Lua engines (at filter.rs:158) — natural per-run isolation.

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
- [ ] `video-filter-header` — `# Title {background-video="url"}` →
  exercises `resources/extensions/quarto/video/video-filter.lua` (the
  only built-in Lua filter under `resources/extensions/`).
- [ ] `include-in-header` — `include-in-header: foo.html` in meta with
  trivial `foo.html` → include-resolve stage.
- [ ] `theme-bootstrap` — `theme: cosmo` (or default) in meta →
  compile-theme-css stage.

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

**Attribution fixture** (needs an `AttributionProvider` installed on
`StageContext.attribution_provider`):

- [ ] `attribution-basic` — document with an installed git-based
  attribution provider → attribution-generate stage, attribution-render
  transform.

**Resource fixture:**

- [ ] `resource-image` — `![alt](./local.png)` with the image file
  present → resource-collector.

If a fixture in this list discovers non-idempotence on first run, file
the fix as a follow-up against the appropriate transform's crate (per
§"What happens when a fixture fails" below) and either land the fix
before Plan 3 closes or open a beads issue and gate the fixture
behind `#[ignore]` with a comment naming the open issue. Do not
silently drop the fixture.

## Open questions for implementation

- **Test crate location**: probably `crates/quarto-core/tests/` as a
  workspace-level integration test crate. New test file
  `q2_preview_idempotence.rs`. Confirm during implementation.
- **Fixture format**: files in
  `crates/quarto-core/tests/fixtures/q2-preview-idempotence/`,
  one `.qmd` per fixture; in-source literals for the trivial cases
  (1-2 lines). Probably files for the substantial cases.
- **`ProjectContext` setup for website fixtures**: the chrome
  transforms need a fully-populated project context. Helper:
  `make_website_project_ctx(temp_dir, navbar_config, sidebar_config, …)`.
  This is the heaviest part of the new fixture work; ~150 lines on its
  own.
- **CI failure expectation**: the test fails noisily if any
  transform / filter is non-idempotent. That's the point. If a built-in
  fails on first run, we either fix it before Plan 3 lands or file a
  beads issue and `#[ignore]` the fixture with a clear pointer.

## References

- `crates/quarto-core/src/pipeline.rs:1220`
  `build_q2_preview_transform_pipeline` — q2-preview transform list,
  source of truth.
- `crates/quarto-core/src/pipeline.rs:1181`
  `Q2_PREVIEW_TRANSFORM_EXCLUDED` — the four transforms that don't run.
- `crates/quarto-core/src/pipeline.rs:379`
  `build_q2_preview_pipeline_stages` — stage-level pipeline.
- `crates/quarto-core/src/pipeline.rs:626`
  `run_pipeline` — pipeline execution entry point used by the test
  runner.
- `crates/quarto-core/src/transforms/` — the Rust transform crate root.
  Each transform's `name()` matches the kebab-case strings listed in
  §"What 'built-in' covers."
- `crates/quarto-ast-reconcile/src/hash.rs:115`
  `compute_blocks_hash_fresh` — the existing block hasher.
- `crates/quarto-ast-reconcile/src/hash.rs:767`
  `test_same_content_same_hash` — confirms blocks hash excludes
  source_info.
- `crates/pampa/src/lua/filter.rs:158`
  `apply_lua_filters` — Lua engine creation point; fresh per
  invocation.
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

- [ ] Add `compute_meta_hash_fresh` in
  `crates/quarto-ast-reconcile/src/hash.rs`, parallel to
  `compute_blocks_hash_fresh`. Walks `ConfigValue` tree
  source-info-agnostically.
- [ ] Add `compute_meta_hash_fresh_excluding_rendered` variant that
  skips the `rendered` top-level key (HTML-string side outputs from
  chrome transforms).
- [ ] Add unit tests for both: same content → same hash; different
  content → different hash; different source_info → same hash;
  same content with `rendered.foo` key only differing → same hash for
  the excluding variant.

### Phase 2 — Test crate scaffolding

- [ ] Create `crates/quarto-core/tests/q2_preview_idempotence.rs`.
- [ ] Implement `run_q2_preview_pipeline(content, source_name) -> DocumentAst`
  helper (see §"What gets tested concretely" pseudocode).
- [ ] Implement `assert_pipeline_deterministic(name, content)` helper.
- [ ] Implement `make_test_project_ctx()` and
  `make_website_project_ctx(...)` helpers.
- [ ] Create `crates/quarto-core/tests/fixtures/q2-preview-idempotence/`
  directory with a README listing the fixture-format rules
  (no executable engine cells, etc.).

### Phase 3 — Existing-fixture coverage (carry-forward)

- [ ] Add fixtures: `meta-single`, `meta-markdown`, `include-trivial`,
  `callout-warning`, `theorem`, `figure-ref-target`,
  `crossref-to-theorem`, `sectionize-multi`, `footnotes-mixed`,
  `appendix-license`, `combined-stress`.
- [ ] Wire one assertion per fixture.

### Phase 4 — New-fixture coverage (gap closure)

- [ ] Add document-level fixtures: `code-block-fenced`, `proof`,
  `equation-labeled`, `toc-on`, `video-filter-header`,
  `include-in-header`, `theme-bootstrap`.
- [ ] Add website-project fixtures: `website-chrome`, `website-links`,
  `website-listing`.
- [ ] Add attribution fixture: `attribution-basic`.
- [ ] Add resource fixture: `resource-image`.

### Phase 5 — Failure triage

- [ ] Run the full test suite. For each failing fixture, classify the
  cause (filter non-idempotence, transform non-determinism,
  metadata-merge issue, etc.).
- [ ] For each failure: either fix in-place (if scope is contained) or
  open a beads issue + `#[ignore]` the fixture with a comment naming
  the issue.
- [ ] Document any `#[ignore]`d fixtures in the commit message.

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

If a fixture fails on first run, file the fix as a follow-up against
the appropriate file and either fix-then-land or `#[ignore]`-with-issue
(per Phase 5 above). Do not silently disable.

## Risk areas

- **A transform or filter might fail the test on first run**. Triaged
  per Phase 5; fixed or `#[ignore]`d-with-issue.
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
  `ProjectContext` is non-trivial. Risk: time spent on test scaffolding
  rather than transform coverage. Mitigation: a single
  `make_website_project_ctx` helper covers most chrome transforms in
  one fixture.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `compute_meta_hash_fresh` + excluding-rendered variant + tests | ~120 |
| Test crate scaffolding (helpers, project-ctx builders) | ~200 |
| Per-fixture `.qmd` files (~25 fixtures, 5-30 lines each) | ~280 |
| Per-fixture test assertions (mostly one-liners) | ~80 |
| `idempotence-contract.md` + fixtures README | ~80 |
| **Total** | **~760** |

**Inventory note**: an earlier draft estimated "~10-20 built-in filters"
in `resources/extensions/`. That was wrong — `resources/extensions/`
contains one Lua filter (`video-filter.lua`) plus five shortcodes
(kbd, video, lipsum, version, placeholder). The bulk of the universe
under test is the **36 Rust transforms** in
`build_q2_preview_transform_pipeline`, plus the stage-level work in
`build_q2_preview_pipeline_stages`. The new estimate reflects the
actual split.

Likely two focused sessions — one for hashing infrastructure +
scaffolding + carry-forward fixtures, one for gap-closure fixtures
(particularly the website-project ones).

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

# P1 — Neutral core + PipelineProfile

**Date:** 2026-09-20
**Status:** Landed.
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-18-pandoc-hybrid-P1-implementation.md`](2026-09-18-pandoc-hybrid-P1-implementation.md) — this plan's Coarse checklist converted into dispatchable `## Task N` units, each test bound to a named production seam and revert hunk.

## Goal
Make the format-neutral semantic core explicit and enforced, so the wire-format cut produces a
format-neutral AST. Extend today's family/kind dispatch mechanism to a `Pandoc(fmt)` case.
**No Pandoc work beyond that dispatch.** Reviewed against **HTML / revealjs / q2-preview
byte-identity** — this is a pure refactor with a hard no-regression bar.

## The dispatch mechanism

Two independent axes, each resolved by a small, clean mechanism — not two independently-maintained
pipeline builders, and not asymmetric carve-backs:

- **Family** (Html vs. Revealjs) is resolved by one inline `if is_revealjs {...} else {...}`
  *inside* `build_transform_pipeline` (`pipeline.rs:1264-1277`), gated on
  `is_revealjs_target(target_format)` (`format.rs:138`), which is
  `matches!(target_format, "revealjs" | "q2-slides")` — q2-slides is unconditionally reveal-family,
  confirmed by `render_qmd_to_preview_ast_builds_reveal_slides_for_q2_slides` (`pipeline.rs:2990`).
- **Kind** (Render vs. Preview) is resolved *afterward*, uniformly across both families, by
  `build_q2_preview_transform_pipeline` calling `build_transform_pipeline(...)` and then
  `.retain_excluding(Q2_PREVIEW_TRANSFORM_EXCLUDED)` — a 7-item deny-list (`pipeline.rs:1594-1643`).
  `build_q2_preview_pipeline_stages` does the same one level up
  (`build_html_pipeline_stages_with_options(...).retain(...)` against `Q2_PREVIEW_STAGE_EXCLUDED`).

`Pandoc(fmt)` generalizes this existing, proven two-axis mechanism to a third family-ish case —
it does not introduce new per-family segment abstractions.

## `PipelineProfile` shape

Five variants, family × kind made explicit:

```
PipelineProfile { HtmlRender, HtmlPreview, RevealjsRender, RevealjsPreview, Pandoc(fmt) }
```

`RevealjsRender` = native `revealjs` output; `RevealjsPreview` = `q2-slides`. `Pandoc(fmt)` has
no render/preview split (no q2-pandoc-preview concept is planned).

### Seam definition

1. **Module:** `crates/quarto-core/src/format.rs`, next to `FormatIdentifier` (`Copy, Clone,
   PartialEq, Eq` derives — it's matched, not owned).
2. **`Pandoc(fmt)`'s payload is `String`** (the raw `target_format`, e.g. `"docx"`/`"pptx"`), not
   `FormatIdentifier`. `Docx` and `Pptx` are both variants of `FormatIdentifier` (`format.rs:29`,
   `:31`); the genuinely exhaustive matches on the enum are `as_str()` and the free function
   `output_extension_for()` (`format.rs:279-291`) — adding a variant is a two-arm change.
   `FormatIdentifier::Pptx` and its two match arms are part of this seam's implementation, so
   `Format::from_format_string("pptx")` resolves correctly upstream of the CLI gate; the
   format-specific validation (docx vs. pptx `execute` defaults, `--reference-doc`, etc.) belongs
   to P7's invocation builder, but resolvability of the format string is this seam's job, the same
   as it already is for every other `FormatIdentifier` variant.
3. **Dispatch:** `PipelineProfile::from_format(target_format: &str) -> PipelineProfile` is the
   single derivation point, replacing the old two independent axis-checks (the inline
   `if is_revealjs` family check inside `build_transform_pipeline`, and the
   `ctx.format.pipeline_kind: Option<&'static str>` kind check in
   `stage/stages/ast_transforms.rs:138`). `build_transform_pipeline` takes the resulting
   `PipelineProfile` as an explicit parameter; callers call `PipelineProfile::from_format` once,
   upstream.
4. **`RenderContext` gains a `pipeline_profile: PipelineProfile` field**, so a transform (e.g.
   `ExampleEmbedRenderTransform`) can match on it directly instead of reconstructing
   profile-equivalent logic from `ctx.format.identifier.is_html_based()` (not the same predicate —
   `Preview`-kind matters too). This touches the wasm build (every `RenderContext` field does);
   validate with full `cargo xtask verify`, not `--skip-hub-build`.
5. **`wasm32` behavior:** `PipelineProfile::Pandoc(fmt)` is **not** `cfg`-gated at the type level —
   it stays in every exhaustive match unconditionally. What is gated is the actual pandoc
   invocation: P4's `PandocWriteStage` compiles only under `#[cfg(not(target_arch = "wasm32"))]`;
   if a `Pandoc(fmt)` profile reaches the wasm32 build (it shouldn't — hub-client never requests
   it), the stage is simply absent, a compile-time non-issue.

### `title-block`'s non-HTML branch

`title_block.rs:65-75` (`should_add_h1`) unconditionally adds an h1 for any non-HTML format
("since there's no template-based title block"). Verified against real pandoc (`pandoc -s test.md
-t docx`, `-t pptx`, `title:` only, no body heading): pandoc's own docx and pptx writers already
render a `pStyle="Title"` paragraph / title-slide from `Meta.title` when standalone, so forcing an
h1 into the body would produce a duplicate title. Fix: `title-block` is on the `Pandoc`-kind
exclude-list (below), the same mechanism `Q2_PREVIEW_TRANSFORM_EXCLUDED` uses for Preview. Since
Pandoc target formats are never `is_revealjs_target`, no change to family dispatch itself is
needed.

## `Pandoc`-kind exclude-list

Derived from the design doc's own bucket table (§6): **B1 (shared core) and B3 (shared services)
stay in; B2 (per-family scaffolding for html/revealjs) and B4 (the native-HTML-writer's copy of
the renderer trinity — Q1 Lua is the Pandoc tail's own copy) get excluded wholesale.**

- **B4, four (critical):** `crossref-render`, `mermaid-render`, `code-block-render`,
  `table-bootstrap-class`. **`crossref-render` is load-bearing** — if it ran before the Pandoc cut,
  every numbered CustomNode (`FloatRefTarget`, `Theorem`, `Proof`, `CrossrefResolvedRef`,
  `Equation`) would already be destroyed into raw HTML `Figure`/`Div` structure before P5's shim
  ever saw a CustomNode to route. (`reveal-auto-stretch`, the fifth B4 member, is revealjs-family
  only and never reaches the Pandoc branch.) `example-embed-render` is **not** on this list — see
  below; it stays included, parameterized to skip only the iframe emission.
- **B2 + the entire Navigation phase** (20 members): `title-block`, `sectionize`, `title-banner`,
  `website-title-prefix`, `website-favicon`, `website-bootstrap-icons`, `website-canonical-url`,
  `navbar-generate`/`navbar-render`, `sidebar-generate`/`sidebar-render`,
  `page-nav-generate`/`page-nav-render`, `toc-generate`/`toc-render`/`toc-location`,
  `footer-generate`/`footer-render`, `listing-generate`/`listing-render`, `categories-sidebar`,
  `breadcrumbs-render`, `quarto-nav-js`, `repo-actions-render`, `secondary-nav-render`,
  `listing-feed-stage`, `listing-feed-link` (RevealColumns/RevealSlides/etc. are revealjs-family
  B2, moot for the same reason as `reveal-auto-stretch`).
- `attribution-viewer`, `attribution-generate`/`attribution-render`, `draft-alert`, `format-css`,
  `responsive-image`, `config-markdown`, `reference-link-diagnostics`, `llms-capture`, and
  `callout-resolve` (the resolve half — `callout-resolve` destroys the `Callout` CustomNode into
  Bootstrap-specific HTML DOM; only the sugar half, `callout`, that builds the CustomNode is B1).

**Add a "names exist" validator test** for this list, mirroring
`q2_preview_transform_excluded_names_exist_in_html_pipeline` (`pipeline.rs:2851`), and derive the
Navigation portion by querying `phase()` at build time
(`transforms.iter().filter(|t| t.phase() == TransformPhase::Navigation)`) rather than
hand-enumerating, plus a total-bucket-coverage invariant (every transform in
`build_transform_pipeline` classified exactly once).

**Diverges from Preview's list — not a template to copy directly:**
1. Preview excludes both `panel-tabset` and `panel-tabset-resolve` (it has no Tabset preview UI
   story at all). **The Pandoc list excludes only `panel-tabset-resolve` and keeps the sugar
   transform `panel-tabset` enabled** — the Pandoc tail needs the real `Tabset` CustomNode to
   survive to the cut for P5's Route-R shim; excluding the sugar too would leave nothing to route.
2. Preview *includes* several B2/Navigation transforms Pandoc must exclude (`sectionize`,
   `website-*`, `navbar-render`, etc.) because Preview injects their raw HTML output as an opaque
   blob via `dangerouslySetInnerHTML` — a strategy with no Pandoc equivalent (a docx has no DOM to
   inject pre-rendered navbar HTML into).

## Self-gating transforms — a second, independent gating axis

Some B1 transforms self-gate inside their own `transform()` body, independent of the
pipeline-level exclude-list. `panel_tabset.rs:109-114`:

```rust
if !ctx.format.identifier.is_html_based()
    || is_revealjs_target(&ctx.format.target_format)
    || is_minimal_html(&ast.meta)
{ return Ok(()); }
```

`is_html_based()` is `matches!(self, Html | Revealjs)` — so for `--to docx`, `panel-tabset`
returns immediately and never builds a `Tabset` CustomNode, even though it's kept on the include
side of the exclude-list specifically so P5's Route-R shim has a node to route. The self-gate must
be widened: `!ctx.format.identifier.is_html_based() &&
!matches!(ctx.pipeline_profile, PipelineProfile::Pandoc(_))`.

Audited against the real exclude-list membership:

| file | gate shape | on the Pandoc exclude-list? |
|---|---|---|
| `crossref_render.rs:88` | not a gate — `html_float_dom` is a config field, no early return | yes (`crossref-render`) |
| `mermaid.rs:175` | `!is_html_based()` → return | yes (`mermaid-render`) |
| `title_block.rs:97` | `should_add_h1(.., ctx.format.is_html())` → return | yes (`title-block`) |
| `toc_generate.rs:85` | `identifier == FormatIdentifier::Html` | yes (`toc-generate`) |
| `panel_tabset.rs:109-114` | `!is_html_based() \|\| is_revealjs_target \|\| is_minimal_html` | no — deliberately included |
| `draft_alert.rs:127,142` | `is_html() && !is_revealjs_target` | on the exclude-list |
| `format_css.rs:93` | `!ctx.format.is_html()` → return | on the exclude-list |
| `responsive_image.rs:152` | `!format.is_html()` → return | on the exclude-list |

**Only `panel_tabset.rs`'s self-gate is widened** — its inclusion is deliberate and load-bearing
(P5's shim needs the `Tabset` CustomNode it builds). `draft-alert`, `format-css`, and
`responsive-image` are on the exclude-list instead of being widened: widening `format-css`, for
example, would stage stray `.css` files next to a `.docx`. Add a regression test that positively
asserts the included set actually executes under a Pandoc profile (e.g. a fixture with a
`.panel-tabset` div yields a `Tabset` CustomNode at the cut) — an absence-only invariant test would
pass even if the included set silently stopped firing.

## AppendixStructure — classified B3

`AppendixStructureTransform` (`transforms/appendix.rs`) relocates user `.appendix`-classed Divs,
the bibliography (`id=refs`), and footnotes to the document's end, wraps them in a container, and
appends metadata-driven License/Copyright/Citation sections — entirely as plain Pandoc primitives
(`Div`/`Para`/`Header`/`Link`/`Str`), no `RawInline`/`RawBlock` anywhere. Explicitly skipped for
book format (book-mode appendices are a different, unimplemented, out-of-scope concept). The only
format-flavored artifacts are a `role: doc-bibliography` ARIA attribute and an `appendix-style`
class, both harmless if a non-HTML writer ignores them. It's on the "now INCLUDED" list for
`Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1608`) already, i.e. it already runs, unexcluded,
for q2-preview's wire-format leg — the same argument extends to the Pandoc leg. `LinkRewriteTransform`
runs immediately before `AppendixStructureTransform` in Finalization (`pipeline.rs:1465-1466`), a
pure ordering relationship, not a data coupling — rewriting link URLs before relocating the Div is
order-safe regardless of classification. B3, not excluded: excluding it would silently drop real
user content (`.appendix` divs, license/citation notices) from docx/pptx v1 with no replacement.

## ExampleEmbedRender — format-parameterized B1

`transforms/example_embed.rs::render_embed` already separates two kinds of output:

1. `iframe_block` (`example_embed.rs:380-409`) builds `Block::RawBlock(RawBlock { format: "html",
   ... })` — the same Pandoc primitive a hand-authored raw-HTML `<iframe>` would use. Pandoc's own
   non-HTML writers already drop a `RawBlock` whose format doesn't match the output format, by
   convention.
2. The `snippet` slot (hand-authored illustrative code) and the `body`/caption slot (source-link
   fallback, prepended with a "Demo N:" label via `with_number_label` when `plain_data.order` is
   set) are plain, portable Pandoc blocks with nothing HTML-specific about them.

`example-embed-render` is format-parameterized B1, the same pattern as `ShortcodeResolve`: emit
the iframe only when the active `PipelineProfile` supports raw HTML/iframes (`HtmlRender`/
`HtmlPreview`/`RevealjsRender`/`RevealjsPreview`); for `Pandoc(fmt)`, emit `snippet` + the numbered
caption/link, skipping only the `iframe_block` call. `ExampleEmbed` never reaches the Lua shim as a
raw `CustomNode` for Pandoc targets — it's resolved to plain Pandoc blocks upstream of the cut.

## In scope
- Introduce `PipelineProfile { HtmlRender, HtmlPreview, RevealjsRender, RevealjsPreview,
  Pandoc(fmt) }`; the shared core (B1 transforms + Crossref) is emitted verbatim, tails dispatch
  per profile.
- The `Pandoc`-kind exclude-list, per the list above, plus its "names exist" validator test.
- `FormatIdentifier::Pptx` (a two-arm addition: `as_str`, `output_extension_for`), making `pptx`
  resolvable at `Format::from_format_string`, upstream of the CLI gate relaxation.
- Format-parameterize `ExampleEmbedRenderTransform` per the section above.
- Split `FootnotesTransform`: B1 half (`NoteRef`+`Def` → native Pandoc `Note`) stays in the core;
  HTML `<section>`+backlinks half is excluded for `Pandoc`.
- AppendixStructure classified B3: keep in the shared core, running for `Pandoc(fmt)` too.
- Replace `q2-preview`'s deny-list-and-note with a single named `PipelineProfile` dispatch that
  also serves `Pandoc(fmt)` — one mechanism.
- Verify no macro `PipelineStage` (not just `AstTransform`) carries a hidden HTML assumption the
  neutral-core invariant would otherwise miss.
- `ConditionalContentTransform` is B1 in the design doc's bucket table (§6) — it already exists
  (`transforms/conditional_content.rs`), runs unconditionally early in Normalization
  (`pipeline.rs:1172`), and is already format-parameterized via `lua_format_for()`. See P8.
- The `Pandoc`-kind *stage*-level exclude list (`PipelineStage`s are a separate list from
  `AstTransform`s — `build_html_pipeline_stages_with_options`, `pipeline.rs:270`). Drop
  `MathJsStage`, `RenderHtmlBodyStage`, `ApplyTemplateStage` (would error or produce HTML output)
  and `CompileThemeCssStage`/`BootstrapJsStage`/`ClipboardJsStage`/`TabsetsJsStage` (would compile
  Bootstrap SCSS and stage `site_libs/*` into a docx output directory for no reason).
  `CodeHighlightStage` is harmless to leave in (pandoc's docx writer drops its `data-hl-spans`
  attributes) but is wasted work. Write the list as `name()` strings — `&["math-js",
  "render-html-body", "apply-template", "compile-theme-css", "bootstrap-js", "clipboard-js",
  "tabsets-js", "code-highlight"]`, matching `Q2_PREVIEW_STAGE_EXCLUDED`'s convention
  (`pipeline.rs:395`), with its own "names exist" validator (`pipeline.rs:4007`).
  `attribution-generate` is the `name()` of both a transform (excluded above) and a distinct stage
  (`stage/stages/attribution_generate.rs:67`, `stages[16]`, `pipeline.rs:2198`) — the stage
  self-gates on `is_feature_disabled(meta, "attribution")`, off by default. This list is owned
  jointly with P4, which introduces `PandocWriteStage`.
- A byte-identity corpus + capture/diff harness for the "HTML/revealjs/q2-preview byte-identity"
  review bar (`quarto-core`'s fragment-level snapshots don't cover a whole rendered document). Name
  a corpus (e.g. `docs/` + the crossref fixture set), capture output before/after, diff.

## Out of scope (deferred / other plans)
- Any Pandoc emission beyond the dispatch/exclude-list (P4/P7). The wire-format schema itself
  (P2 — P1 just emits at the cut).
- content-hidden verification against genuine Pandoc targets (P8 — the transform itself needs no
  relocation here, only downstream verification).

## Consumes / Produces (seams)
- **Produces:** the neutral-core cut AST (the wire-format content) + the `PipelineProfile` seam
  that P7 plugs the Pandoc tail into.
- **Produces for P7-foundation:** the B3 shared-services segment (ResourceCollector, LinkRewrite,
  Appendix — all confirmed in, not conditional) positioned to run before the Pandoc handoff.
- **Produces for P7:** the `Pandoc`-kind exclude-list P7's invocation builder can extend per
  format if needed.

## Checklist
- [x] `PipelineProfile` (Task 1): five-variant, two-axis shape, module + dispatch + `RenderContext`
  field per the seam definition above, landed in `crates/quarto-core/src/format.rs`.
- [x] `Pandoc`-kind exclude-list (Task 2): `PANDOC_TRANSFORM_EXCLUDED` (`pipeline.rs:1746`) plus
  T2.1 ("names exist") and T2.2 (Navigation-completeness) validator tests.
- [x] Widen `panel_tabset.rs`'s self-gate only (Task 3): T3.1/T3.2 pin the widened gate and the
  other terms left intact; `draft_alert.rs`/`format_css.rs`/`responsive_image.rs` were left
  un-widened and are on the exclude-list instead.
- [x] `Pandoc`-kind stage-level exclude list (Task 6): `PANDOC_STAGE_EXCLUDED` (`pipeline.rs:455`)
  plus T6.1 ("names exist") and T6.2 (exact surviving stage-name list); T6.3 pins
  `attribution-generate` as stage-only, not a transform.
- [x] Byte-identity corpus + capture/diff harness (Task 8):
  `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/` + `expected_hashes.txt`
  before/after capture-diff harness.
- [x] Footnotes split (Task 5): `footnotes` (B1, native `Inline::Note`) and `footnotes-resolve`
  (B4, HTML chrome, excluded for Pandoc) registered adjacently; T5.1–T5.6 cover the split, the
  two-form (`^[inline]` + `[^ref]`/`[^ref]:`) fixture, and registration adjacency.
- [x] Format-parameterize `ExampleEmbedRenderTransform` (Task 5): `example_embed.rs` gates the
  iframe on `PipelineProfile::{HtmlRender,HtmlPreview,RevealjsRender,RevealjsPreview}`; a
  `Pandoc(fmt)` profile gets the snippet + numbered caption only.
- [x] `PipelineProfile` dispatch serves Preview and Pandoc from the same mechanism (Tasks 1/2/6):
  both `build_q2_preview_transform_pipeline` and the `Pandoc(fmt)` branch of
  `build_transform_pipeline` apply their exclude-list via the same `retain_excluding` mechanism,
  dispatched off one `PipelineProfile`.
- [x] No macro `PipelineStage` is implicitly HTML-shaped (Task 6, T6.2): the exact surviving
  stage-name list is pinned against the real pipeline.
- [x] Neutral-core invariant test, extended to assert total bucket coverage (Task 7):
  `neutral_core_invariant_no_b2_b4_survives_the_pandoc_cut` and
  `bucket_classification_is_total_over_the_html_pipeline` (T7.2) against `const BUCKETS`; coverage
  is asserted against the Rust const, the design-doc §6 table is advisory.
- [x] `PipelineProfile::Pandoc(fmt)` / `PandocWriteStage` wasm32 behavior: not `cfg`-gated at the
  type level; `PandocWriteStage` compiles only under `#[cfg(not(target_arch = "wasm32"))]`.
- [x] `ConditionalContentTransform` in design doc §6 bucket table as B1.
- [x] Design doc §5 `PipelineProfile` shorthand matches the landed five-variant shape (Task 9).
- [ ] **Open:** fix `title_block.rs`'s non-HTML branch, or confirm the exclude-list makes it moot
  (it does — `title-block` is on `PANDOC_TRANSFORM_EXCLUDED`, `pipeline.rs:1755`, pinned by
  T2.1/T2.3); add a regression test proving no duplicate title in a docx/pptx smoke fixture. Owned
  by **P7**, which introduces the per-format invocation builder this test needs — there is no
  Pandoc output path to test against from P1 alone.

# P1 — Neutral core + PipelineProfile

**Date:** 2026-08-20  **Updated:** 2026-09-18 (two passes) — see `git log --oneline -- claude-notes/plans/2026-08-20-pandoc-hybrid-P1-neutral-core.md`
for the full correction history (moved out of this header once it grew past readability, same
reasoning as the design doc's "Amendment log"). Latest (round 4 review, Reviewer A): the prior
pass's self-gating-widening criterion was itself wrong — following it would have widened
`format-css` and staged stray CSS next to a docx; corrected to widen only `panel-tabset` and
exclude the other three. Also: `FormatIdentifier::Pptx` added to this plan's seam work (the prior
"ripple cost" justification for declining it was factually wrong on three counts and left `pptx`
unresolvable with no owner); the Navigation exclude-list count corrected to 20 (was still missing
3 after two prior correction passes); closed the wasm32 checklist item against its own already-
decided Seam item 5; added exclude-list "names exist" validators; noted the stage-list is jointly
owned with P4. (Prior pass, same day: an implementation-feasibility review found `PipelineProfile`
was named everywhere and specified nowhere — added a concrete "Seam definition" closing all five
open sub-decisions — and found the self-gating-transforms gating axis in the first place.)
**Status:** Shape draft
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-18-pandoc-hybrid-P1-implementation.md`](2026-09-18-pandoc-hybrid-P1-implementation.md) — this plan's Coarse checklist converted into dispatchable `## Task N` units, each test bound to a named production seam and revert hunk.

## Goal
Make the format-neutral semantic core explicit and enforced, so the wire-format cut produces a
format-neutral AST. Extend today's family/kind dispatch mechanism to a `Pandoc(fmt)` case.
**No Pandoc work beyond that dispatch.** Reviewed against **HTML / revealjs / q2-preview
byte-identity** — this is a pure refactor with a hard no-regression bar.

## What the current mechanism actually is (verified against code)

Two independent axes, each already resolved by a small, clean mechanism — **not** two
independently-maintained pipeline builders, and not "asymmetric carve-backs":

- **Family** (Html vs. Revealjs) is resolved by one inline `if is_revealjs {...} else {...}`
  *inside* `build_transform_pipeline` (`pipeline.rs:1264-1277`), gated on
  `is_revealjs_target(target_format)` (`format.rs:138`), which is
  `matches!(target_format, "revealjs" | "q2-slides")` — **q2-slides is already unconditionally
  reveal-family**, confirmed by a passing test
  (`render_qmd_to_preview_ast_builds_reveal_slides_for_q2_slides`, `pipeline.rs:2990`).
- **Kind** (Render vs. Preview) is resolved *afterward*, uniformly across both families, by
  `build_q2_preview_transform_pipeline` calling `build_transform_pipeline(...)` and then
  `.retain_excluding(Q2_PREVIEW_TRANSFORM_EXCLUDED)` — one small 7-item deny-list
  (`pipeline.rs:1594-1643`). `build_q2_preview_pipeline_stages` does the same thing one level up
  (`build_html_pipeline_stages_with_options(...).retain(...)` against `Q2_PREVIEW_STAGE_EXCLUDED`).

So the real work here is **generalizing an existing, proven two-axis mechanism to a third
family-ish case (`Pandoc(fmt)`)** — not building new per-family segment abstractions from
scratch, and not reconciling divergent builders.

## `PipelineProfile` shape (corrected)

The design doc's `PipelineProfile { HtmlFull, Revealjs, Preview, Pandoc(fmt) }` (design note §5)
flattens two orthogonal axes into four ambiguous names — there's no clear slot for "Revealjs
family, Preview kind" (= q2-slides), even though the runtime already handles that cell
correctly today. Corrected shape, five variants, family × kind made explicit (kind renamed
`Render`/`Preview`, not `Full`/`Preview`, per review):

```
PipelineProfile { HtmlRender, HtmlPreview, RevealjsRender, RevealjsPreview, Pandoc(fmt) }
```

`RevealjsRender` = native `revealjs` output; `RevealjsPreview` = `q2-slides`. `Pandoc(fmt)` has
no render/preview split (no q2-pandoc-preview concept is planned). This needs the same fix in
the design doc's §5 shorthand — flagged there separately since it's inside a frozen-decision
section; not changed here.

### Seam definition (added 2026-09-18 — an implementation-feasibility review found the shape
above was named everywhere and specified nowhere: no module, no dispatch mechanism, and
`Pandoc(fmt)`'s payload type was undecided while `FormatIdentifier` has no `Pptx` variant at all.
This is pure internal wiring, not new functionality — resolving it doesn't add a feature, it
just says where an already-decided type lives.)

1. **Module:** `crates/quarto-core/src/format.rs`, next to `FormatIdentifier` (`Copy, Clone,
   PartialEq, Eq` derives — it's matched, not owned).
2. **`Pandoc(fmt)`'s payload is `String`** (the raw `target_format`, e.g. `"docx"`/`"pptx"`), not
   `FormatIdentifier`. **Correction (2026-09-18, round 4 review — three independent reviewers plus
   an unsolicited obligations-survey agent all found the same thing): the ripple-cost justification
   below was factually wrong, and its wrongness left `pptx` unresolvable with no owner.** The
   original text read: "Extending `FormatIdentifier` with `Docx`/`Pptx` variants would ripple
   through every exhaustive match on it (`is_native`, `is_html_based`, `is_multi_file`,
   `output_extension`) for no benefit P1 needs." Checked against `crates/quarto-core/src/format.rs`:
   `Docx` **already exists** (`format.rs:29`) — only `Pptx` is missing; `is_native`/`is_html_based`/
   `is_multi_file` are `matches!(...)` expressions with **no exhaustive arms**, so adding a variant
   doesn't touch them; and there is **no `output_extension` method** on `FormatIdentifier` at all —
   `output_extension` is a `String` *field* on `Format`, populated by the free function
   `output_extension_for` (`format.rs:279-291`). The genuinely exhaustive matches are exactly two:
   `as_str()` and `output_extension_for()`. Workspace-wide, `FormatIdentifier` has 171 references,
   of which only ~10 are match-arm sites, all in `quarto-core`/`quarto` — zero in any wasm-only
   crate. **Adding `FormatIdentifier::Pptx` is a two-arm change, not a ripple.**
   **Consequence that went unnoticed because of the wrong cost model:** with no `Pptx` variant,
   `Format::from_format_string("pptx")` fails at `crates/quarto-core/src/format.rs:447`
   (`Err("Unknown format: pptx")`) — **before** `crates/quarto/src/commands/render.rs:676`'s
   `resolve_format(&format_str)?` even returns, i.e. one line *above* the `is_native()` gate at
   `render.rs:680-684` that P7's checklist plans to relax. `--to docx` works once that gate is
   relaxed (step 1 of `from_format_string` succeeds for `Docx`); `--to pptx` does not, because step
   1 fails first. Neither P1 nor P7 previously owned fixing this — P7's checklist said only "relax
   `render.rs:680-684`," which is sufficient for docx and insufficient for pptx. **Decided: P1 adds
   `FormatIdentifier::Pptx` (+ its two match arms) as part of this seam's implementation, closing
   the gap directly rather than deferring format-specific validation to P7** as originally described
   — format-specific validation (docx vs. pptx `execute` defaults, `--reference-doc`, etc.) still
   belongs to P7's invocation builder, but *resolvability* of the format string is P1's seam's job,
   the same as it already is for every other `FormatIdentifier` variant.
3. **Dispatch:** add `PipelineProfile::from_format(target_format: &str) -> PipelineProfile` as
   the single derivation point (replacing today's two independent axes — the inline `if
   is_revealjs` family check inside `build_transform_pipeline`, and the `ctx.format.pipeline_kind:
   Option<&'static str>` kind check in `stage/stages/ast_transforms.rs:138`). `build_transform_
   pipeline` takes the resulting `PipelineProfile` as an explicit parameter (not an 8th ad-hoc
   flag) and uses it internally wherever the two old checks were; callers that used to pass
   `target_format: String` + rely on the internal `is_revealjs` derivation now call
   `PipelineProfile::from_format` once, upstream.
4. **`RenderContext` gains a `pipeline_profile: PipelineProfile` field**, so a transform (e.g.
   `ExampleEmbedRenderTransform`) can match on it directly instead of reconstructing
   profile-equivalent logic from `ctx.format.identifier.is_html_based()` (which is *not* the same
   predicate — `Preview`-kind matters too, and `is_html_based()` can't see it). This is additive
   plumbing on an existing struct, not new functionality; it does touch the wasm build (every
   `RenderContext` field does), so validate with full `cargo xtask verify`, not
   `--skip-hub-build`.
5. **`wasm32` behavior:** `PipelineProfile::Pandoc(fmt)` is **not** `cfg`-gated at the type
   level — keep it in every exhaustive match unconditionally, so wasm32 builds don't need a
   parallel non-exhaustive-match story. What *is* gated is the actual pandoc invocation: P4's
   `PandocWriteStage` compiles only under `#[cfg(not(target_arch = "wasm32"))]`; if a
   `Pandoc(fmt)` profile somehow reaches the wasm32 build (it shouldn't — hub-client never
   requests it), the stage is simply absent from that build's pipeline, which is a compile-time
   non-issue, not a runtime catalog error to design.

## Resolved in this rework (2026-09-16)

Both previously-deferred questions are now answered, not just "resolvable":

1. **Family membership of revealjs vs. q2-slides** — resolved, see above.
   `is_revealjs_target` already treats them identically; nothing to change here beyond naming
   the profile variants so the mapping is explicit instead of implicit.
2. **TitleBlock's minimal-mode h1 "should stay HTML-only"** — **the plan's assumption was
   backwards.** `title_block.rs:65-75` (`should_add_h1`) does the *opposite*: for any non-HTML
   format it unconditionally adds the h1 ("For non-HTML formats (PDF, DOCX, etc.), always add
   the h1 since there's no template-based title block") — dead code today (Q2 blocks non-HTML
   render), git-blamed to 2026-01-26, written speculatively long before this epic.
   Empirically verified against real pandoc (`pandoc -s test.md -t docx`, `-t pptx`, with only
   `title:` in Meta and no body heading): **pandoc's own docx and pptx writers already render a
   `pStyle="Title"` paragraph / title-slide from `Meta.title`** when standalone. Forcing an h1
   into the body as the current code does would produce a **duplicate title** — exactly the
   failure the `is_html` branch already avoids for HTML-full, just not applied symmetrically to
   the branch that will matter for docx/pptx.
   **Fix, and it's nearly free once the mechanism above is generalized:** add `title-block` to a
   new `Pandoc`-kind exclude-list, the same mechanism `Q2_PREVIEW_TRANSFORM_EXCLUDED` already
   uses for Preview. Since Pandoc target formats are never `is_revealjs_target`, they always
   take the existing HTML-family `else` branch of the family if/else already — **no change to
   the family dispatch itself is needed**, only a new exclude-list applied on top, mirroring
   Preview's.

## Pandoc-kind exclude-list, derived mechanically from the design doc's own §6 bucket table

The design doc's own bucket table (§6) already settles this — the gap was that this plan hadn't
connected it to a concrete `&[&str]` list yet. The principle: **B1 (shared core) and B3 (shared
services) stay in; B2 (per-family scaffolding for html/revealjs, families Pandoc isn't) and B4
(the native-HTML-writer's copy of the renderer trinity — Q1 Lua is the Pandoc tail's *own* copy)
get excluded wholesale.** This isn't a per-item risk call the way Preview's smaller list was
built (see "diverges from Preview" below) — it follows directly from the already-frozen bucket
assignments. Verified every name below against the transform's real `fn name()` in
`crates/quarto-core/src/transforms/`, not guessed:

- **B4, four (critical):** `crossref-render`, `mermaid-render`, `code-block-render`,
  `table-bootstrap-class`. **`crossref-render` is the load-bearing one** —
  if it ran before the Pandoc cut, every numbered CustomNode (`FloatRefTarget`, `Theorem`,
  `Proof`, `CrossrefResolvedRef`, `Equation`) would already be destroyed into raw HTML
  `Figure`/`Div` structure before P5's shim ever saw a CustomNode to route — this would silently
  break the entire epic's premise, not just degrade one feature. (`reveal-auto-stretch`, the
  fifth B4 member, is revealjs-family-only and never reaches the Pandoc branch regardless.)
  **`example-embed-render` moved off this list** (reclassified B4→format-parameterized B1,
  decided with Gordon — see the design doc §6 update): unlike `crossref-render`, it doesn't need
  to leave a raw `CustomNode` for the Lua shim to route, since its own logic already separates a
  portable snippet/caption from an HTML-only `RawBlock("html")` iframe. It stays **included** for
  `Pandoc(fmt)`, parameterized to skip only the iframe emission.
- **B2 + the entire Navigation phase** (design doc §6 already states "Pandoc skips wholesale"
  for Navigation — this just enumerates it): `title-block` (already identified, duplicate-title
  bug), `sectionize`, `title-banner`, `website-title-prefix`, `website-favicon`,
  `website-bootstrap-icons`, `website-canonical-url`, and Navigation's own transforms —
  `navbar-generate`/`navbar-render`, `sidebar-generate`/`sidebar-render`,
  `page-nav-generate`/`page-nav-render`, `toc-generate`/`toc-render`/`toc-location`,
  `footer-generate`/`footer-render`, `listing-generate`/`listing-render`, `categories-sidebar`
  (RevealColumns/RevealSlides/etc. are revealjs-family B2, moot for the same reason as
  `reveal-auto-stretch`).
- **`attribution-viewer`** — not in the §6 table at all (an omission, not a wrong classification),
  but its own behavior (raw HTML `<style>/<script>` injected into `meta.rendered.includes.*`)
  is exactly what B4/B2 exclusion is for; exclude it on that basis.
- **Design doc §6 bug found: `callout-resolve` is misclassified as B1.** The table's B1 row lists
  "Callout, CalloutResolve" together, but `callout-resolve` is empirically B4-shaped — it's
  *already* excluded from `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1595`) precisely because
  it destroys the `Callout` CustomNode into Bootstrap-specific HTML DOM (confirmed by
  `Callout.tsx`'s own comment: "`callout-resolve` is excluded from q2-preview's pipeline... so
  the Rust side hands us the `Callout` CustomNode wrapper unchanged"). Only the *sugar* half
  (`callout` — the transform that builds the CustomNode in the first place) is genuinely B1.
  `callout-resolve` needs excluding from `Pandoc` for the same reason it's excluded from Preview:
  P5's shim needs the raw `Callout` CustomNode, not pre-rendered Bootstrap HTML (**corrected
  2026-09-17** — this said "Route L" when written; Callout was later reclassified Route R by P6
  Finding 4, but the exclusion reasoning is unchanged either way, since both routes need the raw
  CustomNode surviving to the cut, not `callout-resolve`'s pre-rendered output).
- **Two more real gaps found by an epic-wide review pass (2026-09-17), same "unclassified, not
  wrongly classified" shape as `attribution-viewer` above:**
  - **Three more Navigation-phase transforms were missing from the enumeration above**, despite
    declaring `TransformPhase::Navigation` like their siblings: `breadcrumbs-render`
    (`breadcrumbs_render.rs`), `quarto-nav-js` (`quarto_nav_js.rs`), `repo-actions-render`
    (`repo_actions_render.rs`). All three are HTML/website chrome and exclude on the same basis
    as the rest of Navigation. This undercuts the "mechanically derived, not risk-assessed"
    claim above exactly as much as `attribution-viewer`'s omission did — three of eighteen
    Navigation-phase transforms were missed by manual enumeration, not by a query against
    `phase()`.
  - **Correction (2026-09-18, round 4 review, Reviewer A) — the miss recurred a third time.** The
    real Navigation-phase count is **20**, not 18: still missing `secondary-nav-render`
    (`transforms/secondary_nav_render.rs`, no self-guard), `listing-feed-stage`
    (`project/listing/feed/stage.rs`, self-guards on `ctx.resolved_listings.is_empty()`, moot once
    `listing-generate` is excluded), `listing-feed-link` (`project/listing/feed/link_inject.rs`,
    same self-guard). Add all three to the exclude list. Separately, enumerating every
    `*Transform::new()` in `build_transform_pipeline` turns up **six transforms with no bucket in
    design §6 and no membership in this exclude-list at all**: `config-markdown`,
    `reference-link-diagnostics` (both format-agnostic and harmless either way),
    `draft-alert`/`format-css`/`responsive-image` (see the self-gating Finding's correction above —
    add all three to this exclude-list), and `llms-capture` (`project/llms_post_render.rs`,
    `website.llms-txt`-gated, moot for Pandoc). This is the third consecutive round in which
    hand-enumeration missed real Navigation/B1 members — promoting the "derive by querying
    `phase()`" recommendation below from a nice-to-have to a requirement, and add a **parallel
    "names exist" validator test** for this exclude-list (mirroring
    `q2_preview_transform_excluded_names_exist_in_html_pipeline`, `pipeline.rs:2851`) — there is
    currently no such guard for the ~33-name Pandoc list, unlike Preview's 7-item list.
  - **`attribution-generate`/`attribution-render`** — like `attribution-viewer`, absent from the
    §6 table entirely. Investigated directly (not just pattern-matched against
    `attribution-viewer`): both populate `ctx.format_options`/`ctx.attribution_data` fields
    consumed only by the HTML writer (`write_block_source_attrs`) and the JSON/preview writer
    (`astContext.attribution`) — no Pandoc consumer exists or is planned, and neither mutates the
    AST itself (they only annotate sideband pointer-keyed maps), so including them is wasted
    compute for a Pandoc render, not a correctness bug — but exclude them for cleanliness on the
    same B4 basis as `attribution-viewer`.
  - **Fix (recommended, not yet implemented):** don't hand-enumerate Navigation-phase transforms
    at all — derive the exclude-list's Navigation portion by querying `phase()` at build time
    (`transforms.iter().filter(|t| t.phase() == TransformPhase::Navigation)`), and make the
    neutral-core invariant test (checklist below) assert *total* bucket coverage (every
    transform in `build_transform_pipeline` classified exactly once), so a future addition can't
    silently go unclassified the way these four did.

**Diverges from Preview's list — do not blanket-copy it.** Preview's 7-item
`Q2_PREVIEW_TRANSFORM_EXCLUDED` overlaps heavily with the list above (both need the same B4
items excluded, both need `title-block`/`callout-resolve` excluded) but is **not** a template to
copy directly, for two reasons:
1. Preview excludes `panel-tabset` **and** `panel-tabset-resolve` — both halves of the tabset
   pair — because it has no Tabset preview UI story at all yet (tracked as its own follow-up
   strand, per the exclusion comment). **The Pandoc list must exclude only `panel-tabset-resolve`
   and keep the sugar transform `panel-tabset` enabled** — P5 (2026-09-16 rework) found the
   Pandoc tail needs the real `Tabset` CustomNode to survive to the cut for its Route-R shim;
   excluding the sugar too would leave nothing to route.
2. Preview *includes* several B2/Navigation transforms Pandoc must exclude (`sectionize`,
   `website-*`, `navbar-render`, etc.) because Preview's strategy for them is different: it lets
   them run and injects their raw HTML output as an opaque blob via `dangerouslySetInnerHTML`
   (`meta.rendered.navigation.*`/`includes.header`) — a strategy with no Pandoc equivalent (a
   docx has no DOM to inject pre-rendered navbar HTML into).

## Finding — self-gating transforms are a second, independent gating axis the exclude-list model misses (added 2026-09-18)

An implementation-feasibility review found that the exclude-list is only half of how transforms
are actually gated in this codebase. Some B1 transforms **self-gate inside their own
`transform()` body**, independent of whether the pipeline-level name deny-list excludes them —
and for at least one, the self-gate silently defeats a decision this very plan already made.

`panel_tabset.rs:105-114`:
```rust
if !ctx.format.identifier.is_html_based()
    || is_revealjs_target(&ctx.format.target_format)
    || is_minimal_html(&ast.meta)
{ return Ok(()); }
```
`is_html_based()` is `matches!(self, Html | Revealjs)` — so for `--to docx`, `panel-tabset`
**returns immediately and never builds a `Tabset` CustomNode**, no matter that this plan's
exclude-list deliberately keeps `panel-tabset` enabled (see above) specifically so P5's Route-R
shim has a node to route. **The self-gate must be widened for the Pandoc-kind exclude-list to do
what this plan says it does.** Recommended widening: `!ctx.format.identifier.is_html_based() &&
!matches!(ctx.pipeline_profile, PipelineProfile::Pandoc(_))` (i.e., the same seam added above —
`RenderContext::pipeline_profile` — is also the fix here).

This is a *class*, not a one-off: eight transforms self-gate on `is_html_based()`-shaped checks
(`crossref_render.rs`, `draft_alert.rs`, `format_css.rs`, `mermaid.rs`, `panel_tabset.rs`,
`responsive_image.rs`, `title_block.rs`, `toc_generate.rs`). Most are also on the Pandoc
exclude-list, so their self-gate is redundant and harmless — only `panel-tabset` is both
(a) included and (b) self-gated in a way that defeats the inclusion. **Action:** when
implementing the exclude-list, audit all eight against the exclude-list membership above; widen
only the ones that are both included and HTML-gated (today, just `panel-tabset`). Don't widen the
harmless ones speculatively — an already-excluded transform's self-gate never runs regardless.

**Correction (2026-09-18, round 4 review, Reviewer A) — the criterion above is wrong, and
following it literally causes a regression.** Re-audited all eight against the real exclude-list
membership, not just "is it HTML-gated":

| file | gate shape | on the Pandoc exclude-list? |
|---|---|---|
| `crossref_render.rs:88` | **not a gate at all** — `html_float_dom: ctx.format.identifier.is_html_based()` is a config field, no early return | yes (`crossref-render`) |
| `mermaid.rs:175` | `!is_html_based()` → return | yes (`mermaid-render`) |
| `title_block.rs:97` | `should_add_h1(.., ctx.format.is_html())` → return | yes (`title-block`) |
| `toc_generate.rs:85` | `identifier == FormatIdentifier::Html` | yes (`toc-generate`) |
| `panel_tabset.rs:109-114` | `!is_html_based() \|\| is_revealjs_target \|\| is_minimal_html` | **no — deliberately included** |
| `draft_alert.rs:127,142` | `is_html() && !is_revealjs_target` | **no — not on the list** |
| `format_css.rs:93` | `!ctx.format.is_html()` → return | **no — not on the list** |
| `responsive_image.rs:152` | `!format.is_html()` → return | **no — not on the list** |

So **four**, not one, are "both included [by omission from the exclude-list] and HTML-gated":
`panel_tabset.rs`, `draft_alert.rs`, `format_css.rs`, `responsive_image.rs`. Widening all four per
the literal instruction above would be a real regression: `format-css`
(`transforms/format_css.rs`, module doc lines 10-30) *copies user-declared stylesheets into the
output tree and rewrites `css:` metadata entries to per-page hrefs* — widening it for `Pandoc(fmt)`
would stage stray `.css` files next to a `.docx`, exactly the failure shape this plan's own
stage-exclude-list item invokes the 2026-04-20 `CodeHighlightStage` incident about.

**Corrected criterion: not "included and HTML-gated," but "inclusion is deliberate and
load-bearing" — the Pandoc tail actually needs this transform's *output*.** Today that is only
`panel-tabset` (P5's shim needs the `Tabset` CustomNode it builds). **Action, corrected:** widen
only `panel_tabset.rs`'s self-gate (per the snippet above); add `draft-alert`, `format-css`, and
`responsive-image` to the Pandoc-kind exclude-list instead of widening them (they were previously
absent from the list entirely, by omission, not deliberately included). Also: `crossref_render.rs`
does not self-gate at all — the real count is 7 self-gaters, not 8. Citation fix: the
`panel_tabset.rs` gate above is at lines 109-114, not 105-114 (105 is the `async fn transform`
signature).
**Add a regression test that positively asserts the included set actually executes** under a
Pandoc profile (e.g. a fixture with a `.panel-tabset` div yields a `Tabset` CustomNode at the
cut) — the "no B2/B4 before the cut" neutral-core invariant test would pass while this bug
existed, since it only checks *absence*, not that the *included* set fires.

## Finding — AppendixStructure resolved to B3 (the design doc's "last open cell")

Read `transforms/appendix.rs` directly, not the litmus in the abstract. `AppendixStructureTransform`
relocates user `.appendix`-classed Divs, the bibliography (`id=refs`), and footnotes to the
document's end, wraps them in a container, and appends metadata-driven License/Copyright/Citation
sections — entirely as plain Pandoc primitives (`Div`/`Para`/`Header`/`Link`/`Str`). No
`RawInline`/`RawBlock` anywhere in the file. It's explicitly skipped for book format (`is_book_format`
check) — book-mode appendices are a different, unimplemented, out-of-scope concept.

**Decided (confirmed with Gordon): B3, not B2.** Three points of evidence, not just the litmus in
the abstract:
1. The only format-flavored artifacts are a `role: doc-bibliography` ARIA attribute
   (`wrap_bibliography`) and a `class` matching `appendix-style` — both harmless if a non-HTML
   writer ignores them (Pandoc's docx/pptx writers silently drop unknown Div attributes).
2. **The codebase already answered this for a different non-HTML consumer.** `pipeline.rs:1608`:
   `"appendix-structure" (Plan 2B) — pure Pandoc primitives` — and it's on the "now INCLUDED" list
   for `Q2_PREVIEW_TRANSFORM_EXCLUDED`, meaning it already runs, unexcluded, for q2-preview's
   React/wire-format leg. The same argument extends to the Pandoc leg.
3. The `link_rewrite.rs` coupling this plan flagged as needing verification turns out to be pure
   pipeline **ordering** (`LinkRewriteTransform` runs immediately before `AppendixStructureTransform`
   in Finalization, `pipeline.rs:1465-1466`), not a data/code coupling — rewriting link URLs
   before relocating the Div is order-safe regardless of classification.

The B2 alternative (exclude for Pandoc) was rejected: it would silently drop real user content
(`.appendix` divs, license/citation notices) from docx/pptx v1 with no replacement mechanism — a
correctness regression, not a simplification. No split needed either (unlike Footnotes): there's
no format-branching tension here, no native-Pandoc construct being bypassed the way `Note`
bypasses HTML's `<section>`+backlinks.

## Finding — ExampleEmbedRender reclassified from B4 to format-parameterized B1

Originally sourced from a P5 open question ("decide ExampleEmbed's non-HTML behavior — no
proposal yet"). Reading `transforms/example_embed.rs::render_embed` directly (not just its
module doc comment) shows the transform already separates two independent kinds of output:

1. `iframe_block` (`example_embed.rs:380-409`) builds `Block::RawBlock(RawBlock { format:
   "html".to_string(), ... })` — the **identical Pandoc primitive** a hand-authored raw-HTML
   `<iframe>` would use (confirmed this is genuinely how Q1's own docs achieve the same reading
   experience today — `quarto-web/docs/presentations/revealjs/index.qmd:13` is a bare, hand-typed
   `<iframe class="slide-deck" src="demo/">`, with **no** supporting Quarto mechanism at all: no
   Lua filter, no shortcode; grepped the whole `quarto-web` repo to confirm). Pandoc's own
   non-HTML writers already drop a `RawBlock` whose format doesn't match the output format, by
   convention — this needs zero new mechanism to "not render" for docx/pptx.
2. `render_embed`'s other two pieces — the `snippet` slot (hand-authored illustrative code) and
   the `body`/caption slot (source-link fallback, prepended with a "Demo N:" label via
   `with_number_label` when `plain_data.order` is set) — are **plain, portable Pandoc blocks**
   with nothing HTML-specific about them.

**Decided (confirmed with Gordon): reclassify `example-embed-render` from B4 (wholesale excluded
from Pandoc) to format-parameterized B1**, the same pattern as `ShortcodeResolve`: emit the
iframe only when the active `PipelineProfile` supports raw HTML/iframes (`HtmlRender`/
`HtmlPreview`/`RevealjsRender`/`RevealjsPreview`); for `Pandoc(fmt)`, emit `snippet` + the
numbered caption/link, skipping only the `iframe_block` call. This is strictly better than the
B4-exclude alternative, which would silently drop the snippet and source-link content entirely
(the same content-loss failure mode as the AppendixStructure B2 alternative above) — and it's
better than leaving it excluded and reconstructing the same logic in P5's Lua shim (Route N),
since `render_embed` already has this logic in one place. **Consequence for P5:** `ExampleEmbed`
never reaches the Lua shim as a raw `CustomNode` for Pandoc targets — it's resolved to plain
Pandoc blocks upstream of the cut, so P5's "decide ExampleEmbed's non-HTML behavior" open
question is closed, not deferred. Landed in the design doc §6 table alongside this plan's other
corrections.

## In scope
- **Read `transforms/footnotes.rs`, `transforms/appendix.rs`, and `transforms/link_rewrite.rs`
  directly** — done for `appendix.rs`/`link_rewrite.rs` (see Finding above: pure Pandoc
  primitives, and the `link_rewrite.rs` coupling is pipeline ordering only, not a data
  coupling). Still needed for `footnotes.rs`, to confirm the split below is as internally
  decoupled as it looks.
- Introduce the corrected `PipelineProfile { HtmlRender, HtmlPreview, RevealjsRender,
  RevealjsPreview, Pandoc(fmt) }`; the shared core (B1 transforms + Crossref) is emitted
  verbatim, tails dispatch per profile.
- **Add the `Pandoc`-kind exclude-list** (parallel to `Q2_PREVIEW_TRANSFORM_EXCLUDED`) per the
  concrete, name-verified list above: B4 (`crossref-render`, `mermaid-render`,
  `code-block-render`, `table-bootstrap-class` — **not** `example-embed-render`, reclassified
  B1, see above), all of B2 + Navigation (`title-block`, `sectionize`, `title-banner`, the four
  `website-*` transforms, and every Navigation-phase transform — **corrected 2026-09-18: 20
  members, not 18** — add `secondary-nav-render`, `listing-feed-stage`, `listing-feed-link`),
  `attribution-viewer`, `draft-alert`, `format-css`, `responsive-image` (**added 2026-09-18** —
  see the self-gating Finding's correction: these three were previously omitted from this list
  entirely, which is what made their self-gates a real regression risk if widened), and
  `callout-resolve` (design-doc-table correction — see above). **Keep `panel-tabset` (the sugar
  half) enabled** — only `panel-tabset-resolve` gets excluded, diverging from Preview's list.
  **Add a parallel "names exist" validator test** for this list (2026-09-18 — mirroring
  `q2_preview_transform_excluded_names_exist_in_html_pipeline`), given three rounds of
  hand-enumeration have each missed real members.
- **Add `FormatIdentifier::Pptx`** (2026-09-18, round 4 review — see the corrected Seam definition
  item 2 above): a two-arm addition (`as_str`, `output_extension_for`) that makes `pptx` resolvable
  at `Format::from_format_string`, upstream of P7's `render.rs` gate relaxation. Without this,
  `--to pptx` fails before P7's relaxed gate is ever reached.
- **Format-parameterize `ExampleEmbedRenderTransform`** (decided with Gordon — see Finding
  above and design doc §6): emit the iframe (`RawBlock("html", ...)`) only when the active
  profile supports raw HTML/iframes (`HtmlRender`/`HtmlPreview`/`RevealjsRender`/`RevealjsPreview`);
  for `Pandoc(fmt)`, still emit the `snippet` blocks and the (possibly "Demo N:"-labeled) caption
  content, just skip the `iframe_block` call. Same pattern as `ShortcodeResolve`.
- **Split `FootnotesTransform`**: B1 half (`NoteRef`+`Def` → native Pandoc `Note`) stays in
  the core; HTML `<section>`+backlinks half is excluded for `Pandoc` the same way, after
  confirming — by reading the file — that the split is as internally decoupled as it looks.
- **AppendixStructure classified B3** (decided with Gordon — see Finding above): keep it in the
  shared core, running for `Pandoc(fmt)` too, not excluded. Add it to the design doc §6 table
  (currently listed as the open "B3?" cell) as B3, settled.
- Replace `q2-preview`'s two-line deny-list-and-note with a single named `PipelineProfile`
  dispatch that also serves `Pandoc(fmt)` — same underlying mechanism, one seam.
- **Verify no macro `PipelineStage`** (not just `AstTransform`) carries a hidden HTML assumption
  that the neutral-core invariant would otherwise miss.
- **Add `ConditionalContentTransform` to the design doc's bucket table (§6) as B1.** It already
  exists (`transforms/conditional_content.rs`, shipped via the unrelated project-profiles epic,
  bd-fu16z22k), runs unconditionally early in Normalization (`pipeline.rs:1172`), and is already
  format-parameterized via `lua_format_for()` — no relocation needed here, just recognition in
  the design doc that a second format-parameterized-core transform (besides ShortcodeResolve)
  already exists. See P8.

## Out of scope (deferred / other plans)
- Any Pandoc emission beyond the dispatch/exclude-list (P4/P7). The wire-format schema itself
  (P2 — P1 just emits at the cut).
- content-hidden verification against genuine Pandoc targets (P8 — the transform itself needs
  no relocation here, only downstream verification).

## Consumes / Produces (seams)
- **Produces:** the neutral-core cut AST (the wire-format content) + the `PipelineProfile` seam
  that P7 plugs the Pandoc tail into.
- **Produces for P7:** the B3 shared-services segment (ResourceCollector, LinkRewrite, Appendix —
  all confirmed in, not conditional) positioned to run before the Pandoc handoff, and the
  `Pandoc`-kind exclude-list P7's invocation builder can extend per format if needed.

## Coarse checklist
- [x] Read `appendix.rs`, `link_rewrite.rs` directly — done (see Finding above).
- [ ] Read `footnotes.rs` directly; confirm the split below is as internally decoupled as it looks.
- [ ] **Introduce** `PipelineProfile` per the "Seam definition" above (corrected 2026-09-18 from
  "Rename" — nothing exists to rename; `PipelineProfile` doesn't exist in the codebase today).
  Five-variant, two-axis shape (`HtmlRender`/`HtmlPreview`/`RevealjsRender`/`RevealjsPreview`/
  `Pandoc(fmt)`), module + dispatch + `RenderContext` field per the seam definition.
- [ ] Add the `Pandoc`-kind exclude-list per the concrete name list above (B4 minus
  `example-embed-render` + B2 + 20-member Navigation + `attribution-viewer` + `draft-alert` +
  `format-css` + `responsive-image` + `callout-resolve`; keep `panel-tabset` sugar enabled) plus
  its "names exist" validator test (all corrected 2026-09-18, see the exclude-list and Navigation
  Findings above).
- [ ] **Widen `panel_tabset.rs`'s self-gate only** per the corrected "self-gating transforms"
  finding above — required for the exclude-list decision above to actually produce a `Tabset`
  CustomNode for Pandoc targets. **Corrected 2026-09-18: do not widen `draft_alert.rs`,
  `format_css.rs`, or `responsive_image.rs`** — add those three to the exclude-list item above
  instead (widening them would stage stray artifacts like user CSS into a docx output dir). The
  real self-gater count is 7, not 8 (`crossref_render.rs` doesn't self-gate at all).
- [ ] **Add the `Pandoc`-kind *stage*-level exclude list** (new item, 2026-09-18 — the exclude-
  list above only covers `AstTransform`s; `PipelineStage`s are a separate list,
  `build_html_pipeline_stages_with_options`, `pipeline.rs:270`, and Preview already needs its own
  parallel `Q2_PREVIEW_STAGE_EXCLUDED`, `pipeline.rs:395`). At minimum drop `MathJsStage`,
  `RenderHtmlBodyStage`, `ApplyTemplateStage` (would error or produce HTML output) and
  `CompileThemeCssStage`/`BootstrapJsStage`/`ClipboardJsStage`/`TabsetsJsStage` (would compile
  Bootstrap SCSS and stage `site_libs/*` into a docx output directory for no reason — silent
  wasted work and stray artifacts, the same failure shape as the 2026-04-20 `CodeHighlightStage`
  incident this repo's own CLAUDE.md records). `CodeHighlightStage` is harmless to leave in
  (pandoc's docx writer drops its `data-hl-spans` attributes) but is also wasted work worth
  knowing about. **Corrected 2026-09-18 (round 4 review, Reviewer A) — three specification gaps
  found in this item itself:** (1) the mechanism consumes `name()` strings, not Rust type names —
  write this list as `&["math-js", "render-html-body", "apply-template", "compile-theme-css",
  "bootstrap-js", "clipboard-js", "tabsets-js", "code-highlight"]`, matching
  `Q2_PREVIEW_STAGE_EXCLUDED`'s own convention (`pipeline.rs:395`), and give it the same
  "names exist" validator Preview's has (`pipeline.rs:4007`). (2) `attribution-generate` is the
  `name()` of **both** a transform (excluded above, per the B4 Navigation-adjacent finding) and a
  distinct stage (`stage/stages/attribution_generate.rs:67`, registered as `stages[16]`,
  `pipeline.rs:2198`) — the stage self-gates on `is_feature_disabled(meta, "attribution")` and
  attribution is off by default, so it currently no-ops, but decide explicitly whether the stage
  also belongs on this list (same "silent wasted work" rationale) rather than relying on the
  default-off self-gate. (3) **This list is owned jointly with P4**, not solely by P1: P4 Finding 3
  states "P4 owns... the Pandoc-leg stage list itself, since P4 is the plan that introduces
  `PandocWriteStage`." Cross-reference P4's item explicitly here so ownership isn't
  one-directional (P4 already cross-references this item; this item did not, until now,
  cross-reference P4).
- [ ] **Build a byte-identity corpus + capture/diff harness for the "HTML/revealjs/q2-preview
  byte-identity" review bar** (new item, 2026-09-18 — this plan's stated hard no-regression bar
  has no backing mechanism; `quarto-core`'s 18 existing snapshots are all fragment-level, none a
  whole rendered document). Name a corpus (e.g. `docs/` + the crossref fixture set), capture
  output before this plan's refactor, capture again after, diff. Without this, "byte-identical"
  will be demonstrated only by "workspace tests pass," a materially weaker claim.
- [x] Fix the design doc §6 bucket table: `CalloutResolve` was misclassified as B1 — it's
  empirically B4 (already excluded from Preview for exactly that reason); moved to the B4 row.
  Also updated `AppendixStructure`'s row from the open "B3?" to settled **B3**, and moved
  `ExampleEmbedRender` from the B4 row to B1 (format-parameterized).
- [ ] Format-parameterize `ExampleEmbedRenderTransform` (decided with Gordon — see Finding
  above): emit the iframe only for iframe-capable profiles; emit snippet + numbered caption for
  `Pandoc(fmt)`. Closes P5's "ExampleEmbed non-HTML behavior" open question.
- [ ] Fix `title_block.rs`'s non-HTML branch or confirm the exclude-list makes it moot; add a regression test proving no duplicate title in a docx/pptx smoke fixture (coordinate with P7).
- [ ] Footnotes split (native `Note` in core; HTML section excluded for Pandoc same as HTML-family); byte-identical HTML.
- [x] AppendixStructure litmus → classified **B3**, decided with Gordon (see Finding above).
- [ ] `PipelineProfile` dispatch serves Preview and Pandoc from the same mechanism; preview parity holds.
- [ ] Confirm no macro `PipelineStage` is implicitly HTML-shaped.
- [ ] Neutral-core invariant test (no B2/B4 before the cut); **extend to assert total bucket
  coverage** (every transform registered in `build_transform_pipeline` classified exactly once
  in the design doc §6 table) — added 2026-09-17 per an epic-wide review finding that the
  hand-enumerated exclude-list missed 4 real transforms (`breadcrumbs-render`, `quarto-nav-js`,
  `repo-actions-render`, plus the already-known `attribution-viewer` gap) that a total-coverage
  test would have caught mechanically.
- [x] **Decide and document how `PipelineProfile::Pandoc(fmt)` and P4's `PandocWriteStage` behave
  under `#[cfg(target_arch = "wasm32")]`** — **resolved 2026-09-18 by the Seam definition item 5
  above** (this checklist item was left open a round after the decision itself landed in prose —
  same never-landed-fix pattern this epic keeps rediscovering): the type is **not** `cfg`-gated —
  `PipelineProfile::Pandoc(fmt)` stays in every exhaustive match unconditionally — while
  `PandocWriteStage` itself compiles only under `#[cfg(not(target_arch = "wasm32"))]`, so an
  unreachable-for-wasm32 profile is a compile-time non-issue, not a runtime catalog error to
  design. Still validate with full `cargo xtask verify` (not `--skip-hub-build`) once implemented,
  per this repo's own CLAUDE.md warning about exactly this trap.
- [ ] Add `ConditionalContentTransform` to design doc §6 bucket table as B1.
- [ ] Fix the design doc §5 `PipelineProfile` shorthand to the corrected five-variant shape.

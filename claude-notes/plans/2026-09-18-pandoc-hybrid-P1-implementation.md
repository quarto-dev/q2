# P1 — Implementation tasks & Test Seam Spec

**Date:** 2026-09-18
**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P1-neutral-core.md`](2026-08-20-pandoc-hybrid-P1-neutral-core.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Depends on:** nothing (per the epic's graph, P1/P2/P3 are parallel immediately). **Consumed by:**
P5 (needs the `Pandoc`-kind exclude-list decision — specifically that `panel-tabset`'s sugar half
stays enabled), P7 (needs the `PipelineProfile` seam + the B3 shared-services segment).
**Status:** Ready for subagent-driven execution. **All nine tasks are dispatchable** — the three
findings that previously parked work (F1's Footnotes shape, F3's bucket-coverage mechanism, F6's
`LinkRewrite` cell) were decided by Gordon on 2026-09-18 and are applied; see
`## Findings for Gordon`. No task is blocked on a finding.

This file adds nothing to P1's scope — it converts P1's Coarse checklist into `## Task N` units
`superpowers:subagent-driven-development` can dispatch, and binds every test P1 needs to a named
production seam and revert hunk before any code is written (the `/prevalidating-test-seams`
discipline). The Spec is P1 + the design doc; where this file and the plan disagree, the plan wins.

Every `file.rs:line` below was opened and confirmed against the worktree at
`feature/pandoc-writer-hybrid` on 2026-09-18. Where a plan citation had drifted, this file cites
what is actually there and records the drift in **Findings for Gordon** (§F7) rather than silently
propagating or silently "fixing" it.

## Tiers used in this file

| Tier | What it is | Where it lives | How it runs |
|---|---|---|---|
| **U** | Rust unit test | `#[test]` in `mod tests` inside the crate under test — for P1 that is almost always `crates/quarto-core/src/pipeline.rs::tests` (the only place `build_transform_pipeline` is exercised today) or `crates/quarto-core/src/format.rs::tests` | `cargo nextest run -p quarto-core` |
| **I** | Rust integration test | `crates/quarto-core/tests/integration/<name>.rs`, registered as `pub mod <name>;` in `crates/quarto-core/tests/integration/main.rs` (98 modules today). **Never** a top-level `crates/quarto-core/tests/<name>.rs` — `.claude/rules/integration-tests.md` | `cargo nextest run -p quarto-core` (binary `integration`) |
| **L** | Lua/pandoc integration test | requires a real `pandoc --lua-filter` / `pandoc -f json -t <fmt>` invocation against the vendored Q1 tree | **P1 needs zero L-tier tests.** P1 performs no Pandoc invocation and vendors no Lua; the vendored tree does not exist until P4. Any L-tier assertion in this file would be a placeholder. |
| **G** | dev-only capture/diff | a local-only harness driving the real `q2` binary; artifacts are not committed unless stated | local/dev gate only |

**One pandoc invocation was used to *write* this file** (not to test the implementation): verifying
the load-bearing empirical premise behind excluding `title-block`. Recorded under Task 2.

**End-to-end verification ceiling for P1 (CLAUDE.md "End-to-end verification before declaring
success").** P1 deliberately ships **no Pandoc output path** — design §9: "No Pandoc." So no P1
task can be verified through `cargo run --bin q2 -- render x.qmd --to docx`: `render.rs:680-685`
still rejects every non-native format, and even with that gate relaxed there is no writer. The
honest status any implementer must report is: *"in-process pipeline-composition tests pass; the
docx/pptx render path does not exist yet and was not exercised."* The one leg that **is**
end-to-end verifiable in P1 is the no-regression bar — `q2 render` of the HTML/reveal/preview
corpus (Task 8), and that is exactly why Task 8 is not optional.

---

## Task 1: Introduce `PipelineProfile` + `FormatIdentifier::Pptx` (the seam)

**Scope.** Add the five-variant `PipelineProfile` enum and its single derivation point; add the
missing `FormatIdentifier::Pptx` variant so `pptx` resolves at all; thread the profile through
`build_transform_pipeline` and `RenderContext`, replacing the two existing ad-hoc axes (the inline
`is_revealjs` family check and the `pipeline_kind` string kind check). **Pure wiring — no
behavioral change to any existing format.**

**Files.**
- `crates/quarto-core/src/format.rs` — new `PipelineProfile` + `from_format`, next to
  `FormatIdentifier` (`format.rs:23-42`, `Copy, Clone, PartialEq, Eq` derives per the plan's Seam
  item 1). `Pptx` variant into the enum at `format.rs:29` (`Docx` already exists — confirmed).
  Three edits follow it: the exhaustive `as_str()` match (`format.rs:44-56`), the exhaustive
  `output_extension_for()` match (`format.rs:279-291`), **and the non-exhaustive
  `TryFrom<&str> for FormatIdentifier` string match (`format.rs:82-96`)** — see Finding F5: only
  the third one actually makes `pptx` resolvable, and the compiler will not force it.
- `crates/quarto-core/src/format.rs:110-131` (`builtin_pseudo_format`) and
  `format.rs:137-139` (`is_revealjs_target`) — the two existing mechanisms `from_format` must
  reproduce exactly.
- `crates/quarto-core/src/render.rs:207` (`pub struct RenderContext<'a>`) — new
  `pub pipeline_profile: PipelineProfile` field; derived inside `RenderContext::new`
  (`render.rs:442`), which already receives `&format`. **This is why the field is not a ripple:**
  266 `RenderContext::new(...)` call sites exist workspace-wide (including
  `crates/quarto-lsp-core/src/analysis.rs:68` and ~40 integration-test helpers) and none needs to
  change if the derivation happens inside `new()`.
- `crates/quarto-core/src/pipeline.rs:1144` (`build_transform_pipeline`) — takes the profile as an
  explicit parameter; the internal `let is_revealjs = crate::format::is_revealjs_target(...)`
  (`pipeline.rs:1157`) becomes a profile match, consumed at `pipeline.rs:1264`, `:1279`, `:1455`
  (`footer_render_stage(is_revealjs)`) and `:1482` (`reveal_finalization_transforms(is_revealjs)`).
- `crates/quarto-core/src/stage/stages/ast_transforms.rs:138` — `jit_pipeline = match
  ctx.format.pipeline_kind { Some("preview") => … }` becomes a match on the profile.

**Acceptance criterion.**
1. `PipelineProfile::from_format` returns, exactly: `"html"`→`HtmlRender`, `"q2-debug"`→`HtmlRender`,
   `"acm-html"`→`HtmlRender`, `"q2-preview"`→`HtmlPreview`,
   **`"q2-sandboxed-preview"`→`HtmlPreview`** (see F4), `"revealjs"`→`RevealjsRender`,
   `"q2-slides"`→`RevealjsPreview`, `"docx"`→`Pandoc("docx")`, `"pptx"`→`Pandoc("pptx")`,
   `"gfm"`→`Pandoc("gfm")` — asserted by T1.1.
2. `Format::from_format_string("pptx")` returns `Ok`, with `identifier == FormatIdentifier::Pptx`,
   `output_extension == "pptx"`, `native_pipeline == false` — asserted by T1.2. (Today it returns
   `Err("Unknown format: pptx")` from `format.rs:449`.)
3. `RenderContext::new(&project, &doc, &docx_format, &binaries).pipeline_profile ==
   PipelineProfile::Pandoc("docx".into())`, with **zero** changes to any existing
   `RenderContext::new` call site — asserted by T1.3, enforced by the build.
4. The ordered transform-name list produced by `build_transform_pipeline(HtmlRender)` and
   `(RevealjsRender)` is **byte-identical to today's** for `"html"`/`"revealjs"` — asserted by T1.4
   and T1.5 as exact `&[&str]` literals (not snapshots, so P1's "zero `.snap` changes" criterion is
   unaffected).
5. `cargo clippy -p quarto-core --all-targets -- -D warnings` and
   `cargo nextest run -p quarto-core` green; zero `.snap` files under
   `crates/quarto-core/tests/integration/snapshots/` (18 today) change.
6. **Full `cargo xtask verify`, not `--skip-hub-build`** — per the plan's Seam item 4, every
   `RenderContext` field touches the wasm build.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T1.1 | U | `quarto_core::format::PipelineProfile::from_format` | call with the 10 format strings in criterion 1 → `assert_eq!` each against its variant | none | the `"q2-slides"` and `"q2-sandboxed-preview"` arms of `from_format` |
| T1.2 | U | `quarto_core::format::Format::from_format_string` | `from_format_string("pptx")` → `Ok`, identifier/extension/native fields | none | the `"pptx" => Ok(FormatIdentifier::Pptx)` arm in `TryFrom<&str>` (`format.rs:82-96`) |
| T1.3 | U | `quarto_core::render::RenderContext::new` | build a docx `Format`, construct a context → `ctx.pipeline_profile` | `ProjectContext`/`DocumentInfo`/`BinaryDependencies` test doubles already used by `format.rs`/`pipeline.rs` tests | the `pipeline_profile: PipelineProfile::from_format(&format.target_format)` initializer inside `RenderContext::new` |
| T1.4 | U | `quarto_core::pipeline::build_transform_pipeline` | build with `HtmlRender` → `assert_eq!` the full ordered `Vec<&str>` of `t.name()` against a literal captured pre-refactor | test `SystemRuntime` (`make_test_runtime()`, already used at `pipeline.rs:3835`) | the `PipelineProfile::HtmlRender \| HtmlPreview => { TitleBlock, Sectionize }` arm replacing the `else` branch at `pipeline.rs:1274-1277` |
| T1.5 | U | same | build with `RevealjsRender` → exact ordered name list contains `reveal-columns`/`reveal-slides`/`reveal-footer-alias` and **not** `title-block`/`sectionize` | same | the `RevealjsRender \| RevealjsPreview` arm replacing `if is_revealjs` at `pipeline.rs:1264` |

**Revert hunks, stated exactly:**

- **T1.1** — Revert ⟨the `"q2-slides" => PipelineProfile::RevealjsPreview` arm in
  `PipelineProfile::from_format`, so `"q2-slides"` falls through to `RevealjsRender`⟩ →
  ⟨`assert_eq!(PipelineProfile::from_format("q2-slides"), PipelineProfile::RevealjsPreview)`⟩ RED.
  And: Revert ⟨the `"q2-sandboxed-preview" => HtmlPreview` arm⟩ →
  ⟨`assert_eq!(from_format("q2-sandboxed-preview"), HtmlPreview)`⟩ RED.
- **T1.2** — Revert ⟨the `"pptx" => Ok(FormatIdentifier::Pptx)` arm in `TryFrom<&str> for
  FormatIdentifier`, `format.rs:82-96`⟩ → ⟨`assert!(Format::from_format_string("pptx").is_ok())`⟩
  RED (it returns `Err("Unknown format: pptx")` from `format.rs:449`). Note the inverse is *not*
  true: adding only the `as_str`/`output_extension_for` arms leaves this assertion RED, which is
  exactly F5.
- **T1.3** — Revert ⟨the `pipeline_profile: …` initializer in `RenderContext::new`,
  `render.rs:442`, replacing it with `PipelineProfile::HtmlRender`⟩ →
  ⟨`assert_eq!(ctx.pipeline_profile, PipelineProfile::Pandoc("docx".into()))`⟩ RED.
- **T1.4** — Revert ⟨any reordering or omission inside `build_transform_pipeline`'s
  `HtmlRender` path — e.g. drop `pipeline.push(Box::new(SectionizeTransform::new()))` at
  `pipeline.rs:1276`⟩ → ⟨the `assert_eq!` on the exact ordered name list⟩ RED.
- **T1.5** — Revert ⟨the `RevealjsRender`/`RevealjsPreview` arm at `pipeline.rs:1264`, making every
  profile take the HTML-family branch⟩ → ⟨`assert!(!names.contains(&"title-block"))` for
  `RevealjsRender`⟩ RED.

### Refactor-induced vacuity check

**The four→five-variant correction (landed in commit `917b943a5`, "PipelineProfile's four flat
variant names conflate the two axes with no slot for reveal+preview").** The old shorthand
`{HtmlFull, Revealjs, Preview, Pandoc(fmt)}` — still standing in design §5 line 177 — has **no
q2-slides cell**. A test that asserts only four variants, or that asserts
`from_format("q2-slides")` is "some reveal variant", **survives the bug**: `q2-slides` would map to
`RevealjsRender`, the full reveal-render pipeline would run for the preview leg, and nothing would
be asserted about it. T1.1 therefore must assert the `RevealjsPreview` cell **by name and by
distinctness from `RevealjsRender`**, which is the only surface on which the two differ. The
corrected expected value still discriminates: `RevealjsRender != RevealjsPreview`, and after Task 2
their surviving name lists differ too (the preview exclude-list is applied to one and not the
other) — so T2.5 is the behavioral second anchor for the same cell.

**A sibling trap the five-variant shape does not close, found here:** `q2-slides`'s *base* format
is `"html"`, not `"revealjs"` (`format.rs:122`, `"q2-slides" => Some(("html", Some("preview")))`).
Its reveal-family membership comes only from `is_revealjs_target` matching the **string**. So a
`from_format` implementation that derives family from `format.identifier` instead of from
`target_format` maps `q2-slides` to `HtmlPreview` and **still passes any test that only checks
`is_html_based()`-shaped predicates**. T1.1's explicit `q2-slides → RevealjsPreview` assertion is
the discriminator; `is_html_based()` is not.

**The `Pptx` cost-model correction (commit `4844c707c`).** The corrected expected value —
"two-arm change, not a ripple" — is *load-bearing for scope* but **does not discriminate the bug
it was written about.** `as_str` and `output_extension_for` are the two matches the compiler
forces; neither is on the failure path. The state this test exists to distinguish is
"`--to pptx` resolves" vs. "`--to pptx` dies at `format.rs:449`", and the only hunk that moves
that is the `TryFrom` arm. T1.2 asserts `from_format_string("pptx")`, not `Pptx.as_str()`, for
exactly that reason. An `assert_eq!(FormatIdentifier::Pptx.as_str(), "pptx")` test would compile
only *after* the fix and pass whether or not `pptx` is resolvable — vacuous. Keep such an
assertion, if at all, as shape/gating only.

---

## Task 2: Add the `Pandoc`-kind transform exclude-list + its validators

**Scope.** Add `PANDOC_TRANSFORM_EXCLUDED: &[&str]` next to `Q2_PREVIEW_TRANSFORM_EXCLUDED`, apply
it at the same seam the preview list is applied at, and add the two guards three consecutive
review rounds proved are needed: a "names exist" validator and a Navigation-totality assertion
derived by querying `phase()` rather than by hand-enumeration.

**Files.**
- `crates/quarto-core/src/pipeline.rs:1594-1639` — `Q2_PREVIEW_TRANSFORM_EXCLUDED` (7 entries:
  `callout-resolve` :1595, `attribution-viewer` :1603, `title-block` :1604, `crossref-render` :1620,
  `mermaid-render` :1626, `panel-tabset` :1637, `panel-tabset-resolve` :1638). The new const sits
  beside it.
- `crates/quarto-core/src/pipeline.rs:1651-1671` (`build_q2_preview_transform_pipeline`) and
  `crates/quarto-core/src/transform.rs:230-233` (`TransformPipeline::retain_excluding`, which
  **silently drops unknown names** — the reason the validator exists).
- `crates/quarto-core/src/pipeline.rs:2851` (`q2_preview_transform_excluded_names_exist_in_html_pipeline`)
  — the mirror for T2.1.
- `crates/quarto-core/src/transform.rs:72-94` (`TransformPhase`) — the `phase()` query for T2.2.

**The list, derived and name-verified (38 entries).** Every name below was confirmed to be a real
`fn name()` return in `crates/quarto-core/src/`, and every one was confirmed to be a member of
`build_transform_pipeline` (`pipeline.rs:1144-1555`, 66 `pipeline.push`/`extend` sites):

- **B4, four:** `crossref-render`, `mermaid-render`, `code-block-render`, `table-bootstrap-class`.
- **B2, seven:** `title-block`, `sectionize`, `title-banner`, `website-title-prefix`,
  `website-favicon`, `website-bootstrap-icons`, `website-canonical-url`.
- **Navigation, twenty — count independently re-verified here** by grepping
  `TransformPhase::Navigation` across `crates/quarto-core/src/`: `navbar-generate`,
  `navbar-render`, `sidebar-generate`, `sidebar-render`, `secondary-nav-render`,
  `breadcrumbs-render`, `quarto-nav-js`, `page-nav-generate`, `page-nav-render`, `toc-generate`,
  `toc-render`, `toc-location`, `footer-generate`, `footer-render`, `listing-generate`,
  `listing-render`, `listing-feed-stage`, `listing-feed-link`, `categories-sidebar`,
  `repo-actions-render`. (A 21st `TransformPhase::Navigation` declaration exists at
  `crates/quarto-core/src/revealjs/footer_logo.rs:124` — the reveal-family alternative spliced by
  `footer_render_stage(is_revealjs)`, `pipeline.rs:1455`; it never reaches the Pandoc branch, so
  the HTML-family count is 20. The plan's corrected figure checks out.)
- **B4-adjacent / previously unclassified, five:** `attribution-viewer`, `attribution-render`,
  `draft-alert`, `format-css`, `responsive-image`.
- **Design-doc-table corrections, two:** `callout-resolve`, `panel-tabset-resolve`.

4 + 7 + 20 + 5 + 2 = **38**. The plan's "~33-name Pandoc list" (`pipeline.rs:2851` note) is a
stale approximation; 38 is the derived figure. **`attribution-generate` is deliberately absent** —
see Finding F2: it is not a member of `build_transform_pipeline` at all, so listing it here would
fail T2.1. **`panel-tabset` (sugar) is deliberately absent** (diverges from Preview's list, per the
plan and design §6's `panel-tabset` row). `config-markdown`, `reference-link-diagnostics` and
`llms-capture` are deliberately absent per the plan's round-4 correction (format-agnostic /
`website.llms-txt`-gated).

After Task 5 lands, the list grows by exactly one: the Footnotes HTML-half's new `name()`.

**`title-block` sub-item (P1 checklist "Fix `title_block.rs`'s non-HTML branch or confirm the
exclude-list makes it moot").** Confirmed moot at the pipeline level by T2.3's exact-list
assertion. The premise was re-verified empirically here against real pandoc **3.8.1** (the plan's
verification was against an unnamed version): `pandoc -s t.md -t docx` on a document whose only
metadata is `title: My Doc Title` and whose body has no heading emits
`<w:pStyle w:val="Title"/>` carrying `My Doc Title`, and `-t pptx` emits the same title into
`ppt/slides/slide1.xml`. So `should_add_h1`'s non-HTML `true` branch (`title_block.rs:65-75`,
reached via `title_block.rs:97`) would genuinely duplicate the title. The *output-level* proof
(no duplicate title in a real `.docx`) is **deferred — seam deferred until P7's per-format
invocation builder**; P1 can only assert the transform does not run.

**Acceptance criterion.**
1. `PANDOC_TRANSFORM_EXCLUDED` has exactly the 38 names above; every one is a member of
   `build_transform_pipeline` (T2.1).
2. `PANDOC_TRANSFORM_EXCLUDED` contains **every** transform in the HtmlRender pipeline whose
   `phase() == TransformPhase::Navigation` — asserted by a `phase()` query, with the resulting set
   asserted to have length 20 (T2.2).
3. `build_transform_pipeline(Pandoc("docx"))`'s surviving ordered name list equals an exact
   `&[&str]` literal that **does** contain `conditional-content`, `callout`, `panel-tabset`,
   `shortcode-resolve`, `metadata-normalize`, `date-normalize`, `authors-normalize`,
   `code-block-generate`, `example-embed`, `theorem-sugar`, `proof-sugar`,
   `float-ref-target-sugar`, `equation-label`, `crossref-index`, `crossref-resolve`,
   `example-embed-render`, `link-rewrite`, `appendix-structure`, `resource-collector`, and does
   **not** contain any of the 38 (T2.3).
4. `build_transform_pipeline(HtmlPreview)`'s surviving name list equals today's
   `build_q2_preview_transform_pipeline(...)` name list, captured as a literal pre-refactor (T2.5)
   — the preview-parity gate for "one mechanism serves Preview and Pandoc".
5. `cargo nextest run -p quarto-core` green; zero `.snap` changes.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T2.1 | U | `PANDOC_TRANSFORM_EXCLUDED` ∩ `build_transform_pipeline` | build `HtmlRender` pipeline, collect `name()`s → every exclude-list entry is present in that set (`unknown.is_empty()`) | `make_test_runtime()` | the const's own entries (a typo'd/renamed entry is the failure this guards) |
| T2.2 | U | `AstTransform::phase` + the const | build `HtmlRender`, filter `phase() == Navigation` → that set has len 20 **and** is a subset of `PANDOC_TRANSFORM_EXCLUDED` | same | any Navigation name deleted from the const |
| T2.3 | U | `build_transform_pipeline` + `retain_excluding` | build `Pandoc("docx")` → `assert_eq!` the **exact** ordered surviving name list against a literal | same | the `retain_excluding(PANDOC_TRANSFORM_EXCLUDED)` application inside `build_transform_pipeline`'s `Pandoc(_)` arm |
| T2.5 | U | same, Preview axis | build `HtmlPreview` → `assert_eq!` exact ordered name list against the pre-refactor `build_q2_preview_transform_pipeline` capture | same | the `HtmlPreview`/`RevealjsPreview` arm that applies `Q2_PREVIEW_TRANSFORM_EXCLUDED` |

**Revert hunks, stated exactly:**

- **T2.1** — Revert ⟨rename `"secondary-nav-render"` to `"secondary-nav-renders"` inside
  `PANDOC_TRANSFORM_EXCLUDED`⟩ → ⟨`assert!(unknown.is_empty(), …)`⟩ RED. (Without this test the
  typo is *silent*: `retain_excluding` at `transform.rs:231` is a bare
  `self.transforms.retain(|t| !exclude.contains(&t.name()))` with no unknown-name diagnostic.)
- **T2.2** — Revert ⟨delete `"listing-feed-link"` from `PANDOC_TRANSFORM_EXCLUDED`⟩ →
  ⟨`assert!(nav_names.iter().all(|n| PANDOC_TRANSFORM_EXCLUDED.contains(n)), …)`⟩ RED. This is the
  exact failure mode that recurred in three consecutive review rounds (18 → 20 Navigation members);
  the query-based form cannot miss a member a hand-list can.
- **T2.3** — Revert ⟨add `"panel-tabset"` to `PANDOC_TRANSFORM_EXCLUDED`⟩ → ⟨the `assert_eq!` on
  the exact surviving name list⟩ RED. And: Revert ⟨remove the
  `retain_excluding(PANDOC_TRANSFORM_EXCLUDED)` call from the `Pandoc(_)` arm⟩ → same assertion RED.
- **T2.5** — Revert ⟨delete `"crossref-render"` from `Q2_PREVIEW_TRANSFORM_EXCLUDED`
  (`pipeline.rs:1620`)⟩ → ⟨the `HtmlPreview` exact-name-list `assert_eq!`⟩ RED.

### Refactor-induced vacuity check

**The exclude-list superset trap — this is the single most important vacuity finding in P1.** An
exclude-list test written as a *superset* relation (`assert!(excluded.contains(name))` for each
name the author thinks should be excluded, or `assert!(!surviving.contains("crossref-render"))`)
**passes when a needed transform is wrongly excluded.** Concretely: adding `"panel-tabset"` to the
list — the exact mistake Preview's list already makes deliberately and the Pandoc list must not —
breaks P5's entire Route-R Tabset path, and **every absence-shaped assertion still passes**. So:

- T2.3 asserts the **exact, ordered, complete** surviving name list, not a set of absences. Both
  directions are pinned by one `assert_eq!`: a wrongly-added exclusion and a wrongly-omitted
  exclusion both redden it.
- T2.2's subset assertion (`Navigation ⊆ excluded`) is deliberately one-directional, because its
  job is to catch *omissions* mechanically; T2.3 is its counterweight for over-exclusion. Neither
  alone is sufficient, and neither should be written without the other.

**The Navigation count correction (18 → 20, commit `4844c707c`).** The corrected expected value
20 still discriminates — but only in the query-based form. An `assert_eq!(nav_names.len(), 20)`
written **against a hand-list** collapses the moment a 21st Navigation transform lands: the
hand-list has 20, the pipeline has 21, and the assertion compares the hand-list to itself. T2.2
derives both sides from `build_transform_pipeline` + `phase()`, so the `20` is a tripwire on
*pipeline growth*, not a restatement of the list. If a reviewer ever finds the `20` literal and
the exclude-list's Navigation portion coming from the same source, the test has gone vacuous.

**The `example-embed-render` B4→B1 reclassification (commit `a684cfac0`).** Expected value changed
from "on the exclude-list" to "not on the exclude-list." It still discriminates only because T2.3
asserts the exact list: `example-embed-render` appearing in the surviving list is the positive
assertion. A test that only asserted the excluded set would read identically before and after the
reclassification for this name, since its absence from an absence-list is not an assertion.

---

## Task 3: Widen `panel_tabset.rs`'s self-gate — and only that one

**Scope.** Widen the one self-gate whose early return defeats a deliberate, load-bearing inclusion,
and add the positive regression test the plan calls for. Do **not** widen `draft_alert.rs`,
`format_css.rs` or `responsive_image.rs` — those three are added to Task 2's exclude-list instead.

**Files.** `crates/quarto-core/src/transforms/panel_tabset.rs:109-114` — verified verbatim:

```rust
if !ctx.format.identifier.is_html_based()
    || is_revealjs_target(&ctx.format.target_format)
    || is_minimal_html(&ast.meta)
{
    return Ok(());
}
```

(The plan's corrected citation `109-114` is right; the `async fn transform` signature is at
`:105`.) Widening per the plan: `!ctx.format.identifier.is_html_based() &&
!matches!(ctx.pipeline_profile, PipelineProfile::Pandoc(_))` for the first term, the other two
terms unchanged.

**Prerequisite.** Requires Task 1 (`RenderContext::pipeline_profile`) and Task 2 (the exclude-list
that keeps `panel-tabset` enabled). Neither is external to this plan.

**Acceptance criterion.**
1. A fixture containing `::: {.panel-tabset}` with two `## ` tab-title headers, run through
   `build_transform_pipeline(Pandoc("docx"))`, yields a `CustomNode` with `type_name == "Tabset"`
   in the resulting AST, and no surviving `Header` carrying a tab title (T3.1).
2. The other two gate terms are intact: under `HtmlRender` with `is_minimal_html(meta)` true, no
   `Tabset` CustomNode is produced (T3.2).
3. `draft-alert`, `format-css`, `responsive-image` are absent from
   `build_transform_pipeline(Pandoc("docx"))` — already covered by T2.3's exact list; **no separate
   test** (it would be a restatement, not a second discriminator).
4. `cargo clippy -p quarto-core --all-targets -- -D warnings` + `cargo nextest run -p quarto-core`
   green; zero `.snap` changes.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | I (`crates/quarto-core/tests/integration/pandoc_profile_cut.rs`, new; register in `tests/integration/main.rs`) | `PanelTabsetTransform::transform` inside the real `Pandoc("docx")` transform pipeline | parse the tabset fixture, run the `Pandoc("docx")` pipeline over it → a `CustomNode("Tabset")` exists; **and** the pipeline's own name list contained `"panel-tabset"` (the path-was-exercised assertion) | test `SystemRuntime`; no external binary | `panel_tabset.rs:109`'s widened first gate term |
| T3.2 | U (`panel_tabset.rs::tests`) | `PanelTabsetTransform::transform` | `HtmlRender` profile + `format: html` + minimal-HTML meta → no `Tabset` CustomNode | none | `panel_tabset.rs:111`'s `\|\| is_minimal_html(&ast.meta)` term |

**Revert hunks, stated exactly:**

- **T3.1** — Revert ⟨`panel_tabset.rs:109`'s widened first term back to the bare
  `!ctx.format.identifier.is_html_based()`⟩ → ⟨`assert!(ast_contains_custom_node(&ast, "Tabset"))`⟩
  RED. This is the bug the plan's self-gating Finding exists for: `is_html_based()` is
  `matches!(self, Html | Revealjs)` (`format.rs:63-65`), so for a docx `Format` the transform
  returns at `:113` and builds nothing — **while Task 2's exclude-list still reads as "panel-tabset
  enabled."**
- **T3.2** — Revert ⟨delete the `|| is_minimal_html(&ast.meta)` term at `panel_tabset.rs:111`⟩ →
  ⟨`assert!(!ast_contains_custom_node(&ast, "Tabset"))` under minimal HTML⟩ RED.

### Refactor-induced vacuity check

**The widening-criterion correction (commit `4844c707c`: "widen only `panel-tabset`", was "widen
all four").** The corrected criterion changed *which hunks are edited*, not an expected value — so
the vacuity question here is about the **absence** of three tests. There is deliberately **no**
test asserting `format_css.rs`'s gate is un-widened, because such a test would have to assert the
*absence* of a code change, which no runtime surface distinguishes: with `format-css` on the
exclude-list, its gate never executes for a Pandoc profile, so widened or not, every behavioral
assertion reads identically. The state this would need to distinguish — "stray `.css` files staged
next to a `.docx`" — has **no observable surface in P1** (no Pandoc output directory exists).
Logged in the Missing-test pass as a deferred seam, not silently assumed covered.

**T3.1's "the path was actually exercised" assertion is not optional.** Without the
`names.contains(&"panel-tabset")` co-assertion, T3.1 goes RED for two indistinguishable reasons —
the self-gate wasn't widened, or `panel-tabset` got onto the exclude-list. Both are real bugs with
different fixes; the co-assertion disambiguates the failure message and stops a future "fix" that
removes the transform from the pipeline from reading as a legitimate way to satisfy the test.

---

## Task 4: Format-parameterize `ExampleEmbedRenderTransform`

**Scope.** Emit the `RawBlock("html")` iframe only for iframe-capable profiles; for
`Pandoc(fmt)`, emit the `snippet` blocks and the (possibly `Demo N:`-labelled) caption, skipping
only the `iframe_block` call. Same pattern as `ShortcodeResolve`.

**Files.** `crates/quarto-core/src/transforms/example_embed.rs` — `name()` → `"example-embed-render"`
at `:273-275`; `iframe_block` at `:380-408`, whose tail is verified verbatim:

```rust
Block::RawBlock(RawBlock {
    format: "html".to_string(),
    text: html,
    source_info: source_info.clone(),
})
```

`render_embed`'s snippet/caption path and the `with_number_label` "Demo N: " prepend follow
immediately after (`example_embed.rs:410+`). The sugar half is `"example-embed"` (`:125-127`),
registered at `pipeline.rs:1303`; the render half at `pipeline.rs:1475`.

**Prerequisite.** Requires Task 1 (`RenderContext::pipeline_profile`).

**Acceptance criterion.**
1. Under `Pandoc("docx")`, `ExampleEmbedRenderTransform` leaves **no** `Block::RawBlock` whose
   `format == "html"` in the output, and **does** leave the `snippet` slot's blocks (T4.1).
2. Under `HtmlRender`, the output still contains the `RawBlock("html")` iframe (T4.2) — byte-identical
   to today.
3. When `plain_data.order` is set, the `Demo N:` caption label is present under `Pandoc("docx")`
   too (T4.3).
4. `example-embed-render` is in the surviving `Pandoc("docx")` name list (Task 2's T2.3), and
   `cargo nextest run -p quarto-core` is green with zero `.snap` changes.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T4.1 | U (`example_embed.rs::tests`) | `ExampleEmbedRenderTransform::transform` | `CustomNode("ExampleEmbed")` with `snippet` + `body` slots, `Pandoc("docx")` profile → no `RawBlock{format:"html"}`; snippet blocks present | `ResourceResolverContext` test double already used by `example_embed.rs::tests` | the profile guard wrapping the `iframe_block(...)` push in `render_embed` |
| T4.2 | U | same | same node, `HtmlRender` profile → exactly one `RawBlock{format:"html"}` containing `<iframe` | same | the same guard, inverted-polarity failure |
| T4.3 | U | same | node with `plain_data.order = 2`, `Pandoc("docx")` → caption's first inline run contains `Demo 2:` | same | the `with_number_label` call retained on the Pandoc path |

**Revert hunks, stated exactly:**

- **T4.1** — Revert ⟨the `matches!(ctx.pipeline_profile, HtmlRender | HtmlPreview |
  RevealjsRender | RevealjsPreview)` guard around the `iframe_block(...)` push in `render_embed`⟩ →
  ⟨`assert!(!blocks.iter().any(|b| matches!(b, Block::RawBlock(r) if r.format == "html")))`⟩ RED.
- **T4.2** — Revert ⟨widen the same guard to exclude `HtmlRender` (e.g. guard on `Pandoc(_)`
  instead)⟩ → ⟨`assert_eq!(raw_html_blocks.len(), 1)` under `HtmlRender`⟩ RED. This is the
  over-application counterweight: T4.1 alone passes if the iframe is dropped for *every* profile.
- **T4.3** — Revert ⟨short-circuit `render_embed` for `Pandoc(_)` before `with_number_label`⟩ →
  ⟨`assert!(caption_text.contains("Demo 2:"))`⟩ RED. This is the content-loss failure mode the
  B4-exclude alternative would have had, and the reason the reclassification exists.

### Refactor-induced vacuity check

**The B4→format-parameterized-B1 reclassification (commit `a684cfac0`).** The expected value moved
from "excluded, so nothing is emitted" to "included, emits everything but the iframe." These two
states are **indistinguishable on an absence-only assertion**: "no iframe in docx output" is true
under both. The discriminator is T4.3 (`Demo 2:` label present) and T4.1's positive half (snippet
blocks present) — without those two, the whole reclassification is untested and a regression back
to wholesale exclusion would be invisible. Any implementer who writes only "no `RawBlock("html")`"
has written a test that passes for the classification the plan explicitly rejected.

---

## Task 5: Split `FootnotesTransform` into its B1 and B2/B4 halves

**Scope.** Make the Pandoc leg carry footnotes as native Pandoc `Note` inlines (pandoc's own
writers number and place them per format), while the HTML/reveal legs keep today's
`Span#fnrefN` + `Div#footnotes` chrome byte-identically.

**Files.** `crates/quarto-core/src/transforms/footnotes.rs` (1223 lines), read in full for this
document:
- `name()` → `"footnotes"` (`:97-99`); `phase()` → `Normalization` (`:101-103`).
- `transform` (`:105-141`): early-returns for `reference-location: block|section` (`:109-114`);
  `collect_note_definitions` (`:118`, impl `:235-291`) **removes** `NoteDefinitionPara`/
  `NoteDefinitionFencedBlock` from the AST into a `HashMap<String, NoteContent>`;
  `process_blocks` (`:125`); appends `create_footnotes_section` (`:131-132`).
- `process_inline` (`:380-466`) — the destructive core: `Inline::Note` (`:382-389`),
  `Inline::NoteReference` (`:390-396`) and the pampa-lowered
  `Span.quarto-note-reference` form (`:425-451`) are each replaced with
  `create_footnote_ref(...)`.
- `create_footnote_ref` (`:473-514`) builds `Span{attr:(fnrefN,…)}[Superscript[Link{class:
  "footnote-ref", role: "doc-noteref", target: "#fnN"}]]`.
- `create_footnotes_section` (`:527+`) builds `Div{id:"footnotes", classes:["footnotes","section"]}`
  with an embedded `<hr>` and an `OrderedList`; `create_footnote_item` (`:567+`) appends the
  `footnote-back` backlink.

**Prerequisite — and the reason this task carried Finding F1.** The plan's In-scope text describes
this as *"B1 half (`NoteRef`+`Def` → native Pandoc `Note`) stays in the core; HTML
`<section>`+backlinks half is excluded for `Pandoc` the same way"* — i.e. via the name exclude-list.
The direct read says that is not available as written:
- The transform has **one** `name()` (`"footnotes"`), so `retain_excluding` cannot exclude half of
  it. The exclude-list mechanism alone cannot express the split.
- There is **no existing B1 half to relocate.** Nothing in `footnotes.rs` ever produces an
  `Inline::Note`; `Inline::Note` is a *parser* output that this transform *destroys* at `:384-388`.
  The `NoteReference`→`Note` reconstruction the design doc's §6 SPLIT row describes
  ("B1: `NoteRef`+`Def` → native Pandoc `Note`") must be **written**, not carved out. Its inputs
  are all present (`definitions: HashMap<String, NoteContent>`, with `NoteContent::Inlines` needing
  a `Paragraph`/`Plain` wrap to become `Note`'s `Blocks`), so the work is small and local — but it
  is new behavior, and the seam for T5.1 therefore does not exist until this task creates it.

**RESOLVED 2026-09-18, decided with Gordon: two registered transforms, not an in-transform profile
branch.** This is the shape the codebase already uses twice for exactly this
semantics-then-presentation split — `callout` / `callout-resolve` and `panel-tabset` /
`panel-tabset-resolve` — and it keeps the exclude-list as the single mechanism that decides what
crosses the cut, rather than introducing a second, invisible one (a profile `match` inside a
transform that the exclude-list validators cannot see).

**The concrete shape:**

| | B1 half | B2/B4 half |
|---|---|---|
| `name()` | `"footnotes"` (**unchanged**, so the existing exclude-list entries, snapshots and test filters keep referring to the same thing) | `"footnotes-resolve"` (**new**) |
| `phase()` | `Normalization` (unchanged, `:101-103`) | `Normalization` — see the phase note below |
| Does | `collect_note_definitions` (`:235-291`), then resolves `Inline::NoteReference` (`:390-396`) and the pampa-lowered `Span.quarto-note-reference` form (`:425-451`) into `Inline::Note`; leaves an existing `Inline::Note` alone | consumes `Inline::Note` → `create_footnote_ref` (`:473-514`); appends `create_footnotes_section` (`:527+`) |
| `PANDOC_TRANSFORM_EXCLUDED` | **absent** (runs for Pandoc) | **present** (excluded for Pandoc) |
| Registered | where `FootnotesTransform` is pushed today in `build_transform_pipeline` | immediately after it, same push site |

**Three things this shape forces, stated so the implementer does not have to re-derive them:**

1. **The two halves communicate through `Inline::Note`, not through a shared field.** The B1 half's
   output *is* the B2/B4 half's input, which is what makes the split honest: the Pandoc leg stops
   after B1 and hands pandoc's own writers a native `Note`, and the HTML leg runs both and ends up
   byte-identical to today. No `RenderContext` sideband is needed, and none should be added — a
   sideband would be a second cut mechanism, which is the thing this decision avoids.
2. **`collect_note_definitions` belongs to the B1 half**, because it is what *populates* the
   definitions the B1 half needs to build `Note`s. Leaving it in the B2/B4 half would make the
   Pandoc leg drop `[^1]: definition` blocks entirely — they would survive as
   `NoteDefinitionPara`/`NoteDefinitionFencedBlock` into the wire format, an AST node type pandoc
   has no equivalent for. This is the failure mode T5.1's *positive* assertion exists to catch.
3. **The `reference-location: block|section` early return (`:109-114`) must be duplicated into
   both halves, not left in one.** It is a no-op-the-whole-feature gate; if only the B2/B4 half
   keeps it, the B1 half still rewrites `NoteReference` → `Note` under
   `reference-location: block`, which changes the HTML leg's input to the (now-skipped) chrome
   step. T5.3 asserts this under both profiles for exactly this reason.

**Phase note.** Both halves stay `Normalization`, so the phase-ordering invariant
(`test_build_transform_pipeline_phase_ordering`) is satisfied trivially and the pair does **not**
need the `Finalization` treatment the transform-pipeline-phases contract requires of
presentation transforms that consume *crossref* structure. Footnote chrome consumes no float,
caption, number or resolved `@ref` — it consumes `Inline::Note`, which the B1 half produced one
step earlier in the same phase. Worth stating because `callout-resolve`, the pattern being
mirrored, **is** `Finalization`; the symmetry is in the *name/registration* shape, not the phase.

**Design-doc impact: none required.** §6's SPLIT row reads "B1: `NoteRef`+`Def` → native Pandoc
`Note`. B2/4: HTML `<section>`+backlinks" — accurate prose for the two-transform shape, and it
never claimed the split was achievable by exclude-list alone. Left untouched deliberately (the
epic's frozen-decision sections are not edited without a reason that survives a re-read).

**Acceptance criterion.**
1. Under `Pandoc("docx")`, a document containing both `^[inline note]` and a `[^1]` /
   `[^1]: definition` pair yields, at the cut: an `Inline::Note` for each footnote, **no** block
   with `id == "footnotes"`, and **no** `Inline::Span` whose id matches `fnref\d+` (T5.1).
2. Under `HtmlRender`, the emitted shape is byte-identical to today's: `Span#fnref1 >
   Superscript > Link[target "#fn1", class "footnote-ref"]`, plus a trailing
   `Div#footnotes.footnotes.section` whose items carry `footnote-back` backlinks (T5.2). All 18
   existing `.snap` files unchanged.
3. `reference-location: block` and `reference-location: section` remain a no-op under **every**
   profile including `Pandoc(fmt)` (T5.3) — pandoc handles those itself; running the transform
   would double-handle them.
4. Both halves are registered with distinct `name()`s — `"footnotes"` (B1) and
   `"footnotes-resolve"` (B2/B4) — and **only** `"footnotes-resolve"` is on
   `PANDOC_TRANSFORM_EXCLUDED`, growing Task 2's exact list by one (T5.4 = an update to T2.1/T2.3,
   not a new test). `"footnotes"` must **not** be on that list; a test that only checks the list
   grew would pass with both halves excluded, which is failure mode 2 above.
5. Under `Pandoc("docx")` a `[^1]: definition` block leaves **no** `NoteDefinitionPara` or
   `NoteDefinitionFencedBlock` standing at the cut (T5.5) — the assertion that
   `collect_note_definitions` went to the B1 half and not the excluded one.
6. `cargo clippy -p quarto-core --all-targets -- -D warnings` +
   `cargo nextest run -p quarto-core` green.

**Byte-identity exception (final-review Important #1, recorded 2026-09-19).** The split is
byte-identical to the pre-split transform under `HtmlRender`/`RevealjsRender` **except** for
documents mixing an inline `^[...]` note with a named footnote whose id is a decimal integer (e.g.
`[^1]`). Pre-split, an inline note's synthetic id (`number.to_string()`) collided with a
same-valued named reference id, and the dedup scan silently dropped the named footnote's already-
detached definition content. The split's marker-based dedup (keyed on the named reference id only)
does not have this collision, so both footnotes now render correctly. This is a genuine, beneficial
bug fix — not a regression, and not reverted — but it is an undocumented exception to the "byte-
identical to today" bar until this note. Pinned by
`crates/quarto-core/tests/integration/footnotes_dedup.rs`'s
`inline_note_and_numerically_named_footnote_do_not_collide`.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T5.1 | I (`tests/integration/pandoc_profile_cut.rs`) | the footnotes B1 half inside the real `Pandoc("docx")` pipeline | parse a two-footnote fixture, run the `Pandoc("docx")` pipeline → `Inline::Note` count == 2; no `id == "footnotes"` block; no `fnref\d+` Span | test `SystemRuntime` | **H5a**, the B1 half's `NoteReference`→`Note` resolution |
| T5.5 | I (same file) | `collect_note_definitions`' placement in the B1 half | same fixture, `Pandoc("docx")` → zero `NoteDefinitionPara` / `NoteDefinitionFencedBlock` blocks survive | same | **H5c**, `collect_note_definitions`'s call site in the B1 half |
| T5.6 | U (`pipeline.rs::tests`) | both halves' registration | build `HtmlRender` → the name list contains `"footnotes"` **then** `"footnotes-resolve"`, adjacent and in that order; build `Pandoc("docx")` → contains `"footnotes"` and **not** `"footnotes-resolve"` | `make_test_runtime()` | **H5b**, the `footnotes-resolve` push site / its exclude-list entry |
| T5.2 | U (`footnotes.rs::tests`, extending the existing tests at `:652+`) | `FootnotesTransform` HTML path | `HtmlRender` profile, same fixture → exact `Span#fnref1[Superscript[Link…]]` shape + `Div#footnotes` classes `["footnotes","section"]` | none | `create_footnote_ref` (`:473-514`) / `create_footnotes_section` (`:527+`) |
| T5.3 | U | `FootnotesTransform::transform`'s early return | `reference-location: block`, then `section`, each under `HtmlRender` **and** `Pandoc("docx")` → AST unchanged (`Inline::Note` still standing, no `Div#footnotes`) | none | `footnotes.rs:109-114` |

**Revert hunks, stated exactly:**

The three hunks this task creates, named now so the implementer cannot substitute different ones:

- **H5a** — in the **B1** half (`name() == "footnotes"`): resolve `Inline::NoteReference`
  (`:390-396`) and the pampa-lowered `Span.quarto-note-reference` form (`:425-451`) into
  `Inline::Note` using the `definitions` map, leave an existing `Inline::Note` untouched instead
  of calling `create_footnote_ref` (`:388`), and **do not** call `create_footnotes_section`
  (`:131-132`).
- **H5b** — the new `FootnotesResolveTransform` with `name() == "footnotes-resolve"`, its
  `pipeline.push(...)` immediately after the existing `FootnotesTransform` push, and its
  `"footnotes-resolve"` entry in `PANDOC_TRANSFORM_EXCLUDED`. It owns `create_footnote_ref`
  (`:473-514`) and `create_footnotes_section` (`:527+`), moved rather than copied.
- **H5c** — `collect_note_definitions`'s call site (`:118`) staying in the **B1** half.

**Revert hunks, stated exactly:**

- **T5.1** — Revert ⟨**H5a**⟩ → ⟨`assert_eq!(count_notes(&ast), 2)`⟩ RED. This is a TDD-RED-first
  test by construction: it fails today for the correct reason (the behavior is absent), not
  because the harness is wrong.
- **T5.5** — Revert ⟨**H5c**, i.e. move `collect_note_definitions` into the B2/B4 half⟩ →
  ⟨`assert!(no_note_definition_blocks_survive(&ast))`⟩ RED, because under `Pandoc("docx")` the
  B2/B4 half never runs and the definition blocks are never collected. **This row exists because
  T5.1 alone does not catch it**: an `^[inline note]` yields an `Inline::Note` with no definition
  block at all, so a fixture's inline footnote can satisfy T5.1's count while its `[^1]`/`[^1]:`
  pair silently leaks `NoteDefinitionPara` into the wire format. The fixture must contain **both
  forms** (the acceptance criterion says so) and T5.5 is the assertion that uses the second one.
- **T5.6** — Revert ⟨**H5b**'s exclude-list entry⟩ → ⟨the `Pandoc("docx")` half of T5.6's
  assertion⟩ RED (the HTML chrome would run for docx). Separately revert ⟨H5b's push site⟩ →
  ⟨T5.6's `HtmlRender` adjacency assertion⟩ RED **and** T5.2 RED (no chrome is produced at all).
  **Note the adjacency assertion is doing real work**: if `footnotes-resolve` were registered at
  an arbitrary later position, some intervening transform could consume or rewrite the
  `Inline::Note`s before the chrome step saw them — a silent HTML-leg regression that the
  byte-identity snapshots *would* catch, but only if a snapshot happens to cover footnotes. None
  of the 18 does (verified in Task 7's vacuity note), so this assertion is the only guard.
- **T5.2** — Revert ⟨any edit to `create_footnote_ref`, e.g. drop the `Superscript` wrapper at
  `footnotes.rs:507-510`⟩ → ⟨the exact-shape `assert!(matches!(…))` chain⟩ RED.
- **T5.3** — Revert ⟨delete the `matches!(reference_location, Block | Section)` early return at
  `footnotes.rs:109-114`⟩ → ⟨`assert!(ast_unchanged)` for `reference-location: block`⟩ RED.

### Refactor-induced vacuity check

**No expected value changed for footnotes across P1's four review rounds** — the checklist item
("Read `footnotes.rs` directly; confirm the split below is as internally decoupled as it looks")
has been open since the 2026-08-20 freeze and is named by the epic doc as *"One item remains
genuinely open."* So the vacuity risk here is not a collapsed discriminator; it is the opposite —
**a test written against the design doc's §6 SPLIT row rather than against the code.** Specifically:

- A test asserting "`footnotes` is on `PANDOC_TRANSFORM_EXCLUDED`" would pass while dropping every
  footnote from every docx entirely (definitions are *removed* from the AST at
  `collect_note_definitions`, `:239-254`, but only if the transform runs; if the whole transform is
  excluded, the `NoteDefinitionPara` blocks survive into the wire format as an AST node type
  pandoc has no equivalent for). That is why T5.1 asserts the **positive** `Inline::Note` count,
  not the transform's exclusion.
- A test asserting only "no `Div#footnotes` in the Pandoc output" is satisfied by *not running the
  transform at all* — the collapsed-discriminator trap in its purest form. T5.1's `Inline::Note`
  count == 2 is the assertion that distinguishes "split correctly" from "excluded wholesale."

---

## Task 6: `Pandoc`-kind *stage*-level exclude-list + the hidden-HTML-stage audit

**Scope.** Add the stage-level list (a separate mechanism from Task 2's transform list), its
"names exist" validator, and discharge the plan's "verify no macro `PipelineStage` carries a hidden
HTML assumption" item by freezing the surviving stage list.

**Files.**
- `crates/quarto-core/src/pipeline.rs:270` (`build_html_pipeline_stages_with_options`).
- `crates/quarto-core/src/pipeline.rs:395` — `Q2_PREVIEW_STAGE_EXCLUDED: &[&str] = &["math-js",
  "render-html-body", "apply-template"]` (verified verbatim; the new const's naming/convention
  model) and `:427` (`stages.retain(|s| !Q2_PREVIEW_STAGE_EXCLUDED.contains(&s.name()))`).
- `crates/quarto-core/src/pipeline.rs:4007` (`q2_preview_stage_excluded_names_exist_in_html_pipeline`)
  — the validator to mirror.
- `crates/quarto-core/src/pipeline.rs:2157-2211` — the positional stage-name assertions, which
  independently confirm all eight target names: `compile-theme-css` (`stages[12]`, `:2182`),
  `bootstrap-js` (13, `:2185`), `clipboard-js` (14, `:2190`), `tabsets-js` (15, `:2194`),
  `code-highlight` (21, `:2204`), `math-js` (22, `:2209`), `render-html-body` (23, `:2210`),
  `apply-template` (24, `:2211`).
- `crates/quarto-core/src/stage/stages/attribution_generate.rs:61-63` (the stage's `name()`;
  the plan's `:67` citation drifted by 4 lines — see F7).

**The list** (`name()` strings, per the plan's round-4 correction (1)):
`&["compile-theme-css", "bootstrap-js", "clipboard-js", "tabsets-js", "code-highlight",
"math-js", "render-html-body", "apply-template"]`.

**Joint ownership.** Per the plan's round-4 correction (3), this list is **owned jointly with P4**
(P4 Finding 3: "P4 owns … the Pandoc-leg stage list itself, since P4 is the plan that introduces
`PandocWriteStage`"). P1's half is the const + the two validators; P4's half is inserting
`PandocWriteStage` and deciding the final composition. An implementer must not treat the list as
final.

**The `attribution-generate` decision (plan round-4 correction (2)) is answered here, and the
answer differs from the plan's.** The plan says `attribution-generate` is "the `name()` of **both**
a transform (excluded above …) and a distinct stage." Verified: the *stage* is real
(`attribution_generate.rs:61-63`, `stages[16]`, `pipeline.rs:2198`), but
`AttributionGenerateTransform` is **never pushed into `build_transform_pipeline`** — grepping the
whole workspace, its only non-test references are doc comments at `pipeline.rs:672` and `:1091`
plus its `pub use` at `transforms/mod.rs:102`; it runs from inside `AstTransformsStage`. It has no
`phase()` override either, consistent with not being a pipeline member. So it cannot go on Task 2's
transform list without failing T2.1. See Finding F2. **Stage-level only** — and T6.3 pins that.

**Acceptance criterion.**
1. `PANDOC_STAGE_EXCLUDED` holds exactly the 8 names above; every one is a real stage name in
   `build_html_pipeline_stages_with_options(None)` (T6.1).
2. The surviving stage-name list for a `Pandoc("docx")` render equals an exact `&[&str]` literal
   containing none of the 8 and still containing `source-conversion`, `parse-document`,
   `metadata-merge`, `document-profile`, `unwrap-profile`, `pre-engine-sugaring`,
   `engine-execution`, `user-filters-pre`, `ast-transforms`, `user-filters-post`,
   `resource-report` (T6.2).
3. `"attribution-generate"` is a stage name and is **not** a member of
   `build_transform_pipeline(HtmlRender)` (T6.3).
4. `cargo nextest run -p quarto-core` green; the positional assertions at `pipeline.rs:2157-2211`
   are untouched (P1 changes no HTML stage ordering).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T6.1 | U | `PANDOC_STAGE_EXCLUDED` ∩ `build_html_pipeline_stages_with_options` | build the HTML stage list, collect `name()`s → `unknown.is_empty()` | none (stage construction is pure) | the const's own entries |
| T6.2 | U | the `Pandoc`-kind stage-list builder | build the `Pandoc("docx")` stage list → `assert_eq!` the exact ordered name list against a literal | none | the `retain(…PANDOC_STAGE_EXCLUDED…)` application |
| T6.3 | U | `build_transform_pipeline` + `build_html_pipeline_stages_with_options` | assert `"attribution-generate"` ∈ stage names and ∉ transform names | `make_test_runtime()` | — (an invariant test; see below) |

**Revert hunks, stated exactly:**

- **T6.1** — Revert ⟨write the const with a Rust type name instead of a `name()` string, e.g.
  `"CompileThemeCssStage"` in place of `"compile-theme-css"`⟩ → ⟨`assert!(unknown.is_empty(), …)`⟩
  RED. This is the plan's own round-4 specification gap (1), made mechanical.
- **T6.2** — Revert ⟨delete `"compile-theme-css"` from `PANDOC_STAGE_EXCLUDED`⟩ → ⟨the exact
  surviving stage-list `assert_eq!`⟩ RED. (This is the "stage stray `site_libs/*` next to a
  `.docx`" failure shape the plan invokes the 2026-04-20 `CodeHighlightStage` incident about — and
  T6.2's exact-list form catches over-exclusion too, e.g. dropping `engine-execution`, which would
  silently skip code execution for a docx render.)
- **T6.3** — Revert ⟨add `"attribution-generate"` to `PANDOC_TRANSFORM_EXCLUDED`⟩ → ⟨T2.1's
  `unknown.is_empty()`⟩ RED, and T6.3's own `∉ transform names` assertion stays GREEN, naming the
  cause. T6.3 has no revert hunk of its own because it asserts a *structural fact about the
  existing code* (a transform type that is not a pipeline member) rather than new behavior; it is
  the guard that keeps Finding F2 from being re-introduced. Flagged rather than omitted, per Check 1.

### Refactor-induced vacuity check

**No expected value changed for the stage list across rounds** other than the plan's own round-4
specification corrections, which are all *naming* corrections (type names → `name()` strings). The
live risk is the reverse of vacuity: T6.2 is the **only** assertion that a Pandoc stage list even
exists, and it can be satisfied trivially (an implementer who never wires the const into any
builder can still make the literal match by writing the literal from the unfiltered list). The
guard is that T6.2's literal must be written to **exclude** the 8 names *and* the test must build
via the real profile-selecting entry point, not by hand-filtering in the test body. Stated here so
a reviewer can check it.

---

## Task 7: Neutral-core invariant + total bucket coverage

**Scope.** Assert the plan's central invariant (no B2/B4 transform survives to the Pandoc cut) and
the totality the plan promoted to a requirement after three rounds of hand-enumeration misses
(every transform registered in `build_transform_pipeline` has exactly one bucket).

**Files.** `crates/quarto-core/src/pipeline.rs` — the new `const BUCKETS: &[(&str, Bucket)]` in the
module proper (see the note below), plus the new tests beside
`test_build_transform_pipeline_phase_ordering` at `:3829`. **F3's specification question is
resolved** (Rust const + `#[test]`, not an xtask lint rule).

**Prerequisite — resolved, no longer blocked.** The plan's checklist says "every transform
registered in `build_transform_pipeline` classified exactly once **in the design doc §6 table**."
Reconciling a Rust pipeline against a *markdown table* would be a repo-level xtask lint rule (the
pattern `error-docs-page-missing` / `ci-test-suite-unwired` establish); reconciling against a Rust
const is a plain `#[test]`. **Decided 2026-09-18 with Gordon (F3): the Rust `#[test]` against
`const BUCKETS: &[(&str, Bucket)]`, declared in `crates/quarto-core/src/pipeline.rs` beside
`PANDOC_TRANSFORM_EXCLUDED`. No lint rule; the design-doc §6 table is advisory.** T7.1/T7.2 below
were already written against this form and are unchanged by the decision. This task has no
remaining open specification question.

**One thing the implementer must get right, because the decision moves the authority.** `BUCKETS`
is now the authoritative classification, so it must be declared where the pipeline is — not in a
test module. A `#[cfg(test)]` const would make T7.2 assert a *test fixture* against the pipeline,
which is circular: whoever adds a transform would add it to both and the test would stay green
while §6 and the real classification both drifted. Declare it in the module proper.

**Acceptance criterion.**
1. Every transform in `build_transform_pipeline(HtmlRender)` appears **exactly once** in the bucket
   classification — no unclassified members, no duplicate entries, no stale entries naming a
   transform that is no longer registered (T7.2).
2. Every transform surviving in `build_transform_pipeline(Pandoc("docx"))` is classified `B1` or
   `B3` — zero `B2`, zero `B4` (T7.1).
3. `test_build_transform_pipeline_phase_ordering` (`pipeline.rs:3829`) still passes, with its loop
   extended from `["html", "revealjs"]` to cover the profile axis (T7.3) — **see the vacuity note
   below before writing this one.**
4. `cargo nextest run -p quarto-core` green.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T7.1 | U | `build_transform_pipeline` + the bucket classification | build `Pandoc("docx")` → every surviving name's bucket ∈ {B1, B3} | `make_test_runtime()` | any B2/B4 name removed from `PANDOC_TRANSFORM_EXCLUDED` |
| T7.2 | U | the bucket classification vs. `build_transform_pipeline` | build `HtmlRender`, collect names → each appears exactly once in the classification, and the classification has no entry that is not a pipeline member | same | a bucket entry deleted, duplicated, or left stale |
| T7.3 | U | `test_build_transform_pipeline_phase_ordering` (existing, `pipeline.rs:3829`) | extend the loop to the five profiles → the existing exhaustiveness (`phase() != Unclassified`) and monotonicity assertions hold | same | a transform added to the pipeline without a `phase()` override |

**Revert hunks, stated exactly:**

- **T7.1** — Revert ⟨delete `"crossref-render"` from `PANDOC_TRANSFORM_EXCLUDED`⟩ → ⟨`assert!(bad.is_empty(),
  "B2/B4 transforms survive to the Pandoc cut: {bad:?}")`⟩ RED. `crossref-render` is the
  load-bearing case: if it ran before the cut, every numbered `CustomNode` would already be
  destroyed into raw HTML `Figure`/`Div` structure and P5's shim would have nothing to route.
- **T7.2** — Revert ⟨add a new `pipeline.push(Box::new(SomeNewTransform::new()))` to
  `build_transform_pipeline` without adding its bucket entry⟩ → ⟨`assert!(unbucketed.is_empty(), …)`⟩
  RED. This is the test whose absence let `breadcrumbs-render`, `quarto-nav-js`,
  `repo-actions-render`, `attribution-viewer`, then `secondary-nav-render`, `listing-feed-stage`,
  `listing-feed-link`, then `config-markdown`, `reference-link-diagnostics`, `draft-alert`,
  `format-css`, `responsive-image`, `llms-capture` each go unclassified across three rounds.
- **T7.3** — Revert ⟨remove the `fn phase()` override from `SectionizeTransform`⟩ → ⟨the existing
  `assert!(unclassified.is_empty(), …)` inside `test_build_transform_pipeline_phase_ordering`
  (`pipeline.rs:3829`, exhaustiveness block beginning at the `// (1) Exhaustiveness` comment)⟩ RED.

### Refactor-induced vacuity check

**`test_build_transform_pipeline_phase_ordering`'s monotonicity assertion goes vacuous for every
Pandoc profile, by construction.** Read at `pipeline.rs:3829-3880`: it loops
`for format in ["html", "revealjs"]` (`pipeline.rs:3834`), builds the pipeline, and asserts (1) no member is
`TransformPhase::Unclassified` and (2) `prev_phase <= next_phase` for every adjacent pair. After the
refactor, a `Pandoc(fmt)` pipeline is the `HtmlRender` pipeline with names removed
(`retain_excluding`) — and **a subsequence of a non-decreasing sequence is always non-decreasing.**
So extending the loop to `Pandoc("docx")` adds a passing assertion that cannot fail for any reason
the Pandoc work could introduce. It is worth extending anyway, but only as *shape/gating* (it keeps
covering assertion (1), exhaustiveness, which is genuinely load-bearing if P4/P7 ever splice a
Pandoc-only transform in). **The discriminators for the Pandoc profile are T2.3's exact surviving
name list and T7.1's bucket check — not monotonicity.** An implementer who extends the loop and
stops there has added coverage that reads as an invariant and is not one; this must be said in the
test's own doc comment.

**The byte-identity bar's discriminating surface.** P1's review bar is "HTML/reveal/preview
byte-identity," and the natural acceptance criterion is "zero `.snap` changes." Verified: there are
exactly 18 `.snap` files, all under `crates/quarto-core/tests/integration/snapshots/`, all
fragment-level (13 × `title_block_pipeline`, 4 × `listing_pipeline`/`llms_txt`, 1 ×
`attribution_baseline_snapshot`). **For `HtmlRender` the refactor produces the same pipeline, so
these snapshots are identical before and after by construction — they cannot discriminate a
Pandoc-profile regression at all.** They are a necessary *no-regression* gate on the HTML leg and
nothing more. The claim "byte-identical" is carried by Task 8's corpus diff plus T1.4/T1.5/T2.3/T2.5's
exact name lists; anyone who reads "zero snapshot changes" as evidence the profile work is correct
has read a tautology.

---

## Task 8: Byte-identity corpus + capture/diff harness

**Scope.** Give P1's stated hard no-regression bar a backing mechanism: name a corpus, capture
whole-document output before the refactor, capture after, diff.

**Files.** A new dev-only capture/diff entry point (the plan does not name its home; the repo's
convention for dev-only capture is `crates/xtask/src/` — cf. P7's proposed
`cargo xtask capture-pandoc-goldens`). Corpus per the plan: `docs/` plus the crossref fixture set
exercised by `crates/quarto-core/tests/integration/crossref_fixtures.rs`. The HTML site build
invocation is `cargo run --bin q2 -- render docs/` (per CLAUDE.md — never Q1's `quarto render`).

**Prerequisite.** Requires Tasks 1-7 landed (this is the last gate). Requires **no** Q1 binary and
**no** `external-sources/` — distinguishing it from P7's golden captures, which do need both.

**Acceptance criterion.**
1. The harness renders the named corpus at the pre-refactor commit and at HEAD and reports a
   byte-level diff; the diff is empty (T8.1). **Scope correction (final-review Important #2,
   2026-09-19): this proves byte-identity for the `HtmlRender` leg of the `docs/` corpus
   specifically** — `docs/` contains zero real `format: revealjs` documents (every occurrence is
   inside a fenced code block) and exactly one live footnote (an inline `^[...]`, no reference-style
   `[^a]`/`[^a]:` pair). The harness does **not** itself exercise revealjs, the preview AST leg, or
   footnote-reference-id dedup; those are covered instead by `revealjs_features.rs` (revealjs
   end-to-end render + per-slide footnote coalescing), `t1_5`/`pipeline.rs` (reveal transform
   name-list pinning), `t2_5`/`pipeline.rs` (preview transform name-list pinning), and
   `footnotes_dedup.rs` (footnote-reference dedup, at the unit/integration level, not corpus level).
   Extending the corpus (or adding a sibling fixture corpus) to close this gap directly is valid
   future follow-up work, out of scope here.
2. It **fails loudly** (non-zero exit, named missing path) when either capture directory is absent
   — a silently-skipping harness is a vacuous harness (T8.2).
3. The existing committed HTML-leg tests are green with zero `.snap` updates:
   `cargo nextest run -p quarto-core` including
   `tests/integration/render_to_html_captures.rs`, `preview_render_css_parity.rs`,
   `title_block_pipeline.rs`, `tabset_pipeline.rs`, `crossref_fixtures.rs` (T8.3).
4. `cargo nextest run --workspace` green, reported as a delta against the live baseline (not a
   figure copied from a document), and `cargo xtask verify` (**full**, not `--skip-hub-build` —
   `RenderContext` gained a field) green.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T8.1 | G (dev-only; two builds of `q2`, no Q1, no `external-sources/`) | the real `q2 render` binary over `docs/` + the crossref fixtures | render at pre-refactor commit → dir A; render at HEAD → dir B; byte-diff A vs B → empty | none (this is the point — the real binary, real files) | any behavioral change in the `HtmlRender` path |
| T8.2 | U (in the harness's own crate) | the harness's precondition check | invoke with a nonexistent capture dir → `Err` naming the path, non-zero exit | filesystem via `tempfile` | the precondition check itself |
| T8.3 | I (existing, unchanged) | `render_to_file` / `render_qmd_to_html` / `render_qmd_to_preview_ast` | the existing integration suite → green, zero `.snap` writes | as already written | any `HtmlRender`-path change |

**Revert hunks, stated exactly:**

- **T8.1** — Revert ⟨drop `pipeline.push(Box::new(SectionizeTransform::new()))` from the
  `HtmlRender` arm at `pipeline.rs:1276`⟩ → ⟨the corpus byte-diff is non-empty (every
  `<section id=…>` wrapper disappears from `docs/`)⟩ RED. Stated as a *deliberate* revert because
  T8.1's real job is to catch an *accidental* one.
- **T8.2** — Revert ⟨replace the harness's missing-capture-dir `bail!` with `return Ok(())`⟩ →
  ⟨`assert!(result.is_err())` and the exit-code assertion⟩ RED. This is the "if you spec a skip,
  spec how its skipping is itself visible" requirement: the harness has **no** skip path; absence
  of a capture is a failure, not a skip.
- **T8.3** — Revert ⟨any edit to `create_footnote_ref` (`footnotes.rs:473-514`) or
  `TitleBlockTransform`⟩ → ⟨`integration__title_block_pipeline__title_block_simple.snap` mismatch⟩
  RED.

### Refactor-induced vacuity check

See Task 7's second note — the snapshot leg is tautological for `HtmlRender` and must not be cited
as evidence about the Pandoc profile. One further trap specific to this task: **a corpus diff taken
between two builds of the *same* commit is empty for trivial reasons.** The harness must record
which commit each capture came from and refuse to compare a directory against itself; otherwise the
"empty diff" result survives the harness being wired to the wrong checkout. Bound by T8.2's
precondition test (extend it to assert the two capture manifests carry different commit SHAs).

---

## Task 9: Design-doc + plan bookkeeping (grouped — one task, not five)

**Scope.** Land every documentation-only correction P1 owns. All five sub-items are the same shape
(edit a frozen-decision section to match a decision already made elsewhere), so they are one task.

**Files.** `claude-notes/designs/pandoc-hybrid-architecture.md`,
`claude-notes/plans/2026-08-20-pandoc-hybrid-P1-neutral-core.md`.

**Sub-items, each verified against the current file:**
1. **§5, lines 176-183** — replace the four-variant shorthand
   `{HtmlFull, Revealjs, Preview, Pandoc(fmt)}` and its "Superseded — flagged 2026-09-17, not yet
   landed here" marker with the five-variant, two-axis shape
   `{HtmlRender, HtmlPreview, RevealjsRender, RevealjsPreview, Pandoc(fmt)}`. This is the plan's
   last open checklist line.
2. **§6 Navigation row, line 202** — its "Added 2026-09-17" note lists only `breadcrumbs-render`,
   `quarto-nav-js`, `repo-actions-render`. The three found in round 4 —
   `secondary-nav-render`, `listing-feed-stage`, `listing-feed-link` — reached P1's exclude-list
   but **never reached this row**. Add them.
3. **§6 — the six transforms with no row at all**, per P1's round-4 correction:
   `config-markdown` (Normalization), `reference-link-diagnostics` (Normalization),
   `draft-alert` (Normalization), `format-css` (Normalization), `responsive-image` (Finalization),
   `llms-capture` (Finalization). All six verified as real `build_transform_pipeline` members with
   real `phase()` overrides.
4. **§6 line 211-216** — "tracked as a P1 recommendation, **not yet a checklist item**" is stale:
   P1's checklist now carries the total-coverage item. **F3 is resolved (2026-09-18, Gordon), so
   the replacement sentence is now determined:** say that the authoritative classification is
   `const BUCKETS` in `crates/quarto-core/src/pipeline.rs`, that **T7.2 is the mechanical guard
   over it**, and that **this table is advisory documentation of intent with no mechanical guard of
   its own**. That last clause is the important one — it is the accepted cost of the decision, and
   without it a future reader will keep treating the table as the enforced artifact (which is how
   it drifted three times already).
5. **Plan checklist reconciliation** (per the user-global rule about reconciling a plan's checklist
   against reality before hand-off): `- [ ] Add ConditionalContentTransform to design doc §6 bucket
   table as B1` is **already done** — the row exists at design §6 line 206, added 2026-09-17. Check
   it off. Likewise re-verify the three `[x]` items (`appendix.rs`/`link_rewrite.rs` read;
   §6 CalloutResolve/AppendixStructure/ExampleEmbedRender fixes; the wasm32 decision) — all three
   confirmed landed in the current design doc text.
6. **§6 `LinkRewrite` row, line 208, read `B3?`** while this plan's own "Produces for P7" section
   asserts "ResourceCollector, LinkRewrite, Appendix — **all confirmed in, not conditional**."
   **RESOLVED and LANDED 2026-09-18 with Gordon's explicit sign-off (F6): the cell now reads plain
   `B3`**, with the resolution noted in the cell and a full entry in the design doc's Amendment
   log. Nothing left for this task to do on this sub-item except *not* re-open it — the `?` is
   gone, and the epic's convention that frozen sections need explicit sign-off was honored rather
   than routed around.

**Acceptance criterion.** `cargo xtask lint` green; the design doc contains no remaining
"not yet landed here" marker in §5; §6's bucket table names every one of the transforms registered
in `build_transform_pipeline`; P1's checklist has no `- [ ]` item whose work is already done and no
`- [x]` item whose work is not.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| — | — | — | **No automated seam.** Documentation-only. | — | — |

`accepted-untested: Task 9 edits only markdown in claude-notes/. The one reconciliation that could
be mechanized — design §6's table vs. build_transform_pipeline's membership — is deliberately NOT
mechanized: F3 (resolved 2026-09-18) made const BUCKETS authoritative and the §6 table advisory, so
T7.2 guards the Rust classification and nothing guards the table. Adding a doc-parsing check here
would re-authoritize the table against that decision. The table's accuracy is this task's
bookkeeping responsibility, re-checked when §6 is next touched.`

---

## Missing-test pass

Behaviour reachable in P1 with **no** test above, reasoned across the whole change rather than
per-task. Each gets a bound seam or an explicit `accepted-untested:` / `seam deferred:` verdict.
Silent omission would read as "covered."

### Verdicts the prompt requires explicitly

1. **Self-gating transforms (the plan's own Finding, 7 gaters not 8).** Re-verified all seven
   against real code: `mermaid.rs:175` (`!is_html_based()` → return), `title_block.rs:97`
   (`should_add_h1(.., ctx.format.is_html())` → return, helper at `:65-75`),
   `panel_tabset.rs:109-114`, `draft_alert.rs:126` (gate) with its predicate at `:142`,
   `format_css.rs:93`, `responsive_image.rs:152`, and `toc_generate.rs:85`. Plus
   `crossref_render.rs:88` which is **not** a gate (`html_float_dom: …is_html_based()` is a
   `FloatState` config field, no early return) — the plan's correction is right.
   - **Bound:** `panel-tabset` → T3.1 (widened, positive assertion) + T3.2 (other gate terms
     intact).
   - **Bound (by exclusion, not by gate):** `mermaid-render`, `title-block`, `draft-alert`,
     `format-css`, `responsive-image` → T2.3's exact surviving-name list. Their self-gates never
     execute for a Pandoc profile, so there is nothing else to assert in P1.
   - **`toc_generate.rs:85` — a drift correction, not a gate.** The plan's table calls it a gate
     shape (`identifier == FormatIdentifier::Html`). Read directly, it is inside
     `fn toc_title_term(ctx)` selecting between the `toc-title-website` and `toc-title-document`
     language terms — no early return. `toc-generate`'s real early return, if any, is elsewhere in
     the file. Moot for the decision (`toc-generate` is excluded either way), recorded in F7.
   - `accepted-untested: the "a self-gated transform left on the exclude-list is invisible to a
     membership test" case. Correct, and unfixable in P1 — with the transform excluded there is no
     runtime surface on which its gate's state is observable. The compensating control is T7.2's
     total bucket coverage (a transform cannot be silently unclassified) plus T2.2's phase()
     query (a Navigation member cannot be silently unexcluded). Neither observes a gate; both
     observe the list, which is the layer P1 actually owns.`
   - `seam deferred until P7's per-format invocation builder: the output-level consequence of the
     three not-widened gates — no stray .css / .docx-adjacent site_libs artifacts. P1 has no
     Pandoc output directory to inspect.`

2. **Design §6's "coverage is not yet asserted as total" admission (lines 211-216).** **Verdict:
   a P1 seam, not `accepted-untested`** — it is Task 7's T7.2, and the plan already promoted it
   from a recommendation to a requirement. **Mechanism resolved 2026-09-18 (F3, decided with
   Gordon): a Rust `#[test]` over `const BUCKETS` in `pipeline.rs`, not an xtask lint rule over
   the markdown. Bound, no longer deferred.** Note the verdict has shifted slightly in the process:
   T7.2 now guards the **Rust classification's** totality against `build_transform_pipeline`, while
   design §6's table — the thing lines 211-216 are actually about — becomes advisory and is
   guarded by **nothing**. So the honest split is: *pipeline-vs-classification* bound (T7.2);
   *classification-vs-§6-table* **`accepted-untested` by decision**, with Task 9 sub-item 4 owning
   the prose that says so. Recorded this way rather than letting T7.2's green be read as evidence
   the table is correct.

3. **The Footnotes B1/B2-4 split's decoupling premise** (the epic doc's "One item remains genuinely
   open"). **Verdict: verified by direct read, and the premise does not hold as stated** — see
   Task 5's Prerequisite and Finding F1. The *data* is decoupled enough (a `definitions` map, a
   `NoteContent` enum, a collector) but the *transform* is not: one `name()`, and the B1 behavior
   (`→ Inline::Note`) exists nowhere in the file. **Shape resolved 2026-09-18 (F1, decided with
   Gordon): two registered transforms**, so the split is now expressible through the exclude-list
   mechanism after all. Bound seams: T5.1 → hunk **H5a**, plus **T5.5** (`collect_note_definitions`
   stayed in the B1 half — the leak T5.1 cannot see) and **T5.6** (the two halves are registered
   adjacently and only the resolve half is excluded). **Not
   deferred, but Gordon must choose the split's shape (two registered transforms vs. an
   in-transform profile branch) before an implementer starts.**

4. **`test_build_transform_pipeline_phase_ordering` (`pipeline.rs:3829`) — does the profile refactor
   preserve it, and does it still discriminate?** Preserved: yes, mechanically — its only coupling
   is the `build_transform_pipeline(…, format.to_string(), …)` call signature, which Task 1
   changes, so the test must be updated to pass a profile (T7.3). Still discriminates: **only for
   assertion (1)**, exhaustiveness. Assertion (2), monotonicity, is vacuous for every Pandoc
   profile — a subsequence of a monotone sequence is monotone. Written up in full under Task 7's
   vacuity check. **Verdict: bound (T7.3), with the vacuity explicitly documented in the test's own
   doc comment so a future reader does not over-trust it.**

### Further load-bearing branches and contracts found in this pass

5. **`q2-sandboxed-preview` and `q2-debug`.** `builtin_pseudo_format` (`format.rs:110-131`) has
   **five** pseudo-formats, not the two P1 names: `q2-slides` and `q2-preview` and
   `q2-sandboxed-preview` all carry `Some("preview")`; `q2-debug` carries `None`. A `from_format`
   that omits `q2-sandboxed-preview` silently routes the sandboxed-preview port (bd-jgpz4hfq)
   through the full HTML pipeline. **Bound: T1.1.** See F4.
6. **`Pandoc(fmt)` for formats nobody asked for.** `from_format("gfm")`/`("pdf")`/`("typst")`/
   `("epub")` must derive `Pandoc(fmt)` from "not native", not from a docx/pptx allowlist — an
   allowlist would leave `gfm` mapping to `HtmlRender` and running the whole HTML tail.
   **Bound: T1.1** (the `"gfm" → Pandoc("gfm")` row). The `render.rs:680-685` `is_native()` gate
   still rejects all of them at the CLI until P7 relaxes it, so this is shape, not reachability.
7. **`retain_excluding`'s silent unknown-name drop** (`transform.rs:230-233`). A renamed transform
   silently un-excludes itself from both lists. **Bound: T2.1 and T6.1** (the two "names exist"
   validators). This is the single highest-value pair of tests in P1 relative to their cost.
8. **`reference-location: block|section` no-op** (`footnotes.rs:109-114`). A safety branch that
   must survive the split under the Pandoc profile too. **Bound: T5.3.**
9. **The `is_minimal_html` and `is_revealjs_target` terms of `panel_tabset.rs`'s gate.**
   **Bound: T3.2** (minimal-HTML term). The `is_revealjs_target` term: `accepted-untested: reveal's
   tabset story is explicitly a future strand (bd-y5j0m776) and the term is unchanged by this task;
   T1.5's RevealjsRender name list already pins that panel-tabset is registered for reveal, so a
   widening that accidentally enabled it would surface as a reveal-leg output change in T8.1.`
10. **`RenderContext`'s 266 construction sites and the wasm leg.** The field addition compiles or
    it does not — the build is the test. `accepted-untested: adding a field with an in-new()
    derivation is compiler-enforced; a test asserting "no call site changed" is not expressible.
    The gate is criterion 6 of Task 1: full cargo xtask verify (not --skip-hub-build), per the
    plan's Seam item 4 and CLAUDE.md's wasm warning.`
11. **`PipelineProfile::Pandoc(fmt)` under `wasm32`** (the plan's Seam item 5: not `cfg`-gated).
    `accepted-untested: there is no wasm32 test harness in this repo for quarto-core, and the
    decision's content is precisely "nothing is conditional," so there is no branch to exercise.
    The gate is the wasm build inside full cargo xtask verify.`
12. **The docx duplicate-title outcome.** `seam deferred until P7's per-format invocation builder
    (the plan's checklist already says "coordinate with P7").` P1's proxy is T2.3
    (`title-block` absent from the Pandoc pipeline) plus the pandoc-3.8.1 empirical check recorded
    under Task 2.
13. **"No macro `PipelineStage` is implicitly HTML-shaped."** Bound as *freezing the surviving
    list* (T6.2), which is the only thing P1 can assert. `seam deferred until P4's PandocWriteStage:
    whether a surviving stage errors or misbehaves when the pipeline terminates in a Pandoc write
    rather than ApplyTemplateStage cannot be observed until a Pandoc terminal stage exists.`
14. **The wire-format cut itself.** P1 "produces the neutral-core cut AST" but P2 owns the schema.
    `seam deferred until P2's wire-format schema v1: P1 asserts the AST's *shape* at the cut
    (T3.1's Tabset CustomNode, T5.1's Inline::Note) but not its serialized form.`
15. **A durable, committed whole-document tripwire.** Task 8's harness is a one-shot before/after
    diff; once P1 lands, nothing committed guards the next refactor at whole-document granularity
    (the 18 `.snap`s are all fragment-level). `accepted-untested: committing whole-document
    snapshots is not on P1's checklist and would be new scope. Recorded so the gap is visible
    rather than assumed closed; a natural follow-on, and the kind of thing that belongs in a
    braid strand rather than this plan.`

---

## Findings for Gordon

Six substantive findings plus a citation-drift list. None of these relitigate a frozen decision;
each is a place where a test cannot be written the way the plan describes, or where a bookkeeping
cell has no owner.

**Status of these findings (updated 2026-09-18).** The three that needed a decision — **F1**
(Footnotes split shape), **F3** (bucket-coverage mechanism) and **F6** (§6's `LinkRewrite` cell) —
**were all decided by Gordon on 2026-09-18 and are applied**: two registered transforms, a Rust
`#[test]` over `const BUCKETS`, and a confirmed `B3` landed in the design doc with explicit
sign-off. **F2, F4 and F5 never needed a decision** — each is a verified correction with one
behavior-preserving answer, already written into the tasks (F2: stage-level list only, pinned by
T6.3; F4: `q2-sandboxed-preview` → `HtmlPreview` and family derived from
`is_revealjs_target(target_format)`, bound by T1.1; F5: the third, non-compiler-forced `"pptx"`
arm, bound by T1.2). **F7 is a drift list, informational.** **Nothing on P1 is open**, and no task
in this file is parked behind a finding.

**F1 — The Footnotes split's B1 half does not exist, and the exclude-list mechanism cannot express
the split.** `crates/quarto-core/src/transforms/footnotes.rs` was read in full. `FootnotesTransform`
has one `name()` (`"footnotes"`, `:97-99`), so `retain_excluding` cannot exclude half of it — the
plan's phrasing ("HTML `<section>`+backlinks half is excluded for `Pandoc` the same way") is not
available. More importantly, nothing in the file ever *produces* an `Inline::Note`: the transform
**consumes** `Inline::Note` at `:382-389` and replaces it with `create_footnote_ref`'s
`Span#fnrefN[Superscript[Link]]` (`:473-514`), resolves `Inline::NoteReference` (`:390-396`) and
the pampa-lowered `Span.quarto-note-reference` form (`:425-451`) the same way, and appends
`create_footnotes_section` (`:131-132`). So the design doc §6 SPLIT row's "B1: `NoteRef`+`Def` →
native Pandoc `Note`" is **new behavior to be written**, not an existing half to relocate. The
inputs are all there (`definitions: HashMap<String, NoteContent>`; `NoteContent::Inlines` needs a
`Paragraph` wrap to satisfy `Note`'s `Blocks`), so this is small. **This is the epic doc's own
"one item remains genuinely open"; it is now closed as a fact and (see below) as a decision.**

**RESOLVED 2026-09-18, decided with Gordon: two registered transforms** — `"footnotes"` (B1) and
`"footnotes-resolve"` (B2/B4) — **not** one transform with an internal
`PipelineProfile::Pandoc(_)` branch. Rationale as Gordon framed it: it is the shape the codebase
already uses twice for this exact semantics-then-presentation split
(`callout`/`callout-resolve`, `panel-tabset`/`panel-tabset-resolve`), and it keeps the exclude-list
as the *single* mechanism deciding what crosses the cut. A profile `match` inside a transform would
be a second cut mechanism that Task 2's exclude-list validators cannot see.

Task 5 is rewritten against that shape: the two halves are tabulated (name, phase, what each does,
exclude-list membership, registration point), three hunks are named (**H5a** the B1 resolution,
**H5b** the new transform + its push site + its exclude-list entry, **H5c** `collect_note_definitions`
staying in B1), and T5.1's "seam deferred" is replaced by a binding to H5a.

Binding it surfaced two things worth recording, because neither was in the original F1 text and
both are silent-failure shaped:
- **`collect_note_definitions` must go to the B1 half, and T5.1 cannot detect it if it doesn't.**
  An `^[inline note]` produces an `Inline::Note` with no definition block, so a fixture's inline
  footnote satisfies T5.1's count while its `[^1]`/`[^1]:` pair leaks `NoteDefinitionPara` into the
  wire format — a node type pandoc has no equivalent for. Added **T5.5** and made the two-form
  fixture an explicit acceptance item.
- **`footnotes-resolve`'s registration must be *adjacent* to `footnotes`, and nothing else guards
  that.** If it were registered at an arbitrary later position, an intervening transform could
  consume or rewrite the `Inline::Note`s before the chrome step saw them. The byte-identity
  snapshots would catch it only if one covered footnotes, and **none of the 18 does** (verified in
  Task 7's vacuity note). Added **T5.6** with an adjacency+order assertion.

Also checked, so it is not left implicit: both halves stay `Normalization`, so the phase-ordering
invariant is satisfied trivially. Note the asymmetry with the pattern being copied —
`callout-resolve` is `Finalization` — because footnote chrome consumes `Inline::Note`, not crossref
structure. The symmetry is in the name/registration shape, not the phase.

**Design-doc impact: none.** §6's SPLIT row ("B1: `NoteRef`+`Def` → native Pandoc `Note`. B2/4:
HTML `<section>`+backlinks") is accurate prose for the two-transform shape and never claimed the
split was reachable by exclude-list alone, so it is left untouched — per the epic's convention that
frozen-decision sections are not edited without a reason that survives a re-read.

**F2 — `attribution-generate` cannot go on the transform exclude-list; it would fail the
validator the same plan requires.** The plan's stage-exclude-list item says `attribution-generate`
is "the `name()` of **both** a transform (excluded above …) and a distinct stage." The stage is real
(`stage/stages/attribution_generate.rs:61-63`, `stages[16]`, asserted at `pipeline.rs:2198`). The
transform is **not a member of `build_transform_pipeline`**: grepping the whole workspace,
`AttributionGenerateTransform`'s only non-test references are doc comments at `pipeline.rs:672` and
`:1091` and its `pub use` at `transforms/mod.rs:102`; it runs from inside `AstTransformsStage`, and
it has no `phase()` override (consistent with not being a pipeline member — the phase-ordering test
would otherwise fail its exhaustiveness check). So putting `"attribution-generate"` on
`PANDOC_TRANSFORM_EXCLUDED` makes T2.1 (the "names exist" validator the plan itself requires) go
RED. **Proposed resolution: stage-level list only; `attribution-render` and `attribution-viewer`
stay on the transform list (both verified registered, `pipeline.rs:1541` and `:1552`).** T6.3 is
specified to pin this so it cannot be re-introduced.

**F3 — The total-bucket-coverage test's mechanism is undecided, and the test cannot be written
until it is.** P1's checklist says "every transform registered in `build_transform_pipeline`
classified exactly once **in the design doc §6 table**." Reconciling Rust against a markdown table
is a repo-level `cargo xtask lint` rule (the pattern `error-docs-page-missing` and
`ci-test-suite-unwired` establish) — a new lint rule in `crates/xtask/src/lint/`, not a `#[test]`.
Reconciling against a Rust `const BUCKETS: &[(&str, Bucket)]` is a plain `#[test]` in `pipeline.rs`
but makes the design-doc table advisory rather than authoritative. Both are defensible; they are
different amounts of work in different crates. I specified T7.1/T7.2 against the Rust-const form
and flagged it rather than deciding.

**RESOLVED 2026-09-18, decided with Gordon: the Rust `#[test]` against
`const BUCKETS: &[(&str, Bucket)]`, in `crates/quarto-core/src/pipeline.rs`. The design doc's §6
table becomes advisory, not authoritative.** No `cargo xtask lint` rule.

**Task 7 and Task 9 sub-item 4 are unblocked; nothing else changes.** T7.1 and T7.2 were already
written against this option, and re-reading them against the final decision confirms they need
**no edits** — T7.1 asserts every surviving `Pandoc("docx")` name's bucket ∈ {B1, B3}; T7.2 asserts
exact-once coverage in both directions against `build_transform_pipeline`; and both name reverts
that are mutations of *Rust*, not of markdown (`crossref-render` removed from the exclude-list; a
pipeline push added without a bucket entry). Task 7's "Prerequisite / blocked sub-item" paragraph
and its `(conditional on F3's resolution)` qualifier were the only stale parts, and are corrected.

**One consequence worth stating plainly, since it is the cost of this choice.** The design-doc §6
table is now *documentation of intent* with no mechanical guard — nothing fails if it drifts from
`BUCKETS`. That is a real, accepted loss: §6's own text already admits the table "is not yet
asserted as total," and it has in fact drifted three times (the round-4 additions). What the
decision buys is that the *pipeline* can no longer drift unnoticed, which is the failure mode that
actually shipped bugs. Task 9 sub-item 4 now rewrites §6's "tracked as a P1 recommendation, not yet
a checklist item" sentence to say exactly this — table advisory, `BUCKETS` authoritative, T7.2 the
guard — so a future reader does not mistake the table for the enforced artifact. Keeping the two in
sync stays Task 9's bookkeeping, not a lint rule's job.

**F4 — P1's `PipelineProfile` variant enumeration omits `q2-sandboxed-preview`, and `q2-slides`'
base format is `html`, not `revealjs`.** `builtin_pseudo_format` (`format.rs:110-131`) has five
entries: `q2-slides → ("html", Some("preview"))`, `q2-debug → ("html", None)`,
`q2-preview → ("html", Some("preview"))`, `q2-sandboxed-preview → ("html", Some("preview"))`. P1
names only `q2-preview` and `q2-slides`. A `from_format` that omits `q2-sandboxed-preview` routes
the sandboxed-preview port (bd-jgpz4hfq) through the full HTML pipeline — a silent regression with
no test naming it. And because `q2-slides`' *identifier* is `Html`, family must be derived from
`is_revealjs_target(target_format)` (the string), never from `format.identifier`. Neither point is a
design decision — mapping `q2-sandboxed-preview` to `HtmlPreview` is the only behavior-preserving
choice — but both need to be in the plan's enumeration, and both are bound by T1.1.

**F5 — `FormatIdentifier::Pptx` is a three-edit change, and the third edit is the only one that
matters.** The plan's round-4 correction says "a two-arm addition (`as_str`,
`output_extension_for`)". Verified: those are indeed the only two *exhaustive* matches on the enum
(`format.rs:44-56` and `:279-291`; confirmed by grepping `FormatIdentifier::Html =>` workspace-wide
— exactly two non-test hits). But `Format::from_format_string` resolves via
`FormatIdentifier::try_from(format_str)` (`format.rs:401`), whose match is on **strings** with a
`_ => Err` catch-all (`format.rs:82-96`). Without a `"pptx" => Ok(FormatIdentifier::Pptx)` arm
there, `from_format_string("pptx")` still returns `Err("Unknown format: pptx")` from
`format.rs:449` — and **the compiler will not tell you**, because that match is not exhaustive.
So: two compiler-forced arms plus one silent-but-required arm. T1.2 asserts
`from_format_string("pptx")` rather than `Pptx.as_str()` for exactly this reason. Worth a one-line
correction in the plan so nobody implements the two arms and believes `--to pptx` now resolves.

**F6 — Design §6's `LinkRewrite` cell still reads `B3?`, and no checklist item owns landing it.**
Design doc line 208: `| LinkRewrite | **B3?** | cross-doc/relative links; project/book-gated; no-op
standalone |`. Meanwhile P1's own "Produces for P7" section asserts "the B3 shared-services segment
(ResourceCollector, LinkRewrite, Appendix — **all confirmed in, not conditional**)", and
`link-rewrite` is in the surviving set T2.3 pins. So the plan has effectively decided it while the
frozen-decision table still shows the question mark. The AppendixStructure cell got an explicit
checklist item and a Finding; LinkRewrite got neither. I have listed it as Task 9 sub-item 6 and
marked it **do not land without you**, since a `?` in a frozen section reads as an open decision
rather than a typo. (Supporting evidence, if it helps: `link-rewrite` is `TransformPhase::Finalization`,
runs at `pipeline.rs:1465` immediately before `appendix-structure` at `:1466`, and is already
unexcluded for q2-preview's non-HTML consumer — the same argument that settled Appendix as B3.)

**RESOLVED and LANDED 2026-09-18, decided with Gordon: confirmed `B3`.** The design doc's §6 cell
now reads plain `B3` with an inline "Resolved 2026-09-18" note, and the Amendment log carries an
entry recording the decision, the supporting evidence above, and the fact that Gordon signed off
explicitly. **This is the only frozen-§6-cell edit made during the whole prevalidation pass** —
every other design-doc-touching finding either needed no edit (F1) or was recorded as a
consequence rather than a change (F3). Noted because the epic's convention is that frozen sections
are not edited without explicit sign-off, and this pass should not read as having relaxed it.

**F7 — Citation drift (cited as-is above; not silently propagated, not silently fixed).** Seven
anchors in P1 and the design doc have moved or were slightly off. All are harmless to the decisions
they support; listing them so the plans can be corrected in one pass:

| Cited in | Says | Actually |
|---|---|---|
| P1, "What the current mechanism actually is" | `pipeline.rs:2990` — `render_qmd_to_preview_ast_builds_reveal_slides_for_q2_slides` | `pipeline.rs:2996` (2990 is mid doc-comment) |
| P1, same section | `Q2_PREVIEW_TRANSFORM_EXCLUDED`, `pipeline.rs:1594-1643` | `1594-1639` |
| P1, same section | `is_revealjs_target` at `format.rs:138` | fn at `format.rs:137`; the `matches!` body at `:138` |
| P1, Seam item 2 | `Format::from_format_string` fails at `format.rs:447` | `format.rs:449` |
| P1, stage-list item (2) | `stage/stages/attribution_generate.rs:67` | `:61-63` (`fn name` at 61, string at 62) |
| P1, Navigation correction | `llms-capture` at `project/llms_post_render.rs` | `transforms/llms.rs:178`. (`project/llms_post_render.rs` exists, but is not where the transform's `name()` lives.) |
| P1, self-gating table | `draft_alert.rs:127,142` | gate at `:126`, `return Ok(())` at `:127`, predicate `fn format_supports_draft_alert` at `:141-143` |
| P1, self-gating table | `toc_generate.rs:85` listed as a gate shape | `:85` is inside `fn toc_title_term` selecting a language term — **not an early return**. Same category as the `crossref_render.rs` entry the plan already corrected. Moot for the decision (`toc-generate` is excluded), but it means the "7 self-gaters" count may itself be 6. |

Verified-and-correct anchors, for the record (no action needed): `pipeline.rs:1264-1277` (family
if/else, exact), `:1595` (`callout-resolve`), `:1465-1466` (LinkRewrite→Appendix ordering), `:1608`
(the appendix-structure "now INCLUDED" comment), `:2198` (`stages[16]`), `:2851` and `:4007` (both
validators), `:270`, `:395`, `:1144`, `:3829`; `format.rs:29` (`Docx`), `:58-60` (`is_native`),
`:279-291` (`output_extension_for`); `render.rs:680-684` (the `is_native()` gate, actually 680-685);
`panel_tabset.rs:109-114` (the round-4 citation fix is right); `title_block.rs:65-75` and `:97`;
`mermaid.rs:175`; `format_css.rs:93`; `responsive_image.rs:152`; `crossref_render.rs:28-31` and
`:88`; `example_embed.rs:380-409`. The **20**-member Navigation count and the
`Q2_PREVIEW_TRANSFORM_EXCLUDED` 7-item count both independently re-derived and confirmed.

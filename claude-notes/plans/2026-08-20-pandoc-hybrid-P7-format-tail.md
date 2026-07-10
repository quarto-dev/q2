# P7 — Per-format tail + invocation builder (docx first)

**Date:** 2026-08-20  **Updated:** 2026-09-18 (two passes) — see `git log --oneline -- claude-notes/plans/2026-08-20-pandoc-hybrid-P7-format-tail.md`
for the full correction history. Latest (round 4 review): the `render.rs:680-684` relaxation is
sufficient for docx and insufficient for pptx (confirmed independently by three reviewers —
`Format::from_format_string("pptx")` fails one line above the gate this item relaxes; fixed in
coordination with P1's added `FormatIdentifier::Pptx`); routed `Pandoc(fmt)` through P4's
`render_qmd_to_pandoc` entry point (this plan never referenced it before, despite being the only
plan touching the CLI path); landed **Gordon's decision** on the project-mode containment gate
(design doc §13) as an explicit checklist item, since this plan's own relaxation is what makes the
risk reachable; explicitly took ownership of applying pptx's `execute` defaults into
`EngineExecutionStage` (previously stated but disowned); added equation-content (`<m:oMath>`) to
the golden extraction; corrected the mermaid fixture-exclusion reasoning (it's fixture-blindness,
not structural blindness — capture one as a labeled accepted-divergence snapshot instead of
excluding it entirely). (Prior pass, same day: user-facing/backward-compat + implementation-
feasibility reviews found four gaps in this plan's own scope — format-specific `execute` defaults
missing pptx's `echo: false`/`warning: false`, an unstated pandoc-defaults forwarding policy, the
multi-format render guardrail this plan's own `render.rs` relaxation removes, and three concrete
build gaps in this plan's own golden-artifact proposal — closed, see the "Finding" section below.)
**Status:** Shape draft
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`  |  Depends on: P1, P2, P4, P5
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-18-pandoc-hybrid-P7-implementation.md`](2026-09-18-pandoc-hybrid-P7-implementation.md) — this plan's Coarse checklist converted into dispatchable `## Task N` units, each test bound to a named production seam and revert hunk.

## Goal
Turn the working transport + shim into real per-format output. Each format's **tail** = skip
Navigation, run B3 shared services, serialize the wire format, invoke pandoc with the right
`--to`/defaults/filters, plus the **Meta-block mapping**. **docx first, then pptx; latex a stub.**

## The TS-orchestration research gap is closed — this is a lookup task, not a research task

The superseded 2026-07-10 plan flagged the Q1 TypeScript format-orchestration layer (exact
per-format Pandoc invocation, Tier-1/2/3 classification, non-Pandoc post-processing) as
unresearched and blocking. It no longer is:
`claude-notes/research/2026-07-13-q1-format-typescript.md` is marked **Status: Complete —
answers the Phase 0 open questions in the epic plan**, and the epic doc cites it as done
research. P7's job is to **pull the exact invocation out of that doc**, not to re-derive it.

Concretely, for this plan's targets:
- **docx/odt** (`createWordprocessorFormat`): page-width/fig defaults only. **Quarto does no
  `--reference-doc` resolution** — a user-supplied value passes straight through to Pandoc
  unmodified. docx additionally wires 5 callout-icon filter params via `formatExtras`.
- **pptx**: `output-divs: false` (overrides the HTML-family base default), fig defaults; no
  reference-doc handling, no post-step. Tier-2-grouped with docx for post-processing purposes,
  but its Pandoc invocation is Tier-1-shaped (pure writer + Lua).
- **latex** (`latexFormat()`, stub only for this plan): `format: latex` emits `.tex` directly —
  the file extension wins over the inner `pdf` recipe, so no latexmk/tectonic step applies here.

Both items previously carried as "Deferred in-plan questions" in this plan
(exact per-format invocation; `--reference-doc` passthrough for docx/pptx) are answered above.

## Finding — four gaps closed (2026-09-18, user-facing/backward-compatibility + implementation-feasibility reviews)

1. **Format-specific `execute` defaults have no owner, and this plan's own research summary
   dropped two of them.** Q1's format definitions carry `execute` defaults, not just render/
   pandoc ones (`src/format/formats.ts:315-331`, `formats-shared.ts:170-186`):

   | | `page-width` | `execute.fig-width` | `execute.fig-height` | other `execute` | `pandoc` |
   |---|---|---|---|---|---|
   | **docx/odt** | 6.5 | 5 | 4 | — | `default-image-extension: png` |
   | **pptx** | 10 | 11 | 5.5 | **`echo: false`, `warning: false`** | `default-image-extension: png` |
   | *(base default)* | — | 7 | 5 | `echo: true` | — |

   The research doc records only "page-width/fig defaults only" for docx and "fig defaults" for
   pptx — **`echo: false`/`warning: false` for pptx were missing entirely**. Without them, a
   computational pptx shows every chunk's full source code and any `warning()` output on the
   slides, where Q1 hides both — the difference between a presentable deck and an unusable one.
   These are consumed by `EngineExecutionStage`, upstream of this plan's own per-format tail
   (`pipeline.rs:322` vs. the tail at the very end) — **this plan still owns stating the values**
   (it's the only plan that has the per-format facts), but applying them is a seam into
   `EngineExecutionStage`'s own defaulting, not something P7's invocation builder does directly.
   **Correction (2026-09-18, round 4 review, Reviewer C): "not something P7 does directly" left
   this seam with no owner at all** — no other plan mentions `EngineExecutionStage`, and this
   plan's own checklist folded the values back into the invocation-builder item, the same
   "each plan points at the other" shape this epic already had to resolve twice (P7↔P8, epic-wide
   review I10). **P7 owns this checklist item too, explicitly**, since it already has the values —
   see the checklist below.
   Also: `default-image-extension: png` (both formats) keeps engines from emitting SVG into a
   Word/PowerPoint document — needs the same ownership. The 5 docx callout-icon PNGs P4 already
   defers to this plan live at `src/resources/formats/docx/{note,tip,warning,caution,important}.png`
   — **outside P4's traced vendoring closure** (`src/resources/filters/`) — this plan must vendor
   them separately; `docxCalloutImage` returns `nil` when unset (`modules/callouts.lua:84-96`),
   so the degradation without them is graceful (icon-less callouts) but silent.
2. **The pandoc-defaults forwarding policy is unstated, and `--reference-doc`/`template` are new
   config-authored *paths* this repo's own contract requires registering.** "Quarto does no
   `--reference-doc` resolution" is a true statement about Q1's TS — it says nothing about what
   Q2 forwards. **Decided:** allow-list forwarding, not full pass-through of
   `kPandocDefaultsKeys` — forward exactly the keys this plan names a v1 need for (`reference-doc`,
   `template`, `highlight-style`, `toc`/`toc-depth`, `reference-location`,
   `shift-heading-level-by`, `slide-level` for pptx), each with an explicit test, rather than
   silently forwarding everything Q1's TS type happens to declare (most of which docx/pptx v1
   doesn't need and hasn't audited). `reference-doc` and `template` are new path-resolution
   consumers this repo's `CLAUDE.md` requires registering: neither is in
   `FORMAT_PATH_KEYS` (`crates/quarto-core/src/project/format_paths.rs:99-105`, currently 5
   entries: `css, theme, include-in-header, include-before-body, include-after-body`) or in
   `claude-notes/designs/path-resolution-model.md`'s consumption inventory. Add both there when
   implementing (resolve relative to the declaring file, a leading `/` means project root — the
   same convention every other path key already follows), per the repo rule requiring a strand
   linked to `bd-oejuizi9` for any deliberate scope-out.
3. **Multi-format render guardrail — see design doc §14.** Relaxing `render.rs:680-684`'s format
   check removes the only existing signal that Q2 renders one format per invocation. Add a
   warning when `format:` declares more than one key and only one is rendered, naming which was
   used and which were skipped.
4. **The golden-artifact proposal (this plan's own, from an earlier pass) needs three concrete
   fixes to actually build, per a fresh implementation-feasibility review:**
   - **Insta mechanics.** `.snap` files are produced *by tests*, with filenames derived from
     `module_path!()` + snapshot name and a YAML header (`source:`, `expression:` —
     `.claude/rules/integration-tests.md`'s `integration__<module>__<name>.snap` convention). An
     **xtask** writing files a later test will match by name must either hand-author that
     convention exactly or use `insta::Settings` with an explicit snapshot path + name (simplest);
     getting this wrong yields "snapshot not found, created new" — a silent pass, not a build
     error. The shared extraction function (used by both the xtask and the integration test) needs
     a home — a small library crate (or a feature-gated module in `quarto-core`), since `xtask` is
     not a dependency of any test crate today.
   - **Fixture selection.** "`tests/docs/`" is 2,018 files — name 6–10 concretely (e.g.
     `docs/crossrefs/all-docx.qmd`, `docs/callouts.qmd`, `docs/crossrefs/theorems.qmd`, plus the
     Tabset-with-subfloat case P6 Finding 5 asks for), state the engine-free rule (capture renders
     with a real `quarto`, so an R/Python-engine fixture needs that toolchain — exclude those for
     v1). **Corrected 2026-09-18 (round 4 review, Reviewer D) — the exclusion bullet below lumped
     two very different gap shapes under one rule, and doing so for mermaid specifically
     neutralized this same Finding's own extraction improvement below.** Original text: "exclude
     fixtures exercising a feature this epic already accepts as a known gap (mermaid, the
     `filename` header, `crossref:` presentation options) unless deliberately capturing the
     *accepted* divergence with provenance." Split this: a mermaid code block reaching the Pandoc
     leg becomes a plain `CodeBlock` containing diagram source, which is **maximally visible** to
     even the base paragraph-text extraction — not a structural blind spot at all, the opposite of
     the `filename`-header/pagebreak/media gaps the extraction-scope item below actually fixes.
     The only thing hiding the mermaid gap is the fixture-exclusion rule itself, compounding with
     the engine-free rule (Q1 renders mermaid for non-HTML targets via Puppeteer/headless Chromium
     in TS, which the engine-free capture rule also excludes) and design doc §12's "no warning
     ships in v1" — three justifications for the same silence, each citing the other two. **Fix:
     capture *one* mermaid fixture as a labeled accepted-divergence snapshot** (per the provenance
     mechanism below), converting "we know it's broken, somewhere" into a committed, reviewable
     artifact — this adds no Q2 rendering capability, only a test fixture. Keep the `filename`-
     header and `crossref:`-presentation-option exclusions as-is; those genuinely are structural
     blind spots for the base extraction (see below) and belong under the original rule, not
     mermaid's. Add a `zip`-reading dependency (`quick-xml` exists at `Cargo.toml:37`; there is no
     `zip` crate yet).
   - **Extraction scope + provenance.** "Paragraph text + style name + list nesting + resolved
     numbering fields" is blind to whether an image resolved at all, docx callout icons (pure
     image, zero text), and pagebreaks — several of *this epic's own* flagged risks sit exactly in
     that blind spot. Extend the extraction to also record the relationship/media inventory
     (`word/_rels/document.xml.rels` entry count + targets, `word/media/` file list) and
     `<w:drawing>`/`<w:br w:type="page"/>` element counts — cheap, version-stable, and turns "image
     silently missing" into a visible snapshot diff. **Also add `<m:oMath>` text content (or at
     minimum an `<m:oMath>` element count plus its flattened text)** (2026-09-18, round 4 review,
     Reviewer B) — for docx/pptx, `renderEquation`'s fallback branch mutates the equation's TeX
     source itself before Pandoc's writer converts it to OMML, so a rendered equation number ends
     up inside `<m:oMath>`, not in the paragraph's `<w:t>` runs; without this, the extraction shows
     identical text whether the equation number is present, absent, or wrong — exactly the Route-N
     behavior P5 just decided to implement by calling Q1's function. Name
     `docs/crossrefs/equations.qmd` (or equivalent) among the 6-10 fixtures so this path is
     actually exercised. Separately: the first real capture run will legitimately diverge from Q1
     for the accepted-limitation cases (Route N's Q1-normative ref/equation text, the dropped
     `filename` header, missing section numbers, the one labeled mermaid divergence above) — record
     *why* each such snapshot was accepted via a one-line comment or adjacent `.md` note per
     fixture, so accepting a divergence via `cargo insta review` doesn't
     silently convert a "Q1-parity" snapshot into an unlabeled "Q2 baseline" that stops meaning
     what the plan says it means.

## In scope
- **Build the golden-test methodology for binary formats first.** docx/pptx are zipped XML; the
  epic's "byte-similarity" testing strategy doesn't apply as stated. Every P3/P5/P6/P7
  parity gate depends on a structural-diff approach (unzip + normalize + XML-diff) that doesn't
  exist yet, and neither does the `external-sources`-based local Q1-comparison harness the
  epic's testing strategy assumes is already there. **Concrete proposal, resolved 2026-09-17
  (closes the design doc §11 "golden-parity artifact strategy is unresolved" open question):**

  1. **Fixture source: reuse quarto-cli's own test suite, don't hand-author from scratch.** Draw
     fixtures from `tests/docs/` in the pinned `v1.11.3` checkout (`~/src/quarto-cli`), which
     already has extensively-exercised `.qmd` documents for every semantic concept this epic's
     Route-R types care about (crossref figures/tables, theorems, callouts, code-block
     filename/fold, tabsets, footnotes, appendices) — these are Q1's own canonical spec for "what
     the feature does," maintained by the team that owns the behavior. Add a fixture in our own
     tests only for a combination no existing quarto-cli fixture covers (e.g. a Tabset containing
     a FloatRefTarget subfloat, per P6 Finding 5).
  2. **Capture is a dev-only `cargo xtask capture-pandoc-goldens` command, never part of CI or
     `cargo xtask verify`** (matches design doc §8: "Q1 byte-parity ... is a local/dev gate"):
     - Locates a real `quarto` binary at the same pinned `v1.11.3` **release** used for the
       vendored Lua (download the official release binary for the host platform — simpler than
       building quarto-cli's own Deno/TS toolchain, and matches the "vendor a release tag, not a
       dev commit" precedent P4/P5 already established).
     - Renders each selected fixture with that real `quarto` to `--to docx` and `--to pptx`.
     - Unzips the output and extracts a **semantic text representation**, not raw XML — walk
       `word/document.xml` (docx) / `ppt/slides/slideN.xml` (pptx) into a simplified tree: paragraph
       text + style name + list nesting + resolved numbering fields (e.g. "Figure 1: caption
       text"). This is the concrete implementation of "structural, not byte-for-byte" — raw
       normalized XML is still noisy (Pandoc's own writer isn't guaranteed byte-stable across
       Pandoc versions for identical input), while a semantic extraction only breaks when content
       actually differs.
     - Writes each fixture's extraction as an **insta snapshot** (`.snap` file), one per
       fixture/format pair — reusing this repo's existing snapshot infrastructure and
       `cargo insta review` workflow rather than inventing a new diff/review UI.
  3. **The comparison test IS CI-runnable** (no `external-sources`, no live `quarto` needed): a
     P7 integration test renders each fixture through Q2's own new hybrid path, applies the
     *identical* extraction logic to Q2's output, and asserts it via `insta::assert_snapshot!`
     against the same committed snapshot the capture step produced from real Q1. **The Q1-parity
     check and the regression check are the same assertion** — the moment Q2's hybrid output
     diverges from Q1's captured behavior (a real regression) or intended new content is added,
     it fails exactly like any other insta snapshot test, with the same reviewer ergonomics.
  4. **Re-capture policy mirrors P4/P5's v1.11.3-pin philosophy:** capture once, at the pinned
     tag; only re-run `capture-pandoc-goldens` on a deliberate Q1 re-vendor (bump the pin,
     re-capture, review the resulting insta diff same as any other snapshot update — no separate
     drift-tracking mechanism needed).
  5. **Committed artifacts are the extracted `.snap` files only, never the raw quarto-cli-sourced
     `.qmd` fixture files or docx/pptx binaries themselves**, if the source fixture isn't already
     one we own — per this repo's External Sources Policy, anything needed at test time must be a
     local, committed resource, not a read from `external-sources/`; fixtures drawn from
     quarto-cli get copied into our own `tests/fixtures/` directory once (mirroring
     `resources/scss/`'s "copy in, track locally" pattern), not referenced from the sibling
     checkout at test time.
- **Triage two pre-existing Q2 bugs before trusting any golden diff:** the nested-`<p>` bug from
  missing `ensureMetaInlines` block→inline coercion (`template.rs:221` /
  `titleblock_field_to_html`), and the silent multi-id crossref drop (`[@fig-a; @fig-b]` resolves
  only the first, `crossref_resolve.rs:487`). Both will produce Q1/Q2 diffs unrelated to the
  hybrid work — fix or explicitly flag them so parity failures stay diagnosable.
- A per-format **invocation builder** implementing the facts above: `--to`, defaults, filter
  list, Meta variables. docx + pptx concretely; latex stubbed with its shape documented.
- **Meta-block contract:** map Q2's normalized doc metadata (title/date/authors) into the wire
  output's Pandoc `Meta` (P2 confirmed carriage; P7 does the per-format mapping — trivial for
  docx, richer for jats later).
- **B3 shared services for the Pandoc tail:** confirm ResourceCollector staging (mediabag drain)
  and LinkRewrite run before the handoff. **The `link_rewrite.rs:29` stale-comment/behavior gap
  this plan previously flagged no longer exists** — verified 2026-09-17 by reading the file
  directly: `link_rewrite.rs`'s doc comment (lines ~19-30) already documents that
  `Image::target.0` is rewritten via `resolve_static_resource_href`, explicitly "matching Q1."
  Image rewriting landed later than this plan's original claim, via Case B
  (bd-root-relative-paths-design-fc5pvkcv, commit `1d17a9ce7`) — the transform can be reused
  verbatim for the Pandoc tail with no fix needed here.
- Relax `render.rs:680-684` (corrected 2026-09-17 from `~628`, matching the epic doc's own
  citation fix) to admit docx/pptx through the hybrid path; end-to-end via
  `cargo run --bin q2 -- render fixture.qmd --to docx`. **Corrected 2026-09-18 (round 4 review —
  confirmed independently by three reviewers): this relaxation is sufficient for docx and
  insufficient for pptx.** `Format::from_format_string("pptx")` fails at
  `crates/quarto-core/src/format.rs:447` (`Err("Unknown format: pptx")`) at
  `render.rs:676`'s `resolve_format(&format_str)?` call — **one line above** the `is_native()`
  gate this item relaxes — because `FormatIdentifier` had no `Pptx` variant. P1 now adds
  `FormatIdentifier::Pptx` (see P1's corrected Seam definition); this item's end-to-end test must
  cover **both** `--to docx` and `--to pptx`, not just docx, to actually catch the gap the prior
  text missed. **Also route `Pandoc(fmt)` formats through P4's `render_qmd_to_pandoc` entry point**
  (P4 Finding 3) — the real call chain today is `render.rs` → `render_document_to_file` →
  `render_to_file.rs:358`'s `render_qmd_to_html(...)`, and nothing in this plan previously routed
  a relaxed docx/pptx render anywhere else; confirm `RenderedOutput.content` being empty (P4's
  binary-output decision) doesn't break the `OutputSink`/artifact path at `render_to_file.rs:358-375`.
- **Confirm no in-scope docx/pptx fixture uses `.algorithm`; if one does, land
  `THEOREM_CLASSES`/`RefTypeRegistry::BUILTINS` first; otherwise file the follow-on strand.**
  Added 2026-09-17 (epic-wide review, I10): the epic's Definition of done has this exact
  conditional item, but P5 (its cited owner) explicitly disowns the implementation twice, and no
  other plan picked it up. P7 is the only plan that can evaluate the condition, since it owns the
  fixture set.
- **The `.content-visible`/`.content-hidden` smoke fixture is P8's item, not this plan's —
  corrected 2026-09-17 (epic-wide review, I1).** This plan and P8 each previously pointed at the
  other for the same fixture (P7: "once P8's target-format verification lands"; P8: "once P7's
  tail exists") — a latent cycle with no owner. Resolved: it's added to **P8's** checklist once
  this plan's Pandoc tail lands; removed from here (see the design doc §9 for the corrected
  dependency note).

## Out of scope
- Tier-2 post-processing (pdf/typst/epub) — follow-on. Tier-3 (`confluence-publish`) — follow-on.
- latex beyond a documented stub.

## Consumes / Produces (seams)
- **Consumes:** P1 profile/tail seam + B3 services (including the `Pandoc`-kind exclude-list
  P1 introduces — this plan's invocation builder may need to extend it per format); P2 wire
  format + Meta carriage; P4 machinery; P5 shim; (P6 for correct numbers); the per-format facts
  in `claude-notes/research/2026-07-13-q1-format-typescript.md`.
- **Produces:** docx/pptx golden-parity to Q1; **the working Pandoc tail P8 needs** for its own
  `.content-visible`/`.content-hidden` smoke fixture (corrected 2026-09-17 — see checklist above;
  this used to be phrased as a P7 dependency on P8, which was the wrong direction).

## Coarse checklist
- [ ] Build the golden-test harness per the concrete proposal above (name 6-10 fixtures, engine-
  free rule, `zip` dependency, `insta::Settings`-based capture from the xtask, extraction extended
  to media/pagebreak counts, per-fixture accepted-divergence provenance — see Finding 4 above):
  `cargo xtask capture-pandoc-goldens` (dev-only, locates real pinned-release `quarto`, renders
  selected quarto-cli-sourced + own fixtures, extracts semantic text, writes insta snapshots) +
  the P7 integration test that asserts Q2's own hybrid output against the same snapshots. Copy
  any quarto-cli-sourced fixtures into our own `tests/fixtures/` (External Sources Policy).
- [ ] Triage the nested-`<p>` and multi-id-crossref-drop bugs (fix or explicitly flag before parity testing).
- [x] `link_rewrite.rs:29`'s stale comment / behavior gap — **resolved 2026-09-17: no longer
  exists.** Read the file directly; the doc comment already correctly documents image rewriting
  matching Q1 (landed later via commit `1d17a9ce7`, after this plan's original claim was
  written). No fix needed; reuse the transform verbatim.
- [ ] Invocation builder (docx, pptx) per the facts pulled from the TS research doc above; latex
  stub documented. **Include format-specific `execute`/`pandoc` defaults** (Finding 1: pptx
  `echo: false`/`warning: false`, both formats' fig sizes and `default-image-extension: png`) and
  **the pandoc-defaults forwarding allow-list** (Finding 2: `reference-doc`, `template`,
  `highlight-style`, `toc`/`toc-depth`, `reference-location`, `shift-heading-level-by`,
  `slide-level`) — register `reference-doc`/`template` as `FORMAT_PATH_KEYS` consumers per the
  path-resolution contract.
- [ ] **New (2026-09-18, round 4 review): apply the pptx `execute` defaults into
  `EngineExecutionStage`'s own defaulting** — this plan states the values (Finding 1) but no plan
  previously owned applying them; explicitly this plan's item now (see the correction above),
  since P7 is the only plan with the per-format facts and no other plan mentions
  `EngineExecutionStage`.
- [ ] **New (2026-09-18): vendor the 5 docx callout-icon PNGs** from
  `src/resources/formats/docx/` (Finding 1 — outside P4's traced `src/resources/filters/`
  closure) or explicitly accept icon-less docx callouts in writing.
- [ ] **New (2026-09-18): multi-format render warning** (Finding 3 / design doc §14) — when
  `format:` declares more than one key and only one renders, name which was used and which were
  skipped. Restores the guardrail this plan's own `render.rs` relaxation removes. **Needs a real
  `Q-pandoc-*` code** (round 4 review, Reviewer C) — this is a user-facing diagnostic, not routed
  through stderr passthrough like the other `pandoc`-subsystem cases; assign it a code + docs page
  + sidebar entry alongside P4's initial code set (subsystem 18), don't leave it as a bare warning
  string with no catalog entry.
- [ ] **New (2026-09-18, round 4 review — Gordon's decision, design doc §13): land the
  project-mode containment gate in this plan.** Relaxing `render.rs:680-684`/`resolve_format`
  makes `q2 render <website-project> --to docx` reachable in the same commit, and
  `WebsiteProjectType::post_render` (`crates/quarto-core/src/project/orchestrator.rs:517-580`) has
  **no format gate at all** — it unconditionally writes a sitemap of `.html` URLs that don't exist
  (`output_href` is hardcoded `.qmd`→`.html`), and `write_alias_redirects` can hard-fail with an
  HTML-specific diagnostic for a render that produced no HTML. Gordon decided (round 4 review):
  gate `WebsiteProjectType::post_render`'s hook sequence on `format.identifier.is_html_based()` —
  the same one-line fix `bd-bgeet2mw` already describes, landed here rather than waiting on that
  strand, since this plan is what makes the risk reachable. This matches Q1's own established
  behavior (`websiteProjectType.postRender` already filters to HTML-only) and is squarely inside
  the "no new functionality" principle — it removes a regression this plan's own relaxation
  introduces, the same shape as the multi-format-render warning above (design doc §14).
- [ ] Meta-block mapping (docx/pptx).
- [ ] B3 services wired into the Pandoc tail (resources staged, links rewritten).
- [ ] `render.rs` admits docx/pptx; end-to-end inspected output for **both formats** (corrected
  2026-09-18 — see the render.rs correction above, pptx needs `FormatIdentifier::Pptx` from P1
  first); Q1 golden per format.
- [ ] Confirm no in-scope docx/pptx fixture uses `.algorithm`; land the `THEOREM_CLASSES`
  fix first if one does, otherwise file the follow-on strand (epic DoD item, owned here as of
  2026-09-17 — see In scope above).

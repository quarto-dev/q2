# Heading identifiers are not disambiguated across include boundaries (bd-duplicate-heading-ids-mou5z7ux)

**Date:** 2026-08-18
**Braid:** bd-duplicate-heading-ids-mou5z7ux (p2, bug, label `markdown`)
**Checkout:** main checkout, branch `main` @ `4eaede00` (investigation only; implementation branch TBD)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The bug reproduces at HEAD exactly as filed, the root cause named in the strand is accurate (verified against the code), and the main open work is choosing between two fix shapes and a placement — questions below.

## Issue context

Filed 2026-08-18 by Carlos (fresh — no staleness risk). A fragment with a heading, included N times via `{{< include >}}`, emits the same auto-generated id N times. Q1 emits `create-the-integration`, `-1`, `-2` because its include is a textual splice *before* pandoc parses, so pandoc's `uniqueIdent` sees one document. Consequences: invalid HTML (duplicate ids) and dead TOC entries (all point at the first occurrence). No diagnostic; exit 0.

Real-world hit: Posit Connect docs port — 7 duplicate heading ids across 2 OAuth-integration pages (5-tab tabsets each including `_azure_intro.qmd` per tab). Origin strand in the connect-docs skein: br-duplicate-heading-ids-ye3j3gkr.

## Dependency graph

Sparse — one edge:

- **related: bd-2wv8431v** (open, p4) — "Heading ids do not reflect shortcode expansion." Same underlying shape: the auto id is decided **too early and too locally** (in the reader's postprocess, per-parse). That strand was deliberately deferred ("design it then, not now" — nobody has asked for it). It matters here because the *maximal* fix (move auto-id assignment out of the reader entirely, to a document-level pass after include **and** shortcode expansion) would resolve both; the minimal fix resolves only this one.
- Also referenced in the description (not an edge): **bd-tabset-headings-in-toc-t04ie7f7** (open, p2) — why those headings reach the TOC at all. Independent; no ordering constraint between the two fixes.
- No `discovered-from` or `blocks` edges; the urgency pin is the Connect-docs port itself.

## What the code looks like today

All paths in the strand check out at HEAD (`4eaede00`):

- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:903` — `seen_ids: HashMap<String, usize>` local to one `postprocess()` call; the `with_header` closure (:931) assigns `auto_generated_id` + `-N` dedup **only when `attr.0` is empty**.
- `crates/quarto-core/src/stage/stages/include_expansion.rs:218` — child file parsed standalone via `pampa::readers::qmd::read(...)`; each child parse gets a fresh `seen_ids`.
- **`AttrSourceInfo.id` already discriminates auto vs. author-written ids.** The qmd writer (`crates/pampa/src/writers/qmd.rs:647-649`) suppresses ids where `header.attr_source.id.is_none() && attr.0 == auto_generated_id(content)`. A document-level pass can use the same test — the strand's "parser leaves the id empty" trick is *one* way to get that discrimination, but not the only one.
- Pipeline builders that run `IncludeExpansionStage` (a fix must cover all, or live where they all pass through):
  - `build_html_pipeline_stages` / `build_pipeline_stages_with_registry` (`pipeline.rs:293`) — native render
  - `build_wasm_html_pipeline` (`pipeline.rs:560`) — hub-client / preview WASM
  - `build_analysis_pipeline` (`pipeline.rs:713`)
  - orchestrator profile pass (`project/orchestrator.rs:1944`)
  - `quarto-preview/src/config.rs:296` runs the stage standalone for dep-tracking
- Downstream consumers of heading ids, in stage order: `DocumentProfileStage` (outline), `LinkResolutionStage`, `PreEngineSugaringStage` (crossref registry), engines, user filters, then in `AstTransformsStage`: `PanelTabsetTransform` → `ShortcodeResolveTransform` → `SectionizeTransform` (copies id onto section Div) → crossref → `TocGenerateTransform`. Ids must be final before `DocumentProfileStage` for the profile outline to be right.

**Reproduced at HEAD** (fixture committed at `claude-notes/plans/duplicate-heading-ids-includes-investigation/repro/`):

```
$ cargo run --bin q2 -- render claude-notes/plans/duplicate-heading-ids-includes-investigation/repro/index.qmd
$ grep -o 'id="create-the-integration[^"]*"' .../repro/index.html
id="create-the-integration"
id="create-the-integration"
id="create-the-integration"
```

Control (`control-inline.qmd`, same three headings written inline in one file) correctly emits `create-the-integration`, `-1`, `-2` — confirming the gap is scope, not logic. (Output was inspected directly; TOC entries didn't appear in the minimal fixture because level-4 headings sit below the default `toc-depth`, but the id collision is format-independent.)

## Fix shapes considered

**Option A — strand's suggestion (maximal):** the reader stops assigning auto ids ("empty = wants one"); a document-level pass on the assembled AST assigns + dedups. Cleanest semantics and the shape bd-2wv8431v eventually needs (run it after shortcode expansion too). But blast radius is large: every standalone pampa consumer (pampa CLI, wasm-qmd-parser, qmd-syntax-helper, hub-client) either needs the pass appended to its own entry point or changes behavior; the qmd writer's round-trip check and a wall of snapshots are implicated; and the pass must be wired into every pipeline builder.

**Option B — minimal (recommended):** keep the reader as-is; add a document-level **re-dedup pass over the assembled AST** that recomputes ids for exactly the headers with `attr_source.id.is_none()` (auto-assigned): reset to `auto_generated_id(content)`, re-dedup in document order with one fresh map. Deterministic, idempotent (a no-include document is unchanged, since the per-parse pass already produced the document-level answer), and it does **not** make fragment parses depend on splice position — the fragment parse stays independently cacheable; only the assembled document is renumbered. No parser, writer, or snapshot churn. Does not advance bd-2wv8431v, but that strand is p4 and deferred by explicit decision.

**Rejected (by the strand, agreed):** threading a shared `seen_ids` into child parses — makes parse results position-dependent.

## Proposed phases (draft — assumes Option B; renumber if A is chosen)

- **Phase 0 — Test plan (TDD).**
  - quarto-core integration test: repeated include → ids `x`, `x-1`, `x-2` (fails today).
  - Control: inline repeats unchanged; explicit `{#id}` headings never renumbered (fails-safe today, pins behavior).
  - Nested include (`a.qmd` includes `b.qmd` twice, each carrying a heading).
  - Profile test: `DocumentProfileStage` outline sees deduped ids (extends `document_profile_pipeline.rs`).
  - E2e per CLAUDE.md: `q2 render` of the committed fixture, inspect HTML (and a TOC-visible variant with `toc-depth: 4` to pin the scroll-target fix).
- **Phase 1 — Document-level re-dedup pass** (function over `Pandoc`, lives in pampa or quarto-core — see Q2) + unit tests.
- **Phase 2 — Wire into the pipeline** at the chosen placement; confirm all four builders + `quarto-preview` are covered; WASM leg via full `cargo xtask verify`.
- **Phase 3 (optional, see Q4) — duplicate-explicit-id diagnostic** (new Q-code, catalog entry + `docs/errors/` page in the same commit per lint rule).
- **Phase N — Docs** (if user-visible behavior notes are warranted) + close-out; comment on br-duplicate-heading-ids-ye3j3gkr in the connect-docs skein after a release ships the fix.

## Open design questions for the user

1. **Fix shape.** Option B (document-level re-dedup of auto ids, keyed off `attr_source.id.is_none()`) or Option A (reader stops assigning; document-level assignment — the bd-2wv8431v-compatible shape)? I recommend **B**: it fixes the filed bug with a small, testable surface, and A remains available later since B's pass is exactly where A's assignment would live.
2. **Placement.** (a) Tail of `IncludeExpansionStage::run()` — every builder gets it for free, invariant stays local ("after this stage, auto ids are document-unique"); or (b) a separate stage/pass added to each of the four builders — cleaner separation, but four call sites to keep in sync (and future builders can forget it). I lean (a).
3. **Engine-output headings.** Q1 assigns ids *after* the engine runs (knitr/jupyter output is part of the text pandoc parses); q2's `EngineExecutionStage` runs after include expansion, so a heading emitted by a code cell could still collide post-fix. Fixing that would mean renumbering *after* engines — but `DocumentProfileStage` (outline) runs pre-engine and wants final ids, so there's a real tension. Treat as out of scope + file a follow-up strand, or fold in now?
4. **Duplicate explicit `{#id}` diagnostic.** The fix deliberately never renumbers author-written ids, and today nothing warns when two collide. In scope here (new Q-code + docs page), or a follow-up strand?
5. **Dedup universe.** Pandoc's `uniqueIdent` avoids collisions against *all* seen ids, including explicit ones (auto heading "Foo" after an explicit `{#foo}` becomes `foo-1`); q2's current pass dedups only among auto ids. Preserve current (auto-only) semantics at document level, or seed the map with explicit ids for closer pandoc parity? (Auto-only is the no-surprise choice; parity is a one-liner but changes ids in documents that mix the two.)

## Risks / tradeoffs (draft)

- Any renumbering changes ids some site may already link to — but those links were ambiguous/dead anyway; Q1 parity is the defensible target.
- The re-dedup pass must recompute with the same `auto_generated_id` the reader used (content is pre-shortcode-expansion at that point; `Shortcode` inlines are skipped by explicit decision — bd-2wv8431v) or ids would drift from the per-parse result in the no-include case.
- `attr_source.id.is_none()` as the "auto id" discriminator must hold for all header producers; the qmd writer already relies on it, which is good precedent, but engine-spliced or transform-fabricated headers should be spot-checked.
- Snapshot churn expected to be zero for Option B (no-include documents are fixpoints); any churn that does appear is a red flag worth review, per the CLAUDE.md snapshot policy.

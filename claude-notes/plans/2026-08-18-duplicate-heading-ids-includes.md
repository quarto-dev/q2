# Heading identifiers are not disambiguated across include boundaries (bd-duplicate-heading-ids-mou5z7ux)

**Date:** 2026-08-18
**Braid:** bd-duplicate-heading-ids-mou5z7ux (p2, bug, label `markdown`)
**Checkout:** main checkout, branch `main` @ `4eaede00` at investigation time (implementation branch TBD)
**Status:** Design aligned with user 2026-08-18 (answers recorded below). One residual design point (whole-document recompute vs. inserted-nodes-only, see Decision 2) surfaced back to the user; **implementation waits on that confirmation.**

## Design decisions (user-aligned, 2026-08-18)

1. **Fix shape: Option B.** Keep the reader assigning ids per-parse; add a document-level dedup pass keyed off `attr_source.id.is_none()`. User note for the future: id duplication is more general than includes (engine output, transform-fabricated headers), and the pass may eventually relocate to a single late stage that dedups everything — Option B's pass is exactly the code that would move.
2. **Placement: tail of `IncludeExpansionStage::run()`**, gated on "at least one include was actually expanded" so the stage remains a strict no-op on documents without includes. **Residual point (awaiting user confirmation):** the user asked that the pass touch only the just-inserted nodes; the plan instead recomputes over the whole assembled document, for reasons argued in "Why whole-document recompute" below. The gate preserves the spirit (no includes → stage changes nothing); the whole-doc recompute preserves Q1 numbering in mixed inline+included cases.
3. **Engine-output headings: out of scope.** Filed as **bd-4qjl87ax** (discovered-from this strand).
4. **Duplicate explicit `{#id}` diagnostic:** implementable — `AttrSourceInfo.id` is `Some(source-span)` exactly when the author wrote the id, `None` when postprocess synthesized it (the qmd writer already relies on this at `crates/pampa/src/writers/qmd.rs:647-649`). Recommendation: follow-up strand, not folded in. **Awaiting user decision.**
5. **Dedup universe: pandoc `uniqueIdent` parity.** The seen-set contains *all* header ids in document order — explicit and auto alike — and a colliding auto id probes `base-1`, `base-2`, … for the first free candidate (set-membership probe, not a per-base counter: an explicit `{#foo-1}` must push auto "Foo" to `foo-2`). Headers only, matching pandoc's `registerHeader`; explicit ids on divs/spans are not in the set. Explicit ids are never renamed.

### Why whole-document recompute (Decision 2 residual)

Restricting renumbering to just-inserted nodes sounds safer but:

- The seen-set must include ids *outside* the inserted nodes anyway (an included heading colliding with an inline one is the general case), so "only look at inserted nodes" is not available; the question is only what may be *renamed*.
- Renaming only inserted nodes diverges from Q1 whenever an included heading precedes an inline duplicate: Q1's counter runs in document order and renames the *later* (inline) one; inserted-only renaming would rename the earlier (included) one.
- Whole-document recompute is idempotent for non-include content **provided the reader's per-parse pass implements the same uniqueIdent semantics** (Decision 5 forces that reader change anyway): every auto id recomputes to the value it already has, so untouched content is observably untouched — except where Q1 would also renumber, which is the bug being fixed.

So: Decision 5 updates the reader's per-parse dedup to uniqueIdent semantics; the doc-level pass is the *same routine* re-run over the assembled document; the include-expansion gate keeps no-include documents byte-identical through the stage.

## Triage verdict (investigation, 2026-08-18)

**Ready to design** (now: designed). Bug reproduces at HEAD exactly as filed; root cause confirmed against the code.

## Issue context

Filed 2026-08-18 by Carlos. A fragment with a heading, included N times via `{{< include >}}`, emits the same auto-generated id N times. Q1 emits `create-the-integration`, `-1`, `-2` because its include is a textual splice *before* pandoc parses, so pandoc's `uniqueIdent` sees one document. Consequences: invalid HTML (duplicate ids) and dead TOC entries (all point at the first occurrence). No diagnostic; exit 0.

Real-world hit: Posit Connect docs port — 7 duplicate heading ids across 2 OAuth-integration pages (5-tab tabsets each including `_azure_intro.qmd` per tab). Origin strand in the connect-docs skein: br-duplicate-heading-ids-ye3j3gkr.

## Dependency graph

- **related: bd-2wv8431v** (open, p4) — heading ids don't reflect shortcode expansion; same "id decided too early and too locally" shape. Deliberately deferred. Option B here does not advance it, but the doc-level pass is where its eventual fix would live.
- **discovered-from (outgoing): bd-4qjl87ax** — engine-output heading collisions, filed during this design (Decision 3).
- Referenced, independent: **bd-tabset-headings-in-toc-t04ie7f7** (why those headings reach the TOC at all).

## What the code looks like today

All paths verified at `4eaede00`:

- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:903` — `seen_ids: HashMap<String, usize>` local to one `postprocess()` call; `with_header` (:931) assigns `auto_generated_id` + `-N` (per-base counter) **only when `attr.0` is empty**; explicit ids are not recorded in the map.
- `crates/quarto-core/src/stage/stages/include_expansion.rs:218` — child parsed standalone via `pampa::readers::qmd::read(...)`; fresh `seen_ids` per child.
- `AttrSourceInfo.id` (`crates/quarto-pandoc-types/src/attr.rs:52`) discriminates auto (`None`) vs. author-written (`Some`) ids; qmd writer precedent at `writers/qmd.rs:647-649`.
- Pipeline builders running `IncludeExpansionStage` (all covered for free by the tail placement): native render (`pipeline.rs:293`), WASM (`pipeline.rs:560`), analysis (`pipeline.rs:713`), orchestrator profile pass (`project/orchestrator.rs:1944`), plus `quarto-preview/src/config.rs:296` (dep-tracking).
- Downstream id consumers in stage order: `DocumentProfileStage` (outline) → `LinkResolutionStage` → `PreEngineSugaringStage` → engines → user filters → `AstTransformsStage` (`PanelTabsetTransform` → `ShortcodeResolveTransform` → `SectionizeTransform` → crossref → `TocGenerateTransform`). Tail-of-include placement lands before all of them.

**Reproduced at HEAD** (fixture at `claude-notes/plans/duplicate-heading-ids-includes-investigation/repro/`):

```
$ cargo run --bin q2 -- render .../repro/index.qmd
$ grep -o 'id="create-the-integration[^"]*"' .../repro/index.html
id="create-the-integration"   (x3)
```

Control (`control-inline.qmd`, same heading three times inline) correctly emits base, `-1`, `-2` — the gap is scope, not logic.

## Implementation plan

### Phase 0 — Tests first (TDD; all must fail or pin current behavior before Phase 1)

pampa (uniqueIdent semantics in the reader — Decision 5):

- [ ] Explicit `{#foo}` then auto heading "Foo" → `foo-1` (fails today: emits colliding `foo`).
- [ ] Explicit `{#foo-1}` present, then two auto "Foo" headings → `foo`, `foo-2` (set-probe, not counter — fails today).
- [ ] Auto heading "Foo" then explicit `{#foo}` → explicit id kept verbatim (pin: explicit never renamed).
- [ ] Empty-content heading base-id edge case: check `auto_generated_id` output for empty inlines against pandoc's `section` fallback; pin whatever we decide.
- [ ] qmd-writer round-trip: a deduped header (`foo-1`) round-trips with an explicit `{#foo-1}` (existing behavior for `-N` ids — pin it).

quarto-core (the filed bug):

- [ ] Integration test: repeated include → `x`, `x-1`, `x-2` (fails today). Extend `crates/quarto-core/tests/integration/` per the integration-test layout rule (module inside the `integration` binary).
- [ ] Nested include (a.qmd includes b.qmd twice; b carries a heading).
- [ ] Mixed ordering, Q1 parity: inline "H" then included "H" → included gets `-1`; included "H" then inline "H" → *inline* gets `-1` (this is the case that forces whole-document recompute).
- [ ] Explicit `{#id}` inside an included file, included twice → both keep the id verbatim (no renaming), pinning Decision 4's premise.
- [ ] No-include document passes through `IncludeExpansionStage` unchanged (the gate).
- [ ] Profile outline: `DocumentProfileStage` sees deduped ids (extend `document_profile_pipeline.rs`).

End-to-end (per CLAUDE.md):

- [ ] `cargo run --bin q2 -- render` on the committed fixture; inspect HTML for `x`, `x-1`, `x-2`; add a `toc-depth: 4` variant and inspect distinct `data-scroll-target`s.

### Phase 1 — pampa: shared uniqueIdent routine

- [ ] Extract a document-level routine (working name `assign_unique_heading_ids(&mut Pandoc)`) in `pampa` (near `utils::autoid`): walk headers in document order; maintain `HashSet<String>` of used ids; for `attr_source.id.is_some()` insert verbatim; for auto headers recompute `auto_generated_id(content)` and probe `base`, `base-1`, `base-2`, …; write result and leave `attr_source.id` as `None`.
- [ ] Restructure `postprocess()` to use it: `with_header` keeps attr-extraction/linebreak handling but stops assigning ids; the routine runs as a whole-doc pass at the end of postprocess. Single source of truth for both call sites.
- [ ] Traversal scope must match current `with_header` filter reach (headers inside divs, blockquotes, list items, etc.).
- [ ] Run pampa + workspace snapshots; expected churn: only documents mixing explicit and auto ids that now probe differently (intended, Decision 5). Audit and report per the snapshot policy.

### Phase 2 — quarto-core: wire into IncludeExpansionStage

- [ ] At the tail of `run()`, if ≥1 include was expanded, run `assign_unique_heading_ids` over the assembled `Pandoc`.
- [ ] Doc-comment the stage invariant: "after this stage, heading auto ids are document-unique (uniqueIdent semantics)". Note bd-4qjl87ax as the known post-engine gap.
- [ ] Confirm coverage of all builders (native/WASM/analysis/orchestrator) via the Phase 0 integration tests where practical.

### Phase 3 — Verification & close-out

- [ ] `cargo build --workspace`, `cargo nextest run --workspace`.
- [ ] Full `cargo xtask verify` (quarto-core changed → WASM leg affected).
- [ ] E2e render inspection recorded in the session transcript (invocation + output snippet).
- [ ] Optional: re-run the site-wide duplicate-id scan on the connect-docs port to confirm 7 → 0.
- [ ] Close strand; comment on br-duplicate-heading-ids-ye3j3gkr (connect-docs skein) that the fix needs the next q2 release to verify there.
- [ ] If user opts in on the diagnostic (Decision 4): file the follow-up strand.

## Risks / tradeoffs

- **Intended id changes beyond the filed bug:** Decision 5 changes ids in documents mixing explicit and auto ids (`foo` → `foo-1` after an explicit `{#foo}`). Q1-parity is the defensible target; snapshot diffs must be audited, not rubber-stamped.
- The doc-level pass recomputes with the same `auto_generated_id` the reader used (pre-shortcode-expansion content; `Shortcode` inlines skipped by explicit decision — bd-2wv8431v), so recompute-to-same-value holds for untouched headers.
- `attr_source.id.is_none()` as the auto-id discriminator: qmd writer precedent is strong; spot-check transform-fabricated headers (none should exist pre-include-expansion, but verify).
- Duplicated-logic risk is retired by sharing one routine between postprocess and the stage (Phase 1).
- Deduped ids round-trip through the qmd writer as explicit `{#foo-1}` (writer's suppress-check compares against the recomputed base). Existing behavior; pinned in Phase 0.

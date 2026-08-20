# Heading identifiers are not disambiguated across include boundaries (bd-duplicate-heading-ids-mou5z7ux)

**Date:** 2026-08-18
**Braid:** bd-duplicate-heading-ids-mou5z7ux (p2, bug, label `markdown`)
**Checkout:** main checkout, branch `main` @ `4eaede00` at investigation time (implementation branch TBD)
**Status:** **MERGED AND CLOSED.** PR #546 merged to `main` as `736d595a` (2026-08-18); strand bd-duplicate-heading-ids-mou5z7ux closed; origin strand br-duplicate-heading-ids-ye3j3gkr (connect-docs skein) commented — its verification against the full docs port waits on the next q2 release. Open follow-ups: **bd-4qjl87ax** (engine-output headings), **bd-8wf5brc8** (duplicate explicit-id diagnostic).

## Design decisions (user-aligned, 2026-08-18)

1. **Fix shape: Option B.** Keep the reader assigning ids per-parse; add a post-include dedup pass keyed off `attr_source.id.is_none()`. User note for the future: id duplication is more general than includes (engine output, transform-fabricated headers); the scoped-pass mechanism below is designed to be re-runnable over other parts of the document later.
2. **Placement + scope: tail of `IncludeExpansionStage::run()`, scoped uniqueIdent** (user refinement, second design round). The pass uses pandoc's `uniqueIdent` *algorithm* but scopes the **renameable set** to include-injected auto headers only. The **seen-set is still document-wide** — this asymmetry is load-bearing: an inline "H" colliding with an included "H" must be detected, and only the included one may be renamed. Concretely: pre-pass collects every non-renameable id (all headers outside included files, plus explicit ids anywhere, includes included files); main pass walks include-injected auto headers in document order, probing `base`, `base-1`, `base-2`, … (set-membership, not per-base counter) and inserting each assignment into the set. Recompute the base via `auto_generated_id(content)` rather than probing the fragment-assigned id, so results are independent of the fragment's internal numbering.
   - **Why scoped beats whole-document recompute:** (a) zero behavior change outside include-injected headers — no reader change, no pampa snapshot churn; (b) **monotonicity composes**: once assigned, an id is never renamed by a later stage, so the same mechanism can run post-engine for bd-4qjl87ax (scope = engine-inserted headers) without invalidating ids `DocumentProfileStage` already observed — the tension that made bd-4qjl87ax hard under whole-doc recompute dissolves; (c) future generalization is "run the same process over a different scope", per the user's framing.
   - **Injected-ness = file-id provenance:** the stage remaps each included parse's `FileId(0)` to a fresh file id; accumulate the set of included file ids during expansion and classify headers by their `SourceInfo` file id. Covers nested includes (each registered on recursion).
   - Gate: skip the pass entirely when no include was expanded (stage stays a strict no-op).
3. **Engine-output headings: out of scope.** Filed as **bd-4qjl87ax** (discovered-from this strand); the scoped mechanism is its intended future implementation.
4. **Duplicate explicit `{#id}` diagnostic:** implementable — `AttrSourceInfo.id` is `Some(source-span)` exactly when the author wrote the id, `None` when postprocess synthesized it (qmd writer precedent at `crates/pampa/src/writers/qmd.rs:647-649`). Recommendation: follow-up strand, not folded in. **Awaiting user decision.**
5. **Dedup algorithm: pandoc `uniqueIdent` parity in mechanism, not in document-wide outcome** (user decision, second round). The probe algorithm matches pandoc exactly (set-membership over explicit + assigned ids; headers only, matching pandoc's `registerHeader`; explicit ids never renamed), but it is applied only to the scoped renameable set. Accepted divergences from Q1, stated explicitly:
   - When an included "H" *precedes* an inline "H": Q1 emits `h` (included) / `h-1` (inline); we emit `h-1` (included) / `h` (inline) — collision-free, reversed, because inline headers are never renameable.
   - No-include documents are untouched, so the existing reader quirks stay: explicit `{#foo}` then auto "Foo" still both emit `foo`; the reader's per-base counter (vs. set probe) also stays. Pure-include repetition — the filed repro and the Connect-docs hit — matches Q1 exactly.

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

quarto-core (the filed bug; new module `tests/integration/include_heading_id_dedup.rs`, registered in `main.rs`; reuses `include_expansion_diagnostics::render_fixture`). **Written 2026-08-18; all 7 behavior tests fail with the expected duplicate-id collisions; the 2 pins pass:**

- [x] Repeated include → `x`, `x-1`, `x-2` (`repeated_include_disambiguates_heading_ids` — FAILS as expected).
- [x] Nested include (`nested_repeated_include_disambiguates` — FAILS as expected).
- [x] Mixed ordering, scoped semantics (`inline_then_included_duplicate`, `included_then_inline_duplicate_renames_the_included_one` — both FAIL as expected; the latter documents the accepted Q1 divergence).
- [x] Set-probe, not counter (`probe_skips_explicitly_taken_suffix` — FAILS as expected).
- [x] Explicit `{#id}` inside an included file, included twice → kept verbatim (`explicit_id_in_included_file_kept_verbatim` — passes today, pins).
- [x] Non-injected headers never renamed (`inline_duplicates_keep_reader_ids_included_probes_past` — FAILS as expected).
- [x] No-include document unchanged (`no_include_document_keeps_reader_dedup` — passes today, pins; the byte-identical gate itself gets a unit test in Phase 2).
- [x] Profile outline sees deduped ids (`profile_outline_ids_deduped_across_includes` in `document_profile_pipeline.rs` — FAILS as expected).

pampa (pins only — the reader is deliberately untouched):

- [x] Empty-content heading base-id edge: already pinned by existing tests (`test_auto_id_empty_falls_back_to_section`, `test_auto_id_repeated_empty_headings_are_deduplicated` — `auto_generated_id` falls back to `section`, matching pandoc; nothing to add).
- [x] qmd-writer round-trip of a deduped id (`test_deduped_id_roundtrips_as_explicit_attr` in `test_heading_auto_id.rs` — passes, pins `{#setup-1}` emission).

End-to-end (per CLAUDE.md):

- [x] `cargo run --bin q2 -- render` on the committed fixture (2026-08-18, post-fix): `index.qmd` emits `id="create-the-integration"`, `-1`, `-2`; new `toc-variant.qmd` (`toc-depth: 4`) emits three distinct `data-scroll-target`s (`#create-the-integration`, `-1`, `-2`). Output inspected directly via grep on the rendered HTML.

### Phase 1 — the scoped uniqueIdent routine

- [x] Implemented as **`pampa::utils::autoid::dedup_scoped_heading_ids(doc: Pandoc, in_scope: impl FnMut(&Header) -> bool) -> Pandoc`** — home moved from the planned quarto-core location into `pampa`'s `autoid` module (deliberate improvement: pampa owns id-assignment semantics and the filter machinery; the generic scope predicate keeps include-provenance policy in the caller). Pass 1 seeds the seen-set (all non-renameable ids: explicit anywhere + out-of-scope headers); pass 2 probes renameable headers in document order (`base`, `base-1`, … set-membership), recomputing the base via `auto_generated_id`. `attr_source.id` stays `None`. Assigned via mutate-and-return-`Unchanged` (the `with_cite` precedent) — `FilterResult(_, true)` would re-apply the filter to the returned header and double-probe it.
- [x] Traversal via pampa's standard `topdown_traverse` filter — same reach as the reader's `with_header`.
- [x] Unit tests (5) in `crates/pampa/tests/integration/test_heading_auto_id.rs`: scope-only renaming, set-probe vs counter, explicit-id immunity, empty-scope identity, recompute-ignores-fragment-numbering. All pass.

### Phase 2 — wire into IncludeExpansionStage

- [x] `IncludeExpander` accumulates `injected_file_ids: HashSet<FileId>` (inserted at the splice point, per occurrence; error paths never reach it); `expand_includes_in_blocks` returns the set.
- [x] Tail of `run()`: when the set is non-empty, `dedup_scoped_heading_ids` runs over the assembled AST with the predicate `header.source_info.root_file_id() ∈ injected_file_ids`. Gate keeps no-include documents bit-identical (`no_include_document_ast_is_untouched` unit test, passes).
- [x] Stage invariant doc-comment written at the call site (uniqueIdent probe, monotonic ids, bd-4qjl87ax gap noted).
- [x] Builder coverage: native pipeline via the 8 `include_heading_id_dedup` integration tests; profile/orchestrator path via `profile_outline_ids_deduped_across_includes`. (WASM builder shares the same stage; verified by the full `cargo xtask verify` in Phase 3.)
- [x] Snapshot audit: **zero snapshot churn** — the full workspace run passed with no `.snap` file modified, exactly as predicted (no-include documents are untouched by the gate; the fixture suite has no colliding-include snapshots).

### Phase 3 — Verification & close-out

- [x] `cargo nextest run --workspace` — 12334/12334 passed (2026-08-18; build implied). Clippy + `cargo fmt --check` clean on changed crates.
- [x] Full `cargo xtask verify` — all steps passed (2026-08-18), including the hub-build/WASM legs.
- [x] E2e render inspection recorded in the session transcript and in this plan (Phase 0 End-to-end item: exact invocation + grep output).
- [ ] Optional, waits on the next q2 release: re-run the site-wide duplicate-id scan on the connect-docs port to confirm 7 → 0 (tracked on br-duplicate-heading-ids-ye3j3gkr; the minimal repro there was already re-rendered clean with the fixed binary).
- [x] Strand closed (post-merge, 2026-08-18); br-duplicate-heading-ids-ye3j3gkr commented (c-y42xkhd5).
- [x] Diagnostic follow-up filed as bd-8wf5brc8 (user opted in).

## Risks / tradeoffs

- **Accepted Q1 divergences are enumerated in Decision 5** (included-before-inline renames the included heading, not the inline one; no-include documents keep today's reader quirks). Both are deliberate; tests document them so future parity work is a conscious change, not drift.
- The pass recomputes with the same `auto_generated_id` the reader used (pre-shortcode-expansion content; `Shortcode` inlines skipped by explicit decision — bd-2wv8431v), so an injected heading with no collision recomputes to the id it already carries.
- `attr_source.id.is_none()` as the auto-id discriminator: qmd writer precedent is strong; spot-check that nothing fabricates headers with empty attr_source before include expansion (none should exist that early, but verify).
- File-id provenance as "injected": verify that every include occurrence's headers land in the accumulated file-id set (per-occurrence remap; nested recursion), and that no main-document header can share a file id with an included file (circular includes are already rejected).
- Deduped/pass-assigned ids round-trip through the qmd writer as explicit `{#h-1}` (writer's suppress-check compares against the recomputed base). Existing behavior for reader-deduped ids; pinned in Phase 0.

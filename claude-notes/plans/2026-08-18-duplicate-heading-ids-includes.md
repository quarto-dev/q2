# Heading identifiers are not disambiguated across include boundaries (bd-duplicate-heading-ids-mou5z7ux)

**Date:** 2026-08-18
**Braid:** bd-duplicate-heading-ids-mou5z7ux (p2, bug, label `markdown`)
**Checkout:** main checkout, branch `main` @ `4eaede00` at investigation time (implementation branch TBD)
**Status:** Design aligned with user 2026-08-18, refined same day to the **scoped uniqueIdent** shape (Decision 2/5 below). Awaiting implementation go-ahead; Decision 4 (diagnostic) still open.

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

quarto-core (the filed bug; extend `crates/quarto-core/tests/integration/` per the integration-test layout rule — module inside the `integration` binary):

- [ ] Repeated include → `x`, `x-1`, `x-2` (fails today).
- [ ] Nested include (a.qmd includes b.qmd twice; b carries a heading).
- [ ] Mixed ordering, scoped semantics (Decision 5): inline "H" then included "H" → included gets `h-1`; included "H" then inline "H" → **included** gets `h-1`, inline keeps `h` (documents the accepted Q1 divergence).
- [ ] Set-probe, not counter: main doc has explicit `{#h-1}`, includes "H" twice → included get `h`, `h-2` (fails today; pins uniqueIdent probing).
- [ ] Explicit `{#id}` inside an included file, included twice → both keep the id verbatim (no renaming; explicit ids are seen-set members but never renameable).
- [ ] Non-injected headers are never renamed even on collision: two inline "H" duplicates + one included "H" → inline pair keeps its reader-assigned `h`, `h-1`; included gets `h-2`.
- [ ] No-include document passes through `IncludeExpansionStage` unchanged (the gate).
- [ ] Profile outline: `DocumentProfileStage` sees deduped ids (extend `document_profile_pipeline.rs`).

pampa (pins only — the reader is deliberately untouched):

- [ ] Empty-content heading base-id edge case: check `auto_generated_id` output for empty inlines (pandoc falls back to `section`); pin whatever we decide for the probe base.
- [ ] qmd-writer round-trip: a deduped header (`h-1`) round-trips with an explicit `{#h-1}` (existing behavior for `-N` ids — pin it; applies equally to pass-assigned ids since `attr_source.id` stays `None` but the id no longer equals the recomputed base).

End-to-end (per CLAUDE.md):

- [ ] `cargo run --bin q2 -- render` on the committed fixture; inspect HTML for `x`, `x-1`, `x-2`; add a `toc-depth: 4` variant and inspect distinct `data-scroll-target`s.

### Phase 1 — the scoped uniqueIdent routine

- [ ] Implement (working name `dedup_injected_heading_ids(&mut Pandoc, injected_file_ids: &HashSet<FileId>)`) — home: `quarto-core` next to the stage, since scoping by include provenance is a pipeline concept, but keep the probe helper generic enough to re-scope for bd-4qjl87ax. Pre-pass walk collects the seen-set (all non-injected header ids + explicit ids everywhere); main walk in document order over injected headers with `attr_source.id.is_none()`: recompute `auto_generated_id(content)`, probe `base`, `base-1`, …, assign, insert into set; leave `attr_source.id` as `None`.
- [ ] Traversal scope must match the reader's `with_header` reach (headers inside divs, blockquotes, list items, footnote definitions, etc.) — use the standard filter traversal, not a top-level-blocks loop.
- [ ] Unit tests for the routine itself (scoping, probing, explicit-id immunity).

### Phase 2 — wire into IncludeExpansionStage

- [ ] Accumulate `injected_file_ids` during expansion (the `FileId(0) → new_file_id` remap already names them; nested includes covered by recursion).
- [ ] At the tail of `run()`, if the set is non-empty, run the routine over the assembled `Pandoc`.
- [ ] Doc-comment the stage invariant: "after this stage, include-injected heading auto ids are unique against the whole document (pandoc uniqueIdent probe); pre-existing ids are never renamed". Note bd-4qjl87ax as the known post-engine gap and the intended reuse of this mechanism.
- [ ] Confirm coverage of all builders (native/WASM/analysis/orchestrator) via the Phase 0 integration tests where practical.
- [ ] Snapshot audit: expected churn is **zero** outside documents with includes whose headings collide; any other churn is a red flag (snapshot policy).

### Phase 3 — Verification & close-out

- [ ] `cargo build --workspace`, `cargo nextest run --workspace`.
- [ ] Full `cargo xtask verify` (quarto-core changed → WASM leg affected).
- [ ] E2e render inspection recorded in the session transcript (invocation + output snippet).
- [ ] Optional: re-run the site-wide duplicate-id scan on the connect-docs port to confirm 7 → 0.
- [ ] Close strand; comment on br-duplicate-heading-ids-ye3j3gkr (connect-docs skein) that the fix needs the next q2 release to verify there.
- [ ] If user opts in on the diagnostic (Decision 4): file the follow-up strand.

## Risks / tradeoffs

- **Accepted Q1 divergences are enumerated in Decision 5** (included-before-inline renames the included heading, not the inline one; no-include documents keep today's reader quirks). Both are deliberate; tests document them so future parity work is a conscious change, not drift.
- The pass recomputes with the same `auto_generated_id` the reader used (pre-shortcode-expansion content; `Shortcode` inlines skipped by explicit decision — bd-2wv8431v), so an injected heading with no collision recomputes to the id it already carries.
- `attr_source.id.is_none()` as the auto-id discriminator: qmd writer precedent is strong; spot-check that nothing fabricates headers with empty attr_source before include expansion (none should exist that early, but verify).
- File-id provenance as "injected": verify that every include occurrence's headers land in the accumulated file-id set (per-occurrence remap; nested recursion), and that no main-document header can share a file id with an included file (circular includes are already rejected).
- Deduped/pass-assigned ids round-trip through the qmd writer as explicit `{#h-1}` (writer's suppress-check compares against the recomputed base). Existing behavior for reader-deduped ids; pinned in Phase 0.

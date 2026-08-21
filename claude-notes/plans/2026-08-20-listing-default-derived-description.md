# Default listing emits no description for items without explicit `description:` (bd-listing-default-no-derived-desc-m0wrr8ty)

**Date:** 2026-08-20
**Braid:** bd-listing-default-no-derived-desc-m0wrr8ty
**Branch:** `main` (investigated in the main checkout; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Reproduced at HEAD; the strand's suggested fix targets a
*symptom* — the actual root cause is that the Pass-1 head pipeline
(`pass1_profile_single_file_live`) omits `ListingItemInfoStage`, so **no
L1-autofilled field** (description, image, word-count, reading-time,
date-modified) has ever reached a listing profile. A one-line spike adding the
stage to Pass-1 makes the default template emit derived, truncated
descriptions *and* reading times. The remaining design work is deciding
whether the template-marker change the strand proposes is *also* needed
(for engine-output-first pages), and how to keep the two pipelines from
drifting again.

## Issue context

Filed 2026-08-20 by "Claude (q2-connect-docs)", P3, bug, label `listings`.
Origin strand in the q2-connect-docs skein: `br-listing-default-no-derived-desc-ywc4zvu8`.

Summary: in a `type: default` listing, an item whose page has no
`description:` front matter renders as a title-only card. Q1 derives a
description from the page's first paragraph, truncated at a word boundary to
`max-description-length` (default 175). The strand attributes this to
`item-default.template` wrapping the `$description-placeholder-begin/end$`
markers inside `$if(description)$`, so the L7 post-render substitution has
nowhere to land, and proposes (a) emitting the markers unconditionally or
(b) resolving the derived description before template evaluation.

## Dependency graph

**Empty.** `braid dep list` / `dep tree` show no edges — no `discovered-from`
parent, no dependents. No incoming pressure; P3 reflects that the Connect
docs use custom templates that happen to sidestep the bug.

Relevant history (not linked in braid): listings epic `bd-61cd`, L1 autofill
`bd-izqh` (`claude-notes/plans/2026-05-05-listings-L1-autofill-stage.md`),
L7 post-render `claude-notes/plans/2026-05-07-listings-L7-postrender-upgrade.md`.
Sibling repro strand `bd-listing-ellipsis-no-matching-l963osy1` shares the
user's repro directory.

## What the code looks like today

All paths in the strand still exist and have the described shape:

- `crates/quarto-core/src/project/listing/templates/item-default.template:32-42`
  and `item-grid.template:38-48` — markers inside `$if(description)$`. Confirmed.
- `crates/quarto-core/src/project/listing/post_render_upgrade/reader.rs:187`
  `maybe_truncate`. Confirmed.
- `crates/quarto-core/src/project/listing/item.rs:74-77` —
  `item.description = listing_item.description.or(profile.description)`.

**But the premise "the description binding is empty" has a deeper cause.**
L1 (`crates/quarto-core/src/stage/stages/listing_item_info.rs`) was designed
to fill `meta.listing-item.description` from the first `Para`/`Plain` block
*pre-checkpoint*, so `$if(description)$` would be true for any page with
body prose. It is wired into the full render pipeline
(`pipeline.rs:308`) — but listings consume **Pass-1 profiles**, produced by
`pass1_profile_single_file_live` in
`crates/quarto-core/src/project/orchestrator.rs:2150-2190`, whose stage list
is `SourceConversion → Parse → MetadataMerge → IncludeExpansion →
DocumentProfile → LinkResolution`. **No `ListingItemInfoStage`.** Checked
`git show ccb22002` (the listings merge, PR #169): the Pass-1 list never
had it. L1 autofill has been dead for listings since it shipped; its unit
tests drive the stage directly and the listing tests construct profiles by
hand, so nothing caught it.

### Head-pipeline drift (the general bug)

Two hand-maintained stage lists lead up to `DocumentProfileStage`:

| Stage | Full render pipeline (`pipeline.rs:277–318`) | Pass-1 head (`orchestrator.rs:2169–2186`) |
|---|---|---|
| `SourceConversionStage` | ✅ | ✅ |
| `ParseDocumentStage` | ✅ | ✅ |
| `MetadataMergeStage` | ✅ | ✅ |
| `LanguageResolveStage` (bd-llhlzd7p, `quarto.language`) | ✅ | ❌ |
| `IncludeExpansionStage` | ✅ | ✅ |
| `IncludeResolveStage` (writes `profile.includes`, bd-r82e) | ✅ | ❌ |
| `ListingItemInfoStage` (L1 autofill) | ✅ | ❌ |
| `DocumentProfileStage` | ✅ | ✅ |
| `LinkResolutionStage` | ✅ | ✅ |

Three pre-checkpoint stages are missing from Pass-1. Each was added to
`pipeline.rs` with a comment saying "runs before the checkpoint so X lands
in the profile", and none was mirrored. Why it's invisible:

- Pass-2 does **not** resume from the Pass-1 profile; it re-runs the full
  pipeline from source. Every document is profiled twice by two different
  pipelines. The complete Pass-2 profile feeds only that document's own
  render; the incomplete Pass-1 profile feeds the `ProjectIndex` — every
  cross-document consumer (listings, nav/sidebar, link resolution, dep
  graph, cache invalidation).
- No test crosses the seam: L1 tests drive the stage directly, listing
  tests hand-build profiles, the checkpoint test uses the full pipeline.
- The profile cache key (`cache_key.rs`) has no notion of pipeline shape;
  the manual `PROFILE_KEY_VERSION` (currently 2) must be bumped by hand when
  the head pipeline's behaviour changes.

**Decision (2026-08-21, user):** do the refactor — extract a single
`head_stages()` in `pipeline.rs` used by both the full pipeline and
Pass-1, add a shape test, bump `PROFILE_KEY_VERSION`. Native/WASM impact
survey: see §"Native vs WASM stage gating" below.

(`bd-do1nv39s` tracks verifying the `IncludeResolveStage` consequence;
`LanguageResolveStage`'s consequence — un-localized values in the index —
is unverified.)

### Repro (reproducible at HEAD)

`claude-notes/plans/listing-default-derived-desc-investigation/repro/` —
website with `index.qmd` (`listing: {contents: posts, type: default}`) and
two posts with body prose and no `description:`.

```
cargo run --bin q2 -- render claude-notes/plans/listing-default-derived-desc-investigation/repro
grep -c listing-description _site/index.html      # → 0
grep -c listing-reading-time _site/index.html     # → 0
jq '{listing_item}' .quarto/cache/profiles/<hash>  # → {"listing_item": null}
```

### Spike (reverted, not committed as code)

Adding `Box::new(crate::stage::ListingItemInfoStage::new())` between
`IncludeExpansionStage` and `DocumentProfileStage` in
`pass1_profile_single_file_live`, then re-rendering:

```html
<div class="delink listing-description">
This is the first paragraph of post A, which is long enough that it should be truncated somewhere around the one hundred and seventy-fifth character when Quarto derives a
</div>
...
<p><span class="listing-reading-time">1 min read</span></p>
```

Both items get a derived description (truncated at a word boundary ≤175)
and a reading time. Full output saved at
`claude-notes/plans/listing-default-derived-desc-investigation/spike-output-index.html`.
Output was inspected by hand.

## Proposed phases (draft)

- Phase 0 — Test plan (TDD):
  - End-to-end test through the project orchestrator: default listing,
    item with body prose and no `description:` → rendered host page contains
    `listing-description` with the first paragraph; `listing-reading-time`
    present. Must drive the real Pass-1 path (not hand-built profiles).
  - Profile-level test: Pass-1 profile for a prose page has
    `listing_item.description` / `word_count` / `reading_time_minutes` set.
  - A **pipeline-shape guard**: assert the Pass-1 head pipeline's stage list
    is a prefix of / consistent with `build_transform_pipeline`'s
    pre-checkpoint stages, so the two can't drift silently again.
  - (If Q2 below says yes) test for a page whose first block is a code cell
    with no prose: markers still emitted, L7 injects the engine-output
    paragraph.
- Phase 1 — Add `ListingItemInfoStage` to Pass-1 (ideally by extracting a
  shared `head_stages()` builder used by both pipelines rather than a
  second hand-maintained list).
- Phase 2 — (per Q2) make the description envelope unconditional in
  `item-default.template` / `item-grid.template` so L7 can land on
  engine-output-first pages; check the grid template's `[$description$]($path$)`
  link wrapper still degrades cleanly when empty.
- Phase 3 — Re-run the user's repro
  (`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/listing-ellipsis-no-matching/`)
  and diff against `_site-q1`; update `claude-notes/designs/document-profile-contract.md`
  if the head-pipeline description changes.
- Phase 4 — Docs: none expected (behaviour now matches the documented Q1 default).

## Open design questions for the user

1. **Shared head-pipeline builder vs. one-line addition.** Pass-1 and the
   full render pipeline each hand-list their pre-checkpoint stages and have
   already drifted (L1 missing; `IncludeResolveStage` also missing). Do you
   want Phase 1 to extract a single `head_stages()` used by both (bigger
   diff, prevents recurrence), or just add the one stage plus a shape-guard
   test?
2. **Also make the template markers unconditional?** With L1 in Pass-1,
   any page with static prose gets a description, but a page whose body
   starts with a code cell and has no prose paragraph still yields
   `$if(description)$ == false`, and L7's engine-output extraction can never
   land. Q1 handles that case. Should this strand cover it (strand's option
   (a)), or file it separately?
3. **`IncludeResolveStage` in Pass-1.** It writes `profile.includes` for
   bd-r82e cache invalidation but isn't in Pass-1 either. Out of scope for
   this strand — file a separate strand, or fold into the shared-builder
   work if Q1 = yes?
4. **Cache invalidation.** Existing `.quarto/cache/profiles/*` entries have
   `listing_item: null`. Does the cache key cover pipeline shape, or does
   this need a `profile_version` bump so stale profiles are discarded?

## Risks / tradeoffs (draft)

- Adding L1 to Pass-1 changes *every* project's profiles (image autofill,
  word count, date-modified via mtime) — mtime in particular makes the
  profile non-deterministic across checkouts; verify it doesn't poison
  the profile cache key or any snapshot tests.
- Pass-1 runs on rayon workers (bd-m7x9s); L1 does a filesystem `mtime`
  read per doc — cheap, but confirm it's `?Send`-compatible with the
  existing Pass-1 dispatch.
- Performance: L1 walks the AST once per doc; negligible next to parsing.
- Making markers unconditional (Q2) changes rendered HTML for items with no
  description at all (an empty `listing-description` div unless L7 strips
  it) — check L7's "no preview content" fallback path
  (`substitute.rs:412`) and Q1 parity.

## Native vs WASM stage gating (survey 2026-08-21)

Question: if Pass-1 and the full pipeline share one `head_stages()` list,
does that break either target? Answer: **no** — the head contains no
target-specific stage, and every per-target difference in the codebase is
already expressed inside stages or at the push site, never by a separate
stage list.

**Where target differences live today**

| Mechanism | Instances |
|---|---|
| Stage *excluded* on WASM at the push site (`#[cfg(not(wasm32))] stages.push(..)`) | `BootstrapJsStage` (`pipeline.rs:329`), `ClipboardJsStage` (`:336`), `TabsetsJsStage` (`:342`). Their modules/`pub use` are also gated in `stages/mod.rs:31,43,67,75,78,102`. All three are **tail** stages (after `CompileThemeCssStage`). |
| Transform excluded on WASM inside `build_transform_pipeline` | `ListingFeedStageTransform` (`:1343`), `SecondaryNavRenderTransform` (`:1384`, intentional preview/render divergence). |
| Same stage, cfg'd branch inside `run` | `metadata_merge.rs:395/397` (WASM drops missing-`css:` diagnostics), `:523/554` (trace observer vs. warn); `user_filters.rs:157/179` (`block_in_place` vs. `.await`); `code_highlight.rs:91/99` (disk grammar scan vs. built-ins); `compile_theme_css.rs:964–1017` (`grass` vs. dart-sass bridge). |
| Same stage, differs via `SystemRuntime` / registry, no cfg | `engine_execution` (WASM registry has only markdown → fallback warning unless spliced); `compile_theme_css` cache (`cache_get` is `Ok(None)` on WASM); `capture_splice` (VFS vs. disk); all file I/O in `listing_item_info`, `language_resolve`, `include_*`, `apply_template`. |
| WASM-only stages | **none**. |

**Head stages** (`SourceConversion → Parse → MetadataMerge → LanguageResolve
→ IncludeExpansion → IncludeResolve → ListingItemInfo → DocumentProfile →
LinkResolution`): `MetadataMergeStage` is the only one with a cfg inside,
and it's internal to the stage. Sharing the list changes nothing per target.

**Other stage-list builders** (for completeness; none should use `head_stages()`):

- `build_q2_preview_pipeline_stages` (`pipeline.rs:418`) and
  `build_html_pipeline_stages_with_captures` (`:478`) — derive from the
  full list by exclusion/insertion; inherit the shared head automatically.
- `preview_record.rs:120,186` — *truncate* the full list after
  `engine-execution` / `pre-engine-sugaring`; also inherit.
- `build_analysis_pipeline` (`:619`, LSP) — deliberately no includes-resolve /
  profile / engine; leave alone.
- `parse_qmd_to_ast` (`:763`, hub-client AST debug) — 3 stages, engine before
  metadata merge; leave alone.
- `get_config.rs:56` — `[Parse, MetadataMerge]` only; leave alone.
- `orchestrator.rs:2168` Pass-1 — **the one to replace** with `head_stages()`.

**Pass-1 on WASM**: hub-client drives `ProjectPipeline::with_renderer`
(`wasm-quarto-hub-client/src/lib.rs:1690,1712`), so Pass-1 runs on WASM via
`pass_one_dispatch_async` (`orchestrator.rs:1750`), calling the same ungated
`pass1_profile_single_file_live`. The profile cache is a transparent miss on
WASM (`profile_cache.rs:21-23`), so the `PROFILE_KEY_VERSION` bump matters
only natively. Net: the fix lands on both targets identically, and hub
project previews gain L1-derived listing fields too.

Cost on WASM of the three added head stages: `LanguageResolveStage` and
`IncludeResolveStage` are metadata-only; `ListingItemInfoStage` is one AST
walk + one `mtime` lookup through the runtime (WASM VFS may return none →
`date-modified` stays unset, which is the existing native-without-mtime path).

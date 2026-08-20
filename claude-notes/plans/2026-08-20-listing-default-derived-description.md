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

(`IncludeResolveStage` is also absent from Pass-1 — see Risks; possibly a
separate gap for `profile.includes` / bd-r82e cache invalidation.)

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

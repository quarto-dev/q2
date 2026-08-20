# Error-docs sidebar: backfill + lint rule (bd-wcmk1fsq)

## Overview

The error-reference sidebar in `docs/_quarto.yml` enumerates every error page
by hand. Nothing enforced that the list stayed complete, and it drifted: on
`main` at 2026-08-18 it listed **153 of 207** pages. Two subsystems had no
`- section:` block at all — `extension` (9 pages) and `crossref` (1) — so every
`Q-15-*` and `Q-16-*` page was unreachable by navigation.

The pages render and resolve by direct URL, so no diagnostic shipped a 404 and
`cargo xtask lint` stayed green. What was missing was navigation only.

This plan backfills the sidebar and adds `error-docs-sidebar-unlisted` to
`cargo xtask lint` so the hand-maintained list cannot silently drift again.

## Scope decision: hand-maintained list stays

bd-wcmk1fsq suggested the "real fix" might be a listing or generated include.
Investigated and rejected — **no automatic mechanism exists in either Quarto**:

- **Q2** supports `auto:` in sidebars (`crates/quarto-core/src/transforms/sidebar_auto.rs`);
  `group_with_subdirs` even produces exactly the section-per-subsystem shape.
  But within-section sort falls back to the document **title** (prose), not the
  code, so it cannot produce code order.
- **Q1** cannot either. It *does* merge arbitrary top-level front matter into
  listing items (`website-listing-read.ts:1184-1187`), so `sort: code` is
  accepted — but its build-time comparator is lodash `orderBy` → plain
  relational operators (`_compareAscending.js:23,30`). No `localeCompare`, no
  `{numeric: true}`. Q1 sorts `Q-1-10` before `Q-1-2`, same as Q2. Q1's
  auto-sidebar is the same story: `order:` first, then `localeCompare` on
  basename (`website-sidebar-auto.ts:313-333`).
- The only mechanism either offers is a hand-maintained numeric `order:` on
  every page. **Rejected** (Gordon, 2026-08-18): not worth a redundant
  per-page integer field.

So: hand-maintained list, backfilled, with a lint rule. Index ordering waits on
bd-otmqu.

## Work items

- [x] Measure the real drift (not just `extension`): 207 pages, 153 entries,
      54 unlisted across 9 subsystems
- [x] Confirm no automatic alternative exists in Q2 or Q1 (see above)
- [x] Write `error_docs_sidebar.rs` tests, confirm the ordering test fails
      against the membership-only implementation
- [x] Implement `error-docs-sidebar-unlisted` with three problem classes:
      unlisted page, stale entry, out-of-order entry within a section
- [x] Register it in `crates/xtask/src/lint/mod.rs` as a repo-level check
- [x] Verify the rule reports all 54 real violations before the backfill
- [x] Backfill the sidebar: 54 entries added, `crossref` and `extension`
      sections appended → 207 entries, 15 sections
- [x] Confirm within-section numeric order (already correct in all 15 sections;
      no resequencing needed — see note below)
- [x] Prove the ordering check binds: inject a swap of `Q-2-9`/`Q-2-10`,
      confirm it is flagged at the right line, revert
- [x] Document the rule in `CLAUDE.md` and `docs/errors/README.md`
      (including the new step 4 in "Adding a new page")
- [x] `cargo xtask lint` green; `cargo nextest run -p xtask` 118/118
- [x] E2E: `cargo run --bin q2 -- render docs/` — all 15 sections present in
      the rendered sidebar
- [x] `cargo clippy -p xtask --all-targets` clean — the first `verify` run
      failed on `clippy::collapsible_if` in the new ordering check (`-D warnings`
      makes it an error). Collapsed to a let-chain. `cargo xtask lint` and
      `cargo nextest run -p xtask` had both passed at that point, which is the
      case CLAUDE.md's step 4 exists for.
- [x] Full `cargo xtask verify` green — all 14 steps, 12182/12182 tests.
      Two environmental blockers on the way, neither from this change: the
      fresh worktree had no `node_modules` (ts-packages `Cannot find module
      '@quarto/mapped-string'`), fixed by `npm install`; and under
      `--skip-hub-build` the preview-renderer integration tests still run while
      the `wasm-quarto-hub-client` artifact they import is not built. The
      shared-package tests have their own `skip_shared_package_tests` gate, so
      `--skip-hub-build` alone leaves an unsatisfiable leg in a cold worktree.
      Not touched here; noted in case it is worth a strand.
- [x] Commit
- [x] Rebase onto `feature/ts-engine-extensions` and ff-merge into it.
      **The rule immediately earned its keep on rebase:** the feature branch
      carries 211 error pages to main's 207, and all four extras were unlisted —
      `Q-2-50` (added by the merge runbook's B3 step) and `Q-16-10/11/12` (added
      by bd-exhbc6h8). Both pieces of in-flight work added error pages without
      sidebar entries, which is exactly the drift this rule exists to stop.
      Backfilled those four; sidebar is 211/211 on the feature branch.
- [ ] Push — **needs explicit approval** (not yet requested)

## Note: the sections were already in numeric order

The course-correction brief expected churn, on the premise that `yaml` had
`Q-1-10`..`Q-1-29` sitting before `Q-1-2`. It does not — **`yaml` has no
`Q-1-2` through `Q-1-9`.** Its codes are `1, 10..29, 99`, so
`Q-1-1, Q-1-10, Q-1-11, …` *is* ascending; it only looks lexicographic.

Verified across all 15 sections: zero were out of numeric order. The backfill
script sorted merged entries numerically and the result matched the existing
order, which is why `docs/_quarto.yml` came out **purely additive (+58/−0)**.
Consequently the ordering check passes trivially on the real tree — hence the
injected-drift step above, so the check is known to bind rather than assumed to.

## Deliberately out of scope

- **Section ordering.** The 13 historical sections keep their arbitrary order;
  `crossref` and `extension` are appended. No canonical section order has been
  agreed, and reordering would churn every section for no reader benefit.
- **`order:` front matter and `listing-item: extra:`.** Both rejected.
- **`docs/errors/index.qmd` and its `sort:` key.** That is bd-otmqu.
- **Any `quarto-core` change.**

## Discovered, filed elsewhere (bd-otmqu)

`docs/errors/index.qmd` renders its **Code, Subsystem and Status columns
empty** — all 207 rows; only Title (a built-in) appears. No `filter-ui`,
`sort-ui` or category markup is emitted either.

One root cause: top-level front-matter keys never reach `ListingItem`.
`item.extra` is populated only from an explicit `listing-item: extra:` block
(`document_profile.rs:200-213`), and both the sort resolver (`sort.rs:117`) and
the table-column resolver (`binding.rs:586`) fall through to that same map.
Reproduced E2E with a scratch project: a bare `code:` key emits
`Warning [Q-12-3]: Unknown sort field code` and renders empty cells; adding
`listing-item: extra:` fixes both.

This is a genuine Q1-parity gap and was recorded as a comment on bd-otmqu,
recommending the strand widen from "sort" to "arbitrary front matter must reach
listing items for sort **and** display". Recorded there too: **do not** add a
numeric-aware build-time comparator to Q2 as part of it — Q1 has none, so that
would be divergence rather than parity, and needs arguing on its own merits.

# Sidebar `contents: <dir>` shorthand only recognizes `index.qmd` (bd-sidebar-dir-index-md-5khf3lds)

**Date:** 2026-08-19
**Braid:** bd-sidebar-dir-index-md-5khf3lds
**Branch:** `main` @ `f387bd68` (investigation committed in place; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The strand's root-cause analysis is confirmed accurate at HEAD, the fix is small and localized (one function, `section_for_dir` in `crates/quarto-core/src/transforms/sidebar_auto.rs`), and the repro reproduces. Two design questions below (tie-break order, search domain) are the only open decisions.

## Issue context

Filed 2026-08-19 by Carlos Scheidegger, priority 2, type bug, label `navigation`. The sidebar `contents: <directory>` shorthand promotes the directory's landing page to the section header only when it is `index.qmd`. With `index.md` — discoverable since `.md` became a first-class input (bd-6d2wj4zp) when matched by an explicit `project.render` pattern — the landing page is neither promoted (header falls back to capitalized dir name, no href) nor excluded from the child list. Prev/next pagination shifts as a consequence.

Real-world hit: the Posit Connect docs (`contents: how-to`, all landing pages `.md`) — the last remaining sidebar difference in the 451-page port. Origin strand in the connect-docs porting skein: `br-l18qnflo`, follow-up to the dir-shorthand feature bd-sidebar-contents-dir-shorthand-z7arvhx8.

## Dependency graph

The graph is **empty in this skein** — no edges. Context instead comes from the strand description itself, which names:

- **bd-sidebar-contents-dir-shorthand-z7arvhx8** (in_progress): the feature that introduced `AutoSpec::Path` dir-shorthand expansion and, with it, `section_for_dir`. Its plan recorded a known limitation about nesting depth; the hardcoded extension was a second, quieter MVP shortcut ("Only `.qmd` is discoverable by Phase-1 project walking; that's fine for MVP") whose premise `.md` render support later invalidated.
- **br-l18qnflo**: the connect-docs skein origin (not resolvable from this skein; context is summarized in the description).

## What the code looks like today

Confirmed at HEAD (= v0.24.0):

- `crates/quarto-core/src/transforms/sidebar_auto.rs:356` — `let index_src = format!("{}/index.qmd", dir);` with the stale MVP comment. Drives both the header promotion (`lookup_by_source`) and the child-exclusion filter at line 383. This is the **only** production occurrence of the hardcoded pattern (all other `index.qmd` hits in the workspace are test fixtures).
- `section_for_dir` is called only from `group_with_subdirs` (line 347), which serves **both** `auto: true` (`Scope::Grouped`) and the bare-directory shorthand (`AutoSpec::Path` + `is_bare_directory`). One fix covers both entry points.
- The stale premise: `crates/quarto-core/src/project/discovery.rs` has `FIXED_RENDERABLE = &["qmd", "md"]` — `.md` profiles do land in the `ProjectIndex` when an explicit `project.render` pattern matches them.
- Neighboring code already does stem-based matching: `is_top_level_index` (line 300) checks `file_stem().eq_ignore_ascii_case("index")` — a model for the fix.
- Q1 parity reference: `indexFileHrefForDir` in `external-sources/quarto-cli/src/project/types/website/website-sidebar-auto.ts:234` probes `index<ext>` over `engineValidExtensions()` **in order** and takes the first that exists — extension-agnostic with a deterministic preference order.

**Repro:** `claude-notes/plans/sidebar-dir-index-md-investigation/repro/` — a website with `sidebar.contents: guides`, `guides/index.md` ("The Guides Landing Page"), `guides/alpha.qmd`, `guides/beta.qmd`, and `project.render` including `**/*.md`. Expected (Q1): section header "The Guides Landing Page" (href to the landing page) with children Alpha Guide, Beta Guide. Actual at HEAD: header "Guides", no href, landing page listed as a third child. See `RESULTS.md` in the investigation dir for the observed render output.

## Proposed fix (sketch)

In `section_for_dir`, resolve the directory's index profile **by stem** instead of by hardcoded extension: find the profile whose source path is a *direct* child of `dir` with file stem `index` (any extension the `ProjectIndex` admitted). Use that resolved source string for both the header href and the child-exclusion filter — both symptoms are one fix. Nested `dir/sub/index.md` must not match.

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD, failing tests first).** Unit tests in `sidebar_auto.rs`'s test module (helpers `make_profile` et al. already exist):
  - `index.md` in a bare-dir shorthand → promoted to section header (title + href), excluded from children.
  - Same under `auto: true` grouping.
  - `dir/sub/index.md` is **not** promoted for `dir`'s section.
  - Tie-break: dir with both `index.qmd` and `index.md` → deterministic winner (per design Q1).
  - Pagination-adjacent assertion: child list ordering/count with `.md` index excluded (prev/next is downstream of the entry list, so entry-level tests cover it).
- **Phase 1 — Implement** stem-based index resolution in `section_for_dir` (single function; delete the stale MVP comment).
- **Phase 2 — End-to-end verification.** `cargo run --bin q2 -- render` on the investigation repro; inspect `#quarto-sidebar` in `guides/alpha.html`; confirm rename-to-`.qmd` equivalence. Spot-check the Connect docs site if available. Full workspace test suite + snapshot audit (report any `.snap` deltas).
- **Phase 3 — Docs.** Likely none needed (this is parity, not new surface); confirm `docs/` doesn't document the `.qmd`-only limitation anywhere.

## Open design questions for the user

1. **Tie-break when both `guides/index.qmd` and `guides/index.md` exist.** Q1 uses `engineValidExtensions()` order (first existing wins). Options: (a) fixed preference order `qmd` > `md` (matches Q1's practical outcome, deterministic, cheap); (b) `ProjectIndex` insertion order (simplest but order is walk-dependent). Recommendation: (a).
2. **Search domain: `members` vs the whole `ProjectIndex`.** Today the header lookup goes through `index.lookup_by_source` (whole index) while children come from `members` (draft-filtered, matcher-filtered candidates). Resolving the index within `members` instead is simpler, guarantees the promoted page actually belongs to the section, and — as a side effect — stops a **draft** `index.qmd` from being promoted to a linked header (today it would be, since drafts are filtered out of candidates but not out of the lookup). Is that draft-behavior change desirable (I believe yes — Q1 doesn't link drafts), or should we preserve the whole-index lookup?
3. **Case sensitivity of the stem match.** `is_top_level_index` matches `Index.qmd` case-insensitively (`eq_ignore_ascii_case`). Should `section_for_dir` do the same for consistency? (Href would still use the actual path.) Recommendation: yes, mirror `is_top_level_index`.

## Risks / tradeoffs (draft)

- Snapshot exposure: website-render snapshot tests that exercise sidebars may shift if any fixture has a directory `index.md`; expected to be zero or small, but the snapshot-audit step in Phase 2 covers it.
- The draft-index behavior change in design Q2 (if accepted) is a subtle semantic change beyond the reported bug; it should be called out in the commit message and covered by its own test.
- No cross-crate surface: `quarto-navigation`'s `sidebar.rs` parses the shorthand; expansion lives entirely in `quarto-core`. WASM leg is in scope for `cargo xtask verify` (change is under `quarto-core`).

# Sidebar `contents: <dir>` shorthand only recognizes `index.qmd` (bd-sidebar-dir-index-md-5khf3lds)

**Date:** 2026-08-19
**Braid:** bd-sidebar-dir-index-md-5khf3lds
**Branch:** `main` @ `f387bd68` (investigation committed in place; no worktree created)
**Status:** Design questions answered by user 2026-08-19 — approved for TDD implementation. See "Design decisions" below.

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

## Design decisions (answered by user, 2026-08-19)

1. **Tie-break when both `guides/index.qmd` and `guides/index.md` exist:** fixed preference order, `qmd` > `md` (> anything else, in member order). User: "might need tweaking, but for now it'll do."
2. **Search domain:** resolve the index within `members` (the draft-filtered, matcher-filtered candidates), not the whole `ProjectIndex`. Side effect accepted: a **draft** directory index is no longer promoted to a linked section header. User also flagged the broader question — does q2 have a *structural* mechanism preventing draft pages from being linked anywhere? — delegated to a study agent; its outcome (summary or new-strand recommendation) is tracked separately from this fix.
3. **Case sensitivity:** the stem match is **case-sensitive** (`stem == "index"` exactly). User's call: case-insensitive matching invites trouble across case-sensitive vs case-preserving/insensitive filesystems (macOS). Note this deliberately diverges from `is_top_level_index`'s `eq_ignore_ascii_case`; the broader case-handling inconsistency is filed as discovered work (low priority) rather than fixed here.

## Work items

- [x] Phase 0 — failing tests written and observed to fail (6 new tests in `sidebar_auto.rs`; `auto_bare_directory_section_uses_md_dir_index`, `auto_all_promotes_md_dir_index`, `auto_draft_dir_index_is_not_promoted` failed at HEAD exactly as predicted; the nested/tie-break/case-sensitivity guards passed at HEAD by design)
- [x] Phase 1 — stem-based index resolution in `section_for_dir` (members domain, direct-child only, case-sensitive, `.qmd` > `.md` > other preference; `index: &ProjectIndex` param dropped from `section_for_dir`/`group_with_subdirs`; all 24 sidebar_auto tests pass)
- [x] Phase 2 — full workspace tests + snapshot audit (12,902 passed, 0 failed; **zero `.snap` files changed**)
- [x] Phase 2 — end-to-end repro render verified: `cargo run --bin q2 -- render claude-notes/plans/sidebar-dir-index-md-investigation/repro`, inspected `_site/guides/alpha.html` — header is `<a href="index.html">The Guides Landing Page</a>`, children exactly Alpha/Beta. See RESULTS.md "Post-fix verification".
- [x] File discovered-work strand: case-sensitivity inconsistency → **bd-0yp90370** (p3)
- [x] Draft-link study agent outcome recorded → audit at `claude-notes/research/2026-08-19-draft-visibility-audit.md`; recommendation accepted, filed **bd-zeormbsa** (p2, "Centralize draft visibility: a single is_linkable predicate on ProjectIndex"), linked related to bd-w0o9 / bd-p4sc / bd-4zdf. Headline: 25 ProjectIndex access sites, only 6 draft-checked; live leaks in sitemap, listings/RSS, body links, explicit nav items.

## Risks / tradeoffs (draft)

- Snapshot exposure: website-render snapshot tests that exercise sidebars may shift if any fixture has a directory `index.md`; expected to be zero or small, but the snapshot-audit step in Phase 2 covers it.
- The draft-index behavior change in design Q2 (if accepted) is a subtle semantic change beyond the reported bug; it should be called out in the commit message and covered by its own test.
- No cross-crate surface: `quarto-navigation`'s `sidebar.rs` parses the shorthand; expansion lives entirely in `quarto-core`. WASM leg is in scope for `cargo xtask verify` (change is under `quarto-core`).

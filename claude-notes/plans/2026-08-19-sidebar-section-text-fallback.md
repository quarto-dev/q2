# Sidebar item with `text:` + `file:` + `contents:` renders the page title, ignoring `text:` (bd-sidebar-section-text-ignored-sdp5g7ns)

**Date:** 2026-08-19
**Braid:** bd-sidebar-section-text-ignored-sdp5g7ns
**Checkout:** `main` @ `e6ac236d` (investigation ran in the main checkout; no worktree created)
**Status:** Complete (2026-08-19). Fix in `a93b908e`, Q-13-10 warning in `5c9ee27d`, verification evidence in `sidebar-section-text-fallback-investigation/observed-output.md`. Full `cargo xtask verify` green.

## Triage verdict

**Ready to design.** The bug is confirmed at HEAD by code reading, the root cause named in the strand is accurate, and the fix is a one-expression change with clear test seams; the only open questions are small behavioral edge cases (precedence and Q1 parity details) listed below.

## Issue context

Filed today (2026-08-19) by Carlos, priority 2, type bug, label `navigation`. A sidebar entry with `text:` + `file:` (or `href:`) + `contents:` should display the configured `text:` (Q1 behavior for section-with-landing-page items), but q2 shows the linked page's title instead. Real-world impact: the Posit Connect docs port's Cookbook sidebar shows "Posit Connect Cookbook" instead of "Cookbook" across ~109 pages. Observed on q2 0.19.0 through 0.24.0.

The strand already contains a root-cause analysis and a suggested fix, both of which this investigation confirmed (below).

## Dependency graph

**Empty.** `braid dep tree` shows only the strand itself; `braid dep list` shows no edges. The origin strand (`br-sidebar-section-text-ignored-yr34ofdu`) lives in the separate connect-docs porting skein, not this one, so no in-skein `discovered-from` edge exists. No incoming pressure, but the strand description itself carries the discovery context.

## What the code looks like today

Two cooperating sites produce the symptom — both verified at `e6ac236d`:

1. **Parse drops `text:`** — `SidebarEntry::from_config_value`, `crates/quarto-navigation/src/sidebar.rs:229-236`. Any object with `contents:` (or `section:`) takes the Section branch, and Section display text is read from the `section:` key *only*:
   ```rust
   let section_text = cv.get("section").cloned();
   ...
   let text = section_text.filter(|v| v.as_plain_text().is_some());
   ```
   `text:` is never consulted, so the Section comes out with `text: None`.

2. **Enrichment fills the gap with the page title** — `enrich_text_from_index`, `crates/quarto-core/src/transforms/sidebar_generate.rs:266-275`. A Section with `text: None` and an href gets its text from the project index profile's title. That fallback is correct and desirable for genuinely text-less sections (`href:` + `contents:` only); it just should never fire for entries that *did* configure a `text:`.

Adjacent facts that shape the fix:

- **`text:` + `file:` without `contents:`** skips the Section branch entirely, parses as a leaf `Link`, and renders correctly today (confirmed in strand and by reading the branch order at `sidebar.rs:235`).
- **The Heading branch** (`sidebar.rs:265-273`, `text:` with no link bits) sits *after* the Section branch, so adding a `text:` fallback inside the Section branch can't shadow it.
- **Serialization normalizes to `section:`** — `SidebarEntry::to_config_value` (`sidebar.rs:315-321`) always emits Section text under the `section:` key. After the fix, a `text:`-authored section roundtrips to the `section:` spelling, which reparses to an identical Section. Entry-level roundtrip stability holds after one normalization; the existing roundtrip test (`roundtrip_sidebar_to_config_value`, `sidebar.rs:1108`) constructs from Rust values and is unaffected.

The suggested fix from the strand is right:

```rust
let text = section_text
    .or_else(|| cv.get("text").cloned())
    .filter(|v| v.as_plain_text().is_some());
```

**Repro:** copied (sources only) to `claude-notes/plans/sidebar-section-text-fallback-investigation/repro/` from the connect-docs repro. **Confirmed end-to-end at `e6ac236d`**: `cargo run --bin q2 -- render` on a scratch copy renders the section label as "The Much Longer Landing Page Title" instead of the configured "Short Name", while the contents-less "Plain Item" entry renders correctly. Observed markup captured in `sidebar-section-text-fallback-investigation/observed-output.md`.

## Design decisions (settled with user, 2026-08-19)

1. **Precedence: `section:` wins, plus a new warning.** Q1's `normalizeSidebarItem`
   (`external-sources/quarto-cli/src/project/project-config.ts:47-69`) does
   `item.text = section` when `section:` is present and is not an existing file path —
   unconditionally overwriting any author-supplied `text:`. So `section:`-wins matches Q1,
   and the draft `.or_else` fallback is correct. Per user decision we additionally emit a
   **new warning (next free code: Q-13-10)** when an entry carries *both* keys, since q2
   can diagnose what Q1 silently clobbers.
   - Emission-site constraint: `SidebarEntry::from_config_value` is diagnostic-free and
     must stay that way (it's called from reparse paths and per-page; quarto-navigation
     deliberately has no quarto-error-reporting dependency). Emit from
     `SidebarGenerateTransform` in quarto-core, modeled on Q-13-5/Q-13-6 in
     `crates/quarto-core/src/transforms/sidebar_auto.rs:37-49`.
   - **Correction to the earlier "once per project" assumption** (verified empirically
     2026-08-19): the existing convention is **per-page emission for the picked sidebar**.
     A no-match `auto:` on a 4-page site emits Q-13-6 four times at HEAD; the
     per-sidebar-diags dance in `sidebar_generate.rs:98-146` only prevents *discarded*
     sidebars from warning. There is no project-level diagnostic dedup machinery anywhere
     in quarto-core. Q-13-10 follows the same convention (picked sidebar, per page) for
     consistency; the systemic repetition issue is filed separately (see below).
   - Mechanism: a pure scanner in quarto-navigation next to the parser
     (`section_text_conflicts`-style, returning the conflicting entries' source infos and
     label texts, grouped per sidebar to match `parse_list_from_config` indexing — sidebar
     shape knowledge stays in the crate that owns the shape), consumed by
     `sidebar_generate.rs`, which builds the `DiagnosticMessage`s (with
     `.with_location(...)` on the ignored `text:` value, cf. `toc_location.rs:160-172`)
     and pushes them into the existing per-sidebar diags vectors.
   - Lint obligations in the same commit: catalog entry in
     `crates/quarto-error-catalog/error_catalog.json`, docs page
     `docs/errors/navigation/Q-13-10.qmd`, sidebar entry in `docs/_quarto.yml`
     (`error-docs-page-missing` + `error-docs-sidebar-unlisted` enforce this).
2. **`text:` as inlines: already consistent with Q1 — no change.** Q1 renders sidebar
   item/section text as markdown inlines via the navigation markdown pipeline
   (`sidebarContentsHandler` in `website-navigation-md.ts` feeds `item.text` through
   Pandoc and splices rendered HTML back in). q2 already does the same:
   `as_plain_text()` (`config_value.rs:675-684`) accepts `PandocInlines` (it's a shape
   check, not a formatting restriction), the Section keeps the full inline structure, and
   `render_text` (`render_html.rs:892-894`) renders it via `inlines_to_html`. Verified
   end-to-end at HEAD: `text: "*Plain* Item"` renders `<span class="menu-text"><em>Plain</em> Item</span>`.
   The fix should carry a regression test that a `text:`-fallback Section preserves
   formatting (my earlier design-question premise that the filter drops formatted values
   was wrong).
3. **Verification depth: full `cargo xtask verify`** (quarto-navigation is in
   hub-client's WASM dependency closure).

## Work items

Each phase is TDD-internally: its tests are written and observed failing before its
implementation. Warning tests live at the head of Phase 2 (they need the scanner API to
exist to compile, so they can't precede Phase 1 usefully).

### Phase 0 — Parse + pipeline tests (written first, observed failing)

- [x] Unit test in `sidebar.rs`: `text:` + `file:` + `contents:` parses as `Section` with `text: Some(…)` and the file as `href` (currently fails).
- [x] Unit test in `sidebar.rs`: `section:` wins when both `section:` and `text:` are present.
- [x] Unit test in `sidebar.rs`: `text:`-fallback Section preserves `PandocInlines` formatting (not flattened to plain text).
- [x] Integration test in `crates/quarto-core/tests/integration/sidebar_pipeline.rs`: rendered sidebar HTML shows the configured `text:`, not the linked page's title.
- [x] Run the new tests; record the expected failures. *(Observed 2026-08-19: `parse_sidebar_text_with_contents_is_section_with_text` and `parse_sidebar_text_fallback_preserves_inlines` fail on `text: None`; `parse_sidebar_section_key_wins_over_text` passes trivially at HEAD as a precedence lock; `pipeline_section_with_text_key_shows_configured_text` fails with the sidebar rendering `sidebar-link">The Much Longer Landing Page Title</a>` — the exact bug symptom.)*

### Phase 1 — Parse fix

- [x] `.or_else(|| cv.get("text").cloned())` in the Section branch of `SidebarEntry::from_config_value`.
- [x] Phase 0 tests pass; full workspace tests green (12,863 passed); commit.

### Phase 2 — Q-13-10 warning

- [x] Scanner tests in quarto-navigation (`section_text_conflicts_per_sidebar`: both-keys flagging, per-sidebar grouping, nested contents recursion) — written first, observed failing to compile (API absent).
- [x] Scanner implementation in `crates/quarto-navigation/src/sidebar.rs` (`SectionTextConflict` + `section_text_conflicts_per_sidebar`, exported from lib.rs).
- [x] Emission tests in `sidebar_generate.rs`: both-keys entry produces Q-13-10 naming both labels; conflict in an *unselected* sidebar does not warn — positive test observed failing first.
- [x] Catalog entry Q-13-10 in `crates/quarto-error-catalog/error_catalog.json` + emission in `sidebar_generate.rs` with `.with_location()` on the ignored `text:` value.
- [x] Docs page `docs/errors/navigation/Q-13-10.qmd` + sidebar entry in `docs/_quarto.yml` (lint-enforced, same commit).
- [x] `cargo xtask lint` green; full workspace tests green (12,868 passed); commit.

### Phase 3 — End-to-end verification

- [x] `cargo run --bin q2 -- render` on the investigation repro; inspect `_site/*.html` sidebar markup (configured text, formatted-inlines case, Q-13-10 warning on a both-keys fixture). Evidence in `sidebar-section-text-fallback-investigation/observed-output.md`.
- [x] Full `cargo xtask verify` (no skips) — all 14 steps green. *(One environment repair along the way, unrelated to this change: `node_modules/@esbuild/darwin-arm64` had gone missing after this session's earlier `npm install`, failing quarto-hub-mcp's bundle test; fixed with `npm install --no-save @esbuild/darwin-arm64@0.28.0`.)*
- [x] `cargo run --bin q2 -- render docs/` renders the site including the new Q-13-10 page (253/253 files; `errors/navigation/Q-13-10.html` present with correct title; the 40 render warnings are pre-existing docs-site issues, none referencing this change).
- [x] Commit (plan + investigation notes updates).

### Phase 4 — Close out

- [x] Update this plan's status; close the strand with `braid close`.

## Risks / tradeoffs (draft)

- Very low risk: single-expression parse change, well-fenced by branch order (Link and Heading branches unaffected).
- The `section:`-normalizing serialization means `text:`-authored config changes spelling across a roundtrip. No current consumer is known to care (reparse is stable), but any future "write config back to YAML" feature would rewrite user spelling. Not worth acting on now; noting for the record.
- The Q-13-10 warning's emission site must not be the parser (reparse paths would double-emit). Per-page repetition turned out to be a non-issue for this code: `coalesce_by_source` (bd-mg3ckvp7, `render.rs:1250-1272`) groups per-page diagnostics that share a source location, and Q-13-10 carries one — a 4-page site prints the warning once with an "Affected files:" tail (verified end-to-end). Q-13-5/Q-13-6 repeat per page only because they carry no location; that observation moved to bd-drdx1pew.
- Discovered parity gap, filed separately: Q1's `section:` value doubles as a *file path* (`project-config.ts:54-59` — if the value names an existing file it becomes `href`, and the author's `text:` survives). q2 treats `section:` purely as display text. Filed as bd-byrb9yqi (discovered-from this one).

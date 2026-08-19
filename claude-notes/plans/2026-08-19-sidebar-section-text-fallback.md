# Sidebar item with `text:` + `file:` + `contents:` renders the page title, ignoring `text:` (bd-sidebar-section-text-ignored-sdp5g7ns)

**Date:** 2026-08-19
**Braid:** bd-sidebar-section-text-ignored-sdp5g7ns
**Checkout:** `main` @ `e6ac236d` (investigation ran in the main checkout; no worktree created)
**Status:** Design settled with user (2026-08-19); see "Design decisions" below. Ready to implement pending user go-ahead on this revision.

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
     must stay that way (it's called from reparse paths and per-page). Emit from a
     quarto-core transform holding the `&mut Vec<DiagnosticMessage>` sink, modeled on
     Q-13-5/Q-13-6 in `crates/quarto-core/src/transforms/sidebar_auto.rs:37-49`, and fire
     **once per project, not once per page** (see the Q-13-6 dedup guard test at
     `sidebar_generate.rs:456-476`). Likely site: wherever the sidebar ConfigValue is
     first scanned project-wide; detect the key conflict on the raw ConfigValue.
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

## Proposed phases

- **Phase 0 — Test plan (TDD).**
  - Unit test in `sidebar.rs` tests: `text:` + `href:`/`file:` + `contents:` parses as `Section` with `text: Some(…)` (currently fails).
  - Unit test: `section:` wins when both `section:` and `text:` are present.
  - Unit test: `text:`-fallback Section preserves `PandocInlines` formatting.
  - Warning test: both-keys entry emits Q-13-10 exactly once per project (model: Q-13-5 test in `sidebar_auto.rs:858-881`, Q-13-6 dedup test in `sidebar_generate.rs`).
  - Pipeline/integration test in `crates/quarto-core/tests/integration/sidebar_pipeline.rs`: rendered sidebar HTML shows the configured `text:`, not the page title.
- **Phase 1 — Parse fix.** `.or_else(|| cv.get("text").cloned())` in the Section branch of `SidebarEntry::from_config_value`.
- **Phase 2 — Q-13-10 warning.** Catalog entry + emission in quarto-core + docs page `docs/errors/navigation/Q-13-10.qmd` + `docs/_quarto.yml` sidebar entry, all in one commit (lint-enforced).
- **Phase 3 — End-to-end verification.** `cargo run --bin q2 -- render` on the investigation repro; inspect `_site/*.html` sidebar markup; full `cargo xtask verify`.
- **Phase 4 — Close out.** Close the strand.

## Risks / tradeoffs (draft)

- Very low risk: single-expression parse change, well-fenced by branch order (Link and Heading branches unaffected).
- The `section:`-normalizing serialization means `text:`-authored config changes spelling across a roundtrip. No current consumer is known to care (reparse is stable), but any future "write config back to YAML" feature would rewrite user spelling. Not worth acting on now; noting for the record.
- The Q-13-10 warning's emission site must not be the parser (reparse paths would double-emit) and must dedup across pages; this is the only genuinely fiddly part of the change.
- Discovered parity gap, filed separately: Q1's `section:` value doubles as a *file path* (`project-config.ts:54-59` — if the value names an existing file it becomes `href`, and the author's `text:` survives). q2 treats `section:` purely as display text. Filed as bd-byrb9yqi (discovered-from this one).

# Sidebar item with `text:` + `file:` + `contents:` renders the page title, ignoring `text:` (bd-sidebar-section-text-ignored-sdp5g7ns)

**Date:** 2026-08-19
**Braid:** bd-sidebar-section-text-ignored-sdp5g7ns
**Checkout:** `main` @ `e6ac236d` (investigation ran in the main checkout; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD).**
  - Unit test in `sidebar.rs` tests: object with `text:` + `href:`/`file:` + `contents:` parses as `Section` with `text: Some("…")` (currently fails).
  - Unit test: `section:` wins over `text:` when both present (or whatever precedence Q1 has — see design question 1).
  - Pipeline/integration test in `crates/quarto-core/tests/integration/sidebar_pipeline.rs`: rendered sidebar HTML shows the configured `text:`, not the page title (exercises the enrichment interplay).
- **Phase 1 — Fix.** One-expression change in the Section branch of `SidebarEntry::from_config_value`.
- **Phase 2 — End-to-end verification.** `cargo run --bin q2 -- render` on the investigation repro; inspect `_site/*.html` sidebar markup. Full workspace tests + `cargo xtask verify --skip-hub-build` (quarto-navigation is WASM-reachable via quarto-core, so consider full verify — see design question 3).
- **Phase 3 — Close out.** Changelog entry if the repo convention calls for one; close the strand.

No docs phase expected: `docs/` documents `text:` as a sidebar-item key already; this is a conformance fix, not a new feature.

## Open design questions for the user

1. **Precedence when both `section:` and `text:` are present.** The draft gives `section:` priority (`.or_else`). Q1 accepts both spellings; do we know (or care to match) what Q1 does when an author writes *both* on one entry? Straw answer: `section:` wins, matching the draft — but if you'd rather diagnose the conflict (a Q-code warning), that grows the change.
2. **Non-plain-text `text:` values.** The filter drops a `text:` whose value isn't plain text (e.g. formatted inlines), same as `section:` today — the entry falls back to page title silently. Fine to keep the existing silent behavior for both spellings, or worth a diagnostic while we're here? Straw answer: keep as-is, file a separate strand if desired.
3. **Verification depth.** `quarto-navigation` feeds the WASM preview path. Plain `--skip-hub-build` verify, or full `cargo xtask verify` before commit? Straw answer: full verify, since the crate is in hub-client's dependency closure.

## Risks / tradeoffs (draft)

- Very low risk: single-expression parse change, well-fenced by branch order (Link and Heading branches unaffected).
- The `section:`-normalizing serialization means `text:`-authored config changes spelling across a roundtrip. No current consumer is known to care (reparse is stable), but any future "write config back to YAML" feature would rewrite user spelling. Not worth acting on now; noting for the record.

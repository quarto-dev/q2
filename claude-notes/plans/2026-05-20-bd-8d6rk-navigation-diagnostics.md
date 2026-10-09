---
title: 'bd-8d6rk — Navigation diagnostics: structured warnings with codes + locations'
date: 2026-05-20
description: 'Replaces the plain-text navigation warnings for missing documents and unmatched sidebar `auto:` entries with structured diagnostics that carry codes Q-13-1 through Q-13-6, problem text, and hints.'
---

**Status**: Draft, awaiting user iteration
**Issue**: bd-8d6rk (P2, task)
**Follow-up**: bd-qor9a (blocked by this) — fills in source locations once the
sidebar/navbar parser stops stripping `SourceInfo`.

## Context

Today the missing-document and dropped-`auto:` warnings in the navigation
pipeline are emitted as opaque title strings:

`crates/quarto-core/src/transforms/navigation_href.rs:92-96`
```rust
diagnostics.push(DiagnosticMessage::warning(format!(
    "{} references missing document information for '{}'",
    tag, path_part
)));
```

`crates/quarto-core/src/transforms/navigation_href.rs:236-242`
```rust
diagnostics.push(DiagnosticMessage::warning(format!(
    "{} references missing document information for '{}'",
    tag, project_relative
)));
```

`crates/quarto-core/src/transforms/sidebar_auto.rs:95-100`
```rust
diagnostics.push(DiagnosticMessage::warning(
    "Sidebar `auto:` entry ignored — no project index is available. \
     This usually means the document is being rendered standalone, \
     not as part of a project."
        .to_string(),
));
```

`crates/quarto-core/src/transforms/sidebar_auto.rs:133-136`
```rust
diagnostics.push(DiagnosticMessage::warning(format!(
    "Sidebar `auto:` matched no documents (spec: {})",
    auto_spec_debug(spec)
)));
```

`crates/quarto-core/src/transforms/sidebar_generate.rs:enrich_text_from_index`
also looks up entries by source path but currently emits no diagnostic on
miss (the existing helpers already cover that path) — confirm during
implementation that nothing slips through silently.

All four diagnostics share the same shape: a plain `DiagnosticMessage`
constructed with `::warning(title_string)`. That gives us:

- no `code` → can't be looked up in `error_catalog.json`, no docs URL,
  no `--json` consumer mapping;
- no `location` → renderer can't show "this came from line N of
  `_quarto.yml`" or "frontmatter of `guide/index.qmd:7`";
- no `problem` / `hints` → the tidyverse-style detail breakdown that
  the rest of the catalog uses is unavailable;
- no separation between *kinds* of misses (sidebar miss vs navbar miss
  vs body link miss vs auto-no-index vs auto-empty-match).

The structured `DiagnosticMessageBuilder` already exists
(`crates/quarto-error-reporting/src/builder.rs`) and is used by most other
subsystems (markdown Q-1-* / Q-2-*, listing Q-12-*, yaml validation
Q-1-1\*). Listing's Q-12-15/Q-12-16 are good shape templates for what we
want here: a title, a `with_code`, a `problem`, an `add_hint`, a catalog
entry naming the docs URL.

## Why diagnostics first (and not path resolution first)

The follow-up bd-qor9a fixes the path-resolution bug that *causes* most
of the missing-document warnings users see today. Once that lands, the
warnings should fire only for genuinely-broken references (typos,
deleted files, etc.). But those residual cases are exactly the ones
where users most want a useful diagnostic — code, location, hint.
Landing the diagnostic shape first means bd-qor9a can wire `SourceInfo`
into the same structured surface in one place rather than reworking the
warning strings twice.

`location` stays `None` on every diagnostic emitted in this issue. The
follow-up plugs it in.

## New error subsystem: navigation (Q-13-\*)

The catalog currently has subsystems: `cli`, `internal`, `listing`,
`lua`, `markdown`, `template`, `writer`, `xml`, `yaml`. Listing's
`Q-12-*` is the most recent allocation; the next contiguous block is
`Q-13-*`. Allocate `navigation` as the subsystem name.

Proposed initial allocation (catalog entries to be added in
`crates/quarto-error-reporting/error_catalog.json`):

| Code | Title | Fires from |
|------|-------|------------|
| `Q-13-1` | Sidebar entry references unknown document | `resolve_href_for_html` w/ source_label "Sidebar …" |
| `Q-13-2` | Navbar entry references unknown document | `resolve_href_for_html` w/ source_label "Navbar" |
| `Q-13-3` | Page footer references unknown document | `resolve_href_for_html` w/ source_label "Page footer" |
| `Q-13-4` | Body link references unknown document | `resolve_doc_relative_href` |
| `Q-13-5` | Sidebar `auto:` ignored — no project index | `sidebar_auto::strip_entries` |
| `Q-13-6` | Sidebar `auto:` matched no documents | `sidebar_auto::expand_spec` |

Open question for iteration: do we want Q-13-1/2/3 to be one code with
the surface (sidebar/navbar/footer) carried as a *detail*, or three
codes? Listing went the "one code per situation" route; markdown
sometimes overloads codes by detail. The split above mirrors listing.

Open question: `Q-13-4` (body link) is technically not navigation — it
comes from inline `Link` nodes in markdown body, not from
`website.sidebar` / `navbar` / `page-footer`. Two reasonable choices:
(a) keep it under `Q-13-*` because it shares the helper and the diagnostic
shape; (b) put it under `markdown` (`Q-1-*` / `Q-2-*`) since it's about
markdown body content. Recommendation: (a), with `subsystem: "navigation"`
in the catalog and a docs page that links the four "unknown document"
codes together.

## Working checklist

- [x] Phase 1.1: Catalog entries Q-13-1..6 added to `error_catalog.json`
- [x] Phase 1.2: Catalog presence tests added (mirror Q-12-15/Q-12-16 shape)
- [x] Phase 1.3: Catalog tests pass
- [x] Phase 2.1: `NavSurface` enum introduced; `source_label: Option<&str>` migrated
- [x] Phase 2.2: `missing_document_warning` helper using `DiagnosticMessageBuilder`
- [x] Phase 2.3: Helper accepts forward-looking `Option<SourceInfo>` (always None for now)
- [x] Phase 2.4: `resolve_href_for_html` migrated to helper
- [x] Phase 2.5: `resolve_doc_relative_href` migrated to helper (body-link, Q-13-4)
- [x] Phase 2.6: Existing `navigation_href.rs` tests updated for new shape
- [x] Phase 3.1: `sidebar_auto::strip_entries` migrated (Q-13-5)
- [x] Phase 3.2: `sidebar_auto::expand_spec` empty-match migrated (Q-13-6)
- [x] Phase 3.3: `sidebar_auto.rs` tests updated
- [x] Phase 4.1: `render_page_in_project.rs:438-451` regression test updated to assert on `code` rather than title substring
- [x] Phase 4.2: Spot-check other tests touching these diagnostic titles (footer_render, navbar_render, sidebar_render, sidebar_generate, link_rewrite, link_rewriting_pipeline — all migrated)
- [x] Phase 5.1: End-to-end `cargo run --bin q2 -- render docs/guide/index.qmd` confirms warnings now carry `[Q-13-1]` code with structured problem + hint
- [x] Phase 5.2: `cargo xtask verify --skip-hub-build` clean (no warnings; all 9189 workspace tests green)
- [x] Final: Plan checklist all green; ready for commit

## Title and message wording (locked)

The existing regression test
(`crates/quarto-core/tests/render_page_in_project.rs:438-451`) enforces
that *"references unknown document"* is **not** the wording (it was
removed in a prior rename, per bd-rqba) and that *"missing document
information"* **is** the wording for the navigation surface today. The
structured-diagnostic migration keeps the user-visible wording aligned:

| Code | Title | Problem | Hint |
|------|-------|---------|------|
| Q-13-1 | `Sidebar references missing document` | `'{path}' is not in the project index.` | `Check the spelling, or confirm the target file is included in the render set.` |
| Q-13-2 | `Navbar references missing document` | (same) | (same) |
| Q-13-3 | `Page footer references missing document` | (same) | (same) |
| Q-13-4 | `Body link references missing document` | (same) | (same) |
| Q-13-5 | `` Sidebar `auto:` ignored `` | `No project index is available — `auto:` entries cannot be expanded.` | `Render this document as part of a project to expand `auto:` entries.` |
| Q-13-6 | `` Sidebar `auto:` matched no documents `` | `Spec {spec_debug} found no matches.` | `Check the path/glob pattern, or confirm the target files exist in the project.` |
| Q-13-7 | `Page navigation references missing document` | (same as Q-13-1) | (same) |

(Q-13-7 was added once Phase 2 surfaced `page_nav_render.rs` as a fifth
navigation surface — separate from sidebar / navbar / page-footer /
body-link.)

The regression test asserts on `code == Some("Q-13-1")` post-migration
rather than substring-matching the title, so the wording can evolve
without churn there.

## Implementation plan (TDD)

### Phase 1 — Catalog entries (write first)

Add the six entries to `error_catalog.json` with placeholder
`message_template`, `docs_url`, `since_version: "99.9.9"`. Add catalog
presence tests in `crates/quarto-error-reporting/src/catalog.rs` (mirror
the existing `Q-12-15` / `Q-12-16` test shape at lines 142-185).

Tests fail until the catalog entries exist (TDD step 2).

### Phase 2 — Builder helpers in `navigation_href.rs`

Replace the two `DiagnosticMessage::warning(format!(...))` call sites
with a small helper:

```rust
fn missing_document_warning(
    surface: NavSurface,
    raw_path: &str,
    resolved_path: Option<&str>,
) -> DiagnosticMessage {
    let (code, surface_label) = match surface {
        NavSurface::Sidebar  => ("Q-13-1", "Sidebar"),
        NavSurface::Navbar   => ("Q-13-2", "Navbar"),
        NavSurface::Footer   => ("Q-13-3", "Page footer"),
        NavSurface::BodyLink => ("Q-13-4", "Body link"),
    };
    DiagnosticMessageBuilder::warning(format!(
        "{surface_label} references unknown document"
    ))
    .with_code(code)
    .problem(format!("'{}' is not in the project index.", raw_path))
    .add_hint("Check the spelling, or confirm the target file is included in the render set.")
    .build()
}
```

(Exact `NavSurface` plumbing — whether to thread an enum or keep
parsing the `source_label` string — to be decided during
implementation. The `source_label` string is *already* the surface
identifier in today's call sites; the cleanest move is to switch it
from `Option<&str>` to a typed enum at the helper signature.)

Tests:

1. `resolve_href_for_html` miss for each surface emits the right
   `code`, `kind: Warning`, `location: None`, `problem` containing the
   raw path. (Five tests — sidebar, navbar, footer, default-when-no-label,
   body-link via `resolve_doc_relative_href`.)
2. Existing snapshot tests that assert on the old title string
   (`crates/quarto-core/tests/render_page_in_project.rs:441-442`) get
   updated to assert on the new title and on `code == Some("Q-13-1")`.

### Phase 3 — `sidebar_auto` warnings

Same shape:

- `strip_entries` → `DiagnosticMessageBuilder::warning("…").with_code("Q-13-5")`
- `expand_spec` (empty match) → `Q-13-6` with the spec rendered into the
  `problem` field, not the title.

### Phase 4 — Downstream consumers

The CLI's text renderer and the `--json` output path already know how
to read `code`; no work there. The hub-client receives `DiagnosticMessage`
via the existing wire shape and should start surfacing `code` once we
emit it (verify by inspection — should be a no-op).

Update `crates/quarto-core/tests/render_page_in_project.rs:382-441`
(the `missing document information` regression test) to assert on the
new structured shape:

- `kind == Warning`,
- `code == Some("Q-13-…")` for the surface in question,
- `title` matches the new wording,
- `problem` contains the offending path.

### Phase 5 — Docs URLs

Each catalog entry gets a `docs_url` of the form
`https://quarto.org/docs/errors/Q-13-N`. The pages themselves don't have
to exist yet (no other subsystem has authored its docs site either) —
follow the pattern: catalog entry now, page later. Flag this as a
discovered-from issue if user wants to track the docs work separately.

## What is *not* in scope for bd-8d6rk

- Threading `SourceInfo` through to the diagnostics\' `location` field.
  That requires the navigation parser to stop calling `as_plain_text()`
  on href strings, which is exactly what bd-qor9a does.
- Changing the path-resolution rule (the actual bug). Diagnostics emitted
  here still fire on the *current* misses, which include the
  `docs/guide/index.qmd` reproducer. After bd-qor9a, the same diagnostic
  surface stays in place but is exercised only by genuinely-missing
  references.
- Generalizing the structured-diagnostic migration to other transforms
  (`theorem.rs`, `crossref_resolve.rs`, `crossref_index.rs`,
  `attribution_render.rs`) — same shape problem, separate scope.
  Tracked as **bd-m2w7a** (discovered-from bd-8d6rk).

## Verification checklist

- [ ] `cargo nextest run -p quarto-error-reporting` (catalog tests)
- [ ] `cargo nextest run -p quarto-core --lib navigation_href`
- [ ] `cargo nextest run -p quarto-core --lib sidebar_auto`
- [ ] `cargo nextest run -p quarto-core --test render_page_in_project`
- [ ] `cargo xtask verify` (full workspace + hub-client)
- [ ] End-to-end: `cargo run --bin q2 -- render docs/guide/index.qmd`
      inspects the JSON diagnostics output and confirms each warning
      carries a `code` (still `location: null` at this point).

## Resolved design decisions

1. **One code per surface** — six codes (`Q-13-1` sidebar, `Q-13-2` navbar,
   `Q-13-3` page footer, `Q-13-4` body link, `Q-13-5` auto-no-index,
   `Q-13-6` auto-empty-match). No detail-overloading.

2. **Body-link diagnostic lives in the `navigation` subsystem.**
   `Q-13-4`, `subsystem: "navigation"`. Same helper, same shape.

3. **`Q-13-5` (auto-no-index) wording.** Title short ("Sidebar `auto:`
   ignored"); problem one-liner ("No project index is available.");
   hint is the actionable suggestion ("Render this document as part of
   a project to expand `auto:` entries.").

## Out of scope (tracked elsewhere)

- **Strict mode** — promoting these warnings to errors under a strict
  flag is the right shape for the new builder API to make trivial later
  (toggle `kind` based on the flag), but the strict-mode work itself
  is tracked separately at
  [quarto-dev/q2#220](https://github.com/quarto-dev/q2/issues/220).

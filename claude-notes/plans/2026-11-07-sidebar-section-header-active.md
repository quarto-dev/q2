---
title: "Sidebar section-header pages are not marked active"
---

Strand: bd-gtpwby98 (related to the websites epic, bd-0tr6).

## Overview

On a page that is also a sidebar section header, Quarto 1 marks the
header link `sidebar-item-text sidebar-link active`. This covers a
`section:` with `href:`, and a `contents:` entry with both `file:` and
`contents:`. q2 0.42.0 leaves that link unmarked, so the sidebar shows
no current page. Leaf links are marked correctly. The report came from
the Posit Connect docs port, where 19 pages are affected (every cookbook
section index, `cookbook/index.html`, `how-to/index.html`,
`admin/user-management/`, `admin/process-management/`).

Root cause, in `crates/quarto-navigation`:

- `mark_active_in` (`src/sidebar.rs`) detects the header match but only
  uses it to set `expanded = true`. `SidebarEntry::Section` has no
  `active` field to record it.
- `render_sidebar_section` (`src/render_html.rs`) hard-codes the header
  anchor's class to `sidebar-item-text sidebar-link`.

## Checklist

### Phase 1: tests (must fail first)

- [x] Unit test in `sidebar.rs`: `resolve_active_state` on a page equal
  to a section's `href` sets the section's `active`. The ancestor
  sections are expanded but not active, and sibling leaves stay inactive.
- [x] Unit test in `sidebar.rs`: `active` survives
  `to_config_value` / `from_config_value` (the Generate → Render handoff).
- [x] Unit test in `render_html.rs`: an active section with an `href`
  renders `class="sidebar-item-text sidebar-link active"` on the header
  anchor, and an inactive one does not.
- [x] Integration test in `quarto-core/tests/integration/sidebar_pipeline.rs`,
  driven through the website render path, covering both authoring forms
  (`section:` + `href:` and `file:` + `contents:`).
- [x] Confirm the new tests fail on the unfixed code.

### Phase 2: fix

- [x] Add `active: bool` to `SidebarEntry::Section`. Round-trip it as
  `active: true` in `to_config_value` / `from_config_value`, the same
  way `NavigationItem` does.
- [x] Set it in `mark_active_in` when the header `href` matches.
- [x] Emit ` active` on the header anchor in `render_sidebar_section`.
- [x] Update every other `Section` construction site.

### Phase 3: verification

- [x] `cargo nextest run -p quarto-navigation -p quarto-core`, plus
  clippy with `-D warnings`.
- [x] End-to-end: render a small website with `cargo run --bin q2` and
  inspect the section-index pages' HTML.
- [x] `cargo xtask verify --skip-hub-build`.
- [ ] Open the PR and watch CI until it is green.

## Details

### Verification record

End-to-end check with the branch binary (`cargo run --bin q2 -- render`)
on a five-page site that has both header forms. The output HTML was
inspected:

```text
== groups/index   (section: Groups + href: groups/index.qmd)
<a href="index.html" class="sidebar-item-text sidebar-link active"
== groups/b       (file: groups/b.qmd + contents:)
<a href="b.html" class="sidebar-item-text sidebar-link active"
== groups/a       (leaf inside Groups: header stays unmarked)
<a href="index.html" class="sidebar-item-text sidebar-link"
<a href="a.html" class="sidebar-item-text sidebar-link active"
```

Each page has exactly one `sidebar-link active`. Locally,
`dashboard_goldens::dashboard_skeletons_match_goldens` fails because the
machine's R install lacks `bslib`. That failure is unrelated to this
change.

### Design

The active flag goes only on the header anchor, as in Quarto 1. A
section that *contains* the active page is expanded but not marked
active. Breadcrumbs (`trail_in`) already handle a header match, so they
need no change.

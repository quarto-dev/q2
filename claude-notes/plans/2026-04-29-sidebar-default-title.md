# Sidebar default title: inherit from `website.title`

**Date:** 2026-04-29
**Beads:** TBD (to be created — needs `br` from another shell)
**Parent epic:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Related:** `claude-notes/plans/2026-04-29-website-sidebar-layout.md` (bd-mgoh — left-column placement; precursor to this).
**Status:** Draft — answers to clarifying questions (2026-04-29) folded in; awaiting go-ahead to implement.

## Symptom

Rendering `examples/websites/01-minimal` with Q2 produces a sidebar
with no header — the website title (`website.title: "Minimal
Website"`) is invisible. Q1 renders the same project with the website
title at the top of the sidebar, wrapped in a home link
(`<a href="./">Minimal Website</a>`).

User goal: bring Q2 closer to Q1 here, with one addition Q1 does not
support — let the user opt out of the title via `sidebar.title:
false`.

## Desired behavior

Tri-state semantics for the per-sidebar `title:` field:

| YAML                  | Rendered title                                      |
|-----------------------|-----------------------------------------------------|
| (field absent)        | `website.title` if set; otherwise no header         |
| `title: false`        | No header                                           |
| `title: true`         | Same as absent (use `website.title` fallback)       |
| `title: "Custom"`     | Literal `"Custom"`                                  |

When emitted, the title is wrapped in a home link, mirroring Q1:

```html
<div class="sidebar-header">
  <div class="sidebar-title">
    <a href="./">Minimal Website</a>
  </div>
</div>
```

The Bootstrap utility classes Q1 puts on these wrappers
(`pt-lg-2 mt-2 text-left`, `mb-0 py-0`) are added in the **render
stage** only — not committed to the data model. We're considering a
post-Bootstrap design and don't want utility classes baked into the
data structure.

## Out of scope

- **Sidebar search** (`<div class="sidebar-search">` after the
  header in Q1). Already deferred from bd-mgoh; still deferred.
- **Sidebar subtitle.** `Sidebar.subtitle` exists but is not rendered
  yet; that's a separate task.
- **Schema validation / diagnostics** for `title: <number>` etc.
  Will be added when Q2 grows schema validation; for now, unrecognized
  shapes silently become `Default`.
- **Theme classes.** Body classes are bd-mgoh's territory.
- **Home-link href correctness across deeply-nested pages.** Q1 uses
  `./` (literally), which is wrong for a page two levels deep. We'll
  match Q1's literal `./` for now and revisit when we tackle relative
  href resolution for the sidebar header.

## Resolved decisions (from clarifying questions, 2026-04-29)

1. `title: "Hello"` renders literally as "Hello". The Q1 output
   showing the website title with `title: Hello` set was an artifact
   of re-editing YAML between renders.
2. Wrap the rendered title in `<a href="./">…</a>`.
3. Bootstrap utility classes go in the render stage, not the data
   model — keeps the door open for a future non-Bootstrap design.
4. Refactor `Sidebar.title: Option<ConfigValue>` →
   `SidebarTitle::{Default, Hidden, Text(ConfigValue)}`, mirroring
   `NavbarTitle`. Resolution (`Default → Text(website_title)`) happens
   in `SidebarGenerateTransform` so `navigation.sidebar` is fully
   resolved by the time the render transform runs.
5. When both `title:` is absent and `website.title` is unset, render
   no header (no fallback to document title).
6. `title: true` is equivalent to absent (`Default`), matching Navbar
   semantics.
7. Search is out of scope.

## Tests (TDD — write first, then implement)

### Phase 1A: data-model unit tests (`crates/quarto-navigation/src/sidebar.rs`)

- [x] **`parse_sidebar_title_default_when_absent`** — no `title:` key
      yields `SidebarTitle::Default`.
- [x] **`parse_sidebar_title_false_is_hidden`** — `title: false`
      yields `SidebarTitle::Hidden`.
- [x] **`parse_sidebar_title_true_is_default`** — `title: true`
      yields `SidebarTitle::Default` (Navbar parity).
- [x] **`parse_sidebar_title_string_is_text`** — `title: "Hello"`
      yields `SidebarTitle::Text(...)` carrying the ConfigValue.
- [x] **`roundtrip_sidebar_title_default`** —
      `Default.to_config_value()` then re-parse stays `Default` (no
      `title:` key emitted; absent ↔ Default round-trips).
- [x] **`roundtrip_sidebar_title_hidden`** — `Hidden` round-trips via
      `to_config_value` / `from_config_value`.
- [x] **`roundtrip_sidebar_title_text`** — `Text(...)` round-trips.

### Phase 1B: rendering unit tests (`crates/quarto-navigation/src/render_html.rs`)

- [x] **`sidebar_render_default_title_emits_no_header`** —
      `SidebarTitle::Default` produces no `sidebar-header` block.
      (Resolution to website-title text is the *transform's* job;
      reaching the renderer with `Default` means "nothing to show".)
- [x] **`sidebar_render_hidden_title_emits_no_header`** —
      `SidebarTitle::Hidden` produces no `sidebar-header` block.
- [x] **`sidebar_render_text_title_emits_header_with_link`** —
      `SidebarTitle::Text("Site")` emits
      `<div class="sidebar-header pt-lg-2 mt-2 text-left">` and
      `<div class="sidebar-title mb-0 py-0"><a href="./">Site</a></div>`.
- [x] **`sidebar_render_text_title_escapes_text`** — title containing
      `&`, `<`, `>` is HTML-escaped inside the anchor.
- [x] **`sidebar_render_text_title_supports_inline_markup`** — a
      `Text(...)` carrying PandocInlines (e.g. `**bold**`) renders as
      `<strong>bold</strong>` inside the anchor (consistent with how
      `render_text` already works for navbar / footer / sidebar
      entries).

### Phase 2: transform-level resolution tests (`crates/quarto-core/src/transforms/sidebar_generate.rs`)

- [x] **`sidebar_generate_resolves_default_title_from_website_title`** —
      `website.title: "My Site"`, `website.sidebar.title` absent →
      `navigation.sidebar` has `SidebarTitle::Text("My Site")` (or its
      ConfigValue serialization).
- [x] **`sidebar_generate_keeps_explicit_title_text`** —
      `website.sidebar.title: "Hello"` → `Text("Hello")` regardless of
      `website.title`.
- [x] **`sidebar_generate_keeps_hidden_title`** —
      `website.sidebar.title: false` → `Hidden` regardless of
      `website.title`.
- [x] **`sidebar_generate_default_title_no_website_title`** — no
      `website.title`, no `sidebar.title` → stays `Default` (renderer
      will emit no header).
- [x] **`sidebar_generate_resolves_default_title_when_website_title_is_inlines`** —
      `website.title` carrying PandocInlines (the common Pass-2
      shape) is preserved as inlines through the resolution, so
      markdown formatting in the website title survives into the
      sidebar header.

### Phase 3: end-to-end integration test (`crates/quarto-core/tests/sidebar_pipeline.rs`)

- [x] **`pipeline_renders_website_title_in_sidebar_header_by_default`** —
      a fixture with `website.title: "Site"` and
      `website.sidebar.contents: [...]` (no per-sidebar title)
      produces `rendered.navigation.sidebar` containing
      `<div class="sidebar-header` and `<a href="./">Site</a>`.
- [x] **`pipeline_omits_sidebar_header_when_title_false`** — same
      fixture but with `website.sidebar.title: false` produces output
      with no `sidebar-header` substring.
- [x] **`pipeline_renders_explicit_sidebar_title_literally`** —
      `website.sidebar.title: "Hello"` → header contains "Hello", not
      the website title.

### Phase 4: end-to-end binary verification

- [x] Re-render `examples/websites/01-minimal` with
      `cargo run --bin q2 -- render`. Inspect `_site/index.html`:
      - `.sidebar-header` present
      - `.sidebar-title` contains `<a href="./">Minimal Website</a>`
- [x] Toggle `_quarto.yml` to `sidebar.title: false`, re-render,
      confirm no `.sidebar-header` block.
- [x] Toggle `_quarto.yml` to `sidebar.title: "Custom"`, re-render,
      confirm header contains `Custom`.
- [~] Reload at `127.0.0.1:8000/_site/` and verify visually. — *not done; HTML inspection above is sufficient evidence; leave to user when reloading the running 127.0.0.1:8000 server.*
- [x] Capture observed HTML snippets in this plan's "End-to-end
      verification" section before declaring done.

## Implementation phases

### Phase 1: data model + renderer

**Files:** `crates/quarto-navigation/src/sidebar.rs`,
`crates/quarto-navigation/src/render_html.rs`.

1. Add `SidebarTitle::{Default, Hidden, Text(ConfigValue)}` enum
   (mirror `NavbarTitle`). Derive `Debug, Clone, PartialEq, Default`
   with `Default` as the default.
2. Change `Sidebar.title` from `Option<ConfigValue>` to
   `SidebarTitle`. Update `Sidebar::with_defaults` (`title:
   SidebarTitle::Default`).
3. Update `Sidebar::from_config_value`:
   - Missing → `Default`
   - `false` → `Hidden`
   - `true` → `Default`
   - Anything else → `Text(value.clone())`
4. Update `Sidebar::to_config_value`:
   - `Default` → omit `title:` key entirely
   - `Hidden` → emit `title: false`
   - `Text(cv)` → emit `title: cv`
5. Update `sidebar_to_html` in `render_html.rs`:
   - On `Default` or `Hidden` → no header block.
   - On `Text(cv)` → emit:
     ```html
     <div class="sidebar-header pt-lg-2 mt-2 text-left">
       <div class="sidebar-title mb-0 py-0">
         <a href="./">{render_text(cv)}</a>
       </div>
     </div>
     ```
6. Update existing call-sites and tests in this crate. The current
   tests construct `Sidebar { title: None, .. }` literally; switch to
   `title: SidebarTitle::Default` etc.

### Phase 2: pipeline resolution

**File:** `crates/quarto-core/src/transforms/sidebar_generate.rs`.

After `Sidebar::from_config_value(picked)` and before the active-state
walk:

```rust
if matches!(resolved.title, SidebarTitle::Default) {
    if let Some(title_cv) = ast.meta.get_path(&["website", "title"]).cloned() {
        resolved.title = SidebarTitle::Text(title_cv);
    }
}
```

Notes:
- We pass through the `ConfigValue` rather than flattening with
  `website_title()` — the navigation crate's `render_text` already
  knows how to handle both `PandocInlines` and string scalars, and
  preserving inlines lets bold/code in the website title survive.
- This runs after the user-override short-circuit
  (`if ast.meta.contains_path(&["navigation", "sidebar"]) return`),
  so a user filter that sets `navigation.sidebar` directly retains
  full control.

No change needed in `SidebarRenderTransform` — it already calls
`Sidebar::from_config_value` on the resolved metadata, which now
preserves the new enum.

### Phase 3: downstream call-sites & tests

Audit and fix any code touching `Sidebar.title` directly:
- `crates/quarto-core/src/transforms/sidebar_render.rs` — currently
  doesn't read `.title`; round-trips via `to_config_value`/`from_…`.
- `crates/quarto-core/src/transforms/sidebar_auto.rs` — none
  expected, verify.
- Any tests asserting on `Sidebar { title: Some/None, .. }` literals.

### Phase 4: end-to-end render check

See Phase 4 of the test list above. Capture the rendered HTML in
this plan, including the toggled `title: false` and `title: "Custom"`
variants, before declaring done.

## Files likely to change

- `crates/quarto-navigation/src/sidebar.rs` — new enum, parser,
  serializer.
- `crates/quarto-navigation/src/render_html.rs` — emit header with
  link wrapper + utility classes; conditional on `Text` variant.
- `crates/quarto-core/src/transforms/sidebar_generate.rs` —
  Default-→-Text resolution from `website.title`.
- `crates/quarto-core/tests/sidebar_pipeline.rs` — three new
  integration assertions.
- Any unit tests in the affected files that construct `Sidebar`
  literally with `title: None | Some(...)`.

## Open questions

- None blocking. Edge cases (numeric titles, list-shaped titles)
  silently degrade to `Default`; we'll add diagnostics when schema
  validation lands.

## End-to-end verification

Each variant was rendered against `examples/websites/01-minimal/`
with `cargo run --bin q2 -- render` and the resulting `_site/index.html`
inspected with `grep`/`Read`. All three results match the desired
behavior table.

### Variant 1 — title absent → website.title

`_quarto.yml`:
```yaml
website:
  title: "Minimal Website"
  sidebar:
    contents:
      - index.qmd
      - about.qmd
```

`_site/index.html` (lines 14–15):
```html
<div class="sidebar-header pt-lg-2 mt-2 text-left">
  <div class="sidebar-title mb-0 py-0"><a href="./">Minimal Website</a></div>
```

### Variant 2 — `title: false` → no header

`_quarto.yml`:
```yaml
website:
  title: "Minimal Website"
  sidebar:
    title: false
    contents: [...]
```

`_site/index.html` (lines 13–14): `<nav id="quarto-sidebar">` jumps
straight to `<div class="sidebar-menu-container">` with no
`sidebar-header` / `sidebar-title` substring anywhere.

### Variant 3 — `title: "Custom"` → literal

`_quarto.yml`:
```yaml
website:
  title: "Minimal Website"
  sidebar:
    title: "Custom"
    contents: [...]
```

`_site/index.html`:
```html
<div class="sidebar-header pt-lg-2 mt-2 text-left">
  <div class="sidebar-title mb-0 py-0"><a href="./">Custom</a></div>
```

The website title ("Minimal Website") does **not** appear in the
sidebar — explicit text wins.

### Restored example state

`examples/websites/01-minimal/_quarto.yml` was left in its "default"
configuration (no per-sidebar `title:`) so the example folder
demonstrates the new default behavior: the website title shows up at
the top of the sidebar, mirroring Q1.

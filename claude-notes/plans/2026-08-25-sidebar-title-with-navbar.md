# Suppress the sidebar title when the page has a navbar

**Strand:** `bd-sidebar-title-with-navbar-82wxow6m` (bug, p3, labels: `navigation`, `parity`)
**Branch:** `braid/bd-sidebar-title-with-navbar-82wxow6m-sidebar-title-with-navbar`
**Base:** `origin/main` @ `c11aa0e4d` (rebased 2026-08-26; originally planned at `99e7db175`)
**Worktree:** `.worktrees/workspace-3`

## Overview

When a Quarto website declares **both** a navbar and a titled sidebar, q2 renders
the sidebar title. Quarto 1 does not.

This plan ports Q1's missing gate: **a sidebar title is emitted only when the
page has no navbar.**

### What the rule actually is

Q1's gate is `!navbar`, full stop. Verified as a **matrix** against the real Q1
binary (`quarto` 99.9.9) — one fixture, varying only navbar presence and sidebar
logo:

| navbar | sidebar logo | `sidebar-title` | `sidebar-header` | `sidebar-logo` | title text shown |
| --- | --- | --- | --- | --- | --- |
| no  | no  | 1 | 1 | 0 | yes |
| no  | yes | 1 | 1 | 3 | yes |
| yes | no  | **0** | 0 | 0 | no |
| yes | yes | **0** | 1 | 3 | no |

Three consequences, each of which has already misled someone on this issue:

1. **It is not about the logo.** The logo controls the *wrapper*; row 4 emits
   `sidebar-header` and the logo and still no title.
2. **It is not about string duplication.** In the repro fixture `website.title`
   is `"Site Title"` and the sidebar title is `"Guides"` — different strings,
   still suppressed. And the Positron site that surfaced this sets
   `navbar: title: false`, so its navbar carries no brand text at all — still
   suppressed. **Do not narrow this into an equality check**; it would not fix
   the reported bug.
3. **Sidebar titles are not broken in Q1.** They render whenever there is no
   navbar — the ordinary case for books and sidebar-only doc sites. That is the
   `without-navbar/` control below.

The actual rationale is **navbar-item duplication**. Positron's navbar has
`- text: "Guides" href: welcome.qmd`, and the sidebar titled `"Guides"` is the
one containing `welcome.qmd`. That navbar item renders *active* on every page
the sidebar covers, so the navbar already shows the reader which section they
are in; a "Guides" banner atop the sidebar restates it. The markup confirms the
intent — the element is `<a href="/">`, a home link, and Q1 defaults
`sidebar.title` to `website.title`. It is site/section branding for when there
is no navbar to carry it, **not a heading for the sidebar's contents**.

The premise holds in q2: `navbar_generate.rs:146` calls `mark_active`, and
`render_html.rs:671` emits `nav-link active`. Suppressing the sidebar title does
not remove the reader's only position indicator.

### What is NOT affected: `- section:` labels

Only the sidebar's own `title:` is suppressed. Group headings written as
`- section:` inside `contents:` render normally under a navbar — verified
against Q1 (a top-level `- section: "Error reference"` produces 3 hits with a
navbar present). Given:

```yaml
  sidebar:
    - title: "Guides"                  # suppressed under a navbar
      contents:
        - section: "Getting Started"   # renders
        - section: "Configuration"     # renders
        - section: "User Interface"    # renders
```

the sidebar keeps its entire structure and loses only the banner above it.

### Observed behaviour

Two projects, identical except that one declares a navbar. Same sidebar, same
`website.title`, same pages. Count of `sidebar-title` occurrences in
`_site/index.html`:

| fixture           | q2 (`99e7db175`) | Q1  | after this plan |
| ----------------- | ---------------- | --- | --------------- |
| `with-navbar/`    | **1**            | 0   | 0               |
| `without-navbar/` | 1                | 1   | 1               |

`without-navbar/` is the **control**: with no navbar the two engines already
agree, so the defect is specific to the navbar-present case, not to sidebar
titles in general.

Reproduced on this branch's base with a freshly built `q2`. See
§"Reproduction fixture".

### Real-world impact

The Positron website docs declare one navbar and one sidebar titled "Guides".
Every page carrying that sidebar renders the extra title — **81 pages**. After
the other chrome-affecting differences in that port were fixed, this is the
**sole remaining chrome difference** between its Q1 and Q2 builds: a chrome
sweep across 105 pages reports `sidebar:1` on all 81 and no other bucket.

### Expected collateral: 259 of 266 pages on this repo's own docs site

**Measured, not inferred.** Rendered `docs/` with the pre-fix binary and
counted. Re-measured at the rebased base `c11aa0e4d` (2026-08-26); the earlier
figure at `99e7db175` was 258 of 265, and the difference is exactly the one
error page `errors/listing/Q-12-24.qmd` that landed in between. Today **259 of
266** pages carry a `sidebar-title`, in three flavours:

| rendered title | pages | where it comes from | after the gate |
| --- | ---: | --- | --- |
| `Error reference` | 224 | explicit `title:` on the `errors` sidebar (`docs/_quarto.yml:84`) | gone |
| `Quarto 2` | 34 | **auto-substituted `website.title`** on the sidebars with no `title:` (`Guides` and `Authoring` — see below) | gone |
| `Presentations` | 1 | explicit `title:` on the `presentations` sidebar (`:77`) | gone |
| *(none)* | 7 | pages carrying no sidebar | unchanged |

The 34 `Quarto 2` pages are the **pure bug case** — nobody authored that; it is
the default substitution restating the navbar brand, exactly what this strand
describes. Removing it is unambiguously right.

The 225 authored-label pages are the editorial question. Which sidebar uses
which pattern:

| sidebar | heading pattern | matching navbar item |
| --- | --- | --- |
| `Guides` (`:25`) | `- section: "Guides"` — plus an auto `Quarto 2` banner | `guides/index.qmd` |
| `Authoring` (`:47`) | `- section: "Authoring Quarto Documents"` — ditto | — |
| `guide` (`:71`) | `- section: "Guide"` — **dead config**: its only entry points at `docs/guide/index.qmd`, which does not exist, so no page ever selects this sidebar and it contributes 0 of the 34. Pre-existing; out of scope. | — |
| `presentations` (`:76`) | `title: "Presentations"` (`:77`) | **"Presentations"** (`:20-21`) |
| `errors` (`:83`) | `title: "Error reference"` (`:84`) | **"Errors"** (`:22-23`) |

Both `title:` sidebars duplicate a navbar item, which is the shape the rule
exists for, so Q1's suppression is correct for both. The inner `- section:`
labels survive untouched — verified on our own site (`Reveal.js` still present
on the presentations page, `yaml`/`markdown`/… on the errors pages).

#### Correction: deleting the two `title:` lines must NOT precede the gate

An earlier draft put the deletion in a phase *before* the gate, reasoning that
it would stop `main` from carrying a commit where the headings silently vanish.
**That reasoning is wrong, and the empirical check caught it.** A sidebar with no
`title:` does not render blank — `SidebarTitle::Default` substitutes
`website.title`. Verified against the pre-fix binary:

```
sidebar with no `title:`, website.title = "Quarto 2"
  → <div class="sidebar-title mb-0 py-0"><a href="./">Quarto 2</a></div>
```

So deleting `title: "Error reference"` before the gate would turn 224 pages from
`Error reference` into `Quarto 2` — a worse interim state, not a preserved one.

**There is no ordering that avoids the docs headings going away. The gate
removing them *is* the fix.** The two `title:` deletions are therefore *dead-config
cleanup* — once the gate lands, a `title:` on a sidebar under a navbar does
nothing — and they belong in the **same commit as the gate** (Phase 4.5), never
before it.

#### Open editorial question (not a blocker for the code)

`Error reference` is currently the sidebar banner on **224 pages**. Before
deciding anything, note that **the gate removes a duplicate of that label, not
its only instance.** The errors sidebar's first content entry is
`errors/index.qmd`, whose own page title is `"Error reference"`
(`docs/errors/index.qmd:2`), so today the string appears twice:

```html
<div class="sidebar-title mb-0 py-0"><a href="../">Error reference</a></div>   <!-- banner: goes away -->
<div class="sidebar-menu-container">
  <li class="sidebar-item">
    <a href="index.html" class="sidebar-item-text sidebar-link active"><span class="menu-text">Error reference</span></a>
```

After the fix the sidebar still opens with an `Error reference` link, then the
`yaml` / `markdown` / … sections, with the navbar's "Errors" item active — which
is what Q1 does. Keeping the banner would mean converting `title:` to a
`- section:`, nesting the whole sidebar one level deeper and interacting with
`collapse-level` across 224 entries — hard to justify for a label that does not
actually disappear.

`presentations` is the weaker case: after deletion "Presentations" survives only
in the navbar. But it is one page.

Flagged for Gordon; the parity fix does not depend on the answer, and the plan
proceeds either way.

## Root cause

**Quarto 1** — `quarto-cli/src/resources/projects/website/templates/sidebar.ejs`.
Two nested gates, both consulting `navbar`:

```ejs
<%# line 34: the outer wrapper %>
<% if (sidebar.logo || (sidebar.title && !navbar)) { %>
  <div class="pt-lg-2 mt-2 <%= alignCss %> sidebar-header<%= sidebar.logo && sidebar.title ? ' sidebar-header-stacked' : '' %>">
  ...
  <%# line 51: the title block %>
  <% if (!navbar) { %>
  <% if (sidebar.title) { %>
  <div class="sidebar-title mb-0 py-0">
```

`navbar` is a plain boolean parameter. `nav-before-body.ejs` invokes the partial
with an explicit param bag:

```ejs
<% partial('sidebar.ejs', { sidebar: nav.sidebar, sidebarStyle: nav.sidebarStyle,
                            navbar: !!nav.navbar, toc: navbarTocLeft,
                            language: nav.language, draftMode: nav.draftMode }) %>
```

**q2** — `crates/quarto-navigation/src/render_html.rs:410`. Gates only on the
title resolving to a concrete value:

```rust
if let SidebarTitle::Text(ref title_cv) = sidebar.title {
    html.push_str("  <div class=\"sidebar-header pt-lg-2 mt-2 text-left\">\n");
    html.push_str(&format!(
        "    <div class=\"sidebar-title mb-0 py-0\"><a href=\"{}\">{}</a></div>\n",
        escape_attr(home_url),
        render_text(title_cv)
    ));
    html.push_str("  </div>\n");
}
```

Q1's title block requires three things at once: a sidebar exists, `sidebar.title`
is truthy, and `!navbar`. q2 honours the first two and misses the third:

- sidebar absent → the transform never calls the renderer.
- `SidebarTitle::Default` — the resolver had no `website.title` to substitute.
  Unit-tested: `sidebar_render_default_title_emits_no_header`
  (`render_html.rs:2558`).
- `SidebarTitle::Hidden` — an explicit `title: false`. Unit-tested:
  `sidebar_render_hidden_title_emits_no_header` (`render_html.rs:2574`).
- **navbar present — not implemented. This plan.**

The comment immediately above that block notes the Bootstrap utility classes
"match Q1's spacing/alignment for visual parity", so Q1's markup was
deliberately reproduced; only the enclosing condition was not carried over.

## Findings that shape the fix

Established during investigation and recorded so the implementer does not have
to rediscover them.

### 1. Q2's `Sidebar` has no logo and no tools, so Q1's two gates collapse into one

Q1's outer wrapper fires on `sidebar.logo || (sidebar.title && !navbar)`, and
inside it the title block fires on `!navbar && sidebar.title`. The wrapper's
`sidebar.logo` disjunct exists so a **logo** still renders its `sidebar-header`
under a navbar (gaining `sidebar-header-stacked` when both are configured).

`quarto_navigation::Sidebar` (`crates/quarto-navigation/src/sidebar.rs:387`) has
fields `id, title, subtitle, style, collapse_level, background, border,
contents, pinned` — **no `logo`, no `tools`, no `search`**. With `logo` always
absent, Q1's outer condition reduces to `sidebar.title && !navbar`, which is
exactly the inner one. So adding `&& !has_navbar` to q2's single existing gate
is a faithful port, not a partial one.

**Forward note:** leave a comment at the gate recording that when a `logo` field
lands, the single gate must be split back into Q1's two, so a logo still emits
`sidebar-header` under a navbar. Do **not** build that structure now.

### 2. The signal is already in scope, and the pipeline order already guarantees it

`build_transform_pipeline` (`crates/quarto-core/src/pipeline.rs:1130`, the only
such builder in the tree) pushes:

```
1394:    pipeline.push(Box::new(NavbarRenderTransform::new()));
1395:    pipeline.push(Box::new(SidebarRenderTransform::new()));
```

Adjacent, unconditional (no `cfg`), and this is the **only** registration site
for `SidebarRenderTransform` in the tree — the same builder serves native render
and the preview pipeline. So `rendered.navigation.navbar` is populated by the
time the sidebar renders.

`NavbarRenderTransform` bails on `is_feature_disabled` (`navbar_render.rs:66`)
and on a pre-populated `rendered.navigation.navbar` (`:72`), and otherwise
writes at `:152`. When it writes, the value comes from `navbar_to_html`
(`render_html.rs:44`), which unconditionally emits
`<nav class="navbar navbar-expand-…">` — **never an empty string**.

### 3. There is already a house idiom for "does this page ship a navbar"

`crates/quarto-core/src/transforms/quarto_nav_js.rs:99` asks the same question
with an inline closure:

```rust
let rendered_non_empty = |key: &str| {
    meta.get_path(&["rendered", "navigation", key])
        .and_then(|v| v.as_plain_text())
        .is_some_and(|s| !s.is_empty())
};
let has_header = rendered_non_empty("navbar") || rendered_non_empty("secondary-nav");
```

This plan lifts that predicate into a shared helper so the two copies cannot
drift.

### 4. No JavaScript consumes `.sidebar-title` in q2

Q1's `quarto-nav.js` copies the sidebar title into the narrow-viewport
`.quarto-secondary-nav-title`. q2's vendored
`resources/js/quarto-nav/quarto-nav.js` does **not** — zero hits for
`sidebar-title`, `secondary-nav-title`, and `sidebarTitle`. Removing the element
has no JS knock-on.

q2 *does* emit `.quarto-secondary-nav-title` (`render_html.rs:349`), but from
`SecondaryNavContent::CollapsedTitle`, fed independently by
`secondary_nav_render.rs` — unaffected by this change.

Only SCSS otherwise references the class
(`resources/scss/bootstrap/_bootstrap-rules.scss:508-514` (plus `.sidebar-title > a` at `:516-519`)); a rule with no
matching element is inert.

### 5. `sidebar.title` has non-HTML consumers, and this fix leaves them alone

`llms_txt.rs::llms_txt_multi_sidebar_h2_per_sidebar` (`:352`) declares two
*titled* sidebars and asserts `## Guide` / `## Reference` headings in
`llms.txt`. Those headings come from the sidebar **data model**, not from
rendered HTML.

Gating at the renderer (D1) leaves every such consumer correct by construction.
This is the strongest argument for D1 — see there.

## Design decisions

Recorded with rationale so they are not re-litigated during implementation.
An independent blank-slate review agreed with all four.

### D1. Gate in the renderer, not in the transform

**Decision:** add the condition at `render_html.rs:410`, alongside the markup.

**Rejected alternative:** have `SidebarRenderTransform` set
`sidebar.title = SidebarTitle::Hidden` on its local `Sidebar` before calling the
renderer. Smaller diff, and it does not leak into metadata (the transform owns a
local `Sidebar` built by `from_config_value`), but:

- **Blast radius.** It suppresses by *editing a value other code reads*
  (finding 5: llms.txt derives its `##` headings from `sidebar.title`). Nothing
  breaks today, because the mutation is on a local copy — but that is luck, not
  design, and it establishes a "suppress by editing the model" pattern a later
  refactor could push upstream into the shared model, silently dropping llms.txt
  headings. Gating the renderer cannot do that: the model is never touched.
- It overloads `SidebarTitle::Hidden`, whose doc comment says "Explicitly
  suppressed via `title: false`. Never substituted." — the variant would acquire
  a second, undocumented meaning.
- The render-site comment would stay incomplete. **The entire defect is that the
  enclosing condition was dropped when the markup was ported.** Putting the
  condition anywhere but next to the markup invites the same drift again.

### D2. Read `rendered.navigation.navbar`, not `navigation.navbar`

**Decision:** the predicate is "`rendered.navigation.navbar` exists and is
non-empty".

Q1's `nav.navbar` is the *resolved config object*, which would suggest reading
`navigation.navbar` plus `is_feature_disabled(meta, "navbar")`. Reading the
*rendered* key is preferred because one read handles cases the config-side read
would need extra logic for, or would get wrong:

| case | `rendered.…navbar` | `navigation.navbar` + `is_feature_disabled` |
| --- | --- | --- |
| ordinary navbar | non-empty ✓ | present, not disabled ✓ |
| per-page `navbar: false` | absent ✓ | needs the second read |
| user pre-rendered `rendered.navigation.navbar`, no `navigation.navbar` config | non-empty ✓ | **absent — wrong** |
| `navbar: false` **plus** a user-supplied `rendered.navigation.navbar` | non-empty ✓ | **reads "disabled" — wrong** |

That last row is the sharpest: `NavbarRenderTransform` returns early at
`navbar_render.rs:66`, leaving the user's HTML in place, and the template emits
it. A navbar ships. Only the rendered-key read stays consistent with what
actually reaches the page.

It is also the predicate `quarto_nav_js.rs` already uses for the same question
(finding 3), so the two stay consistent by construction.

### D3. Pass page context as a named options struct, not a fourth positional `bool`

**Decision:** replace `sidebar_to_html_with_appended(sidebar, home_url, appended)`
with `sidebar_to_html_with_options(sidebar, &SidebarRenderOptions { … })`.
`sidebar_to_html(sidebar, home_url)` keeps its signature and becomes a
delegation.

Rationale:

- `appended_html` was already tacked on as a trailing positional once. A call
  reading `(…, toc_block.as_deref(), true)` is the classic boolean-parameter
  smell — two unlabelled trailing args, one a bare `bool`. The next parity fix
  (logo, tools, search) would make it five positionals.
- It is the faithful analogue of Q1, which hands `sidebar.ejs` a **named param
  bag** (`{ sidebar, sidebarStyle, navbar, toc, language, draftMode }`).
- The cost is low: `sidebar_to_html_with_appended` has exactly **2** call sites,
  both in `crates/quarto-core/src/transforms/sidebar_render.rs` (`:178`, `:254`).
  It is not re-exported from `quarto-navigation/src/lib.rs` (only `pub mod
  render_html` at `:26`), and no other crate uses it. The **20** test call sites
  all use `sidebar_to_html`, which is untouched.

`sidebar_to_html_with_appended` is **removed**, not kept as a deprecated shim —
it has no external consumers, so leaving both would be dead surface.

### D4. `synthesize_toc_sidebar` passes the real `has_navbar`, though it cannot matter

The second call site (`sidebar_render.rs:254`) builds a TOC-only sidebar from
`Sidebar::with_defaults()`, whose `title` is `SidebarTitle::Default`
(`sidebar.rs:417`) — so no header is emitted regardless of the flag. Pass the
real value anyway rather than a hardcoded `false`, so the call site does not
encode an assumption a future change to `with_defaults()` could silently
invalidate.

## Reproduction fixture

Self-contained; recreate under a scratch directory (not `/tmp` — use the
session scratchpad or a project-local dir).

`with-navbar/_quarto.yml`:

```yaml
project:
  type: website
website:
  title: "Site Title"
  navbar:
    left:
      - text: Home
        href: index.qmd
  sidebar:
    - title: "Guides"
      contents:
        - index.qmd
        - two.qmd
```

`without-navbar/_quarto.yml` is identical with the whole `navbar:` block
removed. Each project holds two pages. `index.qmd`:

```
---
title: Index
---

Hello.
```

`two.qmd` is the same with `title: Two` and body `Two.`

Then, from each project directory:

```bash
cargo run --bin q2 -- render
grep -c 'sidebar-title' _site/index.html
```

Use `cargo run --bin q2 --` (or an explicit `target/debug/q2` path), **never a
bare `q2` / `quarto`** — `CLAUDE.md` warns the ambient `quarto` may be a
quarto-cli dev checkout, which would silently compare against Q1.

The upstream fixture (with committed Q1 output for comparison) lives outside
this repo at
`/Users/gordon/src/q2-positron-docs/llms-info/repros/sidebar-title-with-navbar/`.
Not required — the inline fixture is sufficient.

## Work plan

**Sequencing principle.** The two refactors and the fixture split are all
**behaviour-preserving**, so they land first, each with a genuinely green gate.
The `docs/_quarto.yml` cleanup travels *with* the gate in Phase 4, never before
it — see §Expected collateral for the measurement that forced that ordering.
The behaviour change then lands in a single phase where the two tests that can
be red go red → green **on assertions, not on compile errors** (the third new
test asserts current behaviour and is green from the start — it is the A/B
partner, see 4.2).

This is deliberate: an earlier draft interleaved them and produced phase gates
that were red by construction, plus a "watch it fail" step that only ever
produced a missing-symbol compile error.

Commit at each phase boundary (project rule for approved plan execution).
**Never push without explicit approval.**

### Prerequisite for any `q2 render docs/` step (3.3 and 4.6)

**`q2 render docs/` fails on a clean worktree.** `docs/_quarto.yml:6-7` declares
`resources: - examples`, and `docs/examples/` is generated and gitignored:

```
Rendering project: …/docs (type: website)
Error: Declared resource '…/docs/examples' does not exist on disk
```

(emitted by `project_resources.rs:1194`). Reproduced directly on this branch.

**`cargo xtask stage-doc-examples` does not fix this from inside a worktree.**
`crates/xtask/src/stage_doc_examples.rs:43` calls
`create_worktree::repo_root()`, which walks up to the `Cargo.toml` carrying
`[workspace]` — shared across worktrees, so it resolves to the **main repo**.
Running it here stages into `/Users/gordon/src/q2/docs/examples/` and leaves the
worktree's own `docs/examples/` empty.

Either run the xtask and copy the staged tree across, or render `docs/` from the
main checkout. This worktree already has a staged copy (gitignored, persists),
so 3.3/4.6 will run here as-is — but do not assume that elsewhere.

> **Already tracked: `bd-32c8egkf`** (open, filed by Carlos 2026-08-06), linked
> `related` to this strand. This is its **fifth** independent hit —
> `bd-afi4avsf`, `bd-u7kdy6fy`, `bd-rbhfv5xx` were all closed as duplicates, and
> Gordon hit it from `workspace-5` on 2026-08-24. Do **not** file another; a
> comment recording this encounter is already on it. Out of scope here.

### Phase 0 — Baseline

- [x] **0.1** Record the workspace baseline on a clean tree at the base commit.
      Re-measured after the 2026-08-26 rebase onto `c11aa0e4d`:

      ```
      Starting 13447 tests across 77 binaries (199 tests skipped)
      Summary [119.008s] 13447 tests run: 13447 passed, 199 skipped
      ```

      Every later delta is reported against **this** figure. (The pre-rebase
      figure at `99e7db175` was `13407 passed, 199 skipped`; the +40 is other
      people's work that landed in between, not ours.)

- [ ] **0.2** Repoint `claude-notes/plans/CURRENT.md` at this plan
      (it currently targets `2026-08-25-scheme-href-path-normalized.md`).

### Phase 1 — `SidebarRenderOptions` (behaviour-preserving)

- [ ] **1.1** In `crates/quarto-navigation/src/render_html.rs`, add
      `SidebarRenderOptions<'a>` with fields `home_url: &'a str`,
      `appended_html: Option<&'a str>`, `has_navbar: bool`, each documented.
      The type doc should name Q1's param bag in `nav-before-body.ejs` as the
      model (see §Root cause).

- [ ] **1.2** Rename `sidebar_to_html_with_appended` to
      `sidebar_to_html_with_options(sidebar: &Sidebar, opts: &SidebarRenderOptions) -> String`
      and rewrite its body to read `opts.home_url` / `opts.appended_html`.
      Redefine `sidebar_to_html(sidebar, home_url)` to delegate with
      `appended_html: None, has_navbar: false`.
      **Do not add the gate yet** — `has_navbar` is carried but unread.

- [ ] **1.3** Update the call sites. §D3 counts **2 external** ones, but the
      rename touches **4 places**:
      - `crates/quarto-core/src/transforms/sidebar_render.rs:50` — the `use`
        importing `render_html::sidebar_to_html_with_appended`;
      - `sidebar_render.rs:178` and `:254` — the two calls, passing
        `has_navbar: false` for now;
      - `crates/quarto-navigation/src/render_html.rs:361` — `sidebar_to_html`'s
        internal delegation.

- [ ] **1.4** Gate: `cargo clippy -p quarto-navigation -p quarto-core --all-targets -- -D warnings`
      then `cargo nextest run -p quarto-navigation -p quarto-core`.
      **Must be fully green** — nothing has changed behaviourally. The unread
      `has_navbar` will *not* trip `dead_code`: `SidebarRenderOptions` is a
      `pub` struct with `pub` fields in a `pub mod` of a library crate, so it is
      publicly reachable and the lint does not fire. If something unexpected
      does object, fix it in place — do **not** reach forward and add the gate
      early, which would undo the sequencing this plan just established.

### Phase 2 — `page_has_navbar` (behaviour-preserving)

- [ ] **2.1** Add to `crates/quarto-core/src/transforms/config.rs`, next to
      `is_feature_disabled` (`:23`) and `resolve_website_bool` (`:38`):
      ```rust
      pub fn page_has_navbar(meta: &ConfigValue) -> bool
      ```
      reading `rendered.navigation.navbar` via `as_plain_text()` and testing
      non-empty. Doc it as Q1's `navbar: !!nav.navbar`, record D2's reason for
      reading the *rendered* key, and note that callers must run after
      `NavbarRenderTransform`.

      **Use `as_plain_text()`, not `as_str()`** — the `metadata-as-str` lint rule
      exists because `as_str()` returns `None` for
      `ConfigValueKind::PandocInlines`.

- [ ] **2.2** Export it: `mod config;` is **private**
      (`transforms/mod.rs:43`), and the crate convention is the re-export list
      at `transforms/mod.rs:115`. Add `page_has_navbar` there, the way
      `navbar_render.rs:36` reaches `is_feature_disabled`.

- [ ] **2.3** Unit-test `page_has_navbar` in `config.rs`'s test module (`:200`):
      absent key → `false`; empty-string value → `false`; non-empty → `true`.

      **The existing `meta_with` in that module (`config.rs:207`) will not
      serve** — it builds a flat one-level map, and this needs the 3-level path
      `rendered.navigation.navbar`. Use the pattern from
      `quarto_nav_js.rs:184`: `ConfigValue::null(SourceInfo::for_test())` then
      `insert_path`. Either copy that helper or generalise it.

- [ ] **2.4** Refactor `quarto_nav_js.rs::decide` (`:98-104`) onto the shared
      helper for the `navbar` half, keeping the local closure for
      `secondary-nav`, or generalise the helper — implementer's call, but the
      two copies of the navbar predicate must not both survive. Its four
      predicate tests (`:201`, `:216`, `:233`, `:245`) must stay green
      **unchanged**.

- [ ] **2.5** Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings`;
      `cargo nextest run -p quarto-core -E 'test(page_has_navbar) or test(ships_) or test(empty_rendered_navbar)'`.
      Fully green. (Note: `test()` is a **substring match on the test name** —
      `test(config)` would match unrelated tests and miss `page_has_navbar`
      entirely.)

### Phase 3 — Split the shortcode fixture (behaviour-preserving)

- [ ] **3.1** `sidebar_title_shortcode_substitutes`
      (`crates/quarto-core/tests/integration/shortcode_config_pipeline.rs:203`)
      renders `full_fixture` (`:122`), which declares **both** a navbar and
      `sidebar.title: "Side {{< meta version >}}"`, and asserts `"Side 9.9.9"`
      appears. It is the one existing test the gate breaks (§Blast-radius
      sweep).

      The fixture's navbar is load-bearing for a sibling —
      `website_title_shortcode_substitutes_in_navbar_brand` (`:178`) asserts on
      `navbar-brand` — so **do not remove the navbar from `full_fixture`**.
      Give the sidebar-title test its own navbar-free fixture (same `version:`
      and `sidebar.title:`, no `navbar:`). `headroom_pipeline.rs:133` already
      uses the name `sidebar_only_fixture` for exactly this shape — follow it.

      Comment the new fixture with **why** it exists and this strand id;
      otherwise the next reader will "helpfully" re-merge the two.

- [ ] **3.2** Gate: `cargo nextest run -p quarto-core -E 'test(shortcode_config)'`.
      Fully green — a navbar-free fixture substitutes the shortcode identically
      either side of the gate, which is what makes this safe to land first.

- [ ] **3.3** Capture the **pre-fix** docs baseline, for comparison in 4.6.
      Already measured at the rebased base `c11aa0e4d` (recorded in §Expected
      collateral). Re-run only if something touching navigation rendering or
      `docs/_quarto.yml` has landed since — note the errors sidebar grows by one
      entry every time an error page is added, so the `Error reference` count
      drifts on its own:
      ```bash
      cargo run --bin q2 -- render docs/
      grep -rho '<div class="sidebar-title[^>]*><a[^>]*>[^<]*' docs/_site \
        --include='*.html' | sed 's/.*>//' | sort | uniq -c | sort -rn
      ```
      Expected: `224 Error reference`, `34 Quarto 2`, `1 Presentations`.

      Use `cargo run --bin q2 --`, never the ambient `quarto`: `CLAUDE.md` is
      explicit that `docs/` is a Quarto **2** site and Q1 gives misleading
      results on it.

### Phase 4 — The behaviour change (TDD)

- [ ] **4.1** Write the failing tests. Everything compiles now, so these fail on
      **assertions**.

      In `crates/quarto-navigation/src/render_html.rs`, after
      `sidebar_render_hidden_title_emits_no_header` (ends ~`:2587`):
      - `sidebar_render_text_title_with_navbar_emits_no_header` — a
        `SidebarTitle::Text` sidebar rendered via `sidebar_to_html_with_options`
        with `has_navbar: true` emits neither `sidebar-header` nor
        `sidebar-title`.
      - `sidebar_render_text_title_without_navbar_emits_header` — the same
        sidebar with `has_navbar: false` emits both. (Near-duplicate cover for
        `sidebar_render_text_title_emits_header_with_link` at `:2590`; keep it
        so the pair reads as an explicit A/B on the new flag.)

      In `crates/quarto-core/tests/integration/sidebar_pipeline.rs`, next to
      `pipeline_renders_website_title_in_sidebar_header_by_default` (`:410`):
      - `pipeline_omits_sidebar_header_when_navbar_present` — copy that test's
        fixture, add a `navbar:` block; assert the rendered `index.html`
        contains neither `sidebar-header` nor `sidebar-title`, and — as a
        positive control that the fixture is wired — that it *does* contain
        `navbar-brand`.

- [ ] **4.2** Confirm all three fail **on their assertions**, not on a panic in
      fixture setup and not on a compile error:
      ```
      cargo nextest run -p quarto-navigation -E 'test(sidebar_render_text_title_with_navbar)'
      cargo nextest run -p quarto-core -E 'test(pipeline_omits_sidebar_header_when_navbar_present)'
      ```
      (`sidebar_render_text_title_without_navbar_emits_header` passes already —
      it asserts current behaviour. Only the `with_navbar` one is red here.)

- [ ] **4.3** Add the gate in `render_html.rs`. Edition is 2024 and let-chains
      are already used in this workspace (`appendix.rs:192`):
      ```rust
      if let SidebarTitle::Text(ref title_cv) = sidebar.title
          && !opts.has_navbar
      {
      ```
      Extend the existing comment above the block to state Q1's `!navbar` gate
      (`sidebar.ejs:51`), *why* it exists (**navbar-item duplication** — the
      navbar item for this section already renders active, so the sidebar
      banner restates it; **not** "it duplicates the brand", which is false —
      see §What the rule actually is), the strand id, and the forward note from
      finding 1.

      **Expect a partial green here.** The two renderer unit tests go green at
      this step; `pipeline_omits_sidebar_header_when_navbar_present` stays
      **red** until 4.4, because `sidebar_render.rs` is still passing the
      Phase-1 hardcoded `false`. That is not 4.3 failing to take — there is no
      gate between 4.3 and 4.4 for exactly this reason.

- [ ] **4.4** Wire the real value in `sidebar_render.rs`: compute
      `let has_navbar = page_has_navbar(&ast.meta);` and pass it at both call
      sites, replacing the Phase-1 `false`.

      `synthesize_toc_sidebar` may either take the value as a parameter or
      recompute it from `ast.meta`; both compile (the read returns a `bool`, so
      the immutable borrow ends at the statement and NLL accepts it anywhere
      before the later `insert_path`). See D4 for why it passes the real value
      rather than `false`.

- [ ] **4.5** **Same commit as the gate**: delete the two now-dead `title:` lines
      in `docs/_quarto.yml` — `:77` (`title: "Presentations"`) and `:84`
      (`title: "Error reference"`). With the gate in place they render nothing,
      so this is dead-config cleanup. See §Expected collateral for why it must
      not be done earlier (deleting them pre-gate substitutes `website.title`
      and makes 224 pages read `Quarto 2` instead).

- [ ] **4.6** Re-render `docs/` and compare against the 3.3 baseline:
      ```bash
      cargo build --bin q2 && cargo run --bin q2 -- render docs/
      grep -rl 'sidebar-title' docs/_site --include='*.html' | wc -l
      grep -c 'Reveal.js' docs/_site/presentations/revealjs/index.html
      grep -c '>yaml<'    docs/_site/errors/index.html
      ```
      Expected: `sidebar-title` page count **259 → 0**, while the inner
      `- section:` labels survive (`Reveal.js` still 5, `>yaml<` still 1 —
      §Expected collateral's central claim, checked on our own site). Append the
      observed numbers to this plan.

      A non-zero `sidebar-title` count means a sidebar is rendering a title
      under a navbar — the gate did not take. A dropped section label means the
      gate is over-reaching. Either way, stop.

- [ ] **4.7** Update the `sidebar_render.rs` module doc's §"Skip conditions"
      list to record that a page with a navbar renders no sidebar title.

- [ ] **4.8** Add the pipeline ordering comment above
      `SidebarRenderTransform::new()` (`pipeline.rs:1395`): SidebarRender MUST
      come after NavbarRender because its title gate reads
      `rendered.navigation.navbar`.

      Model the *prose* on the `QuartoNavJsTransform` comment block (`:1416`,
      push at `:1424`) — but **not its structure**: that push is
      `#[cfg(not(target_arch = "wasm32"))]`, whereas `SidebarRenderTransform` is
      unconditional and must stay that way (see 5.5 on the preview).

- [ ] **4.9** Add `test_sidebar_render_registered_after_navbar_render` in
      `pipeline.rs`'s test module, modelled on
      `test_quarto_nav_js_registered_after_nav_renders` (`:3704`): assert
      `sidebar-render` is in `TransformPhase::Navigation` and positioned after
      `navbar-render`.

- [ ] **4.10** Gate:
      `cargo clippy -p quarto-navigation -p quarto-core --all-targets -- -D warnings`;
      `cargo nextest run -p quarto-navigation -p quarto-core`. Fully green —
      including `sidebar_title_shortcode_substitutes`, already repaired in
      Phase 3.1.

### Phase 5 — Verification

- [ ] **5.1** `cargo nextest run --workspace`, captured to a log file and
      inspected with `grep`/`tail` (do **not** pipe nextest through `tail`
      inline). Report the delta against the Phase 0 baseline
      (`13447 passed, 199 skipped`). Expected: **+7 passed** (2 renderer unit +
      3 `page_has_navbar` unit + 1 integration + 1 pipeline ordering), no
      removals, skips unchanged — i.e. **13454 passed, 199 skipped**.

      **That baseline is pinned to `c11aa0e4d` and goes stale on any further
      rebase.** It already moved once: at `99e7db175` it was 13407, and 24
      commits of other people's work took it to 13447. If you rebase again,
      re-measure on the new base before claiming a delta; never subtract from a
      figure copied out of this document.

- [ ] **5.2** **End-to-end through the binary.** `cargo build --bin q2`, then run
      the §Reproduction fixture. Record in the session transcript **and** append
      to this plan: the exact invocation, the observed `grep -c 'sidebar-title'`
      counts for both fixtures, and an explicit note that the output was
      inspected. Expected: `with-navbar → 0`, `without-navbar → 1`.

      Also confirm the with-navbar page still contains `navbar-brand` (the
      navbar is unaffected) and that `nav#quarto-sidebar` still contains its
      `sidebar-menu-container` (only the header went away, not the menu).

- [ ] **5.3** Docs already verified in 4.6. Re-run it only if anything landed
      after that step. Do **not** commit `docs/_site/` — it is build output.

- [ ] **5.4** `cargo xtask lint`.

- [ ] **5.5** `cargo xtask verify`. **Full, not `--skip-hub-build`**:
      `quarto-core` is in `wasm-quarto-hub-client`'s dependency closure, and
      `quarto-navigation`'s public API changes here.

      Optional: `q2-preview-spa/e2e/chrome.spec.ts:217` reads `.sidebar-title`
      and only runs under `cargo xtask verify --e2e`. Its fixture
      (`examples/websites/02-auto-sidebar/`) has **no navbar**, so it is
      provably unaffected — run it only if a browser leg is cheap.

      **The preview changes too.** `SidebarRenderTransform` is registered
      unconditionally at `pipeline.rs:1395` (only `quarto-nav-js` is cfg-gated),
      so `q2 preview` and the hub-client preview also stop showing sidebar
      titles on navbar sites. That is correct and intended — preview/render
      parity is preserved, not broken — but state it when reporting, since
      parity is a tracked concern in this repo.

      Note: full `verify` rebuilds the WASM and SPA bundle but **not** the `q2`
      binary's `include_dir!` re-embed. If anyone will eyeball `q2 preview`
      after this chrome change, re-run `cargo build --bin q2` afterwards
      (`CLAUDE.md` §Verifying Rust changes in `q2 preview`).

- [ ] **5.6** Reconcile this plan against what actually happened — verify every
      checkbox against the landed work, correct any that are wrong, and commit
      the updated plan.

- [ ] **5.7** `braid close bd-sidebar-title-with-navbar-82wxow6m --reason "..."`.

## Blast-radius sweep

Repo-wide sweep, run on a **clean tree** (before any `_site/` build output
existed):

```bash
grep -rl 'sidebar-title' . | grep -vE '^\./(target|external-sources|\.git)/' \
  | grep -v node_modules | grep -vE '/_site/'
```

Finds **20** files. Every one was checked. The `_site/` exclusion matters: once
you render `docs/` (Phase 3.3) the same grep returns ~260 more hits, all build
output.

**The one breakage:**

- `crates/quarto-core/tests/integration/shortcode_config_pipeline.rs` —
  `sidebar_title_shortcode_substitutes` (`:203`). Repaired in Phase 3.1.

**Live code and tests — verified safe:**

| file | why safe |
| --- | --- |
| `crates/quarto-navigation/src/render_html.rs` | the emitter + its own tests. Its 20 in-file test call sites (a coincidence that this matches the file count above) all use `sidebar_to_html`, i.e. `has_navbar: false` |
| `crates/quarto-core/tests/integration/sidebar_pipeline.rs` | contains **zero** occurrences of `navbar`; its three title tests (`:410`, `:444`, `:474`) are all navbar-free |
| `crates/quarto-core/src/resource_resolver.rs` | doc comment only |
| `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt` | **byte-identity hash fixture** — the `sidebar-title` mentions are in re-capture comments. Its `doc.qmd` is a single document with no website config, so `doc.html` has no sidebar at all, and `doc_files/styles.css` is SCSS-derived (no SCSS changes here). Named explicitly because a hash fixture is the kind of thing that breaks silently. |
| `q2-preview-spa/e2e/chrome.spec.ts:217` | asserts `.sidebar-title` is `'Auto Sidebar'`, against `examples/websites/02-auto-sidebar/`, whose `_quarto.yml` has **no navbar**. Also `--e2e`-only. |
| `resources/scss/bootstrap/_bootstrap-rules.scss:508-514` (plus `.sidebar-title > a` at `:516-519`) | SCSS rule; inert with no matching element |

**Generated / reference / prose — no consumer:**
`examples/websites/06-site-metadata/q1-site/{index,api,guides}.html` (committed
Q1 reference output; that example has a sidebar and **no** navbar, so it stays
consistent), vendored `bootstrap*.min.css` under `old-docs/` and
`claude-notes/`, `.beads/issues.jsonl`, `.braid/snapshot.jsonl`, and four
`claude-notes/plans/*.md`.

**Fixtures declaring both `navbar:` and `sidebar:`** — five integration files,
checked individually:

- `shortcode_config_pipeline.rs` — the breakage above.
- `headroom_pipeline.rs` — clean separation: `navbar_fixture` (`:112`) has no
  sidebar, `sidebar_only_fixture` (`:133`) has no navbar, and nothing in the
  file asserts on `sidebar-header`/`sidebar-title`.
- `llms_txt.rs` — its titled-sidebar fixture (`:352`) has no navbar, **and** its
  assertions read `llms.txt` (data model), not sidebar HTML. Doubly safe.
- `idempotence.rs::website_chrome` (`:672`) — renders twice and compares; output
  changes identically on both runs.
- `metadata_path_resolution.rs` — asserts on rewritten hrefs.

**No `examples/websites/*` fixture declares both**: `04-navbar-footer` is the
only one with a `navbar:` and it has no sidebar.

**No `.snap` file anywhere references `sidebar-title` or `sidebar-header`.**

**`sidebar-header` sweep** (Phase 4.1 asserts on that string too, and the
`sidebar-title` grep above does not cover it). Three additional files match, all
hub-client's own file-tree component — a different DOM with no relation to the
website sidebar, so all are unaffected:

- `hub-client/src/components/FileSidebar.tsx:578`
- `hub-client/src/components/FileSidebar.css:14`
- `hub-client/e2e/files-header.spec.ts:47` — measures `.sidebar-header`'s
  bounding box in the hub UI

Recorded explicitly because a reviewer of an earlier draft asserted these files
do not exist. They do; they are simply irrelevant.

## Out of scope

- **Sidebar logo / tools / search.** Q2's `Sidebar` has none of these fields.
  This plan only leaves a comment marking where the gate must split when a
  `logo` field arrives (finding 1).
- **`sidebar.subtitle`.** Parsed but not rendered; unchanged here.
- **`sidebar.alignment`.** Q1 computes `alignCss` (`text-center` / `text-end` /
  `text-left`, `sidebar.ejs:7-14`); q2 hardcodes `text-left`
  (`render_html.rs:411`). A real parity gap, and a genuine out-of-plan
  digression — so it belongs in a strand, not this checklist. Search the skein
  first, then:
  `braid create "Sidebar alignment: Q1's alignCss vs q2's hardcoded text-left" -t bug -p 3 -l navigation -l parity --deps discovered-from:bd-sidebar-title-with-navbar-82wxow6m --json`
- **User-facing docs prose.** `docs/guides/authoring/navigation.qmd` covers the
  TOC and sidebar *location* only; nothing there documents sidebar titles, and
  this change restores Q1's behaviour rather than introducing a knob. (Distinct
  from §Expected collateral, which is about the docs site's own `_quarto.yml`
  configuration — the two dead `title:` lines removed in Phase 4.5 — not its
  prose.)
- **New error codes.** None; no catalog or errors-sidebar work.

# Sidebar `auto:` expansion + membership contract

**Status:** Initial draft, 2026-04-27 (Phase 8 sub-phase 8.0).
**Code:**

- `crates/quarto-core/src/transforms/sidebar_auto.rs` —
  `expand_auto` (the underlying expansion algorithm).
- `crates/quarto-core/src/project/sidebar_membership.rs` —
  `resolve_sidebar_membership` (Pass-1 / static membership query).
- `crates/quarto-core/src/transforms/sidebar_generate.rs` —
  Pass-2 transform that adds enrichment, active-state marking,
  and per-page sidebar selection on top of the same underlying
  expansion.
- `crates/quarto-navigation/src/sidebar.rs` —
  `Sidebar::parse_list_from_config`, `sidebar_for_page`,
  `resolve_active_state` (the format-agnostic data-model layer).

## What this contract is for

A Quarto sidebar can be declared statically in `_quarto.yml` (or
in a document's frontmatter) with one or more `auto:` directives
that enumerate pages from the project. The Phase-8 dependency
graph needs to know **which project documents are members of which
sidebar** at Pass-1 time (read-only, no engine / filter / Pass-2
transform involvement) so it can:

1. Add a co-membership edge for every pair of pages that share a
   sidebar (a title change to one ripples to siblings'
   rendered output).
2. Identify prev/next neighbors from the sidebar's flattened
   member order.

The same expansion algorithm runs in Pass-2 to render the actual
sidebar HTML. The two halves must agree on the membership set —
otherwise the dependency graph could miss an edge that ends up in
the rendered output, and warm renders would show stale sidebars.

## Inputs

- `meta`: the document's merged metadata. Read-only.
- `index`: the project's `ProjectIndex` (every rendered page's
  `DocumentProfile`). Read-only.
- `diagnostics`: a mutable buffer for warnings emitted during
  expansion (unresolved `auto:` paths, draft pages, etc.). The
  Pass-1 helper appends to this; Pass-2 channels them through to
  the per-doc `RenderContext`.

## Output

A `Vec<ResolvedSidebar { id, members }>`, one entry per sidebar
declared under `meta.website.sidebar`. Each `members` list is the
project-relative source paths of every page that appears in the
resolved sidebar, in document order, deduplicated by path
(first-occurrence wins).

## Algorithm

1. **Read `meta.website.sidebar`.** Absent → return empty Vec.
   Present-but-empty → return one empty `ResolvedSidebar`.

2. **Parse the config slice into one or more `Sidebar` values.**
   `Sidebar::parse_list_from_config` accepts:
   - A single `Sidebar` object: `{ id, title, contents, ... }`.
   - An array of `Sidebar` objects.
   - A bare contents array (treated as a single anonymous sidebar).

3. **For each sidebar, run `expand_auto` against `index`.** This
   walks `sidebar.contents` and replaces every `SidebarEntry::Auto(spec)`
   with a flat list of `SidebarEntry::Link` entries chosen
   from the index according to `spec`:

   - `auto: true` → every non-draft, non-top-`index.qmd` page in
     the index.
   - `auto: <path>` → every page under that subdirectory.
   - `auto: [a.qmd, b.qmd]` → exactly those listed pages.
   - Order: the index's discovery order (which is deterministic
     by Phase-1 invariant), filtered to match the spec.
   - Drafts (`profile.draft == true`) are skipped silently.
   - Unresolved entries (paths with no profile) emit a warning
     diagnostic and are dropped.

4. **Walk the resolved entries and collect member paths.** For
   each entry:
   - `Link { item }` with an `href` that's not external (no
     scheme, not `//`, not starting with `#`): record `href` as
     a project-relative path.
   - `Section { href, contents, ... }`: if `href` is project-
     relative, record it; recurse into `contents`.
   - `Separator`, `Heading(_)`, unexpanded `Auto(_)` (defensive;
     should not survive `expand_auto`): skip.
   - Dedupe by path: a page that appears twice in the same
     sidebar (rare but legal) gets one membership entry.

## Examples

| Config | Resolved members |
|---|---|
| `contents: [a.qmd, b.qmd]` | `[a.qmd, b.qmd]` |
| `contents: [{auto: true}]` (index has 3 pages) | `[a.qmd, b.qmd, c.qmd]` |
| `contents: [{section: "Group", contents: [a.qmd, b.qmd]}]` | `[a.qmd, b.qmd]` |
| `contents: [a.qmd, https://example.com]` | `[a.qmd]` (external dropped) |
| `contents: [a.qmd, a.qmd]` | `[a.qmd]` (deduped) |

## Pattern semantics in `auto:`

Since bd-mt7a6uc4, an `auto:` path is a **real glob**, matched with
q2's shared glob API (`crates/quarto-core/src/glob/`, contract:
`claude-notes/designs/glob-semantics.md`). Patterns resolve against
the **project root** — `auto:` enumerates project pages, and
`AutoSpec` carries no provenance, so there is no declaring-file
directory to anchor to.

| Config | Members |
|---|---|
| `auto: docs` | everything beneath `docs/` (bare directory rule) |
| `auto: "docs/*"` | documents directly in `docs/`, not nested |
| `auto: "docs/*.qmd"` | same, restricted to `.qmd` |
| `auto: "docs/**/*.qmd"` | `.qmd` anywhere beneath `docs/` |
| `auto: ["docs", "!docs/internal"]` | `docs/` minus `docs/internal/` |
| `auto: "docs/ch-[0-9].qmd"` | numbered chapters only |

**This changed behavior** (2026-08-06). `auto:` previously stripped
`*.qmd` / `**` / `*` off the end of each entry and prefix-matched what
remained, so `docs/*.qmd`, `docs/**`, `docs/` and `docs` were all the
same pattern and every one of them swept up nested documents. A
project relying on `docs/*.qmd` to include `docs/deep/nested.qmd` must
now write `docs` or `docs/**/*.qmd`. The bare-directory spelling —
by far the most common — is unaffected.

## Equivalence with Pass-2 sidebar render

For a fixed `(meta, index)`:

- `resolve_sidebar_membership(meta, index, _)` returns the same
  member set per sidebar that `SidebarGenerateTransform` would
  produce after `expand_auto` runs but before
  enrichment / active-state / rendering.

- The two helpers literally call the same `expand_auto` function,
  so any future change to expansion logic is shared automatically.

## What this contract does NOT cover

- **Active-state marking.** Phase-2's
  `resolve_active_state` flips `active` flags on entries so the
  current page is highlighted in the rendered sidebar. The
  dependency graph doesn't need this; Pass-1 membership skips it.
- **Bare-href text enrichment.** Phase 2 fills in display text
  from index titles when the user supplied only a path. The
  dependency graph doesn't need text either.
- **Sidebar-for-page selection.**
  `sidebar_for_page(sidebars, page_source, meta)` picks *which*
  declared sidebar applies to a given page (e.g. the one that
  contains the page, or the one whose `id` matches an explicit
  `site-sidebar:` override). Phase-1 membership returns *all*
  sidebars; the graph builder uses every one.
- **Per-page sidebar overrides via `site-sidebar:`.** Same
  reason — the membership contract returns sidebars uniformly,
  and the graph is built against all of them.
- **Sidebar ordering for prev/next.** The Pass-2 page-nav
  transform orders prev/next by traversing the fully-resolved
  sidebar; Phase 8's dependency graph derives prev/next neighbor
  edges from the same flattened member sequence (which `members`
  here exposes in document order).
- **Multi-sidebar ambiguity diagnostics.** If a page appears in
  more than one declared sidebar, both are reported; the user
  resolves the ambiguity via `site-sidebar:` (which is Pass-2 /
  per-page concern, not membership).

## When to bump this contract

Any change to `expand_auto`'s behavior that would alter the set
of pages a `auto:` directive resolves to, or to the
`collect_member_paths` walk, requires:

1. Updating this document.
2. Bumping `PROFILE_KEY_VERSION` in `cache_key.rs` (Phase 8.1) —
   the dependency-graph structure changes, so cache hits would
   serve stale data.

Adding new sidebar entry kinds (e.g. a new `SidebarEntry::Foo`
variant) requires extending the walk explicitly. The default
match arms are exhaustive; the compiler will surface any missed
case.

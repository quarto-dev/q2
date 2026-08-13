# Sidebar `contents: <directory>` shorthand renders a broken sidebar (bd-sidebar-contents-dir-shorthand-z7arvhx8)

**Date:** 2026-08-12
**Braid:** `bd-sidebar-contents-dir-shorthand-z7arvhx8` (bug, p1, label `navigation`)
**Branch:** `braid/bd-sidebar-contents-dir-shorthand-z7arvhx8`, off `main` @
`152ed8fb`, in the **main checkout** (no worktree — user's call, Q5).
**Also fixes:** `bd-4feoon8u` (multi-sidebar `auto:` selection).
**Status:** Design settled 2026-08-13 — implementing.

## Triage verdict

**Ready to design** — the bug is real, reproducible at HEAD, and precisely
located. But the fix the strand proposes is **necessary and not sufficient**:
it repairs the minimal repro while leaving the real-world Connect-docs failure
in place. Two further decisions were needed before implementation; both are
settled below.

## Issue context

A website sidebar whose `contents:` is a bare directory name renders no usable
navigation. Q1 treats the directory as an auto-generation source; q2 treats the
string as a single href and emits one dead link — and in a multi-sidebar project
it emits no sidebar element at all.

Filed 2026-08-13 by Claude (q2-connect-docs) against q2 0.19.0, re-verified on
0.20.0. Priority 1. Origin strand `br-sidebar-contents-dir-shorthand-8rvnztv6`
lives in the connect-docs skein.

## Dependency graph

**Empty.** `braid dep tree` shows the strand alone; `braid dep list` returns no
edges. The `discovered-from` context the strand references
(`br-sidebar-contents-dir-shorthand-8rvnztv6`, and `br-wu5cbkws` for the Connect
impact) is in a *different skein* and is not reachable from here — the strand
description is the only carrier of that context, which is why it is unusually
detailed.

No incoming `blocks` edges, so nothing in this skein is waiting on it. The
urgency is external: the Connect docs port.

## What the code looks like today

Every file and line the strand cites still exists and has the described shape.
Nothing has touched `crates/quarto-navigation` since the v0.20.0 release merge.

### Root cause (confirmed)

`parse_contents` — `crates/quarto-navigation/src/sidebar.rs:528` — special-cases
exactly one bare string:

```rust
if let Some(s) = cv.as_plain_text() {
    if s == "auto" {
        return vec![SidebarEntry::Auto(AutoSpec::All)];
    }
    return vec![SidebarEntry::from_plain_string(&s, cv.source_info.clone())];
}
```

`from_plain_string` (`:288`) classifies a 3+-dash run as a separator and
*everything else* as an href. So `contents: guides` becomes
`SidebarEntry::Link { href: "guides" }`.

Confirmed in the repro's committed output — `_site/guides/first.html` contains
`href="guides"`, which from `guides/first.html` resolves to `guides/guides`.
(Minor correction to the strand: the element is `<nav id="quarto-sidebar">`, not
a `<div>`. Worth knowing when writing the regression assertion.)

### What Q1 actually does (read from source, not inferred)

`external-sources/quarto-cli/src/project/types/website/website-sidebar-auto.ts:96`,
`normalizeSidebarItems`:

```js
if (typeof items === "string") {
  if (items === "auto") items = [{ auto: true }];
  else                  items = [{ auto: items }];   // <-- the shorthand
}
```

So in Q1 **`contents: guides` is literally `contents: [{auto: "guides"}]`.** The
strand's suggested routing is exactly Q1's rule.

Two further Q1 facts settle the strand's open questions:

1. **The shorthand is scalar-only.** A bare string that is an *array element*
   goes through a different function — `normalizeSidebarItem` in
   `src/project/project-config.ts:24` — which makes it `{href}` if the path
   exists on disk and `{text}` (a plain label) otherwise. It is never turned
   into an `auto`. Ordering confirmed at `website-navigation.ts:1051-1058`:
   `expandAutoSidebarItems` runs on the whole `contents` first, then
   `normalizeSidebarItem` runs per resulting item. `expandAutoSidebarItems`
   also recurses into nested `item.contents`, so the shorthand applies to
   nested section `contents:` too.

   This resolves the strand's "one judgement call" (`contents: intro.qmd`):
   **Q1 makes no special case.** A scalar is always an `auto`; a scalar naming a
   single file simply auto-expands to that one file. No extension sniffing, no
   file-vs-directory branch is needed at the *parse* site.

2. **Q1's `auto: <dir>` also produces a section.** `globsFromAuto` rewrites an
   existing directory to `<dir>/**`, and `isAutoDir` (`:113`) re-shuffles the
   node set so the directory becomes a wrapping node. Q1 decides "is a
   directory" by filesystem existence (`safeExistsSync` + `isDirectory`).

Verified against the repro's committed Q1 output — `_site-q1/guides/first.html`
has a `sidebar-item-section` titled **"Guides"** (capitalized directory name,
because `guides/` has no `index.qmd`), expanded, with the two guides nested and
**no href on the section header**.

### The machinery q2 already has — and the gap

`section_for_dir` (`crates/quarto-core/src/transforms/sidebar_auto.rs:320`)
already builds exactly Q1's shape: section text from the directory's
`index.qmd` title or `capitalize(dir)`, href to that index when present, children
excluding the index. The repro's Q1 output matches its no-index branch precisely.

**But `AutoSpec::Path` never reaches it.** `collect_candidates` (`:200`) hardwires
`AutoSpec::Path` to `Scope::Flat`, so `expand_spec` calls `flatten_as_links`.
Only `AutoSpec::All` gets `group_with_subdirs`. This is deliberate and pinned by
`auto_path_scopes_to_subdir` (`:504`, "Test 21"), which asserts `auto: docs`
yields flat `Link`s.

So **routing the bare string to `AutoSpec::Path` alone produces a flat run of
links, not Q1's titled section.** The strand anticipated this ("confirm the
expansion produces Q1's section-with-title shape") — it is a real gap, and it is
where design question Q1 below comes from.

### The second defect: selection runs before expansion

This is not in the strand, and it is why the fix as proposed would **not** fix
the Connect docs.

In `SidebarGenerateTransform`
(`crates/quarto-core/src/transforms/sidebar_generate.rs`):

- `sidebar_for_page(...)` is called at **`:89`**
- `expand_auto(...)` is called at **`:127`**

Selection therefore operates on *unexpanded* contents. And `contains_source_path`
(`crates/quarto-navigation/src/sidebar.rs:647`) explicitly ignores
`SidebarEntry::Auto(_)` (`:663`).

`sidebar_for_page`'s rules explain both reported symptoms as one root cause:

- **Minimal repro (one sidebar, no `id`)** — Rule 2, the single-sidebar wildcard
  (`:637`), applies the sidebar regardless of containment. You get the sidebar
  *with* the dead link. Matches the observed output.
- **Connect docs (several sidebars)** — Rule 2 does not apply, so Rule 3
  (containment) runs. The only entry is `Link { href: "how-to" }`, which matches
  no page source, so **no sidebar is selected at all**. Matches the reported
  "no `#quarto-sidebar` element whatsoever".

Consequence: if the shorthand becomes `SidebarEntry::Auto(...)` and nothing else
changes, Rule 3 still cannot see through it, and the Connect-docs pages still get
no sidebar.

**This is confirmed empirically, not inferred.** A probe fixture at
`claude-notes/plans/sidebar-contents-dir-shorthand-investigation/multi-sidebar-auto/`
declares two `id:`-bearing sidebars — defeating the Rule-2 wildcard, the same
shape as the Connect docs — where one is defined by an explicit `auto: how-to`.
It uses only syntax q2 supports today. Rendered at `main` @ `152ed8fb`:

```
how-to/index.html      sidebar=0
how-to/one.html        sidebar=0
how-to/two.html        sidebar=0
other/alpha.html       sidebar=1
index.html             sidebar=0
```

So **a pre-existing bug exists today, independent of this strand**: a
multi-sidebar project using explicit `auto: <dir>` loses that sidebar entirely.
Filed as `bd-4feoon8u` (`discovered-from` this
strand), and fixed on this branch (D4).

Note `resolve_sidebar_membership`
(`crates/quarto-core/src/project/sidebar_membership.rs:77`) *does* expand before
walking, but it feeds Phase 8's dependency graph, not sidebar selection — it does
not compensate.

### Scope notes from the strand, resolved

- **Listings** — *not affected*. `crates/quarto-core/src/project/listing/config.rs:631`
  is an independent `parse_contents` that turns a scalar `contents:` into a
  `ListingContents::Glob`, which is already correct for listings.
- **Book chapters** — *nothing to check*. q2 has no `book.chapters:` parser;
  `ProjectKind::Book` (`project/mod.rs:317`) is only an accepted project-type
  string, and every `chapters` hit in the tree is a test fixture directory name.
  When books land, they should route through whatever this fix establishes.

## Design decisions (settled 2026-08-13)

**D1 — `auto: <bare dir>` produces a titled section too (full Q1 parity).**
One spelling, one behavior. `contents: <dir>` then routes to
`AutoSpec::Path` and needs no representation of its own.

The implementation falls out more cleanly than expected: `group_with_subdirs`
partitions candidates by their *first path component*. When every candidate is
beneath `how-to/`, `top_level` is empty and `dir_groups` has exactly one entry,
so it already returns `vec![section_for_dir("how-to", …)]` — precisely Q1's
shape. So D1 is **not** a new code path; it is choosing `Scope::All` instead of
`Scope::Flat` for a bare-directory `AutoSpec::Path`.

Consequence: `auto_path_scopes_to_subdir` ("Test 21", `sidebar_auto.rs:504`) is
rewritten — it asserts the behavior we are deliberately changing. Globs
(`auto: docs/*`, `auto: docs/**/*.qmd`) are unaffected and stay flat.

**D2 — Expand before selecting.** Hoist expansion above `sidebar_for_page` and
expand *every* declared sidebar, then pick. Correct by construction, and it is
also the fix for `bd-4feoon8u`.

The trap: this transform runs **per page**, so naively expanding all sidebars
would fire `Q-13-6` ("`auto:` matched no documents") for unselected sidebars on
every page. Expansion must therefore collect diagnostics **per sidebar**, and
only the picked sidebar's diagnostics may reach `ctx.diagnostics`. Same applies
to `strip_auto`'s `Q-13-5` on the no-index path.

**D3 — Directory-ness comes from the project index, not the filesystem.** A
pattern is a bare directory when it carries no glob metacharacter *and* at least
one indexed profile lives beneath it. No I/O, WASM-safe.

Accepted quirk (user-confirmed): an empty or unindexed directory is not
recognised as a directory and falls through to the `Q-13-6` empty-match warning.
Unavoidable under the WASM/automerge project representation.

**D4 — `bd-4feoon8u` is fixed here.** It is the same change as D2; splitting
would be artificial.

**D5 — Branch, not worktree.** `braid/bd-sidebar-contents-dir-shorthand-z7arvhx8`
in the main checkout.

## Phases

- [x] **Phase 1 — Expansion shape (D1, D3).** *Test first.*
      `sidebar_auto.rs`: add the index-driven bare-directory test; select
      `Scope::All` for such a spec in `collect_candidates`. Rewrite Test 21 to
      the new contract and keep the glob tests green as the regression fence.
- [x] **Phase 2 — Parse the shorthand.** *Test first.*
      `sidebar.rs:528`: route a scalar `contents:` that is neither `auto` nor a
      separator to `SidebarEntry::Auto(AutoSpec::Path(s))`. One site covers both
      top-level and nested `contents:`, matching Q1's recursion. Bare strings in
      an *array* are untouched (Q1 parity — they stay `Link`/`Separator`).
- [x] **Phase 3 — Selection ordering (D2, `bd-4feoon8u`).** *Test first.*
      `sidebar_generate.rs`: resolve hrefs + expand every parsed sidebar, then
      `sidebar_for_page`, then enrich + active-state. Per-sidebar diagnostics;
      only the picked sidebar's are emitted.
- [x] **Phase 4 — End-to-end verification.** `cargo run --bin q2 -- render` on
      both the minimal repro and the committed multi-sidebar probe; compare the
      sidebar DOM against the repro's `_site-q1/`. Per CLAUDE.md, record the
      exact invocation and observed output in this plan.
- [x] **Phase 5 — Docs.** Document the shorthand and the `auto: <dir>` section
      shape. Re-read `Q-13-6`'s wording for the case where the user wrote
      `contents: <dir>` and never typed `auto:`.
- [ ] **Phase 6 — Full `cargo xtask verify`** (not `--skip-hub-build`;
      `quarto-core` is WASM-relevant), then request push approval.

## Phase 4 — end-to-end verification (2026-08-13)

Both fixtures rendered through the real binary (`cargo build --bin q2`), output
inspected by hand. Not inferred from test results.

### Minimal repro — `contents: guides`

```
$ q2 render   # in .../repros/sidebar-contents-dir-shorthand/
Rendered 3 of 3 files
```

`_site/guides/first.html` now emits, where it previously emitted a single
`href="guides"` dead link:

```html
<li class="sidebar-item sidebar-item-section">
  <a ... data-bs-target="#quarto-sidebar-section-0" aria-expanded="true">Guides</a>
  ...
  <ul id="quarto-sidebar-section-0" class="collapse list-unstyled sidebar-section depth1 show">
    <li class="sidebar-item"><a href="first.html" class="sidebar-item-text sidebar-link active">
      <span class="menu-text">First Guide</span></a></li>
    <li class="sidebar-item"><a href="second.html" class="sidebar-item-text sidebar-link">
      <span class="menu-text">Second Guide</span></a></li>
  </ul>
</li>
```

Structurally identical to `_site-q1/guides/first.html`: a
`sidebar-item-section` titled **Guides**, expanded (`aria-expanded="true"`,
`sidebar-section depth1 show`), both guides nested, current page `active`.
Remaining cosmetic deltas are pre-existing and unrelated to this fix — Q1 writes
`../guides/first.html` where q2 writes `first.html` (both resolve), and Q1 wraps
the section header text in `<span class="menu-text">`.

### Multi-sidebar probe — `bd-4feoon8u`

```
$ q2 render   # in claude-notes/plans/sidebar-contents-dir-shorthand-investigation/multi-sidebar-auto/
Rendered 5 of 5 files

how-to/index.html      sidebar=1      (was 0)
how-to/one.html        sidebar=1      (was 0)
how-to/two.html        sidebar=1      (was 0)
other/alpha.html       sidebar=1      (unchanged)
index.html             sidebar=0      (unchanged — in neither sidebar)
```

And each page gets the *correct* sidebar, not merely some sidebar:
`how-to/one.html` carries the **How To** sidebar (How To Index / Guide One /
Guide Two); `other/alpha.html` still carries **Other**.

## Known limitation (deliberate, not a regression)

`section_for_dir` builds its children as flat links, so documents nested
*deeper* than one level under the directory (`how-to/deep/x.qmd` under
`auto: how-to`) appear as flat entries rather than a nested sub-section. Q1
recurses arbitrarily (`nodesToEntries`). This matches q2's existing one-level
`group_with_subdirs` behavior and is out of scope here; file a follow-up if the
Connect docs need it.

## Risks / tradeoffs

- **Behavior change to a shipped feature.** Per D1, any project relying on
  `auto: <dir>` producing a flat list gets a titled section instead. This is Q1
  parity, but it *is* a visible change for existing q2 users and should be
  called out in the release notes.
- **Selection ordering is load-bearing and under-tested.** The
  `sidebar_for_page` → `expand_auto` order looks incidental rather than
  designed. Changing it affects every multi-sidebar project, and the
  Rule-2 wildcard currently masks the defect in every single-sidebar project —
  which is why this went unnoticed. The committed probe pins the pre-fix
  behavior; Phase 3 must keep single-sidebar projects working unchanged.
- **`quarto-core` is WASM-relevant**, so full `cargo xtask verify` (not
  `--skip-hub-build`) is required before pushing, per CLAUDE.md.
- **Per-page cost.** D2 expands every declared sidebar on every page instead of
  just the picked one. Fine at Connect-docs scale; worth a look if a project ever
  declares many sidebars over many pages.
- **Machine contention.** Sibling checkouts at `~/rooms/room-N/q2` share this
  disk and CPU; `verify` runs here are slow and have previously died on disk
  space. Budget for it.
- **The repro under-tests the real failure.** Its single sidebar hits the Rule-2
  wildcard, so it cannot show the total-sidebar-loss symptom. Do not treat a
  green repro as proof the Connect-docs case is fixed — that is what the
  `multi-sidebar-auto` probe is for. Per the repro's README, it also does not
  demonstrate the prev/next pagination loss.

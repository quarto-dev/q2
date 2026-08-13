# Sidebar `contents: <directory>` shorthand renders a broken sidebar (bd-sidebar-contents-dir-shorthand-z7arvhx8)

**Date:** 2026-08-12
**Braid:** `bd-sidebar-contents-dir-shorthand-z7arvhx8` (bug, p1, label `navigation`)
**Branch:** investigated in the **main checkout** on `main` @ `152ed8fb`. No worktree
was created — see "Where to do the work" below.
**Status:** Investigation — pending design alignment with user.
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design** — the bug is real, reproducible at HEAD, and precisely
located. But the fix the strand proposes is **necessary and not sufficient**:
it repairs the minimal repro while leaving the real-world Connect-docs failure
in place. Two further decisions are needed before implementation (Q1/Q2 below).

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
strand). See Q4 for whether to fold the fix in here.

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

## Proposed phases (draft)

Contents depend on the answers to Q1/Q2 below.

- **Phase 0 — Pin with failing tests.** Both failures are already reproduced
  (the repro for the shorthand, the committed probe for the selection gap), so
  this phase is about turning them into tests. Write failing tests: (a) parse —
  scalar `contents: guides` produces the chosen `Auto` representation;
  (b) expansion — it yields a titled section matching Q1's shape;
  (c) end-to-end — a multi-sidebar project renders `#quarto-sidebar` on the
  directory's pages. Confirm each fails before touching implementation.
- **Phase 1 — Parse.** Route the scalar shorthand in `parse_contents` (`:528`).
  One site covers both top-level and nested `contents:`, matching Q1's recursion.
- **Phase 2 — Expansion shape.** Make a bare-directory spec expand via
  `section_for_dir` rather than `flatten_as_links`. Shape determined by Q1.
- **Phase 3 — Selection ordering.** Make `sidebar_for_page` see expanded
  contents, per Q2.
- **Phase 4 — End-to-end verification.** `cargo run --bin q2 -- render` on the
  repro *and* the multi-sidebar probe; diff the sidebar DOM against
  `_site-q1/`. Per CLAUDE.md, record the invocation and observed output.
- **Phase 5 — Docs + error catalog.** Document the shorthand. Check whether
  `Q-13-6` ("`auto:` matched no documents") wording still reads correctly when
  the user wrote `contents: <dir>` and never typed `auto:`.

## Open design questions for the user

**Q1. Should `auto: <bare dir>` also produce a titled section, or only the new
`contents: <dir>` shorthand?**

In Q1 these are the same code path and both produce sections. In q2 they would
diverge unless we change `auto:` too.

- **(a) Full Q1 parity** — make a bare-directory `AutoSpec::Path` expand via
  `section_for_dir`; `contents: <dir>` then routes to `AutoSpec::Path` and the
  strand's one-line fix genuinely suffices. Cost: changes documented `auto:`
  behavior and rewrites `auto_path_scopes_to_subdir` (Test 21). *My
  recommendation* — one spelling, one behavior, matches Q1, smallest surface.
- **(b) Keep `auto:` flat; give the shorthand its own representation** (e.g. an
  `AutoSpec::Dir(String)` variant, or a grouping flag). No existing behavior
  changes. Cost: two different meanings for what Q1 treats as one spelling, and
  a lasting parity gap on `auto:`.

Note this only bites for a bare directory. `auto: docs/*` and `auto: docs/**/*.qmd`
are globs, not directories, and stay flat under either option.

**Q2. How should sidebar selection see through `auto:`?**

- **(a) Expand before selecting** — hoist `expand_auto` above `sidebar_for_page`
  and expand each candidate sidebar, then pick. Correct by construction and also
  fixes the pre-existing `auto:`-in-multi-sidebar bug. Cost: expands every
  declared sidebar instead of just the chosen one, per page.
  *My recommendation.*
- **(b) Teach `contains_source_path` to match `Auto` specs directly** against the
  glob without expanding. Cheaper, but duplicates matching logic in a second
  place and risks the two drifting.

**Q3. Should "is this a directory?" be decided from the project index or the
filesystem?** Q1 stats the filesystem. q2's expansion is index-driven and must
work under WASM. I'd suggest **treating it as a directory when any indexed
profile lives beneath it** — no I/O, WASM-safe, and consistent with the fact that
only indexed documents can ever become entries. Confirm that's acceptable, since
it means an empty or unindexed directory behaves as "not a directory" and falls
through to the `Q-13-6` empty-match warning.

**Q4. Should `bd-4feoon8u` be fixed here, or on its own?** The pre-existing
multi-sidebar `auto:` selection bug is now filed separately (it reproduces with
today's syntax, so it deserves its own record either way). But the Connect-docs
case cannot be fixed without it. I'd implement both under this plan and close
`bd-4feoon8u` with the same PR — confirm, or say if you want them split across
branches.

**Q5. Where should the work happen?** This investigation ran in the main
checkout, which is clean at `origin/main`. Per the skill I did not create a
branch or worktree. Given this touches `quarto-core` (WASM-relevant) and wants a
full `cargo xtask verify`, a worktree via
`cargo xtask create-worktree bd-sidebar-contents-dir-shorthand-z7arvhx8` seems
right — but that's your call.

## Risks / tradeoffs (draft)

- **Behavior change to a shipped feature.** Under option Q1(a), any project
  relying on `auto: <dir>` producing a flat list gets a titled section instead.
  This is Q1 parity, but it *is* a visible change for existing q2 users.
- **Selection ordering is load-bearing and under-tested.** The
  `sidebar_for_page` → `expand_auto` order looks incidental rather than
  designed. Changing it affects every multi-sidebar project, and the
  Rule-2 wildcard currently masks the defect in every single-sidebar project —
  which is why this went unnoticed. Phase 0's probe should pin the current
  behavior before it moves.
- **`quarto-core` is WASM-relevant**, so full `cargo xtask verify` (not
  `--skip-hub-build`) is required before pushing, per CLAUDE.md.
- **Machine contention.** Sibling checkouts at `~/rooms/room-N/q2` share this
  disk and CPU; `verify` runs here are slow and have previously died on disk
  space. Budget for it.
- **The repro under-tests the real failure.** Its single sidebar hits the Rule-2
  wildcard, so it cannot show the total-sidebar-loss symptom. Do not treat a
  green repro as proof the Connect-docs case is fixed — that is what the
  `multi-sidebar-auto` probe is for. Per the repro's README, it also does not
  demonstrate the prev/next pagination loss.

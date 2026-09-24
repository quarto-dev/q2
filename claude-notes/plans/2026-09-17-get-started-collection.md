# Get Started collection: seeded example projects for new hub users

**Strand:** not yet filed (will link bd-d147nkqx as parent or related)
**Branch:** `feature/bd-d147nkqx-hub-only-project-templates` (Carlos's PR #684, draft)
**Status:** draft, shaping the example set with Andrew (2026-09-17)
**Related:** `2026-09-15-hub-only-project-templates.md` (mechanism),
`2026-09-15-auto-create-project-set-on-first-run.md` (first-run hook),
`2026-09-01-unified-invite-landing.md` (deferred "seeded first-run samples"),
`2026-02-12-new-file-templates.md` (per-project file templates)

## Overview

The goal is not one more entry in the New project menu. A brand-new user should
land with a **Get Started** collection holding three or four small projects, one
per document type the hub can preview. Each project shows what that format can
do, explains it in its own text, and doubles as a starting point the user can
Duplicate.

PR #684 supplies the mechanism: hub-only `ProjectChoice`s whose scaffolds live
in `quarto-project-create` and never appear in `q2 create`. PR #681 supplies the
moment: `useAutoEstablishRoot` already fires once on a fresh browser to create
the personal root collection. Seeding adds a second collection at that moment
and fills it from the hub-only templates.

## Constraint that shapes the design

The hub has no read-only access and no revocation. A single shared Get Started
collection that every user subscribes to would be edited by the first person who
touched it. So each user gets **their own copies**, created client-side at first
sign-in. Cost: about four projects and fifteen to twenty small documents per
new user on the sync server.

## Decisions so far

- [x] Seed per-user copies at first run, not a shared collection (see above).
- [x] **Every example set includes samples of all four editorial marks**
      (`[++ ]` insertion, `[-- ]` deletion, `[!! ]` highlight, `[>> ]` comment),
      with a plain statement that they are a suggestion notation rendered in
      every output, not tracked changes with accept and reject. Andrew,
      2026-09-17.
- [ ] Automatic seeding at first sign-in (skipped when the user arrives via an
      invite link) versus a one-click "Set up my Get Started collection" button
      on the empty home. Leaning automatic, matching PR #681's silent root.
- [x] **Three examples**, decided by Andrew 2026-09-17: team meeting notes
      (blog layout), website, reveal.js presentation. Content is authored
      first as live hub projects (below), then copied into the crate as
      hub-only templates.

## The three examples (authored live on quarto-hub.com, 2026-09-17)

Each is one hub-only template. The first two are `Website + template` arms
like `blog`; the deck needs a `Default + template` arm, which `get_scaffold`
does not have yet.

1. **Team Meeting Notes** (plain project, no website type; Andrew dropped
   the blog-listing idea on 2026-09-17 in favor of the shape of the real
   quarto-team sync project, copy at `3wmTzUcgMm2rE9E9a9kuRXXGQV1y`).
   Project `45mjZk9VuzyUveyQyXdz4Gk1duLn`. `index.qmd` is a hand-maintained
   home page: two Quarto `::: {.columns}` (Meetings links, Team resources)
   plus "Adding a meeting" steps; `tweaks.scss` holds heading tweaks;
   `team-sync/2026-09-10.qmd` (finished) and `team-sync/2026-09-17.qmd`
   (in progress, all four editorial marks) follow the team's section order:
   date `##`, Attendees, Status as `\[who\] what`, Discussions, Questions
   and feedback, Social; both `format: q2-preview`. File template at
   `_quarto-hub-templates/team-meeting.qmd` ("Team meeting"). The template
   file reports "Project render produced no output for the active page" in
   the hub because `_`-prefixed folders are excluded from the project
   render; harmless, but it will show in the error panel.
   **q2 quirk found while porting:** nested single-child divs collapse into
   the heading's section. `::: foo-columns` > `::: foo-column` > `### H` +
   list rendered as `<section class="level3 foo-column foo-columns">`, so a
   `display: flex` on `.foo-columns` laid the h3 and the list side by side
   (the "link on top of the title" symptom). Quarto's own `.columns` with two
   `.column` children renders correctly (`div.columns` > two
   `section.column`). Note the theme's `div.column` is `inline-block` at 50%
   and the authored `width="50%"` lands as a bare attribute, not a style.
2. **Website Example** (website project type). Project
   `3u7q6fSuo3ukSZ7sxm9zAoiZUnQE`. `index.qmd` (with `format: q2-preview`)
   explains navbar-driven pages and carries the editorial-marks review sample;
   `features.qmd` shows callouts, a tabset, a cross-referenced table, a Mermaid
   diagram, a footnote, highlighted code; `about.qmd` is a team table.
3. **Presentation Template** (`format: revealjs`). Project
   `2iB9obW3zs49rC4uhMRqrFpC6Wf9`. The previous real presentation content was
   removed. `index.qmd` is an eleven-slide deck: headings as slides, cards in
   `.columns`, a `.big` one-idea slide, `.incremental`, two dark divider
   slides via `{background-color="#12324b"}` (custom key-values are emitted
   as `data-*`, so reveal paints the background and adds
   `has-dark-background`), code, an image slide (`quarto-icon.svg`, copied
   from `hub-client/public/`, also used as `logo:` on every slide), a styled
   blockquote, presenting tips as cards, "make it yours".
   **Hub renders decks in React, not from the assembled HTML** (2026-09-18):
   `ts-packages/preview-renderer/src/q2-preview/RevealDeck.tsx` maps
   section Divs onto `@revealjs/react` `<Slide>`s and `sectionAttrProps`
   forwards only `id` + `className`, so slide key-values such as
   `background-color="…"` (emitted as `data-background-color` by the
   writer) never reach the hub's `<section>`. Divider slides therefore
   rendered light in the hub and dark in `q2 render`. Template workaround:
   a `.divider` class plus `.reveal:has(.slides > section.divider.present)
   .backgrounds { background-color }` in `styles.scss`, which works in both
   paths. Candidate strand: forward kvs as `data-*` in `sectionAttrProps`.
   **Mermaid is deliberately absent from the deck**: in `q2 render` for
   revealjs, node labels come out clipped ("Draf", "Previe") because Mermaid
   measures text while reveal has the section hidden; the same block renders
   correctly under `format: html` (verified 2026-09-17 with headless Chrome,
   `--virtual-time-budget=8000`, at both 1280x720 and the native 960x700, so
   it is not the reveal zoom issue). Candidate bug for a strand: run
   `mermaid.run()` after `Reveal.on('ready')` or per `slidechanged`, as
   Quarto 1 does. Mermaid stays on the website example's Features page. `styles.scss` (`theme: [default, styles.scss]`)
   sets three fonts (Fraunces, Inter, JetBrains Mono via Google Fonts), five
   colors, two sizes, and rules for the accent bar under titles, the
   left-aligned title slide, cards, quotes, code shadows. Speaker notes and
   auto-animate are not used because q2's reveal port does not implement them
   yet (checked `crates/quarto-core/src/revealjs/`). A `brand.yml` was tried
   first and dropped so the template has one source of truth for its look.

4. **Article** (added 2026-09-18; Andrew cloned the website project as
   `2p2i1JirrrnRN9dx4bTCJp2shK3C` and asked for a full rewrite). Single
   `index.qmd` + `figure-1.svg`, plain project. Title block with subtitle,
   two authors with affiliations, date, abstract, keywords; `toc: true`,
   `number-sections: true`; sections labelled `#sec-`; a display equation
   `{#eq-copies}` and inline math; a `.theorem` div `{#thm-one}`; a figure
   `{#fig-copies}` and a table `{#tbl-copies}` with `@` references; three
   citations and a reference list; footnote; the editorial-marks review
   paragraph; "Make it yours"; `format: q2-preview`.
   **Topic rewritten 2026-09-18 at Andrew's request** (the earlier
   reading-time piece read as a fake paper with fake data): the running
   example is now Quarto Hub itself — copies in circulation under email
   review, `c = 1 + nr`, versus one shared document; the theorem is CRDT
   convergence. All three references are real (Shapiro et al. 2011 CRDTs;
   Kleppmann et al. 2019 local-first; Knuth 1984 literate programming).
   `figure-1.svg` redrawn to match (grouped bars, email vs hub, legend).
   Hub render check on the live project: clean, zero warnings; Andrew to
   eyeball the preview.
   **No fictional person names anywhere in the templates** (Andrew,
   2026-09-18): article authors are `Author One`/`Author Two`, website
   tables use `Name`, the meeting sample attributes lines to roles
   (`Editor`/`Author`/`Reviewer`). Real bibliography authors (Shapiro,
   Kleppmann, Knuth, …) are fine; "Jun" in `sample-chart.svg` is the
   month, not a person.
   **Citations, what works and what does not (verified 2026-09-18 with the
   local `q2` build and the hub render check):**
   - `[@key]` with no filter renders as the literal `@key` in a
     `span.citation`; nothing is processed and `#refs` stays empty.
   - `filters: [citeproc]` runs citeproc *before* crossref resolution, so
     `@eq-time` is sent to citeproc and the render fails with "Reference
     'eq-time' not found". Candidate strand.
   - `filters: [quarto, citeproc]` (citeproc after the built-in filters)
     works: "(Brysbaert 2019)" in text, crossrefs intact, bibliography
     emitted. Locally the bibliography is appended near the end (after the
     footnotes) rather than into the `::: {#refs}` div. Minor.
   - `bibliography: references.bib` fails: the filter reads CSL-JSON only
     ("expected value at line 1 column 1"). BibTeX is not parsed.
   - `bibliography: references.json` works locally but fails in the hub:
     "Bibliography file 'references.json' not found: operation not supported
     on this platform" (the WASM filter cannot read project files).
   - **Inline `references:` in the front matter (CSL-JSON as YAML, with
     `issued: {date-parts: [[YYYY]]}`) plus `filters: [quarto, citeproc]`**
     passes the hub render check and renders locally. That is what the
     article uses. `issued: {year: 2019}` renders as "n.d."; use date-parts.
   Still to eyeball in the hub: title block with affiliations and abstract,
   equation and section numbering, theorem styling, `#refs` placement.

Note for the crate copy: a `_brand.yml` referenced from front matter trips
the Q-1-20 markdown-metadata warning because of the leading underscore; name
it `brand.yml` if brand is reintroduced.

## Editorial marks sample (draft for the Article and Interactive projects)

Constraints from the parser: marks cannot nest (`[++[-- a]` is error Q-2-19).
Chaining is fine (`[>> one][>> two]`). Emoji work inside comments.

Comment UI (the hover `+` to add a comment, the bubble, and the `✓` Resolve
that removes the span) is only active on documents with `format: q2-preview`,
which is what the placeholder `index.qmd` already uses. Insert, delete and
highlight have no UI; they render with theme colors only.

```markdown
## Suggesting changes together

Quarto Hub documents carry review notes inside the text itself, so everyone
sees them in the preview and in every rendered output.

The launch is planned for [-- next quarter][++ October 14], pending the
[!! final security review], which has not been scheduled.
[>> Can we confirm the date with the platform team before this goes out?]

Four marks, one pattern:

| Mark        | Meaning                        | Renders as            |
|-------------|--------------------------------|-----------------------|
| `[++ text]` | proposed insertion             | green, underlined     |
| `[-- text]` | proposed deletion              | red, struck through   |
| `[!! text]` | highlight, no change proposed  | yellow background     |
| `[>> text]` | comment                        | comment bubble        |

Try it: put the cursor after "scheduled" above and type a comment of your own.
The preview updates as you type, and so does everyone else's.

::: {.callout-note}
These are suggestions, not tracked changes. Accepting an insertion means
removing the brackets by hand; the hub does not yet have accept and reject.
:::
```

## Implementation (strand bd-3fwtdhil, branch `braid/bd-3fwtdhil-seed-examples-collection` off main at d1ba3440, 2026-09-18)

Decisions:

- Collection name: **"Examples / Templates"**. Seeded once, on the
  `needs-setup` → `connected` transition of `useAutoEstablishRoot` (a fresh
  browser creating its root). Not on `needs-migration`, not when
  `enabled: false` (link-project-set boot), not when the boot URL is an
  invite (`share` / `join-collection`), so an invitee lands on the shared
  project alone.
- Which choices seed: a `seed: bool` flag on `ProjectChoice` (builder
  `.seed()`), serialized as `seed` in `get_project_choices`. The client
  seeds `choices.filter(c => c.seed)` in registry order. One registry, no
  TypeScript id list to drift.
- Ids/names: `example-meeting-notes` "Meeting Notes", `example-website`
  "Website", `example-article` "Article", `example-presentation`
  "Presentation"; all `.hub_only().seed()`. They also appear in the New
  project menu, so a user can create a fresh copy of any example later.
- Resources under `resources/templates/examples/<name>/`, all
  `static_text` (no `$title$`; the example content carries its own titles).
  Meeting Notes, Article and Presentation are `Default + template`;
  `get_scaffold`'s `Default` arm gains a template match. Website is
  `Website + template`. The deck's `fork_icon.png` becomes the text
  `fork-icon.svg` (the connector cannot fetch binaries); the same SVG is
  what the other three already use.
- Seeding lives in `hub-client/src/services/seedExamples.ts` as a pure
  function with injected deps (createProject, createNewProject, addProject
  to IDB, createCollection, addProjectToCollection), so it is unit-tested
  without WASM or a sync server. Per-project failures are logged and
  skipped; the collection is still created. `useAutoEstablishRoot` gets an
  optional `onFreshRoot` callback fired once on `needs-setup` → `connected`.

- [x] Phase 1 tests: `choices.rs` (four seed ids are hub-only and `seed`;
      `default`/`website`/`blog` are not seed), `scaffold.rs` path lists
      for the four examples, `projectCreate.wasm.test.ts` (seed flag in
      JSON, files for each seed id), `useAutoEstablishRoot.test.tsx`
      (`onFreshRoot` fires once on needs-setup → connected only),
      `seedExamples.test.ts` (collection created, four projects in order,
      one failure skipped). Red state observed 2026-09-18: crate failed to
      compile on `seed_choices`/`.seed`; the two positive hook tests failed
      (0 calls); `seedExamples.test.ts` failed to import its module.
- [x] Phase 2 Rust: resources, `templates.rs` consts, `scaffold.rs` arms
      (Default gained a template match), `choices.rs` entries + `seed`
      field + `seed_choices()`, WASM JSON `seed`. `cargo nextest run -p
      quarto-project-create`: 50 passed. The pre-existing
      `registry_has_an_implemented_hub_only_choice` now expects 5 and the
      surface-agnostic test checks `_quarto.yml`+`index.qmd` instead of
      `type: website` (three examples are Default projects).
- [x] Phase 3 client: `seedExamples.ts`, hook option `onFreshRoot`, App
      wiring (skips invite boots and ephemeral hubs), `.d.ts` and
      `wasmRenderer.ts` `ProjectChoice.seed?`. Unit tests: 23 passed.
- [ ] Phase 4: `cargo nextest run --workspace`, `npm run build:wasm`,
      `test:wasm`, `test:ci`, `build:all`, `cargo xtask verify`; e2e in a
      fresh browser against a local hub (needs Andrew's browser; the
      sandbox cannot drive a Bash-side hub); changelog two-commit.

## Follow-up design: a tree-shaped New menu driven by a per-choice path (2026-09-23)

Problem (Andrew, screenshot of the New menu on PR #700): the four seeded
examples appear in the flat New menu next to the project types, so the menu
lists "Website" twice and eight entries in one column. Carlos's suggestion:
give each template an array of strings as its hierarchical path and let
that same array turn the menu into a deep tree.

### Two kinds of entry (Andrew's definitions)

- **Template**: a skeleton. Sparse, but with enough structure to get going.
  Today: Default, Website, Blog. Wanted: at least a Presentation skeleton
  (a `format: revealjs` deck with a title slide and two or three empty
  slides). Templates interpolate `$title$`.
- **Example** (working name; alternatives below): a populated document with
  instructive content that *shows* what people do in that format. Today: the
  four seeded examples, plus Carlos's "Welcome to the Quarto-Hub preview"
  tour. Examples carry fixed content; the typed name is not interpolated.

### Mechanism

- `ProjectChoice.path: Vec<String>` (serde default empty = top level),
  builder `.in_path(["Templates"])`. The registry stays the single source
  of truth; ids stay flat and unique, so `q2 create project <id>` and the
  colon form are untouched.
- `get_project_choices` adds `path: string[]`; TS `ProjectChoice.path?`.
- `seed` stays a separate flag. "Is seeded on first run" and "sits under
  Examples" are different questions (an example may exist without being
  seeded, and the welcome tour is Carlos's call).
- **Hub menu** (`ProjectsHome.tsx`): build a tree from `path`, render each
  node as `MenuSubmenu` and each leaf as `MenuItem`. `MenuSubmenu` already
  exists in `Menu.tsx` with APG keyboard behavior (ArrowRight/ArrowLeft,
  focus management) and is used in production for "Move to collection", so
  the menu work is a grouping function plus a recursive render. Depth is
  unlimited by construction; two levels is what we ship.
- **Classic selector** (`ProjectSelector.tsx`, the `<select>`): group with
  `<optgroup label={path.join(' / ')}>`.
- **CLI `--list`**: group by path with indentation; unchanged ids.
- Ordering: registry order within a node; node order = first appearance.

### Proposed registry

```
Templates/
  Default        A minimal Quarto project
  Website        A Quarto website with navigation
  Blog           A blog using the Quarto blog template
  Presentation   A reveal.js deck                      (new skeleton, CLI + hub)
Examples/
  Welcome to the Quarto-Hub preview                     (Carlos's tour, hub-only)
  Meeting Notes  …                                       (hub-only, seed)
  Website        …                                       (hub-only, seed)
  Article        …                                       (hub-only, seed)
  Presentation   …                                       (hub-only, seed)
```

Naming for the second group, to decide: "Examples" (short, matches the
seeded collection "Examples / Templates"), "Worked examples", "Guided
examples", "Show me". Recommendation: **Examples**, and rename the seeded
collection to match whatever is chosen so the menu and the home agree.

### Tests first

- `choices.rs`: `path` defaults empty; templates under `["Templates"]`,
  examples under `["Examples"]`; a helper `choices_tree(surface)` returns
  the grouped structure in registry order; serde round trip.
- WASM test: `path` present in JSON for every choice.
- `ProjectsHome` integration test: two submenus with the expected labels
  and items; the duplicate-name case (two "Website") lands in different
  submenus.
- CLI integration: `--list` output grouped.
- New `presentation` template: scaffold path list; `$title$` substituted
  into `_quarto.yml`/`index.qmd`; `q2 create project presentation d` works.

Not in this PR (#700): keep #700 as the seeding mechanism plus content;
land the tree menu and the Presentation template as a follow-up PR on top,
so each stays reviewable.

### Follow-up implementation (strand bd-q33ylfxf, branch `braid/bd-q33ylfxf-tree-menu` off main at 9e5d519c, 2026-09-24)

Decisions (Andrew, 2026-09-24): the second group is named **"Examples"**;
the Presentation skeleton is offered on the CLI as well as the hub.

- [x] Tests first: `choices.rs` (path per choice, Templates/Examples split,
      `in_path`, serde default, `choices_grouped_by_path`, presentation
      choice), `scaffold.rs` (presentation is two templates), `lib.rs`
      (`$title$` in both files), `create.rs` (JSON `path`, grouped
      `--list`, `q2 create project presentation`), WASM test (`path` on
      every choice; presentation scaffold), `choiceTree.test.ts`, and
      `ProjectsHome.newMenu.integration.test.tsx`. Red observed: crate
      failed to compile on `path`/`in_path`/`choices_grouped_by_path`;
      choiceTree module missing; the three menu tests failed.
- [x] Rust: `ProjectChoice.path` + `in_path`, registry regrouped
      (Templates: default, website, blog, presentation, manuscript, book;
      Examples: welcome tour + four seeded), `ChoiceGroup` +
      `choices_grouped_by_path`, `presentation` skeleton
      (`resources/templates/presentation/*.template`, `Default +
      "presentation"` arm), WASM JSON `path`, CLI `ChoiceListing.path` and
      grouped `--list` output, docs `create.qmd`. Crate 58/58; CLI create
      tests green after updating the interactive-prompt expectation to
      include Presentation.
- [x] Client: `utils/choiceTree.ts` (`buildChoiceTree`), `ProjectsHome`
      renders roots + `MenuSubmenu` per group recursively (label now
      "START FROM"), `ProjectSelector` groups its `<select>` with
      `<optgroup>`, `ProjectChoice.path?` in the two `.d.ts` and
      `wasmRenderer.ts`.
- [x] Verify (2026-09-24): `cargo xtask verify` green (14727 Rust tests
      passed, 200 skipped; hub-client test:ci 106 + 19 + 25 files; all
      preview/MCP suites). Machine prerequisites found on the way: pandoc
      3.11 (verify preflight) and a `typst` binary (quarto-core
      typst_compile tests); both installed via brew.
      CLI e2e, real `target/debug/q2` from an empty scratch dir, output
      inspected: `q2 create --list` prints `project (Project)` then a
      `  Templates` line with `default`, `website`, `blog`,
      `presentation`, `manuscript (not yet implemented)`, `book (not yet
      implemented)` indented beneath; `q2 create project presentation deck
      "Team Update"` writes `_quarto.yml` + `index.qmd` with
      `title: "Team Update"` and `format: revealjs`; `q2 render deck`
      succeeds.
      Hub e2e, local-prod rebuilt from this branch, fresh Playwright
      profile against http://127.0.0.1:8080, DOM inspected: the ＋ New
      menu lists exactly `Templates ▸` and `Examples ▸`; Templates opens
      to Default, Website, Blog, Presentation; Examples opens to Welcome
      to the Quarto-Hub preview, Meeting Notes, Website, Article,
      Presentation; the seeded "Examples / Templates" collection is
      present; no page errors.
- [x] Submenu placement (Andrew, trying the branch locally, 2026-09-24:
      "Submenus always seem to be off the edge of the window"). The ＋ New
      menu is pinned to the header's right edge and `MenuSubmenu` always
      opened to the right, so every submenu left the viewport.
      `MenuSubmenu` now measures itself in a layout effect on open and adds
      `qh-submenu-left` (CSS: `inset-inline-end: calc(100% + 4px)`) when
      the right side overflows and the left fits; if neither fits it stays
      right. Test first: `Menu.submenu.integration.test.tsx` mocks
      `getBoundingClientRect`/`innerWidth` (right when room, flip when
      overflowing, stay right when neither fits); red 2/3, then green.
      Local-prod rebuilt; Playwright at 1000px: Templates submenu has the
      flip class and spans 488..679 of a 1000px window. Commits `e4342b12`
      (code + test) and `43afbbd0` (changelog).
- [x] Menu polish (Andrew, 2026-09-24, three asks after trying it): leaf
      items should tint on hover and as the current item; the Templates
      submenu stayed open when moving to Examples; the two groups need
      subtext explaining the difference, like the per-item descriptions.
      Tests first (`Menu.submenu.integration.test.tsx` siblings block,
      `choiceTree.test.ts` descriptions, `ProjectsHome.newMenu` subtext,
      `choices.rs` every group described, `projectCreate.wasm.test.ts`
      `groups`); red observed on each, then:
      - Registry: `path_description(path)` beside the registry, surfaced
        on `ChoiceGroup.description`; WASM response gains `groups`
        (`[{path, description}]`); runtime `getProjectChoiceGroups()`;
        `buildChoiceTree(choices, groups)` attaches `description` to
        nodes; `MenuSubmenu` gets `subtext`. Copy: Templates "Bare
        skeletons with just enough structure to start writing", Examples
        "Filled-in projects that show what each format can do". The CLI
        `--list` output is unchanged.
      - One open submenu per level: `SubmenuLevelContext` provided by
        `Menu` and by each open submenu; `useSubmenuOpen(id)` derives
        open state from the level, so hovering a sibling closes the open
        one (even with focus inside) and nested groups never close their
        parent. Standalone fallback keeps local state.
      - Tint: `.qh-menu-item:focus` and `.qh-menu-item-inner:focus` share
        the hover tint; `[aria-expanded="true"]` keeps the group tinted.
      - Found by the browser probe: a hover-opened submenu closed when
        the pointer crossed the 4px gap into it (mouseleave on the parent
        with no focus inside). Fixed with a `::before` bridge over the
        gap plus a 150ms close grace period (`SUBMENU_CLOSE_GRACE_MS`).
      - The submenu is now `aria-labelledby` the label span only, so the
        subtext does not join its accessible name; the viewport flip
        toggles the class on the node in the layout effect (no state).
      Verified: quarto-project-create 59 tests, CLI create 42, hub-client
      unit 1243, integration menu files 11, WASM 151 (rebuilt package);
      Playwright on local-prod at 1200px: subtext present, focused leaf
      and parent tinted `rgba(68,112,153,0.08)`, Templates closes when
      Examples is hovered, walking the pointer from Examples into its
      submenu keeps it open with the hovered leaf tinted. Pre-existing
      eslint `react-hooks/refs` and `set-state-in-effect` errors in
      `Menu.tsx`/`ProjectsHome.tsx` are on main too and untouched.
      Commit `d3a87667`. Follow-up ask: group labels bold like the
      leaves (`MenuSubmenu strong`), next commit.

## Work items

### Phase 1: content (can start now, no code)

- [ ] Confirm the example set and names with Andrew.
- [ ] Author each project under
      `crates/quarto-project-create/resources/templates/<type>/<name>/`,
      `.template` suffix only on files that need `$title$`.
- [ ] Editorial-marks section present in at least the Article and Interactive
      projects, with the caveat callout.
- [ ] Render each project with `cargo run --bin q2 -- render <dir>` and
      preview it in the hub to check every page and link.

### Phase 2: scaffold wiring (Rust, tests first)

- [ ] Add a `Default + Some(template)` arm to `get_scaffold` for the
      single-document examples.
- [ ] One `include_str!`/`include_bytes!` const per file in `templates.rs`;
      one `add_file` per file in `scaffold.rs`; registry entries with
      `.hub_only()` in `choices.rs`.
- [ ] Rename or retire `hub-placeholder`; update `HUB_ONLY_CHOICE_ID` in the
      WASM test and the CLI integration tests.
- [ ] `cargo nextest run -p quarto-project-create`, then workspace.

### Phase 3: client seeding (TypeScript, tests first)

- [ ] A `STARTER_CHOICE_IDS` list in hub-client naming the hub-only ids to seed.
- [ ] In the first-run flow after the root exists: `createCollection(server,
      "Get Started")`, then for each id `create_project(id, name)` →
      `createNewProject` → `addProjectToCollection(getStartedId, entry)`.
- [ ] Once-only guard persisted per user; skip when the boot URL is an invite
      or share link.
- [ ] Empty-state copy on the home so the collection's purpose is obvious.
- [ ] Changelog entry (two-commit workflow), `npm run build:all`, `test:ci`.

### Phase 4: end-to-end

- [ ] Fresh browser profile against a local hub: sign in, see Get Started with
      the seeded projects, open each, confirm previews render and marks show
      with color. Record invocations and observations here.

## Status 2026-09-18

Implementation diverged from the sketch above in two ways: the collection is
named "Examples / Templates", and instead of a hub-client `STARTER_CHOICE_IDS`
list the Rust registry flags choices with `seed: true` (single source of
truth; hub-client reads it via `getProjectChoices`). Orchestration lives in
`hub-client/src/services/seedExamples.ts`; the trigger is `onFreshRoot` in
`useAutoEstablishRoot` (fires once per fresh `needs-setup` boot; never for
migrations, returning browsers, or share/join routes).

End-to-end verified in local-prod on a wiped profile: fresh boot lands on the
home with Examples / Templates (4): Presentation, Article, Website, Meeting
Notes. Per-user copies confirmed (fresh doc ids per profile; nothing shared).

Andrew's review pass on the Article example found rendering issues; evidence
gathered by comparing hub preview against native `q2 render` of the same file:

- Filed bd-daa1nw40 (P1): preview table caption lacks "Table N:" prefix and
  leaves crossrefs inside the caption unresolved (native render correct).
- Filed bd-bj5y33jo (P1): preview drops a citation in the paragraph that
  opens with `@thm-one` (native render correct).
- Filed bd-6xyjtnb9 (P2): pipe-table column alignment markers ignored in both
  pipelines.
- Filed bd-jvu6cw57 (P2): `@thm-one` renders as "Section 1: Theorem 1";
  related to bd-5aklrxgi (no number-sections implementation).

Template edits from the review: the article table caption no longer uses a
crossref (works around bd-daa1nw40's visible artifact), and every template's
"Make it yours" section (or meeting-notes' customization tail) now points at
the getting started guide:
https://quarto-dev.github.io/quarto-hub/get-started.html

Screenshots of each rendered template's first page (native `q2 render`,
1200px viewport): `claude-notes/plans/assets/2026-09-17-get-started-collection/`.

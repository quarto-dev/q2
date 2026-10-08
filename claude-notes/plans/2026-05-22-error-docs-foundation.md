# Error-docs foundation

**Status:** drafting — pending user review
**Beads:** [bd-nvlxn](../../.beads/issues.jsonl) (child of [bd-94x8a](2026-05-22-error-docs-website-epic.md))
**Blocks:** bd-8otua (tooling), bd-an6z4 (content)

## Goal

Lay down the convention so content + tooling work has a stable
target. After this lands:

- A page for `Q-1-1` exists at `docs/errors/Q-1-1.qmd`,
  status=complete, rendering correctly under `quarto preview` from
  `docs/`.
- `docs/errors/index.qmd` lists it (just one entry for now), grouped
  under the `yaml` subsystem.
- The Errors section is reachable from the website navbar.
- `docs/errors/README.md` tells a human author "here's how to add a
  new page".

## Directory layout

Subsystem subdirectories under `docs/errors/`:

```
docs/
  errors/
    README.md                ← human authoring guide
    index.qmd                ← top-level listing, grouped by subsystem
    yaml/
      _metadata.yml          ← (optional, future) subsystem-level config
      Q-1-1.qmd
      Q-1-10.qmd
      ...
    markdown/
      Q-2-1.qmd
      ...
    theme/
      Q-14-1.qmd
      Q-14-2.qmd
    ...
```

**Rationale for subdirectories.** Per-subsystem directories leave
room for `_metadata.yml` entries later — e.g. subsystem-wide
listing config, default front-matter, custom page layout — without
restructuring. Quarto's listing feature happily globs across
subdirectories, so the top-level index isn't constrained.

**Knock-on effect on `docs_url`.** The catalog (`crates/quarto-error-reporting/error_catalog.json`)
currently has `"docs_url": "https://quarto.org/docs/errors/Q-X-Y"`
for every entry — flat path. With subsystem subdirectories the
rendered URL is `/docs/errors/<subsystem>/Q-X-Y.html`. Foundation
work includes a one-shot script that rewrites every `docs_url` in
the catalog to match the new layout. Going forward, new catalog
entries get `docs_url` constructed from their `subsystem` field.

(Alternative considered: keep `docs_url` flat and add `aliases:`
on each page to serve at the flat path too. Rejected as more
moving parts for no real win — the catalog is allowed to evolve.)

## Front-matter schema

```yaml
---
title: "YAML Syntax Error"
description: "The YAML document being parsed has a syntax error that prevents parsing."
code: Q-1-1
subsystem: yaml
status: complete       # draft | stub | complete | deprecated
since: "99.9.9"
categories:            # used by the listing page
  - yaml
---
```

Field semantics:

- **`title`** — page title; should match `error_catalog.json[code].title`.
  The audit tool flags drift.
- **`description`** — one-sentence summary used in the listing. May
  differ from `error_catalog.json[code].message_template` (which is
  terminal-output text, often more terse). The two are intentionally
  independent.
- **`code`** — `Q-X-Y`, redundant with the filename but explicit so
  tooling doesn't have to parse the filename and so the page renders
  the code prominently.
- **`subsystem`** — must match `error_catalog.json[code].subsystem`.
- **`status`** — health-tracking field. Definitions:
  - `draft` — file exists, body is auto-generated placeholder; no
    human has authored prose.
  - `stub` — a human has reviewed and lightly fleshed out the page,
    but it's not yet a finished explanation.
  - `complete` — usable as the canonical reference; passes the
    quality bar below.
  - `deprecated` — newer Quarto versions no longer emit this
    code, but the page stays live so users on older versions can
    still look it up. Catalog entries are append-only (see epic
    plan); `deprecated` is for *pages*, not for catalog removal.
- **`since`** — should match `error_catalog.json[code].since_version`.
- **`categories`** — at minimum the subsystem name, so the Quarto
  listing page can group by it.

## Page template

````markdown
---
title: "..."
description: "..."
code: Q-X-Y
subsystem: ...
status: stub
since: "..."
categories: [...]
---

# `Q-X-Y` — {{title}}

> {{description}}

## What this means

Plain-language summary of what went wrong, written for a user who
hit this error and doesn't know Quarto internals.

## Why this happens

Common causes, in rough order of frequency:

- ...
- ...

## How to fix

Specific remediation steps. Where applicable, show the bad input
and the corrected version side by side.

```yaml
# Before
...

# After
...
```

## Example (optional)

A minimal reproducer if it helps clarify the scenario above.

## Related errors (optional)

- [`Q-X-Y'`](Q-X-Y'.qmd) — short note on the relationship.
````

## Quality bar by status

- **stub** (minimum): all required front-matter fields valid; the
  three core sections (What / Why / How) present even if terse.
- **complete**: stub-quality, *and* the "Why" section names at least
  one concrete user scenario, *and* the "How to fix" section gives
  an actionable change the user can make.

## Listing page

`docs/errors/index.qmd` uses Quarto's
[listing](https://quarto.org/docs/websites/website-listings.html)
feature, globbing across subsystem subdirectories:

```yaml
---
title: "Error reference"
listing:
  contents: "*/Q-*.qmd"
  type: table
  fields: [code, title, subsystem, status]
  categories: numbered
  sort: code
---
```

Open question: a `table` view with a `subsystem` column may read
better than category grouping. Decide once we have a handful of
pages to look at.

Future enhancement once subsystems have their own subdirectories:
add an `index.qmd` per subsystem (`docs/errors/<subsystem>/index.qmd`)
showing just that subsystem's pages. Not in v1.

## Navbar + sidebar wiring

`docs/_quarto.yml` gains:

```yaml
website:
  navbar:
    left:
      - href: index.qmd
      - about.qmd
      - guide/index.qmd
      - href: errors/index.qmd        # ← new
        text: Errors
  sidebar:
    - id: errors                       # ← new
      title: "Error reference"
      contents:
        - errors/index.qmd
        - section: "yaml"
          contents:
            - errors/yaml/Q-1-1.qmd
        # ... populated by hand as pages are added; the listing
        # page is the canonical browse, the sidebar is a
        # supplementary navigation
```

Open question: do we want the sidebar to be hand-maintained or
generated? Hand-maintained for v1 is fine — the listing page is the
real browsing surface. If sidebar maintenance gets annoying we can
add a sidebar-generator to the xtask later.

## Worked-example page

`Q-1-1` (YAML Syntax Error) is a good seed because it's universally
encountered and the explanation is small. The foundation issue ships
this one page at `status: complete`, located at
`docs/errors/yaml/Q-1-1.qmd`, so the template isn't just hypothetical.

## Catalog `docs_url` rewrite

One-shot rewrite of `crates/quarto-error-reporting/error_catalog.json`
so every entry's `docs_url` matches the new subdirectory layout:

```
https://quarto.org/docs/errors/Q-X-Y
→ https://quarto.org/docs/errors/<subsystem>/Q-X-Y
```

Implemented as a small `jq` invocation or a one-off Rust helper;
the change is mechanical and reviewable in one commit.

A regression-time check belongs in the tooling work (bd-8otua) —
the `audit` subcommand should flag any catalog entry whose
`docs_url` doesn't match its `subsystem` + `code`.

## Work items

- [ ] Read existing docs/ rendering setup to confirm which binary
      builds it (Q1 external vs in-repo `q2 render`), so end-to-end
      verification targets the right tool
- [ ] Create topic branch `beads/bd-nvlxn-error-docs-foundation`
- [ ] Write `docs/errors/README.md` — schema reference + authoring
      conventions (prose pass via `/reader-expectations-prose`)
- [ ] Update `docs/_quarto.yml` — add Errors navbar entry + sidebar
- [ ] Create `docs/errors/index.qmd` — top-level listing page
- [ ] Create `docs/errors/yaml/Q-1-1.qmd` — seed page at
      `status: complete` (prose pass via `/reader-expectations-prose`)
- [ ] Rewrite `error_catalog.json` `docs_url` fields to include
      subsystem segment; commit in its own step so the diff is
      reviewable
- [ ] Verify: `quarto preview` (or `q2 preview`) renders the docs
      site, listing picks up the seed page, no warnings
- [ ] Verify: every `docs_url` in the catalog matches the
      `<subsystem>/<code>` pattern (one-liner check)
- [ ] Verify: full workspace builds — `cargo build --workspace`
- [ ] Verify: full workspace tests — `cargo nextest run --workspace`
      (the catalog-crate tests in particular)
- [ ] Verify: `cargo xtask verify --skip-hub-build` clean
- [ ] Commit on topic branch
- [ ] Request user approval to merge `--no-ff` into main

## Prose-quality note

Pages authored as part of this epic, and the README, **must** be
revised through the `/reader-expectations-prose` skill (Gopen &
Swan's reader-expectations methodology) before being marked
`status: complete`. The skill applies to any prose-heavy artifact
in this work: the README, the seed page, and every future content
page when its author promotes it from `stub` to `complete`.

`stub`-status pages may skip the prose pass, but the prose pass is
required for `complete`.

## Test strategy (TDD)

The foundation work is mostly Quarto config + content, so "tests"
here are end-to-end render checks, not Rust tests. Before declaring
done:

1. **Render check:** `cd docs && quarto preview` (or `quarto
   render`) builds without errors. The Errors navbar entry resolves.
   The `Q-1-1` page renders. The listing on `docs/errors/index.qmd`
   shows the seed entry.

2. **Front-matter shape check:** even though the audit tool (bd-8otua)
   doesn't exist yet, write the seed page using exactly the schema
   above so tooling has something real to validate against on day 1.

3. **Document inspection step in the PR/commit:** per CLAUDE.md
   end-to-end verification policy, include in the commit message the
   actual `quarto preview` invocation used and a snippet of the
   rendered listing.

## Out of scope

- Tooling. That's bd-8otua. The seed page is hand-written.
- Authoring more than the seed page. That's bd-an6z4.
- Auto-generating the sidebar from front-matter. Possible follow-up.
- Cross-linking from in-binary error output to anchors within these
  pages. The ariadne footer already renders the page URL; deep
  anchors aren't needed for v1.

## Decisions (resolved 2026-05-22)

1. **Page format** = plain markdown body inside qmd. Shortcode-driven
   page generation (e.g. `{{< error-page Q-1-1 >}}` pulling catalog
   data at render time) is over-engineered for v1. A shortcode for
   *linking to* error pages from other content (e.g. `{{< error
   Q-1-1 >}}` → a styled link) is interesting future work but
   explicitly out of scope here.
2. **Listing view** — defer to when content has landed; ship with
   the table-grouped-by-category form drafted above and iterate.
3. **Sidebar generation** = hand-maintained for v1. Revisit if it
   becomes annoying.
4. **`docs_url` rewrite scope** = update every entry's `docs_url`
   in `error_catalog.json` to include the subsystem segment as part
   of the foundation commit. Not using `aliases:` — the catalog is
   allowed to evolve.

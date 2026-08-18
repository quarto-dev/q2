# Error reference pages

This directory holds one page per error code emitted by Quarto.
Each page expands a terminal-format error message into a fuller
explanation: what the error means in plain language, why it
typically fires, and how a user can fix it. The codes themselves
live in the catalog at
[`crates/quarto-error-catalog/error_catalog.json`](../../crates/quarto-error-catalog/error_catalog.json),
which defines which codes exist; the pages in this directory
explain them.

## Directory layout

Pages are organized by subsystem:

```
docs/errors/
├── README.md          ← this file
├── index.qmd          ← top-level listing (all codes, grouped)
├── yaml/
│   ├── Q-1-1.qmd
│   ├── Q-1-10.qmd
│   └── ...
├── markdown/
│   ├── Q-2-1.qmd
│   └── ...
└── ...
```

Every page is named `<code>.qmd` and sits in a directory named for
its subsystem. The rendered URL is
`/docs/errors/<subsystem>/<code>.html`, and this URL must match the
`docs_url` field of the corresponding catalog entry.

The per-subsystem directories give Quarto a natural place to attach
subsystem-wide settings later through `_metadata.yml`, without
restructuring.

## Front-matter schema

Each page begins with YAML front matter:

```yaml
---
title: "YAML Syntax Error"
description: "The YAML document being parsed has a syntax error that prevents parsing."
code: Q-1-1
subsystem: yaml
status: complete
since: "99.9.9"
categories:
  - yaml
---
```

| Field         | Type                  | Required | Purpose                                                                 |
| ------------- | --------------------- | -------- | ----------------------------------------------------------------------- |
| `title`       | string                | yes      | Page title. Must match the catalog's `title` for this code.             |
| `description` | string                | yes      | One-sentence summary used in the listing page.                          |
| `code`        | string (`Q-X-Y`)      | yes      | Error code. Must match the filename and the catalog key.                |
| `subsystem`   | string                | yes      | Must match the catalog's `subsystem` and the parent directory name.    |
| `status`      | enum (see below)      | yes      | Authoring health. Tooling reports rollups by status.                    |
| `since`       | string (semver)       | yes      | Must match the catalog's `since_version`.                               |
| `categories`  | list of strings       | yes      | At minimum `[<subsystem>]`. Drives grouping in the listing page.        |

### `description` is not `message_template`

The catalog field `message_template` is the text Quarto prints to
the terminal when the error fires. The page field `description` is
the one-sentence summary shown next to the code in the listing.
They serve different audiences and can legitimately differ — the
audit tool does not flag drift between them.

A `message_template` is often terse so it fits a terminal line;
a `description` has more room and can be plainer.

## Status enum

The `status` field tracks authoring health. The audit tool
(`cargo xtask error-docs`) rolls up coverage by status.

| Status       | Meaning                                                                              |
| ------------ | ------------------------------------------------------------------------------------ |
| `draft`      | Page exists with auto-generated placeholder body. No human has written prose yet.   |
| `stub`       | A human has reviewed and lightly filled out the page. Not yet a finished reference. |
| `complete`   | Usable as the canonical reference. Has met the prose-quality bar described below.   |
| `deprecated` | Newer Quarto versions no longer emit this code, but the page stays live so users on older versions can still look it up. |

Error codes themselves are append-only in the catalog: once a
code has been emitted by any released Quarto version, it stays.
The `deprecated` status applies to *pages*, not to the catalog.
When a newer Quarto version stops emitting a code, the page
becomes `deprecated` so readers know it's no longer current, but
the catalog entry stays in place.

## Page template

Pages follow this body structure:

```markdown
# `Q-X-Y` — {{title}}

> {{description}}

## What this means

Plain-language explanation, written for someone who hit this error
and does not know Quarto internals.

## Why this happens

Common causes, ordered roughly by frequency.

## How to fix

Specific remediation steps. Where it helps, show the bad input
and the corrected version side by side.

## Example (optional)

A minimal reproducer.

## Related errors (optional)

Cross-references to related codes.
```

The three required sections — **What this means**, **Why this
happens**, and **How to fix** — match the order a reader needs:
first they confirm they're on the right page, then they understand
the cause, then they apply the fix.

## Adding a new page

**A page is not optional.** Adding a code to `error_catalog.json`
without adding its page is a lint failure — see [Enforcement](#enforcement)
below. Do both in the same commit.

1. Find the catalog entry in `error_catalog.json`.
2. Create `docs/errors/<subsystem>/Q-X-Y.qmd`, copying the template
   above.
3. Fill the front-matter fields from the catalog entry. Start at
   `status: draft` until you write real prose.
4. Add the page to the errors sidebar in `docs/_quarto.yml` — one
   `- errors/<subsystem>/Q-X-Y.qmd` line under the
   `- section: "<subsystem>"` block, creating the section if this is
   the subsystem's first page. This is also enforced; see
   [Enforcement](#enforcement).

The `cargo xtask error-docs new Q-X-Y` generator described in
[the tooling plan](../../claude-notes/plans/2026-05-22-error-docs-tooling.md)
(bd-8otua) has not shipped; until it does, this is a by-hand step.

## Enforcement

`cargo xtask lint` runs two repo-level rules over this directory.

`error-docs-page-missing` reconciles the catalog against this
directory and fails on two problems:

- **A code with no page.** The catalog declares `Q-X-Y`; no file
  exists at `docs/errors/<subsystem>/Q-X-Y.qmd`. Diagnostics carrying
  the code link to a page that 404s.
- **`docs_url` drift.** The entry's `docs_url` is not
  `https://quarto.org/docs/errors/<subsystem>/<code>`, so the link the
  user clicks does not reach the page even when the page exists.

`error-docs-sidebar-unlisted` reconciles this directory against the
errors sidebar in `docs/_quarto.yml` and fails on three more:

- **An unlisted page.** The page exists but no sidebar entry
  references it. It still renders and still resolves by direct URL —
  so no diagnostic ships a 404 — but a reader browsing the error
  reference cannot find it.
- **A stale entry.** The sidebar references a page that does not
  exist, so the rendered sidebar carries a dead link.
- **An out-of-order entry.** Entries within a `- section:` block must
  ascend by code number, so `Q-1-2` comes before `Q-1-10`. Without
  this, appending entries alphabetically drifts the sidebar into
  lexicographic order.

The sidebar list is hand-maintained (a v1 decision recorded in
`claude-notes/plans/2026-05-22-error-docs-foundation.md`), and before
this rule existed it had drifted to 153 of 211 pages, with two whole
subsystems missing their `- section:` block.

**Section order is not policed.** The sections sit in an arbitrary
historical order and stay that way; only entries *within* a section
are sequenced. Note that a numerically ordered section can still look
lexicographic where codes are sparse — `yaml` runs
`Q-1-1, Q-1-10, … Q-1-29, Q-1-99` because it has no `Q-1-2` through
`Q-1-9`.

Sorting the [listing](index.qmd) by code number is a separate,
unsolved problem (bd-otmqu). It is not the same as sidebar order: the
listing's sort key has to come from front matter, and neither Quarto 2
nor Quarto 1 has a numeric-aware build-time comparator.

The check runs as step 1 of `cargo xtask verify` and in CI, so it
fires before you open a PR. It reports violations against the catalog
entry's own line, because that is where the declaration promising the
page lives.

Every code needs a page, with no opt-out for "internal" codes: the
page is the first landing spot for anyone who hits the code, and the
page itself is where the internal/user-facing distinction gets
explained.

The rule lives at `crates/xtask/src/lint/error_docs.rs`. It
deliberately does *not* check orphan pages, misplaced pages, or
front-matter drift — those belong to the richer audit in bd-8otua.
In particular, **a page's `title` is free to differ from the catalog's
`title`** (sentence case usually reads better as a page heading), the
same way `description` is free to differ from `message_template`.

## Promoting through the status enum

Once the page exists at `draft`:

- **`draft` → `stub`** when a human has read the auto-generated
  body and replaced each placeholder with at least a one-sentence
  human draft. Stub-quality is the minimum needed for the audit
  tool to mark a subsystem "covered".
- **`stub` → `complete`** when all three required sections carry
  substantive content, the "How to fix" section gives a concrete
  action a user can take, and the prose has been revised through
  the [reader-expectations-prose](../../.claude/skills/reader-expectations-prose/SKILL.md)
  methodology (run `/reader-expectations-prose` on the file).
- **anything → `deprecated`** when newer Quarto versions stop
  emitting the code. The page stays live and the catalog entry
  stays in place; only the front-matter status changes.

## Verifying your work

Before committing a new page:

```
cargo run --bin q2 -- render docs/
```

The `docs/` website is rendered by **Quarto 2** (built from this
repo), not by the system `quarto` binary (which is Quarto 1). Using
Q1 to verify will produce misleading results because the two
versions accept different YAML schemas.

The Errors entry in the navbar should resolve. Your new page
should appear in the listing at `docs/errors/index.html`, and the
page itself should render without warnings (other than `Q-13-4`
for cross-references whose target pages do not yet exist; see
the cross-reference convention below).

Also run the lint, which confirms the page sits at the path the
catalog's `docs_url` points to:

```
cargo xtask lint
```

See [Enforcement](#enforcement) for what it checks. The broader audit
(front-matter drift, orphan and misplaced pages, coverage rollups by
status) is still bd-8otua's `cargo xtask error-docs`, which has not
shipped.

## Cross-reference convention

The page template's *Related* section uses code spans
(`` `Q-X-Y` ``) rather than links (`` [`Q-X-Y`](Q-X-Y.qmd) ``) when
the target page does not yet exist. Q2 emits a `Q-13-4` warning
for any link whose target is not in the project index, which would
pollute the audit signal. When the target page lands, promote the
cross-reference from a code span to a real link in the same commit
that adds the target.

Since the coverage lint landed, **every catalog code has a page**, so
a cross-reference to any existing code can be a real link. Same-subsystem
targets are relative (`` [`Q-2-7`](Q-2-7.qmd) ``); cross-subsystem
targets go through the parent (`` [`Q-16-1`](../extension/Q-16-1.qmd) ``).
The code-span form is now only for a code being added in a later commit
of the same series.

The audit tool (`cargo xtask error-docs`, bd-8otua) will eventually
flag eligible-but-not-linked cross-references so this promotion
doesn't get forgotten.

## Related

- [`crates/quarto-error-catalog/`](../../crates/quarto-error-catalog/)
  — the crate holding `error_catalog.json` and the `CatalogProvider`
  that installs it. Adding a code means editing the catalog here; the
  lint then requires a page in this directory.
- [`posit-dev/quarto-error-reporting`](https://github.com/posit-dev/quarto-error-reporting)
  — the error-reporting crate itself, with the API used to emit these
  errors from Quarto code. It was externalized out of `crates/` and is
  now consumed as a published dependency; the `Q-*` data stayed behind
  in `quarto-error-catalog`.

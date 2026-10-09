---
title: 'Error-code documentation pages in the website (epic)'
date: 2026-05-22
description: 'Plans a documentation page on the website for each of the 133 structured error codes, covering the page convention, a `cargo xtask error-docs` coverage tool, and hand-written content.'
status: draft  # drafting — pending user review
braid:
  strand: bd-94x8a  # parent epic
  priority: P2
  labels: [documentation, error-reporting, website]
---

**Children:**
- [Foundation](2026-05-22-error-docs-foundation.md) — bd-nvlxn
- [Tooling](2026-05-22-error-docs-tooling.md) — bd-8otua
- [Content](2026-05-22-error-docs-content.md) — bd-an6z4 (umbrella)

## Motivation

`crates/quarto-error-reporting/error_catalog.json` lists **133 structured
error codes** across 12 subsystems. Every entry already has a
`docs_url` pointing at `https://quarto.org/docs/errors/Q-X-Y`, but
none of those pages exist anywhere — they're a promise the catalog
makes that nothing currently fulfills.

The user-visible payoff of structured codes (Google a stable
`Q-2-301` instead of error-message text, get a real explanation with
context) only lands once those pages exist. The Q2 website under
`docs/` is the natural place to host them.

Snapshot of where errors live today:

| Subsystem  | Codes |
| ---------- | ----- |
| markdown   | 36    |
| yaml       | 21    |
| writer     | 21    |
| xml        | 16    |
| listing    | 16    |
| template   |  7    |
| navigation |  7    |
| project    |  3    |
| theme      |  2    |
| internal   |  2    |
| lua        |  1    |
| cli        |  1    |
| **total**  | **133** |

## Goal

Two outcomes, plus a long-running content pipeline:

1. **Mechanism.** A documented convention for what an error page
   looks like — directory location, front-matter schema, body
   template — plus the navbar/sidebar/listing plumbing in
   `docs/_quarto.yml` to render it. One worked example page anchors
   the convention.

2. **Visibility.** A `cargo xtask error-docs` subcommand that
   answers, on demand, "which catalog entries are missing pages,
   which pages are stale, which are stubs vs complete, and what's
   the current coverage by subsystem". Front-matter is the source of
   truth for status; the tool reads it.

3. **Content.** Pages for the 133 current catalog entries, authored
   by hand because each error's "why this happens" / "how to fix"
   genuinely benefits from human framing. Tooling generates *stubs*,
   not finished pages.

## Catalog is append-only

A property of `error_catalog.json` worth recording up front: **error
codes are append-only**. Once a code has been emitted by any released
version of Quarto, it stays in the catalog forever — even if newer
versions stop emitting it — so that users running old versions can
still look the code up.

(Eventually the catalog will grow `since_version` / `until_version`
fields to express true deprecation. Today there's only
`since_version` and no removal semantics.)

Two consequences for this work:

- **"Stale" pages are defensive, not common.** A page existing for
  a code not in the catalog should essentially never happen via
  intentional removal. The audit tool still reports the case
  because typos and renames can create the situation, but the
  routine fix is to *re-add* the catalog entry, not delete the
  page.
- **`status: deprecated`** means "the code is still in the catalog
  but newer Quarto versions no longer emit it." The page stays
  live indefinitely — back-links and SEO matter, and users on old
  versions still need it.

No tooling in this epic ever deletes a catalog entry.

## Non-goal: full auto-generation

We explicitly do **not** want a one-shot pipeline that turns
`error_catalog.json` into rendered pages. The catalog gives us
title + template message + subsystem — enough for a placeholder, but
not enough for a useful page. Useful pages need:

- A plain-language summary that the in-binary message can't always
  afford (terse, must fit in a terminal).
- A description of *why* the error fires, ideally rooted in a
  concrete user-visible scenario.
- Specific remediation steps, often format- or engine-dependent.
- Cross-references to related codes or to docs sections.

Generation can provide the skeleton; humans fill the body. The audit
tool exists to keep us honest about the gap.

## Children

Three pieces of work, sequenced foundation → (tooling, content).

### 1. Foundation — bd-nvlxn

Lay down the directory, schema, template, and rendering integration.
Outputs:

- `docs/errors/` directory with one worked-example page at
  `status: complete`.
- `docs/errors/index.qmd` rendering a Quarto listing grouped by
  subsystem.
- Updates to `docs/_quarto.yml` (navbar entry + sidebar).
- `docs/errors/README.md` documenting the conventions for human
  authors.

Detail plan: [2026-05-22-error-docs-foundation.md](2026-05-22-error-docs-foundation.md).

### 2. Tooling — bd-8otua

`cargo xtask error-docs <subcommand>`:

- `audit` (default, CI-friendly): catalog ↔ docs diff.
- `health`: coverage / status percentages.
- `new <Q-X-Y>`: write a stub qmd from the catalog entry.

Depends on foundation (schema + directory layout must exist).
Detail plan: [2026-05-22-error-docs-tooling.md](2026-05-22-error-docs-tooling.md).

### 3. Content — bd-an6z4 (umbrella)

One sub-issue per subsystem, opened on demand. Closing a sub-issue
requires every page in that subsystem at `status: stub` or better.
Promotion to `status: complete` happens later as we get user
feedback or run into the errors ourselves.

Detail plan: [2026-05-22-error-docs-content.md](2026-05-22-error-docs-content.md).

## Sequencing

```
[Foundation: bd-nvlxn] --blocks--> [Tooling: bd-8otua]
                       \--blocks--> [Content:  bd-an6z4]
```

Foundation must land first because both downstream pieces consume
its schema. Tooling and content can proceed in parallel once
foundation is in place — content authors will benefit from `new
<code>` but don't strictly need it; they can copy the template.

## Out of scope

- Updating the `docs_url` field in `error_catalog.json` to a Q2
  URL. The current `https://quarto.org/docs/errors/Q-X-Y` URLs may
  eventually be served by the Q2 site at the same path, or by a
  redirect; either way that's a deployment concern, not part of
  this epic. (Note: the foundation work *does* rewrite `docs_url`
  to include the new subsystem segment — that's a layout
  alignment, not a deployment change.)
- Linking *from* in-binary error output to specific anchors in
  these pages (we already render `docs_url` in the ariadne footer
  via the catalog).
- A Quarto shortcode for **linking to** error pages from other
  content (e.g. `{{{< error Q-1-1 >}}}` → styled cross-reference).
  Interesting future work — would give guides and how-tos a clean
  way to cite specific error codes — but not part of this epic.
- Internationalization. English-only.
- Reverse mapping (a "if you see this on the website, here's the
  source code that emits it" lookup). Possible follow-up if the
  audit tool naturally grows toward it.

## Decisions (resolved 2026-05-22)

1. **Front-matter status values** = `draft | stub | complete |
   deprecated`. Confirmed.
2. **`description` vs `message_template`** are intentionally
   independent. The audit tool does **not** flag drift between
   them. They serve different audiences (page listing vs terminal
   output) and can legitimately diverge.
3. **Listing organization** = grouped by subsystem, with
   **subsystem subdirectories** on disk (`docs/errors/<subsystem>/Q-X-Y.qmd`).
   This leaves room for per-directory `_metadata.yml` entries
   later (e.g. subsystem-wide listing config, default front-matter,
   page layout overrides) without restructuring.

   Knock-on effect: the catalog's `docs_url` field currently
   points at `https://quarto.org/docs/errors/Q-X-Y` (no subsystem
   in path). With subsystem subdirectories the rendered URL
   becomes `/docs/errors/<subsystem>/Q-X-Y.html`. The foundation
   work updates `error_catalog.json` so `docs_url` includes the
   subsystem segment.
4. **Stub-page status default** = `draft` when `xtask error-docs
   new` writes the skeleton; promoted to `stub` once a human has
   reviewed and lightly edited it. The audit tool reports both,
   distinctly.

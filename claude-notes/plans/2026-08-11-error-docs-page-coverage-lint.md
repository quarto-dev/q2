# Lint: every `error_catalog.json` code needs a `docs/errors/<subsystem>/<code>.qmd` page (bd-u2qj4y29)

**Date:** 2026-08-11
**Braid:** bd-u2qj4y29 (task, p2, filed 2026-08-10 by Carlos)
**Checkout:** main checkout of q2, branch `main` @ `d05e021e`
**Status:** Design settled 2026-08-11 with Carlos; implementing. See
**Design decisions** below — they supersede the *Open design questions*
section, which is kept as the record of what was asked.

## Triage verdict

**Ready to design, but the strand as written understates the problem and
duplicates an existing planned tool** — the drift is 28 missing pages, not
two, and `cargo xtask error-docs audit` (bd-8otua, plan
`2026-05-22-error-docs-tooling.md`) already specifies exactly this check plus
four more. The design decision is therefore not "how do I write this lint
rule" but "does this become a fifth `cargo xtask lint` rule, or does it
finally land bd-8otua's `audit` and wire *that* into the gate."

## Issue context

The strand's premise: catalog entries carry a `docs_url` of the form
`https://quarto.org/docs/errors/<subsystem>/<code>`, the site serves those
from `docs/errors/<subsystem>/<code>.qmd`, and nothing checks the page
exists — so a diagnostic can ship pointing at a 404. It cites two incidents
caught by eye: `Q-2-42` (still missing) and the diagram cell-option codes
shipped page-less in PR #499 and fixed in a follow-up.

Filed 2026-08-10 while shipping bd-mermaid-cell-options-9wo3crl0. Suggested
shape: a `cargo xtask lint` rule under `crates/xtask/src/lint/`. Two
questions left open in the description itself: whether every code truly needs
a page, and whether the rule errors or warns.

Note the strand names the diagram codes as `Q-2-47`/`Q-2-48`; the codes that
actually landed are `Q-2-42`/`Q-2-43` per bd-mermaid's close comment, and
`Q-2-43`/`Q-2-44` are the pages that exist. The numbering in the description
is a misremembering, not a live signal — but it is itself evidence that
codes and pages are tracked by hand.

## Dependency graph

`braid dep tree bd-u2qj4y29` and `dep list` are both **empty** — the strand
has no edges at all, despite the description naming
bd-mermaid-cell-options-9wo3crl0 as its origin. The `discovered-from` link
was never created. (Filing that edge is cheap and worth doing; see below.)

The graph that *matters* is the one the strand isn't attached to:

- **bd-94x8a** — "Error-code documentation pages in the website (epic)", open.
- **bd-nvlxn** (child, **closed**) — foundation: directory layout,
  front-matter schema, page template, `index.qmd` listing, README. This is
  what defined `docs/errors/<subsystem>/<code>.qmd` in the first place.
- **bd-an6z4** (child, **closed**) — content authoring umbrella for the
  then-133 catalog entries, with closed per-subsystem children for `yaml`
  (bd-bj5yp) and `markdown` (bd-lgxdr).
- **bd-8otua** (child, **open**) — "Error-docs tooling: `cargo xtask
  error-docs` (audit / health / new)". Blocked by bd-nvlxn, which is now
  closed, so **bd-8otua is unblocked and ready**. Its plan is
  `claude-notes/plans/2026-05-22-error-docs-tooling.md`.

bd-8otua's `audit` subcommand is specified to report exactly five problem
classes, the first of which is bd-u2qj4y29 verbatim:

> **Missing.** Catalog has the code; no page exists at the expected path
> `docs/errors/<subsystem>/<code>.qmd`.

plus **Stale** (orphan page), **Mismatch** (front-matter drift), **Misplaced**
(wrong subdirectory), and **`docs_url` drift**. That plan even pre-answers the
strand's second open question — "Default `--fail-on` is `none` initially…
once content lands we can flip the default to `missing`. **Decision
deferred** to when bd-an6z4 is mostly complete." bd-an6z4 is now closed. The
condition the deferral was waiting on has been met.

`docs/errors/README.md` references the unbuilt tool three times ("once
`cargo xtask error-docs` ships", "Once the audit tool ships, also run…"), so
the documentation already promises it.

## What the code looks like today

Verified at `main` @ `d05e021e`.

**Everything the strand describes still holds structurally.** The catalog is
`crates/quarto-error-catalog/error_catalog.json` — a JSON *object* keyed by
code (not an array), each value carrying `subsystem`, `title`,
`message_template`, `docs_url`, `since_version`. Pages live at
`docs/errors/<subsystem>/<code>.qmd`. No tooling reconciles them.

**The drift is an order of magnitude larger than the strand says.** A probe
(committed at `claude-notes/plans/error-docs-page-coverage-lint-investigation/audit-probe.py`,
output alongside it) reports:

```
catalog codes: 193   pages: 165
missing:   28
orphan:     0
misplaced:  0
drift:      2   (title case only, see below)
url-drift:  2
```

The 28 missing pages, by subsystem:

| Subsystem   | Missing | Codes |
| ----------- | ------: | ----- |
| `project`   |      11 | Q-5-8 … Q-5-12, Q-5-17 … Q-5-22 |
| `extension` |       9 | Q-16-1 … Q-16-9 |
| `lua`       |       4 | Q-11-2 … Q-11-5 |
| `writer`    |       2 | Q-3-42, Q-3-43 |
| `theme`     |       1 | Q-14-3 |
| `markdown`  |       1 | Q-2-42 |

`docs/errors/extension/` does not exist at all — the whole subsystem is
undocumented.

**This is chronic drift, not a fresh regression.** `git log -S` on the
catalog dates each cluster to the PR that introduced the codes:

| Codes    | Introduced by |
| -------- | ------------- |
| Q-16-*   | `6ff4221e` Shortcode extensions: Q1 compatibility port (#450) |
| Q-5-8+   | `4f3d073b` project pre-render / post-render scripts (#448) |
| Q-11-2+  | `8e05c03e` Lua API Pandoc parity conformance harnesses (#393) |
| Q-14-3   | `fc422255` theme `{light, dark}` map form (bd-o76p01wb) |
| Q-3-42/43| `896b017c` quarto-source-map / pampa Plans 7f+7g |
| Q-2-42   | `73f92e0e` project profiles Phase 4 — conditional content |

Every one of those is a feature PR that added codes and did not add pages.
The habit, not the two named incidents, is the actual defect.

**Two second-order findings the probe surfaced, both in scope for a
catalog↔docs check:**

1. **`docs_url` drift.** `Q-3-42` and `Q-3-43` carry
   `https://quarto.org/docs/errors/Q-3-42` — no `<subsystem>` segment. So
   even once those pages exist, the diagnostic still points at a 404. A
   check that only tests file existence would pass these two while the
   user-visible link stays broken.
2. **Front-matter title drift is negligible** — 2 of 165 pages
   (`Q-2-43`, `Q-2-44`) differ from the catalog only in capitalization
   ("Callout title given twice" vs "Callout Title Given Twice"). Possibly
   deliberate (sentence case reads better as a page title). Worth a decision,
   not worth blocking on. `subsystem`, `since`, and `code` front-matter
   match the catalog everywhere; nothing is misplaced; there are no orphans.

**Page-status rollup:** 164 `stub`, 1 `complete`. So "a page exists" today
means "a stub exists" — which is consistent with the strand's own guess that
`status: stub` implies a page can be minimal but should exist.

**Where the gate would fire.** `cargo xtask lint` runs in CI
(`.github/workflows/test-suite.yml:155`) and as step 1 of `cargo xtask
verify` (`crates/xtask/src/verify.rs:73-83`). So a lint rule inherits both
call sites for free — that is the real appeal of the strand's suggested
shape.

**But the lint module's shape does not fit this check.** `lint/mod.rs`
walks `crates/**/*.rs` and calls `check_file(path, content) -> Vec<Violation>`
per Rust file; every existing rule (`external_sources`, `add_file_with_id`,
`metadata_as_str`) is a per-Rust-file grep. A catalog↔docs reconciliation is
a whole-repo check with no natural anchor file, and `Violation` wants a
`file:line:column`. Making it fit means either adding a repo-level-check seam
to `lint/mod.rs`, or anchoring every violation at the catalog entry's line in
`error_catalog.json` (which is at least honest — that *is* where the offending
declaration lives).

## Design decisions (settled 2026-08-11 with Carlos)

1. **A narrow repo-level `cargo xtask lint` rule, not bd-8otua's full
   `error-docs` tool.** The stated goal is specific: *"avoid the situation
   where an agent implements a new diagnostic code but neglects to add the
   related documentation stub"*, and it must *"fire in our typical workflow,
   ideally during the local checks before opening a PR."* `cargo xtask lint`
   is exactly that surface — it runs standalone, as step 1 of `cargo xtask
   verify` (the documented pre-push gate), and in CI. Decisions 3 and 4 below
   narrow the check to two problem classes, which is well short of bd-8otua's
   five-class audit plus `health`/`new`. So the rule ships as
   `crates/xtask/src/lint/error_docs.rs`; **bd-8otua stays open** and should
   later absorb this module rather than duplicate it.

2. **Backfill all 28 pages in this session**, before the gate goes on.
   No allowlist, no warn-first mode. Pages carry real prose at
   `status: stub` — the bar is "the docs page has the right content (even if
   all LLM-generated stubs)", not a `<!-- TODO -->` skeleton.

3. **The check verifies exactly two things:** the page exists at
   `docs/errors/<subsystem>/<code>.qmd`, and `docs_url` equals
   `https://quarto.org/docs/errors/<subsystem>/<code>`. Orphan, misplaced,
   and front-matter `Mismatch` are explicitly out of scope for now (the probe
   shows all three are currently clean anyway); they remain bd-8otua's.

4. **Page `title` is free to differ from catalog `title`**, the way page
   `description` already differs from `message_template`. `Q-2-43`/`Q-2-44`
   stay as they are. A future audit may *recommend* alignment; it is not an
   error.

5. **Every code needs a page — no opt-out.** We do not distinguish internal
   from user-facing codes: the page is the first landing spot for anyone who
   encounters the code, and the page itself carries the context that makes
   the distinction. So the rule is unconditional over the whole catalog.

## Phases

Ordered so every commit leaves `cargo xtask verify` green — the gate goes on
only after the tree it guards is clean.

- [x] **Phase 0 — Test plan (TDD).** Unit tests over synthetic
      catalog/docs-tree fixtures: a code with a page, a code without, a
      `docs_url` that skips the subsystem, a `docs_url` that is entirely
      wrong. The check takes catalog path + docs root as parameters so tests
      never touch the real tree.
- [x] **Phase 1 — The check.** `crates/xtask/src/lint/error_docs.rs`, plus a
      repo-level-check seam in `lint/mod.rs` (existing rules are all
      per-Rust-file). Violations anchor at the offending entry's line in
      `error_catalog.json` — that is where the declaration that promises the
      page actually lives. Not yet wired into `run_check`.
- [x] **Phase 2 — Fix `Q-3-42` / `Q-3-43` `docs_url`.** Two-line catalog
      edit; independent of everything else.
- [x] **Phase 3 — Backfill the 28 missing pages.** `extension` (9, new
      directory), `project` (11), `lua` (4), `writer` (2), `theme` (1),
      `markdown` (1). Front-matter from the catalog; body follows the
      README's template; `status: stub`.
- [x] **Phase 4 — Turn the gate on.** Call the check from
      `lint::run_check`, so it reaches `cargo xtask lint`, `cargo xtask
      verify` step 1, and CI in one move.
- [x] **Phase 5 — Docs.** `docs/errors/README.md` and
      `crates/quarto-error-reporting/CONTRIBUTING-ERRORS.md`: adding a code
      now *requires* adding a page, and the lint says so.

## Open design questions for the user

1. **New lint rule, or land bd-8otua?** bd-8otua is unblocked, has a written
   plan, is referenced by `docs/errors/README.md` as a promised command, and
   its `audit` subcommand's first problem class *is* this strand. Writing a
   narrow `lint/error_docs_pages.rs` now means either throwing it away when
   `error-docs audit` lands, or shipping two overlapping checkers. My
   recommendation: implement bd-8otua's `audit` (missing + orphan + misplaced
   + `docs_url` drift; defer `health`/`new`/front-matter `Mismatch`), call it
   from `lint::run_check` so it inherits the CI + verify wiring, and close
   bd-u2qj4y29 as subsumed. Do you want that, or do you specifically want the
   small standalone lint rule?

2. **Do we backfill the 28 pages before turning the gate on, or does the
   check start as a warning?** These are the only two orders that don't break
   CI. Backfill-first is honest but is 28 pages of prose (`extension` and
   `project` are the bulk); warn-first ships the tooling today but relies on
   someone reading warnings. A third option: gate on *newly added* codes only,
   with the current 28 in a checked-in allowlist that can only shrink. Which?

3. **What does the check verify beyond "the file exists"?** The probe shows
   we can cheaply also catch orphans, misplaced pages, front-matter drift, and
   `docs_url` drift. `Q-3-42`/`Q-3-43` argue for including the `docs_url`
   check at minimum — file existence alone would have passed them while the
   link stayed broken. Is `docs_url` conformance in scope for this strand, or
   a separate one?

4. **Is title-case drift an error?** `Q-2-43`/`Q-2-44` use sentence case in
   the page title where the catalog uses title case. Flag it, normalize the
   pages to the catalog, or declare page `title` free to differ from catalog
   `title` (the way `description` is already free to differ from
   `message_template`)?

5. **Does every code need a page?** The strand asks this directly. The
   evidence says yes — `docs_url` is emitted unconditionally in diagnostics,
   so any code without a page is a shipped 404. But if there are codes we
   consider internal-only (`Q-0-1`, `Q-0-99` are `internal` and *do* have
   pages, so possibly not), an opt-out mechanism would need designing.
   Confirm "yes, all of them"?

## Risks / tradeoffs (draft)

- **The gate is only as good as its timing.** A check that lands in a red
  state gets `--fail-on none`'d and then ignored. The ordering question (2)
  is the one that decides whether this strand actually prevents the next
  page-less code or just documents that we ship them.
- **Backfilling 28 pages is a content task wearing a tooling task's
  clothes.** If the answer to (2) is backfill-first, most of the work is
  writing `extension` and `project` prose, which is a different kind of
  session than writing a lint rule. It may deserve its own strand under
  bd-94x8a alongside the closed bd-bj5yp/bd-lgxdr per-subsystem children.
- **Shape mismatch with the existing lint module.** Every current rule is a
  per-Rust-file grep with a `file:line:column` anchor. A repo-level check
  needs a new seam in `lint/mod.rs`; anchoring violations at the catalog
  entry's line is the least-invasive way to keep the `Violation` type
  unchanged.
- **The missing `discovered-from` edge.** bd-u2qj4y29 has no graph at all.
  Worth linking to bd-mermaid-cell-options-9wo3crl0 (`discovered-from`) and
  to bd-8otua (`related` or `duplicates`) so the overlap is visible from the
  tracker rather than only from this plan.

## Verification record (2026-08-11)

Two commits: `ebfa7f22` (content — 28 pages + the two `docs_url` fixes)
and the lint-rule commit that follows it.

### The gate fires (end-to-end, through the real binary)

Injected a fake `Q-2-99` into the catalog with no page, then ran the
binary a developer runs, unpiped so the exit code is the real one:

```
$ cargo xtask lint
crates/quarto-error-catalog/error_catalog.json:464:3: [error-docs-page-missing]
  Q-2-99 has no documentation page; diagnostics carrying this code link to
  https://quarto.org/docs/errors/markdown/Q-2-99, which 404s until
  docs/errors/markdown/Q-2-99.qmd exists
  suggestion: create docs/errors/markdown/Q-2-99.qmd following
  docs/errors/README.md (front matter from the catalog entry, `status: stub`)

LINT EXIT CODE = 1
```

Line 464 is `Q-2-99`'s own line in the catalog. Deleting an existing page
(`docs/errors/extension/Q-16-5.qmd`) produces the same shape. Catalog
restored after both probes; `git diff` on the catalog shows only the
intended two-line `docs_url` change.

Before the backfill, `the_real_catalog_and_docs_tree_agree` failed with
all 28 missing pages plus the 2 URL drifts — the TDD red.

### The docs site renders clean

`cargo run --bin q2 -- render docs/`, compared against a stashed
baseline of the same command on unmodified `main`:

|                  | baseline (main) | after |
| ---------------- | --------------: | ----: |
| exit code        |               0 |     0 |
| files rendered   |         197/197 | 225/225 |
| warnings         |              25 |    25 |
| errors           |               0 |     0 |

Same 25 warnings in both: 11 `Q-13-4` body links in `brand.qmd` and 14
`Q-5-6` missing images in `figures.qmd`, all pre-existing. The 28 new
pages contribute **zero** warnings. All 193 codes resolve in the listing
at `docs/errors/index.html`; `docs/_site/errors/extension/` holds all
nine new pages. Spot-checked the rendered HTML of `Q-16-5` and `Q-3-42`.

### Two authoring traps, both real bugs in the first draft

1. **Bare shortcodes in prose *and* in fenced code blocks are executed.**
   The first render fired 15 genuine `Q-16-3`/`Q-16-5` diagnostics from
   the pages documenting those very codes — `{{< meta version >}}` inside
   a ```` ```markdown ```` fence resolved rather than displaying. The
   convention the rest of `docs/` uses is the triple-brace form
   `{{{< … >}}}`, which renders as `{{< … >}}` in both inline code spans
   and fenced blocks. Sibling pages `Q-2-27`/`Q-2-28` instead use a fence
   attribute, ```` ```{.markdown shortcodes="false"} ```` — equivalent
   output, and arguably better source readability for fenced examples.
   Worth standardizing on one; not done here.
2. **A trailing possessive apostrophe opens a single quote.** "the
   scripts'" failed `Q-5-12` with `Q-2-7` (unclosed single quote) and took
   the whole page out of the render, which in turn produced two `Q-13-4`
   warnings on the pages linking to it. Rephrased.

Both are worth knowing before writing the *next* error page; neither is a
defect in the lint rule.

### Discovered work filed

- **bd-8meeijgq** (`discovered-from` bd-u2qj4y29) — `figures.qmd`
  references 14 images that do not exist in the repo. Pre-existing, found
  while establishing the render baseline; not a regression from this work.

### Still open after this strand

**bd-8otua** stays open and unblocked. It owns the richer
`cargo xtask error-docs` audit — orphan pages, misplaced pages,
front-matter `Mismatch`, `health` rollups, and the `new <Q-X-Y>`
generator that `docs/errors/README.md` still describes as unshipped. When
it lands it should **absorb** `lint/error_docs.rs`, not duplicate it.

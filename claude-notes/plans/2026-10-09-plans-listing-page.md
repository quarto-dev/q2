---
title: "claude-notes: a listing page for all plans"
date: 2026-10-09
description: 'Adds a table-based listing page for all top-level plans, backfilling title and date front matter from each plan''s heading and filename because the listing is only useful once that metadata exists.'
status: phases 1–5 done 2026-10-09; strict render clean once PRs #810 and #812 are on main
braid: bd-fvcip3t5 (child of epic bd-uk8zgkha)
---

## Goal

Add one page, `claude-notes/plans/index.md`, that lists every top-level plan
using a Quarto 2 `listing:`. This is the first site-polish step for the
claude-notes website (epic bd-uk8zgkha). We start with something simple and
change the design over later sessions.

All work is checked in `q2 preview --static`, not in the Automerge SPA
preview. The static preview runs a real `q2 render` to `_site`, serves the
result, and re-renders on save. What it shows is what the CI gate
(`q2 render --strict claude-notes`) builds.

## What I found

### Docs

`q2 docs llms guides/projects/listings.md` covers `contents:` (globs,
records, ordering) and custom templates in depth. It does not cover the
basics: what `type:` values exist, `fields:` and the default fields for each
type, `sort:` syntax, `filter-ui` / `sort-ui` / `page-size`, `categories`,
`date-format`, or where an item's title and date come from when front matter
lacks them. I added these gaps to the existing docs strand bd-2nb6i1qv
instead of filing a duplicate. The answers below come from experiments and
from `crates/quarto-core/src/project/listing/`.

### Experiments

All runs use q2 0.33.0-nightly.20261009 on a scratch copy of the 1051
top-level `plans/*.md` files.

1. **The table layout works at full size.** With `type: table` and
   `contents: "plans/*.md"`, all 1051 rows render, and the full project
   render takes 1.7 s. However, `filter-ui`, `sort-ui` and `page-size` emit
   nothing for tables, so there is no search box and no paging. That is
   known gap bd-bl1e00r6.
2. **The default and grid layouts break at about 90 items.** The listing is
   dropped from the page, with only a Q-12-10 warning ("too deeply nested
   (more than 99 levels)"). The threshold does not depend on content: 89
   items work and 90 fail for `default`, and grid already fails at 89. I
   reproduced it with synthetic one-line documents. Filed as **bd-mlmkev01**
   (P1). Until it is fixed, we have to use `table`. Under `--strict`, the
   warning would also fail CI.
3. **Titles fall back to the file stem.** Every plan has a leading `# H1`,
   but only 7 of 1051 have front matter. So the listing shows
   `2025-10-20-k-87-sourceinfo-audit` instead of "k-87:
   SourceInfo::default() Audit". Quarto 1 falls back to the first heading
   (`project-index.ts`), but q2 does not. Filed as **bd-pnajor0b**.
4. **There are no dates.** Plan dates exist only in filenames and in
   free-form `**Date:**` body lines, so the `date` column is empty. Q1 and
   q2 both do not derive dates from filenames. A `sort: "filename desc"`
   gives an approximate newest-first order, but the two undated files
   (`js-execution-performance.md`, `kyoto-676-reconcile-analysis.md`) sort
   first.
5. **Table rows have empty descriptions.** The auto-derived first-paragraph
   description is only used by the default and grid layouts.

The conclusion is that metadata is the real work here, not the listing. The
listing YAML is about ten lines, but it only looks good once each plan has
`title:` and `date:` in front matter.

## Design

### The listing page

The page goes at `claude-notes/plans/index.md`. It must be a `.md` file:
`_quarto.yml` renders only `**/*.md`, because the `.qmd` files in this tree
are repro fixtures.

```yaml
---
title: Plans
listing:
  type: table
  contents: "*.md"        # top level only; *-investigation/ subdirs excluded
  sort: "date desc"
  fields: [date, title]
  date-format: iso
---
```

The `*.md` glob matches only `plans/` itself. The roughly 135
`*-investigation/` directories and their 140 or so nested `.md` files stay
out of the listing for now. We still need to confirm during implementation
that the host page excludes itself and that the gitignored `CURRENT.md`
symlink does not show up. `CURRENT.md` is excluded from rendering, so it
should not.

We also add one navbar entry (`website.navbar.left: [{href: plans/index.md,
text: Plans}]`) so the page can be reached.

### Front matter backfill

The backfill sets two fields on every plan:

- **`title:`** comes from the first `# H1`, and **we delete that H1**.
  Otherwise the page shows the title twice, once in the title block and
  once as the heading.
- **`date:`** comes from the `YYYY-MM-DD` filename prefix. For the two
  undated files, it comes from `git log --diff-filter=A --follow` (the first
  commit date).

Plans that already have front matter get these keys merged in. Existing
keys are never overwritten. In particular, several of those plans already
have a `date:`.

The H1 text is already q2 inline markdown, with code spans and escapes such
as `\@`. The script writes it as a **single-quoted** YAML scalar, doubling
any `'`. In single quotes, backslash escapes survive without change. A
double-quoted scalar would turn `\@` into an invalid YAML escape. Metadata
strings are parsed as markdown, so an unparseable title shows up as Q-1-20
in the strict render.

This step should be a **deterministic script, not an LLM workflow**. It is
purely mechanical, the script is easy to review, and its output can be
diffed. The script will be `scripts/claude-notes-plan-frontmatter.py`,
following the precedent of `scripts/q2-dedent-task-items.py`. It needs
`--dry-run` mode and a report of anything it skipped, such as a file with no
H1 or an H1 that is not the first block.

### Descriptions (Haiku workflow)

`description:` is where model judgment pays off. Each plan gets one sentence
saying what the plan is about, written in plain text with no markup. It goes
in front matter and becomes the table's third column, or the card blurb once
bd-mlmkev01 makes the default layout usable.

This is a good fit for the dynamic-workflow pattern we used in the
render-clean sweep (bd-uk8zgkha, 2026-10-07):

- Haiku agents implement, with about 10 plans per agent, which gives about
  105 agents. Each agent writes only the `description:` line and touches
  nothing else.
- After each batch: a mechanical check (the diff is only additions inside
  the front matter), `q2 render --strict claude-notes`, and a spot review by
  me before each commit.
- This step is independent of the listing and can come last.

Other metadata could follow the same pattern later, in another iteration:
`categories:` (subsystem, or plan kind such as investigation, epic or
design) and a normalized `status:`.

### Keeping new plans clean

New plans should carry front matter from the start:

- `.claude/skills/investigate-beads/references/plan-skeleton-template.md`:
  replace `# <Issue title>` and the `**Date:**` line with a front matter
  block (`title`, `date`, `braid`, `status`).
- `claude-notes/instructions/writing-notes.md`: one short section on
  required front matter.
- Grep the other skills that write into `claude-notes/plans/`
  (`ejs-listing-port`, `preview-render-parity`, `upgrade-cargo-deps`,
  `AGENTS.md`) and align any of them that embed a plan header.

## Phases

1. **Setup.** Create a worktree with `cargo xtask create-worktree bd-fvcip3t5
   --base main`, then start `q2 preview --static --no-browser --port 4400`
   in `claude-notes/` in the background. Look at the pages with the
   Chrome tools.
2. **Listing on today's metadata.** Write `plans/index.md` as above, but
   with `sort: "filename desc"` and `fields: [title, filename]`, and add the
   navbar entry. Check that the page appears, that the host page and
   `CURRENT.md` are excluded, and that the strict render is clean. Commit.
3. **Mechanical backfill.** Write the script, run it as a dry run, review
   the report, then apply it in a single commit. Then switch the listing to
   `sort: "date desc"`, `fields: [date, title]`. Check that
   `q2 render --strict claude-notes` reports 0 diagnostics, and do a visual
   pass in the static preview, including that each plan page shows its
   title block once.
4. **Conventions.** Update the template, `writing-notes.md` and the skills.
   Commit.
5. **Descriptions.** Run the Haiku workflow in growing stages (about 10,
   then 50, then 200, then the rest of the plans), with a review and a
   commit after each stage. Then add `description` to `fields:`.

Phases 2 to 4 are small, and each one leaves a working site. Phase 5 is the
expensive one, so it waits for the earlier design to be accepted.

## Decisions (2026-10-09)

1. **The H1 is removed** when it moves into `title:`.
2. **The `**Date:**` / `**Status:**` body lines stay in the body for now.**
   We do want them in front matter eventually, so that custom listings can
   show them, but that is a later pass. It is probably a Haiku pass, because
   the formats vary.
3. **A table layout is fine for the first version.** bd-mlmkev01 is being
   fixed separately, and cards can come after that.
4. **bd-pnajor0b** stays open as a q2 parity bug. claude-notes will not
   depend on it.
5. **The Haiku description workflow grows in stages, with a review after
   each one:** 1 agent with about 10 plans to debug the process, then about
   50 plans, then about 200, then everything left.

## Progress

- 2026-10-09: phase 2 is done (d07488db8). In phase 3, the script ran
  cleanly on 1046 files, 4 files were edited by hand, and the diff was
  checked mechanically: 1050 H1s were removed, and only `title:` and
  `date:` lines were added. Plan pages render with one title block and a
  date.
- **Blocker found in phase 3: bd-8a9eum6p.** Listing item titles are
  flattened to plain text and then re-parsed as markdown. 27 titles
  contain code spans whose text means something in markdown (`_brand.yml`,
  `<anonymous>`, backticks). Re-parsing them fails, and the whole listing
  is dropped. This is an error under `--strict`, so the CI gate fails on
  this branch until bd-8a9eum6p is fixed or worked around. Four titles
  that used `\_` escapes now use code spans, which is the right markup
  once the bug is fixed.

- 2026-10-09, later: PRs #810 (bd-mlmkev01) and #812 (bd-8a9eum6p) are
  open. A local build of main with both merged in
  (`.worktrees/spec-listing-fixes`) renders the site with `--strict`
  clean. CI builds q2 from the branch under test, so merging main into
  this branch after both PRs land is enough to make CI green.
- Phase 5 is done. Haiku wrote the descriptions as structured output, and
  `--descriptions` mode applied them, with a rule checker run and a strict
  render before every commit. The stages were 11, then 50, then 198, then
  792 plans (1, 5, 20 and 80 agents, about 8M subagent tokens in total).
  Hand fixes: two plural possessives (both are Q-2-10 traps; the prompt
  forbids them after stage 2), two identifiers moved into code spans, and
  one path that Haiku mangled in its output. The listing now shows date,
  title and description.

## Phase 6: status, braid strand and dates in front matter

Requested on 2026-10-09. About 800 plans have a header block of bold
`**Key:** value` lines. The common keys are Status (532), Date (484),
Branch, Strand/Braid/Beads/Issue (about 480 in total, with varied
spelling), and Created/Updated. This phase moves three kinds of line into
front matter:

```yaml
date: 2026-10-08
date-modified: 2026-10-09   # from an Updated / Last updated line
status: in-progress  # the original Status text, kept as a comment
braid:
  strand: bd-windows-arm64-nightly-xms5p652  # relationships from the line
  priority: P2
  labels: [ci, release, windows]
```

- **`status`** records what the plan claims, not the strand status. It
  takes one of `draft`, `approved`, `in-progress`, `blocked`, `done`,
  `superseded` or `abandoned`, and the original free text becomes a YAML
  comment.
- **`braid.strand`** is the lookup key for a future Lua filter that reads
  the skein. It is always a bare id. Old `k-NNN` and short beads ids
  resolve in braid. `kyoto-NNN` ids do not, so those plans get no
  `braid:` and keep their line. Epic, parent and related ids stay in the
  body.
- **`priority` and `labels`** come from the skein
  (`braid list --all --json`), not from the plan text.
- **Dates** are ISO 8601. A Date or Created line that matches `date:` is
  removed. One that does not match is reported and kept.
- **Unchanged for now:** Branch, Worktree, Epic, Depends on and the
  other keys stay in the body.

Haiku extracts each field as structured output, together with the exact
body lines it came from. The `--header-meta` mode of
`scripts/claude-notes-plan-frontmatter.py` validates everything: the
status is in the vocabulary, the strand id exists exactly (`braid show`
matches prefixes, so `bd-9` would be ambiguous), the dates are ISO, and
the lines appear verbatim in the header. Only then does it rewrite the
file. Anything that fails is reported and left as it was. The rollout
grows in stages, as in phase 5.

## Strands

- bd-fvcip3t5: this work (child of bd-uk8zgkha)
- bd-mlmkev01: default and grid listings vanish at about 90 items (found here)
- bd-pnajor0b: listing title should fall back to the first H1 (found here)
- bd-2nb6i1qv: listings guide gaps (commented)
- bd-8a9eum6p: listing titles are flattened to plain text and re-parsed (found here; blocks phase 3)
- bd-bl1e00r6: table `filter-ui` / `sort-ui` / `page-size` (existing)

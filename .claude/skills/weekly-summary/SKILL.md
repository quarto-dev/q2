---
name: weekly-summary
description: Write a Slack-ready summary of Quarto 2 (q2) work landed on main since a point in the past — a short themes paragraph plus a changelog-style bullet list of merged PRs. Use when the user says "weekly summary", "what landed this week", "summarize recent work for Slack", or invokes /weekly-summary [since]. The optional argument is a date (2026-10-27), a relative span ("7 days", "2 weeks"), or a git ref/tag; the default is 7 days.
---

# Weekly summary

Produces a message the user will paste into Slack and lightly edit. The
audience is the Quarto team and adjacent colleagues: people who know what
Quarto is but did not follow every PR. Write for them, not for the PR authors.

Read-only: this skill fetches and reads; it never commits, pushes, comments,
or posts anywhere.

## 1. Resolve the window

Parse the argument into a `SINCE` date (UTC, `YYYY-MM-DD`):

- empty → today minus 7 days
- `YYYY-MM-DD` → as given
- `N days` / `N weeks` → today minus that span
- anything else → treat as a git ref; `SINCE` is that commit's committer date
  (`git log -1 --format=%cs <ref>`)

State the window in one line before gathering ("Summarizing main from
2026-10-27 to 2026-11-03").

## 2. Gather

Fetch first — the local `main` is often behind, and the summary must reflect
`origin/main`:

```bash
git fetch origin main --quiet
```

**Merged PRs into main** (the backbone of the summary):

```bash
gh pr list --repo quarto-dev/q2 --state merged --base main \
  --search "merged:>=$SINCE" --limit 300 \
  --json number,title,author,mergedAt,headRefName,labels,additions,deletions,body \
  --jq '.[] | {number, title, author: .author.login, mergedAt, head: .headRefName,
               labels: [.labels[].name], size: (.additions + .deletions),
               body: (.body // "" | .[0:800])}'
```

If the result count hits the limit, say so and narrow the window rather than
silently truncating.

**Direct commits to main** (pushes that bypassed a PR — usually docs, skills,
small fixes): first-parent, non-merge commits in the window.

```bash
git log origin/main --first-parent --no-merges --since="$SINCE" \
  --format='%h%x09%an%x09%s'
```

Drop any whose subject ends in `(#NNN)` — those are squash-merged PRs already
counted above.

**Integration branches.** PRs merged into a `feature/*` or `refactor/*` branch
(not main) are not shipped yet. Don't list them individually; when the
integration branch itself lands on main, its PR covers them. If one is
notably active, a single "in progress" line in the themes paragraph is enough:

```bash
gh pr list --repo quarto-dev/q2 --state merged --search "merged:>=$SINCE -base:main" \
  --limit 100 --json number,title,baseRefName
```

## 3. Understand before writing

Titles alone mislead. For each PR, use the title, branch name, and body
excerpt to decide:

- **What changed for a user** (a Quarto author, a hub-client user, an
  extension author) — or, for internal work, what it enables.
- **Area** — pick from this list, adding one only if a cluster clearly
  doesn't fit:
  - *Rendering & formats* (HTML, PDF/Typst/LaTeX, docx, revealjs, websites, books, listings)
  - *Authoring features* (crossrefs, citations, shortcodes, callouts, brand, layout)
  - *Execution & engines* (knitr, jupyter, `execute:` options)
  - *Extensions & Lua filters*
  - *Preview & hub-client* (q2 preview, quarto-hub web app, collaboration, MCP)
  - *Parser & language* (tree-sitter-qmd, pampa, diagnostics)
  - *Q1 compatibility* — use when the PR's point is matching Quarto 1 behavior
  - *Infrastructure* (CI, build, deps, perf, test harness, agent tooling/skills)
- **Weight** — new capability > user-visible fix > internal cleanup. Size
  (`additions + deletions`) is a weak hint, not a ranking.

When a body is empty and the title is cryptic, read the diffstat
(`gh pr view N --json files --jq '.files[].path'`) rather than guessing. Do
this sparingly; most PRs are clear from title + branch + body.

**Collapse noise**: dependency bumps (Snyk, Dependabot, `upgrade-cargo-deps`),
CI-only tweaks, and lint passes collapse into one bullet per kind
("Dependency upkeep: 4 PRs (Snyk npm bumps, cargo patch upgrades)").

## 4. Write

Two parts.

**Themes paragraph** (3–6 sentences). Lead with the biggest story of the
period, then the next one or two. Name concrete outcomes ("`q2 render` now
honors `--no-execute` and `execute:` under `format:`"), not activity ("lots of
work on execution"). If there is no real theme, say the period was mostly
fixes across areas and name the two or three most notable — don't invent a
narrative. End with one sentence of totals: "N PRs merged, M direct commits,
K contributors."

**Changelog**, grouped under the areas from step 3 (omit empty areas; order
areas by how much landed). One bullet per PR:

```
- Short user-facing description of the change. (#1046)
```

- Rewrite titles into plain past-tense or present-tense statements; drop
  conventional-commit prefixes, braid ids, and branch jargon.
- Mention the author only when it's not the user (`gh api user --jq .login`)
  — e.g. "… (#1031, @someone)". Never `@`-mention bots.
- Merge two or three PRs into one bullet when they are clearly one change
  delivered in pieces: "(#1040, #1041)".
- Direct commits that matter get bullets too, with the short hash instead of
  a PR number; trivial ones (typos, notes) are omitted.
- Keep each bullet to one line where possible.

### Slack formatting

The user pastes this into Slack's composer, so:

- Bold area headings with single asterisks: `*Rendering & formats*`.
- Plain `- ` bullets, no nesting.
- PR references as bare `#1046`. If the user wants links, switch to full URLs
  (`https://github.com/quarto-dev/q2/pull/1046`); Slack's composer does not
  turn `[text](url)` markdown into links reliably.
- No `#` headings, no tables.

Print the finished message inside a single fenced block (` ```text `) so it
copies cleanly, then, outside the block, a short list of judgment calls the
user may want to revisit (PRs you weren't sure how to describe, things you
grouped or dropped).

---
name: autonomously-fix-small-issue
description: Take a small, unambiguous GitHub issue from report to open PR without further user input — braid strand, plan doc, topic branch, fix with tests, PR. User-invoked only (/autonomously-fix-small-issue <N or issue URL>). Starts with a scope gate that stops and hands off to /triage when the issue is not small, not unambiguous, or needs a product decision. Never merges, never closes the issue or the strand.
disable-model-invocation: true
---

# autonomously-fix-small-issue

The user has pre-approved this work: once the scope gate passes, do **not**
stop to ask for permission at each step. That pre-approval is the risk this
skill's name advertises, so the scope gate is the one place to be strict.
When in doubt, fail the gate.

Reference run: GH #930 → PR #934 (hub-client tab-title order; one helper,
one call site, unit tests, ~20 minutes).

## Invariants

- **Stop at the PR.** Never merge, never close the GH issue, never close the
  braid strand. `Closes #N` in the PR body is enough; the strand stays
  `in_progress` until the PR merges.
- **Never touch the user's in-flight work.** No stashes left behind, no
  commits on someone else's branch, no rewrites of `CLAUDE.local.md` in the
  main checkout.
- **Report verification as it happened.** Every check you ran, its result,
  and every check you did *not* run go in the PR body.
- **No scope creep.** Incidental problems you notice become their own braid
  strands (`--deps discovered-from:<id>`), not extra commits in this PR.

## 1. Read the issue

```bash
gh issue view <N> --repo quarto-dev/q2 --json title,body,author,labels,comments
```

Read the body and every comment. Then locate the code the fix would touch —
enough to know the size of the change, not more.

## 2. Scope gate

Proceed only if **all** of these hold. Otherwise stop, tell the user which
check failed and why, and suggest `/triage <N>`. Do not create a strand or a
branch on a failed gate.

- [ ] **One request.** The issue asks for one thing (not a list of bugs).
- [ ] **Unambiguous.** The desired behaviour is stated, or there is exactly
      one reasonable reading. Maintainer comments don't disagree with it.
- [ ] **No open product/UX/API decision.** If you would have to choose between
      two defensible behaviours, that is the user's call — fail the gate.
- [ ] **Small.** Roughly ≤ 3 production files and ≤ ~100 changed lines,
      excluding tests. No new dependencies, no schema / file-format /
      public-API / CLI-flag changes, no migrations.
- [ ] **Testable.** There is a natural place for an automated test that
      fails before the fix and passes after (or, for docs-only changes, the
      docs build). If the only verification is "look at it in a browser",
      fail the gate.
- [ ] **Clean start.** `git status` is clean (see § 3 if it isn't).

Before you continue, tell the user in one or two sentences that the gate
passed and what you're about to change.

## 3. Strand, branch, plan

```bash
braid create "<area>: <headline>" -t bug|feature|task -p <0-4> -l <area> \
  -d "GH #<N> (<reporter>). <what's wrong, where>. Plan: claude-notes/plans/<date>-<slug>.md"
```

Branch per `.claude/rules/worktrees.md` § Branch naming (`braid/<id>-<slug>`):

- **Main checkout, clean, on `main`:** branch in place —
  `git switch main && git pull --ff-only && git switch -c braid/<id>-<slug>`.
  Don't use `switch-task` here; it rewrites the user's `CLAUDE.local.md`.
- **Anything else** (dirty tree, another branch checked out, in-flight
  work): `cargo xtask create-worktree <id> --base main` and work there.
  Bootstrap per the worktrees rule (`npm install` only if hub-client is in
  scope).

Then `braid update <id> --status in_progress`.

Write a short plan at `claude-notes/plans/<YYYY-MM-DD>-<slug>.md` with the
usual front matter (`title`, `date`, `description`), the strand id, the
issue URL, an Overview (current behaviour → new behaviour, with code
locations), and a checkbox work-item list. Keep it short — it is the record,
not a design doc. Tick items as you finish them.

The note is qmd, not GitHub Markdown: CI runs `q2 render --strict
claude-notes`, and any warning fails the PR (see `AGENTS.md` and
`claude-notes/instructions/writing-notes.md`). The usual trap in this skill
is placeholders: a bare `<sha>` or `<ref>` parses as a raw HTML element
(Q-2-9), and that includes the `title` and `description` front matter, which
are parsed as markdown. Put placeholders in code spans, or leave them out of
the front matter.

## 4. Fix, test-first

- Write or update the test first and watch it fail for the right reason.
- Make the smallest change that passes. Match the surrounding code's idiom
  and comment density. If extracting a tiny pure helper makes the behaviour
  unit-testable (as in #930), do that rather than testing through a
  component.
- Follow the crate/package `AGENTS.md` and `.claude/rules/*` that apply to
  the files you touched (cross-platform, integration-test layout, hub-client
  theming, WASM).

## 5. Verify

Run the checks that cover what you touched, not just the one new test:

- **hub-client / ts-packages:** the affected vitest files, `tsc --noEmit`,
  and ESLint on changed files. Use the pinned Node (`fnm use`).
- **Rust:** `cargo nextest run -p <crate>` for each touched crate, `cargo
  clippy -p <crate> --all-targets -- -D warnings`, and the Windows cross
  clippy from `.claude/rules/cross-platform.md`.
- **Larger blast radius:** if you're unsure what depends on the change,
  run `cargo xtask verify --skip-hub-build`.
- **Notes (always):** from `claude-notes/`, run `cargo run --bin q2 --
  render --strict <path>` on the plan, and on every other note you added or
  changed. Any warning fails CI, so fix them all before you push.

**Pre-existing failures:** if a check fails, re-run it on `main` (e.g.
`git stash`, run, `git stash pop`) to tell whether you caused it. Only
pre-existing failures may be left in place, and the PR body must say so
with counts. If you caused a failure and can't fix it within the scope you
gated on, stop and report to the user instead of opening the PR.

## 6. Commit, push, PR

One commit (or a few logical ones). The message explains why, ends with
`Closes #<N>. Strand: <id>.`, and carries the attribution trailer from the
session's system reminder.

```bash
git push -u origin braid/<id>-<slug>:feature/<id>-<slug>   # bugfix/ for bugs
gh pr create --base main --head feature/<id>-<slug> --title "<area>: <headline>" --body ...
```

The local and remote branch names differ, so a bare `git push` **fails** on
every later push too (a CI fix, review changes). Always repeat the refspec,
don't silence the push's output, and confirm it landed with `git ls-remote
origin <remote-branch>` against `git rev-parse HEAD` before you say it's
pushed. See `.claude/rules/worktrees.md` § Pushing for PR.

PR body: `Closes #<N>.`, the behaviour change in one or two sentences, what
changed (files/functions), **Testing** (each check and its result,
pre-existing failures called out, anything not verified, e.g. "not checked in
a real browser"), and `Strand: <id> · Plan: <path>`.

Then:

```bash
braid comment <id> "PR opened: <url>"
```

## 7. Report back

Tell the user: the PR link, the behaviour change in plain words, what was
verified and what wasn't, and the strand/plan/branch names. Leave the strand
open for the merge.

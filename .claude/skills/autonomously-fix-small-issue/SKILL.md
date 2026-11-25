---
name: autonomously-fix-small-issue
description: Take a small, unambiguous GitHub issue from report to a green, mergeable PR without further user input — braid strand, plan doc, topic branch, fix with tests, PR, then watch CI and fix failures and merge conflicts until the checks pass. User-invoked only (/autonomously-fix-small-issue <N or issue URL>). Starts with a scope gate that stops, records the reason as a marked comment on the issue, and hands off to /triage when the issue is not small, not unambiguous, or needs a product decision. Also has gate-only, sweep, and repair modes used by the nightly-autofix workflow (/nightly-issue-sweep). Never merges, never closes the issue or the strand.
disable-model-invocation: true
---

# autonomously-fix-small-issue

The user has pre-approved this work: once the scope gate passes, do **not**
stop to ask for permission at each step. That pre-approval is the risk this
skill's name advertises, so the scope gate is the one place to be strict.
When in doubt, fail the gate.

Reference run: GH #930 → PR #934 (hub-client tab-title order; one helper,
one call site, unit tests, ~20 minutes).

## Modes

The default is the **interactive** mode, which is everything below. The
nightly-autofix workflow (`/nightly-issue-sweep`) runs the skill in one of
three other modes, and its prompt names the mode:

- **gate-only**: §1, §2, and § Recording a gate failure. Then stop and
  return the verdict. No strand, no branch.
- **sweep**: like interactive, but work in the shared
  `.worktrees/autofix` checkout (§3), label the PR `autofix` (§6), and
  **skip §7**. Stop once the PR is open and the push is confirmed. The
  workflow's repair pass deals with CI later, so don't wait for it.
- **repair**: § Repair mode only, for one existing `autofix` PR.

In every mode other than interactive, ignore the `# Task Context` block in
the main checkout's `CLAUDE.local.md`. It describes the user's own work,
not yours.

## Invariants

- **Stop at a green PR.** Open the PR, then watch CI and fix what breaks
  (§ 7) until every check passes and the PR is mergeable. (In sweep mode,
  stop at an open PR. See § Modes.) Never merge, never
  close the GH issue, never close the braid strand. `Closes #N` in the PR
  body is enough; the strand stays `in_progress` until the PR merges.
- **Never touch the user's in-flight work.** No stashes left behind, no
  commits on someone else's branch, no rewrites of `CLAUDE.local.md` in the
  main checkout.
- **Report verification as it happened.** Every check you ran, its result,
  and every check you did *not* run go in the PR body.
- **No scope creep.** Incidental problems you notice become their own braid
  strands (`--deps discovered-from:<id>`), not extra commits in this PR.
- **Label AI-written GitHub comments.** Every issue or PR comment you post
  goes out under the user's credentials, so its first visible line must be
  `Authored by <model name> 🤖`, using whatever name you know yourself by
  (e.g. "Claude Opus 5.5"; any recognizable form is fine). It comes right
  after the machine-readable marker, if the comment has one, and before
  everything else. PR descriptions carry the attribution footer from the
  session's system reminder instead.

## 1. Read the issue

```bash
gh issue view <N> --repo quarto-dev/q2 --json title,body,author,labels,comments
```

Read the body and every comment. Then locate the code the fix would touch —
enough to know the size of the change, not more.

## 2. Scope gate

Proceed only if **all** of these hold. Otherwise stop, record the failure
(§ Recording a gate failure), tell the user which check failed and why, and
suggest `/triage <N>`. Do not create a strand or a branch on a failed gate.

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
- [ ] **Clean start.** `git status` is clean (see § 3 if it isn't). In
      sweep mode this is the `.worktrees/autofix` checkout, which the
      workflow's housekeeping step has already reset.

Before you continue, tell the user in one or two sentences that the gate
passed and what you're about to change.

### Recording a gate failure

**Gate version: 1.** Bump it here (and in the marker below, and in
`GATE_VERSION` in `.claude/workflows/nightly-autofix.js`) when you change
the checklist above in a way that should re-evaluate issues already
rejected. The nightly selector re-gates any issue whose last marker is
older.

First, sort each failed check:

- **Intrinsic:** the failure is a property of the issue: more than one
  request, ambiguous, needs a product decision, too large, not testable.
  This doesn't change until someone edits or comments on the issue.
- **Environment:** the failure is a property of this run: a dirty tree,
  red HEAD, a missing toolchain, a network failure, or code you couldn't
  find because the checkout is broken.

**Record only intrinsic failures.** An environment failure gets no
comment, so the issue is retried on the next run. When at least one
intrinsic check failed, post exactly one comment. Its first line is a
machine-readable marker that the nightly selector
(`.claude/skills/nightly-issue-sweep/scripts/select_candidates.py`)
parses, so keep it verbatim:

```bash
gh issue comment <N> --repo quarto-dev/q2 --body-file - <<'MARKER'
<!-- q2-autofix gate v=1 verdict=fail checks=<comma-separated slugs> -->
Authored by <model name> 🤖

**Automated triage:** this issue isn't a candidate for an unattended fix yet.

- *<Check name>:* <one sentence, specific to this issue>
- ...

Editing the issue or adding a comment (for example, splitting it into separate issues, or settling the open question) makes it eligible for another look.
MARKER
```

Check slugs: `one-request`, `unambiguous`, `no-product-decision`, `small`,
`testable`. The reporter reads this comment, so keep each reason concrete,
neutral, and short. Point at the part of the issue that causes the
failure, and don't speculate about the fix. When the workflow says
`dry-run`, don't post the comment; return its text instead.

## 3. Strand, branch, plan

```bash
braid create "<area>: <headline>" -t bug|feature|task -p <0-4> -l <area> \
  -d "GH #<N> (<reporter>). <what's wrong, where>. Plan: claude-notes/plans/<date>-<slug>.md"
```

Branch per `.claude/rules/worktrees.md` § Branch naming (`braid/<id>-<slug>`):

- **Sweep mode:** always work in `<repo-root>/.worktrees/autofix`, which
  the workflow leaves detached at a fresh `origin/main`:
  `git -C <repo-root>/.worktrees/autofix switch -c braid/<id>-<slug>`.
  Run every command from there (`cd <abs-path> && ...`, since a subagent's
  working directory may not persist between commands), and run `npm
  install` there if hub-client is in scope. Don't use `create-worktree` or
  `switch-task`.

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
- **Custom lints (always):** `RUSTFLAGS="-D warnings" cargo xtask lint`.
  CI runs it early in the test-suite job, and it catches things the other
  checks don't (e.g. committed q2 render output in a triage folder, which
  failed PR #1093).
- **Notes (always):** from `claude-notes/`, run `cargo run --bin q2 --
  render --strict <path>` on the plan, and on every other note you added or
  changed. Any warning fails CI, so fix them all before you push. A note
  added under `plans/` re-renders the `plans/` listing, so a partial render
  can report Q-12-13 ("produced no preview content") for pages it didn't
  render; if that's all you see, confirm with a full `q2 render --strict`
  from `claude-notes/`, which is what CI runs.

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

In sweep mode, add `--label autofix` to `gh pr create`. If the label
doesn't exist yet, create it first: `gh label create autofix --repo
quarto-dev/q2 --color 0E8A16 --description "PR opened by the nightly
autofix sweep"`. Open the PR ready for review, not as a draft, so CI runs.

PR body: `Closes #<N>.`, the behaviour change in one or two sentences, what
changed (files/functions), **Testing** (each check and its result,
pre-existing failures called out, anything not verified, e.g. "not checked in
a real browser"), and `Strand: <id> · Plan: <path>`.

Then:

```bash
braid comment <id> "PR opened: <url>"
```

## 7. Watch CI and fix what breaks

Opening the PR is not the end. Watch its checks until every one has finished,
and fix failures as they come in. Use a `Monitor` that polls the check runs
for the pushed commit and prints each one as it completes, so a failure
reaches you as soon as it happens:

```bash
SHA=$(git rev-parse HEAD); prev=""
while true; do
  s=$(gh api "repos/quarto-dev/q2/commits/$SHA/check-runs?per_page=100" \
        --jq '[.check_runs[] | {id, name, status, conclusion}]' || echo '[]')
  cur=$(jq -r '.[] | select(.status=="completed") | "\(.id) \(.name): \(.conclusion)"' <<<"$s" | sort)
  comm -13 <(echo "$prev") <(echo "$cur") | cut -d' ' -f2-; prev=$cur
  [ "$(jq length <<<"$s")" -gt 0 ] && jq -e 'all(.status=="completed")' <<<"$s" >/dev/null \
    && { echo "ALL CHECKS DONE"; break; }
  sleep 45
done
```

Watch by commit SHA, not by PR: after every push, stop the old monitor and
start a new one for the new `HEAD`. CI on the PR takes 30–60 minutes, which
outlasts one monitor, so re-arm it when it expires.

**Merge conflicts.** Check `gh pr view <PR> --json mergeable,mergeStateStatus`
once CI starts and again when it finishes. On `CONFLICTING`, merge (don't
rebase or force-push) `origin/main` into the branch. Resolve the conflict,
then re-run the § 5 checks for every crate or package the incoming changes
touched, not just yours. Push with the refspec and confirm with `git
ls-remote`.

**A failing check.** Job logs are unavailable until the *whole run*
finishes, so don't wait on them. First find the failed step:

```bash
gh run view <run-id> --repo quarto-dev/q2 --json jobs \
  --jq '.jobs[] | select(.conclusion=="failure") | .steps[] | select(.conclusion=="failure") | .name'
```

Then find that step's command in `.github/workflows/` and run it locally. If
the run has finished, `gh run view <run-id> --job <job-id> --log-failed` has
the details. Fix the cause in a new commit (don't amend pushed commits), re-run
the § 5 checks it affects, push, and re-arm the monitor.

**Limits.** The no-scope-creep invariant still applies. Fix failures your
change caused, and conflicts with `main`. A failure that is pre-existing
on `main` or flaky (it passes on re-run, or fails the same way on `main`'s
latest run) is not yours to fix: re-run it once (`gh run rerun <run-id>
--failed`), and if it still fails, file a strand and say so in the PR body.
If fixing your own failure would need changes beyond the gated scope, or
three fix attempts haven't turned it green, stop and report to the user.

When everything is green and the PR is mergeable, update the PR body if the
fixes changed what it says (files, testing notes), and add a braid comment
summarizing what CI caught and how you fixed it.

## Repair mode

Repair mode is a single pass over one `autofix` PR whose CI has finished
red or which conflicts with `main`. Fix it once, push, and leave. Don't
start a monitor or wait for the new run, because the next repair pass
looks at it again.

1. In `<repo-root>/.worktrees/autofix`, check out the PR branch and merge
   `origin/main`: `git switch braid/<leaf>` (or `git switch -c
   braid/<leaf> origin/<head>` if the local branch is gone; `<leaf>` is
   the remote head name without its `feature/` or `bugfix/` prefix), then
   `git merge origin/main`. Never rebase or force-push.
2. Resolve conflicts and/or diagnose the failed checks as in §7 ("A
   failing check"). The run has finished, so `--log-failed` works.
3. Apply §7's limits as written: pre-existing or flaky failures get one
   `gh run rerun <run-id> --failed` and a strand, not a fix. Stay inside
   the gated scope. If the workflow lists failures to ignore (failures it
   found on other, unrelated PRs or on `main`), don't fix or rerun them.
   The workflow tracks them separately.
4. Re-run the §5 checks your fix affects, commit, push with the refspec,
   and confirm with `git ls-remote`.
5. Post one PR comment that starts with the attempt marker. K is the
   previous attempt count, which the workflow passes, plus 1:

   ```text
   <!-- q2-autofix repair n=<K> -->
   Authored by <model name> 🤖

   **Autofix repair <K>/3:** <what failed, and what you changed or why you changed nothing>
   ```

6. If the fix needs work outside the gated scope, or this was attempt 3
   and the PR still isn't clean, add the `autofix:needs-human` label
   (create it if needed) and add a braid comment on the strand saying why.

## 8. Report back

Tell the user: the PR link, the behaviour change in plain words, what was
verified (locally and in CI) and what wasn't, what CI or merge conflicts
caught and how you fixed them, and the strand/plan/branch names. Leave the
strand open for the merge.

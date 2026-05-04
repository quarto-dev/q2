---
description: Set up an isolated worktree and triage a GitHub issue into a durable on-branch record. Use when the user says "triage issue #N", "let's look at issue #N", or hands you a github.com/quarto-dev/q2/issues/N URL to investigate.
---

# Triage Skill

This skill takes a GitHub issue from "user pasted a link" to "isolated worktree on its own branch, with a triage record committed to it." It does **not** diagnose the bug or design the fix — that comes after, in whatever workflow the triage outcome calls for (bug fix, doc update, "wai" answer, duplicate close, etc.).

## When to use

User says any of:
- "triage issue #N" / "let's look at issue #N"
- pastes a `github.com/quarto-dev/q2/issues/N` URL and asks you to investigate
- "set up a worktree for issue #N"

**Do not** use for:
- Fixes the user already has scoped (just edit on `main` or an existing branch)
- Internal beads issues without an upstream GH issue (no triage doc needed; the beads description is the record)

## Outcome: three durable artifacts

Every triage produces:

1. **A worktree branch** `issue-<N>` at `.worktrees/issue-<N>/`, with one commit containing the triage record (and any investigative fixtures).
2. **A triage document** at `claude-notes/issue-reports/<N>/triage.md` on that branch.
3. **A beads issue** (only if the triage concludes there is real work to do — see "Outcomes that don't get a beads issue" below).

Investigative artifacts (minimal repros, side-by-side fixtures, comparison outputs) live alongside the triage doc under `claude-notes/issue-reports/<N>/` and are committed with it. They are part of the record, not throwaways.

## Steps

### 1. Pre-flight: verify HEAD is green before anything else

```bash
cargo xtask verify --skip-hub-build
```

This catches "the bug is already there at HEAD" vs. "you introduced it" confusion later, and surfaces environment problems before the user is invested in your worktree. If this fails on a fresh clone, the fix is usually `npm install` from repo root (see "npm install note" below) — re-run verify after.

If `verify` fails for a reason that isn't a fresh-clone npm install, **stop and tell the user.** Do not start the triage on a broken HEAD.

### 2. Read the issue

```bash
gh issue view <N> --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
```

Read the body and every comment. If the issue contains multiple distinct reports (a list of unrelated bugs in one issue is common), confirm with the user which one(s) you're triaging. Capture that scope decision in the triage doc.

### 3. Create the worktree

Branch convention is `issue-<N>` (matches the GH issue number, no prefix; the `bugfix/` prefix only goes on the *remote* branch when the user asks you to push for PR creation).

```bash
git worktree add -b issue-<N> .worktrees/issue-<N> main
```

Then add the beads redirect (the `.beads/` directory already exists from git; just add the `redirect` file):

```bash
echo "../../../.beads" > .worktrees/issue-<N>/.beads/redirect
```

Verify with `br where` from inside the worktree.

### 4. npm install (until `bd-7giz` lands)

Fresh worktrees have no `node_modules/`. `cargo xtask verify` doesn't bootstrap it. Run `npm install` from the worktree root before re-running verify in the worktree:

```bash
cd .worktrees/issue-<N>
npm install
cargo xtask verify --skip-hub-build  # confirm green at branch HEAD
```

When `bd-7giz` (`cargo xtask setup`) lands, replace `npm install` with that command and update this skill.

### 5. Reproduce, investigate, write the triage doc

Create `claude-notes/issue-reports/<N>/` and put inside it:

- `repro.<ext>` — the smallest input that triggers whatever the issue describes (a `.qmd`, a config snippet, a shell script, etc.).
- Any side-by-side comparison fixtures you generate while investigating (see point on diagnosis skills below). Name them descriptively (`exp-prefix.qmd`, `exp-suffix.qmd`, etc.).
- `triage.md` — the doc itself, using the template below.

**For diagnosing the actual bug** (root cause, code locations, fix scope), this skill defers to other skills and the per-crate `CLAUDE.md` files (e.g. `crates/pampa/CLAUDE.md` for the TDD round-trip workflow). Do whatever investigation the issue calls for and capture the conclusions in the triage doc — but the skill itself is silent on *how* to diagnose.

### 6. Triage doc template

```markdown
# Issue #<N> — <one-line headline>

- **GitHub**: https://github.com/quarto-dev/q2/issues/<N>
- **Reporter**: @<login> (<name>), <date>
- **Triage date**: <today>
- **Worktree**: `.worktrees/issue-<N>` (branch `issue-<N>`, based on `main` @ `<short-sha>`)
- **Beads issue**: bd-XXXX (or "none — see Outcome")
- **Scope**: which part(s) of the issue this triage covers, and which it explicitly excludes.

## Summary

One paragraph: what the user reported, whether you reproduced it, and the conclusion in one sentence ("real bug, fix is small", "working as intended, see explanation below", "duplicate of bd-XXXX", etc.).

## Reproduction

Exact commands. Show input and observed-vs-expected output. Reference the fixture under `claude-notes/issue-reports/<N>/`.

## Localization (if applicable)

`file:line` pointers to the code involved. If the bug is "X is missing", note where the analogous working code lives so the fix has a model to copy.

## Open questions — resolved during triage

For each open question raised during investigation, write the question, the experiment that answered it, and the conclusion. **Do not leave forwarded TODOs.** If a question can't be answered in this triage, escalate it to the user before declaring the triage done.

## Outcome / recommended next step

One of:
- "Filed bd-XXXX with fix scope below."
- "Working as intended; see explanation. Will respond on GH."
- "Duplicate of bd-XXXX."
- "Documentation gap; will update `docs/...`."
- "Need more info from reporter; will respond on GH with these questions: ..."

## Verification commands used

The exact `gh`, `cargo`, etc. commands you ran, so a future reader can re-do the investigation.

## Cross-references

- bd-XXXX entries
- related claude-notes/ documents
- relevant `CLAUDE.md` rules
```

### 7. Outcomes that don't get a beads issue

A beads issue is only created when the triage concludes there is concrete work to do in this repo. Skip the beads issue when:

- **Working as intended.** Triage doc explains why, and you (or the user) responds on the GH issue with the explanation.
- **Duplicate.** Triage doc points at the existing bd-XXXX. Optionally comment on the duplicate GH issue.
- **Pure documentation update** that is small enough to do in the same triage session — just do it and skip beads.
- **Need more info from the reporter.** Triage doc captures what you don't yet know; you (or the user) ask the reporter on GH. Revisit when they respond.

When you do file a beads issue:

```bash
br create "<headline> (issue #<N>)" -t bug|task|feature -p <0-4> -d "<description>" --json
```

The description should reference the triage doc path (`claude-notes/issue-reports/<N>/triage.md`) and the worktree branch. If you discovered any incidental work during triage (like `bd-7giz`), file each as its own issue and link with `--deps related:<main-bd-id>`.

### 8. Commit the triage record on the worktree branch

```bash
cd .worktrees/issue-<N>
git add -A
git commit -m "Triage issue #<N>: <one-line summary> (bd-XXXX)"
```

This commit captures the triage doc plus all investigative artifacts (`exp-*.qmd`, comparison outputs, etc.). Do not leave investigative files uncommitted — they are part of the record.

If a fix follows in the same session, that fix is a separate commit on the same branch.

### 9. Beads JSONL changes go on `main`, not the worktree branch

Per `.claude/rules/worktrees.md`: with the redirect active, `br create` writes to the main repo's `.beads/issues.jsonl`. That JSONL change is **not** visible from the worktree's `git status` and **must be committed from the main repo**, not from the worktree branch.

When you're ready to commit the beads change, do it from the main repo separately, after the worktree branch is pushed (or merged):

```bash
cd /path/to/main/repo  # not the worktree
git add .beads/
git commit -m "sync beads (bd-XXXX, ...)"
```

This is awkward — the PR reviewer can't see the beads context until after the JSONL is committed to main — but it is the current rule. (Open question: revisit this convention with the team. Tracked informally for now.)

### 10. Pushing for PR

When the user asks for a PR, push the local `issue-<N>` branch to a remote `bugfix/issue-<N>` branch (or `feature/issue-<N>` etc., matching the work):

```bash
git push -u origin issue-<N>:bugfix/issue-<N>
```

The local branch name stays bare; only the remote uses the prefix.

## Anti-patterns

- **Skipping pre-flight verify.** "I'll just start" hides bootstrap problems and pre-existing failures inside the triage.
- **Putting investigative artifacts in `/tmp` or untracked paths.** They are part of the durable record.
- **Forcing a beads issue when the outcome is "working as intended" or "duplicate".** A triage doc is enough; a stub beads issue is noise.
- **Committing `.beads/issues.jsonl` from a worktree branch.** It belongs on `main`.
- **Silent investigations.** If you spend more than a few minutes exploring without finding the answer, surface what you've tried in the triage doc as evidence — even unanswered. The doc is a record of effort, not just conclusions.

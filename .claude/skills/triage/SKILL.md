---
name: triage
description: Set up an isolated worktree and triage a GitHub issue into a durable on-branch record. Use when the user says "triage issue #N", "let's look at issue #N", or pastes a github.com/quarto-dev/q2/issues/N URL to investigate.
---

# Triage Skill

Takes a GitHub issue from "user pasted a link" to "isolated worktree on its own branch, with a triage record committed to it." Does **not** diagnose the bug or design the fix — that comes after, in whatever workflow the triage outcome calls for (bug fix, doc update, "wai" answer, duplicate close, etc.).

## When to use

User says any of:
- "triage issue #N" / "let's look at issue #N"
- pastes a `github.com/quarto-dev/q2/issues/N` URL and asks you to investigate
- "set up a worktree for issue #N"

**Do not** use for:
- Fixes the user already has scoped (just edit on `main` or an existing branch)
- Internal beads issues without an upstream GH issue (no triage doc needed; the beads description is the record)

## Outcome: three durable artifacts

1. A worktree branch `issue-<N>` at `.worktrees/issue-<N>/`, with one commit containing the triage record (and any investigative fixtures).
2. A triage document at `claude-notes/issue-reports/<N>/triage.md` on that branch.
3. A beads issue (only if the triage concludes there is real work to do — see § Outcomes that don't get a beads issue).

Investigative artifacts (minimal repros, side-by-side fixtures, comparison outputs) live alongside the triage doc under `claude-notes/issue-reports/<N>/` and are committed with it. They are part of the record, not throwaways.

## Steps

### 1. Pre-flight: verify HEAD is green

```bash
cargo xtask verify --skip-hub-build
```

Catches "the bug is already there at HEAD" vs. "you introduced it" confusion later, and surfaces environment problems before the user is invested. If `verify` fails for a non-bootstrap reason, **stop and tell the user.** Do not start the triage on a broken HEAD. For fresh-clone bootstrap, see `.claude/rules/worktrees.md` § Fresh worktree bootstrap.

### 2. Read the issue

```bash
gh issue view <N> --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
```

Read the body and every comment. If the issue contains multiple distinct reports (a list of unrelated bugs in one issue is common), confirm with the user which one(s) you're triaging. Capture that scope decision in the triage doc.

### 3. Create the worktree

Branch + directory naming follows `.claude/rules/worktrees.md` § Branch naming (`issue-<N>` for triage). Beads redirect setup follows § Beads Redirect.

```bash
git worktree add -b issue-<N> .worktrees/issue-<N> main
echo "../../../.beads" > .worktrees/issue-<N>/.beads/redirect
```

Verify with `br where` from inside the worktree.

### 4. Bootstrap the worktree

See `.claude/rules/worktrees.md` § Fresh worktree bootstrap. Then re-run `cargo xtask verify --skip-hub-build` from inside the worktree to confirm green at branch HEAD.

### 5. Reproduce, investigate, write the triage doc

Create `claude-notes/issue-reports/<N>/` and put inside it:

- `repro.<ext>` — the smallest input that triggers whatever the issue describes (a `.qmd`, a config snippet, a shell script, etc.).
- Any side-by-side comparison fixtures you generate while investigating. Name them descriptively (`exp-prefix.qmd`, `exp-suffix.qmd`, etc.).
- `triage.md` — see `references/triage-doc-template.md` for the template.

For diagnosing the actual bug (root cause, code locations, fix scope), this skill defers to the per-crate `CLAUDE.md` files (e.g. `crates/pampa/CLAUDE.md` for the TDD round-trip workflow). Do whatever investigation the issue calls for and capture the conclusions in the triage doc — but the skill itself is silent on *how* to diagnose.

### 6. Outcomes that don't get a beads issue

A beads issue is only created when the triage concludes there is concrete work to do in this repo. Skip the beads issue when:

- **Working as intended.** Triage doc explains why, and you (or the user) responds on the GH issue with the explanation.
- **Duplicate.** Triage doc points at the existing bd-XXXX. Optionally comment on the duplicate GH issue.
- **Pure documentation update** small enough to do in the same triage session — just do it and skip beads.
- **Need more info from the reporter.** Triage doc captures what you don't yet know; you (or the user) ask the reporter on GH. Revisit when they respond.

When you do file a beads issue:

```bash
br create "<headline> (issue #<N>)" -t bug|task|feature -p <0-4> -d "<description>" --json
```

The description should reference the triage doc path and the worktree branch. If you discovered any incidental work during triage, file each as its own issue and link with `--deps related:<main-bd-id>`.

### 7. Commit the triage record on the worktree branch

```bash
cd .worktrees/issue-<N>
git add -A
git commit -m "Triage issue #<N>: <one-line summary> (bd-XXXX)"
```

Captures the triage doc plus all investigative artifacts. Do not leave investigative files uncommitted — they are part of the record. If a fix follows in the same session, that fix is a separate commit on the same branch.

### 8. Beads JSONL changes go on `main`

See `.claude/rules/worktrees.md` § Committing beads changes.

### 9. Pushing for PR

See `.claude/rules/worktrees.md` § Pushing for PR. Local branch stays bare (`issue-<N>`); remote uses a prefix reflecting the work type.

## Anti-patterns

- **Skipping pre-flight verify.** Hides bootstrap problems and pre-existing failures inside the triage.
- **Putting investigative artifacts in `/tmp` or untracked paths.** They are part of the durable record.
- **Forcing a beads issue when the outcome is "working as intended" or "duplicate".** A triage doc is enough; a stub beads issue is noise.
- **Committing `.beads/issues.jsonl` from a worktree branch.** It belongs on `main`.
- **Silent investigations.** If you spend more than a few minutes exploring without finding the answer, surface what you've tried in the triage doc as evidence — even unanswered. The doc is a record of effort, not just conclusions.

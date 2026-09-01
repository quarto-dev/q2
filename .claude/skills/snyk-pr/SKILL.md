---
name: snyk-pr
description: Remediate a Snyk bot PR ([Snyk] Upgrade ..., author posit-snyk-bot, branch snyk-upgrade-*) so it goes green on CI and becomes mergeable. Use when the user asks to fix, unblock, or "make mergeable" a Snyk PR, mentions a red Snyk upgrade PR, or pastes a snyk-upgrade-* branch name. Invoke as /snyk-pr <PR#>; with no argument, list open posit-snyk-bot PRs and stop. Fixes the branch only — never weakens or skips the CI guard tests.
---

# snyk-pr: remediate a Snyk bot PR

Snyk integration is mandatory for this org, and its bot PRs recur. The bot
upgrades a dependency in exactly one `package.json` + lockfile pair, but this
repo deliberately couples dependency versions across multiple surfaces and
enforces the coupling with guard tests. The bot's PRs therefore arrive red
**by design** — the CI system is working. This skill fixes the *branch*, never
the checks.

## Invariants (read first)

- **Never weaken, skip, or `#[ignore]` a guard test.** If a guard fails, more
  copies of the version need bumping — find them.
- **Exact pins, no carets.** Guard tests require exact versions where the repo
  pins exactly; Snyk sometimes writes `^X.Y.Z` into lockfile mirrors — normalize
  it away (see the katex reference).
- **`npm install` only from the repo root** (npm workspaces), never from
  `hub-client/`. Sub-project installs (e.g. `quarto-hub-sandboxed-preview`) are
  driven by their own scripts (`npm run build:sandboxed`).
- **Push only with explicit user approval** (GIT PUSH POLICY in CLAUDE.md).
  Merging the PR is always a human decision.
- **hub-client changes require the changelog two-commit workflow** (CLAUDE.md,
  "hub-client Commit Instructions"). A regenerated committed bundle under
  `hub-client/` counts.

## Workflow

### 0. No argument? List and stop.

```bash
gh pr list --repo quarto-dev/q2 --author posit-snyk-bot --state open \
  --json number,title,headRefName
```

Report the list and stop — remediation needs a specific PR number.

### 1. Orient remotely — extract the *actual* failure

```bash
gh pr view <N> --repo quarto-dev/q2 --json title,headRefName,state,body
gh pr checks <N> --repo quarto-dev/q2
```

For each failing job, pull the log and find the real failing test — do not
assume it is the usual guard:

```bash
gh run view --repo quarto-dev/q2 --job <job-id> --log-failed 2>&1 | \
  sed 's/\x1b\[[0-9;]*m//g' | grep -B2 -A20 "FAIL \["
```

Known signatures:
- `katex_cdn_version_matches_npm_pin` (quarto-core) → the katex case; follow
  `references/katex.md`.
- Merge-conflict / "This branch has conflicts" on a paired package (react +
  react-dom) → follow `references/paired-packages.md`.
- Anything else → treat as a new package playbook: follow the generic steps
  below, and **add a new `references/<package>.md` before closing out** so the
  next occurrence is mechanical.

### 2. Fetch the bot branch and work on it directly

The bot pushes branches to quarto-dev/q2 itself (not a fork), so fix commits
go on the same branch. Use a worktree to avoid disturbing the main checkout:

```bash
git fetch origin <snyk-upgrade-...>
git worktree add .worktrees/snyk-pr-<N> <snyk-upgrade-...>
cd .worktrees/snyk-pr-<N>
```

(Plain `git switch` in the main checkout is acceptable if it is clean and the
user agrees.)

### 3. Merge `origin/main` first

Snyk branches are cut from stale mains — routinely weeks old. Merge before
touching anything else so you fix against current reality:

```bash
git fetch origin main
git merge origin/main
```

Resolve conflicts; for paired-package conflicts see
`references/paired-packages.md`. On a `.braid/snapshot.jsonl` conflict,
regenerate from the skein (`braid export > .braid/snapshot.jsonl`) — never
hand-merge.

### 4. Enumerate every copy of the version

The guard tests may lag reality (the committed sandbox bundle was invisible to
them — PR #571 missed it, PR #573 repaired it). Grep for *all* consumers of the
version, including committed build artifacts:

```bash
grep -rn '<pkg>@\|"<pkg>":' \
  --include='*.json' --include='*.rs' --include='*.ts' --include='*.tsx' \
  --include='*.html' . 2>/dev/null | \
  grep -v node_modules | grep -v target | grep -v '\.worktrees'
# then grep committed artifacts for the OLD version string itself:
grep -rn '<old-version>' hub-client/public/ resources/ 2>/dev/null | head
```

Every hit must end up naming the new version (or be justified as unrelated).

### 5. Apply the package playbook

- katex → `references/katex.md` (four coupled surfaces + committed bundle).
- react/react-dom → `references/paired-packages.md`.
- unknown package → bump all copies found in step 4; run a root `npm install`;
  rebuild any committed artifact that embeds the package.

### 6. Verify

Scale to what changed, per CLAUDE.md:

```bash
# targeted guard first (fast signal)
cargo nextest run -p quarto-core -E 'test(katex_cdn_version_matches_npm_pin)'
# Rust files changed => workspace build + tests
cargo build --workspace && cargo nextest run --workspace
# hub-client touched => production build (stricter than tsc --noEmit)
cd hub-client && npm run build:all
# hub-client touched => CSS lint (a CI gate that neither test:ci nor
# cargo xtask verify runs — it red-flagged main for days unnoticed)
npm run lint:css
# dirty-tree trap: a fresh root install must not modify tracked files
npm install && git status --porcelain
```

`git status` must come back clean after the root install — if it dirties a
lockfile, a caret or pin mismatch survived (step 5 incomplete).

**Attribute failures before reacting to them.** After merging main, the branch
inherits any breakage main already has. When a verification leg fails, run the
same test file against `origin/main` (main checkout) and check main's recent
workflow runs (`gh run list --branch main`) before assuming your change caused
it. A failure that reproduces identically on main is pre-existing: file it as a
braid strand (`discovered-from` the current work), document it in the fix
commit's message, and proceed — the bar for the Snyk branch is "as green as
main", not "greener than main". (Seen 2026-09-01: 29 hub-client vitest
failures on the #637 branch were main's own mock drift, unrelated to katex.)

**Also run out-of-gate suites sensitive to the upgraded package.** Some test
tiers are not in the CI merge gate (see the ci-test-suite-unwired lint's known
gaps, e.g. `preview-renderer`'s `test:integration`). If the upgraded package
can affect one, run it from the remediation branch — it can confirm the
upgrade *fixes* known breakage (katex 0.18.4 fixed the `.katex-tag` strands)
or catch a regression CI would miss.

### 7. Commit (+ changelog if hub-client changed)

Commit the alignment with a message that records **which copies were bumped
and why** (see `ccaa8cc9` for the model). If any `hub-client/` file changed,
follow the two-commit changelog workflow: commit the change, then add the
`hub-client/changelog.md` entry referencing that commit's hash in a second
commit.

When a regenerated committed bundle is part of the diff, report its byte delta:
a pure version bump should change only version strings; a large delta means the
bundle was already stale — call that out explicitly.

### 8. Push with approval, watch CI, report

Ask the user for permission to push (GIT PUSH POLICY). Then:

```bash
git push origin HEAD:<snyk-upgrade-...>
gh pr checks <N> --repo quarto-dev/q2 --watch
```

Report the final check status. Do not merge; offer the green PR back to the
user. Clean up the worktree (`git worktree remove .worktrees/snyk-pr-<N>`).

### 9. Close the loop

If anything new was learned (a new package, a new failure signature, a new
committed artifact), fold it into this skill / a new reference file in the same
session, and file discovered work as braid strands linked
`discovered-from:<the strand you're working under>`.

## History (why each rule exists)

- #471, #571, #634 (katex): only-one-copy-bumped guard failures; #571's fix
  missed the committed sandbox bundle → follow-up PR #573. Playbook commit:
  `ccaa8cc9`.
- #511/#512 (react/react-dom): paired PRs conflicted with each other;
  resolution merged main and took both new versions (`91977474`).
- #637 (katex 0.18.2→0.18.4): validation target for this skill; same guard
  signature, branch predated the doc-branching merge.

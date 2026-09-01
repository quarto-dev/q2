# Snyk PR remediation skill

**Braid strand:** bd-t8bwkr64
**Immediate motivation:** PR #637 ([Snyk] Upgrade katex from 0.18.2 to 0.18.4) is red on CI with the same failure signature as every previous katex Snyk PR.

## Overview

Snyk integration is mandatory for the org and its bot PRs recur (5 merged so far:
katex #471, #571, #634; react #511; react-dom #512; #637 open now). Every katex
PR arrives red because Snyk edits exactly one file pair —
`hub-client/quarto-hub-sandboxed-preview/package.json` + its lockfile — while the
repo deliberately couples the KaTeX version across four surfaces, enforced by the
guard test `katex_cdn_version_matches_npm_pin`
(`crates/quarto-core/src/stage/stages/math_js.rs:1021`, bd-4b7f1hr7). The CI
system is working as designed; the goal is **not** to weaken it but to codify the
remediation workflow as a reusable skill (`/snyk-pr <PR#>`), so future bot PRs
can be made mergeable in one invocation.

This plan: create the skill, validate it by remediating #637 with it, and file
the small pieces of discovered work.

## What past remediations actually did (evidence)

Studied commits, most instructive first:

- **#634 (katex 0.18.1→0.18.2)** — `32ca70fa` (merge origin/main; snyk branches
  are cut from stale mains), `ccaa8cc9` (the full playbook, see below),
  `2ff292ec` (hub-client changelog two-commit workflow).
- **#571 (0.18.0→0.18.1)** — `3642d362` bumped root pin + CDN URL only. It
  **missed the committed sandbox bundle**, which had to be repaired in follow-up
  PR #573 ("refresh stale q2-sandboxed-preview bundle"). The skill exists to
  prevent exactly this class of partial fix.
- **#471 (0.17.0→0.18.0)** — `c0958658`, same shape as #571's fix.
- **#511/#512 (react, react-dom 19.2.7→19.2.8)** — paired-package upgrades
  arrive as **two PRs that conflict with each other**; after one merges, the
  other needs `origin/main` merged in and the version conflicts resolved by
  taking the new version of both packages (`91977474`).

### The katex playbook (from `ccaa8cc9`, verified against the current tree)

Four surfaces must name one exact version:

1. Root `package.json` (`"katex": "X.Y.Z"`, exact pin — no caret) + root
   `package-lock.json`. Bump via `npm install katex@X.Y.Z --save-exact` from the
   repo root (npm-workspaces root; never `npm install` inside hub-client).
2. `hub-client/quarto-hub-sandboxed-preview/package.json` + its lockfile.
   Snyk bumps this pair, **but writes `^X.Y.Z` into the lockfile's root
   dependency mirror while the package.json says `X.Y.Z`**. hub-client's
   postinstall runs `npm install` in that sub-project, which rewrites the caret
   away — merging without normalizing produces a dirty tree on every
   colleague's next install. Fix: run `npm install` in the sub-project (or
   `npm run build:sandboxed` from hub-client, which does it) and commit the
   lockfile delta.
3. `DEFAULT_KATEX_URL_BASE` in `crates/quarto-core/src/stage/stages/math_js.rs:85`
   (`https://cdn.jsdelivr.net/npm/katex@X.Y.Z/dist/`).
4. `hub-client/public/q2-sandboxed-preview.html` — a **committed** ~1.8 MB
   single-file bundle with KaTeX inlined. The guard test covers the three
   version *declarations*, not the bundled bytes, and there is no
   `git diff --exit-code` freshness gate for this artifact (unlike
   quarto-engine-host-deno). Regenerate with `cd hub-client && npm run
   build:sandboxed`; the rebuild is deterministic. Inspect the diff — for a
   pure version bump it should be ~2 bytes of version string; a larger delta
   means the bundle was already stale (as with #573) and deserves a callout in
   the commit message.

Verification for the katex case:
- `cargo nextest run -p quarto-core -E 'test(katex_cdn_version_matches_npm_pin)'`
- `grep -o "0\.18\.[0-9]" hub-client/public/q2-sandboxed-preview.html | sort | uniq -c`
  (all occurrences on the new version)
- `git status` clean after a fresh root `npm install` (dirty-tree trap, item 2)
- `cd hub-client && npm run build:all` (hub-client CI-strictness rule)

### Generic workflow shape (package-agnostic)

1. **Orient remotely**: `gh pr view N --json ...`, `gh pr checks N`, pull the
   failed job log and extract the failing test(s) — do not assume; #637's
   failure was confirmed as the guard test
   (`left: "0.18.2"` / `right: "0.18.4"`), but a future PR could fail
   differently.
2. **Fetch the bot branch** (`git fetch origin <snyk-upgrade-...>`); work on it
   directly — the bot pushes to the main repo, not a fork, so maintainers can
   push fix commits to the same branch (every prior remediation did).
   Use a worktree (`git worktree add`) to avoid disturbing the main checkout.
3. **Merge `origin/main`** into the branch first; snyk branches are routinely
   stale (the #637 branch predates the doc-branching merge `132c3ad3`). For
   paired-package PRs, expect and resolve version conflicts toward the new
   versions.
4. **Find every copy of the version**, not just the reported file:
   `grep -rn "<pkg>@\|\"<pkg>\":" --include='*.json' --include='*.rs'
   --include='*.ts' --include='*.html' .` (excluding node_modules/target),
   plus committed build artifacts embedding the package. This mirrors the
   path-resolution lesson: enumerate *consumers* of the version, per-package
   guard tests may lag reality.
5. **Apply the package playbook** (katex above; react/react-dom pairing; for an
   unknown package, the grep in step 4 + workspace build is the playbook).
6. **Verify**: targeted guard test, `cargo build --workspace` +
   `cargo nextest run --workspace` when Rust files changed,
   `cd hub-client && npm run build:all` when hub-client changed, per
   CLAUDE.md's pre-push checklist.
7. **Changelog**: if `hub-client/` files changed, the two-commit changelog
   workflow applies (see #634's `2ff292ec`).
8. **Push only with explicit user approval** (GIT PUSH POLICY), then watch
   `gh pr checks N --watch` and report. Merging stays a human decision.

## Skill design

- Location: `.claude/skills/snyk-pr/SKILL.md` + symlink
  `.agents/skills/snyk-pr -> ../../.claude/skills/snyk-pr` (matching the eight
  existing skills).
- Invocation: `/snyk-pr 637` (PR number required; with no argument, list open
  `posit-snyk-bot` PRs and stop).
- Structure: SKILL.md carries the generic workflow (steps 1–8 above);
  `references/katex.md` carries the katex four-surface playbook and
  `references/paired-packages.md` the react/react-dom pattern, so the
  main skill stays short and new per-package playbooks accrete as references.
- The skill must state the invariants explicitly: never weaken or skip the
  guard tests; never `npm install` from hub-client; exact pins (no carets);
  push requires user approval.
- Description/trigger phrases: "snyk PR", "posit-snyk-bot", "[Snyk] Upgrade",
  "snyk-upgrade-" branch names, "make the snyk PR mergeable".

## Work items

### Phase 1 — skill

- [x] Write `.claude/skills/snyk-pr/SKILL.md` (generic workflow) +
      `references/katex.md` + `references/paired-packages.md`; add the
      `.agents/skills/` symlink
- [x] Commit the skill (repo artifact, so colleagues' sessions get it)

### Phase 2 — validate on PR #637 (dogfood)

- [x] Follow the skill end-to-end in `.worktrees/snyk-pr-637`: merged
      origin/main (clean), bumped root pin + lockfile, bumped
      `DEFAULT_KATEX_URL_BASE` to 0.18.4, normalized the sub-project
      lockfile caret, regenerated `q2-sandboxed-preview.html`
      (~19-line delta — real 0.18.3/0.18.4 KaTeX code changes plus minifier
      renumbering, unlike the 2-byte 0.18.2 bump; lockfile diffs katex-only).
      Commits on the branch: `e4a72819` (alignment), `8d584e04` (changelog).
- [x] Verification battery: guard test passes; `cargo nextest run
      --workspace` 13483/13483; ts-packages + hub-client `build:all` green;
      fresh root `npm install` leaves the tree clean. hub-client `test:ci`
      fails in 2 files (29 tests) — **verified identical on origin/main**
      (TS Test Suite + E2E workflows are red on main at 9fc7c176);
      pre-existing, filed as bd-v51cly8i, unrelated to katex.
- [x] hub-client changelog two-commit update (`8d584e04`; `npm run
      test:wasm` gate passes)
- [ ] Ask for push approval; push; confirm CI on #637 matches main's
      status (Test Suite green; TS suite red only with main's own
      pre-existing failures, bd-v51cly8i)
- [x] Fold learnings back into the skill: failure-attribution rule
      (compare against origin/main before blaming the upgrade) and
      out-of-gate suite check (preview-renderer `test:integration`)

### Phase 3 — discovered work

- [x] Triaged the three duplicate `.katex-tag` strands: bd-s36g9dav is
      canonical; bd-kn7ln981 and bd-6uyw7w2o marked `duplicates` of it.
      **Verified on the #637 branch that katex 0.18.4 fixes the failure**:
      preview-renderer `test:integration` fully green (632 passed / 1
      skipped; the Equation `\tag` tests 4/4). Close bd-s36g9dav once #637
      merges.
- [x] Filed bd-r3utxrdj: freshness gate for the committed
      `q2-sandboxed-preview.html` bundle (the #571/#573 incident class).
- [x] Filed bd-v51cly8i (discovered during Phase 2): TS Test Suite red on
      main — `useAutomergeSync.test.ts` mock missing `vfsAddFile` (28
      tests) + one BranchBar test; likely from the doc-branching work
      (132c3ad3).

## Facts for quick reference

- Guard test: `katex_cdn_version_matches_npm_pin`,
  `crates/quarto-core/src/stage/stages/math_js.rs:1021` (checks root pin ==
  sandboxed pin, exact-version format, and `DEFAULT_KATEX_URL_BASE` ==
  `https://cdn.jsdelivr.net/npm/katex@{root}/dist/`).
- Current pins (main, 2026-09-01): root and sandboxed both `0.18.2`; #637 wants
  `0.18.4`; the committed bundle embeds two `0.18.2` strings.
- Snyk branch naming: `snyk-upgrade-<hash>`; author `posit-snyk-bot`; branches
  live in quarto-dev/q2 itself (pushable).
- Snyk's own checks (`security/snyk`, `license/snyk`) pass on the bot PRs; the
  red comes from our test suite, which is the intended behavior.

---
description: Survey the workspace for available Rust dependency upgrades, apply patch/minor bumps in an isolated worktree, run full `cargo xtask verify`, and produce a plan doc + per-major braid strands so the user can review and merge. Use when the user says "upgrade cargo deps", "check for dependency upgrades", "do the bi-weekly cargo upgrade", or asks about outdated dependencies. Runs on demand only — there is no schedule.
---

# upgrade-cargo-deps Skill

This skill turns the repetitive "are any of our Rust deps behind?" task into a single review-friendly artifact. It does **not** push, open PRs, or make decisions about major upgrades. It produces:

1. A worktree branch with patch/minor upgrades applied and verified.
2. A plan doc at `claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md` summarizing what happened.
3. One braid strand per available major upgrade, linked from the plan.

The user merges the worktree branch when satisfied, triages the strands at their own cadence, and (optionally) closes the survey plan.

## When to use

- User says "upgrade cargo deps", "run the cargo upgrade survey", "check our Rust dependencies", "do the bi-weekly upgrade".
- The most recent `claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md` is older than ~2 weeks and the user asks for a fresh survey.
- User pastes `cargo update --dry-run` output and asks "what should we do about this".

**Suggested cadence:** bi-weekly. There is no scheduled trigger; the user runs it.

**Do not** use for:
- npm / hub-client dependency upgrades (out of scope for v1).
- Rust toolchain upgrades (`rust-toolchain.toml`).
- `cargo audit` / security-advisory work.
- Single-dependency upgrades the user has already decided to do.

## Why a worktree (not the main checkout)

**All work happens in `.worktrees/cargo-upgrade-YYYY-MM-DD/`, never in the main checkout.** Other Claude agents (or the user) may be working on the same repo concurrently. Running `cargo update` and a full `cargo xtask verify` in the main checkout would:

- Race with another agent's edits to `Cargo.lock` or `Cargo.toml`.
- Tie up `target/` and the test runner for ~10+ minutes during verification.
- Risk leaving the user's working copy in a half-applied state if the skill is interrupted.

The worktree gives this skill its own checkout, its own branch, and its own `target/` — fully isolated from anything else in flight. The user merges the branch when ready; until then, nothing the skill does touches their working tree. Steps 1 (pre-flight verify) and 2 (dry-run survey) are the only read-only operations that run in the main checkout, and they don't write any files.

## Outcome: three durable artifacts

1. **Worktree branch** `cargo-upgrade-YYYY-MM-DD` at `.worktrees/cargo-upgrade-YYYY-MM-DD/` containing the applied `Cargo.lock` change (and any `Cargo.toml` widenings — see "Major upgrades" below; for v1 the answer is *none*) plus a verified test/build run.
2. **Plan doc** at `claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md` with three sections: Applied & verified / Needs review (majors) / Skipped (vendored or excluded).
3. **One braid strand per major upgrade**, type `chore`, priority `3`, linked from the plan.

If verification fails, the skill stops, leaves the worktree intact, and reports the failure so the user can investigate.

## Steps

### 1. Pre-flight: verify HEAD is green

Run from the main repo root (this is the only build/test command that runs outside the worktree):

```bash
cargo xtask verify --skip-hub-build
```

Same rationale as the other skills: catches "broken at HEAD" vs. "this skill broke it" confusion later. If it fails for a non-bootstrap reason, **stop and tell the user.** Don't survey on a broken HEAD.

If another agent appears to be actively editing the main checkout (e.g. `git status` shows uncommitted changes you didn't make), pause and tell the user before proceeding — the worktree will branch from `main`, but a dirty main checkout often means coordination is needed first.

### 2. Survey: what's available?

Run the dry-run survey from the **main repo root** (not a worktree — surveying is read-only):

```bash
cargo update --dry-run --workspace --verbose
```

The output has two important shapes:

- `Locking N packages to latest compatible versions` — patch/minor bumps available within current semver ranges. These are what `cargo update` would apply.
- `Unchanged <crate> v<current> (available: v<latest>)` — a newer version exists but is **outside** the currently-declared semver range. These are the major-bump (or sometimes minor-but-out-of-range) candidates.

Capture the full verbose output — you'll quote it in the plan doc.

Also capture the duplicates baseline:

```bash
cargo tree --duplicates --workspace --depth 0
```

Count the number of duplicate-version entries — each crate listed twice (or more) counts as one duplicate. You'll compare this before/after.

### 3. Classify the "Unchanged" entries

For each `Unchanged X v<a.b.c> (available: v<x.y.z>)` line, classify by comparing the version pair into two buckets:

**Bucket A — Breaking (file individual braid strands in step 11):**
- **Major** (`a.b.c → x.y.z`, x > a, a ≥ 1) — semver-breaking.
- **Pre-1.0 minor** (`0.b.c → 0.y.z`, y > b) — semantically breaking in Cargo's resolver.

**Bucket B — Non-breaking, out-of-range (list in plan, no individual strands):**
- **Patch out-of-range** (`a.b.c → a.b.z`, z > c).
- **Minor out-of-range** (`a.b.c → a.y.z`, y > b, a ≥ 1).
- **Pre-1.0 patch** (`0.b.c → 0.b.z`, z > c).
- **Pre-release transitions** (e.g. `0.6.0-pre.1 → 0.6.0-pre.2`).

The Bucket B entries appear because either the workspace declares a narrower range than upstream is at, or a transitive constraint pins us back. They're non-breaking. Filing 20+ strands for tiny patch deltas like `libc 0.2.185 → 0.2.186` would be noise. The plan doc lists them under "Surfaced but not filed (patch/minor out-of-range)" for reference; the user can opt to widen workspace ranges in a follow-up.

For v1 the skill **does not** edit `Cargo.toml` to widen ranges, even for Bucket B. Bucket A only gets strands; no `Cargo.toml` changes.

### 4. Identify excluded / vendored / pinned crates

Don't propose changes for these — they're either upstream vendored, workspace-excluded, or deliberately pinned:

- **Read `.claude/skills/upgrade-cargo-deps/PINS.md`.** That doc is the authoritative list of every deliberate pin and every known transitive incompatibility, with a written removal condition for each. For every entry:
  1. List it under "Skipped (pinned)" in the survey plan with a one-line "why" + a pointer to PINS.md.
  2. **Re-evaluate the removal condition** against the current state of the repo and the upstream registry. If the condition has been met (e.g. the upstream crate is no longer reverse-deped on; a vendored patch can be re-vendored fresh; a transitive incompat has been fixed by another upgrade landing), call this out at the top of the survey plan as **"Pin can now be removed: <name>"** so the user can land a separate cleanup PR. Update the entry's "Last reviewed" date in PINS.md to today's date as part of the survey worktree's first commit.

- **`crates/wasm-bindgen-futures-patch/`** — vendored upstream `wasm-bindgen-futures` crate. See PINS.md for the full pin chain (`wasm-bindgen-futures = "=0.4.58"` → transitive `wasm-bindgen = "=0.2.108"`, `js-sys = "=0.3.85"`).

- **Workspace-excluded crates** (per root `Cargo.toml`): `wasm-quarto-hub-client`, `wasm-qmd-parser`, `tree-sitter-language-wasm-shim`, `pampa/fuzz`, `crates/experiments/*` (other than reconcile-viewer). Their `Cargo.lock` entries still come from the workspace lock, so `cargo update` covers them, but their `Cargo.toml` dep ranges aren't part of `--workspace` resolution for direct edits.

If a major-upgrade candidate's only consumer is one of the vendored/pinned crates, list it under "Skipped" with the reason; don't file a strand.

### 5. Create the worktree (skip if already inside it)

**First, check if you're already in the right worktree.** A `CLAUDE.local.md` whose `**Task:**` line says `Cargo dependency upgrade — YYYY-MM-DD` for today's date means the worktree exists and you're in it — skip to step 6. Re-running `cargo xtask create-worktree --upgrade` from there would fail (`git worktree add` errors on existing directories).

If you're in the main checkout or a different worktree, create it now:

```bash
cargo xtask create-worktree --upgrade
# Creates a cargo-upgrade-YYYY-MM-DD worktree with CLAUDE.local.md.
# Fallback for fresh clones where the xtask is not yet built:
# see .claude/rules/worktrees.md § Manual bootstrap.
```

The worktree resolves the braid skein automatically — no per-worktree setup.

### 6. Bootstrap the worktree (conditional)

Skip this step if step 7 will be a no-op. Concretely: if step 2's dry-run output started with `Locking 0 packages`, you won't run a worktree-side verify in step 8, so `node_modules/` isn't needed.

Otherwise — fresh worktrees have no `node_modules/`, and `cargo xtask verify` (run in step 8) needs hub-client deps:

```bash
cd .worktrees/cargo-upgrade-$DATE
npm install
```

When `bd-7giz` (`cargo xtask setup`) lands, replace `npm install` with that and update this skill.

### 7. Apply patch/minor upgrades

From inside the worktree:

```bash
cargo update --workspace
```

This rewrites `Cargo.lock` with all in-range upgrades. Stage it but don't commit yet — verification comes first.

If `cargo update` reports "Locking 0 packages" (i.e. nothing in range to upgrade), there's nothing to apply. **Skip steps 8–10 (verify, post-state duplicates, lockfile commit)** — pre-flight in step 1 already validated `main`, and the worktree branches from `main` with an identical lockfile, so re-running `cargo xtask verify` in the worktree confirms only what step 1 already confirmed. Add a note in the plan that the lockfile was already current; still file strands for any Bucket A upgrades from step 3.

### 8. Verify

Run the **full** verification — slower is fine, the value is full output if anything fails:

```bash
cargo xtask verify
```

This runs `cargo build --workspace`, `cargo nextest run --workspace`, the hub-client build, and hub-client tests.

**On failure**: do not commit the lockfile change. Run `git restore Cargo.lock` to revert, leave the worktree in place for diagnosis, and report:

- The full failing command output (or a path to a captured log).
- Which upgrades the lockfile would have applied (from step 2's "Locking" output).
- A recommendation: usually "isolate the offender by `cargo update -p <crate>` one at a time and re-run verify."

In this state, don't file the survey plan yet — escalate to the user. The next session can either land the safe subset or open a strand per failing dep.

### 9. Capture the post-state duplicates

```bash
cargo tree --duplicates --workspace --depth 0
```

Compare the count to the baseline from step 2. Surface any **new** duplicates introduced by the upgrade as a yellow flag in the plan doc.

### 10. Commit the lockfile

```bash
git add Cargo.lock
git commit -m "$(cat <<'EOF'
cargo update: apply in-range upgrades (YYYY-MM-DD survey)

See claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

(Replace `YYYY-MM-DD` with the actual date.)

### 11. File braid strands for breaking upgrades

For each **Bucket A** (Major / pre-1.0 minor) candidate from step 3, file a `chore` strand. Bucket B (out-of-range patch/minor) entries do **not** get individual strands — they're listed in the plan doc only. Run from inside the worktree (the worktree resolves the shared skein automatically):

```bash
braid create "Cargo: upgrade <crate> v<a.b.c> → v<x.y.z>" \
  -t chore -p 3 -l deps -l cargo \
  -d "Major upgrade surfaced by cargo-upgrade survey YYYY-MM-DD. Current version <a.b.c> is range-pinned in workspace; latest is <x.y.z>. Review changelog and bump deliberately. See claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md."
```

braid prints each new strand id on stdout; capture them — you'll list them in the plan.

### 12. Write the plan doc

Create `claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md` using the template below. Commit it to the worktree branch.

```markdown
# Cargo dependency upgrade survey — YYYY-MM-DD

**Worktree:** `.worktrees/cargo-upgrade-YYYY-MM-DD` (branch `cargo-upgrade-YYYY-MM-DD`, based on `main` @ `<short-sha>`)
**Skill:** `.claude/skills/upgrade-cargo-deps/SKILL.md`
**Previous survey:** `<link to prior plan, or "none">`

## TL;DR

- Applied: N patch/minor upgrades via `cargo update` (commit `<hash>`).
- Needs review: M major upgrades (strands: bd-XXXX, bd-YYYY, …).
- Skipped: K (vendored / excluded — see below).
- Duplicates: <before> → <after> (delta: <±N>).
- Verification: `cargo xtask verify` <PASSED | FAILED> — <one-line summary>.

## Applied & verified

Patch/minor upgrades applied in `cargo update`:

| Crate | Before | After |
|---|---|---|
| <name> | <ver> | <ver> |
| … | | |

Verification: full `cargo xtask verify` passed (or: link to log if not).

## Needs review (major upgrades)

| Crate | Current | Available | Strand |
|---|---|---|---|
| <name> | <a.b.c> | <x.y.z> | bd-XXXX |
| … | | | |

Each strand carries a one-line description and labels `deps`, `cargo`. Triage at your cadence.

## Skipped

- **`<crate>`** — <reason: e.g. "consumed only by `crates/wasm-bindgen-futures-patch/` (vendored)">
- …

## Duplicate-version delta

Before: <N> duplicates.
After: <N> duplicates.

<If new duplicates were introduced, list them here as a yellow flag.>

## Notes

<Any judgment calls made during the survey: pre-release version handling, classification edge cases, things that surprised you. Keep terse.>
```

### 13. Final commit on the worktree

If you wrote the plan after the lockfile commit, add it as a separate commit:

```bash
git add claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md
git commit -m "$(cat <<'EOF'
plan: cargo dependency upgrade survey YYYY-MM-DD

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

### 14. Report

braid syncs the skein automatically on every command — the strands filed in step 11 are already durable, with **nothing to commit** (no `.beads/` directory, no JSONL, no `sync` step). Report to the user:

- The worktree path and branch name.
- The plan doc path.
- The list of strands filed.
- The verification status.
- A reminder: **do not push without explicit approval** (per CLAUDE.md GIT PUSH POLICY).

### 15. Stop

Hand the worktree back to the user. They review the lockfile diff, merge or discard the branch, and triage the strands at their cadence.

## Failure modes & escalation

- **Verification fails after `cargo update`**: revert `Cargo.lock`, leave worktree, report. Do **not** file the plan as "applied"; describe the failure in a half-survey plan if useful.
- **`cargo update` reports zero changes but majors are available**: still write the plan, still file strands. The survey's value isn't only the lockfile bump.
- **A `Locking N` lands but `cargo tree --duplicates` count *grew***: not a failure, but call it out prominently in the plan TL;DR. The user may decide to revert.
- **HEAD verification fails in step 1**: stop. Tell the user. Don't survey on a broken HEAD.

## Conventions used by this skill

- **Branch / worktree name**: `cargo-upgrade-YYYY-MM-DD`.
- **Plan filename**: `claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md`.
- **Strand type/priority/labels** for majors: `chore`, `p3`, labels `deps` + `cargo`.
- **Pinning convention**: deliberate version pins are recorded in `.claude/skills/upgrade-cargo-deps/PINS.md`, not as `# pinned:` comments in `Cargo.toml`. PINS.md gives each pin a written reason, an explicit removal condition, and a "last reviewed" date the skill updates each run. The skill reads PINS.md as part of step 4 ("Identify excluded / vendored / pinned crates") and re-evaluates removal conditions every survey. Inline `# pinned: <reason>` comments next to a `Cargo.toml` constraint are still welcome as a local pointer, but PINS.md is the source of truth.

## See also

- **`.claude/skills/upgrade-cargo-deps/PINS.md`** — every deliberate pin and known transitive incompatibility, with a removal condition the skill re-checks every run. Read this before listing the "Skipped" section of the survey plan.
- Design plan: `claude-notes/plans/2026-05-04-cargo-dependency-upgrade-skill.md`
- Braid epic: bd-hb8h
- `CLAUDE.md` GIT PUSH POLICY (the skill must not push)
- `CLAUDE.md` "Full Project Verification" (`cargo xtask verify` semantics)
- `.claude/rules/worktrees.md` (worktree convention)

---
title: 'Cargo dependency upgrade skill — design discussion'
date: 2026-05-04
description: 'Designs a repeatable skill that surveys workspace Cargo dependencies, applies safe patch and minor upgrades in a worktree, and reports major upgrades for human review, with a plan doc rather than a PR.'
status: approved  # Design settled — ready to implement
braid:
  strand: bd-hb8h
  priority: P2
---

**Decisions logged:** 2026-05-04

## Overview

In a recent session involving a cargo dependency, Claude ran a command that listed
the installed version alongside available upgrades for workspace dependencies. The
user observed that this could be packaged into a repeatable, periodically-run
workflow — e.g. "every two weeks, ask Claude to survey our dependencies and propose
upgrades."

This document is a **design discussion**, not an implementation plan. The point is
to align on shape, scope, and guardrails before writing the skill. Implementation
work should be filed as a follow-up beads issue once the design is settled.

## What problem is this solving?

Concretely:

- We have many workspace crates with shared and crate-specific dependencies.
- Patch and minor upgrades are usually safe but accumulate silently.
- Major upgrades carry breaking-change risk and can cascade across the workspace.
- Manually running `cargo outdated` / `cargo upgrade` and triaging results is
  repetitive and easy to defer.
- CI's `-D warnings` strictness (per CLAUDE.md) means even non-breaking upgrades can
  surface new lints.

A skill could turn this from "I'll get to it eventually" into a periodic, low-effort
review that produces a reviewable summary the user can accept, defer, or reject.

## Open design questions (for the user)

These are the things I'd like to talk through before drafting the skill.

### 1. Trigger model: skill vs. scheduled remote agent vs. both?

Three plausible mechanisms exist:

- **Skill only** — user invokes `/upgrade-cargo-deps` (or similar) when they feel
  like it. Lowest infrastructure cost; relies on user remembering.
- **Scheduled remote agent** (the `schedule` skill, cron-based) — runs every N days
  and posts results somewhere (PR? issue? notification?). Costs Claude credits on a
  schedule even when there's nothing interesting.
- **Skill + suggested cadence** — skill exists for on-demand use, plus a
  documented "run this every two weeks" convention.

My instinct: start with **skill only**, document the recommended cadence, and only
add scheduling once we know the skill itself produces useful output. Scheduling is
cheap to add later; cleaning up a noisy schedule is more work.

### 2. Scope: what does "upgrade" mean?

Several flavors of upgrade exist, with different risk profiles:

| Flavor | Tool | Risk | Frequency |
|---|---|---|---|
| `Cargo.lock` refresh (patch versions within existing semver ranges) | `cargo update` | Low | Weekly-ish |
| Compatible upgrades (bump `Cargo.toml` ranges within semver) | `cargo upgrade --compatible` (cargo-edit) | Low-medium | Bi-weekly |
| Breaking upgrades (e.g. `1.x` → `2.x`) | `cargo upgrade --incompatible` | High | Per-crate, deliberate |
| Workspace-wide MSRV bumps | manual | Highest | Rarely |

Should the skill handle all four? My instinct:

- **Yes, but with separate phases.** The skill's first action is a survey; then it
  proposes patch/minor upgrades as one bundle (auto-applyable after verification),
  and lists major upgrades as a separate "needs human judgment" section with links
  to changelogs/release notes.
- Lock-file-only refreshes might be common enough to be their own subcommand or
  flag (`--lock-only`).

### 3. Output format: PR, plan doc, beads issues, or transcript?

Options for where the skill's findings land:

- **A plan document** under `claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md`,
  listing each proposed upgrade with rationale and a check-off list — fits the
  existing plan workflow well.
- **A beads issue** per non-trivial upgrade (major bumps), so they can be triaged
  alongside other work.
- **A draft PR** with patch/minor upgrades already applied and verified.
- **All three**: PR for the safe stuff, beads issues for major bumps, plan doc as
  the index linking them.

My instinct: **plan doc + beads issues for majors**. Generating a PR
automatically conflicts with the GIT PUSH POLICY (CLAUDE.md) and would require
the user to approve every run. A plan doc is review-friendly and lets the user
decide what to apply.

### 4. Verification: how much does the skill run before reporting?

Options:

- **Survey only** — just list outdated deps, don't apply anything. Fast (\~seconds).
- **Apply patch/minor + `cargo build --workspace`** — moderate (\~minutes).
- **Apply + full `cargo xtask verify`** — slow (\~10+ minutes including hub-client).
- **Apply each upgrade in isolation** — slowest, but gives per-dep verification.

My instinct: **apply patch/minor in a worktree, run `cargo xtask verify
--skip-hub-build` at minimum, escalate to full verify if anything in
`quarto-core` / `quarto-pandoc-types` / hub-client deps was touched.** Major
upgrades stay un-applied and just go into the report.

The worktree convention (`.worktrees/`) means we don't disturb the user's working
copy.

### 5. Tooling: do we need cargo-edit / cargo-outdated installed?

- `cargo update` is built-in.
- `cargo outdated` requires `cargo-outdated` (third-party).
- `cargo upgrade` requires `cargo-edit` (third-party).
- `cargo tree --duplicates` is built-in and useful for detecting dep duplication.

The earlier session (which inspired this) presumably used one of these. Worth
confirming what's already installed and whether we want to standardize.

### 6. Frequency / cadence

If we settle on skill-only, what cadence should the documentation suggest?

- Weekly is probably too noisy (most weeks: nothing interesting).
- Monthly risks letting things drift.
- **Bi-weekly** feels right — matches typical release cycles for many ecosystem
  crates.

But this might be a "let's run it once and see what comes out" question.

### 7. Special cases in this monorepo

A few things the skill needs to know about:

- **Workspace inheritance**: many deps are declared in the root `Cargo.toml`\'s
  `[workspace.dependencies]`. Upgrades should prefer the workspace level.
- **WASM target**: `wasm-qmd-parser` and `wasm-quarto-hub-client` build to
  `wasm32-unknown-unknown`. Some crates\' upgrades break on WASM but not native.
  Verification needs to include the WASM build path (per CLAUDE.md, `cargo xtask
  verify` covers this).
- **Tree-sitter crates**: pinned versions matter; upgrading these often means
  regenerating parsers.
- **Pinned-for-reason deps**: are there any? A `# pinned because X` comment
  convention in `Cargo.toml` would let the skill skip them automatically. Worth
  auditing.

## Proposed structure (strawman, for the user to push back on)

If we go with "skill only, plan doc + beads for majors":

```
.claude/skills/upgrade-cargo-deps/
  SKILL.md
```

`SKILL.md` would describe:

1. **Survey phase**: run `cargo outdated --workspace --format json` (or
   equivalent), parse, group by upgrade flavor.
2. **Worktree phase**: create `.worktrees/cargo-upgrade-YYYY-MM-DD/`, apply the
   patch+minor bundle, run `cargo xtask verify --skip-hub-build`.
3. **Report phase**: write `claude-notes/plans/YYYY-MM-DD-cargo-upgrade-survey.md`
   with three sections:
   - **Applied & verified** — patch/minor upgrades the user can merge as-is.
   - **Needs review** — major bumps, per-crate, with changelog links.
   - **Skipped** — anything pinned for a reason, with the reason.
4. **Beads phase**: file one beads issue per major bump, linked from the plan.
5. **Stop** — don't push, don't open PRs. Hand the worktree back to the user.

This deliberately mirrors the `investigate-beads` skill's "produce reviewable
artifacts, don't take terminal actions" shape.

## Out of scope (for this design)

- Automating PR creation or pushes (violates GIT PUSH POLICY).
- npm/hub-client dependency upgrades — that's a separate concern with different
  tooling (`npm outdated`, `npm-check-updates`).
- Rust toolchain (`rust-toolchain.toml`) upgrades — different risk profile.
- Security-advisory triage (`cargo audit`) — related but probably a separate skill.

## Decisions (logged 2026-05-04)

1. **Skill only.** No scheduling yet.
2. **Categorize before acting.** The skill must report which upgrades fall into
   which category (lock-only / compatible / major). Minor bumps and compatible
   upgrades are applied by default; major upgrades are surfaced for human
   judgment and not applied.
3. **Output: plan doc + beads issues + the actual successful work.** When
   minor/compatible upgrades succeed, the worktree contains the applied changes
   so the user can review and merge them; the plan doc is the index, and beads
   issues track each major upgrade.
4. **Verification: full `cargo xtask verify` after patch/minor upgrades.** Slower
   is fine — the value is in catching failures with full output. No per-dep
   verification unless we're already diagnosing a failed upgrade.
5. **Tooling: stick with what's built into `cargo`.** `cargo update` and
   `cargo tree --duplicates` are sufficient for v1; no third-party tools
   required. (See note below on `cargo tree --duplicates`.)
6. **Cadence: bi-weekly**, documented in the skill.
7. **Pinned-for-reason audit: do it once during implementation.** User doesn't
   recall any; the skill's first run effectively audits this. If discovered,
   establish a `# pinned: <reason>` comment convention then.
8. **npm: deferred.** Rust-only for v1. Once this skill proves out, mirror the
   approach for npm in a follow-up.

### Note on `cargo tree --duplicates`

`cargo tree --duplicates` (a.k.a. `cargo tree -d`) lists crates that appear in
the dependency graph at **multiple distinct versions**. This happens when two
direct or transitive deps require incompatible semver ranges of the same crate
— e.g. crate A pulls `serde 1.0.150` and crate B pulls `serde 1.0.197`, and
they end up coexisting in the build.

It's useful in this skill because:

- Upgrading a single dep can collapse duplicates (newer versions of A might
  loosen its serde constraint), shrinking compile times and binary size.
- Conversely, a successful upgrade that *introduces* a new duplicate is a
  yellow flag worth surfacing in the report.
- A baseline duplicate count gives a before/after metric for the survey.

Concretely the skill can run `cargo tree --duplicates --workspace` before and
after, and include any delta in the plan doc.

## Work items

- [x] Get user alignment on the design questions above
- [x] Audit existing `Cargo.toml` files for "pinned for reason" comments
  - **Result (2026-05-04):** no pinning comments anywhere; no exact-version
    (`"=x.y.z"`) pins in any of our crates. The `=` pins in
    `crates/wasm-bindgen-futures-patch/Cargo.toml` are upstream's
    auto-generated `Cargo.toml` (file header explicitly says so) and must be
    treated as untouchable by the skill.
  - Pinning convention established in the skill: a `# pinned: <reason>`
    comment immediately above the dep line in `Cargo.toml`. As of this date,
    none exist.
- [x] Smoke-test the survey commands:
  - `cargo update --dry-run --workspace --verbose` — emits `Locking N
    packages…` for in-range bumps and `Unchanged X v<a> (available: v<b>)`
    for out-of-range candidates. Suitable for parsing.
  - `cargo tree --duplicates --workspace --depth 0` — clean per-crate
    duplicate listing, suitable for before/after delta.
- [x] Write `.claude/skills/upgrade-cargo-deps/SKILL.md`
  - Created at `.claude/skills/upgrade-cargo-deps/SKILL.md` (2026-05-04).
  - 15-step procedure: pre-flight verify → survey → classify → worktree →
    bootstrap → `cargo update` → `cargo xtask verify` (full) → duplicates
    delta → commit lockfile → file beads per major → write plan doc →
    commit plan → sync beads from main repo → report → stop.
  - Documents bi-weekly cadence in "When to use".
  - Documents skip rules for vendored (`wasm-bindgen-futures-patch`) and
    workspace-excluded crates.
  - Documents failure-mode escalation (revert lockfile, leave worktree,
    report).
- [ ] Test the skill end-to-end on the current workspace; capture the first
      survey as an example output and link it from the skill doc
      (next session — user-triggered)

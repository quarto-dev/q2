# Migrate q2 issue tracking from beads_rust to braid

**Status:** Draft / awaiting go-ahead
**Created:** 2026-06-08
**Beads issue:** `bd-a1qb3` (epic) · prerequisite braid-0.3.0 work `bd-sjk4t` (blocks the epic)
**Companion doc (braid-side work, hand off to a braid-repo agent):**
[`2026-06-08-braid-0.3.0-features-for-migration.md`](./2026-06-08-braid-0.3.0-features-for-migration.md)

This issue and plan deliberately stay in **beads_rust**. If the migration
fails or is abandoned, the plan and its tracking survive in the old system.

---

## Overview

We are replacing [beads_rust](https://github.com/Dicklesworthstone/beads_rust)
(`br`) with **braid** as q2's issue tracker. braid is a drop-in replacement for the
simple parts of beads, but instead of a git-committed `.beads/issues.jsonl` +
SQLite database, the source of truth is a single **automerge CRDT document**
(a "skein") synced through a sync server. There is no git involvement in the
issue store and no daemon; any number of agents/worktrees/machines edit in
parallel and the CRDT resolves conflicts.

braid source: `~/rooms/room-1/braid`. Installed binary: `braid` (on PATH).
Run `braid agents-info` for the version-matched usage guide.

### Why this is low-risk (validated in-session 2026-06-08)

A throwaway end-to-end import of the **live `.beads/issues.jsonl` (1145
issues, 1051 dependencies)** was performed and inspected:

| check | result |
|---|---|
| `braid import` wall time | ~0.7s CPU (sync timeout dominates wall) |
| whole automerge doc on disk | **460 KB** |
| `braid list --all --json` | 1145 strands, sub-second |
| dependencies preserved | **1051 / 1051** (all four types: `blocks`, `parent-child`, `discovered-from`, `related`) |
| `ready` / `blocked` parity | computed correctly (351 ready, 373 open) |
| beads ids preserved verbatim | yes — `bd-068k`, `bd-0gkh3`, … unchanged |

The "we've never used braid for a large project" risk is largely retired for
**import + local operations** at this scale. The remaining unknowns are
*long-term* doc growth and *sync throughput to a real server* — both low risk
given a 460 KB starting point.

### The critical property: id preservation

`braid import` **upserts by `id`**, writing each record under its existing id
verbatim (`crates/braid/src/import.rs:186` →
`crates/braid-core/src/amdoc.rs:184`). braid's *own* generated ids are always
8 random base36 chars (`crates/braid-core/src/id.rs:15`), so they **cannot
collide** with our 4–5-char `bd-XXXX` ids.

This matters enormously: **412 distinct `bd-` ids are referenced across 1141
locations in tracked source/plans/docs** (`git grep -oE 'bd-[a-z0-9]{4,5}'`).
Import preserving ids means **none of those references need editing**. The
"force a specific id" capability the migration hinges on is really "import
preserves ids" — note that `braid create` has *no* `--id` flag, and doesn't
need one: with a CRDT, parallel workers never need to pre-agree on ids to
avoid collision (beads' `create --id worker1-100` pattern becomes obsolete).

---

## Decisions (locked 2026-06-08)

1. **Sync server: public relay for now.** Use the default
   `wss://sync.automerge.org` during the experiment. The doc id is a
   **read/write bearer token**. Note the concern here is *write access*, not
   confidentiality: the issue **content** is not private — the
   `.braid/snapshot.jsonl` backup is committed to the GitHub repo and pushed.
   What the secret protects is the *implied permission to edit* the skein
   (anyone holding the id can mutate issues). That's why `.braid.toml` stays
   gitignored and the id never goes into commits/PRs/logs. Moving to a private
   server (`braid rotate`, no id churn) is **low priority** given the above.

2. **Committed snapshot = backup only, strictly one-directional.** We commit a
   periodic `braid export` snapshot to the repo for grep/diff/recovery, but:
   - The snapshot flows **automerge → file only**. It is **never** a sync or
     import source back into the skein (except the *one-time* initial
     migration import, which reads beads' JSONL, not this snapshot).
   - On any git conflict in the snapshot file, **resolve by pulling fresh
     `braid export` from automerge** — even if that means "cross-branch
     contamination" (the snapshot on branch A showing issue state created on
     branch B). The CRDT is always authoritative; the file is a photograph.
   - The snapshot lives on the work branch alongside the work it documents.
   - Docs/workflow must state this loudly so no agent ever runs
     `braid import <snapshot>` to "restore" state.

3. **Build all four braid feature gaps first** (none blocking, all quality):
   recursive `dep tree`, `create --deps` one-shot, import skip-tombstones,
   and a `braid` agents-info skill installer. These are specified in the
   companion doc and target **braid 0.3.0**. q2 cutover **blocks** on that
   work being merged and a 0.3.0 binary installed.

---

## Phases

### Phase 0 — Prerequisite: braid 0.3.0 (EXTERNAL, BLOCKING)

The four features in
[`2026-06-08-braid-0.3.0-features-for-migration.md`](./2026-06-08-braid-0.3.0-features-for-migration.md)
must be implemented in the braid repo, released as 0.3.0, and installed
locally before Phase 2 cutover. Tracked as a single beads task that the q2
epic depends on (`blocks`). This phase is owned by the braid-repo agent, not
this checkout.

- [x] braid 0.3.0 features merged in `~/rooms/room-1/braid` (done by braid-repo agent)
- [x] `braid --version` reports 0.3.0 locally — confirmed 2026-06-08
- [x] new commands present: `dep tree`, `create --deps`, `agents-info --install`
- [ ] `braid import` skip-tombstone behavior confirmed against q2's JSONL (Phase 1)

### Phase 1 — Repeatable dry-run validation (TDD: validation-first)

Codify the in-session validation as a repeatable checklist/script so cutover
is a known-good replay, not a one-off.

- [x] `br sync --flush-only` to bring `.beads/issues.jsonl` fully current
      (JSONL current at 1147 lines: 1145 data + 2 tracking issues)
- [x] Throwaway `braid init --prefix bd` + `braid import` in `/tmp/braid-dryrun`
      (scratch dir, abandoned skein) — 2026-06-08
- [x] Assert: strand count == JSONL minus tombstones (**1145** = 1147−2);
      dependency count **1053/1053**; spot-check ids `bd-068k`, `bd-0gkh3`,
      `bd-t3ny`, `bd-a1qb3`, `bd-sjk4t` survive verbatim — all pass
- [x] Assert: `ready`/`blocked` sane vs beads (`--limit 0`): braid ready=352
      blocked=21 open=373 vs beads ready=301 open=334. Differences are
      **expected semantics**, not data loss: braid "open" = non-closed
      (open+in_progress); braid `ready` counts unblocked `in_progress` as
      ready, beads excludes `in_progress`/`deferred`. Dep graph identical.
- [x] Assert: **0** `tombstone`-status records (0.3.0 skip-tombstones:
      "imported 1145 strands (skipped 2 tombstones)") — clean `braid list`
- [ ] Capture the checklist as a small repeatable script (defer; in-session
      commands recorded in the plan + transcript are sufficient for cutover)

### Phase 2 — Skein setup & initial import (the cutover point)

- [x] `braid init --prefix bd --name q2` → skein `46W8bX…`; doc id in the
      gitignored `.braid.toml` only (never printed to transcript)
- [x] `.braid.toml` added to `.gitignore` **before** init; `git check-ignore`
      confirms it's ignored
- [x] Configured `~/.config/braid/projects.toml` `[projects.q2]` (doc id +
      sync server) — copied from `.braid.toml` without printing the secret
- [x] Committed-marker mechanism: `.braid-project` (contents: `q2`) created;
      verified a clean dir with only the marker resolves to 1145 strands via
      the user config (replaces beads' `.beads/redirect` entirely)
- [x] Final `br sync --flush-only`; `braid import .beads/issues.jsonl` →
      "imported 1145 strands (skipped 2 tombstones)"
- [x] Re-ran assertions on the real skein: 1145 strands, 1053 deps, 0
      tombstone noise, epic `bd-a1qb3` + child `bd-sjk4t` present & linked
- [x] `braid sync` → "synced with wss://sync.automerge.org (1145 strands)"
      — relay accepted the doc

**Note:** `.braid-project` and the `.gitignore` change are staged for commit
at the Phase 3 checkpoint (non-secret; safe to commit).

### Phase 3 — Workflow port (q2 code/docs)

TDD: the xtask changes are the only *code*; write/adjust their unit tests
first (they already split parsing from I/O for exactly this).

- [x] **xtask `create_worktree.rs`** — DONE. `br show` → `braid show`.
      Handled the shape change: array→single object, deps array→keyed map.
      New pure `parse_strand` (returns title/status/external_ref/parent_id)
      + `fetch_issue_metadata` does the **two-fetch** parent resolution
      (braid deps don't carry the parent's title/status). Renamed
      `BeadsMetadata`→`IssueMetadata`, `SectionKind::Beads`→`Braid`,
      `plan_beads`→`plan_braid`. 5 new fixture-driven `parse_strand_*` unit
      tests; "braid is required" error message. **64/64 xtask tests pass.**
- [x] **xtask `switch_task.rs`** — DONE. `br update` → `braid update` in
      `claim_issue`; imports + `update_worktree_context` use the renamed
      symbols; `--no-claim` unchanged.
- [x] **xtask worktree bootstrap** — DONE. Deleted `write_beads_redirect` and
      its call; `build_section` emits a `**Braid:**` line + `braid show`.
      **E2E verified:** `cargo xtask create-worktree bd-068k` resolves the
      title via real `braid show`, writes the correct CLAUDE.local.md, writes
      **no** `.beads/redirect`, and braid resolves from inside the worktree
      via root-`.braid.toml` walk-up. Worktree cleaned up after.
- [x] **braid agents-info skill** — DONE. `braid agents-info --install
      .claude/skills/braid` + prepended Claude Code frontmatter (name +
      description); the `/braid` skill now registers and the installer
      preserves the frontmatter idempotently on re-run.
- [x] **Skills** (`.claude/skills/`) — DONE. Ported `br`→`braid` in
      `investigate-beads` (by hand, the worked example), and `triage`,
      `upgrade-cargo-deps`, `preview-render-parity` (via sub-agent, verified):
      command renames, single-object `show --json` (dropped `.[0]`), repeated
      `-l` labels, removed `--id`/`--limit`, and **deleted the
      `br sync --flush-only; git add .beads/` steps** (nothing to commit now).
      Cross-checked: no stale `br <cmd>` / `.beads/` command refs remain.
- [ ] **Rules** (`.claude/rules/`): rewrite `worktrees.md` (drop the redirect
      section; document `.braid-project` + the no-redirect worktree story)
      and `xtask.md` (the create-worktree row mentions `.beads/redirect`).
- [x] **`CLAUDE.md`** — DONE. Rewrote WORK TRACKING intro + Quick Reference
      for braid; deleted the `br sync --flush-only; git add .beads/` workflow
      step (nothing to commit now); added the **Snapshot backup policy**
      subsection (one-directional, never-re-import, regenerate-on-conflict);
      added braid dependency-gating semantics + `docs`/`question` types;
      fixed trailing `br` references.

### Phase 4 — Snapshot backup mechanism

- [x] **DONE.** Added `cargo xtask braid-snapshot` (`braid_snapshot.rs`,
      registered in `main.rs`) writing `braid export` → `.braid/snapshot.jsonl`
      (committed dir, distinct from the gitignored `.braid.toml` secret).
      Unit test for the path; **e2e verified** (wrote 1145 strands).
- [x] **DONE.** Documented the one-directional / never-re-import /
      regenerate-on-conflict rule in three places: `CLAUDE.md` § Snapshot
      backup policy, the `braid_snapshot.rs` module doc + CLI `--help`, and a
      `.braid/README.md` beside the snapshot file.

### Phase 5 — Cutover & decommission beads

**Cutover executed 2026-06-08** (user approved "cut over now"). braid is now
the canonical tracker.

- [x] Cutover declared; from here, all new issue writes go to braid. CLAUDE.md,
      rules, and skills already point exclusively at braid.
- [x] **`.beads/` kept in git as a frozen historical record** — NOT deleted
      (412 ids referenced from source; the JSONL stays greppable and is the
      rollback source). Prepended a prominent **⛔ FROZEN** notice to
      `.beads/README.md` forbidding `br` writes.
- [x] Final `br sync --flush-only` + final `braid import` done (idempotent;
      synced the `bd-sjk4t` close). This was **the** final re-import — beads is
      now frozen, so braid-only edits are safe (no future import will overwrite).
- [ ] `beads.db`/`beads.db-wal` (~28 MB WAL) left in place for now; harmless
      (gitignored working files). Can be removed in a later cleanup.
- [ ] CI / hooks that touch `.beads/`: none found writing to it; revisit if any
      surface. (The post-edit `cargo fmt` hook is unrelated.)
- **Tracking note:** going forward, the migration epic `bd-a1qb3` is maintained
  in **braid** (canonical), not beads. The beads copy is frozen.

### Phase 6 — Rollback plan (keep until braid is proven in daily use)

- [ ] Document: until braid is trusted, **beads remains the recoverable
      source**. The frozen `.beads/issues.jsonl` + git history can be
      restored at any time. `braid export` can be massaged back into beads
      JSONL if needed (lossy: comment ids were rewritten int→string, beads-only
      fields were dropped) — acceptable for emergency rollback.
- [ ] Define the "braid is proven" exit criterion (e.g. N weeks of daily use,
      no data-loss incident, sync reliability acceptable) after which beads
      can be fully archived.

---

## Known data deltas on import (accepted)

From `crates/braid/src/import.rs` (verified in-session):

- `completed` status → `closed` (3 records).
- `tombstone` status (2 records: `bd-1xf5`, `bd-298oe`) → handled by the
  0.3.0 **skip-tombstones** feature; without it they import as a nonstandard
  `tombstone` status that shows as noise in `braid list`.
- Beads integer comment ids → fresh `c-<base36>` string ids (CRDT-safe).
- Beads-only fields dropped: `source_repo`, `source_repo_path`,
  `compaction_level`, `original_size`, `original_type`, dependency
  `metadata`/`thread_id`/`issue_id`, and the delete-metadata fields.
- All of: `description`, `design`, `acceptance_criteria`, `notes`, `labels`,
  `assignee`, `external_ref`, `defer_until`, timestamps, `close_reason`,
  `closed_at`, priority, type — **preserved**.

---

## Open questions / risks to watch

- **Write-access on the public relay** (Decision 1) — **low priority.** The
  doc id grants *write* access to anyone holding it; issue content itself is
  not secret (it's committed to GitHub via the snapshot). Move to a private
  sync server eventually, but there's no confidentiality urgency.
- **Multi-machine / colleague onboarding** — each consumer needs the doc id
  in `~/.config/braid/projects.toml`; the `.braid-project` marker only names
  the project, not the secret. Document the `braid secret` handoff.
- **Long-term doc growth** — automerge keeps full history; `braid rotate`
  sheds it when needed. Re-measure doc size after a few weeks of churn.
- **No git-tracked issue history** — the old `git log .beads/` audit trail
  goes away; the committed snapshot (Decision 2) partially compensates but is
  a photograph, not an event log. The CRDT itself holds full history.

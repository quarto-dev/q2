# `.braid/` — braid backup snapshot

This directory holds **backup-only** artifacts for the project's braid issue
tracker (the "skein"). The skein itself is an [automerge](https://automerge.org)
CRDT synced through a sync server — **that is the single source of truth**, not
anything in this directory.

## `snapshot.jsonl`

A `braid export` dump of every strand, regenerated with:

```bash
cargo xtask braid-snapshot      # writes .braid/snapshot.jsonl
```

It exists so issues stay greppable in PRs, diffable in git history, and
recoverable. It is committed to whatever work branch you are on.

### ⚠️ One-directional — never import it back

- The snapshot flows **automerge → file only**. It is **never** an import or
  sync source. **Do not run `braid import .braid/snapshot.jsonl`.** The only
  JSONL ever imported was the one-time beads→braid migration (done 2026-06-08).
- On a git **conflict** in `snapshot.jsonl`, do **not** hand-merge. Regenerate
  from the live skein: `cargo xtask braid-snapshot`. The CRDT is authoritative;
  this file is a photograph. (It may show strand state from another branch —
  "cross-branch contamination" — which is expected and harmless, because the
  file is not the truth.)

See `CLAUDE.md` § Snapshot backup policy and
`claude-notes/plans/2026-06-08-braid-migration.md` for the full rationale.

## Not in this directory

The skein **secret** lives in `.braid.toml` at the repo root (gitignored, a
read/write bearer token — never committed). The committed, non-secret
`.braid-project` marker (also at the repo root) only names the project so
worktrees/clones resolve the skein.

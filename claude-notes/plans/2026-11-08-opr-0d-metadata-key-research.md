---
title: 'OPR-0d: research, could the execution-records source list be a metadata key?'
date: 2026-11-08
description: 'Research note that answers Gordon''s question (R5): whether the source list of --execution-records could be a document or project metadata key instead of, or beside, a command-line flag. Covers hash effects, hub trust, path resolution, machine-local sources, convert, and conflict checks. Ends with a recommendation; Gordon decides, which unblocks OPR-5.'
status: ready
depends-on: []
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/0d-metadata-key-research` · **Lane C** (`workspace-5`) · **Blocks:** OPR-5 (through Gordon\'s decision).
**Needs:** nothing beyond the repository.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
git switch -c opr/0d-metadata-key-research feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/0d-metadata-key-research
```

## Goal

Gordon: "Carlos is going to ask why `execution-records` is not just a YAML field, so we'd better evaluate if there is
any reason for it not to be. If we went that way, we would not need `--execution-records` because all commands
already have `-M` (except convert)." The note must answer whether it can be, and what each answer does to OPR-5 and
OPR-6. **No product code changes.** The deliverable is one note,
`claude-notes/research/2026-11-xx-opr-metadata-key-research.md`.

## Background (relied from review agents on 2026-11-08 unless marked; re-check each before you cite it)

- The flag today is `--records` on `q2 preview` (`crates/quarto/src/main.rs:~352`) and `q2 provide-hub` (`:~809`) and
  on `q2 record export-positron` (`:~63`, removed by RIU follow-up Item 8, replaced by `q2 convert`, currently a stub
  with no record option). Grammar and walk are shared in `crates/quarto-core/src/record/sources.rs`.
- `-M` exists on `render` and `preview` (`main.rs:~96-103`, `:~426-438`), sits above the document\'s own metadata and
  applies to every document (read). `preview.engine` is a YAML setting read only by `quarto-preview`.
- A new `execute:` key would count in the document hash of Jupyter, knitr and Julia, because none ignores unknown
  keys; keeping records fresh needs exclusions in Jupyter\'s `DISPLAY_ONLY_KEYS`
  (`engine/jupyter/mod.rs:~251`), knitr\'s `presentation_execute_keys` (with a `KNITR_HASH_VERSION` bump) and Julia\'s
  `record-wire.ts`. `exec_hash.rs:~292` special-cases a top-level `record` key only.
- In the hub provider a guest\'s whole project is materialized on the host and its `_quarto.yml`, `_metadata.yml`
  and front matter take effect; consent guards only `run_and_store`; the provider\'s `q2` source already reads
  `.quarto/execution-records/<doc>.ipynb` from the guest tree.
- Config-authored paths resolve relative to the declaring file; CLI paths relative to the working directory
  (`claude-notes/designs/path-resolution-model.md`).

## Checklist (every box is `[unattended]`)

- [ ] **Hash effect.** Demonstrate by a scratch test or run that adding an `execute:` key changes each engine\'s
      document hash, and list the three exclusion points (file and line) a new key would need. State whether
      `execute: record: true` itself enters the document hash (a review agent could not reproduce the hash by hand).
- [ ] **Hub trust.** In `crates/quarto-hub-provider`, trace what a guest document\'s or project\'s metadata can do
      today, and what a key naming a directory or `positron` would let it read on the host, and who would see the
      result. State plainly whether a metadata key is safe there without a new rule, and what rule would make it so
      (for example: honor the key only from host-controlled metadata, or only for `q2` and `engine`).
- [ ] **Path resolution.** Which base a directory entry would resolve against as a key (the declaring file) and as a
      flag (the working directory); does the `config-path-base` lint or `resolve_config_path` apply.
- [ ] **Machine-local sources.** Which entries (`positron`, `cache`, directory paths) make sense in a committed
      file, and which break another machine or CI.
- [ ] **`convert` and `export-positron`.** `convert` has no `-M` today: what each option would need there.
- [ ] **Conflicts.** `--no-execute` plus `engine`, and `--replay` (`QUARTO_REPLAY` is read in
      `commands/render.rs`, not by clap): where each check lives if the list arrives as metadata.
- [ ] **Per-command defaults.** Could they come from per-command metadata (`render:`, `preview:`)? Does such a
      section exist, and what reads it.
- [ ] **Options and a recommendation.** Lay out (a) flag only, (b) metadata only, (c) a mix (metadata for `engine`
      and `q2`, a flag for machine-local sources and paths), with the effect of each on OPR-5 and OPR-6 (files and
      work), and recommend one. Do not decide.
- [ ] Render-check the note (`target/debug/q2 render --strict <file>`).

## Exit criteria

- The note answers each box with evidence marked ran, read or not done, and ends with the recommendation and a
  table "effect on OPR-5 and OPR-6" for each option.
- **`[Gordon]` (after landing, not part of this plan\'s run):** Gordon reads the note, records his decision in the
  header of the OPR-5 plan file, and flips OPR-5 from `blocked-on-gordon` to `ready`.

## Notes from the run

(Fill in at landing.)

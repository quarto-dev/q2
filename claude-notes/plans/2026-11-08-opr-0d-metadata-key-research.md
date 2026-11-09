---
title: 'OPR-0d: research, could the execution-records source list be a metadata key?'
date: 2026-11-08
description: 'Research note that answers Gordon''s question (R5): whether the source list of --execution-records could be a document or project metadata key instead of, or beside, a command-line flag. Covers hash effects, hub trust, path resolution, machine-local sources, convert, and conflict checks. Ends with a recommendation; Gordon decides, which unblocks OPR-5.'
status: complete
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
`claude-notes/research/2026-11-08-opr-metadata-key-research.md`.

## Background (relied from review agents on 2026-11-08 unless marked; re-check each before you cite it)

- The flag today is `--records` on `q2 preview` (`crates/quarto/src/main.rs:~352`) and `q2 provide-hub` (`:~809`) and
  on `q2 record export-positron` (`:~63`; RFU-1 deletes it and gives `q2 convert` its own `--execution-records`). Grammar and walk are shared in `crates/quarto-core/src/record/sources.rs`.
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

- [x] **Hash effect.** Demonstrate by a scratch test or run that adding an `execute:` key changes each engine\'s
      document hash, and list the three exclusion points (file and line) a new key would need. State whether
      `execute: record: true` itself enters the document hash (a review agent could not reproduce the hash by hand).
- [x] **Hub trust.** In `crates/quarto-hub-provider`, trace what a guest document\'s or project\'s metadata can do
      today, and what a key naming a directory or `positron` would let it read on the host, and who would see the
      result. State plainly whether a metadata key is safe there without a new rule, and what rule would make it so
      (for example: honor the key only from host-controlled metadata, or only for `q2` and `engine`).
- [x] **Path resolution.** Which base a directory entry would resolve against as a key (the declaring file) and as a
      flag (the working directory); does the `config-path-base` lint or `resolve_config_path` apply.
- [x] **Machine-local sources.** Which entries (`positron`, `cache`, directory paths) make sense in a committed
      file, and which break another machine or CI.
- [x] **`convert`.** `convert` (RFU-1) has no `-M`: what each option would need there.
- [x] **Conflicts.** `--no-execute` plus `engine`, and `--replay` (`QUARTO_REPLAY` is read in
      `commands/render.rs`, not by clap): where each check lives if the list arrives as metadata.
- [x] **Per-command defaults.** Could they come from per-command metadata (`render:`, `preview:`)? Does such a
      section exist, and what reads it.
- [x] **Options and a recommendation.** Lay out (a) flag only, (b) metadata only, (c) a mix (metadata for `engine`
      and `q2`, a flag for machine-local sources and paths), with the effect of each on OPR-5 and OPR-6 (files and
      work), and recommend one. Do not decide.
- [x] Render-check the note (`target/debug/q2 render --strict <file>`).

## Exit criteria

- The note answers each box with evidence marked ran, read or not done, and ends with the recommendation and a
  table "effect on OPR-5 and OPR-6" for each option.
- **`[Gordon]` (after landing, not part of this plan\'s run):** Gordon reads the note, records his decision in the
  header of the OPR-5 plan file, and flips OPR-5 from `blocked-on-gordon` to `ready`.

## Notes from the run

Deliverable: [the research note](../research/2026-11-08-opr-metadata-key-research.md). It renders under `q2 render --strict`.

- **Ran:** a scratch test (reverted, not committed) of `hash_document` for the Jupyter and knitr declarations and a
  conservative Julia stand-in; `q2 render --strict` on the note. **Read:** everything else (hub provider, path model,
  `main.rs`, `sources.rs`, preview config). **Not done:** a provider leak test, a full render with a key, RFU-1's final
  `convert` shape.
- **Finding:** a top-level key is in no engine hash, but a key inside `execute:` is, until three exclusion lists are
  edited and the knitr and Julia versions are bumped. `execute: record: true` itself changes the hash on all three
  engines at the `hash_document` level (ran); the "core always excludes `record`" exclusion covers only a top-level key.
  I found no caller that strips it; not confirmed with a full render. Out of scope here; no strand opened.
- **Recommendation:** flag only for OPR-5 and OPR-6, with a narrow key (`engine` and `q2`, flag wins) as an additive
  follow-up. Gordon decides and records it in OPR-5.

**Follow-up after Gordon\'s review (2026-11-09).** Gordon read the note and decided to **move** the source list into the
metadata key `execute: record-sources` and remove the command-line option entirely (decision R5). The note now opens with
the nine sub-decisions and corrects one claim (a committed `positron` entry falls through, it does not break another
machine). The decision is recorded in OPR-5, whose status is still `blocked-on-gordon`: flipping it to `ready` is his
step. Plans edited to match (all later plans; OPR-0a, OPR-0c and OPR-3 needed nothing): OPR-5 (rewritten: key, early
reader on `collect_metadata_layers`, per-command declarations, hash exclusion in three engines, Part B ceiling for
`provide-hub` and a `--share` preview; now depends on OPR-3), OPR-6, RFU-1 (no option for the list, accepts `-M`),
RFU-4, RFU-6, RFU-7, RFU-8, OPR-8, OPR-1b (one phrase), both epics, and the dependency SVG. Strand `bd-ia3idnvk` tracks
the client-visible provider warnings. Not done: no product code, no test of the provider leak.

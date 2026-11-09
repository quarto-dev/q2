---
title: 'OPR-5: one --execution-records definition, and the rename'
date: 2026-11-08
description: 'Defines the grammar, default, allowed values, validation and error text of the execution-records source list once, for every command that takes it, and renames --records to --execution-records (unreleased, no alias). Shape depends on Gordon''s answer to the metadata-key question researched in OPR-0d.'
status: blocked-on-gordon
depends-on: [opr-0d, rfu-1]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) (runs beside the [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md)) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/5-execution-records-definition` · **Lane C** (`workspace-5`) · **Blocks:** OPR-6, RFU-4, RFU-7.
**Needs:** Node tooling for the e2e edits; no R or Julia.

## Start condition

`status` is `blocked-on-gordon` until Gordon reads the OPR-0d note, writes his decision below, and flips it to `ready`.
If the section is empty or the status is not `ready`, stop and report. RFU-1 (the `q2 convert` command; it replaces
`q2 record export-positron`) must also be `complete`: it lands first so this plan never touches the deleted command.

## Decision by Gordon on the metadata-key question (R5)

(Empty. One of: flag only; metadata key only; a mix. Whichever he chooses, the Goal and checklist below are the
baseline for the **flag** part; if he chooses metadata or a mix, he also edits the boxes that change, or tells the
agent which boxes to replace before flipping the status.)

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
git switch -c opr/5-execution-records-definition feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/5-execution-records-definition
```

## Goal (decisions D6, D7)

After RFU-1 three commands declare the flag separately and each has its own resolver:

- clap fields in `crates/quarto/src/main.rs`: `preview` (about line 352), `provide-hub` (about line 809) and `convert`
  (RFU-1, spelled `--execution-records` already), each with its own doc comment;
- resolvers: `commands/preview.rs:~156` (`resolve_record_sources`), `commands/provide_hub.rs:~184` (near-identical),
  and `convert`\'s own default and allow-check from RFU-1 (it replaced `commands/record.rs`);
- the flag name inside user-visible text, much of it asserted by tests: `engine/diagnostics.rs:~289-323`,
  `quarto-preview/src/re_execute.rs:~154`, `capture_driver.rs:~1655`, `config.rs`, `lib.rs:~271`,
  `quarto-hub-provider/src/execute.rs:~103,~145,~602,~841`, `commands/preview.rs:~169-178`, the `reject_cache` text in
  `record/sources.rs`.

Already shared: `RecordSource: FromStr`, `RecordSources` (`new`, `preview_default`, `provider_default`,
`validate_directories`, `reject_cache`), the walk and `RecordLookup` (`crates/quarto-core/src/record/sources.rs`).
(relied, 2026-11-08) A shared definition is about 50 to 70 lines.

Build **one** definition:

1. A `#[derive(clap::Args)]` group carrying `--execution-records` (comma list or repeated flag, value name
   `SOURCES`), flattened into each command.
2. A per-command declaration of **allowed values and default**: `preview` allows `cache`, `q2`, `positron`, `engine`
   and a directory path, default `cache,q2,engine`; `provide-hub` refuses `cache`, default `engine`;
   `q2 convert` allows only `q2` and `engine`, default `q2,engine`; `render` (adopted in OPR-6) refuses `cache`,
   default `engine`. Defaults differ on purpose.
3. One validation and diagnostics path, and one constant for the flag name used in every message.
4. The rename: `--records` becomes `--execution-records`, **no alias, no deprecation** (unreleased: neither `v0.42.0`
   nor `main` has it). The old spelling must be rejected as an unknown flag on every command that had it.
5. `q2 preview --static` runs through `render_once` but declares its own clap conflicts today: bring it into the
   shared definition (it must accept the flag as `preview` does; OPR-6 wires the render path).

## Checklist (tests first; every box is `[unattended]` unless marked)

- [ ] Re-run the inventory (`git grep -n -e '--records'`, outside `claude-notes/`) and write the final list into
      "Notes from the run". (relied) 184 lines at the time of the review.
- [ ] **Tests first, through the real argument parser, per command:** the allowed values and default per command; the
      refusal of `cache` where declared; `--records` rejected as an unknown flag on `preview` and `provide-hub`
      (`convert` never had it); the existing parse tests (`main.rs:~1394-1640`) rewritten to the new spelling; message
      tests rewritten (they fail first on the old name).
- [ ] Implement the shared group, the per-command declarations and the one resolver; replace the three resolvers (preview, provide-hub, convert) with the one.
- [ ] Rename in every message, comment and test. Update the TypeScript e2e files in this repository:
      `q2-preview-spa/e2e/helpers/previewServer.ts` and `q2-preview-spa/e2e/records-sources.spec.ts`; run the
      records-sources e2e if it can run here and say which ran.
- [ ] **Do not edit `docs/`** here; OPR-8 owns every docs change and the final `git grep` below excludes `docs/`.
- [ ] `q2 preview --static` accepts the flag and rejects what it rejected before; add a parse test.
- [ ] **Real binary.** `target/debug/q2 preview <doc> --execution-records positron,engine --no-browser` starts as
      before; `q2 preview <doc> --records positron` fails as an unknown argument. Record the invocations and output.
- [ ] Gates: `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`;
      `cargo nextest run` for the same crates; the Windows cross-clippy for each; `cargo xtask lint`.

## Exit criteria

- One clap group and one resolver; per-command allowed values and defaults are declared in one place.
- `git grep -n -e '--records'` outside `claude-notes/` and `docs/` prints nothing.
- Old spelling rejected on every command; behavior otherwise unchanged.

## Companion (Gordon, outside this repository)

The Positron fork\'s extension (`q2Contract.ts` `buildPreviewArgs`, `previewCommand.ts`, their vitests) and its e2e
use the old spelling; RFU-7 updates them once this plan and RFU-1 have landed. Not part of this plan.

## Notes from the run

(Fill in at landing.)

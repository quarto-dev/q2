---
title: 'OPR-5: the execute: record-sources key, one early reader, and removal of --records'
date: 2026-11-08
description: 'Moves the execution-records source list from command-line options into the metadata key execute: record-sources (a comma-separated scalar), read before the pipeline by one shared early reader built on the metadata layer collector, with per-command allowed values and defaults, in-project directories only, and a hash exclusion in the three engine declarations. Removes --records from preview and provide-hub. Part B gives provide-hub and a --share preview a ceiling: the host list is the allowed set, a guest document may only reorder or narrow it. Decision R5 (Gordon, 2026-11-09): move, no flag.'
status: in-progress
depends-on: [opr-0d, rfu-1, opr-3]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) (runs beside the [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md)) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first) ·
**Research:** [OPR-0d note](../research/2026-11-08-opr-metadata-key-research.md) (read its "Decision" section first).
**Branch:** `opr/5-execution-records-definition` · **Lane C** (`workspace-5`) · **Blocks:** OPR-6, RFU-4, RFU-7.
**Needs:** Node tooling for the e2e edits and the Julia declaration test; no R or Julia.

## Start condition

The decision below is recorded, and Gordon flipped `status` to `ready` on 2026-11-09.
If the status is not `ready`, stop and report. RFU-1 must be `complete` (it lands first: `q2 convert` with `-M` and a
fixed default list, no option for the list) and OPR-3 must be `complete` (it edits the Julia hash declaration that Part A
edits again, so the two cannot run at the same time).

## Decision by Gordon on the metadata-key question (R5), 2026-11-09

**Move, not add.** The source list lives in metadata and there is **no command-line option** for it: `--records`
(`preview`, `provide-hub`) is deleted with no alias, and no command gains `--execution-records`. The command-line way to
set it is `-M`. The reasons and the evidence are in the research note. The decisions, all of them binding on this plan:

1. **Key and grammar.** `execute: record-sources:`, a **comma-separated scalar string** such as `positron,engine`. Not
   a YAML list: the runtime layer (`-M`, the provider overlay) is JSON, cannot carry `!prefer`, and a list from it would
   append to a document list. **Known limitation, a better solution is wanted later** (the provider reading the runtime
   layer directly, or `json_to_config_value` learning a way to say "prefer"); say so in the code comment and the notes.
2. **Entries.** `engine`, `q2`, `positron`, `cache`, or a directory. A directory is **inside the project only** (v1): a
   relative path resolves against the `base_dir` of the layer that set the value, a leading `/` is the project root, the
   result is normalized lexically, and a path outside the project is an error naming the entry. No `~`, no absolute
   paths, no existence fallback, no info diagnostic for absolute paths. A directory that does not exist is an error
   naming the entry. Say in the notes that a Positron cache outside the project has to be copied or linked in; the
   `positron` keyword still finds the default location itself.
3. **Per command allowed values and default.** `preview`: all five, default `cache,q2,engine`. `q2 convert`: `q2` and
   `engine`, default `q2,engine`. `provide-hub`: the host list (see Part B), default `engine`, no directories, no
   `cache`. `render` (OPR-6): refuses `cache`, default `engine`. **A valid entry the command cannot use is ignored with
   one info line per run** naming the entry and the command. **An unknown or malformed entry is an error** (the existing
   `ParseSourceError` text, reworded for the key). Defaults differ on purpose.
4. **Per document.** The list is resolved per document (this supersedes D6\'s "one list for all documents").
5. **Conflicts.** `--no-execute` and `execute: enabled: false` veto `engine` quietly (the entry is ignored; the existing
   `ExecutionPolicy::None` and `preview.engine: off` guards). `--replay` and `QUARTO_REPLAY` win over a committed key, with
   an info line, and are not an error (OPR-6 owns render).
6. **Hash.** The key is under `execute:`, so it counts in every engine document hash unless excluded. Exclude it in
   the three declarations. Record compatibility is not a concern for this epic (records were dropped a few days ago), so
   version bumps are free. The separate finding that `execute: record: true` itself changes the hash is **not** in scope.
7. **Untrusted authors (Part B).** `provide-hub` and a `--share` preview treat the host list as a ceiling.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
git switch -c opr/5-execution-records-definition feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/5-execution-records-definition
```

Part A is a complete, landable state on its own (the flag is gone, the key works for `preview`, `provide-hub` with a
host-only list, and `convert`). If Part B is blocked, follow "When you cannot finish" in the protocol and land Part A
with its notes; do not land a state in which `provide-hub` reads a guest key without the ceiling.

## The problem this plan solves

The list is needed **before any pipeline runs** (the `q2` source is read from disk, and the engine decision comes first),
which is the same problem as the output format, and the format already has the answer: `collect_metadata_layers` (in `metadata_layers.rs`) is the one collector of the layers (project, directory, document front matter, runtime), and
`format_resolution::resolve_document_format` reads one key from them without a pipeline, parsing only front matter,
cheap enough per keystroke, and compiling for wasm. (read) Reuse the collector; do not call the merge stage.

Today (read, before RFU-1) three commands declare the option and each has its own resolver: `preview` and `provide-hub`
in `crates/quarto/src/main.rs` (`records`, about lines 352 and 809) and `commands/preview.rs` (`resolve_record_sources`,
about 156) and `commands/provide_hub.rs` (about 184); the option name is in user-visible text, much of it asserted by
tests: `engine/diagnostics.rs:~289-323`, `quarto-preview/src/re_execute.rs:~154`, `capture_driver.rs:~1655`, `config.rs`,
`lib.rs:~271`, `quarto-hub-provider/src/execute.rs:~103,~145,~602,~841`, `commands/preview.rs:~169-178`, and the
`reject_cache` text in `record/sources.rs`. Already shared and kept: `RecordSource: FromStr`, `RecordSources` (`new`,
`preview_default`, `provider_default`, `reject_cache`), the walk and `RecordLookup` (`record/sources.rs`).

## Part A: the key, the early reader, the removal

- [ ] Re-run the inventory (`git grep -n -e '--records' -e '--execution-records'`, outside `claude-notes/` and `docs/`)
      and write the final list into "Notes from the run". (relied) 184 lines at the time of the review.
- [ ] **Tests first** (they fail for the stated reason before the code exists). In `quarto-core`, beside
      `format_resolution` tests: the layered key helper returns the highest layer that sets `execute.record-sources` and
      the layer kind and `base_dir`; the early reader picks project, directory, document and runtime values with the
      right precedence, and a `-M` value wins; per-command allowed values and defaults; an entry the command cannot use is
      ignored and reported once; an unknown entry is an error; a relative directory resolves against the declaring
      layer\'s `base_dir`, a leading `/` against the project root, `..` out of the project and a missing directory are
      errors; the early reader and the merged metadata agree for each layer (a test that merges the same layers).
- [ ] Add the **layered key helper** to `metadata_layers.rs` (highest layer that sets a key path; the value, its
      `LayerKind` and `base_dir`; a variant that ignores the runtime layer, for Part B). Add the early reader
      `resolve_record_sources(project, doc_path, content_override, runtime, &CommandSources)` beside
      `format_resolution` (target-neutral, wasm-clean), returning the list, the layer that set it and the notes
      (diagnostics) to report. The early reader has no `SourceContext`, so it resolves a directory with the layer\'s
      `base_dir`, not with `resolve_config_path` (which recovers the declaring file from a span); add a line to the
      conforming-sites inventory of `claude-notes/designs/path-resolution-model.md`. An extension\'s
      `contributes.metadata` that sets the key is read by the late merge but not by the early reader (the same
      early-versus-late gap the format resolver has, FR14): do not honor it, and say so in the notes.
- [ ] One `CommandSources` declaration per command (allowed values, default, directories allowed or not) in one place,
      and one message constant for the key name (`execute: record-sources`) used in every diagnostic.
- [ ] **Remove the option.** Delete `records` from `preview` and `provide-hub` in `main.rs`, the two
      `resolve_record_sources` functions and the `warn_engine_under_off` text that names the flag, and the clap
      `conflicts_with_all` on `--join`, `--static` and `--no-project` (those cases ignore the key, per "ignore what the
      command cannot use"). `--records` must be rejected as an unknown argument on both commands.
- [ ] **Wire the reader** into `q2 preview` (the walk in `capture_driver.rs` and `re_execute.rs`, which today read one
      list from `ReExecuteConfig.sources`: resolve per document; `allows_engine()` takes the document; the `--share` banner
      says whether guests can re-run from the ceiling, see Part B), `q2 preview --static` (accepts the key through the same
      path; OPR-6 wires the render side), `provide-hub` (Part A uses the host list only: the operator\'s `-M` value or
      `engine`, parsed once at startup and validated there; the guest document is not read until Part B), and `q2 convert`
      (RFU-1 gave it `-M` and a fixed default `q2,engine`; replace the fixed default with the reader, allowed `q2`,
      `engine`; `q2` alone means the document is never run).
- [ ] **Hash exclusion** of `record-sources` (an `execute:` key) in the three declarations, test first (each test fails
      before: the document hash changes when the key is added): Jupyter (`engine/jupyter/mod.rs`,
      `jupyter_hash_definition`: add it to `presentation_execute_keys` only, not to the cell options), knitr
      (`engine/knitr/hash_rule.rs`: `presentation_execute_keys` of the default declaration, bump `KNITR_HASH_VERSION`,
      update the tests that assert the lists are empty), and Julia (`record-wire.ts` in `resources/extension-subtrees/julia-engine/src/`, `presentationExecuteKeys`, bump its `version`, extend `record-wire.test.ts`, which pins the
      declaration; OPR-3 edited the same declaration, so rebase first and follow the procedure its notes record for
      the subtree and the fork). The push of the Julia engine fork is Gordon\'s companion step; say so in the landing
      report.
- [ ] Rename in every message, comment and test that names the old option. Update the TypeScript e2e files:
      `q2-preview-spa/e2e/helpers/previewServer.ts` and `q2-preview-spa/e2e/records-sources.spec.ts` pass
      `-M` with `execute:{record-sources: ...}` instead of the flag; run the records-sources e2e if it can run here and
      say which ran.
- [ ] **Do not edit `docs/`** here; OPR-8 owns every docs change and the final `git grep` below excludes `docs/`.
- [ ] **Real binary.** `q2 preview <doc> -M 'execute:{record-sources: positron,engine}' --no-browser` starts and the
      walk follows that order; a document whose front matter says `execute: {record-sources: q2}` is honored when no `-M`
      is given and overridden by `-M`; `q2 preview <doc> --records positron` fails as an unknown argument; a directory
      outside the project is an error naming it. Record the invocations and the output you read.
- [ ] Gates for Part A: `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`; `cargo nextest run` for the same crates; the Windows cross-clippy for each;
      `cd hub-client && npm run build:wasm` (`metadata_layers` and the early reader compile for wasm); `cargo xtask lint`.

## Part B: the ceiling for untrusted authors (provide-hub and a --share preview)

**Why.** In the provider a guest\'s whole project is materialized on the host and its `_quarto.yml`, `_metadata.yml` and
front matter take effect; imported sources (`positron`, a directory) need no consent, so a guest key naming one would make
the host read its own cache and publish what matched to every guest. A `--share` preview has the same shape: its render
routes (`POST /api/preview/render` and the downloads) take a guest-supplied `content` and `metadata` (the runtime layer,
the highest precedence), and execution reads the host disk, which a guest reaches with `--allow-edit`. So the rule
applies whenever `--share` is on, not only with `--allow-edit`. No other command needs it: there the author is the
person running the command, and `-M` is a plain override.

**Rule (the same in both).** The allowed set is the host\'s `-M` value, or the command default when there is none. A
guest document may **reorder or narrow** it (drop items, any order, duplicates removed) and may not add to it. An entry
outside the set is ignored with a **warning-level** diagnostic. If nothing is left, the document is inert with the D6
warning; in the provider that is an error whose text names the reason (today an empty list prints "no record to publish
from " with nothing after it).

- [ ] **Tests first.** Provider (`crates/quarto-hub-provider/tests/integration/execute.rs` style): with the default
      allowed set, a guest key of `positron,engine` runs `engine` only and warns; a guest key that narrows to nothing
      yields the named-reason error; with an operator `-M` of `positron,q2,engine` the guest may reorder and drop; a guest
      directory entry is ignored. Preview config tests (no tunnel needed): with `share` set, a document key outside the
      ceiling is ignored with a page diagnostic, and `allows_engine` follows the ceiling; with `share` unset the key is an
      ordinary document setting and `-M` overrides it.
- [ ] Add a trust parameter to the early reader: `Override` (the plain `-M` rule) or `Ceiling { allowed }`, which reads
      the guest value from the layers **below** the runtime layer (the provider already holds the `plain` runtime, with no
      overlay, and uses it for the source view), intersects it with the allowed set and keeps the guest order.
- [ ] **Provider.** The host list is parsed once at startup from the operator\'s `-M` (typed `RecordSources`, validated
      there, directory entries ignored with an info line), default `engine`; resolve per document inside
      `walk_and_publish` right after `ProjectContext::discover`; the shortcut in `execute_document` (default list means
      `run_and_store`, no walk) must read the guest key too or a guest cannot narrow; the out-of-set warning goes to the
      operator log and is appended to `last_error` when the run fails. There is no client route for a warning on a
      successful run: strand `bd-ia3idnvk` tracks a client-visible `warnings` field on the capture sidecar. Do not
      work on it here.
- [ ] **`--share` preview.** `ReExecuteConfig.sources` becomes the ceiling when `config.share` is set; the out-of-set
      warning goes through `diagnostics::current_sink()` for the page; the share banner\'s "guests can re-run" claim is
      computed from the ceiling.
- [ ] The preview **render routes** are OPR-6\'s: leave them, and record in the notes that OPR-6 must use the ceiling
      there.
- [ ] Gates for Part B: the Part A gates, plus `cargo nextest run -p quarto-hub-provider -p quarto-preview`.

## Exit criteria

- `git grep -n -e '--records' -e '--execution-records'` outside `claude-notes/` and `docs/` prints nothing.
- One reader, one per-command declaration table; `--records` rejected on `preview` and `provide-hub`.
- The key is in no engine document hash (three tests).
- `provide-hub` and a `--share` preview never run or read a source the host did not allow, shown by the Part B tests.
- Real-binary runs recorded.

## Companion (Gordon, outside this repository)

The Julia engine fork needs the `record-wire.ts` edit pushed. The Positron fork extension (`q2Contract.ts`
`buildPreviewArgs`, `previewCommand.ts`, their vitests, its e2e) passes the old option; RFU-7 updates it once this plan
and RFU-1 have landed. Not part of this plan.

## Notes from the run

(Fill in at landing.)

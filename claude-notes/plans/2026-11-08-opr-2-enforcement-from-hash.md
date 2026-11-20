---
title: 'OPR-2: who enforces warning and message, derived from the hash definition'
date: 2026-11-08
description: 'One rule replaces knitr_visibility and CellVisibility::resolve: the presenter enforces warning and message exactly when the record declares them display-only for the cell engine; echo, output and include stay always presenter-enforced; an engine with no usable declaration, and an imported record, get presenter enforcement. Includes an audit of what each engine really enforces at capture.'
status: complete
depends-on: [opr-1b, opr-3]
lane: A
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/2-enforcement-from-hash` · **Lane A** (`workspace-3`) · **Blocks:** OPR-7c.
**Needs:** R 4.3 with knitr and Julia 1.12 for the audit runs (the audit boxes say what to do when absent).

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd
git switch -c opr/2-enforcement-from-hash feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/2-enforcement-from-hash
```

## Goal (decision D2)

Today two functions decide who hides a cell\'s warnings: `CellVisibility::resolve` (`presenter/visibility.rs:~87`) filters
stderr for Jupyter, and `knitr_visibility` (`visibility.rs:~113`) deliberately does not read `warning` ("enforced by
R at capture"). That is why spike S2 saw one golden differ (`warning-false-stderr`) once a Jupyter record took the
knitr entry path. After OPR-1a and OPR-1b there is one path; this plan gives it one rule.

**The rule.**
1. For `warning` and `message`: the presenter enforces the option **exactly when** the record\'s hash definition for
   the cell\'s engine (`quarto_engines[].hash_definition`) lists it as display-only. When the option is in the hash,
   capture enforced it and the presenter leaves the output alone.
2. `echo`, `output` and `include` stay **always presenter-enforced** (hiding what capture already hid is harmless,
   and knitr records the outputs, so the presenter must hide them).
3. An engine with **no usable declaration** (a TS engine that sends none gets `HashDefinition::conservative`,
   `engine/ts_protocol.rs:~587`, which lists nothing as display-only) and an **imported Positron record** (no engine
   entries) get **presenter enforcement** of everything. The unsafe failure (showing what the author hid) must not
   be the default.

**Why not "in the hash means capture enforced it".** A hash is conservative: listing too much is safe (extra
re-runs), so "in the hash" means "might matter", not "capture handled it". Julia\'s declaration, for instance,
lists every option as hashed while its capture enforces almost nothing; OPR-3 corrects that declaration, which is why
this plan depends on it.

## Facts (read unless marked)

- Jupyter: `DISPLAY_ONLY_KEYS` (`engine/jupyter/mod.rs:~251`) lists `echo`, `output`, `warning`, `include`;
  `message` and `error` are not listed (`error` changes whether a failure aborts; Jupyter has no `message` option).
- Knitr: the structured profile\'s presentation list (`engine/knitr/hash_rule.rs`) contains `echo`, `results`,
  `collapse`, `prompt`, `comment`, ... and never `include`, `warning`, `message` or `output` (a test asserts it), so
  those four are hashed and `option_sweep.rs` marks them `Hashed`. (relied, one agent\'s run, 2026-11-08) R records
  no warning or message output under `warning: false` or `message: false`, but records outputs for `include: false`
  and `output: false`, which the presenter hides.
- Julia: `JULIA_HASH_DEFINITION` (`resources/extension-subtrees/julia-engine/src/record-wire.ts:15-27`) has empty
  `presentationCellOptions` and `presentationExecuteKeys`; (relied) capture enforces only a cell\'s own `#|` `warning`. OPR-4a confirmed this by running the runner: under `warning: false` it drops **all** stderr at capture (`@warn`, `@info` and raw stderr), so with OPR-3\'s display-only declaration a Julia record taken under `warning: false` holds nothing for the presenter to show. Not fixed here: the fix is in OPR-4b (runner keeps stderr under `emitUnits`). Until it lands the audit should record this Julia gap, and D2 holds for Julia only after OPR-4b.
- (relied) Marimo and the TS fixture engines declare `[]`. Marimo uses the legacy capture splice and is a documented
  exception to this epic (decision D12): record in the notes what its path does and do not change it.
- Knitr\'s `execute.R` sets `message = isTRUE(format$execute[["warning"]])`, the same as Quarto 1\'s line 320
  (checked, nothing to fix).

## Checklist (tests first; every box is `[unattended]`)

- [x] **Audit, by running.** For Jupyter (stored notebook or kernel if available), Julia and knitr: for each of
      `warning`, `message`, `error`, `include`, `output`, `echo`, with the option on a cell and at document level,
      record whether capture or the presenter hid the output (look in the record, then in the HTML). Report any
      option whose declaration and behavior disagree. Do not fix a capture gap in this plan: record it in the notes
      and in a section "Findings for Gordon"; the rule below already makes the presenter safe.
- [x] **Tests first** (partial matrix: `warning` and `message` across display-only, capture-enforced, undeclared and
      imported declarations, plus `echo`/`output`/`include`; not every option on every engine fixture), each failing for the stated reason: for every engine and every visibility option, the option is
      enforced exactly once overall (capture or presenter) on a fixture; `warning-false-stderr` passes through the
      one path; a cell with an undeclared engine and an imported record are presenter-enforced; a knitr cell with
      `warning: false` is not filtered twice; `message: false` on a Jupyter cell does what it does today (nothing).
- [x] Implement the rule; delete `knitr_visibility` and `CellVisibility::resolve` (or reduce to the one rule).
- [x] Update the sweeps: `presenter/jupyter_option_sweep.rs` (a display-only option that changes nothing fails the
      build) and the knitr `option_sweep.rs` `Hashed` rows still agree with the rule.
- [x] Reference `bd-nn2fou8h` (execute-visibility options ignored; in progress) in the notes; do not change its status.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core` (not runnable on macOS: build scripts fail); `cd hub-client && npm run build:wasm` and the hub-client record tests.

## Exit criteria

- One enforcement rule; the two old functions are gone.
- The audit table is in "Notes from the run" with a line per engine and option, marked ran or read.
- `warning-false-stderr` passes on the one path; all other goldens are byte-identical; OPR-0a tests pass.
- Findings for capture gaps (if any) are written up for Gordon; none was papered over.

## Notes from the run

**Landed.** One rule, `HashDefinition::presenter_enforces(profile, option)` in `record/hash_definition.rs`, read through
`presenter_enforces_warning(cell, engine)` in `presenter/knitr_inputs.rs` and applied by one constructor,
`CellVisibility::from_flags(flag, presenter_enforces_warning)` (`presenter/visibility.rs`).
`CellVisibility::resolve(resolved, flag)` is a thin wrapper over it for `ConfigValue` options; `knitr_visibility` is gone
(its re-exports in `engine/mod.rs` and `knitr/pieces_map.rs` too). `CellInputs` gained `capture_enforced_warning` (its
default `false` is presenter enforcement, so a struct with every input absent still follows rule 3); `present_cell_step`
and the expansion path pass the same flag, so the knitr branch, the fallback branch and expanded cells all use it.

**The rule as built.** `warning` is presenter-enforced unless the cell\'s engine entry declares something and leaves
`warning` out of the lists of the profile the cell names (`quarto_hash_rule`, else the default lists; cell and `execute:`
lists are read together). "Declares something" means any non-empty presentation list in the default or any profile.
This is how a bare `HashDefinition::conservative` (a TS engine that sent nothing) gets presenter enforcement while knitr,
whose default profile is empty but whose `structured` profile is not, reads as "capture enforced it". A record with no
entry for the cell\'s engine (imported Positron) gets presenter enforcement. `echo`, `output` and `include` are read
unconditionally, as before. `message` has no switch: no engine declares it display-only, and a bare stderr stream cannot
be told from a warning, so `message: false` changes nothing in the presenter for any engine (test pins it).
Marimo and the TS fixture engines declare `[]`, so they read as undeclared and get presenter enforcement; marimo\'s
legacy capture splice is untouched (D12).

**Tests (red first).** New in `presenter/cell_step.rs`: `warning_false_is_enforced_once_when_the_engine_lists_it_display_only`,
`warning_false_is_not_filtered_again_when_capture_enforced_it`, `a_cell_with_resolved_options_follows_the_same_rule`,
`an_undeclared_engine_gets_presenter_enforcement`, `an_imported_record_with_no_engine_entry_gets_presenter_enforcement`,
`the_document_scope_warning_follows_the_rule_too`, `message_false_changes_nothing_for_any_engine`,
`echo_output_and_include_stay_presenter_enforced_for_every_declaration`; in `record/hash_definition.rs` four
`presenter_enforces` tests; one test each in `knitr/hash_rule.rs` (warning and message are capture-enforced in both
profiles) and `jupyter/mod.rs`. I ran them against a stub that returned `true` and the unchanged presenter: 8 of 15 failed,
each for the stated reason (a knitr-style unit double-filtered; a resolved-options cell and an undeclared or imported cell
kept the stderr the author hid). The pass-from-the-start ones (`message_false`, `echo_output_and_include`, display-only
Jupyter) pin behavior that must not change. `shadow_goldens_through_the_knitr_entry_path` was flipped, as its comment
promised: it now requires all 13 goldens identical through the knitr entry path (it failed once on the old expectation, the
red-first evidence for the flip).

**Sweeps.** `jupyter_option_sweep.rs` needed no change (`warning`, `output` and `include` are still `Presents`: its entry is
undeclared, which presents). The knitr `option_sweep.rs` `Hashed` rows for `warning` and `message` agree with the rule; the new
`hash_rule` test is what ties them (the sweep itself is unchanged).

**Audit (ran unless marked).** `q2 render` of a one-cell document per row, `execute: {record: true, daemon: false}`, an
option on the cell and the same option at document level; each cell warns, messages, writes stderr (Julia), prints and
(for `error`) fails. Capture = the record\'s outputs; HTML = `doc.html`. The HTML columns come from the `q2` built
before this change, so they show the old presenter; the new one differs only for the rows named under Findings.
Scripts are in the session scratchpad, not committed.

| Engine | Option | Cell level | Document level | Declaration |
|---|---|---|---|---|
| knitr (ran) | `warning: false` | capture dropped warning **and message** | same (`execute.R` sets `message = isTRUE(warning)`) | hashed (capture): agrees |
| knitr (ran) | `message: false` | capture dropped the message | not applied by anyone (`message` is not an `execute:` key, as in Q1) | hashed: agrees |
| knitr (ran) | `error: true` | error output recorded | same | hashed: agrees |
| knitr (ran) | `include: false` | outputs recorded, presenter hid them | same | hashed, always presenter-enforced: agrees |
| knitr (ran) | `output: false` | record lost warning and message, kept the printed text; presenter hid it | same (D3: R baked it in) | hashed, always presenter-enforced: agrees |
| knitr (ran) | `echo: false` | record unchanged, presenter hid the source | same | in the structured list: agrees |
| Julia (ran) | `warning: false` | **capture dropped all stderr** (warn, info, raw) | not applied at capture; presenter hid it | **display-only: disagrees at cell level** (Finding 1) |
| Julia (ran) | `message: false` | nothing changed | nothing changed | no such option |
| Julia (ran) | `error: true` | error output recorded | same | hashed: agrees |
| Julia (ran) | `include`, `output`, `echo` | record unchanged, presenter hid | same | display-only: agrees |
| Jupyter (read, plus goldens ran) | `warning`, `output`, `include`, `echo` | no kernel on this machine (`jupyter` is not installed); the stored-notebook goldens (`warning-false-stderr`, `output-false`, ...) show the presenter hides each | scoped goldens in `jupyter_markdown_goldens` | `DISPLAY_ONLY_KEYS` lists all four: agrees |
| Jupyter (read) | `message`, `error` | no `message` option; `error` read by the executor | | absent from the list: agrees |
| Marimo (read) | all | legacy capture splice, unchanged (D12) | | declares `[]` |

**Findings for Gordon.**

1. **Julia cell-level `warning: false` is enforced at capture although the declaration says display-only** (re-confirms
   OPR-4a, ran). The record then holds no stderr, so flipping the cell to `warning: true` leaves a record that still reads fresh
   (the key is not in the hash) but cannot show the warnings. The presenter rule is safe (it hides what the author hid), but
   D2 holds for Julia only after OPR-4b (runner keeps stderr under `emitUnits`). Not fixed here.
2. `execute: message: false` does nothing in knitr and Julia; same in Q1 (not a key there), so I recorded it and did not
   change it. `bd-nn2fou8h` (execute-visibility options ignored; in progress) is the strand about the neighboring options;
   I did not touch its status.
3. A **bare conservative declaration is treated as undeclared.** A TS engine that declares empty lists but enforces
   `warning` at capture gets a second, redundant filter in the presenter. That is harmless (what capture dropped is
   absent) and it is the safe direction. An engine that wants the "capture enforced it" reading must declare at least one
   display-only option. Worth one line in the engine-author docs when the handshake lands (OPR-7a).

**Gates.** `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean. `cargo nextest run -p quarto-core
--no-fail-fast`: 8080 run, 8079 passed, 36 skipped, 1 failed: the environmental `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`.
Baseline: OPR-1b ended at 8061 run plus its five new tests (8066); this plan adds 14 tests (8 in `cell_step`, 4 in
`hash_definition`, 1 in `hash_rule`, 1 in `jupyter`), so 8080 accounts for all of it with nothing lost or duplicated.
The six `julia_engine_e2e` tests `j1` to `j6` that failed in OPR-1b **passed** here (j7 and j8 too): they do not relate to this
plan. OPR-3\'s note found the same symptom (a stale detached runner server whose working directory was gone) and the same
transient cure; I did not investigate further, and RFU-5 owns it. Windows cross-clippy: fails in the `wasmtime` and
`tree-sitter` build scripts on macOS, so not run (no platform API or path literal was added). `cd hub-client && npm run build:wasm`: green (rc 0; the seven `quarto-core` warnings it prints are dead-code warnings in files this plan did not touch). `npx vitest run --config vitest.wasm.config.ts recordPayload.wasm.test.ts captureSpliceHtml.wasm.test.ts`: 8 of 8 pass.

**Snapshots and goldens.** None added, changed or removed. One test expectation changed (`shadow_goldens_through_the_knitr_entry_path`,
above). No golden was regenerated.

**Strands.** None opened. `bd-nn2fou8h` referenced, status untouched.

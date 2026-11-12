---
title: 'Review: Preview with q2 and Show Outputs (implementability)'
date: 2026-11-12
description: 'An implementability review of the draft plan for the fourth q2-records-demo command. Two read-only sub-agents covered the q2 side and the extension side; every finding below that would change the plan was re-checked against the code.'
---

**Plan reviewed:** [2026-11-12-preview-with-q2-and-show-outputs.md](../plans/2026-11-12-preview-with-q2-and-show-outputs.md)
at `989bbd5ef` (branch `plan/preview-with-q2-and-show-outputs`). Fork read at
`/Users/gordon/src/positron-riu6-a`, branch `q2/rfu-7-demo-followups`, untouched. Nothing was run; all evidence is
from reading code. "Verified" means I opened the cited lines myself.

## Verdict

**Implementable with changes.** The changes are now in the plan (see "Gordon's answers"). Gordon's decisions all hold up; none is blocked by the code. But Phase 1 as written
("give the engine-run sites the overlay runtime") is not enough. Three confirmed q2 bugs sit on the plan's own path.
Each one breaks the "push every run" promise:

1. the record hash counts `execute.record`;
2. re-execute reads the capture cache even under `record-sources: engine`;
3. the staleness flag can get stuck after a failed run, or after a save during a run.

On the extension side, the plan does not yet say how to find the copy's path, how to tell a new copy from an old one,
or how to avoid reading a half-written copy.

## Findings, by severity

### 1. High: the record hash counts `execute.record`, so the `-M` wiring makes every event look stale

**Evidence (verified).**

- `hash_document`\'s doc comment (`crates/quarto-core/src/record/exec_hash.rs:276-277`) says "Core always excludes
  `record` (the opt-in of epic decision 24), so toggling it never changes a hash".
- The code filters only a *top-level* `record` out of `document_keys` (`:288-292`). The `execute:` map is filtered by
  the engine's presentation keys only (`:300-307`).
- Jupyter's list (`crates/quarto-core/src/engine/jupyter/mod.rs:307-313`) adds `record-sources` but not `record`.
  The only test, `toggling_record_changes_no_hash_even_when_an_engine_declares_it` (`exec_hash.rs:856`), uses a
  top-level `record`.
- So `execute: {record: true}`, which is the decision-24 spelling and the one this plan passes with `-M`, changes the
  document hash.
- The engine stage hashes the merged metadata, which includes the runtime overlay. `record/overlay.rs:1-9` says so and
  is why `provide-hub` calls `rehash_record` (`crates/quarto-hub-provider/src/execute.rs:816`).
- Staleness recomputes "on the masked, overlay-free pre-engine document" (`crates/quarto-preview/src/capture_driver.rs:410-414`).

**Failure.** Suppose Phase 1 gives the engine run the overlay and leaves staleness as it is. The record then holds a
hash with `execute.record: true`, and every recompute hashes without it. Any file event, even a touch or a prose edit,
flips the flag and fires an auto re-execute. The same goes for any other execution-affecting `-M` key. For engines on
the conservative hash definition it also goes for `record-sources` itself; that part is inferred, since I did not trace
the Julia engine's declaration.

**Proposed edits.**

- Phase 0: add a unit test that `hash_document` ignores `execute.record`, and fix the filter in Phase 1. This is a
  q2-wide change (see question 1).
- Phase 1: decide how the preview's staleness sees the overlay. Two consistent options:
  - (a) the preview calls `rehash_record` against the overlay-free source metadata after each engine run, as
    `provide-hub` does. This keeps the EXR-15 "overlay-free" contract.
  - (b) staleness runs under the same overlay runtime, and the `capture_driver.rs:412` comment is revised.

  Option (a) matches the existing design. With (b), the `.quarto` copy carries `-M`-dependent hashes, and a later plain
  `q2 render` would see the copy as stale.
- Phase 0: add a test with a recording stub engine and `-M execute:{record: true}` on a document with no `record:` of
  its own. Run the startup capture, then call `recompute_staleness` on a no-op event and assert the flag stays
  `Some(false)`.

### 2. High: re-execute consults the capture cache even under `record-sources: engine`

**Evidence (verified).**

- `perform_re_execute` builds its own plain runtime (`crates/quarto-preview/src/re_execute.rs:312`) and calls
  `record_capture_run_cached` (`:316-322`).
- That function returns a cached run before any engine call (`crates/quarto-preview/src/cache.rs:243-248`). It never
  looks at the record sources.
- Finding 4 of the plan ("the copy is only written and never read") is true at startup. It is false for re-execute.

**Failure.** Edit a cell and save, and the copy updates. Undo the edit and save again. The `input_qmd` key matches the
earlier run, so the cache hits, no engine runs, and `write_record_file` is never reached. The `.quarto` copy keeps the
undone version's outputs, and the extension sees no change and pushes nothing. The Viewer's Re-execute button is a
silent no-op in the same case.

**Proposed edits.**

- Phase 1: `perform_re_execute` takes the runtime from its caller instead of building one. It reads the cache only
  when `cache` is in the document's resolved sources (`cfg.document_sources`).
- Phase 0: add a test that edits, reverts and asserts the copy is rewritten.

### 3. High: the staleness flag sticks after a failed run, or after a save during a run

**Evidence (verified).**

- `recompute_staleness` returns before the auto trigger when the flag does not change (`capture_driver.rs:437-439`).
- The error path of `claim_and_spawn` keeps `staleness: existing.staleness`, which is `Some(true)`
  (`re_execute.rs:262-268`).
- The success path sets `Some(false)` unconditionally (`re_execute.rs:334-338`).
- A trigger that arrives while a run is in flight is dropped (`re_execute.rs:218-220`, `AlreadyInFlight`).

**Failure.**

- *A failed run.* A save with a typo runs the engine, and the run fails, so the flag stays `Some(true)`. The save that
  fixes the typo computes stale again, finds no flip, and returns. Nothing re-runs until someone presses Re-execute.
  This is bd-twox482l's mechanism, reached without a restart. In live editing it is the common case, not an edge.
- *A save during a run.* The save's trigger is skipped as in flight. The run finishes against the older file, sets the
  flag to `Some(false)`, and the newest edit never runs.

**Proposed edits.**

- Phase 1: on Auto, trigger whenever the document is stale, not only on a flip. The in-flight guard already collapses
  repeats. Then, after a run finishes, recompute staleness once and re-trigger if the document is stale.
- Phase 0: add a failing-engine test and a save-during-run test.
- Either fix bd-twox482l in this plan or link it: the same change covers it. See question 2.

### 4. Medium: the Phase 0 edit test, as written, depends on the macOS watcher

**Evidence (verified).** The only existing watcher-driven staleness test is `#[ignore]`d for bd-9brz
(`crates/quarto-preview/tests/integration/staleness.rs:88-96`). The in-crate tests drive `recompute_staleness`
directly. The sub-agent cited `capture_driver.rs` around `:2640` for that; I did not re-open those lines.

**Proposed edit.** Phase 0's first test writes the file and calls `recompute_staleness` directly with an overlay
runtime, `EnginePolicy::Auto` and a recording stub engine. It asserts on the copy. A watcher-driven variant, if wanted,
is `#[ignore]`d with the bd-9brz reason. The test for finding 3 needs only the startup capture (`eager_capture.rs` is
the precedent). The `preview.engine` test is a pure unit test of the new helper (finding 6).

The integration binary cannot reach `src/test_support.rs` (it is `cfg(test)`), so the recording stub is written
in-file, as `record_gate.rs` does. A module that boots the server must join the watcher test group in
`.config/nextest.toml`. These two points are inferred from the sub-agent's reading, not re-checked.

### 5. Medium: the extension cannot tell where the copy is, which copy is new, or whether it is whole

**Path (verified).**

- `execution_record_path` (`crates/quarto-core/src/record/location.rs:20-27`) gives
  `<project_dir>/.quarto/execution-records/<project-relative path>.ipynb`.
- `project_dir` is the nearest `_quarto.yml`/`_quarto.yaml` ancestor, canonicalized. Without one, it is the document's
  folder. The canonicalization and walk-up are from the sub-agent's reading.
- The extension spawns q2 with `cwd = dirname(doc)` (`extensions/q2-records-demo/src/previewCommand.ts:77` in the fork).
- So the plan's "the extension must agree" needs a mechanism. The extension walks up for either config filename,
  `realpath`s the root (macOS `/tmp` vs `/private/tmp`), and compares case-insensitively on Windows.

**Not atomic (verified).**

- `write_record_file` (`crates/quarto-core/src/stage/stages/engine_execution.rs:1249-1278`) calls `file_write`.
- Native `file_write` is `fs::write` (`crates/quarto-system-runtime/src/native.rs:190-198`), which truncates and then
  writes.
- A watcher event can therefore hand `q2 convert` an empty or partial file.

**New versus leftover (verified).**

- The serializer is deterministic (`write_record_file`\'s doc comment, `engine_execution.rs:1245-1248`), so a re-run with
  the same outputs writes identical bytes. Only the mtime tells runs apart.
- A copy from an earlier session already exists when the command starts.
- The first engine run can finish before the watcher exists.

**Proposed edits.** These are Phase 2 items, each with a vitest on a pure function.

- *Copy path.* `recordCopyPath(doc)` uses the cases of the Rust tests in `location.rs`: standalone, project root,
  project subfolder, and `a.b.qmd`.
- *Start protocol.*
  1. Create the watcher and a ~1 s stat poll before `spawn`, and record `t0`.
  2. Accept a copy only if `mtime >= t0 - 1 s`.
  3. Stat once more when q2 prints its URL.
  4. Remember the last `(mtime, size)` handled, so the watcher and the poll do not both push.
- *Stability and retry.*
  1. Debounce ~300 ms.
  2. Convert only when size and mtime are unchanged across ~100 ms.
  3. Retry on convert exit 12 (`INVALID_RECORD`) or 10 (`NO_RECORD`) with backoff.
  4. Notify only after the last retry, and rate-limit repeats.

  An atomic temp-and-rename write in q2 needs a new `SystemRuntime` method, which is out of plan. File it as a strand
  if wanted.
- *Watcher form.* Use a `RelativePattern`, not a string glob: a string glob drops events outside the workspace folders
  (`src/vs/workbench/api/common/extHostFileSystemEventService.ts`, `excludeOutOfWorkspaceEvents`, verified). The
  default `files.watcherExclude` does not exclude `.quarto` (`files.contribution.ts:302-310`, verified), and
  `.gitignore` is not consulted. The poll also covers a `.quarto/execution-records/` that does not exist yet.

### 6. Medium: `preview.engine` from `-M` needs a helper, a typo check, and two scope calls

**Evidence (verified).** `crates/quarto/src/commands/preview.rs:380-384` resolves the policy from `_quarto.yml` only,
and `Manual` without a project. The following points are from the sub-agent and were not re-checked:

- `parse_policy_str` maps an unknown string to `Manual` silently (`crates/quarto-preview/src/config.rs:276`).
- `preview_static.rs:448` calls `read_engine_policy_from_project` too.
- Nothing applies the `--share` ceiling to the policy.

**Proposed edits.** Phase 1 item:

- A pure helper reads `preview.engine` from the `-M` JSON and wins over `_quarto.yml`, the same precedence as
  `record-sources`.
- An unknown value is an error, as `resolve_record_sources` does it.
- It runs before `warn_engine_under_off`.
- `--static` either shares it or the plan scopes it out explicitly.
- With `--share` and Auto, print a startup warning that guest edits run host code.

### 7. Medium: the Viewer's big X leaks a live session

**Evidence (verified).** `previewCommand.ts:93-94` kills q2 on `onDidRequestStop` and `onDidDispose`. The RFU-7 plan
records that the big X does not dispose the panel (`claude-notes/plans/2026-11-08-rfu-7-positron-fork.md:81-83`). For
command 3 that leaks an idle server. For this command, the leaked q2 keeps running the kernel on every save and
pushing into the editor, with nothing on screen to say so.

**Proposed edit.** Phase 2:

- one live session per document (a second invocation stops the first);
- a visible way to stop a session whose Viewer is gone;
- the watcher, poll, temp directory and child disposed together.

The form of the visible stop is question 3. The e2e checks that no q2 or kernel process remains after Stop.
`q2 preview` shuts kernels down politely on SIGTERM (`preview.rs:196-204`, comment verified), so `child.kill()`\'s
default signal suffices on macOS and Linux. On Windows it does not.

### 8. Medium: Phase 1 and Phase 2 items are too large, and docs are missing

- Split Phase 1 into:
  1. the session runtime threaded through all engine-run sites (`lib.rs:447`, `lib.rs:505`, `re_execute.rs:312`,
     verified as the three plain-runtime engine sites);
  2. the hash and rehash fix (finding 1);
  3. re-execute honoring the record sources (finding 2);
  4. the staleness re-trigger (finding 3);
  5. the `preview.engine` helper (finding 6).
- Split the Phase 2 "register" item into: spawn and Viewer; a shared convert-and-push function (`buildExportArgs`,
  `q2Contract.ts:35-38`, takes no `-M` today, verified); and the watcher wiring.
- Add a Phase 1 docs item:
  - `docs/guides/projects/preview.qmd:54-55` says `-M` "do\[es\] not reach code execution". That becomes false
    (verified).
  - Document `preview.engine` via `-M` there.
  - Revise the header comment of `render_flags.rs` and the "overlay-free" comment at `capture_driver.rs:412`.
- Add `-M` precedence tests:
  - `-M execute:{record: true}` over front-matter `record: false`;
  - `-M record-sources: engine` over a document's own list;
  - both under `--share`.

### 9. Low: extension housekeeping the checklist omits

All verified in the fork:

- `package.json` needs the command in `activationEvents` and `contributes.commands`. Its `description` says "Three
  commands".
- The live command needs a per-session temp directory. `exportCommand.ts` deletes its temp directory per run, but the
  queue must keep the newest notebook.
- The e2e helpers `runFromPalette`, `warmJuliaServer` and `rProcesses` are module-local in
  `test/e2e/tests/quarto/quarto-render-and-push.test.ts:15,28,199`. Either the new tests live in that file or the
  helpers move out.
- `afterEach` there does not remove `.quarto/` from the fixture workspace.
- The fixtures print constants, so the e2e edit must change printed text, and it asserts the old text is gone.

Also from the sub-agents, not re-checked:

- The demo folder needs a `.gitignore` for `.quarto/` and `*_files/`, so the live command does not dirty the fork.
- In a project, `-M record: true` makes the startup capture write a copy for every document, so the command should be
  documented for standalone files.

### 10. Low: the queued push has no target check of its own

**Evidence (verified).** `_positronQuarto.pushOutputsFromFile` reads `activeTextEditorControl` once and pushes into
whatever code editor is active (`src/vs/workbench/contrib/positronQuarto/browser/quartoPushOutputs.ts:49-73`). The only
guard is the extension's URI check (`exportCommand.ts:92-96`).

**Proposed edit.** Answer 3 as decided is implementable:

- Release the queue on `onDidChangeActiveTextEditor` when the URI matches, and treat `undefined` as "not the
  document".
- Push at once if the document is already active.
- Drop the queue on `onDidCloseTextDocument`.
- Name the remaining time-of-check gap in the README.

Command 2 and this command can both push into one document. Refuse command 2 while a live session owns the document,
or document it as a limit.

## Gordon's answers (2026-11-13)

The draft asked five questions. Two of them, 1 and 4, mixed two problems, so they are restated here as Gordon answered
them. All answers are now decisions 8 to 11 of the plan, and the plan was rewritten to match on 2026-11-13.

1. **Hashes: fix both.** Problem A is the core filter: `hash_document` counts `execute.record` against its own doc
   comment, a bug with or without `-M`. Problem B is the preview's mismatch: an overlaid engine run against an
   overlay-free staleness check. The plan fixes A in core and fixes B by sharing `provide-hub`\'s `overlay_free_run`
   with the preview (the option (a) of finding 1). Existing copies written with `execute: {record: true}` read as stale
   once after A.
2. **bd-twox482l goes into Phase 1.** The fix "under Auto, trigger whenever stale; recompute after each run" covers it
   and both live-editing cases of finding 3. The strand closes with that commit.
3. **A status-bar item** stops a live session, with one session per document.
4. Folded into answer 1.
5. **`--static` shares the `preview.engine` helper.**

## Sub-agent claims: confirmed, downgraded, dropped

**Confirmed** by opening the lines:

- the three plain-runtime engine sites;
- the record hash counts `execute.record`;
- the overlay folds into the stage's hash (`overlay.rs` header);
- the cache hit in re-execute;
- the error path keeps the flag, and the success path resets it unconditionally;
- the early return before the auto trigger;
- the `preview.rs` policy read;
- the `preview.qmd:54-55` wording;
- the `staleness.rs` `#[ignore]`;
- `execution_record_path`;
- the non-atomic `fs::write`;
- the push command's active-editor read;
- the extension spawn's `cwd`;
- the RFU-7 big-X note;
- the default `files.watcherExclude`;
- the string-glob out-of-workspace filter;
- the `package.json` omissions;
- the module-local e2e helpers.

**Downgraded to inferred** (cited, not re-opened):

- the `parse_policy_str` fallback;
- `preview_static.rs:448`;
- the absence of a `--share` ceiling on the policy;
- the nextest watcher group;
- the unreachable `test_support.rs`;
- project-root canonicalization;
- the deterministic serializer's byte-identical output (I read only the doc comment);
- the conservative hash definition not excluding `record-sources` for Julia;
- the browser's WASM render seeing `-M execute:{record: true}` without emitting an "Execution record not written"
  diagnostic. This one is worth a check in the Phase 1 end-to-end step.

**Dropped:**

- *"The extension has no way to find the project root."* It can walk up for the config file, and the plan already
  requires agreement. I kept the substance (finding 5) and dropped the severity.
- *"A SIGTERM may orphan kernels."* `q2 preview` handles SIGTERM politely. Only Windows `TerminateProcess` remains,
  which is out of scope for a macOS demo.
- *The proposed workbench change to give `pushOutputsFromFile` a target URI.* It would reopen answer 3, which is
  settled, and widen scope to workbench code.
- *The idle watchdog.* The sub-agent itself concluded it cannot be detected.

---
title: 'Preview with q2 and Show Outputs: a fourth demo command'
date: 2026-11-12
date-modified: 2026-11-13
description: 'A new command for the q2-records-demo extension in the Positron fork. It runs q2 preview with the engines on, opens the preview in the Viewer, and pushes every run of the preview into the open editor through the .quarto execution-record copy. The q2 side makes -M reach the preview engine runs and the engine policy, and fixes three bugs on the live re-run path; the extension side watches, converts and pushes the copy.'
status: approved  # all questions answered 2026-11-12 and 2026-11-13; review folded in
---

**q2 branch:** `plan/preview-with-q2-and-show-outputs` in `workspace-6`, cut from `main` (`ec6546a33`).
**Fork:** `/Users/gordon/src/positron-riu6-a`, a new branch cut from `q2/rfu-7-demo-followups` (local commits only, as in
[RFU-7](2026-11-08-rfu-7-positron-fork.md)).
**Review:** [implementability review](../research/2026-11-12-preview-and-show-outputs-plan-review.md) (2026-11-12);
its findings are folded into this plan.
**Strand closed by this plan:** bd-twox482l (stale-at-startup record never re-executed under `preview.engine: auto`),
fixed by the Phase 1 staleness change. Standalone: not part of the RFU epic.

## Overview

The demo has three commands:

- Command 2, **Render with q2 and Show Outputs**, runs `q2 convert` once and pushes the outputs into the editor.
- Command 3, **Preview from Saved Outputs**, runs `q2 preview` from Positron's cache and never executes.
- The new command, **Preview with q2 and Show Outputs**, joins the two. It runs `q2 preview` with the engines on. The
  Viewer shows the preview, and each engine run of the preview is pushed into the open `.qmd` editor. A save re-executes
  the document and pushes again.

## Decisions (Gordon)

2026-11-12:

1. Outputs go to **both** the Viewer and the editor.
2. **Live**: every engine run of the preview is pushed, not only the first.
3. **Handoff through `record: true`**: the preview writes the `.quarto/execution-records/` copy. The extension converts
   that copy with `q2 convert -M 'execute:{record-sources: q2}'`, which runs nothing, and pushes it with
   `_positronQuarto.pushOutputsFromFile`, the call command 2 already makes.
4. **`-M`, not new flags.** `q2 preview -M 'execute:{record: true}'` must reach the metadata the engine run reads, and
   `-M 'preview:{engine: auto}'` must set the engine policy. The command never edits the user's files.
5. **The copy goes beside the document**, in `.quarto/execution-records/`, where a render writes it. No location option.
6. **A push while another editor is active is queued.** The extension keeps the newest converted notebook and pushes it
   when the document becomes active again. Older pending runs are dropped.
7. A standalone plan.

2026-11-13, after the review:

8. **Fix both hash problems**: the core hash filter (finding 7) and the preview's overlay-free hashes (finding 8).
9. **Fold bd-twox482l into Phase 1**: one staleness change fixes it and the two live-editing cases (finding 10).
10. **A status-bar item stops a live session**, so a session whose Viewer was closed with the big X is visible and can be
    stopped. One live session per document.
11. **`--static` shares the `preview.engine` helper**, so `q2 preview --static -M 'preview:{engine: off}'` works too.

## What we know

The investigation of 2026-11-12 (main at `ec6546a33`) ran scratch copies of a one-cell Python document, with the demo
`.venv` as `VIRTUAL_ENV`. The review of the same day read the code; findings 7 to 13 come from it.

1. **The preview already writes the `.quarto` copy, when the front matter asks.** With `execute: {record: true}` in the
   document, `q2 preview doc.qmd --no-browser` wrote `.quarto/execution-records/doc.ipynb` at startup. The write is
   `write_record_file` in `EngineExecutionStage` (`crates/quarto-core/src/stage/stages/engine_execution.rs:1249`),
   which the preview capture run goes through. No new writer is needed.
2. **The copy converts without running anything.** `q2 convert --to positron-ipynb doc.qmd -o out.ipynb -M
   'execute:{record-sources: q2}'` exited 0 and reported `{"cells":1,"dropped":[],"skipped":[]}`, and the notebook held
   the cell output. Convert does not check staleness (`crates/quarto/src/commands/convert.rs:14`).
3. **`-M` does not reach the preview's engine runs.**
   - `-M` enters a pipeline only as a runtime layer, read by `MetadataMergeStage` through `runtime_metadata()`
     (`crates/quarto-core/src/overlay_runtime.rs`).
   - The preview applies it to browser renders (`crates/quarto-preview/src/render_flags.rs`), and before any pipeline
     only for `execute: record-sources` (`runtime_with_overlay`, used by `config.rs:81`).
   - The three engine-run sites build a plain `render_flags::native_runtime()`:
     - `lib.rs:447`, the startup captures;
     - `lib.rs:505`, a file change and staleness;
     - `re_execute.rs:312`, both the Re-execute handler and the auto re-execute.
   - The overlay runtime is enough once given: the capture pipeline is the html pipeline cut after `engine-execution`
     (`crates/quarto-core/src/engine/preview_record.rs:168-174`), and the stage reads `execute.record` after the merge
     (`engine_execution.rs:582-588`).
4. **The preview prefers its own stale copy.** The preview default sources are `cache,q2,engine`
   (`CommandSources::PREVIEW`, `crates/quarto-core/src/record/source_key.rs`). After an edit made while the preview was
   stopped, a restart served the old `.quarto` copy without running the engine. The command passes
   `record-sources: engine` in its one `-M execute` map. At startup that means the copy is only written, never read; for re-execute
   see finding 9.
5. **Live re-execution did not happen in the first test.**
   - `preview.engine` is a session setting, decided once in `crates/quarto/src/commands/preview.rs:380-384` by
     `read_engine_policy_from_project`, before any document pipeline exists. It reads `_quarto.yml` only, and is
     `Manual` without a project.
   - With `auto` in `_quarto.yml`, the restart served a stale copy, so the staleness flag started `Some(true)` and no
     save flipped it (bd-twox482l; finding 10).
   - With `auto` only in front matter, the policy was `Manual`.
6. **q2 never writes Positron's cache.** `crates/quarto-core/src/positron_cache.rs` is read-only by design; the push
   goes through Positron, as for command 2.
7. **The record hash counts `execute.record`, against its own contract.**
   - `hash_document`\'s doc comment (`crates/quarto-core/src/record/exec_hash.rs:276-277`) says core always excludes
     `record`, "so toggling it never changes a hash".
   - The code filters only a top-level `record` out of `document_keys` (`:288-292`). The `execute:` map is filtered by
     the engine's presentation keys only (`:300-307`), and Jupyter's list has `record-sources` but not `record`
     (`crates/quarto-core/src/engine/jupyter/mod.rs:307-313`).
   - The only test, `toggling_record_changes_no_hash_even_when_an_engine_declares_it` (`exec_hash.rs:856`), uses a
     top-level key.
   - So turning on `execute: {record: true}`, the decision-24 spelling, changes a document's hash today, with or without
     `-M`.
8. **An overlaid engine run and the overlay-free staleness check hash different metadata.**
   - The engine stage hashes the merged metadata, overlay included (`crates/quarto-core/src/record/overlay.rs:1-9`).
   - Staleness recomputes on the overlay-free pre-engine document (`crates/quarto-preview/src/capture_driver.rs:410-414`,
     EXR-15 decision 5).
   - Wiring the overlay into the engine run alone would make any counted `-M` key look stale on every file event, and
     fire an auto re-execute each time.
   - `provide-hub` already solves this. It runs under the overlay and then `overlay_free_run`
     (`crates/quarto-hub-provider/src/execute.rs:806`) makes the run describe the source: overlay-free `input_qmd`, the
     record rehashed with `rehash_record`, and the overlay's execution keys noted in `quarto_overlay`.
   - That function uses only quarto-core types, so the preview can share it once it moves to quarto-core.
9. **Re-execute reads the capture cache whatever the record sources say.**
   - `perform_re_execute` calls `record_capture_run_cached` (`re_execute.rs:316-322`), which returns a cached run
     before any engine call (`crates/quarto-preview/src/cache.rs:243-248`).
   - Edit, save, undo the edit and save again: the cache key matches the earlier run, no engine runs, the `.quarto` copy
     keeps the undone version's outputs, and nothing is pushed. The Re-execute button is a silent no-op in the same case.
10. **The staleness flag sticks.**
    - `recompute_staleness` returns before the auto trigger when the flag does not change (`capture_driver.rs:437-439`),
      so auto fires only on a flip from fresh to stale.
    - A failed run keeps the flag `Some(true)` (`re_execute.rs:262-268`), so the save that fixes the error does not
      re-run.
    - A save during a run is dropped as in flight (`re_execute.rs:218-220`). The run then sets `Some(false)`
      unconditionally (`:334-338`), so the newest edit never runs.
    - bd-twox482l is the third way into the same state: a copy that is stale at startup.
11. **The copy is written in place, not atomically.** `write_record_file` calls the runtime's `file_write`, which is
    `fs::write` (`crates/quarto-system-runtime/src/native.rs:190-198`). A watcher can see an empty or partial file.
12. **Only the mtime tells a new copy from an old one.** The serializer is deterministic, so a re-run with the same
    outputs writes the same bytes, and a copy from an earlier session already exists when the command starts.
13. **The copy lives under the project root.** `execution_record_path` (`crates/quarto-core/src/record/location.rs:20-27`)
    gives `<project_dir>/.quarto/execution-records/<project-relative path>.ipynb`. `project_dir` is the nearest
    `_quarto.yml` or `_quarto.yaml` ancestor, canonicalized; without one it is the document's folder. The extension
    spawns q2 in the document's folder (`extensions/q2-records-demo/src/previewCommand.ts:77`), so it must find the root
    itself.

## Proposed shape

```text
extension: q2 preview <doc> --no-browser
             -M 'execute:{record: true, record-sources: engine}'        (findings 3, 4; ONE execute map, -M does not merge)
             -M 'preview:{engine: auto}'                               (decision 4)
           status-bar item "q2 live: <doc>" (click to stop)            (decision 10)
           opens the Viewer (as command 3)
           watches <project root>/.quarto/execution-records/<rel>.ipynb, plus a 1 s stat poll
on change: debounce, wait for a stable size and mtime, skip copies older than the session
           q2 convert --to positron-ipynb <doc> -o <session tmp> --source-uri <uri> -M 'execute:{record-sources: q2}'
           push with _positronQuarto.pushOutputsFromFile when the document is the active editor, else queue (decision 6)
on Stop:   kill q2, dispose the watcher, poll, status-bar item, queue and temp directory
```

## Checklist

Gate each q2 task on `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>`. Gate
each fork task on its vitests (see the fork's `AGENTS.md`).

### Phase 0: failing tests first (q2)

The tests drive `recompute_staleness` and the re-execute path directly, not through the file watcher: the only
watcher-driven staleness test is `#[ignore]`d on macOS for bd-9brz (`crates/quarto-preview/tests/integration/staleness.rs:88-96`).
Integration tests go in `crates/quarto-preview/tests/integration/`, registered in `main.rs`
(`.claude/rules/integration-tests.md`). The integration binary cannot reach `src/test_support.rs`, so a recording stub
engine is written in-file, as `record_gate.rs` does. A test that boots the server joins the watcher group in
`.config/nextest.toml`.

- [x] Hash: `hash_document` gives the same hash with `execute: {record: true}`, `{record: false}` and no `record` key
  (`exec_hash.rs` unit test).
- [x] `-M` reaches the engine run: a document with no `record:` of its own, previewed with `-M 'execute:{record: true}'`
  and `record-sources: engine`, gets a `.quarto` copy from the startup capture (`eager_capture.rs` is the precedent).
- [x] Overlay-free hashes: after that startup capture under `-M`, `recompute_staleness` on an unchanged file keeps the
  flag `Some(false)`, and the stored record carries `quarto_overlay`.
- [x] The cache honors the sources: with `record-sources: engine`, edit, re-execute, revert, re-execute; the copy is
  rewritten each time and the engine runs each time.
- [x] Auto after a failure: a stub engine that fails on one input, then succeeds; the save that fixes it re-runs
  without a click.
- [x] Auto after a save during a run: a stub engine that blocks until released; a second edit during the run is run
  after the first finishes.
- [x] Auto when stale at startup (bd-twox482l): a stale copy served at startup under `auto` re-runs on the next save.
- [x] `preview.engine` from `-M`: unit tests of the new helper, covering a standalone document with no `_quarto.yml`,
  `-M` over `_quarto.yml`, the boolean forms, and an unknown value as an error.
- [x] `-M` precedence: `-M 'execute:{record: true}'` over front matter `record: false`; `-M` `record-sources: engine`
  over a document's own list; both under `--share`, where the host list is a ceiling. (Under `--share` a document that
  lists only `q2` narrows the ceiling to nothing; it cannot widen it. The test asserts both.)

Phase 0 notes (2026-11-13): the tests are in `exec_hash.rs`, `config.rs` (`engine_policy_overlay_tests`) and
`crates/quarto-preview/tests/integration/live_reexecute.rs` (seven tests; module added to the watcher group in
`.config/nextest.toml`). Each was red on the pre-fix source for the reason given: the hash test on `record:false` vs no
key; `-M` tests on a missing copy / missing `quarto_overlay`; the cache test on a run count stuck at 2; the three auto
tests on timeouts. Two test-harness traps, both fixed: the stub engine must set `owned_languages` or every record reads
stale; and `StorageManager::new` persists the index under `<root>/.quarto/hub`, so a second context over one folder
inherits the first one's sidecar (the stale-at-startup test passed falsely until each context got its own data dir).

### Phase 1: q2 changes

- [x] **Hash filter.** `hash_document` drops `record` from the `execute:` map as well as the top level, as its doc comment
  says. Existing `.quarto` copies written with `execute: {record: true}` read as stale once afterwards. Decided: no
  `version` bump (the field is each engine's own declaration, and a bump would stale every record); said so in
  `a9192a99c`.
- [x] **One session runtime.** Build the overlay runtime once in `lib.rs` (`render_flags::runtime_with_overlay()`), and
  pass it to the startup captures (`lib.rs:447`), the file-change path (`lib.rs:505`), `trigger_auto_re_execute`,
  `claim_and_spawn` and `perform_re_execute` (which stops building its own at `re_execute.rs:312`). The Re-execute HTTP
  handler takes the same session runtime. The cache key is then computed under the overlay, so sessions with different
  `-M` do not share cache entries.
- [x] **Overlay-free runs.** Move `overlay_free_run` from `quarto-hub-provider/src/execute.rs:806` to quarto-core next to
  `rehash_record`, keep `provide-hub` calling it there, and apply it to every preview capture run before the run is
  stored. Staleness stays overlay-free; it needs a plain runtime alongside the session runtime for `compute_source_inputs`.
  The `.quarto` copy written by the stage keeps the overlaid hashes. That errs toward stale, which is safe; note it in
  the `write_record_file` doc comment.
- [x] **Re-execute honors the sources.** `perform_re_execute` reads the cache only when `cache` is in the document's
  resolved sources (`ReExecuteConfig::document_sources`, `config.rs:75`).
- [x] **Auto re-runs whenever the document is stale** (bd-twox482l). Under `Auto`, `recompute_staleness` triggers when the
  document is stale, not only on a flip; the in-flight guard still collapses repeats. When a run finishes, success or
  failure, recompute staleness once and trigger again if the document is still stale. Close bd-twox482l with the commit.
- [x] **`preview.engine` from `-M`.** A pure helper in `crates/quarto-preview/src/config.rs` reads `preview.engine` from
  the `-M` JSON and wins over `_quarto.yml` (the same precedence as `record-sources`). It maps a string as
  `parse_policy_str` does and a boolean as `read_engine_policy_from_metadata` does, but an unknown string is an error,
  as `resolve_record_sources` reports one. Call it in `preview.rs` before `warn_engine_under_off`, and in
  `preview_static.rs` (decision 11). With `--share` and `Auto`, print a startup warning that guest edits run host code.
- [x] **Comments.** Revise the header of `render_flags.rs` (`-M` now reaches code execution) and the "overlay-free"
  comment at `capture_driver.rs:412`. Extend `runtime_factory_lint.rs` if a site could still build a plain runtime for
  an engine run.
- [x] **Docs.** `docs/guides/projects/preview.qmd:54-55` says `-M` does not reach code execution; rewrite that, and add
  `preview.engine` through `-M` to the options there, with the `--share` caution in the Sharing section. Add a line on
  `-M` to `docs/guides/authoring/execution-records.qmd`.
- [x] **End to end.** On a scratch standalone document: `q2 preview` with the command's final flags; edit and save; see
  the copy change; edit, revert and save, and see it change again; convert it with `record-sources: q2`. Check that the
  Viewer shows no "Execution record not written" diagnostic: the browser's render also sees `-M execute:{record: true}`.
  Record the invocations and output here.
- [x] Recommend to Gordon a wider run: `cargo nextest run -p quarto-hub-provider` (the move of `overlay_free_run`) and
  `-p quarto` (the policy wiring), or the workspace.

Phase 1 notes (2026-11-13):

- Commits: `a9192a99c` (hash filter), `23842d767` (move `overlay_free_run`), `f26a96f8f` (engine runs, overlay-free runs,
  cache, auto), `790d4c880` (`preview.engine` from `-M`, docs). bd-twox482l is fixed by `f26a96f8f`.
- **Deviation: no runtime threaded through signatures.** The engine-run sites call `render_flags::runtime_with_overlay()`
  (the session already keeps `-M` in a process-wide `RenderFlags`, and `ReExecuteConfig::document_sources` already used
  it). The `lib.rs` sites keep passing the plain runtime, which is the *source* runtime: staleness and
  `compute_source_inputs` must stay overlay-free. A lint, `engine_runs_take_the_overlay_runtime`, makes a plain runtime
  at an engine run fail the build.
- **Cache key.** `session_cache_key` is the plain key without `-M`, and the key plus the canonical `-M` JSON with it, so
  sessions with different `-M` share no entry. Startup and re-execute use the same key.
- **Auto.** A request that loses to a run in flight is remembered (`PENDING`, under the in-flight guard); the run's end
  re-checks the document once. A failed run with no newer save does not retry itself, so a failing document cannot loop.
- **`-M` does not merge repeated keys** (`metadata_flag.rs`: "the later wins; nothing is merged"). The first end-to-end
  attempt, `-M 'execute:{record-sources: engine}' -M 'execute:{record: true}'`, silently lost `record-sources`. The
  command must pass **one** `execute` map: `-M 'execute:{record: true, record-sources: engine}'`. Phase 2 uses that form.
- Gate: `clippy --all-targets -D warnings` clean for quarto-core, quarto-hub-provider, quarto-preview, quarto;
  `nextest -p quarto-preview` 383 passed, 1 skipped (bd-9brz); `nextest -p quarto -E 'test(preview)'` 121 passed;
  `nextest -p quarto-core` 8146 passed, 249 failed, all in pandoc-dependent tests (local pandoc 3.12.1, floor 3.13) and 2
  revealjs vendoring checks, none in record or hash code (not compared against a clean-main run).
- Docs were not render-checked (the pandoc floor again).

#### End-to-end run (2026-11-13)

Binary: `q2` built from this branch (`cargo build --bin q2`). Scratch standalone document
`.../scratchpad/e2e/doc.qmd` with one Python cell printing `VERSION ONE`. The demo `.venv` must be on `PATH` as well as in
`VIRTUAL_ENV`: with `VIRTUAL_ENV` alone the page reported "Engine 'jupyter' is registered but its runtime is not available".

```bash
cd scratchpad/e2e
VIRTUAL_ENV=$DEMO/.venv PATH=$DEMO/.venv/bin:$PATH \
  q2 preview doc.qmd --no-browser -vv \
    -M 'execute:{record: true, record-sources: engine}' -M 'preview:{engine: auto}'
```

- Startup log: `resolved preview engine policy engine_policy=Auto`. `GET /api/preview/config` returned
  `render.metadata = {"execute":{"record":true,...},"preview":{"engine":"auto"}}`.
- Startup wrote `.quarto/execution-records/doc.ipynb` (3000 bytes). `jq '[.cells[]|select(.cell_type=="code")|.outputs[]?.text]'`
  gave `["VERSION ONE\n"]`.
- Edit to `VERSION TWO` and save: log `updated sidecar staleness staleness=true`, `auto re-execute kicked off`; the copy
  read `["VERSION TWO\n"]` one second later.
- Revert to `VERSION ONE` and save: the same two log lines; the copy read `["VERSION ONE\n"]` again (the pre-fix path
  hit the cache here and wrote nothing).
- `q2 convert --to positron-ipynb doc.qmd -o out.ipynb -M 'execute:{record-sources: q2}'` exited 0 with
  `{"cells":1,"dropped":[],"skipped":[]}`, and `out.ipynb` held `["VERSION ONE\n"]`.
- `GET /api/preview/diagnostics?page=doc.qmd` returned `[]` during the run. **Not verified:** the browser's own WASM
  render of the page under `-M execute:{record: true}` (no browser here), so the "Execution record not written"
  question in the review is still open for the Viewer.
- The preview was started as a background task and stopped with `TaskStop`. The outputs above were inspected directly.

### Phase 2: the extension (fork)

Read the fork's `CLAUDE.md` and `AGENTS.md` first. Every pure function below gets a vitest before its use.

- [x] **Contract.** In `q2Contract.ts`: `buildLivePreviewArgs(doc)` with the exact flags above (one combined `execute` map: `-M` does not merge repeated keys;
  mind the flow-map quoting note there); `buildExportArgs` takes extra `-M` entries; `recordCopyPath(doc, projectRoot)`, with a
  `findProjectRoot(doc)` that walks up for `_quarto.yml` or `_quarto.yaml` and returns the `realpath` (macOS `/tmp` is
  `/private/tmp`), else the document's folder. Vitest the same cases as the Rust tests in `location.rs`: standalone,
  project root, project subfolder, and `a.b.qmd`; compare paths case-insensitively on Windows.
- [x] **Shared convert and push.** Factor command 2's convert, result parsing and push out of `exportCommand.ts` into one
  function both commands call, rather than copying it. Exit 10 (`NO_RECORD`) and 12 (`INVALID_RECORD`) are retryable in
  the live command.
- [x] **Change detection.** A pure `shouldHandle(stat, sessionStart, lastHandled)`: accept a copy only if its mtime is at
  least the session start minus 1 s and its `(mtime, size)` differs from the last handled one. Vitest it, including a
  leftover copy from an earlier session.
- [x] **Watching.** Before `spawn`, record the session start and create a `RelativePattern` watcher on the copy's folder
  (a string glob drops events outside the workspace folders) and a 1 s stat poll; the poll also covers a folder that
  does not exist yet. Stat once more when q2 prints its URL.
- [x] **Debounce, stability and retry.** Debounce 300 ms; convert only when size and mtime are unchanged across 100 ms;
  retry exits 10 and 12 three times with 300 ms, 600 ms and 1.2 s backoff. One convert at a time per session: an event
  during a convert marks the session dirty, and a dirty session re-stats after the convert and runs again if the stamp
  changed. Notify once per distinct error message, then log only.
- [x] **The queued push** (decision 6). Push at once when the document is the active editor. Otherwise keep the newest
  converted notebook in a per-session temp directory and push it on `onDidChangeActiveTextEditor` when the URI matches
  (an `undefined` editor is not the document). Drop the queue on `onDidCloseTextDocument`. Vitest the queue as a pure
  state machine.
- [x] **Sessions** (decision 10). A shared registry of live sessions, one per document: invoking the command again on
  the same document stops the old session first. Command 2 refuses while a live session owns its document. A status-bar
  item "q2 live: \<doc\>" shows for each session, and clicking it stops the session. Stop, the Viewer's Stop, panel
  dispose, the status-bar click and extension deactivate all go through one `dispose()` that kills q2 and disposes the
  watcher, poll, queue, status-bar item and temp directory.
- [x] **Register the command** `q2.previewAndPush` ("Preview with q2 and Show Outputs"): spawn and Viewer as in
  `previewCommand.ts`, with the session, watcher and push above. Update `package.json` (`activationEvents`,
  `contributes.commands`, and the `description`, which says "Three commands") and the header comment of `extension.ts`.
  Notifications: a toast on the first push and on errors; later pushes go to the status-bar tooltip and the log.
- [ ] **Playwright e2e** (written, not run; see the Phase 2 notes) on the Python demo, in `test/e2e/tests/quarto/` (move `runFromPalette`, `warmJuliaServer` and
  `rProcesses` out of `quarto-render-and-push.test.ts`, or add the tests to that file):
  - outputs appear in the editor and the Viewer;
  - an edit that changes printed text, then a save, updates both: the new text present, the old text gone (120 to 180 s
    timeout, no sleeps);
  - a leftover `.quarto` copy from an earlier session is not pushed;
  - after Stop, no q2 or kernel process remains;
  - `afterEach` removes `.quarto/` from the fixture workspace.

  If the edit-and-save case fails only on macOS, check bd-9brz before blaming the extension.
- [ ] **R and Julia e2e** (written, not run), once each: R needs the `tags.ARK` fixture and `rProcesses()` after Stop; Julia needs
  `Q2_E2E_JULIA_PROJECT`, the copied `Project.toml` and `Manifest.toml`, `warmJuliaServer` and a 480 s timeout.

Phase 2 notes (2026-11-13). Fork branch `q2/preview-and-show-outputs` (cut from `q2/rfu-7-demo-followups`), local commits
`b4fc05f1c3` (code and vitests) and `4dd0cdde52` (e2e, README, `.gitignore`). Pure modules: `q2Contract.ts` (additions),
`liveSync.ts`; vscode glue: `liveSession.ts`, `convertAndPush.ts` (shared with `exportCommand.ts`). 97 vitests in the
extension pass; `tsc --noEmit -p extensions/q2-records-demo` is clean; the fork's hygiene hook passed on both commits.
`findProjectRoot` takes a file-system probe so the vitests cover the same cases as `location.rs`, plus a symlink and
Windows case-insensitivity. Not done as written: the unchecked e2e items below.

- The Playwright cases are written (three Python, one R, one Julia, in `quarto-render-and-push.test.ts`) but **were not
  run**. The harness does not start in this tree: `playwright.config.ts` imports `utils/memory/scenarios.ts`, which imports
  `./lanes.js`, and Playwright 1.64 cannot resolve that specifier from the CJS-typed `test/e2e` folder (every e2e test is
  affected, not only these). A temporary untracked copy of the compiled `lanes.js` got past it, but the run then needs
  `POSITRON_PY_VER_SEL` and three more variables, and collided with another session's Positron in the shared
  `$TMPDIR/vscsmoke` directory (`ENOTEMPTY`). I stopped there rather than disturb that session; the shim is removed (the
  commit hook needs it to pass, so it was restored for the commit only).
- The `.quarto` copy is written in place (finding 11), so the extension never trusts a copy until its size and mtime
  hold still across 100 ms, and retries exits 10 and 12. The atomic write is bd-stbigif1.

### Phase 3: demo guide and wrap-up

- [x] Add the command to `extensions/q2-records-demo/demo/README.md`, written with `/reader-expectations-prose`. Its
  limits:
  - the push goes to the active editor, and is queued until the document is active again;
  - unsaved edits show no new outputs until saved;
  - the Viewer's big X leaves the session running, so stop it from the status bar;
  - in a project, `-M 'execute:{record: true}'` writes a copy for every document at startup, so use the command on
    standalone files.
- [x] Add a `.gitignore` in the demo folder for `.quarto/` and `*_files/`.
- [x] Render-check this plan and the landing note with `q2 render --strict`.
- [x] Reconcile this checklist with what landed, and write a landing note listing the q2 and fork commits.

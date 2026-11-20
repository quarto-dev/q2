---
title: 'GE-17: Windows run C: the scripted scenario and the Playwright subset'
date: 2026-11-18
description: Runs the scripted end-to-end scenario and the permanent Playwright VS Code subset on a Windows machine, repeats every earlier Windows item on the final code, and records what Windows does and does not prove for the epic.
status: approved
depends-on: [ge-14, ge-15, ge-16, ge-16a]
lane: A
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) ·
**Waits for:** [GE-14](ge-14-cross-client-e2e.md), [GE-15](ge-15-windows-run-a.md),
[GE-16](ge-16-windows-run-b.md), and [GE-16a](ge-16a-windows-fixes.md) · **Unblocks:** [GE-18](ge-18-docs-and-closeout.md) ·
**Branch:** `ge/17-windows-run-c` off `feature/git-epochs` for the notes, merged by rebase and fast-forward. The tooling
stays on the temporary `windows/git-epochs` branch, which is extended here and never merged. The only push in this plan
is that branch, after Gordon says go.

## Goal

Close the Windows question for the epic. Run the whole new feature end to end on Windows with the real `q2.exe`: a
`--git track` hub on a temp repo, a headless clone peer, and real `git switch` operations. Run the permanent Playwright
VS Code subset on Windows. Then run every item from GE-15 and GE-16 again on the final code, because fix plans may have
landed since those runs. The deliverable is a final statement of what Windows does and does not prove.

## Decisions implemented

- **G31:** the third Windows run, from the tooling branch, with the `windows/` remote prefix. Only new tests run.
- **G30:** the headless peer and the Playwright subset that GE-14 marked permanent (local hub, no network).
- **G33:** the hub is `q2.exe preview --git track`; the `/api/preview/git-status` route is observed during a real
  operation.
- **G3:** the gate markers are observed during a real `git switch` of a large tree.
- **G9, G13:** arrival and unlist and relist are asserted on a real Windows checkout, including a binary file.
- **G5:** shutdown while a marker is set skips the flush.
- **G32:** the timeouts used by the scenario are the epic\'s numbers.

## Context

- **What earlier plans leave.** GE-15 and GE-16 left `scripts/git-epochs-windows/run.ps1` with items `a*` (Rust and hub)
  and `b1` to `b7` (TypeScript and extension), the failure extractor, the instructions page, and the large-tree
  generator. Read their notes first, including any fix plans inserted after them.
- **What GE-14 left.** The cross-client e2e scenarios and the list of tests it marked permanent, with the command that
  runs them. Read its notes for the exact tag or file list and the Playwright config path; this plan uses them as
  written.
- **The model for the scenario** is GE-13\'s hub e2e (git helpers, the same steps) run through the real binary instead
  of a harness on macOS. The Windows version is a script that drives `q2.exe`, a headless peer, and `git.exe`.
- **Process lessons from RFU-6.** Run each command through `cmd.exe` and wait on that one process (leaked processes hold
  a pipe open); merge the integration tip into the tooling branch first (never rebase a pushed branch) and record the
  base commit; clone with
  `core.autocrlf=true` and report anything else.

## Test specifications

| Item | How it is tested | A pass proves | It does not prove |
|---|---|---|---|
| `c1` (W9) | The scripted scenario below, against the real `q2.exe` and `git.exe` on a generated temp repo in `%TEMP%`; the script exits non-zero if any assertion fails and writes a JSON transcript of each step and the observed values | The feature works end to end on Windows: the gate, the classifier, the registry, the flips, the unlists, a binary file, and shutdown with a marker set | jj on Windows; other Git builds; long soak |
| `c2` (W8) | The Playwright subset GE-14 marked permanent, plus GE-9\'s timing test (`test:e2e:timing`, not in the permanent subset), with the pinned VS Code build that the harness downloads (`downloadAndUnzipVSCode`, the same version as on Linux),
launched the same way the Linux harness launches it. The timing test\'s measured gap (from `index.lock` release to the last buffer reload) is recorded in the results; a gap above the 1 s window is a finding for the experimental fallback in G20, not a failed run | The extension\'s VS Code surfaces (commands, notifications, status bar, buffers) work with `Code.exe`; editor-only\'s quiet window against a real Windows reload | VS Code Insiders or other versions; the unmarked exploratory tests |
| `c3` | Every item from GE-15 (`a*`) and GE-16 (`b1` to `b7`), run in one invocation without `-Only`, on the final code | Nothing regressed on Windows after the fix plans; the whole set is green together | Anything the items did not already cover |

**The scenario for `c1`** (each step asserts; the transcript records what it saw):

1. Generate the deterministic repo (the generator from GE-15, run in its large-tree mode of about 4,000 files, with a
   few text files, one binary, and nested directories, so that a `git switch` holds the lock long enough to observe; add
   a `--with-binary` mode to the generator in GE-15\'s tests if it lacks one) on `main`; create a `feature` branch that
   changes one text file, removes one file, adds one file, and changes the binary.
2. Start `q2.exe preview --git track --allow-edit --ui editor --no-browser --port N` (N free) in the repo.
3. Attach a headless clone peer from a second clone by the Join logic (`/health` gives the index id), and type an edit
   into one file.
4. Commit on `main` in the hub\'s repo while the peer edits: no flip, the peer\'s edit intact.
5. `git switch feature` on the hub\'s repo, polling `.git/index.lock` and `/api/preview/git-status` every 20 ms (the
   large tree of step 1 keeps the lock for long enough): the route reports paused at least once and not paused after; the changed file flips, the removed file is unlisted, the added file is
   listed, the binary\'s doc changes; the peer, still on `main`, keeps its docs and its disk is untouched.
6. `git switch main`: arrival re-attaches the registered docs (the pointer returns to the earlier docIds, no fork), and
   disk equals the doc.
7. Save a file and commit inside the debounce window: handled as a commit (no flip).
8. `git reset --hard` after a dirty edit: the pointer moves to a new doc equal to the HEAD content, and the dirty text
   stays in the retired doc.
9. In a second run, stop the hub during a `git switch` of the large tree: the shutdown flush is skipped while the
   marker exists, and no doc contains conflict markers or half-written text (detail chosen by the plan author;
   confirm). The hub\'s shutdown handler is `tokio::signal::ctrl_c()` (`crates/quarto/src/commands/preview.rs:698`),
   which on Windows listens for `CTRL_C_EVENT`, and a Node parent cannot send that event to one child process. The
   script therefore starts the hub in its own console through a small PowerShell helper and sends the event with
   `GenerateConsoleCtrlEvent` (P/Invoke); if that cannot reach the hub, step 9 is recorded as not run on Windows, and
   the skipped-flush behavior is the hub-level test of GE-3 (which stands in for the marker with a file).

## Checklist

Items marked (agent) run on macOS; items marked (Gordon) need the Windows machine.

- [ ] (agent) Confirm GE-14, GE-15, GE-16, and GE-16a are complete, and read the notes of all four. GE-16a carries three
  Windows steps to this plan: the baseline run of `main` at its merge base (`run.ps1 -Only a6`, GE-15 F3; each
  pre-existing failure becomes a braid strand), one run on a machine with Developer Mode on so that the two skipped
  symlink tests run (GE-16a `B2`), and the `B4` evidence: run the `b4` item with `QUARTO_HUB_DEBUG=1` in the environment
  (GE-16a added it; it logs each watcher event\'s path form, its relative key, and the clone gate state to the log and
  to stderr), and if `clone-modes.test.ts` "each decision writes its event to events.jsonl" still fails, read those
  `debug: disk` lines first. A follow-up fix plan for it, if needed, is inserted at the end of this plan.
- [ ] (agent) In the tooling worktree `/Users/gordon/src/q2/.worktrees/workspace-8/.worktrees/windows-git-epochs` (GE-15 created it inside lane B\'s worktree; the branch
  is checked out there, so do all tooling work in it), merge the final `feature/git-epochs` tip into
  `windows/git-epochs` (never rebase the pushed branch) and
  record the base commit hash.
- [ ] (agent) Write the scenario script for `c1` on the tooling branch (a Node script that drives `q2.exe`, the headless
  peer, and `git.exe`, using GE-13\'s steps as the template). Dry-run its logic against the macOS `q2` binary to check
  the assertions before handoff.
- [ ] (agent) Add items `c1`, `c2`, and `c3` to `run.ps1`; `c2` uses the command and config GE-14 recorded; `c3` runs
  every `a*` and `b*` item. Each fills `proves` and `does_not_prove`.
- [ ] (agent) Update the instructions page on the tooling branch: the pinned VS Code build the harness downloads (record the version), Node at the
  `.nvmrc` major, `core.autocrlf=true`, `core.longpaths=true`, optional `jj`, and the table above.
- [ ] (agent) Gate the tooling: parse-only syntax check of the script with `pwsh` if installed; the extractor\'s help;
  `q2 render --strict` of the instructions page; the Playwright subset passes on macOS or Linux first.
- [ ] (agent) Ask Gordon to say go, then push the tooling branch, unsilenced:
  `git push origin windows/git-epochs:windows/git-epochs`, and check `git ls-remote origin windows/git-epochs` against
  `git rev-parse HEAD`.
- [ ] (Gordon) On the Windows machine, fetch and switch to the branch and run
  `powershell -ExecutionPolicy Bypass -File scripts\git-epochs-windows\run.ps1 -Publish`. Run the extractor once if
  anything failed and push the details.
- [ ] (agent) Fetch, parse `results/*.json`, and record in the notes: run id, host, Windows version, git, node, VS Code
  version, `core.autocrlf`, the base commit, the table of item results, the `c1` transcript, and every failure with
  its output.
- [ ] (agent) For each failure decide whether it is a product defect, a test defect, or the machine, and propose a fix
  plan. A fix plan inserted here means this plan stays open until its follow-up run passes; do not change product
  code in this plan.
- [ ] (agent) Write the final statement in the notes (below) and copy the summary into `ge/17-windows-run-c`; rebase,
  fast-forward, and leave the tooling branch unmerged. Do not delete the tooling branch: GE-18 does that, with
  Gordon\'s agreement, after the notes are recorded.

## The final statement

Write this in the notes, filled from the results. A pass proves compilation and execution of the listed tests with a
real `git.exe`, `q2.exe`, and Node on one Windows machine, and the end-to-end scenario above. It does not prove: jj on
Windows unless installed; other Git builds or Windows versions; antivirus, OneDrive, or Dev Drive effects; a long
soak; VS Code beyond the permanent subset; the public sync server; or the iroh tunnel across real networks.

## Gates

- The tooling gates in the checklist.
- If any test was added or changed on `ge/17-windows-run-c`: its own suite on macOS, `cargo xtask lint`, and
  `q2 render --strict` of the notes file.

## End-to-end verification

`c1` is the end-to-end verification on Windows, run through the real binary. Record the exact invocation, the
transcript excerpt for steps 4 to 6, and an explicit note that the output was inspected. If any step could not run,
say so instead of claiming success.

## Exit criteria

- Items `c1`, `c2`, and `c3` each have a result in the notes, with failures and their output.
- The final statement is written.
- Every failure is classified, with a proposed fix plan where needed.
- The tooling branch is unmerged and not yet deleted.

## Out of scope

- Fixing defects (fix plans follow); deleting the tooling branch (GE-18).
- Failures in older tests that predate the epic.
- A soak test or a performance benchmark.

## Notes from the run

**Session 1 (lane A, 2026-11-19).** Branch `ge/17a-windows-fixes` off `feature/git-epochs` (`32478b2b8`). GE-14, GE-15, GE-16, and
GE-16a were done when the session started. The plan is **not done**: `c1` found two product defects, fixed below; `c1`
now passes steps 1 to 8 on Windows, and the `a6` failures, `c3`, and the GE-16a leftovers are open.

**Tooling** (`windows/git-epochs`, merged with `feature/git-epochs`, never rebased; temporary, never lands). Items `c1`,
`c2`, and `c3` in `run.ps1`; the scenario `scripts/git-epochs-windows/c1/windows-scenario.e2e.ts` (a Playwright-driven
script with no browser; `run.mjs`; `ctrl-c.ps1` for step 9); instructions page section 10. `c1` ran green on macOS before
the handoff: `node scripts/git-epochs-windows/c1/run.mjs` with 4,005 tracked files, 2 tests passed, step 9 saw the marker,
sent SIGINT, logged "the shutdown flush was skipped", and a restarted hub served the switched tree with no markers.
`pwsh` is not installed on this host, so `run.ps1` was never parsed before Windows; it ran on first use.
Deviations: the scenario is a `.e2e.ts` run by Playwright's runner (not a plain Node script) so it can reuse the
extension\'s TypeScript helpers; the `feature` branch also rewrites 800 bulk files so a `git switch` holds the lock
long enough to poll. Registry entries are keyed by path and blob, so after a switch `a.qmd` has one entry per branch;
the scenario asserts on the entry at the HEAD blob.

**Runs on the Windows machine** (Windows 11 Pro, `x86_64-pc-windows-msvc`, git 2.47.1.windows.1, node v24.21.0,
`core.autocrlf=true`, `core.longpaths` unset, no `jj`, VS Code 1.101.2 downloaded by the harness):

| Run | Item | Result |
|---|---|---|
| `20261119-091925` (base `c91a63522`) | `c1` | FAIL: step 3 never finished (10 min test timeout); step 9 timed out waiting for the mirror to hold every file |
| | `c2` | extension flows 17 of 19 passed; timing test passed with a gap of **84 ms** (window 1,000 ms); cross-client scenarios not run (no embed build) |
| `20261119-095638` (base `b6c4cb95e`) | `c1` | FAIL: the mirror held **4 files of 4,005** after 480 s, with the hub alive; hub log: `Index path escapes project root, rejecting path=d37\f2637.qmd` (hundreds of lines) |
| | `c2` | PASS: 20 passed, 0 skipped; gap 84 ms; cross-client scenarios not run |
| (`a6` console, not published) | `a6` | 1,878 of 1,896 passed, 18 failed (list below); not yet compared with the baseline |

**Defect found by `c1` (product, fixed on this branch).** `reconcile_files_with_index` built index keys with
`file_path.to_string_lossy()`, which on Windows gives `d00\f0000.qmd`. GE-3 added a rule to `contained_join` that rejects
any key containing a backslash (G36), so `sync_all` skipped every nested file: the hub never wrote or served it, and a
joining peer received only the four root-level files (`_quarto.yml`, `index.qmd`, `a.qmd`, `c.qmd`; `img/logo.png` was
missing too). `sync_file_by_path_gated` had the same native key for the watcher\'s index lookup, so disk edits to nested
files would not have reached their docs either. The native-key code dates from 2025; GE-3\'s hardening made it fatal,
and every Windows test before `c1` passed because the unit tests build keys by joining components with `/` and never
went through discovery. Fix: both sites use `quarto_util::to_forward_slashes`
(`crates/quarto-hub/src/context.rs`, `sync.rs`; commit `5c0ccc508`). Tests, red then green: 
`context::tests::discovery_indexes_nested_files_with_forward_slash_keys` and
`sync::tests::sync_file_by_path_finds_a_nested_file_by_its_forward_slash_key` (a backslash in a POSIX file name stands
in for the Windows separator). `cargo nextest run -p quarto-hub` 542 of 542, clippy clean, `cargo xtask lint` clean.
The `a4` item\'s `path_keys::` tests could not catch this: they exercise `contained_join`, not the producers of keys.
**Not seen fixed on Windows:** the proof is `c1` on the next run.

**`c2` classification.** The first run had two failures that did not repeat in the second run (20 of 20):
- `join.e2e.ts (b)` read `index.qmd` from `.quarto-hub-tmp-*`, the staging folder `createProjectFolder` renames into
  place: its `waitFor` accepts any folder with `.quarto-hub/project.json`, which the staging folder has before the
  rename. A **test race**; the fix is to skip `.quarto-hub-tmp-*` in that scan. Not changed here (tests only, and it
  passed on the rerun).
- `clone-mode.e2e.ts "Stay detached"` found no toast button within 30 s. **Unexplained**: the run\'s
  `error-context.md` was not pushed, and it passed on the rerun. Treat as an intermittent Windows timing issue
  unless it recurs.
Timing (G20): a gap of 84 ms from `index.lock` release to the last buffer reload; editor-only does not need the
experimental fallback on this machine.

**`a6` failures (18 of 1,896), first look.** In `quarto`: `convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes`, four `lmd_snapshots`, two `render_scripts_cli`, `render_dry_run::would_render_lines_are_project_relative`,
`smoke_all`; in `quarto-preview`: `route_tests` (two), `book_downloads`, `render_scripts_boot`, `live_reexecute`; in
`quarto-hub`: `storage::test_storage_manager_prevents_double_lock` and its standalone twin; in `quarto-git-epochs`:
`probe_real_repo::index_lock_is_resolved_through_git_path_for_a_linked_worktree` and
`probe_real_repo::is_shallow_is_false_for_a_repo_and_true_for_a_depth_one_clone_through_a_file_url`. The two probe
tests are in the epic\'s own crate and are the ones to explain first; none of the 18 is classified until the
`main` baseline (`run.ps1 -Only a6` at the merge base) and the failure text are in hand.

**Later runs (all `c1` on Windows unless noted).**

| Run | Base | Result |
|---|---|---|
| `20261119-113752` | `67336526b` (backslash-key fix) | FAIL: the mirror peer got "Document is unavailable" (cause not seen: nothing logged the hub output at that point) |
| `20261119-114524` | `f2cdb1e27` | `a6`: 23 failing (below); `c3` FAIL only because `-SkipBuild` left `a0` and `a6` out |
| `20261119-140625` | `381085929` (peer-join retry, hub log on failure) | steps 1 to 8 PASS, then FAIL with `UnsafePathError: "d15/f1335.qmd" resolves outside the project root` from the mirror peer |
| `20261119-143919` | `2229d3f75` (`safeJoin` fix) | steps 1 to 6 PASS, step 7 FAIL: "the saved text reaches the mirror" timed out at 120 s (test ran 7.7 min) |
| `20261119-150923` | `4f3c6c433` (failure diagnostics) | **PASS** (1 passed, 1 skipped, 12.3 min): steps 1 to 8 green; step 9 not run (below) |

**Second defect found by `c1` (product, fixed).** `vscode-sync-experiment/src/core/safe-path.ts`: `safeJoin` read *any*
`fs.realpath` failure as "the path does not exist" and walked up to the drive root, so a transient failure under file load
(the error code was not captured; `EMFILE` or `EBUSY` are the candidates) became "resolves outside the project root" for
one file of 4,000, which failed the mirror peer. Fix: only `ENOENT` and `ENOTDIR` mean "missing"; anything else is
rethrown with its own code (commit `f47ec4a7b`). Test, red then green: `safe-path.test.ts` injects `EMFILE` into
`realpath` and expects that error, not an `UnsafePathError`; it failed with the Windows run\'s own message first. The
extension suite is 338 passed, 1 skipped; `tsc` is clean. The next occurrence will name the real error code.

**My two new tests failed on Windows (test bug, fixed).** `discovery_indexes_nested_files_with_forward_slash_keys` and
`sync_file_by_path_finds_a_nested_file_by_its_forward_slash_key` wrote a file named `d00\f.qmd`; on Windows the `\` is a
separator and `d00` did not exist. Both now create the parent directory first, which is a no-op on Unix (commit
`2e646f9cc`). The cross-check `cargo clippy --target x86_64-pc-windows-msvc` cannot run on this Mac (the `aws-lc-sys`
C build fails), so the Windows `a6` rerun is the proof.

**`c1`, what the passing run shows.** On Windows with 4,005 tracked files: the hub starts, the mirror and the clone
attach; a commit on `main` while a peer edits leaves the edit intact (`moved-forward`); `git switch feature` pauses the
gate on `index.lock` and then flips (switch 1.4 s); switching back re-attaches the registered docs and disk equals the doc;
a save and commit inside the debounce window is a commit, not a flip; `git reset --hard` after a dirty edit makes a new doc
equal to HEAD and the dirty text stays in the retired doc (`retired(discarded=1)`).

**Open on `c1`.**
- **Step 7 is intermittent** (failed once in three runs on identical code; the run with the diagnostics passed). Unknown
  whether it is the hub watcher or the mirror. `c1` now records the hub log tail, gate, decisions, and `a.qmd` on the hub
  disk, the mirror, and the clone when any step fails; rerun `c1` until it either fails with that evidence or has passed
  several times in a row.
- **Step 9 (Ctrl+C during a switch) has not run on Windows.** `ctrl-c.ps1` failed with `AttachConsole failed: 6`
  (`ERROR_INVALID_HANDLE`): `detached: true` on Windows is `DETACHED_PROCESS`, a child with no console (and
  `CREATE_NEW_PROCESS_GROUP`, which disables Ctrl+C). Tooling commit `18df11e3f` spawns the hub without `detached`
  (`windowsHide` gives it its own hidden console), and the helper refuses to send the event when the hub shares a console
  with the test runner. Untested on Windows. Run only step 9 with `$env:C1_GREP='step 9'; run.ps1 -Only c1 -Publish`. If
  it still cannot deliver, the exit criterion must say step 9 is covered by the GE-3 tests only.

**`a6` on Windows (run `114524`): 23 failing of 1,906.** Mine, now fixed in the tests: the two nested-key tests above.
Not mine and listed in the first look: `lmd_snapshots` (4), `render_scripts_cli` (2), `render_dry_run`, `convert_positron_cli`,
`smoke_all` (14 smoke-all failures: `great_tables` not installed, and the `brand-monospace-*` typst files fail their
regexes), `preview_static_e2e` (2), `render_record_sources_cli`, `route_tests` (2), `import_route`, `book_downloads`,
`render_scripts_boot`, `live_reexecute`, `storage` double-lock (2). **Not on the base:**
`quarto-hub::integration git_epochs::unreachable_canonical_docs_wait_one_cap_not_one_per_doc` (8.8 s against a 1.5 s
limit; classified below as load-sensitive). The two `quarto-git-epochs` probe tests from the earlier
first look did not fail in this run. The tooling items `d1` (the `quarto-hub` failure text) and `d2` (which failures also
fail on the merge base with `main`) were added to answer this; their first run is below.

**`d1` and `d2` (run `152805`, base `748fd4603`, no `-WithTools`).** This run is not a clean `a6`: the machine\'s own
pandoc is 3.12.1, and q2 requires 3.13 (`Error [Q-20-2]: Pandoc Version Too Old`), so 100 tests failed, 99 of them also on
the merge base `f9936b08d` (the baseline run: 1,751 tests, 102 failed). Those 99 are environment, not code; rerun `a6` with
`-WithTools` (CI\'s pinned 3.13 and typst) to get the real list. What the two diagnostics settled:
- **`storage::test_storage_manager_prevents_double_lock` and `..._standalone_prevents_double_lock`** (`quarto-hub`, run alone
  by `d1`: 520 of 522 pass): `assertion failed: matches!(result, Err(Error::HubAlreadyRunning))` at `storage.rs:733` and
  `:782`. Both also fail on the merge base, so they predate the epic: a second lock attempt in one process does not fail
  on Windows. Known unrelated; not changed here.
- **`git_epochs::unreachable_canonical_docs_wait_one_cap_not_one_per_doc` passed** when `quarto-hub` ran alone (`d1`), and
  failed (8.8 s against a 1.5 s limit) only in the first full four-crate run, with 1,900 tests competing for the machine. The
  test times wall clock with a 300 ms margin (1.5 s limit; one cap is 0.6 s, three caps 1.8 s), which a loaded Windows host
  can overrun. Classified as a **load-sensitive timing assertion**, not a product defect. A sturdier test counts the attach
  attempts instead of the clock; not changed here (it is the epic\'s own test, so it needs a decision, below).

**Open (Gordon, on the Windows machine).**
1. `run.ps1 -Only a6,d2 -WithTools -Publish`: the real `a6` list with pandoc 3.13, and which of its failures also fail on
   the base. Then `c3` needs those classified as known unrelated or fixed.
2. Step 9: `$env:C1_GREP='step 9'; run.ps1 -Only c1 -Publish` (hidden-console fix, pushed as `ff9347d91`); then `c1` twice
   more for the step 7 intermittent.
3. `run.ps1 -Only c3 -WithTools -Publish`.
4. Still open from GE-16a: `b5,b6` with Developer Mode on, and `b4` with `QUARTO_HUB_DEBUG=1`.
5. Optionally `c2` with `-BuildEmbed` for the cross-client scenarios.

**Decisions for Gordon.** (a) Replace the wall-clock assertion in `unreachable_canonical_docs_wait_one_cap_not_one_per_doc`
with an attempt count, or leave it as a known Windows-under-load flake? (b) Fix the `join.e2e (b)` race (skip
`.quarto-hub-tmp-*`) here, or file it? (c) If step 9 still cannot deliver Ctrl+C on Windows, accept GE-3\'s tests as the
only coverage in the exit criteria?

**Also to do on the macOS side.** The `ge/17a-windows-fixes` branch must land with the plan: it carries three code commits
(`5c0ccc508`, `2e646f9cc`, `f47ec4a7b`). The tooling branch `windows/git-epochs` stays unmerged and undeleted (GE-18 deletes it).

**Session 2 (lane A, 2026-11-19, from 16:19 EST): runs `161637` and `163017`.**

- **Run `161637` (`a6,d2`) is void: a tooling defect.** `a0` failed with `could not find Cargo.toml in C:\Users\GordonWoodhull`.
  `run.ps1` called `Set-Location $repo`, which moves only PowerShell\'s location; `Invoke-Logged` starts `cmd.exe` through
  `Process.Start`, which inherits the .NET current directory, and that was the directory the shell was launched from.
  Run from inside the checkout it worked, which is why the earlier runs passed. `d2` then compared against the stale
  `152805` log, so its list of 82 "not on the base" failures is meaningless (it is mostly the pandoc 3.12.1 environment
  failures). Fix: `[Environment]::CurrentDirectory = $repo` after `Set-Location` (tooling commit `b5b9a4cb0`;
  `pwsh` parse check: 0 errors). The next `a6,d2` run must start the real list.
- **Run `163017` (`c1`, step 9 only): Ctrl+C was delivered, and the hub did not exit within 60 s.** `ctrl-c.ps1` returned 0
  (the hidden-console fix works), the marker was seen during the switch, the switch finished with code 0, and the hub logged
  `the shutdown flush was skipped`, so the shutdown handler ran and the G5 behavior is observed on Windows. The test failed
  only on `hubExit: "timeout"`. The hub log was not recorded, so a hang and a slow drain cannot be told apart yet. On macOS
  the same step takes 15 s from Ctrl+C to exit, and the final sync alone takes about 10 s with 4,005 docs, so a slow drain
  on Windows is plausible. Tooling change (same commit): step 9 waits 5 min, records `exitSeconds`, starts the hub with
  `RUST_LOG=quarto_hub=info,warn`, and records the hub log tail. macOS dry run: passed (exit code 0 after 15 s). The next
  step 9 run decides between slow and hung; do not classify before it.

**Session 2, later runs (about 17:00 to 18:00 EST).**

| Run | Item | Result |
|---|---|---|
| `165053` (tooling `81dfb1363`) | `c1` | steps 1 to 8 PASS (10.2 min); step 9 FAIL: no exit in 60 s (the same as `163017`) |
| `170520` | `b5`, `b6` | PASS: 57 passed, 1 skipped; 63 passed, 1 skipped |
| `170623` | `b4` | PASS: 131 passed, 0 skipped (the `events.jsonl` test passes) |
| `171347` (`b5b9a4cb0`, pandoc 3.13, `d2`) | `a0`, clippy | PASS |
| | `a6` nextest | 21 failing; `d2`: **20 also fail on the merge base** `f9936b08d`, 1 does not |
| `174516` (`7cceacd22`) | `c1` step 9 | **PASS**: Ctrl+C delivered, the flush skipped, the hub exited with code 0 after **154 s**, the restarted hub served the switched tree with no markers |

**Step 9, settled.** The hub does not hang; it is slow to exit on Windows. From the log tail: Ctrl+C at 22:50:07, the final
sync (which skips the flush) done at 22:50:18, and the process exits about 143 s later, while `ctx.shutdown()` waits for
samod\'s background storage writes of about 4,000 docs. The same step on macOS exits in 15 s in total. The 60 s limit in the
test was the wrong assumption; it is now 5 min. Recorded outcomes: Ctrl+C was delivered (`GenerateConsoleCtrlEvent` through
the hidden console), `shutdown flush was skipped` was logged with `unwritten=0`, and the restart served the switched tree
with no conflict markers. A 2.5 min exit after Ctrl+C on a 4,000-file project is a usability finding about storage drain
speed on Windows, not an epic defect (nothing here shows the epic added those writes; not measured against the base).

**`a6` classification (21 failures, run `171347`).** The 20 that also fail on the merge base are not caused by this epic and
are now on `known-unrelated.txt` (2 earlier, 18 added; tooling commit `6556b8c5e`): the `quarto` tests `render_dry_run`,
`convert_positron_cli`, four `lmd_snapshots`, two `preview_static_e2e`, `render_record_sources_cli`, two `render_scripts_cli`,
and `smoke_all`; the two `quarto-hub` storage double-lock tests; and the `quarto-preview` tests `import_route`, two
`route_tests`, `book_downloads::a_chapters_recorded_plot_reaches_the_book`, `live_reexecute`, and `render_scripts_boot`. The one
failure that is ours is `git_epochs::unreachable_canonical_docs_wait_one_cap_not_one_per_doc`, the wall-clock assertion
(three unreachable docs under 1.5 s with a 600 ms cap); it has now failed in 2 of 3 loaded full runs and passed alone. It
is not on the unrelated list, so `c3` fails on it until decision (a) is made. Its `fail_attach` hook cannot count attempts,
so an attempt-count version needs a new test hook in `quarto-hub`.

**`b4`, `b5`, `b6`.** `b4` passes with 131 of 131, so the `events.jsonl` test needed no `debug: disk` evidence. Whether
`QUARTO_HUB_DEBUG=1` was set for that run is not recorded in the results. `b5` and `b6` each skipped one test; the results do
not record whether Developer Mode was on, so the two symlink tests (GE-16a `B2`) may still be unproven.

**`c1` step 7 (intermittent).** Across five runs on identical code, one failed (`143919`); `150923` and `165053` passed. The
diagnostics have not fired. One more `c1` pair was requested.

**The final statement, end-to-end record, and landing are not written**: they wait for the `a6` classification with
pandoc 3.13, `c3`, and the step 7 and step 9 outcomes.

# q2 preview --static: content tracker seeded after the port opens

**Strand:** bd-tp0yym04
**Branch:** `braid/bd-tp0yym04-preview-tracker-seed-race`

## Symptom

`preview_static_e2e` tests flake on the ubuntu leg of CI with
`no \`reload\` event within 30s` or `no \`render-start\` event within 30s`.
Which test fails varies from run to run. Every failing test edits a file right after connecting:

| Run | Test | Missing event |
|---|---|---|
| 35910422721 (main) | `editing_an_input_rerenders_it_and_reloads_to_that_page` | `reload` |
| 35995038328 (main) | `editing_the_project_config_rerenders_every_page` | `reload` |
| 36056788947 (main, merge of #723) | `project_preview_keys_set_the_defaults_and_unsupported_keys_warn` | `reload` |
| 36095220439 (PR #726) | `editing_an_input_rerenders_it_and_reloads_to_that_page` | `render-start` |

The PRs that triggered these runs did not touch the preview code. For example, #723 only
changed pampa, tree-sitter and the error catalog.

## Root cause

PR #712 added `ContentTracker`
(`crates/quarto-preview/src/static_mode/watch_policy.rs`) to stop the
Linux re-render loop. With it, a watcher event counts as a change only when
the file's hash differs from the recorded one. The tracker is seeded from the
boot render's inputs and config.

In `run()` (`crates/quarto/src/commands/preview_static.rs`) the order was:

1. Boot render.
2. `start_watcher`. It deliberately runs before the port opens (see the
   "Watcher and signals, before anyone can connect" comment).
3. Bind the listener, print the `→ http://…` boot line, `tokio::spawn(serve)`.
   **Clients can connect from here on.**
4. `tracker.seed(...)`, which reads and hashes each input.

The e2e tests edit a file as soon as the SSE subscription succeeds. If the
driver's main thread is preempted anywhere in step 3, the test can connect,
subscribe and append before step 4 runs. Two points in step 3 invite
preemption on a loaded runner: the stdout write wakes the test process, and
`tokio::spawn` wakes a worker thread. The seed then records the **edited**
bytes as the baseline. When the loop later handles the edit's watcher event,
`tracker.changed()` compares equal and logs
`event without a content change; ignored`, so no render and no reload happen.

Users are affected too, not only the tests. A save made right after
the preview starts could be silently ignored.

## Fix

- [x] Move the seed into the before-anyone-can-connect block, right after
      `start_watcher`. The boot render has already finished, so the baseline
      is the same, but it is now recorded before the port exists. It still
      comes after the watcher starts, so on Linux the seed's own reads
      produce events that hash equal and are ignored, as before.
- [x] Test diagnostics: `SseReader::wait_for` used to panic without the
      server's stderr, so the CI logs gave no hint of the cause. `SseReader` now
      holds the captured stderr and includes it in all three panic messages.
- [x] Test diagnostics: `-v` maps to `info`, but the driver logs dropped
      events at `debug`. The harness now sets
      `RUST_LOG=quarto=info,q2=info,q2::commands::preview_static=debug`.
      `RUST_LOG` takes precedence over `-v`. The module target is `q2::…`
      because `commands` is compiled into the `q2` bin.

## Verification

- Stress script (edit the instant the boot line prints, 10 trials): 0 misses on
  both the old and the fixed binary on macOS. The window is too narrow to hit
  from a shell without help.
- **Widened window:** I injected `std::thread::sleep(300ms)` right after
  `tokio::spawn(serve)` in both versions (this change is not committed):
  - Old code: `editing_an_input_rerenders_it_and_reloads_to_that_page`
    fails with `no render-start`, the same as PR #726. The new stderr dump shows
    `DEBUG q2::commands::preview_static: event without a content change; ignored`.
  - Fixed code: all 5 edit-driven e2e tests pass with the same sleep.
- `cargo nextest run -p quarto -p quarto-preview`: 774 passed.
- `cargo clippy -p quarto --all-targets -D warnings`, `cargo fmt --check`: clean.
- `cargo xtask verify --skip-hub-build`: see the PR.

## Not done / follow-ups

- No deterministic regression test. Adding one would need a test-only delay
  hook in the production driver. The widened-window experiment above stands in
  for it. If this comes back, the new stderr dump will name the cause directly.
- Remaining small window: an edit made between `start_watcher` and the seed
  would also become the baseline. No client can connect yet at that point,
  and the boot render did not see such an edit either, so this is the same
  class of gap as any edit made during the boot render. It is out of scope here.

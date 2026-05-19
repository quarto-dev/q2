# 2026-05-15 — Quiet default logging in `q2 preview` and `q2 hub`

**Beads:** [bd-9mgd](../../.beads/issues.jsonl)
**Branch:** `beads/bd-9mgd-quiet-default-logging` (off `main`)

## Overview

`q2 preview` is chatty by default: every 5-second periodic-sync tick
emits three `INFO` lines from `quarto-hub`, even when nothing changed.
A representative sample (from a session with three files, no edits):

```
INFO quarto_hub::sync:   Starting sync of all documents count=3
INFO quarto_hub::sync:   Sync complete no_changes=3 automerge_changed=0 …
INFO quarto_hub::server: Periodic sync complete synced=3 no_changes=3 …
```

The project uses `tracing` + `tracing_subscriber::EnvFilter`. There is
no `-v` flag — verbosity is controlled only via `RUST_LOG`. The CLI's
default filter is `"quarto=info"`, which (via
`tracing-subscriber`'s `starts_with`-based target matching, see
`directive.rs:246` in 0.3.23) catches all workspace crates whose name
starts with `quarto`, including `quarto_hub` and `quarto_preview`.

Goal: clean terminal by default; full visibility behind a CLI flag and
behind `RUST_LOG`. Land in three phases on one PR.

## Phases

### Phase 1 — Demote per-event chatter

Move these from `info!` to `debug!`:

| File:line | Message | Why |
|---|---|---|
| `crates/quarto-hub/src/sync.rs:502` | `Starting sync of all documents` | per-tick |
| `crates/quarto-hub/src/sync.rs:586` | `Sync complete` (`sync_all` summary) | per-tick |
| `crates/quarto-hub/src/sync.rs:177,185,193` | per-file text-document sync result | per-edit-propagation |
| `crates/quarto-hub/src/sync.rs:333,341,349` | per-file binary-document sync result | per-edit-propagation |
| `crates/quarto-hub/src/server.rs:1064` | `Periodic sync complete` | per-tick |
| `crates/quarto-hub/src/server.rs:734,750` | WebSocket connect / disconnect | per-browser-load |
| `crates/quarto-hub/src/context.rs:482,528` | `Added new text/binary file to index` | per-file at startup |

Also tighten `server.rs:1063` gate from
`total_synced() > 0 || has_errors()` to
`(automerge_changed + filesystem_changed + both_changed) > 0 || has_errors()`.
`total_synced()` sums in `no_changes` (`sync.rs:623`), which is why
the gate fires every tick today. Belt-and-suspenders: even if a
future change moves the level back to `info!`, the all-no-changes
case won't log.

Keep at `info!` — fires a handful of times, not per-event:

- `Hub server listening` (`server.rs:948,950`)
- `starting q2 preview server` (`preview.rs:125`)
- `Initializing samod repo` / `samod repo initialized` (`context.rs:185,206`)
- `Created and persisted new index document` (`context.rs:215`)
- `Reconciled new files with index` (`context.rs:225`)
- `Received Ctrl-C / SIGTERM` / `Server shutting down` / `Performing final filesystem sync before shutdown` (`server.rs:1014,1031,1033,1169,1172`)
- `Starting filesystem watcher` / `Starting peer connection` (`server.rs:989`, `context.rs:249`)
- `recorded engine captures` (`quarto-preview/src/capture_driver.rs:120`) — fires once per eager-capture batch, low volume

### Phase 2 — `-v` flag on the `q2` root command

Wire `--verbose` (`-v`) as a global `clap::ArgAction::Count` on the
root `Cli` struct in `crates/quarto/src/main.rs` so every subcommand
inherits it (e.g. `q2 render -v`, `q2 preview -v`). Map the count to
a default `EnvFilter` directive:

| Count | Default directive |
|---|---|
| 0 (no flag) | `quarto=warn` |
| 1 (`-v`) | `quarto=info` |
| 2 (`-vv`) | `quarto=debug,samod=info` |
| 3+ (`-vvv`) | `quarto=trace,samod=debug,tower_http=debug` |

Keep `RUST_LOG`'s precedence: if it's set, it wins (today's
`try_from_default_env` path). Factor the count→directive mapping into
a pure function (`fn verbose_to_filter(count: u8) -> &'static str`)
that's table-tested.

Default floor at `warn` (not `info`) so the `info!` lines we kept
in Phase 1 become silent by default — they're useful with `-v`,
noise without it. Errors/warnings still surface.

### Phase 3 — Mirror the flag on `q2 hub`

`crates/quarto-hub/src/main.rs:108-114` initializes its own filter.
Add the same `--verbose` flag to its `Args` struct, reuse the
`verbose_to_filter` helper (lifted into a small shared module, see
"Open implementation question" below), and drop the
`tower_http=debug` from the level-0 default. `tower_http=debug` moves
to level 3 alongside the q2 case.

### Phase 4 — Verification

Per CLAUDE.md "end-to-end verification before declaring success":

- Build `q2`, run `cargo run --bin q2 -- preview <fixture>`, leave it
  running ~15 s. Confirm terminal stays quiet after the startup
  banner. Paste the captured output into the commit message.
- Repeat with `-v`, `-vv`, `-vvv`. Confirm each step adds the
  expected category of messages.
- `RUST_LOG=quarto_hub=debug cargo run --bin q2 -- preview <fixture>`
  — confirm override still works (debug lines appear without any
  flag).
- Same matrix for `cargo run --bin quarto-hub -- --project <fixture>`.

## Test plan (TDD; tests written first)

1. **`verbose_to_filter` unit test** — table-driven; assert directive
   strings for `0..=4` (clamped at 3 for `>=3`).
2. **(Optional, if cheap) integration smoke** — spawn `q2 preview` in
   a child process with no flag, scrape stderr for ~6 s, assert that
   `Periodic sync complete`, `Sync complete`, `Starting sync of all`
   are *absent*. Re-run with `-vv`, assert at least one appears.
   Skip if the existing preview test harness doesn't make this easy
   — the table test plus manual end-to-end is the floor.
3. **Pre-change grep** — `rg "Sync complete|Periodic sync complete|Starting sync of all|WebSocket client connected|WebSocket client disconnected|Added new text file to index|Added new binary file to index"` across the workspace to find any test that asserts on those exact strings. Update tests that do.
4. `cargo nextest run --workspace` after each phase.
5. `cargo xtask verify --skip-hub-build` before declaring done
   (Rust-only change; no WASM impact).

## Work items

### Phase 0 — Setup
- [x] Create beads issue (bd-9mgd)
- [x] Write plan file (this file)
- [ ] Branch off main: `beads/bd-9mgd-quiet-default-logging`

### Phase 1 — Demote chatter
- [x] Pre-change grep for assertions on the log strings — none found
- [x] Demote `sync.rs:502` (`Starting sync of all documents`) → `debug!`
- [x] Demote `sync.rs:586` (`Sync complete` summary) → `debug!`
- [x] Demote `sync.rs:177,185,193` (per-file text sync) → `debug!`
- [x] Demote `sync.rs:333,341,349` (per-file binary sync) → `debug!`
- [x] Demote `server.rs:965` (`Periodic sync complete`) → `debug!`
- [x] Tighten the gate at `server.rs:957`: added `SyncAllResult::has_changes()` (with unit test `has_changes_only_counts_real_changes`); gate now uses `has_changes() || has_errors()`
- [x] Demote `server.rs:734,750` (WS connect/disconnect) → `debug!`
- [x] Demote `context.rs:451,497` (per-file added to index) → `debug!`
- [x] Removed now-unused `info` import in `sync.rs`
- [x] `cargo nextest run --workspace` — 8864 passed, 195 skipped (no regressions)

### Phase 2 — `-v` on `q2`
- [x] Write `verbose_to_filter` table-driven unit tests in `quarto-util` (5 cases incl. clamp at u8::MAX)
- [x] Implement `verbose_to_filter` (placed in `quarto-util` from the start so Phase 3 reuses without churn)
- [x] Add `verbose: u8` (global, `ArgAction::Count`) to root `Cli` struct in `quarto/src/main.rs`
- [x] Wire `verbose_to_filter` into `main()` filter init; preserve `try_from_default_env` precedence
- [x] Manual end-to-end against `q2 hub --project <fixture>` matrix:
  - default → 0 stdout lines (silent)
  - `-v` → 14 INFO lines (kept startup/lifecycle only)
  - `-vv` → 33 lines (INFO + DEBUG including the demoted sync lines)
  - `-vvv` → 104 lines (incl. samod + tower_http DEBUG)
  - `RUST_LOG=error` → 0 lines; `RUST_LOG=error` + `-v` → 0 lines (env wins)
  - `RUST_LOG=quarto_hub::sync=debug` → 11 lines, exactly the demoted sync chatter
- [x] Note: end-to-end against `q2 preview` deferred — the preview MVP lives on `feature/q2-preview-command`, not yet merged into `main`. The change is purely additive to `quarto-hub` and the `quarto` root CLI; `q2 preview` will pick it up automatically on the next merge of `main` into the preview branch. The `q2 hub` exercise above covers the same `quarto-hub` logging code path the preview command uses.

### Phase 3 — `-v` on `q2 hub`
- [x] Add `verbose: u8` to `Args` in `quarto-hub/src/main.rs`
- [x] Add `quarto-util` to `quarto-hub`'s Cargo.toml dependencies (helper is shared from `quarto-util` since Phase 2)
- [x] Replace `"quarto_hub=info,tower_http=debug"` default with the shared mapping
- [x] Manual end-to-end on standalone `hub --project <fixture>` matrix — identical to Phase 2 output volumes (default=0, -v=14, -vv=33, -vvv=104, `RUST_LOG=error`=0)

### Phase 4 — Verification + commit
- [x] `cargo xtask verify --skip-hub-build` — all 9 steps passed
- [x] Capture terminal samples for the commit message (see below)
- [x] Update plan file checks as we go
- [x] `br close bd-9mgd --reason "..."` + `br sync --flush-only` + commit `.beads/` together with code changes

## Captured output for commit message

`q2 hub --project <tempdir>` over a 6-second window (one periodic-sync tick at the default 30s interval; tick timing is the user-controllable knob, not in scope here):

| Invocation | stdout lines | Contains |
|---|---|---|
| `q2 hub …` | 0 | (silent) |
| `q2 -v hub …` | 14 | startup + lifecycle INFO only (`Storage manager initialized`, `Initializing samod repo`, `Hub server listening`, `Received Ctrl-C`, `Server shutting down`, etc.) |
| `q2 -vv hub …` | 33 | adds DEBUG: per-file `Discovered .qmd file`, `Starting sync of all documents`, `Sync complete no_changes=…`, `Saved sync state` |
| `q2 -vvv hub …` | 104 | adds samod actor traces and tower_http=debug |
| `RUST_LOG=error q2 hub …` | 0 | env overrides flag |
| `RUST_LOG=error q2 -v hub …` | 0 | env overrides flag |
| `RUST_LOG=quarto_hub::sync=debug q2 hub …` | 11 | exactly the demoted sync-complete chatter |

Standalone `./target/debug/hub --project <tempdir>` matrix matches the q2 path 1:1 (0/14/33/104/0 for default/-v/-vv/-vvv/RUST_LOG=error).

## Implementation notes

### Where to put `verbose_to_filter`

Three options:

1. **`quarto-util`** — already a workspace utility crate; both
   `quarto` and `quarto-hub` could depend on it. Cleanest for a
   shared helper.
2. **A new small `quarto-cli-util` crate** — overkill for one
   function.
3. **Inline in each binary** — duplication, but the function is
   small enough that the duplication is harmless.

Going with **option 1 (`quarto-util`)** unless it pulls in a
dependency surface we don't want in `quarto-hub`. Check the existing
`quarto-util/Cargo.toml` before committing.

### `RUST_LOG` precedence — concrete shape

The existing pattern is:

```rust
tracing_subscriber::EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| "quarto=info".into())
```

The `try_from_default_env` returns `Err` only if `RUST_LOG` is
unset *or* the value fails to parse. We want the new behavior:

```rust
let default_directive = verbose_to_filter(cli.verbose);
tracing_subscriber::EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| default_directive.into())
```

`RUST_LOG`, when set, overrides the flag entirely. Considered: should
`-v` combine with `RUST_LOG` (e.g. `RUST_LOG=samod=trace q2 preview -v`)?
Not in this PR. Doing so would require parsing the env var and
merging directives — out of scope. The current behaviour matches the
expectation that "if you're using `RUST_LOG`, you know what you're
doing".

### What about `clap`'s `ArgAction::Count`?

`Count` returns `u8`. `clap` already gives us `-vvv` parsing for
free. Clamping at 3 happens in `verbose_to_filter`.

### Tracing-subscriber version note

Workspace uses `tracing-subscriber = { version = "0.3", features = ["env-filter"] }`
(top-level `Cargo.toml` line 50). No version bump needed.

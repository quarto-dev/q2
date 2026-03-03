# Hub: Optional Local Project Watching

**Beads issue**: `bd-3aga`

## Overview

Make local project watching optional in the hub server, so `hub` can serve as a
standalone sync server without being bound to a local Quarto project directory.

**Goal**: Two deployment modes from the same code:
- **`hub` binary**: Defaults to sync-only (no local project). Add `--project <path>` to watch a local project.
- **`quarto hub` command**: Defaults to watching the current project (current behavior). Add `--no-project` to run without a local project.

### Motivation

Currently, both `hub` and `quarto hub` require a local Quarto project directory.
The project root is used for:
1. `StorageManager` — creates `.quarto/hub/` under the project root for lockfile, config, automerge storage
2. `ProjectFiles::discover()` — walks the project tree for `.qmd`, config, and binary files
3. File watching — monitors the project directory for changes
4. Periodic sync — syncs automerge documents back to the filesystem
5. Initial sync — reconciles discovered files with the automerge index on startup

For a standalone sync server, items 2–5 should be skipped entirely. Item 1 needs
a different storage location (a standalone data directory instead of `.quarto/hub/`
inside a project).

## Work Items

### Phase 1: Storage decoupling

- [x] Add a `StorageManager::new_standalone(data_dir)` constructor that creates
      storage in a specified directory (e.g., `~/.quarto/hub/` or a user-provided
      path) without requiring a project root
- [x] Make `StorageManager::project_root()` return `Option<&Path>` instead of `&Path`
- [x] Update all callers of `project_root()` to handle `None`
- [x] Add `--data-dir <path>` CLI arg to `hub` binary (defaults to platform-appropriate
      default via `dirs::data_dir()`)

### Phase 2: Make discovery and sync optional

- [x] Guard `ProjectFiles::discover()` call in `HubContext::new()` — skip when no project root
- [x] Guard `reconcile_files_with_index()` — skip when no project root
- [x] Guard initial `sync_all_documents()` — skip when no project root
- [x] Guard periodic sync task in `run_server()` — skip when no project root
- [x] Guard file watcher task in `run_server()` — skip when no project root
- [x] Guard final shutdown sync — skip when no project root
- [x] Make `HubContext::project_files()` return `Option<&ProjectFiles>`
- [x] Make `HubContext::sync_all()` and `sync_file()` no-op when no project root

### Phase 3: CLI changes

- [x] **`hub` binary (`main.rs`)**: Change `--project` from "defaults to cwd" to
      "optional, no local project if omitted". When `--project` is given, watch
      that directory (current behavior). When omitted, run as sync-only server.
- [x] **`quarto hub` command**: Add `--no-project` flag. When present, do not
      watch any local project. Default behavior remains unchanged (watch cwd or
      `--project` path). Also added `--data-dir` flag for standalone data directory.
- [x] Update help text for both binaries to describe the two modes
- Note: `HubConfig` does not need `project_root` — the `StorageManager` already
  carries this information and it's checked via `has_project()` / `project_root()`.

### Phase 4: API adjustments

- [x] Update `GET /health` endpoint to handle missing project root gracefully
      (report `project_root: null` instead of a path)
- [x] Update `GET /api/files` endpoint to return empty list when no project
- [x] WebSocket sync works without a local project — samod handles pure
      automerge-to-automerge sync between clients and peers regardless of
      filesystem mode

### Phase 5: Tests

- [x] Add tests for `StorageManager::new_standalone()` (3 tests: creates data dir,
      has no project root, prevents double lock)
- [x] Add test for `StorageManager` project mode has project root
- [x] Verify existing tests still pass (6500 passed, 0 failures)
- [x] Add test for `HubContext::new()` without a project root
- [x] Add test for `HubContext::new()` in project mode (verifies file discovery)
- [ ] Add integration test that server starts and accepts WebSocket connections
      without a project (deferred — requires spawning a server in tests, which is
      complex; the unit tests cover the core logic)

## Design Notes

### Storage location for standalone mode

When `hub` runs without a project, it needs a place to store:
- `hub.lock` — prevent multiple instances
- `hub.json` — config (peers, index document ID)
- `automerge/` — samod document storage
- `sync-state.json` — not needed (no filesystem to sync with)

The `--data-dir` flag controls this. A sensible default would be platform-specific
(XDG on Linux, ~/Library/Application Support on macOS), but for simplicity we
can start with requiring it explicitly in standalone mode and add defaults later.

### What "sync-only" means

In sync-only mode, the hub:
- Accepts WebSocket connections from clients (hub-client instances)
- Syncs automerge documents between connected clients in real-time
- Connects to configured peer servers for federation
- Persists all automerge documents to disk (via samod's TokioFilesystemStorage)
- Does NOT read/write `.qmd` files on disk
- Does NOT discover project files
- Does NOT watch the filesystem

This is sufficient for a shared server where multiple users connect via hub-client
and collaborate on documents. The documents live entirely in automerge.

### Peer URL configuration still works

In sync-only mode, `--peer` still works for federating with other hub instances.
This is the primary use case: a central server that multiple hub-clients connect to,
optionally peering with other servers for redundancy.

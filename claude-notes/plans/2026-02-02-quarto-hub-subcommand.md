# Plan: Add `quarto hub` Subcommand

**Issue**: kyoto-3erh
**Date**: 2026-02-02

## Overview

Add a `quarto hub` subcommand to the `quarto` binary that provides the same functionality as the standalone `hub` binary. This allows users to start the collaborative editing server directly from the Quarto CLI without needing a separate binary.

The `hub` binary (`crates/quarto-hub/`) is already structured as both a binary and a library crate, with the server logic exposed through `quarto_hub::server::run_server()`. This makes integration straightforward.

## Current State

### `hub` Binary (`crates/quarto-hub/src/main.rs`)
- Uses clap with `#[derive(Parser)]` for argument parsing
- Accepts: `--project`, `--port`, `--host`, `--peer`, `--sync-interval`, `--no-watch`, `--watch-debounce`
- Async entry point (`#[tokio::main]`)
- Delegates to `quarto_hub::server::run_server(storage, config)`

### `quarto` Binary (`crates/quarto/src/main.rs`)
- Uses clap with `#[derive(Parser)]` and `#[derive(Subcommand)]`
- Flat subcommand structure (no nested subcommands)
- Commands defined as enum variants in `Commands`
- Each command has a module in `commands/`
- Synchronous `main()` (uses `pollster::block_on` for async operations)

### Library Exports (`crates/quarto-hub/src/lib.rs`)
- `StorageManager` - manages hub.json and file synchronization
- `HubConfig` - server configuration struct
- `server::run_server()` - main server entry point

## Work Items

- [x] Add `quarto-hub` dependency to `crates/quarto/Cargo.toml`
- [x] Add `Hub` variant to `Commands` enum with all CLI arguments
- [x] Create `crates/quarto/src/commands/hub.rs` module
- [x] Add `pub mod hub;` to `crates/quarto/src/commands/mod.rs`
- [x] Wire up the `Hub` command in `main.rs` match statement
- [x] Test `quarto hub` command works identically to `hub` binary
- [x] Update CLAUDE.md or other docs if needed

## Implementation Details

### 1. Add Dependency

In `crates/quarto/Cargo.toml`:
```toml
quarto-hub.workspace = true
```

### 2. Add Command Variant

In `crates/quarto/src/main.rs`, add to `Commands` enum:

```rust
/// Start collaborative hub server for real-time editing
Hub {
    /// Project root directory (defaults to current directory)
    #[arg(short, long)]
    project: Option<PathBuf>,

    /// Port to listen on
    #[arg(short = 'P', long, default_value = "3000")]
    port: u16,

    /// Host to bind to
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

    /// Sync server URL to peer with (can be specified multiple times)
    #[arg(long = "peer", value_name = "URL")]
    peers: Vec<String>,

    /// Periodic filesystem sync interval in seconds (0 to disable)
    #[arg(long, default_value = "30")]
    sync_interval: u64,

    /// Disable filesystem watching
    #[arg(long)]
    no_watch: bool,

    /// Debounce duration for filesystem events in milliseconds
    #[arg(long, default_value = "500")]
    watch_debounce: u64,
},
```

### 3. Create Command Module

Create `crates/quarto/src/commands/hub.rs`:

```rust
//! Hub command - collaborative editing server

use std::path::PathBuf;

use anyhow::Result;
use quarto_hub::{StorageManager, context::HubConfig, server};
use tracing::info;

pub struct HubArgs {
    pub project: Option<PathBuf>,
    pub port: u16,
    pub host: String,
    pub peers: Vec<String>,
    pub sync_interval: u64,
    pub no_watch: bool,
    pub watch_debounce: u64,
}

pub fn execute(args: HubArgs) -> Result<()> {
    // Build async runtime and run the server
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run_hub(args))
}

async fn run_hub(args: HubArgs) -> Result<()> {
    // Determine project root
    let project_root = args
        .project
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));
    let project_root = project_root
        .canonicalize()
        .expect("Failed to canonicalize project root");

    info!(project_root = %project_root.display(), "Starting hub");

    // Initialize storage
    let mut storage = StorageManager::new(&project_root)?;

    // Determine peers
    let peers = if !args.peers.is_empty() {
        storage.set_peers(args.peers.clone())?;
        info!(peers = ?args.peers, "Using peers from CLI");
        args.peers
    } else {
        let stored_peers = storage.peers().to_vec();
        if !stored_peers.is_empty() {
            info!(peers = ?stored_peers, "Using peers from hub.json");
        }
        stored_peers
    };

    // Configure server
    let sync_interval_secs = if args.sync_interval == 0 {
        None
    } else {
        Some(args.sync_interval)
    };

    let config = HubConfig {
        port: args.port,
        host: args.host,
        peers,
        sync_interval_secs,
        watch_enabled: !args.no_watch,
        watch_debounce_ms: args.watch_debounce,
    };

    server::run_server(storage, config).await?;

    Ok(())
}
```

### 4. Wire Up Command

In `crates/quarto/src/main.rs`, add to the match statement:

```rust
Commands::Hub {
    project,
    port,
    host,
    peers,
    sync_interval,
    no_watch,
    watch_debounce,
} => commands::hub::execute(commands::hub::HubArgs {
    project,
    port,
    host,
    peers,
    sync_interval,
    no_watch,
    watch_debounce,
}),
```

### 5. Import PathBuf

Add `use std::path::PathBuf;` at the top of `main.rs` if not already present.

## Testing Plan

1. Build and verify compilation:
   ```bash
   cargo build --package quarto
   ```

2. Verify help output matches `hub` binary:
   ```bash
   cargo run --package quarto -- hub --help
   cargo run --package quarto-hub -- --help
   ```

3. Test basic functionality:
   ```bash
   # Create test project
   mkdir /tmp/test-hub-project
   echo '---\ntitle: Test\n---\nHello' > /tmp/test-hub-project/test.qmd

   # Start hub via quarto
   cargo run --package quarto -- hub --project /tmp/test-hub-project

   # In another terminal, verify server is running
   curl http://127.0.0.1:3000/
   ```

4. Test CLI arguments work:
   ```bash
   cargo run --package quarto -- hub --port 8080 --host 0.0.0.0 --no-watch
   ```

## Notes

- The `quarto` binary's `main()` is synchronous, so we need to create a tokio runtime in the hub command's `execute()` function rather than using `#[tokio::main]`
- The hub command creates its own runtime rather than reusing `pollster::block_on` because the hub server needs a full tokio runtime for its async operations (websockets, file watching, etc.)
- All CLI arguments match the standalone `hub` binary exactly to ensure identical behavior

## Future Considerations

This implementation keeps the hub server as a standalone async future, which allows future composition with other servers (e.g., LSP) in a combined command.

A future combined command (e.g., `quarto serve-all` or similar) could spawn both the hub and LSP servers as concurrent tokio tasks within the same runtime:

```rust
async fn run_combined(hub_config: HubConfig, storage: StorageManager) -> Result<()> {
    let hub_handle = tokio::spawn(async move {
        server::run_server(storage, hub_config).await
    });

    let lsp_handle = tokio::spawn(async {
        quarto_lsp::run_server().await
    });

    tokio::select! {
        result = hub_handle => { /* hub exited */ }
        _ = lsp_handle => { /* lsp exited */ }
    }

    Ok(())
}
```

That future work may require:
- **Shared document state** between LSP and Hub (currently they have separate document stores)
- **Coordinated shutdown handling** (if one fails, gracefully stop the other)
- **Unified file watching** (avoid duplicate watchers on the same project)

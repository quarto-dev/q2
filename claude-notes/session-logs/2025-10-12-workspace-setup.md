# Session: Workspace Setup and Initial Architecture

**Date:** 2025-10-12
**Duration:** ~2 hours
**Focus:** Create Rust workspace skeleton, establish versioning strategy, design machine-readable I/O

## Summary

This session established the foundational structure for the Quarto Rust implementation (codename: kyoto). We created a complete workspace with 3 crates, implemented all 18 CLI commands with full option parsing, resolved versioning strategy questions, upgraded to Rust Edition 2024, and designed a comprehensive machine-readable I/O system.

## Accomplishments

### 1. Versioning Strategy (Option A)

**Problem:** Need to balance Rust ecosystem conventions (0.x.y for unstable) with Quarto's extension compatibility requirements (version must compare > 1.x).

**Solution:** Dual versioning approach
- `Cargo.toml`: `version = "0.1.0"` (idiomatic Rust, signals instability)
- CLI reports: `99.9.9-dev` (ensures extension compatibility)
- Implementation: `quarto-util/src/version.rs` with `cli_version()` function

**Documentation:** `claude-notes/versioning-strategy.md`

### 2. Workspace Structure

Created Cargo workspace with 3 initial crates:

```
crates/
├── Cargo.toml              # Workspace config
├── quarto/                 # Main CLI binary
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         # Entry point with clap parser
│       └── commands/       # 18 command modules (stubs)
├── quarto-core/            # Core rendering infrastructure
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       └── error.rs        # QuartoError types
└── quarto-util/            # Shared utilities
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        └── version.rs      # Version handling
```

**Architecture:** Follows turborepo-style workspace + cargo-style commands directory pattern (as documented in `rust-cli-organization-patterns.md`)

### 3. Complete CLI Definition

All 18 Quarto commands implemented with full clap parsing:

- `render` - Complete options: --to, --output, --execute, --cache, metadata, profiles, pandoc args
- `preview` - Server options, rendering control, watch settings
- `serve`, `create`, `use`, `add`, `update`, `remove` - Extension/project management
- `convert` - Document conversion
- `pandoc`, `typst` - Embedded tool execution
- `run` - Script execution
- `list`, `install`, `uninstall`, `tools` - Dependency management
- `publish`, `check`, `call` - Publishing and system access

**Status:** All commands return "Command not yet implemented" errors. CLI parses correctly and shows comprehensive help.

**Testing:**
```bash
$ ./target/debug/quarto --version
quarto 99.9.9-dev

$ ./target/debug/quarto render --help
# Shows all render options

$ cargo test
# 2 tests passing (version handling)
```

### 4. Rust Edition 2024 Upgrade

**Decision:** Upgraded from Edition 2021 to Edition 2024

**Rationale:**
- Improved temporary lifetimes (prevents deadlocks in `if let`)
- Better unsafe ergonomics (`unsafe_op_in_unsafe_fn`)
- RPIT capture improvements
- Reserves `gen` keyword for future generators
- Stable as of Rust 1.85.0 (Feb 2025)

**Result:** Clean upgrade, all tests pass, no code changes needed

### 5. Machine-Readable I/O Design

**Motivation:** Quarto is often invoked programmatically. Need consistent machine-readable I/O across all commands.

**Decision:** Option C - Hybrid Approach
- Global `--format` flag (human/json/yaml)
- Per-command `--json` flags for backward compatibility with v1
- Auto-detection: JSON if piped, Human if terminal

**Architecture:**
- `Outputable` trait for all result types
- `OutputWriter` handles formatting and I/O separation
- stdout: Structured data only (JSON/YAML)
- stderr: Human-readable progress/errors
- Line-delimited JSON for streaming operations

**Pattern:** Follows cargo's `--message-format json` and ripgrep's `--json`

**Documentation:** `claude-notes/machine-readable-io-design.md` (comprehensive 200+ line design document with code examples, implementation phases, testing strategy)

## Key Design Decisions

### Versioning
- **Cargo version:** 0.1.0 (signals instability)
- **CLI version:** 99.9.9-dev (extension compatibility)
- **Rationale:** Best of both worlds - Rust-idiomatic crates + Quarto compatibility

### Edition
- **Choice:** 2024 (with nightly toolchain)
- **Rationale:** Latest safety/ergonomics improvements, stable, forward-compatible

### CLI Architecture
- **Pattern:** Workspace + commands directory
- **Rationale:** 17+ subcommands need organization; enables parallel compilation, reusable crates, third-party engines

### Machine-Readable I/O
- **Pattern:** Global --format + per-command --json
- **Rationale:** Cargo/ripgrep proven patterns; backward compatible; separates data from presentation

## Files Created/Modified

### New Files
- `crates/Cargo.toml` - Workspace configuration (updated)
- `crates/quarto/Cargo.toml` - CLI binary crate
- `crates/quarto/src/main.rs` - CLI entry point with full clap parser
- `crates/quarto/src/commands/*.rs` - 18 command stubs
- `crates/quarto-core/Cargo.toml` - Core infrastructure crate
- `crates/quarto-core/src/lib.rs` - Core library entry
- `crates/quarto-core/src/error.rs` - Error types
- `crates/quarto-util/Cargo.toml` - Utilities crate
- `crates/quarto-util/src/lib.rs` - Utilities entry
- `crates/quarto-util/src/version.rs` - Version handling with tests
- `claude-notes/versioning-strategy.md` - Versioning design doc
- `claude-notes/machine-readable-io-design.md` - I/O design doc
- `claude-notes/session-logs/2025-10-12-workspace-setup.md` - This file

### Modified Files
- `crates/README.md` - Updated with complete workspace documentation
- `claude-notes/00-INDEX.md` - Added new docs to index, updated technical decisions and completion status

## Dependencies Added

```toml
[workspace.dependencies]
# CLI and error handling
clap = { version = "4.5", features = ["derive", "cargo"] }
anyhow = "1.0"
thiserror = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
```

**Future additions (for machine-readable I/O):**
- `colored` - Terminal colors for human output
- `clap-serde-derive` - Config file integration

## Build Status

```bash
$ cargo build
   Compiling quarto-util v0.1.0
   Compiling quarto-core v0.1.0
   Compiling quarto v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.54s

$ cargo test
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured

$ cargo check --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.08s
```

**Toolchain:**
- Rust nightly (configured via `rust-toolchain.toml`)
- Edition 2024
- All tests passing

## Next Steps

### Immediate Priorities

1. **Implement machine-readable I/O infrastructure** (Week 1)
   - Add `OutputFormat` enum
   - Implement `OutputWriter` and `Outputable` trait
   - Add `colored` and dependencies

2. **First working command** (Week 2)
   - Implement basic `render` for simple markdown
   - Create `RenderResult` with `Outputable` impl
   - Test JSON/human output modes

3. **Core rendering pipeline** (Week 3)
   - Basic pipeline in `quarto-core`
   - Markdown engine (simplest case)
   - File I/O and path handling

### Medium Term

4. **Additional crates as needed:**
   - `quarto-engines` - Engine system (jupyter, knitr, julia)
   - `quarto-filters` - Pandoc filter system
   - `quarto-yaml` - YAML infrastructure
   - `quarto-formats` - Output format definitions

5. **Config file integration:**
   - Add `clap-serde-derive`
   - Implement layered config loading
   - Support `_quarto.yml`, user config, env vars

6. **Extend commands:**
   - `inspect`, `check`, `list`, `tools` (simpler commands)
   - Add streaming support for long operations
   - Implement per-command `--json` flags

### Long Term

- Complete all 18 commands
- LSP server implementation
- Extension system
- Third-party engine API
- Parallelization for project renders

## Questions Resolved

**Q: Why Rust 2021 instead of nightly?**
A: Clarified - using nightly *toolchain* with edition 2024. Toolchain vs edition are separate concepts.

**Q: Should we use edition 2024 instead of 2021?**
A: Yes, upgraded. Edition 2024 stable since Feb 2025, provides safety/ergonomic improvements valuable for large projects.

**Q: How to handle machine-readable output?**
A: Comprehensive design created. Hybrid approach with global `--format` flag + per-command `--json` for backward compatibility. Follows cargo/ripgrep patterns.

## Notes for Next Session

1. The workspace skeleton is complete and builds successfully
2. All command-line interfaces are defined; implementations are stubs
3. Versioning strategy is documented and implemented
4. Machine-readable I/O design is comprehensive and ready to implement
5. Focus should shift to implementing first working command (`render` for simple markdown)
6. Consider creating `quarto-engines` crate early to establish engine trait
7. The `Outputable` trait pattern will be key to consistent machine-readable output

## References

- `claude-notes/rust-cli-organization-patterns.md` - CLI architecture rationale
- `claude-notes/versioning-strategy.md` - Dual versioning approach
- `claude-notes/machine-readable-io-design.md` - Comprehensive I/O design
- `crates/README.md` - Complete workspace documentation
- [Rust CLI Book - Machine Communication](https://rust-cli.github.io/book/in-depth/machine-communication.html)
- [Cargo message format](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages)

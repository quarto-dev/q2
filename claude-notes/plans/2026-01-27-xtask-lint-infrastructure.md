# Xtask Lint Infrastructure Plan

**Issue**: kyoto-e6h
**Created**: 2026-01-27
**Status**: Planning

## Overview

Create a custom linting infrastructure using `cargo xtask` and the `syn` crate to mechanically detect build violations. The first check will detect references to `external-sources/` in `include_dir!` macros, but the infrastructure should be extensible for future lint rules.

## Background

### The Problem

We accidentally introduced `include_dir!("...external-sources/quarto-cli/...")` references in the SASS compilation code. This broke the build in CI and for other users because:

1. **external-sources is not version-controlled**: The `external-sources/` directory contains reference implementations (like `quarto-cli`) that are checked out separately and may be at different commits on different machines.

2. **Reproducibility is not guaranteed**: Unlike the main repository, the exact contents of `external-sources/` are not meant to be identical across deployments. Different developers may have different versions, or none at all.

3. **CI doesn't have external-sources**: CI builds typically don't check out the external reference repositories, so any code that depends on them at compile time will fail.

4. **Silent breakage**: The build works fine for the developer who has `external-sources/` set up, making it easy to accidentally commit code that breaks for everyone else.

The fix was to copy required resources to a local `resources/` directory that IS version-controlled. But we need mechanical enforcement to prevent this from happening again.

### Why Xtask?

The `cargo xtask` pattern is a convention for project-specific automation tasks. Instead of shell scripts or Makefiles, tasks are written in Rust and invoked via `cargo xtask <command>`. This provides:
- Type safety and IDE support
- Cross-platform compatibility
- Access to the Rust ecosystem (syn, walkdir, etc.)
- Can run in CI alongside other Rust tooling

## Architecture

```
crates/xtask/
├── Cargo.toml
├── src/
│   ├── main.rs           # CLI entry point
│   ├── lint/
│   │   ├── mod.rs        # Lint orchestration
│   │   ├── external_sources.rs  # Check for external-sources references
│   │   └── rules.rs      # Future: more lint rules
│   └── lib.rs            # Shared utilities
```

## Work Items

### Phase 1: Basic Infrastructure

- [x] Create `crates/xtask` crate with basic CLI structure
- [x] Add xtask alias to workspace `.cargo/config.toml`
- [x] Implement file discovery (find all `.rs` files in crates/)
- [x] Add basic `cargo xtask lint` command that lists found files

### Phase 2: Syn-based Macro Detection

- [x] Add `syn` dependency with `full` and `visit` features
- [x] Implement AST visitor to find macro invocations
- [x] Detect `include_dir!` macro calls
- [x] Extract string literal arguments from macro calls
- [x] Report violations with file path and line number

### Phase 3: External Sources Rule

- [x] Implement rule: `include_dir!` must not contain `external-sources/`
- [x] Add clear error messages explaining the violation
- [x] Suggest fix (copy to local resources/)
- [ ] Add `--fix` flag placeholder for future auto-fixing

### Phase 4: Extensibility & Polish

- [ ] Create trait-based lint rule interface for adding new rules
- [x] Add `--quiet` and `--verbose` flags
- [x] Return proper exit codes (0 = pass, 1 = violations found)
- [x] Add documentation to CLAUDE.md about running lints
- [x] CI integration (GitHub Actions) - added to `.github/workflows/test-suite.yml`

## Technical Details

### Xtask Setup

Add to `.cargo/config.toml`:
```toml
[alias]
xtask = "run --package xtask --"
```

### Cargo.toml for xtask
```toml
[package]
name = "xtask"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
syn = { version = "2", features = ["full", "visit", "parsing"] }
walkdir = "2"
proc-macro2 = "1"
```

### Syn Visitor Pattern

```rust
use syn::visit::Visit;

struct MacroFinder {
    violations: Vec<Violation>,
    file_path: PathBuf,
}

impl<'ast> Visit<'ast> for MacroFinder {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("include_dir") {
            // Parse the macro tokens to find string literals
            // Check for forbidden patterns
        }
        syn::visit::visit_macro(self, mac);
    }
}
```

### String Literal Extraction

The `include_dir!` macro takes a string literal. We need to:
1. Parse the macro's token stream
2. Look for string literals (LitStr)
3. Check the string value against forbidden patterns

### Exit Codes

- `0`: All checks pass
- `1`: One or more violations found
- `2`: Error during lint execution

## Future Lint Rules (Ideas)

Once the infrastructure is in place, we can add more rules:
- Detect `unwrap()` in non-test code
- Check for missing `#[must_use]` on functions returning Result
- Verify all public items have documentation
- Check for hardcoded paths that should be configurable
- Detect TODO/FIXME comments without associated issues

## Testing

- [ ] Add unit tests for the AST visitor
- [ ] Add integration test with sample files containing violations
- [ ] Test that clean code passes without errors

## References

- [cargo-xtask](https://github.com/matklad/cargo-xtask) - Original pattern description
- [syn crate](https://docs.rs/syn) - Rust source parsing
- [include_dir crate](https://docs.rs/include_dir) - The macro we're checking

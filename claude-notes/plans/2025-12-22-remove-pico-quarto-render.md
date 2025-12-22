# Plan: Remove pico-quarto-render crate

## Summary

Remove the stale `pico-quarto-render` crate from the workspace. This was a prototype that is no longer needed now that actual rendering work has started.

## Analysis

### What pico-quarto-render is
- A minimal rendering tool for testing (160KB total)
- Located at `crates/pico-quarto-render/`
- Contains: `main.rs`, `embedded_resolver.rs`, `format_writers.rs`, `template_context.rs`, tests, and fixtures

### Dependencies analysis
- **pico-quarto-render depends on**: `pampa`, `quarto-doctemplate`, `anyhow`, `clap`, `walkdir`, `include_dir`, `rayon`
- **Nothing depends on pico-quarto-render**: Verified by searching all Cargo.toml files in `crates/`

### References found
| Location | Action Required |
|----------|----------------|
| `crates/pico-quarto-render/` | **DELETE** - the crate itself |
| `CLAUDE.md` line 116 | **EDIT** - remove from binaries list |
| `Cargo.lock` | **AUTO** - will update on next `cargo build` |
| `.beads/issues.jsonl` | **NONE** - closed historical issues, safe to keep |
| `claude-notes/plans/*.md` | **NONE** - historical documentation, safe to keep |
| `claude-notes/analysis/*.md` | **NONE** - historical documentation, safe to keep |

### Workspace configuration
The root `Cargo.toml` uses `members = ["crates/*"]`, so removing the directory automatically removes the crate from the workspace. No edit to `Cargo.toml` is needed.

### CI/CD impact
- `.github/workflows/test-suite.yml` - does NOT reference pico-quarto-render
- `.github/workflows/build-wasm.yml` - does NOT reference pico-quarto-render
- No scripts in `scripts/` reference pico-quarto-render

## Execution steps

### Step 1: Remove the crate directory
```bash
rm -rf crates/pico-quarto-render
```

### Step 2: Update CLAUDE.md
Remove line 116: `- `pico-quarto-render`: minimal rendering tool for testing`

### Step 3: Verify the workspace still builds
```bash
cargo build
cargo nextest run
```

The `cargo build` will also regenerate `Cargo.lock` without the removed crate.

## Risk assessment

**Risk: LOW**

- No other crates depend on pico-quarto-render (verified)
- The crate is not referenced in CI workflows
- Historical references in beads/plans are benign (they document past work)
- The workspace glob pattern means no Cargo.toml edit is needed

## Rollback

If needed, the crate can be restored from git:
```bash
git checkout HEAD -- crates/pico-quarto-render
```

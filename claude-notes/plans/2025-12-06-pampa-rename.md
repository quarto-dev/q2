# Rename quarto-markdown-pandoc to pampa

**Beads issue**: k-z1ji (closed)
**Status**: Completed
**Date**: 2025-12-06
**Related**: 2025-12-06-project-naming.md (naming decision)

## Summary

Rename the `quarto-markdown-pandoc` crate to `pampa` per the project naming decision. The codename "kyoto" remains for the workspace, and "quarto" remains for the main Quarto binary.

## Scope of Changes

### 1. Directory Rename

```
crates/quarto-markdown-pandoc → crates/pampa
```

Use `git mv` to preserve history.

### 2. Cargo.toml Changes

**Root Cargo.toml** (`/Cargo.toml`):
- Line 4: `"crates/quarto-markdown-pandoc/fuzz"` → `"crates/pampa/fuzz"`
- Line 60-61: Rename workspace dependency:
  ```toml
  [workspace.dependencies.pampa]
  path = "./crates/pampa"
  ```

**crates/pampa/Cargo.toml** (formerly quarto-markdown-pandoc):
- Line 2: `name = "quarto-markdown-pandoc"` → `name = "pampa"`
- Add explicit binary section (for clarity):
  ```toml
  [[bin]]
  name = "pampa"
  path = "src/main.rs"
  ```

**crates/pampa/fuzz/Cargo.toml**:
- Line 2: `name = "quarto-markdown-pandoc-fuzz"` → `name = "pampa-fuzz"`
- Lines 14-15: Update dependency:
  ```toml
  [dependencies.pampa]
  path = ".."
  ```

**crates/wasm-qmd-parser/Cargo.toml**:
- Line 14: `quarto-markdown-pandoc = {...}` → `pampa = {...}`

**crates/pico-quarto-render/Cargo.toml**:
- Line 14: `quarto-markdown-pandoc = { workspace = true }` → `pampa = { workspace = true }`

**crates/qmd-syntax-helper/Cargo.toml**:
- Line 25: `quarto-markdown-pandoc.workspace = true` → `pampa.workspace = true`

### 3. Binary Name Changes

**crates/pampa/src/main.rs**:
- Line 37: `#[command(name = "quarto-markdown-pandoc")]` → `#[command(name = "pampa")]`
- Line 38: Update about text if desired

### 4. Source Code Import Changes

Search pattern: `use quarto_markdown_pandoc`
Replace with: `use pampa`

Files to update:
- `crates/wasm-qmd-parser/src/lib.rs` (3 imports + 6 type references)
- `crates/pico-quarto-render/src/format_writers.rs` (multiple imports)
- `crates/pico-quarto-render/src/template_context.rs` (multiple imports)
- `crates/pico-quarto-render/tests/end_to_end.rs` (multiple imports)
- `crates/qmd-syntax-helper/src/diagnostics/q_2_30.rs`
- `crates/qmd-syntax-helper/src/conversions/grid_tables.rs`
- `crates/qmd-syntax-helper/src/conversions/definition_lists.rs`
- `crates/pampa/fuzz/fuzz_targets/hello_fuzz.rs`
- ~20 test files in `crates/pampa/tests/` (will be automatically correct after directory rename since they use the crate name)

**Documentation files with code examples** (update examples):
- `crates/quarto-error-reporting/README.md`
- `crates/quarto-error-reporting/CONTRIBUTING-ERRORS.md`

Note: Internal module imports within pampa itself use `crate::` so they don't need changes.
Note: Historical references in `claude-notes/` don't need updates.

### 5. VSCode Launch Configuration

**`.vscode/launch.json`**: Update all occurrences:
- `--package=quarto-markdown-pandoc` → `--package=pampa`
- `--bin=quarto-markdown-pandoc` → `--bin=pampa`
- `"name": "quarto-markdown-pandoc"` → `"name": "pampa"`
- `"name": "quarto_markdown_pandoc"` → `"name": "pampa"`
- `--package=quarto-markdown-pandoc-fuzz` → `--package=pampa-fuzz`
- File paths: `crates/quarto-markdown-pandoc/...` → `crates/pampa/...`

### 6. Documentation Updates

**Root CLAUDE.md**:
- Line 113: Update description
- Line 142: Update wasm-qmd-parser description reference

**crates/pampa/README.md**:
- Update heading if referencing old name
- Content is mostly name-agnostic

**ts-packages/README.md**:
- Line 10: Update reference from "quarto-markdown-pandoc JSON output"

### 7. Other Files

**Insta snapshots** (if any contain paths):
- Check `crates/pampa/snapshots/` for embedded paths
- May need to regenerate with `cargo insta test --accept`

**Fuzz test files**:
- Check `crates/pampa/fuzz/fuzz_targets/` for any hardcoded references

### 8. Files NOT Requiring Changes

- **Beads issues** (`.beads/issues.jsonl`): Historical references, no need to modify
- **Claude notes/plans**: Historical references in completed plans are fine
- **Internal struct names**: Per user request, keeping internal struct names unchanged for now
- **Cargo.lock**: Will be regenerated automatically

### 9. Cleanup: Unused error-message-macros

The directory `crates/quarto-markdown-pandoc/error-message-macros/` appears unused:
- Main crate uses `quarto-error-message-macros` from `quarto-parse-errors/error-message-macros/`
- Consider deleting or clarifying if this is dead code

## Approach Evaluation

### Option A: Simple Rename (Recommended)

Just rename everything directly.

**Pros:**
- Simple, direct, immediately clear
- Clean break
- No confusion about which name to use

**Cons:**
- Breaks any external references immediately

### Option B: Rename with Alias

Keep a shim crate at old location that re-exports from pampa.

**Pros:**
- Backwards compatibility for external code

**Cons:**
- Extra maintenance burden
- Confusing with two names
- Not needed for internal development

### Option C: Phased Rename

1. Add pampa as alias
2. Migrate references gradually
3. Remove old name later

**Pros:**
- Can catch issues gradually

**Cons:**
- More complex, longer transition
- Overkill for internal project

## Recommendation

**Option A (Simple Rename)** because:

1. This is internal development, not a published crate
2. No external consumers need backwards compatibility
3. Clean break avoids naming confusion
4. Scope is manageable in one session

## Execution Plan

### Phase 1: Preparation
1. Ensure clean working tree (`git status`)
2. Run full test suite to establish baseline (`cargo nextest run`)
3. Create a new branch for the rename

### Phase 2: Directory Rename
1. `git mv crates/quarto-markdown-pandoc crates/pampa`

### Phase 3: Cargo Configuration
1. Update root Cargo.toml (workspace members and dependencies)
2. Update crates/pampa/Cargo.toml (package name, add [[bin]])
3. Update crates/pampa/fuzz/Cargo.toml
4. Update dependent crates' Cargo.toml files

### Phase 4: Source Code
1. Update main.rs command name
2. Update all `use quarto_markdown_pandoc` imports to `use pampa`

### Phase 5: Supporting Files
1. Update .vscode/launch.json
2. Update documentation (CLAUDE.md files, READMEs)

### Phase 6: Verification
1. `cargo clean` (clear any cached artifacts)
2. `cargo build` (verify compilation)
3. `cargo nextest run` (verify all tests pass)
4. Test binary invocation: `cargo run --bin pampa -- --help`

### Phase 7: Cleanup
1. Verify unused error-message-macros directory
2. Delete if confirmed unused
3. Commit changes with descriptive message

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Git history confusion | Low | Low | Use `git mv`, history preserved per-file |
| IDE caching issues | Medium | Low | Document reload workspace step |
| Snapshot path issues | Medium | Medium | Check snapshots, regenerate if needed |
| Cargo cache issues | Low | Low | Run `cargo clean` after rename |
| Missing references | Medium | Medium | Use comprehensive grep search |
| External documentation links | N/A | N/A | Internal project, no external docs |

## Post-Rename Verification Checklist

- [ ] `cargo build` succeeds
- [ ] `cargo nextest run` all tests pass
- [ ] `cargo run --bin pampa -- --help` works
- [ ] `cargo run --bin pampa -- -i test.qmd -t json` produces output
- [ ] WASM build works (if applicable)
- [ ] VSCode debugging configurations work
- [ ] No orphaned references to old name (final grep check)

## Notes

- The structs inside pampa (like Pandoc AST types) keep their current names per user request
- The workspace codename "kyoto" and the `quarto` binary name are unchanged
- This rename is part of reducing dependency on "Pandoc" in naming, making the tool identity clearer

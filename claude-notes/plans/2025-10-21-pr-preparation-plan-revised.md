# PR Preparation Plan (REVISED): Extract crates/ Changes from kyoto to 2025-10-21

<!-- quarto-error-code-audit-ignore-file -->

**Date**: 2025-10-21
**Goal**: Extract ONLY `crates/` changes from `kyoto` branch to `2025-10-21` branch for PR to quarto-dev/quarto-markdown

**Key Constraint**: NO `private-crates/` changes should be transferred (those stay in the private repo only)

## Current Situation

### Branch States
- **`main`**: At commit `28840b4` (includes new crates merge)
- **`2025-10-21`**: At commit `28840b4` (same as main)
- **`kyoto`**: Diverged from `20887b8`, contains:
  - ~88 changed files in `crates/`
  - ~44 changed files in `private-crates/` (EXCLUDE from PR)
  - Many changed files in root (claude-notes, etc.) (EXCLUDE from PR)

### What We're Transferring
Changes in `crates/` directory ONLY:
1. **k-69**: Complete source-map migration
2. **k-70/k-83**: Remove SourceLocation trait
3. **k-90/k-95**: YAML tag-based metadata markdown parsing with diagnostics
4. **k-103**: In-memory content support for ariadne rendering
5. **k-109**: Partial work
6. **Today's fix**: Restore `_scope: lexical` blockmetadata behavior

## Strategy: Selective File Checkout

Since we only want `crates/` changes (not a full branch merge), we'll use a **selective checkout** approach:

1. Checkout 2025-10-21 branch (based on main)
2. Checkout ONLY the `crates/` directory from kyoto
3. Commit those changes
4. Test and verify

This avoids:
- Bringing in private-crates/ changes
- Bringing in claude-notes/ changes
- Bringing in root-level changes (CLAUDE.md, .gitignore, etc.)

## Step-by-Step Plan

### Phase 1: Safety Preparations

**Step 1.1: Verify current state**
```bash
# Check that kyoto tests pass
git checkout kyoto
cargo test

# Check git status is clean
git status
```

**Step 1.2: Verify 2025-10-21 state**
```bash
# Check that 2025-10-21 is at same commit as main
git checkout 2025-10-21
git log -1
git diff main  # Should show no differences

# Verify tests pass on 2025-10-21
cargo test
```

**Step 1.3: List files to transfer**
```bash
# Get list of all changed files in crates/ directory
git diff --name-only main...kyoto -- crates/ > /tmp/crates-changes.txt

# Review the list to ensure it's what we expect
cat /tmp/crates-changes.txt
```

### Phase 2: Transfer crates/ Changes

**Step 2.1: Checkout crates/ directory from kyoto**
```bash
# Ensure we're on 2025-10-21
git checkout 2025-10-21

# Checkout the entire crates/ directory from kyoto
git checkout kyoto -- crates/

# Check what changed
git status
```

**Step 2.2: Review the changes**
```bash
# See the diff
git diff --stat

# Review specific files if needed
git diff crates/quarto-markdown-pandoc/src/pandoc/meta.rs
git diff crates/quarto-markdown-pandoc/src/readers/qmd.rs
```

**Step 2.3: Handle any workspace-level changes**
If `Cargo.toml` or `Cargo.lock` at workspace level need updates:
```bash
# Check if workspace Cargo files changed
git diff Cargo.toml Cargo.lock

# If they have crates-related changes, selectively apply them
# Otherwise, reset them
git checkout HEAD -- Cargo.toml Cargo.lock
```

### Phase 3: Build and Test

**Step 3.1: Update Cargo.lock**
```bash
# Let cargo update the lock file for the new changes
cargo check
```

**Step 3.2: Run tests**
```bash
# Run all tests
cargo test

# If any tests fail, investigate and fix
```

**Step 3.3: Verify specific functionality**
```bash
# Test the _scope: lexical fix
cargo run --bin quarto-markdown-pandoc -- -i crates/quarto-markdown-pandoc/tests/snapshots/json/003.qmd -t json

# Test YAML tag parsing
cargo run --bin quarto-markdown-pandoc -- -i crates/quarto-markdown-pandoc/tests/claude-examples/meta-warning.qmd

# Test error messages
cargo run --bin quarto-markdown-pandoc -- -i crates/quarto-markdown-pandoc/resources/error-corpus/001.qmd
```

### Phase 4: Commit

**Step 4.1: Format code**
```bash
cargo fmt
```

**Step 4.2: Review changes one more time**
```bash
# Ensure ONLY crates/ changes are included
git status
git diff --stat

# Make sure no private-crates/ changes snuck in
git diff --name-only | grep private-crates  # Should be empty
```

**Step 4.3: Commit**
```bash
git add crates/

# Also add Cargo.lock if it changed
git add Cargo.lock

# Commit with comprehensive message
git commit -m "Merge kyoto crates/ work: source-map migration, YAML tags, and lexical scope fix

This commit brings in all the crates/ directory changes from the kyoto branch:

- k-69: Complete source-map migration to quarto_source_map::SourceInfo
- k-70/k-83: Remove old SourceLocation trait, update JSON writer
- k-90/k-95: YAML tag-based metadata markdown parsing with diagnostics
  - !str, !path, !md tags for controlling markdown parsing
  - Warning Q-1-101 for untagged values that fail markdown parsing
  - Error Q-1-100 for !md tagged values that fail markdown parsing
- k-103: In-memory content support for ariadne error rendering
- k-109: Partial work on improved error messages
- Fix: Restore _scope: lexical blockmetadata behavior
  - Added is_string_value() helper to MetaValueWithSourceInfo
  - Fixed regression where lexical block metadata was incorrectly merged into document metadata

All tests pass. No private-crates/ changes included."
```

### Phase 5: Final Verification

**Step 5.1: Verify the commit**
```bash
# Check the commit
git show --stat

# Verify no unwanted files
git show --name-only | grep -E "private-crates|claude-notes"  # Should be empty
```

**Step 5.2: Final test run**
```bash
# Clean build
cargo clean
cargo test
```

**Step 5.3: Compare with kyoto**
```bash
# Check that crates/ matches between branches
git diff kyoto -- crates/  # Should show no differences
```

## Potential Issues and Solutions

### Issue 1: Workspace Cargo.toml conflicts
**Problem**: kyoto might have workspace-level Cargo.toml changes that reference private-crates/

**Solution**:
- Manually review Cargo.toml changes
- Only keep changes related to crates/, not private-crates/
- Or: Reset workspace Cargo.toml and let it auto-update

### Issue 2: Cargo.lock conflicts
**Problem**: Cargo.lock might have dependencies from private-crates/

**Solution**:
- Reset Cargo.lock: `git checkout HEAD -- Cargo.lock`
- Run `cargo check` to regenerate it for crates/ only

### Issue 3: Tests fail after transfer
**Problem**: Some tests might depend on private-crates/ or root-level changes

**Solution**:
- Investigate which tests fail
- Fix by either:
  - Bringing in minimal necessary changes
  - Updating tests to work without those dependencies
  - Removing tests that depend on private work

### Issue 4: Cross-dependencies between crates/ and private-crates/
**Problem**: If crates/ code references private-crates/ code

**Solution**:
- This shouldn't happen by design (crates/ should be standalone)
- If it does, need to refactor before transferring
- Or: Include minimal private-crates/ changes (but avoid if possible)

## Success Criteria
- [ ] Only crates/ directory changes are on 2025-10-21
- [ ] NO private-crates/ changes
- [ ] NO claude-notes/ changes
- [ ] NO root-level changes (CLAUDE.md, .gitignore, etc.)
- [ ] All tests pass
- [ ] Code is formatted with cargo fmt
- [ ] Specific functionality verified:
  - [ ] `_scope: lexical` works correctly
  - [ ] YAML tags work correctly (!str, !md, !path)
  - [ ] Source tracking works correctly
  - [ ] Ariadne error messages work correctly
- [ ] `git diff kyoto -- crates/` shows no differences

## Alternative Approach (If Needed)

If the selective checkout approach has issues, we can use an alternative:

**Create patch and apply**:
```bash
# Create a patch of just crates/ changes
git diff main...kyoto -- crates/ > /tmp/kyoto-crates.patch

# Checkout 2025-10-21
git checkout 2025-10-21

# Apply the patch
git apply /tmp/kyoto-crates.patch

# Review and commit
git add crates/
git commit
```

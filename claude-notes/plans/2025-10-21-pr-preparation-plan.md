# PR Preparation Plan: Transfer kyoto Changes to 2025-10-21

**Date**: 2025-10-21
**Goal**: Transfer all `crates/*` changes from `kyoto` branch to `2025-10-21` branch for PR to quarto-dev/quarto-markdown

## Current Situation

### Branch States
- **`main`**: At commit `28840b4` (includes new crates merge)
- **`2025-10-21`**: At commit `28840b4` (same as main)
- **`kyoto`**: Diverged from `20887b8` (before new crates merge)

### Merge Base
- `kyoto` and `main` diverged at commit `20887b8` ("Fix: shortcode parsing (#69)")
- `main` has one additional commit: `28840b4` ("new crates: quarto-source-map, quarto-yaml, quarto-error-reporting, ... (#71)")

### Work on kyoto (in chronological order)
1. k-69: Source-map migration (multiple commits)
2. k-70/k-83: Remove SourceLocation trait
3. k-90/k-95: YAML tag-based metadata markdown parsing
4. k-103: In-memory content support for ariadne
5. k-109: Partial work
6. **Today's fix**: Restore blockmetadata behavior (98d383a)

### Files Changed
- Approximately **50 files** modified in `crates/` directory
- Primarily in `crates/quarto-markdown-pandoc/`
- Some changes in `crates/quarto-error-reporting/`, `crates/quarto-source-map/`, `crates/quarto-yaml/`

## Strategy: Rebase kyoto onto main, then merge into 2025-10-21

### Why Rebase?
- `kyoto` branched from `20887b8`
- `main` (and `2025-10-21`) are at `28840b4`
- We need kyoto's changes to be compatible with the state at `28840b4`
- Rebasing will replay kyoto's commits on top of `28840b4`

### Risks
- Potential conflicts during rebase (especially if the new crates merge touched overlapping files)
- Need to verify tests pass after rebase

## Step-by-Step Plan

### Phase 1: Safety Preparations
1. **Create a backup branch** from current kyoto
   ```bash
   git branch kyoto-backup-2025-10-21
   ```

2. **Verify current kyoto state**
   - Ensure all tests pass on kyoto
   - Check git status is clean

3. **Verify 2025-10-21 state**
   - Check that 2025-10-21 is clean
   - Confirm it's at the same commit as main

### Phase 2: Rebase kyoto onto main
1. **Checkout kyoto branch**
   ```bash
   git checkout kyoto
   ```

2. **Rebase onto main**
   ```bash
   git rebase main
   ```

3. **Resolve any conflicts**
   - If conflicts occur, resolve them carefully
   - For each conflict:
     - Understand what changed in both branches
     - Preserve kyoto's intent while incorporating main's changes
     - Mark resolved with `git add`
     - Continue rebase with `git rebase --continue`

4. **Test after rebase**
   ```bash
   cargo test
   ```
   - If tests fail, investigate and fix
   - Commit any fixes as part of the rebase

### Phase 3: Merge into 2025-10-21
1. **Checkout 2025-10-21 branch**
   ```bash
   git checkout 2025-10-21
   ```

2. **Merge rebased kyoto**
   ```bash
   git merge kyoto --no-ff -m "Merge kyoto work: source-map migration, YAML tags, ariadne support, and lexical scope fix"
   ```

3. **Verify merge**
   - Check that all expected changes are present
   - Run `cargo test` to ensure tests pass
   - Run `cargo fmt` to ensure code is formatted

### Phase 4: Final Verification
1. **Review changes**
   ```bash
   git diff main..2025-10-21 -- crates/
   ```

2. **Test comprehensively**
   ```bash
   cargo test
   cargo build --release
   ```

3. **Check specific functionality**
   - Test the `_scope: lexical` fix with `003.qmd`
   - Test YAML tag parsing with meta-error.qmd and meta-warning.qmd
   - Verify source location tracking

### Phase 5: Cleanup
1. **Format code**
   ```bash
   cargo fmt
   ```

2. **Review commit history**
   ```bash
   git log --oneline main..2025-10-21
   ```

3. **If needed, squash/organize commits** (optional)
   - Could use interactive rebase to clean up commit history
   - But probably not necessary if commits are well-organized

## Potential Issues and Solutions

### Issue 1: Merge conflicts during rebase
**Solution**:
- Carefully review each conflict
- Understand the intent of both changes
- Prefer kyoto's changes for crates/ directory
- If uncertain, ask user for guidance

### Issue 2: Tests fail after rebase
**Solution**:
- Investigate which tests are failing
- Check if it's due to new crates structure from main
- Fix issues and commit as part of rebase

### Issue 3: Rebase becomes too complex
**Solution**:
- Abort rebase: `git rebase --abort`
- Try alternative: Create new branch from main and cherry-pick commits
- Or: Merge main into kyoto first, then merge kyoto into 2025-10-21

## Success Criteria
- [ ] All kyoto commits are on 2025-10-21
- [ ] All tests pass on 2025-10-21
- [ ] Code is formatted with cargo fmt
- [ ] Changes only affect crates/ directory (no private-crates/ changes)
- [ ] Specific fixes verified:
  - [ ] `_scope: lexical` works correctly
  - [ ] YAML tags work correctly
  - [ ] Source tracking works correctly
  - [ ] Ariadne error messages work correctly

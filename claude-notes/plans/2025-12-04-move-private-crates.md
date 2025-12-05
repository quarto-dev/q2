# Plan: Move private-crates to crates

**Status**: Completed
**Created**: 2025-12-04

## Overview

Move all 5 crates from `private-crates/` to `crates/`, unifying the workspace structure.

### Crates to Move

| Crate | Type | Dependencies (internal) |
|-------|------|------------------------|
| `quarto-util` | library | none (leaf) |
| `quarto-core` | library | quarto-util |
| `quarto` | binary | quarto-core, quarto-util |
| `quarto-yaml-validation` | library | quarto-yaml, quarto-source-map, quarto-error-reporting |
| `validate-yaml` | binary | quarto-yaml-validation, quarto-yaml, quarto-error-reporting, quarto-source-map |

### Key Observation

All workspace dependencies are defined in the root `Cargo.toml`, so moving all crates at once with a single path update is cleaner than moving incrementally.

---

## Step-by-Step Plan

### Phase 1: Move Directories

```bash
# Move all 5 crates using git mv to preserve history
git mv private-crates/quarto-util crates/quarto-util
git mv private-crates/quarto-core crates/quarto-core
git mv private-crates/quarto crates/quarto
git mv private-crates/quarto-yaml-validation crates/quarto-yaml-validation
git mv private-crates/validate-yaml crates/validate-yaml

# Remove the now-empty private-crates directory
rmdir private-crates
```

### Phase 2: Update Root Cargo.toml

**File**: `/Cargo.toml`

Changes needed:

1. **Remove `private-crates/*` from workspace members**:
   ```toml
   # Before
   members = [
       "crates/*",
       "crates/quarto-markdown-pandoc/fuzz",
       "private-crates/*",
   ]

   # After
   members = [
       "crates/*",
       "crates/quarto-markdown-pandoc/fuzz",
   ]
   ```

2. **Update workspace dependency paths** (3 crates referenced):
   ```toml
   # Before
   [workspace.dependencies.quarto-core]
   path = "./private-crates/quarto-core"

   [workspace.dependencies.quarto-util]
   path = "./private-crates/quarto-util"

   [workspace.dependencies.quarto-yaml-validation]
   path = "./private-crates/quarto-yaml-validation"

   # After
   [workspace.dependencies.quarto-core]
   path = "./crates/quarto-core"

   [workspace.dependencies.quarto-util]
   path = "./crates/quarto-util"

   [workspace.dependencies.quarto-yaml-validation]
   path = "./crates/quarto-yaml-validation"
   ```

### Phase 3: Verify Build

```bash
# Verify workspace compiles
cargo check --workspace

# Run all tests
cargo nextest run
```

### Phase 4: Update Documentation

#### 4.1 CLAUDE.md

Update the "Workspace structure" section:

**Before** (lines 107-124):
```markdown
### `crates` - corresponds to the crates in the public quarto-markdown repo
...

### `private-crates` - private crates we are not going to release yet

- `private-crates/quarto-yaml-validation`: A library to validate YAML objects using schemas
- `private-crates/validate-yaml`: A binary to exercise `quarto-yaml-validation`
- `private-crates/quarto`: The future main entry point for the `quarto` command line binary.
- `private-crates/quarto-core`: supporting library for `quarto`
```

**After**:
```markdown
### `crates` - all Rust crates in the workspace

- `crates/qmd-syntax-helper`: a binary to help users convert qmd files to the new syntax
- `crates/quarto-error-reporting`: a library to help create uniform, helpful, beautiful error messages
- `crates/quarto-markdown-pandoc`: a binary to parse qmd text and produce Pandoc AST and other formats
- `crates/quarto-source-map`: a library to help maintain information about the source location of data structures in text files
- `crates/quarto-yaml`: a YAML parser that produces YAML objects and accurate fine-grained source location of elements
- `crates/tree-sitter-qmd`: tree-sitter grammars for block and inline parsers
- `crates/wasm-qmd-parser`: A WASM module with some entry points from `crates/quarto-markdown-pandoc`
- `crates/quarto-yaml-validation`: A library to validate YAML objects using schemas
- `crates/validate-yaml`: A binary to exercise `quarto-yaml-validation`
- `crates/quarto`: The main entry point for the `quarto` command line binary
- `crates/quarto-core`: supporting library for `quarto`
- `crates/quarto-util`: shared utilities for Quarto crates
```

(Note: also add the other crates that are currently undocumented in CLAUDE.md)

#### 4.2 claude-notes/dependency-rules.md

This file can be significantly simplified or removed entirely since the distinction between public/private no longer exists.

**Option A**: Delete the file entirely
**Option B**: Simplify to just document the crate dependency order

Recommendation: Keep a simplified version that documents legitimate layering rules (e.g., `quarto-util` should remain a leaf dependency).

#### 4.3 scripts/README.md

Update line 21 from:
```
- Finds all Q-*-* error codes in the codebase (crates + private-crates)
```
to:
```
- Finds all Q-*-* error codes in the codebase
```

### Phase 5: Update Historical Documentation (Optional)

The following files in `claude-notes/` reference `private-crates/` in historical context (plans, audits, investigations). These references are **historical records** and may not need updating:

- `claude-notes/audits/2025-10-27-bd-8-phase1-schema-from-yaml-audit.md`
- `claude-notes/plans/2025-11-21-k-87-refresh-audit.md`
- `claude-notes/completion/2025-10-27-bd-8-complete.md`
- `claude-notes/investigations/2025-11-23-error-code-audit-results.md`
- `claude-notes/investigations/2025-11-23-add-missing-catalog-entries.md`
- `claude-notes/workflows/2025-11-23-error-code-audit-workflow.md`
- `claude-notes/plans/2025-10-27-*.md` (multiple)
- `claude-notes/plans/2025-10-21-*.md` (multiple)

**Recommendation**: Leave these as-is. They document historical state. Add a note at the top of `dependency-rules.md` (if kept) noting when the merge happened.

---

## Checklist Summary

- [ ] Phase 1: `git mv` all 5 directories
- [ ] Phase 1: Remove empty `private-crates/` directory
- [ ] Phase 2: Update `Cargo.toml` workspace members
- [ ] Phase 2: Update 3 workspace dependency paths in `Cargo.toml`
- [ ] Phase 3: `cargo check --workspace` passes
- [ ] Phase 3: `cargo nextest run` passes
- [ ] Phase 4: Update `CLAUDE.md` workspace structure section
- [ ] Phase 4: Update/simplify `claude-notes/dependency-rules.md`
- [ ] Phase 4: Update `scripts/README.md`
- [ ] Phase 5: (Optional) Add historical note to documentation

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Name collision | None | Verified: no existing crate in `crates/` shares names with private crates |
| Build failure | Low | Simple path changes; cargo workspaces handle this well |
| CI failure | Low | CI uses workspace-level commands; no hardcoded paths |
| Documentation drift | Low | Most references are historical and don't need updating |

---

## Questions for Review

1. **Historical docs**: Should we update the `claude-notes/` historical files, or leave them as historical records?

2. **dependency-rules.md**: Delete entirely, or keep a simplified version documenting crate layering?

3. **Commit strategy**: Single atomic commit, or separate commits for (a) moves, (b) Cargo.toml, (c) docs?

4. **Additional crates to document**: The current CLAUDE.md doesn't list all crates in `crates/` (missing: pico-quarto-render, quarto-citeproc, quarto-csl, quarto-doctemplate, quarto-pandoc-types, quarto-parse-errors, quarto-treesitter-ast, quarto-xml, tree-sitter-doctemplate). Should we add them all while we're updating the docs?

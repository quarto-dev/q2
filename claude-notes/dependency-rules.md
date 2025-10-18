# Workspace Dependency Rules

## Critical Rule: One-Way Dependencies Only

**IMPORTANT**: Crates in `crates/` (public-ready) must NEVER depend on crates in `private-crates/`.

### Directory Structure

```
kyoto/
├── crates/              # Public-ready crates
│   ├── quarto-error-reporting/
│   ├── quarto-source-map/
│   ├── quarto-yaml/
│   ├── quarto-markdown-pandoc/
│   └── ...
└── private-crates/      # Private crates (not yet ready for public)
    ├── quarto-core/
    ├── quarto-util/
    ├── quarto-yaml-validation/
    └── ...
```

### Allowed Dependencies

✅ **Allowed**: `crates/` → `crates/` (public depends on public)
✅ **Allowed**: `private-crates/` → `crates/` (private depends on public)
✅ **Allowed**: `private-crates/` → `private-crates/` (private depends on private)

❌ **FORBIDDEN**: `crates/` → `private-crates/` (public depends on private)

### Rationale

The `crates/` directory contains crates that will be published to the public repository. They cannot depend on private crates because:
1. Those dependencies won't exist in the public repository
2. It creates a coupling that prevents independent migration
3. It breaks the clean separation between public and private code

### Migration Workflow

When moving a crate from `private-crates/` to `crates/`:
1. Move the crate directory: `git mv private-crates/X crates/X`
2. Update workspace path in root `Cargo.toml`
3. **Check**: Ensure no public crate now depends on private crates
4. Run `cargo check --workspace` to verify
5. Commit the change

### Verification

Before committing changes, always check that no public crate depends on private crates:

```bash
# Check dependencies from public crates
cd crates
for dir in */; do
  echo "=== $dir ==="
  grep -r "path.*private-crates" "$dir/Cargo.toml" 2>/dev/null && echo "❌ VIOLATION FOUND"
done
```

## Current Public Crates

As of 2025-10-18:
- `quarto-error-reporting` - Error reporting infrastructure
- `quarto-source-map` - Source location tracking
- `quarto-yaml` - YAML parsing with source tracking
- `quarto-markdown-pandoc` - Markdown to Pandoc AST converter
- `tree-sitter-qmd` - Tree-sitter grammar
- `wasm-qmd-parser` - WASM bindings
- `qmd-syntax-helper` - Syntax helper tools

## Current Private Crates

As of 2025-10-18:
- `quarto` - Main CLI binary
- `quarto-core` - Core rendering infrastructure
- `quarto-util` - Shared utilities
- `quarto-yaml-validation` - YAML validation
- `validate-yaml` - YAML validation CLI

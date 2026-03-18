# Windows Development Notes

## v8/Deno Test Limitation

The `v8` crate does not produce an rlib on Windows, causing **test compilation**
(not regular builds) to fail. `cargo build --workspace` works fine because library
crates only need `.rmeta` for type checking. Test binaries require rlib from every
dependency for static linking, which v8 cannot provide on Windows.

This cascades to all crates that transitively depend on v8 via `quarto-system-runtime`:

| Crate | Dependency path |
|-------|----------------|
| `quarto-system-runtime` | direct v8 dep (via deno_core) |
| `pampa` | → quarto-system-runtime |
| `quarto-core` | → quarto-system-runtime |
| `quarto-sass` | → quarto-system-runtime |
| `quarto-test` | → quarto-system-runtime |
| `quarto` | → pampa, quarto-core |
| `quarto-project-create` | → quarto-system-runtime |
| `qmd-syntax-helper` | → pampa |
| `comrak-to-pandoc` | → pampa |
| `quarto-lsp` | → quarto-lsp-core |
| `quarto-lsp-core` | → quarto-core |
| `reconcile-viewer` | → pampa |

### Running Tests on Windows

Use `cargo xtask test` — it automatically excludes v8-dependent crates on Windows:

```bash
cargo xtask test                                    # run all testable crates
cargo xtask test -- -p quarto-doctemplate           # run a specific crate
cargo xtask test -- --no-fail-fast                  # don't stop on first failure
cargo xtask test --deny-warnings                    # match CI strictness
```

Nextest's `default-filter` only controls which tests *run*, not which *compile*.
`cargo xtask test` handles this by passing `--exclude` flags for each affected crate.
The exclude list is maintained in `crates/xtask/src/test.rs`.

### Testable crates on Windows

These workspace crates have no v8 dependency and can compile tests:

- `quarto-pandoc-types`, `quarto-yaml`, `quarto-yaml-validation`
- `quarto-xml`, `quarto-csl`, `quarto-citeproc`, `quarto-doctemplate`
- `quarto-treesitter-ast`, `tree-sitter-qmd`, `tree-sitter-doctemplate`
- `quarto-source-map`, `quarto-error-reporting`, `quarto-error-message-macros`
- `quarto-parse-errors`, `quarto-util`, `quarto-ast-reconcile`
- `quarto-hub`, `quarto-config`, `quarto-analysis`
- `validate-yaml`, `xtask`

## Dev Drive

For faster builds and tests on Windows, use a Dev Drive (ReFS volume).
See `memory://main/docs/windows-dev-drive` for setup details.

Key benefit: Windows process creation is slower than Unix, and nextest spawns one
process per test. Dev Drive + antivirus exclusions can reduce build/test times
significantly.

## CI

Windows is not currently in the CI matrix. All Windows testing is manual.

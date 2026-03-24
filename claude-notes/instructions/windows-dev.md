# Windows Development Notes

## Running Tests on Windows

Use `cargo xtask test` to run the full workspace test suite:

```bash
cargo xtask test                                    # run all workspace crates
cargo xtask test -- -p quarto-doctemplate           # run a specific crate
cargo xtask test -- --no-fail-fast                  # don't stop on first failure
cargo xtask test --deny-warnings                    # match CI strictness
```

### Historical note: v8 rlib exclusions (resolved 2026-03-23)

The v8 crate previously did not produce rlib on Windows, requiring 12 crates to be
excluded from test compilation. This was resolved (likely by an upstream v8/deno_core
update) and all workspace crates now compile tests on Windows.

## Dev Drive

For faster builds and tests on Windows, use a Dev Drive (ReFS volume).
See `memory://main/docs/windows-dev-drive` for setup details.

Key benefit: Windows process creation is slower than Unix, and nextest spawns one
process per test. Dev Drive + antivirus exclusions can reduce build/test times
significantly.

## CI

Windows is not currently in the CI matrix. All Windows testing is manual.

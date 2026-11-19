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

## pampa-fuzz

The `pampa-fuzz` crate is excluded from the workspace (`exclude` in root `Cargo.toml`).
`libfuzzer-sys` requires nightly Rust and only builds on Linux/macOS — it fails with
`LNK1561` on Windows. This does not affect any other crate; run fuzz targets from
`crates/pampa/` on a supported platform.

## Dev Drive

For faster builds and tests on Windows, use a Dev Drive (ReFS volume).
See `memory://main/docs/windows-dev-drive` for setup details.

Key benefit: Windows process creation is slower than Unix, and nextest spawns one
process per test. Dev Drive + antivirus exclusions can reduce build/test times
significantly.

## CI

Windows is not in the automatic CI matrix. `test-suite.yml` has a compile-only
`windows-clippy` job that runs on every PR. Every other leg is opt-in through a
shared `runners` input, so the platform is a separate axis from the suite:

| Workflow | Default runners | Extra inputs |
|---|---|---|
| `test-suite.yml` (Rust, nextest) | `ubuntu-latest`, `macos-latest` | `filter` (nextest `-E` expression) |
| `ts-test-suite.yml` (TS suites) | `ubuntu-latest`, `macos-latest` | |
| `hub-client-e2e.yml` (Playwright) | `ubuntu-latest` | `run-smoke-all`, ... |
| `q2-preview-spa-e2e.yml` (Playwright) | `ubuntu-latest` | |

Push, PR and scheduled runs always use the default. To add Windows, start a
manual run with `runners` set to a JSON array, like quarto-cli's `test-smokes.yml`:

```bash
gh workflow run test-suite.yml --ref <branch> \
  -f runners='["windows-latest"]' -f filter='package(pampa)'
gh workflow run q2-preview-spa-e2e.yml --ref <branch> -f runners='["windows-latest"]'
```

On Windows each job runs its `run:` steps under Git Bash, so the steps stay POSIX.
The Windows legs of the TS and Playwright workflows have not been run yet; expect
to fix setup steps (WASM build, Playwright browsers) on the first runs. The Rust
leg skips the tree-sitter grammar test and the Jupyter kernel setup on Windows.

The pre-landing gate is still a cross-check, which works
from any OS because `cargo check`/`cargo clippy` do not link:

```bash
rustup target add x86_64-pc-windows-msvc   # once
cargo clippy -p <crate> --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

Run it for every crate you touch when your change uses `std::os::unix` /
`std::os::windows`, adds `#[cfg(...)]` gates, or touches test helpers. The full
rules and patterns are in `.claude/rules/cross-platform.md`; failure history:
bd-j0pgiips, bd-lhi82470, bd-5tdzaf3t.

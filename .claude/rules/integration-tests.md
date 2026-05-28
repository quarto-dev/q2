# Integration test layout

All integration tests in this workspace live in
`tests/integration/<name>.rs` + a `tests/integration/main.rs` that
declares each as `pub mod <name>;`. Cargo compiles **one
`integration` binary per crate** instead of one binary per file.

**Do not add new `tests/<name>.rs` files at the top level of any
crate's `tests/` directory.** Cargo would treat each one as its
own test binary that statically links the crate's full dependency
closure — pampa's closure alone is ~130 MB. That was the bloat we
removed in bd-xvdop (commits `1b420d65` through `1592e8cd`), worth
~9 GB in `target/debug` and ~6.5 GB in `target/release` at the
workspace level.

## Adding a new integration test

1. Create the test file: `crates/<crate>/tests/integration/<your_test>.rs`.
2. Register it in `main.rs`:
   ```rust
   // crates/<crate>/tests/integration/main.rs
   pub mod <your_test>;
   ```
   Keep the list alphabetized.

That's it — nextest still runs each `#[test]` in its own process,
so test isolation is preserved. Filter expressions use the new
selector form `package(<crate>) & binary(integration) & test(<file>::)`.

## If you move test files

Any **source-file-relative path** in the moved files needs to be
re-evaluated against the new location. The pampa pilot migration
burned several iterations on insta `set_snapshot_path` calls that
silently resolved to the wrong directory.

Audit grep before declaring a move done:

```bash
grep -nE 'include_str!|include_bytes!|include_dir!|#\[path|set_snapshot_path|"\.\./' \
  crates/<crate>/tests/integration/*.rs
```

Each `../` needs to be re-checked: moving from `tests/foo.rs` to
`tests/integration/foo.rs` adds one directory level, so any
relative path inside the file usually needs one more `../`.

Insta `.snap` files also live in a snapshot directory adjacent to
the test source by default. If you move a test that uses default
insta snapshot paths, the existing `.snap` files need to move too:

- Directory: `tests/snapshots/` → `tests/integration/snapshots/`
- Filename: gains an `integration__` prefix because insta's
  `module_path!()`-based filenames now start with the binary's
  name (`integration`)

## Why this matters (the measurements)

From bd-xvdop's Phase 6 measurement (controlled, back-to-back
`cargo clean` + `cargo build --workspace --tests`, alternating
between baseline and full rollout):

|                            |  Before |    After |      Δ |
| -------------------------- | ------: | -------: | -----: |
| target/debug               |   21 GB |    12 GB | −43 % |
| target/release             |   11 GB |   4.5 GB | −59 % |
| Executables in deps/       |     220 |       76 | −65 % |
| Sum of executable bytes    | 10.5 GiB |  2.5 GiB | −77 % |
| Release-mode build wall    |   158 s |    120 s | −24 % |

The two extra `../` characters in a relative path are easy to get
wrong; the disk savings are not. Prefer the convention even when it
feels redundant.

## References

- `claude-notes/plans/2026-05-28-integration-test-consolidation.md`
- `claude-notes/research/2026-05-28-integration-test-bloat.md`
- [matklad: "Delete Cargo Integration Tests"](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html)
- [posit-dev/ark#1240](https://github.com/posit-dev/ark/pull/1240) —
  the precedent that motivated this change

## Fuzzing

Requires **nightly Rust** and **Linux or macOS** (`libfuzzer-sys` does not build on Windows).

This crate is excluded from the workspace (`exclude` in root `Cargo.toml`), so `cargo build --workspace` won't touch it. Run fuzz targets directly from the `pampa` crate directory:

```
$ cd crates/pampa
$ cargo fuzz run hello_fuzz --fuzz-dir ./fuzz
```

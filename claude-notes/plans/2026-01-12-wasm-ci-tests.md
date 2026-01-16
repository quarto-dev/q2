# Plan: Add WASM Compilation Tests to CI

**Issue**: k-685
**Status**: Draft - awaiting review

## Problem

The two WASM crates (`wasm-qmd-parser` and `wasm-quarto-hub-client`) are excluded from the default workspace build. Currently, nothing verifies they compile on push/PR, so WASM builds can silently break.

Note: This is purely about build verification. Artifact production for `wasm-qmd-parser` remains in the existing manual `build-wasm.yml`. Artifact production for `wasm-quarto-hub-client` will eventually go through hub-client's npm script.

## Proposed Solution

Add a `wasm-build-check` job to `test-suite.yml` that:
1. Runs on Linux only (simpler, no LLVM setup needed)
2. Builds both WASM crates with wasm-pack
3. Does NOT upload artifacts (not needed for verification)

## Implementation

Add this job to `.github/workflows/test-suite.yml`:

```yaml
  wasm-build-check:
    runs-on: ubuntu-latest
    name: WASM build check
    if: github.repository == 'quarto-dev/kyoto'

    steps:
      - uses: actions/checkout@v4

      - name: Set up Rust nightly
        uses: dtolnay/rust-toolchain@nightly
        with:
          targets: wasm32-unknown-unknown

      - name: Cache Rust dependencies
        uses: Swatinem/rust-cache@v2
        with:
          cache-on-failure: true

      - name: Set up Clang
        uses: egor-tensin/setup-clang@v1
        with:
          version: latest
          platform: x64

      - name: Install wasm-pack
        run: cargo install wasm-pack

      - name: Build wasm-qmd-parser
        run: |
          cd crates/wasm-qmd-parser
          export CFLAGS_wasm32_unknown_unknown="-I$(pwd)/wasm-sysroot -Wbad-function-cast -Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H"
          wasm-pack build --target web --dev

      - name: Build wasm-quarto-hub-client
        run: |
          cd crates/wasm-quarto-hub-client
          export CFLAGS_wasm32_unknown_unknown="-I$(pwd)/wasm-sysroot -Wbad-function-cast -Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H"
          wasm-pack build --target web
```

## Notes

- Runs in parallel with existing `test-suite` job (not a dependency)
- Uses `--dev` for wasm-qmd-parser to match existing build-wasm.yml
- Uses default (release) for wasm-quarto-hub-client to match hub-client's build script
- No artifacts uploaded - this is verification only

## Files to Modify

- `.github/workflows/test-suite.yml` - Add `wasm-build-check` job

## Acceptance Criteria

- [ ] Both WASM crates are built on every push/PR to main/kyoto
- [ ] Build failures are clearly reported
- [ ] Job runs in parallel with native tests (doesn't slow down feedback)

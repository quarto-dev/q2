# Bumping the pandoc.wasm pin

hub-client's "Download as" runs the official pandoc.wasm. It is pinned to the
native pandoc version `PANDOC_PIN` (`crates/quarto-core/src/pandoc_filters/mod.rs`),
so hub and `q2 render` share one pandoc (design D12). The pin lives in four places
that must agree; tests catch drift in all of them.

| Where | What |
|---|---|
| `PANDOC_PIN` | the version (floor enforced by `version::gate`; the wasm is an exact pin beside it) |
| `resources/pandoc-wasm.json` | `asset_name`, `upstream_zip_sha256`, `wasm_sha256`, `share_root`, mount `limits` |
| `.github/workflows/ts-test-suite.yml`, `test-suite.yml` | `PANDOC_VERSION` |
| `resources/pandoc-filters/README.md` | the recorded pandoc pin |

**Owner:** whoever bumps `PANDOC_PIN` (the author of that PR) owns the wasm bump in
the same PR; the two never move separately.

## Runbook

1. Bump `PANDOC_PIN`, both workflows' `PANDOC_VERSION` and the filters README.
2. Download the asset and measure it (needs `gh`):
   ```bash
   gh release download <ver> -R jgm/pandoc -p 'pandoc-<ver>.wasm.zip'
   shasum -a 256 pandoc-<ver>.wasm.zip                  # -> upstream_zip_sha256
   unzip -o pandoc-<ver>.wasm.zip && shasum -a 256 pandoc-wasm/pandoc.wasm   # -> wasm_sha256
   ```
3. Update `asset_name`, `upstream_zip_sha256` and `wasm_sha256` in
   `resources/pandoc-wasm.json`. Leave `share_root` and `limits` alone unless the
   design changes.
4. `cargo nextest run -p quarto-core pandoc_wasm_constants pandoc_filters` (asset
   name, workflow versions, README pin).
5. Regenerate the native recordings (`cargo xtask capture-pandoc-recordings`) and goldens against the new native pandoc and
   review the diffs (`crates/quarto-core/tests/fixtures/pandoc-recordings/README.md`).
6. Re-run the host runtime gates (H0) with the new wasm; check the wasm still needs
   only the exnref exception-handling proposal.

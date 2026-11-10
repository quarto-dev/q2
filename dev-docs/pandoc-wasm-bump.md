# Bumping the pandoc.wasm pin

hub-client's "Download as" runs the official pandoc.wasm. It is pinned to the
native pandoc version `PANDOC_PIN` (`crates/quarto-core/src/pandoc_filters/mod.rs`),
so hub and `q2 render` share one pandoc (design D12). The pin lives in four places
that must agree; tests catch drift in all of them.

| Where | What |
|---|---|
| `PANDOC_PIN` | the version (floor enforced by `version::gate`; the wasm is an exact pin beside it) |
| `resources/pandoc-wasm.json` | `version`, `asset_name`, `upstream_zip_sha256`, `wasm_sha256`, `share_root`, mount `limits` |
| `.github/workflows/ts-test-suite.yml`, `test-suite.yml`, `q2-preview-spa-e2e.yml` | `PANDOC_VERSION` |
| `resources/pandoc-filters/README.md` | the recorded pandoc pin |
| `crates/xtask/src/dev_setup.rs` | `PANDOC_HYBRID_MIN_VERSION` (the pin's major.minor) |
| `resources/vendored.toml` | the `pandoc-wasm` entry's `version` and `last_reviewed` |

**Owner:** whoever bumps `PANDOC_PIN` (the author of that PR) owns the wasm bump in
the same PR; the two never move separately.

## Runbook

1. Bump `PANDOC_PIN`, every workflow's `PANDOC_VERSION`, the dev-setup floor and the
   filters README (`cargo xtask lint` checks that they agree).
2. Download the asset and measure it (needs `gh`). **Upstream's asset name is not
   stable** — 3.12.1 shipped `pandoc-3.12.1.wasm.zip` with a `pandoc-wasm/` folder,
   3.13 ships `pandoc-3.13-wasm.zip` with a `pandoc-3.13-wasm/` folder — so look it
   up rather than guessing:
   ```bash
   gh release view <ver> -R jgm/pandoc --json assets --jq '.assets[].name' | grep wasm
   gh release download <ver> -R jgm/pandoc -p '<asset>'
   shasum -a 256 <asset>                                          # -> upstream_zip_sha256
   unzip -p <asset> '*/pandoc.wasm' | shasum -a 256               # -> wasm_sha256
   ```
3. Update `version`, `asset_name`, `upstream_zip_sha256` and `wasm_sha256` in
   `resources/pandoc-wasm.json`, and `expected_pandoc_wasm_sha256` in the two
   `crates/quarto-core/schemas/pandoc-request*.golden.json` files. Nothing parses the
   version out of `asset_name`; `version` is the source of truth. Leave `share_root`
   and `limits` alone unless the design changes.
4. `cargo nextest run -p quarto-core pandoc_wasm_constants pandoc_filters` (asset
   name, workflow versions, README pin).
5. Regenerate the native recordings (`cargo xtask capture-pandoc-recordings`) and goldens against the new native pandoc and
   review the diffs (`crates/quarto-core/tests/fixtures/pandoc-recordings/README.md`).
6. Re-run the host runtime gates (H0) with the new wasm; check the wasm still needs
   only the exnref exception-handling proposal.
7. Regenerate the import recordings too (`cargo xtask capture-import-recordings`, same
   pandoc on PATH), then `Q2_UPDATE_IMPORT_EXPECTED=1 cargo nextest run -p quarto-core
   every_fixture_finishes` and review the `expected.qmd` diffs. Reader changes land here
   (3.12: comment markers became `comment-id`, and the ODT reader keeps more formatting).
8. Run `cargo xtask pandoc-check` against the new pandoc and raise
   `PANDOC_ORACLE_MAX_VERSION` in `crates/pampa/tests/integration/test.rs`: CI installs
   exactly `PANDOC_VERSION`, so pampa's 4 oracle tests hard-fail above the ceiling.
9. The docx goldens are Quarto 1's output with *its* bundled pandoc. A writer change in a
   newer pandoc shows up as a `pandoc_goldens` diff; record it in
   `crates/quarto-core/tests/fixtures/pandoc-goldens/DIVERGENCES.md` with native `parity/`
   references (see the pandoc ≥ 3.12 entry there) rather than editing the Q1 snapshots.
10. Rebuild the hub-client wasm (`npm run build:wasm`) before `npm run test:wasm`: the
    expected `wasm_sha256` is compiled in, so a stale build fails every pandoc test with a
    checksum mismatch.

The 3.11 → 3.12.1 bump (bd-7vum2l3y) and the 3.12.1 → 3.13 bump (bd-dti84hkx) are worked examples of all of the above.

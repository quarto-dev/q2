---
title: 'extensions: resolve a contributed format''s reference-doc against the extension'
date: 2026-11-03
description: 'An extension format''s `reference-doc` failed with Q-5-30 because the unmarked value was never resolved through its provenance; resolve it against the declaring `_extension.yml` and register manifests for single-file renders.'
---

**Strand:** bd-5fyii9at
**Issue:** https://github.com/quarto-dev/q2/issues/1010

## Overview

Current behaviour: `contributes.formats.docx.reference-doc: ref.docx` in an
extension fails with `Q-5-30 reference-doc: ref.docx does not exist`.

- The merge-time marking (`FORMAT_PATH_KEYS`) never runs for extension
  layers, so the value stays a plain `Scalar`.
- `build_forwarded_args` (`crates/quarto-core/src/pandoc_filters/format_defaults.rs`)
  treated every unmarked `reference-doc`/`template` as missing.
- In a single-file render (no `_quarto.yml`),
  `ProjectConfig::extension_manifest_paths` was empty, so the metadata merge
  never registered the manifest in the source context and the value's
  provenance could not be recovered (`crates/quarto-core/src/project/mod.rs`).

New behaviour: an unmarked value is resolved with
`config_paths::resolve_config_file_with`, which uses the value's
`SourceInfo` to find the declaring file (the extension dir for a manifest
value). It is forwarded when that file exists; otherwise Q-5-30 as before.
Single-file renders now record extension manifest paths too.

## Work items

- [x] Unit test: extension-provenance `reference-doc` resolves; missing still Q-5-30
- [x] `build_forwarded_args`: provenance resolution + existence probe for unmarked values
- [x] Thread an `is_file` probe from `PandocWriteStage` (`ctx.runtime`)
- [x] Single-file `ProjectConfig` records `extension_manifest_paths`; regression test
- [x] End-to-end repro from the issue renders, with the bundled styles in the output
- [x] `cargo nextest run -p quarto-core`, clippy, `cargo xtask lint`

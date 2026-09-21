# Brand YAML test fixtures

These fixtures were copied once on 2026-05-20 from the Quarto 1 source
tree (`external-sources/quarto-cli/`) and are now the authoritative
reference for this crate's tests.

Provenance:

- `brand-yaml/{kitchen-sink,monospace-colors,palette-colors}/` ←
  `tests/docs/brand-yaml/`
- `use-brand/{basic-brand,multi-file-brand,nested-brand}/` ←
  `tests/smoke/use-brand/`

Per Q2's External Sources Policy (`AGENTS.md`), the original location
is reading-only reference material; the build and test process must
not read from `external-sources/`. If a fixture needs to be refreshed
to track a Q1 brand-schema change, re-copy the relevant files in a
single commit and document the change here.

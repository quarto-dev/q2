# bd-duplicate-heading-ids-mou5z7ux investigation fixtures

Minimal repro for the include-boundary heading-id collision, mirroring
the external repro at ~/repos/github/cscheid/q2-connect-docs/llms-info/repros/duplicate-heading-ids/.

- `repro/index.qmd` — includes `_shared.qmd` three times; at main @ 4eaede00,
  `q2 render` emits `id="create-the-integration"` three times (expected: base, -1, -2).
- `repro/control-inline.qmd` — same heading repeated inline; correctly emits base, -1, -2.

Run: `cargo run --bin q2 -- render claude-notes/plans/duplicate-heading-ids-includes-investigation/repro/index.qmd`
then grep the emitted HTML for `id="create-the-integration`.

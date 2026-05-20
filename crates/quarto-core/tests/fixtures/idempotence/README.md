# Plan 3 — idempotence fixtures

Holds the per-fixture project directories the q2-preview idempotence
gate at `crates/quarto-core/tests/idempotence.rs` drives through the
pipeline twice and hashes for equality.

For the contract a transform / filter / stage author must meet to
land here without breaking the gate, read
`claude-notes/instructions/idempotence-contract.md`. The full plan
that introduced the gate lives at
`claude-notes/plans/2026-05-04-q2-preview-plan-3-builtin-filter-idempotence.md`.
The rules below are the ones that bite at fixture-authoring time.

## Fixture-format rules

1. **No executable engine cells.** Use only fenced code blocks
   (`` ```python ``, `` ```r ``, etc.) — these are AST nodes, not
   executed. Do NOT use `{python}` / `{r}` / `{julia}` style cells; CI
   has no kernels, the `engine-execution` stage either fails or falls
   through to the markdown passthrough, and the resulting two runs
   are not reliably comparable.

2. **No absolute process paths in fixture content.** Use only paths
   that resolve relative to the fixture root (`./local.png`, not
   `/private/var/.../local.png`). Resource-collector, include-resolve,
   built-in-extension lookup, and similar transforms record paths into
   meta; the built-in extensions resource bundle extracts to a
   process-specific `temp_dir()`. Stable within a process — fine for
   Plan 3's two-runs-compare contract today, but a latent issue for
   any future stored-snapshot variant.

3. **Per-fixture mode mapping.** Document-only fixtures (plain text,
   callouts, theorems, code blocks, …) run in both `SingleFile` and
   `ProjectOrchestrator` modes. Website-chrome fixtures (navbar,
   sidebar, listings, page-nav, footer) are **orchestrator-only**
   because the chrome transforms require a populated `ProjectIndex`;
   driving them through `SingleFile` mode would test a partial pipeline
   that doesn't exist in production.

## What lives here

Subdirectories named for each non-trivial fixture (typically the
website / multi-file cases that need a `_quarto.yml` plus several
sibling pages). Trivial single-page fixtures live as in-source
literals in `idempotence.rs` — the fixture's `setup` closure writes
them into a `TempDir` at run time.

Pattern matches `tests/fixtures/websites/hub-smoke/` and
`tests/fixtures/phase5-website-baseline/`; use `copy_fixture(...)` from
`render_page_in_project.rs:616` as the lift point if a fixture grows
big enough to want a pre-built directory tree.

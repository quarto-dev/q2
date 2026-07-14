# Lua API divergence registry

Deliberate, permanent divergences between Quarto 2's Pandoc Lua API and
real Pandoc's. Every entry here is a *decision*, not a gap: it has an
owner, a rationale, a Q-code, and permanent `# DIVERGENCE` entries in
the conformance `xfail.txt` files. Anything not listed here that fails
conformance is a bug or unfinished parity work.

This registry is consumed by both conformance ratchets (Track 1
`xfail.txt` and `differential/xfail.txt`); the error contract and the
ratchet's formal `# DIVERGENCE` handling are being designed on strand
bd-9p2686pc — that work should extend this file, not replace it.

Format per entry: what diverges, why, the Q-code the user sees, and
which conformance cases are permanently xfailed.

---

## SimpleTable (Q-11-2)

- **Decision**: epic-plan Decision 6 (Carlos, 2026-07-13), strand
  bd-d4wd6r3i. Epic plan:
  `claude-notes/plans/2026-07-13-lua-api-pandoc-parity.md`.
- **What**: q2 does not implement the legacy pre-pandoc-2.10
  simple-table representation. `pandoc.SimpleTable`,
  `pandoc.utils.to_simple_table`, and `pandoc.utils.from_simple_table`
  all exist but raise the Q-11-2 error pointing users at
  `pandoc.Table`.
- **Why**: SimpleTable is a backward-compatibility shim Pandoc keeps
  for pre-2.10 filters; q2 has no legacy filter corpus to serve, and
  supporting the shim would mean maintaining a second, lossy table
  representation alongside the real one.
- **User-visible error**: `Q-11-2: <entry point> is not supported:
  Quarto does not implement the legacy pre-pandoc-2.10 SimpleTable
  API. Construct a pandoc.Table instead (see
  https://quarto.org/docs/errors/lua/Q-11-2).` Catalog entry:
  `crates/quarto-error-catalog/error_catalog.json`.
- **Permanent xfails** (`xfail.txt`, marked `# DIVERGENCE`):
  - `test-simpletable.lua::SimpleTable::can access properties`
  - `test-simpletable.lua::SimpleTable::can modify properties`
- **Code**: `crates/pampa/src/lua/constructors.rs`
  (`simpletable_divergence_error`), `crates/pampa/src/lua/utils.rs`.
- **Tests**: `crates/pampa/tests/integration/test_lua_constructors.rs`
  (`test_simpletable_*`, `test_utils_*_simple_table_*`).

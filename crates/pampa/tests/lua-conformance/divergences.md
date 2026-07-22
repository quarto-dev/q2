# Lua API divergence registry

Deliberate, permanent divergences between Quarto 2's Pandoc Lua API and
real Pandoc's. Every entry here is a *decision*, not a gap: it has an
owner, a rationale, a Q-code, and permanent `# DIVERGENCE` entries in
the conformance `xfail.txt` files. Anything not listed here that fails
conformance is a bug or unfinished parity work.

This registry is consumed by both conformance ratchets (Track 1
`xfail.txt` and `differential/xfail.txt`): an xfail entry whose
trailing comment starts with `DIVERGENCE` must have a record here
(enforced by `divergence_xfails_are_registered` in
lua_conformance.rs), and if such an entry ever *passes*, the ratchet
reports it as a stale registry entry rather than ordinary progress.
The marshaling error contract these entries reference is Q-11-2..5
(bd-9p2686pc): granular, actionable codes — Q-11-3 invalid argument
(`"<expected> expected, got <type>"`), Q-11-4 invalid filter return,
Q-11-5 invalid property assignment.

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

## Meta numbers are first-class (D-num)

- **Decision**: Meta↔ConfigValue design review (Carlos, 2026-07-20),
  strands bd-2llqjsms / bd-a9g50za2. Plan:
  `claude-notes/plans/2026-07-20-lua-meta-pandoc-filters.md`.
- **What**: metadata numbers are never stringified. Pandoc's
  `peekMetaValue` renders Lua numbers to `MetaString` ("5", "13.37");
  q2 keeps them as `Scalar(Integer)`/`Scalar(Real)`, and they read back
  from `meta` as Lua numbers. This applies uniformly — including the
  `pandoc.MetaList`/`pandoc.MetaString` coercion constructors — one
  rule, no per-constructor special cases.
- **Why**: q2 config genuinely has typed numbers (YAML integers/reals
  survive into `ConfigValueKind::Scalar`), and downstream consumers
  (schema validation, theming, listing config) rely on them. Emulating
  pandoc's stringification would destroy type information q2's own
  pipeline round-trips.
- **User-visible behavior**: `meta.count` is `3`, not `'3'`;
  `tostring(meta.count)` still renders "3" where filters format text.
- **Permanent xfails** (`xfail.txt`, marked `# DIVERGENCE`):
  - `test-metavalue.lua::MetaValue elements::Numbers are treated as strings`
- **Code**: `crates/pampa/src/lua/config_value.rs`
  (`push_yaml_scalar`, `build_config_value`).

## Meta null reads as nil (D-null)

- **Decision**: same review as D-num (Carlos, 2026-07-20).
- **What**: an explicit YAML `null` metadata value
  (`ConfigValueKind::Scalar(Yaml::Null)`) is pushed to Lua as `nil`, so
  a null-valued key is indistinguishable from an absent key inside a
  filter. Reconciliation treats "null in the original, key absent in
  the returned table" as *unchanged*, so passthrough filters do not
  silently delete null keys. Writing an explicit null is
  `quarto.config.null()`.
- **Why**: any truthy sentinel would break `if meta.draft` on
  `draft: ~`; Lua's own idiom for "no value" is `nil`. (Pandoc has no
  null MetaValue at all, so there is no pandoc behavior to match.)
- **Permanent xfails**: none (no upstream conformance case exercises
  null metadata).
- **Code**: `crates/pampa/src/lua/config_value.rs` (`push_yaml_scalar`
  null arm, `build_map` null-preservation rule, `LuaConfigNull`).

## Meta map iteration order is not guaranteed (D-order)

- **Decision**: same review as D-num (Carlos, 2026-07-20).
- **What**: `pairs(meta)` iteration order over metadata maps is
  unspecified (plain Lua tables), matching pandoc's own MetaMap
  marshaling. On the return path, reconciliation preserves the
  original entry order for kept keys and appends *new* keys in sorted
  order — Lua hash iteration order is not deterministic across runs,
  and q2 requires deterministic output.
- **Why**: preserving observable insertion order would require a proxy
  table with `__pairs`, breaking pandoc's plain-table programming
  model for meta.
- **Permanent xfails**: none.
- **Code**: `crates/pampa/src/lua/config_value.rs` (`build_map`).

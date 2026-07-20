# Lua `function Meta` / `function Pandoc` filters + Meta↔ConfigValue design

**Strands:** bd-2llqjsms (constructors + design), bd-a9g50za2 (doc-level
invocation). Parent epic: bd-grkrb9nj. Supersedes bd-uy3z (older duplicate,
close when this lands).

**Status:** design proposal — iterating with Carlos before implementation.

## Overview

Q2's `Pandoc.meta` is a `ConfigValue` (source-tracked, merge-aware,
YAML-tag-driven), not Pandoc's `Meta = Map Text MetaValue`. We need:

1. Doc-level filter handlers `function Meta(meta)` / `function Pandoc(doc)`
   (and legacy alias `Doc`) actually invoked — today they're collected and a
   Q-11-6 "unimplemented" warning fires (`filter.rs:414-443`).
2. `pandoc.Pandoc(blocks, meta)`, `pandoc.Meta`, and the six `pandoc.Meta*`
   constructors — 15 `test-pandoc.lua` + 2 `test-metavalue.lua` xfails are the
   empirical spec.
3. A principled ConfigValue ↔ Lua mapping that neither lies to users nor
   destroys q2's provenance (source_info) and merge metadata.

## Key insight: modern Pandoc has no MetaValue objects in Lua

Since pandoc 2.17, `pushMetaValue` marshals **natively**
(`pandoc-lua-marshal/.../MetaValue.hs:30-37`):

| MetaValue      | Lua value                          |
|----------------|------------------------------------|
| MetaBool       | boolean                            |
| MetaString     | string                             |
| MetaInlines    | `Inlines` userdata list            |
| MetaBlocks     | `Blocks` userdata list             |
| MetaList       | `List` table                       |
| MetaMap        | plain table                        |

The `pandoc.Meta*` constructors are *coercion functions* returning these
native shapes (`MetaBool` is literally the identity). So q2 does **not** need
a MetaValue userdata hierarchy — it needs a ConfigValue → native-Lua push, a
native-Lua → ConfigValue peek, and coercion functions. This dissolves most of
the "F2 structural divergence" fear.

## Proposed design

### D1. ConfigValue → Lua (push)

| ConfigValueKind        | Lua value                                        |
|------------------------|--------------------------------------------------|
| Scalar(String)         | string                                           |
| Scalar(Boolean)        | boolean                                          |
| Scalar(Integer)        | integer *(divergence D-num, see below)*          |
| Scalar(Real)           | number *(divergence D-num)*                      |
| Scalar(Null)           | nil — key reads as absent *(see D5)*             |
| PandocInlines          | `Inlines` userdata list (existing `create_inlines_table`) |
| PandocBlocks           | `Blocks` userdata list                           |
| Array                  | `List` table of converted items                  |
| Map                    | plain table, string keys (iteration order not guaranteed, as in pandoc) |
| Path(s)/Glob(s)/Expr(s)| tagged value (userdata or `__name`-tagged table) with `__tostring` = the raw string; round-trips losslessly; recognizable via `quarto.utils.type` |

`source_info` and `merge_op` are not representable in the pushed value — the
return path recovers them via reconciliation (D3).

This same push replaces the shapes `config_value_to_lua`
(`readwrite.rs:200-240`) currently produces for `pandoc.read(...).meta`,
which today wraps PandocInlines in a **legacy tagged table**
(`{t="MetaInlines", content=...}`) — itself a parity bug vs modern pandoc.
One marshaling to rule both paths.

### D2. Lua → ConfigValue (peek)

Mirror of D1, with pandoc's `peekMetaValue` guessing rules for plain tables:

- boolean → Scalar(Boolean); string → Scalar(String) — **not** markdown-parsed
  (matches pandoc MetaString semantics; use `quarto.config.md` when parsing is
  wanted).
- number → Scalar(Integer/Real) *(divergence D-num: pandoc stringifies)*.
- `Inlines`/`Blocks` userdata (or single Inline/Block userdata → singleton) →
  PandocInlines/PandocBlocks.
- Tagged Path/Glob/Expr values → the corresponding variant, unchanged.
- Table with `__name` metafield "Inlines"/"Blocks"/"List" → as named;
  otherwise `rawlen == 0` → Map, else try Inlines → Blocks → Array
  (pandoc's guessing order).
- `quarto.config.*`-constructed values → their exact variant (D4).

### D3. Provenance: round-trip with reconciliation

`function Meta(meta)` receives the plain converted structure (pandoc's
programming model: materialize, mutate freely, return). On return, we
**reconcile** against the original ConfigValue tree instead of blindly
rebuilding:

- Recursive descent matching map keys / array indices.
- Where the returned Lua value is structurally equal to the original node's
  projection → **keep the original ConfigValue node** (source_info, merge_op,
  key_source all intact). The source-insensitive structural-equality
  machinery (`types.rs:2046`) already exists for Inlines/Blocks.
- Where it differs → build a new node; `source_info` attributed to the filter
  via the existing `filter_source_info` Lua-stack walk (`types.rs:1813`), so
  diagnostics can point at `my-filter.lua:12` instead of "unknown".
  `merge_op` = default (merging already happened by filter time; the field is
  spent).
- `nil` return from the handler = document unchanged (pandoc semantics).

Rejected alternative — **userdata proxy over ConfigValue** (lazy
`__index`/`__newindex` write-through): perfect provenance but breaks the
pandoc mutation model (`meta.albums:insert(x)` mutates a fetched copy unless
we proxy all the way down with identity caching). Too much machinery for
small tables; reconciliation gets the same provenance result for untouched
keys with a fraction of the complexity.

### D4. Namespaces: `pandoc.*` = compat surface, `quarto.config.*` = native surface

- **Keep `pandoc.*` for the Pandoc-compatibility API** (`Pandoc`, `Meta`,
  `MetaString`, `MetaBool`, `MetaInlines`, `MetaBlocks`, `MetaList`,
  `MetaMap`). The name is truthful there: it *is* the pandoc-compatible
  surface, semantics pinned by the conformance suite, deliberate deviations
  in the divergence registry.
- **New `quarto.config` table** for q2-native ConfigValue constructors,
  mirroring the YAML tag system users already know:

  | Lua                        | YAML analog | ConfigValueKind          |
  |----------------------------|-------------|--------------------------|
  | `quarto.config.str(s)`     | `!str`      | Scalar(String)           |
  | `quarto.config.md(s)`      | `!md`       | PandocInlines/Blocks (parsed) |
  | `quarto.config.path(s)`    | `!path`     | Path                     |
  | `quarto.config.glob(s)`    | `!glob`     | Glob                     |
  | `quarto.config.expr(s)`    | `!expr`     | Expr                     |
  | `quarto.config.null()`     | `~`         | Scalar(Null)             |

  (`quarto.config.md` reuses `parse_yaml_string_as_markdown_to_config`
  semantics so Lua and YAML agree about what markdown means.)
- **No `pampa.` global.** pampa is an internal crate name; users' identity
  for this system is Quarto, Q1 filters already use `quarto.*`, and q2
  already ships `quarto.{warn,error,log,utils,doc,...}`. If pampa is ever
  externalized as a standalone tool, alias `pampa = quarto`-subset then.

### D5. Scalars Pandoc can't represent (registered divergences)

- **D-num — numbers are first-class, never stringified.** `meta.count`
  reads as `5`, not `'5'`; Lua numbers round-trip to Scalar(Integer/Real).
  One uniform rule, no per-constructor special cases; the two
  `test-metavalue.lua` number tests become permanent `DIVERGENCE`-annotated
  xfails (SimpleTable precedent). Rationale: q2 config genuinely has numbers
  and downstream consumers (schema validation, theming) want them typed.
- **D-null — `null` reads as `nil`.** A null-valued key is indistinguishable
  from an absent key inside a filter (Lua semantics make any truthy sentinel
  worse: `if meta.draft` on `draft: ~` must not be truthy). Reconciliation
  treats "Null in original, absent in returned table" as *unchanged* so
  passthrough filters don't silently delete null keys; writing an explicit
  null is `quarto.config.null()`.
- **D-order — Map iteration order in `pairs()` is not guaranteed** (same as
  pandoc MetaMap). Reconciliation preserves original entry order for kept
  keys; new keys append.

All three go in the divergence registry (catalog class H3) with pinning
tests.

### D5b. `!prefer`/`!concat` need no Lua surface at all

The only semantic reader of `merge_op` is the merge algorithm
(`quarto-config/src/merged.rs:302`), which runs in `MetadataMergeStage`
(pipeline stage 2). `UserFiltersStage::pre()/post()` run at stages ~12/14 —
**nothing downstream of the filter passes ever merges again**, so by filter
time `merge_op` is spent. The two post-filter touchers are non-semantic:
the JSON writer serializes it (`pampa/src/writers/json.rs:3921`) and
`quarto-ast-reconcile` hashes its discriminant (`hash.rs:558`) for preview
rebuild stability. Both are satisfied by reconciliation keeping original
nodes for untouched keys; filter-created nodes default to `Concat`
harmlessly. Consequence: no `quarto.config.prefer()`/`concat()`
constructors, no merge_op field in the Lua representation.

### D6. Handler invocation

Insertion point: `apply_lua_filter` between the block walk and the doc
rebuild (`filter.rs:260-273`).

- **Typewise:** element functions during walk → `Meta` → `Pandoc` (pandoc's
  documented order). `Doc` accepted as deprecated alias for `Pandoc`
  (already in `filter_names`; add `Meta`, which is missing from the list at
  `filter.rs:346-398`).
- **Topdown:** `Pandoc` runs first and can truncate via second return value
  `false`; exact `Meta` position pinned empirically against the system-pandoc
  oracle via the differential harness before implementation.
- `doc:walk{...}` shares the same invocation machinery (test-pandoc.lua
  "uses `Meta` function" exercises walk, not just the filter pass).
- When all three handler kinds are invoked: remove
  `unimplemented_doc_handler_warnings`, retire Q-11-6 from
  quarto-error-catalog, flip `test_doc_level_handler_emits_unimplemented_warning`
  into an invocation test (per bd-2llqjsms comment).

### D7. The `Pandoc` document value

Extend the existing plain-table-with-`__name="Pandoc"` representation
(`rust_pandoc_to_lua_table`, `readwrite.rs:316-342`) with a shared metatable
providing `walk`, `clone`, `normalize`, `__concat` (meta union right-biased),
`__eq`. Constructor `pandoc.Pandoc(blocks, meta)` applies fuzzy blocks
coercion + D2 meta normalization. Keeps `pandoc.read`'s doc shape and the
constructor's shape identical. (Userdata upgrade is a possible later
refactor; not needed by the conformance suite.)

`doc:normalize()` (whitespace + table normalization) is real but separable
work — proposed as a follow-up strand rather than blocking this one; its 2
xfails stay until then.

## Design decisions (reviewed with Carlos, 2026-07-20)

1. **Native namespace is `quarto.config`.** ✅
2. **D-num uniform rule confirmed** — numbers are never stringified anywhere;
   the 2 metavalue conformance tests become permanent `DIVERGENCE`-annotated
   xfails. ✅
3. **Path/Glob/Expr are userdata** with a mutable `.value` property and
   `__tostring`. Rationale: exact/unforgeable round-trip, consistent with
   LuaInline/LuaBlock, and mlua field getters/setters keep `.value`
   ergonomics; a tagged table would be forgeable and force the peek to
   define malformed-value semantics. ✅ (proposed; confirm during Phase 1
   review)
4. **`doc:normalize()` deferred** to a follow-up strand; its 2 xfails stay.
   (normalize = Builder-style inline whitespace coalescing + padding table
   rows to the ColSpec count — self-contained, needs table-normalization
   helpers q2 doesn't have yet.) ✅

## Work items

### Phase 0 — design sign-off
- [x] Iterate this document with Carlos; record decisions on the open
      questions above. (2026-07-20)
- [x] Mark bd-uy3z superseded (closed, `supersedes` link); bd-2llqjsms +
      bd-a9g50za2 in_progress with plan reference.

### Phase 1 — marshaling core (TDD: unit tests first)
- [x] Tests: ConfigValue → Lua push for every ConfigValueKind (incl.
      Path/Glob/Expr tags, null, numbers).
- [x] Tests: Lua → ConfigValue peek incl. pandoc guessing rules and
      `quarto.config.*` values.
- [x] Tests: reconciliation — untouched keys keep source_info/merge_op
      byte-for-byte; changed/new keys get filter-attributed SourceInfo;
      null-preservation rule.
- [x] Implement marshaling core: new module `crates/pampa/src/lua/config_value.rs`
      with `push_config_value` / `push_meta` (attaches shared `Meta`-named
      metatable) / `peek_config_value` (reconciling) /
      `config_value_structurally_eq`, plus `LuaConfigSpecial` (Path/Glob/Expr
      userdata with mutable `.value`) and `LuaConfigNull`. `pandoc.utils.type`
      reports Path/Glob/Expr/Null/Meta.
- [x] Switch `pandoc.read`/`write` meta to the native shape (readwrite.rs
      rewired; legacy `meta_value_to_lua`/`lua_to_meta_value`/
      `meta_to_lua_table`/`lua_table_to_meta` deleted from types.rs — no
      remaining callers; obsolete readwrite unit tests removed, new shape
      tests added).
- [x] `quarto.config.*` constructors (`str`/`md`/`path`/`glob`/`expr`/`null`),
      registered from `register_quarto_namespace` so every Lua environment
      gets them.

Phase 1 notes:
- New-key emission order in reconciled maps is **sorted** (Lua hash-iteration
  order is nondeterministic across runs); original keys keep original order.
- Untagged-table guessing is strict-then-array like pandoc: all-Inline-ud →
  PandocInlines, all-Block-ud → PandocBlocks, else Array — so `{'a','b'}` is
  an Array of strings, never word-split inlines.
- `Yaml::Real` that fails to parse as f64 pushes as its raw string (the old
  readwrite converter silently produced 0.0).
- Verified: full pampa suite green (4217 tests) after the readwrite shape
  change.

### Phase 2 — Meta handler invocation
- [x] Pin pandoc's Meta invocation semantics with pampa integration tests
      (filter_tests.rs): typewise walk→Meta order, topdown Meta→walk order,
      nil return keeps meta byte-for-byte, passthrough preserves provenance,
      mutation reconciles (new keys filter-attributed via
      `By::filter(path, 0)`), Inlines values, invalid return → Q-11-4,
      pushed meta is "Meta"-typed with native values.
      **Plan change — no differential (oracle) cases in this phase:** any
      frontmatter-bearing doc diverges from pandoc in the meta *wire shape*
      (q2 ConfigValue JSON vs pandoc MetaValue JSON) regardless of filter
      behavior, and with only `Meta` implemented a filter cannot move meta
      observations into the comparable block stream. Differential cases land
      in Phase 4 with the `Pandoc` handler, using a clear-meta + body-marker
      pattern. (Doc-level order semantics were instead pinned directly from
      pandoc-lua-marshal `Pandoc.hs applyFully`: typewise = walk→Meta→Pandoc,
      topdown = Pandoc→Meta→walk; nil-return semantics from
      `Walk.hs applyStraightFunction`.)
- [x] Add `Meta` to `filter_names`; `apply_meta_function` invoked in
      pandoc's order for both traversals; Q-11-6 narrowed to Pandoc/Doc
      (Meta no longer warns; tests updated).
- [x] E2E: verified through the real binary —
      `cargo run --bin q2 -- render .../doc.qmd` with a `function Meta`
      filter produced `<title>Title From Meta</title>` and a
      handler-created subtitle in the HTML (output inspected). Permanent
      regression: smoke-all fixture `filters/meta-handler.qmd` +
      `set-meta.lua` (asserts title replacement, new-key creation, and
      typewise ordering via a `seen-2-strs` marker).

Phase 2 notes:
- Reconciliation refinement: an edited container that keeps its kind
  (Map→Map, Array→Array) keeps the original container node's
  source_info/merge_op — only changed children get filter attribution.
  Top-level meta map therefore keeps its YAML span across filter edits.
- `peek_meta` forces map interpretation of the handler return (pandoc's
  peekMeta renders integer keys to strings; never list-guesses).
- bd-o8pr additivity E2E (noted in bd-uy3z) is now unblocked but not
  written here; it belongs to that strand's scope.

### Phase 3 — pandoc.Pandoc/Meta/Meta* constructors + doc value
- [x] Flip xfails (ratchet-driven TDD: removed the lines first, watched the
      12 unexpected failures, then implemented). Conformance now 196 pass /
      7 xfail: normalize ×2 (follow-up strand), walk order ×2 (Phase 4:
      meta-value traversal + Pandoc leg), D-num numbers ×1 (permanent
      DIVERGENCE), SimpleTable ×2 (pre-existing DIVERGENCE).
- [x] `pandoc.Pandoc(blocks, meta?)`, `pandoc.Meta`, and the six `Meta*`
      coercion constructors, in new module
      `crates/pampa/src/lua/pandoc_doc.rs`. MetaBool is strictly typed
      (mlua's bool coercion would have accepted any truthy value —
      caught by a red test first). MetaString renders numbers (explicit
      stringification; D-num is about implicit values only).
- [x] Doc value: shared registry metatable ("Pandoc") with `walk` (element
      legs + Meta leg in applyFully order; Pandoc leg is Phase 4), deep
      `clone`, `__concat` (right-biased meta union per pandoc-types
      Semigroup), `__eq` (source-free structural equality). `pandoc.read`
      docs now carry the same metatable, so read/constructed docs are
      interchangeable (`pandoc.read(...) == pandoc.Pandoc(...)` works);
      readwrite.rs delegates to pandoc_doc.rs.
- [x] Error-path integration tests (Q-11-3 on bad meta args, strict
      MetaBool, MetaList/MetaMap normalization, cross-path doc equality).

### Phase 4 — Pandoc handler + full-doc walk parity + cleanup
- [ ] **Element walk must traverse meta values** (discovered from upstream
      test-pandoc.lua walk tests): pandoc's `walkBlocksAndInlines` visits
      MetaInlines/MetaBlocks payloads inside meta — meta-first (Pandoc's
      field order), inline pass then block pass in typewise; root-down in
      topdown. q2's walk currently touches only `pandoc.blocks`, so element
      filters never see meta content (e.g. Str-uppercase doesn't uppercase
      the title in q2 but does in pandoc). Implement with reconciliation
      (only changed PandocInlines/Blocks payloads get new nodes).
- [ ] Invoke Pandoc/Doc handler (typewise last; topdown first with
      truncation), shared with `doc:walk` (pandoc's `walk` method IS
      `applyFully`, so the doc walk needs the full order incl. meta
      traversal — implement as one shared helper).
- [ ] Remove Q-11-6 warning path + retire catalog entry + flip its test.
- [ ] Divergence registry entries (D-num, D-null, D-order) with pinning tests.
- [ ] WASM smoke test addition (`tests/wasm_lua.rs`); verify WASM leg builds
      (`cargo xtask verify`).
- [ ] Docs: user-facing Lua filter page notes Meta/Pandoc support +
      `quarto.config.*`.
- [ ] File follow-up strand for `doc:normalize()`; close bd-2llqjsms,
      bd-a9g50za2, bd-uy3z.

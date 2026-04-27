# Lua attribute-mutation proxy (bd-195t)

## Problem

Quarto 2's Lua bridge returns **fresh copies** of element `attr` and its
`.attributes` / `.classes` fields on every read. As a consequence, the
idiomatic Pandoc-Lua pattern

```lua
function CodeBlock(cb)
  cb.attr.attributes["data-hl-spans"] = my_encoding
  return cb
end
```

silently does nothing: the write lands on an ephemeral Lua table that is
discarded the moment the filter returns. The AST that comes out of the
filter has no `data-hl-spans` set. No error, no warning.

This was uncovered while building the Phase 3.5 filter-authored-spans
fixture (`crates/quarto/tests/smoke-all/highlighting/04-filter/`), where
we worked around it by rebuilding the whole `Attr` and reassigning:

```lua
local attrs = cb.attr.attributes
attrs["data-hl-spans"] = pandoc.json.encode(spans)
cb.attr = pandoc.Attr(cb.attr.identifier, cb.attr.classes, attrs)
return cb
```

That workaround is acceptable for an internal test fixture but is a
usability regression compared to Pandoc's Lua API — and we don't want
it to be the shape of the Lua filter examples we ship with the syntax
highlighting docs. Before we encourage filter-authored highlighting as
a user-facing feature, idiomatic attribute mutation must persist.

The follow-up pointer is in
`claude-notes/plans/2026-04-20-syntax-highlighting-phase-3.5.md`
("Follow-up task: Lua attribute-mutation proxy").

## Root cause (code references)

- `crates/pampa/src/lua/types.rs:1733` —
  `attr_to_lua_userdata` creates `LuaAttr::new(attr.clone())`. The
  `LuaAttr` is disconnected from the parent block/inline.
- `crates/pampa/src/lua/types.rs:1591-1596` — `LuaAttr::get_field`
  returns a fresh Lua table populated from the cloned attributes map
  on each access to `.attributes`. No write-back.
- Similarly `crates/pampa/src/lua/types.rs:1570-1583` — positional and
  `.classes` accessors return fresh tables.
- `crates/pampa/src/lua/types.rs:662` (block) and `:120` (inline) —
  the `cb.attr` / `code.attr` read returns a fresh `LuaAttr` userdata
  (via `attr_to_lua_table` → `attr_to_lua_userdata`).

The chain `cb.attr.attributes["k"] = v` thus produces three
disconnected values, none of which route back to the original block.

## Design options

### A. Shared interior mutability — `Rc<RefCell<...>>` (preferred)

Store the AST nodes behind `Rc<RefCell<...>>` inside the Lua userdata
wrappers:

```rust
pub struct LuaBlock(pub Rc<RefCell<Block>>);
pub struct LuaInline(pub Rc<RefCell<Inline>>);
```

Then accessing `cb.attr` returns a *proxy* userdata that shares the
same `Rc` and knows how to reach the `Attr` inside. Accessing
`.attributes` returns a *proxy* userdata that shares the same `Rc` and
routes writes back into `attr.2`. Likewise for `.classes`.

We add a new userdata type (`LuaAttrView` / `LuaAttrProxy`, name TBD)
that carries:

```rust
enum LuaAttr {
    /// Standalone Attr (e.g. built via `pandoc.Attr(...)`). Mutations
    /// stay local until explicitly assigned back to an element.
    Owned(RefCell<crate::pandoc::Attr>),
    /// Proxy into a block's attr. Mutations are visible on the block.
    BlockRef(Rc<RefCell<Block>>),
    /// Proxy into an inline's attr.
    InlineRef(Rc<RefCell<Inline>>),
}
```

Similarly `LuaAttributesProxy` and `LuaClassesProxy` carry the shared
`Rc` and whether to look in `Block` or `Inline`.

**Pros**

- Matches Pandoc's API. `cb.attr.attributes["k"] = v`,
  `cb.attr.classes[#cb.attr.classes+1] = "warn"`,
  `cb.attributes["k"] = v` (the shortcut) — all persist.
- Aliases within a filter (`local a = cb.attr` then mutating `a`)
  behave as users expect.
- No behavioural surprises at the walker boundary: the walker still
  clones out an owned `Block`/`Inline` on `FromLua`, so mutations are
  scoped to the filter invocation — the same contract we have today.

**Cons**

- Touches `LuaBlock`/`LuaInline` internals. ~60 `LuaBlock(...)` and
  ~86 `LuaInline(...)` constructor sites in `crates/pampa/src/`. Most
  are in `types.rs` itself and the pattern is mechanical
  (`LuaBlock(b)` → `LuaBlock(Rc::new(RefCell::new(b)))`). The
  `FromLua` impls already clone — switching them to clone the inner
  value out of the cell preserves current semantics.
- New code is required to implement the three proxy userdata types
  with the expected `__index`/`__newindex`/`__pairs`/`__len`/`__ipairs`
  metamethods.

### B. Convenience methods only (`cb:set_attribute(k, v)`)

Add `cb:set_attribute("k", "v")`, `cb:set_class(i, name)`, etc. Keep
the current read-returns-copy semantics and document them.

**Pros**: small, surgical, no userdata plumbing.

**Cons**: diverges from Pandoc's API. Anyone copying an `elem.attributes["loading"] = "lazy"` snippet from Pandoc docs into a
Quarto filter will hit the same silent-drop bug we're trying to fix.
Helpful as a *complement* to A, but not a substitute.

### C. Commit-on-return or finalizer-based writeback

Rejected. Relying on Lua GC for correctness is fragile; the write
timing would be non-obvious.

**Decision: option A.** B's helper methods can be added on top for the
"I know exactly what I want to set, give me the short form" case, but
the core pattern must work because that's what users will copy from
Pandoc's docs.

## Plan (TDD)

### Phase 1 — Failing test

Before any implementation, write a Lua filter test that exercises the
idiomatic pattern and confirm it fails in the expected way.

- [x] **1.1** `crates/pampa/tests/test_lua_attr_mutation.rs ::
  test_cb_attr_attributes_nested_write_persists` — asserts that
  `cb.attr.attributes["data-hl-spans"] = v` persists onto `CodeBlock.attr.2`.
- [x] **1.2** `test_cb_attributes_shortcut_write_persists` — asserts
  that the block-level `cb.attributes[k] = v` shortcut persists.
- [x] **1.3** `test_cb_attr_classes_append_persists` — asserts
  `cb.attr.classes[#cb.attr.classes+1] = "warn"` persists.
- [x] **1.4** `test_inline_code_attr_attributes_write_persists` —
  asserts inline `code.attr.attributes[k] = v` persists.
- [x] **1.5** `test_pandoc_attr_owned_semantics` — exercises the
  Owned variant end-to-end: mutate a standalone `pandoc.Attr(...)`
  with `a.attributes[k] = v`, then assign to `cb.attr`, then mutate
  through `cb.attr.*` after assignment. All three mutations must
  land on the block.
- [x] **1.6** Ran all 5 tests pre-refactor. All 5 fail as expected:
  - 1.1 / 1.4 / 1.5: silent drop — `attrs = {}` in the output.
  - 1.2: `cb.attributes` is not a field at the block level today
    (Phase 5 adds the shortcut). Error: *attempt to index a nil
    value (field 'attributes')*.
  - 1.3: silent drop — classes list unchanged.
  Reality check: test 1.5 intentionally failed pre-refactor because
  the standalone-Attr write (`a.attributes[...] = ...`) also hits the
  same ephemeral-table bug today. Post-refactor it will pass because
  the Owned variant will also have a proxy write-path.

### Phase 2 — Refactor `LuaBlock` / `LuaInline` to shared-cell storage

- [x] **2.1** `LuaInline(pub Rc<RefCell<Inline>>)` and
  `LuaBlock(pub Rc<RefCell<Block>>)` with `::new(..)` constructors
  and `borrow_inline/borrow_block`, `clone_inline/clone_block`
  accessors.
- [x] **2.2** All constructor call sites migrated to `::new(..)` via
  targeted replace_all across `types.rs`, `constructors.rs`,
  `filter.rs`, `list.rs`, `shortcode.rs`, `diagnostics.rs`,
  `utils.rs`.
- [x] **2.3** `FromLua` for both types: `LuaInline::new(ud.borrow_inline().clone())`
  — deep-clones out of the source cell into a fresh cell,
  preserving per-invocation independence at filter boundaries.
- [x] **2.4** `get_field` borrows through the cell. Special-cased
  `tag`/`t`/`clone`/`walk` *before* the borrow (so closures don't
  capture a `Ref` lifetime). The main match is
  `match (&*inner, key)`. `set_field` takes `&self` and uses
  `self.0.borrow_mut()`.
- [x] **2.5** `cb:clone()` snapshots the inner value at
  `.clone`-field-access time (matching today's behavior), then each
  call of the returned function deep-clones that snapshot into a
  fresh `Rc`. `__pairs` captures a snapshot too.
- [x] **2.6** `cargo nextest run -p pampa --features lua-filter
  --no-fail-fast`: 3717 passed, 5 failed. The 5 failures are the
  Phase 1 tests that are *supposed* to fail until Phases 3–5 land.
  All pre-existing pampa tests still pass. `cargo build --workspace`
  succeeds — no external caller regressions.
- [x] **2.7** Secondary behavioral change: `__newindex` switched from
  `add_meta_method_mut` to `add_meta_method` (interior mutability now
  comes from the `RefCell`).

### Phase 3 — Proxy userdata: `LuaAttr` variants

- [x] **3.1** `LuaAttr` is now an enum with three variants —
  `Owned(Rc<RefCell<Attr>>)`, `BlockRef(Rc<RefCell<Block>>)`,
  `InlineRef(Rc<RefCell<Inline>>)`. `with_attr` / `with_attr_mut`
  helpers route reads/writes through the active variant via four
  new helpers: `block_attr_ref`, `block_attr_mut`,
  `inline_attr_ref`, `inline_attr_mut`.
- [x] **3.2** Added `attr_to_lua_userdata_for_block` /
  `attr_to_lua_userdata_for_inline` helpers alongside the existing
  `attr_to_lua_userdata` (now explicitly documented as the Owned
  path — used by `pandoc.Attr(...)` and by table-row-like wrappers
  that produce detached snapshots).
- [x] **3.3** All 13 `attr_to_lua_table(lua, &x.attr)` call sites
  in `get_field` for block/inline variants now route through the
  proxy helpers, passing `Rc::clone(&self.0)` for the parent cell.
  The dead `attr_to_lua_table` wrapper is removed.
- [x] **3.4** `set_field` on `"attr"` (block/inline) already went
  through `lua_value_to_attr(val, lua)`, which was updated to call
  `lua_attr.clone_attr()` (works across all enum variants) — so
  assigning a `BlockRef` or `InlineRef` proxy to another element's
  `.attr` copies the target's current Attr value in, correctly
  detaching it from its source cell.
- [x] **3.5** Same machinery (via `clone_attr`). No cross-cell
  aliasing is possible because every assignment copies the value
  through an owned `Attr`.
- [x] **3.6** `LuaAttr::get_field` still returns *fresh Lua tables*
  for `.attributes` and `.classes`. That deliberate decision isolates
  Phase 4 (proxy userdata for those tables) from the Phase 3 enum
  migration. Phase 3 tests therefore still fail — Phase 4 closes the
  gap.
- [x] **3.7** Build: `cargo build -p pampa --features lua-filter`
  succeeds. Tests: 3717 pass, 5 fail (the phase-1 regression
  targets). No new test regressions from Phase 3.

### Phase 4 — Proxy userdata: attributes + classes tables

- [x] **4.1** `LuaAttributesProxy(LuaAttr)` and
  `LuaClassesProxy(LuaAttr)` — each wraps a `LuaAttr` (enum with
  Owned/BlockRef/InlineRef variants) so reads/writes naturally
  dispatch through `with_attr`/`with_attr_mut` to the right cell.
- [x] **4.2** `LuaAttributesProxy` metamethods: `__index` (string-key
  read), `__newindex` (string-key write, `nil` deletes),
  `__pairs` (iterate with key-snapshot so iteration doesn't hold a
  borrow between `next` calls), `__len`, `__tostring`.
- [x] **4.3** `LuaClassesProxy` metamethods: `__index` (int-key read
  + string-key method lookup via the shared List metatable),
  `__newindex` (int-key overwrite/append, `nil` deletes with shift),
  `__pairs`, `__len`, `__tostring`. String-key `__index` builds a
  snapshot-backed List table at lookup time and returns a closure
  that forwards `(proxy, ...args)` as `(snapshot, ...args)` to the
  metatable method — read-only list methods (`includes`, `map`,
  `filter`, etc.) work, matching pre-refactor semantics.
- [x] **4.4** `LuaAttr::get_field` for `.attributes` / `.classes` /
  `attr[3]` / `attr[2]` returns the new proxies.
- [x] **4.5** Whole-table assignment (`cb.attr.attributes = {…}`,
  `cb.attr.classes = {…}`) already works through
  `LuaAttr::set_field` → `lua_table_to_string_map` /
  `lua_table_to_strings`, unchanged.
- [x] **4.6** Tests: 3721 pass, 1 fail (the block-level
  `cb.attributes[k]=v` shortcut, which is Phase 5's scope). The
  other four Phase-1 regression targets — nested `cb.attr.attributes[k]=v`,
  inline `code.attr.attributes[k]=v`, classes append, Owned Attr
  semantics — all pass now.

### Phase 5 — Block/inline shortcuts

Pandoc exposes `cb.attributes`, `cb.classes`, `cb.identifier` as
shortcuts for `cb.attr.2`, `cb.attr.1`, `cb.attr.0`. Block level
previously had `classes` and `identifier` but not `attributes`, and
`classes` returned a snapshot table.

- [x] **5.1** `"attributes"` added to `field_names()` for every
  attr-bearing block (CodeBlock, Header, Div, Figure, Table) and
  inline (Code, Link, Image, Span, Insert, Delete, Highlight,
  EditComment). `classes` already listed; kept.
- [x] **5.2** `get_field` for `"attributes"` returns
  `attributes_proxy_for_block`/`_for_inline`.
  `get_field` for `"classes"` now returns
  `classes_proxy_for_block`/`_for_inline` (upgraded from the
  old fresh-table path — the proxy still supports list-method
  lookups via the snapshot-and-bind dispatch added in Phase 4, so
  `div.classes:includes("foo")` keeps working).
- [x] **5.3** `set_field` for `"attributes"` / `"classes"` whole-
  table assignment added via generic pattern arms (matching any
  `block`/`inline` with `block_attr_mut`/`inline_attr_mut` giving
  `Some`). Piecewise writes go through the proxy's `__newindex`.
- [x] **5.4** `identifier` writes still work unchanged — still use
  `String::from_lua` through per-variant arms.
- [x] **5.5** Unblocked by Phase 5: fixed `pandoc.Attr(id, classes,
  attrs)` to accept proxy userdata for classes/attributes. Updated
  `lua_table_to_strings` and `lua_table_to_string_map` to coerce
  the proxies transparently. Needed because the old 04-filter
  workaround `cb.attr = pandoc.Attr(cb.attr.identifier, cb.attr.classes, attrs)`
  was passing proxy userdata now that `cb.attr.classes` returns a
  proxy.
- [x] **5.6** Tests: 3722 pass, 0 fail on `pampa` — *all* Phase 1
  regression targets now pass.

### Phase 6 — Verify failing tests now pass

- [x] **6.1** `cargo nextest run -p pampa --features lua-filter
  --test test_lua_attr_mutation` — all 5 pass.
- [x] **6.2** `cargo nextest run -p pampa --features lua-filter
  --no-fail-fast` — 3722 passed, 0 failed, 2 skipped.
- [x] **6.3** `cargo nextest run --workspace --no-fail-fast` —
  7624 passed, 0 failed, 195 skipped. Including the smoke-all
  highlighting/04-filter/04-filter-authored-spans.qmd fixture
  (which exercises the Phase 1.5-style workaround for
  backward-compat through `pandoc.Attr(id, cb.attr.classes, attrs)`).

### Phase 7 — Update the 04-filter fixture

- [x] **7.1** `highlight-words.lua` rewritten to the idiomatic
  pattern:
  ```lua
  cb.attr.attributes["data-hl-spans"] = pandoc.json.encode(spans)
  return cb
  ```
  Workaround note removed; replaced with a one-line pointer to
  bd-195t.
- [x] **7.2** End-to-end verified via
  `cargo run --bin q2 -- render crates/quarto/tests/smoke-all/highlighting/04-filter/04-filter-authored-spans.qmd`
  (note: the CLI binary is `q2`, not `quarto`, in this workspace).
  Inspected the generated HTML:
  ```
  <pre class="sourceCode log"><code>2026-04-20T10:12:01 <span class="hl-error">ERROR</span> connection refused
  2026-04-20T10:12:04 <span class="hl-warning">WARN</span> high latency detected
  ```
  `hl-error` appears 3×, `hl-warning` 2×, `sourceCode log` 1×.
- [x] **7.3** `cargo nextest run -p quarto smoke_all` — 1 passed,
  36 skipped. The 04-filter-authored-spans fixture passes with the
  rewritten idiomatic filter.

### Phase 8 — User-facing examples

The whole point of the fix: being able to show filter-authored
highlighting in the docs.

- [x] **8.1** Added
  `crates/quarto/tests/smoke-all/highlighting/06-filter-severity/`
  — a structured systemd-log highlighter with multiple captures
  (`severity.err`, `severity.warning`, `severity.info`,
  `timestamp`). Exercises the idiomatic
  `cb.attr.attributes["data-hl-spans"] = …` idiom end-to-end.
  Verified the produced HTML contains `<span class="hl-severity-err">`
  etc. Smoke-all harness passes (`cargo nextest run -p quarto
  smoke_all`: 1/1). Updated the highlighting README's fixture table.
- [x] **8.2** Added `docs/syntax/highlighting-filter.qmd` — a
  standalone user-facing page describing the `data-hl-spans`
  encoding, a copy-pasteable 15-line filter example, styling
  guidance, pointers to the two smoke-all fixtures, and
  when-to-use-which guidance (filter vs tree-sitter grammar).
  Linked from `docs/syntax/index.qmd` in the Available Features
  list.

### Phase 9 — Cross-platform verification + verification harness

- [ ] **9.1** `cargo xtask verify` — full Rust + hub-client + WASM
  chain. `LuaAttr` is a pampa type; hub-client's WASM build uses
  pampa, so the refactor must keep it green.
- [ ] **9.2** Document completion with the end-to-end snippet per
  CLAUDE.md section "End-to-end verification before declaring
  success".

## Design decisions (confirmed 2026-04-21)

1. **Naming.** Keep the name `LuaAttr` and make it an enum. The vast
   majority of call sites reference it abstractly, and the Lua-side
   userdata type-check is unchanged.
2. **Scope of Rc.** Only `LuaBlock` and `LuaInline` move to
   `Rc<RefCell<…>>`. `LuaAttr` carries either its own owned data or
   a handle back to the block/inline cell. Other Lua-exposed types
   (`LuaMeta`, citations, captions, etc.) are untouched. ~150
   constructor sites affected, mostly mechanical.
3. **Ordering of phases 3 & 4.** Phase 3 first. Phase 4 without 3
   means `cb.attr.attributes[k]=v` still doesn't persist because
   `cb.attr` remains a fresh copy.
4. **Thread safety.** `Rc`/`RefCell` are `!Send`/`!Sync`. Lua state
   runs single-threaded inside a filter invocation (walker is async
   but non-parallel per element; mlua's `Lua` is `!Send` on most
   configurations). If we ever want parallel filtering, switch to
   `Arc<Mutex<…>>`. Out of scope here; flagged as future work.
5. **`LuaAttr` Owned-vs-proxy semantics.** `pandoc.Attr(id, classes, attrs)`
   produces `LuaAttr::Owned(...)`. Mutations on the Owned variant
   don't propagate — correct, because the Attr isn't attached to any
   element. The `cb.attr = a` assignment copies the Owned Attr's
   value into the block's cell. Phase 1.5 locks this down as a test.
6. **Detached-proxy semantics.** If a filter stashes
   `cb.attr.attributes` in a global during one invocation and
   mutates it during a later (different-element) invocation, the
   write lands on a cell the walker already cloned out of — no
   effect on the AST. Document this ("you can't save an element
   across invocations"); don't try to detect via `__gc`. Same rule
   Pandoc effectively enforces.
7. **Docs/examples ordering.** Lua-filter highlighting examples
   (Phase 8) land *after* the proxy fix, so the shipped examples use
   idiomatic code from day one. No interim "known quirk" callout.

## Non-goals

- Generalizing proxy mutation to arbitrary fields (e.g. `cb.content[1] = x`
  modifying the content in place). That's a larger design question and
  is out of scope; today users either read the whole `content` table,
  mutate the table, and reassign, or use `cb:walk(...)`. We're only
  fixing the Attr path because that's what blocks the highlighting
  filter docs.
- Changing how filters are composed or how the walker traverses the
  AST.
- Adding new Lua APIs beyond proxy-enabled reads of existing fields
  (and the `attributes` shortcut at block level that Pandoc provides).

## References

- `claude-notes/plans/2026-04-20-syntax-highlighting-phase-3.5.md` —
  "Follow-up task: Lua attribute-mutation proxy"
- `crates/pampa/src/lua/types.rs:1566-1646` — the read/write path that
  currently returns fresh copies.
- `crates/pampa/resources/lua-types/pandoc/global.lua:17` — the
  `elem.attributes["loading"] = "lazy"` idiom we want to support.
- `crates/quarto/tests/smoke-all/highlighting/04-filter/highlight-words.lua:47-57` —
  the workaround we want to remove.
- Pandoc's native Lua proxy mechanism (for reference): every Lua
  element that wraps an AST node installs `__newindex` on its
  attribute children so writes propagate back. This plan mirrors
  that approach.

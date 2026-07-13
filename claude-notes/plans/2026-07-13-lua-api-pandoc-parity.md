# Plan: Lua API Pandoc parity — mismatch catalog + conformance harness

**Strand**: bd-grkrb9nj (epic)
**Status**: Draft — iterating with Carlos before execution
**Related**: bd-195t (attr-mutation proxy), claude-notes/plans/2026-04-01-lua-constructor-coercion.md (completed predecessor)

## Overview

Quarto 2's Lua API deviates from Pandoc's Lua API in a long tail of small
ways. Pandoc's marshaling layer (`pandoc-lua-marshal`) is extremely
forgiving about how AST objects are constructed; where Q2 falls short of
that flexibility, the failure mode is usually **silent** — attributes
dropped, filter return values ignored, list attributes discarded — which
is the worst possible ergonomics for filter authors.

Goals, in order:

1. **Catalog** the mismatches systematically, starting with AST-node
   constructors (`pandoc.Div`, `pandoc.Span`, `pandoc.Str`,
   `pandoc.Blocks`, …) and the filter-return marshaling boundary.
2. **Build testing infrastructure** that turns the catalog into a
   feedback loop: a conformance suite ported from Pandoc's own tests
   plus a differential oracle harness against a real `pandoc` binary,
   with a burn-down scoreboard.
3. **Fix iteratively**, driven by the scoreboard.
4. **Where we deliberately stay stricter than Pandoc** (a legitimate
   choice — Pandoc's flexibility partly compensates for its opaque
   errors), every strictness divergence must (a) raise an actionable,
   Q-coded diagnostic with source-location info (qmd document and/or
   filter `file:line`), never a silent no-op, and (b) be recorded in a
   divergence registry so the choice is documented and testable.

## Where things stand (verified 2026-07-13)

### Prior work already done

- `claude-notes/plans/2026-04-01-lua-constructor-coercion.md` ported the
  core fuzzy peekers. `peek_inline_fuzzy` / `peek_inlines_fuzzy` /
  `peek_block_fuzzy` / `peek_blocks_fuzzy` +
  `split_string_to_inlines` live in `crates/pampa/src/lua/types.rs`
  (~L1608–1801) and are used by all content-bearing constructors. So
  `pandoc.Para("hello world")`, `pandoc.Div(pandoc.Para(...))`,
  `pandoc.Blocks("text")` etc. already work.
- bd-195t (open) covers the attr-mutation proxy gap:
  `el.attr.attributes[k] = v` hits an ephemeral copy and is silently
  discarded.

### Reference materials (all local)

| What | Where |
|---|---|
| Pandoc marshaling source of truth | `external-sources/pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/*.hs` (cloned 2026-07-13) |
| Pandoc's own Lua conformance tests | `external-sources/pandoc-lua-marshal/test/test-{inline,block,attr,pandoc,metavalue,table,cell,citation,listattributes,simpletable}.lua` — ~2,100 lines, tasty.lua-based |
| Documented contract | `external-sources/pandoc/doc/lua-filters.md` |
| Oracle binary | system `pandoc` 3.9.0.2 with `+lua` (`/opt/homebrew/bin/pandoc`) |
| Q2 implementation | `crates/pampa/src/lua/{constructors,types,list,filter}.rs` |
| Q2 test harnesses | `crates/pampa/src/lua/filter_tests.rs` (unit-level, `apply_lua_filter` + TempDir pattern); `crates/pampa/tests/integration/test_lua_constructors.rs` (`run_filter` helper); `constructors.rs`/`types.rs` in-file `mod tests` |

### Pandoc semantics cheat-sheet (from pandoc-lua-marshal)

The full catalog is long; the load-bearing rules:

- **`peekInlinesFuzzy`** (Inline.hs:138): bare string → *word-split*
  (`Str`/`Space`/`SoftBreak`); table → element-wise `peekInlineFuzzy`
  (strings inside a list become single `Str`s, **no** word-split);
  single userdata → singleton. `__toinline` metamethod hook.
- **`peekBlocksFuzzy`** (Block.hs:145): `__toblock` singleton, else
  element-wise `peekBlockFuzzy`, else single value → singleton. Any
  inlines-coercible value becomes `Plain(inlines)`.
- **`peekAttr`** (Attr.hs:202): string → identifier-only; userdata;
  table with `rawlen > 0` → positional triple `{id, classes, kv}`;
  table with `rawlen == 0` → **HTML-like map** (`id` key → identifier,
  `class` key → space-split into classes, everything else → attributes).
  `peekAttributeList` (Attr.hs:83) accepts a named-key map *or* a list
  of `{k,v}` pairs.
- **Property assignment re-runs the fuzzy peekers** — `div.content =
  "text"`, `header.attr = 'second-test'`, `cite.content = 'boring'`
  all work. Block `content` assignment goes through `peekContent` +
  `setBlockContent` re-projection (Block.hs:292–335).
- **List types**: `Blocks`/`Inlines` get generic List methods
  (`map/filter/clone/extend/find/includes/insert`, `__concat`, `__eq`)
  plus `walk`/`__tostring`; `Blocks:clone`/`Inlines:clone` are **deep**
  (unlike generic `List:clone`, shallow).
- **MetaValue** (MetaValue.hs:40): boolean→MetaBool, string→MetaString,
  number→MetaString (rendered), userdata→singleton MetaInlines/Blocks,
  table→guess via `__name` metafield, then rawlen==0→MetaMap else
  MetaInlines/MetaBlocks/MetaList.
- **Optionals & defaults**: attr defaults `nullAttr` everywhere; Image/
  Link title default `""`; OrderedList listAttributes default
  `(1, DefaultStyle, DefaultDelim)`; Figure caption optional;
  `peekItemsFuzzy` lets BulletList/OrderedList take a single item.

## Seed mismatch catalog (verified against Q2 source, 2026-07-13)

Grouped by class; each class will become one or more child strands.
Items marked ✅-verified were confirmed by reading the Q2 source;
everything must still be **empirically confirmed with a failing test**
before a fix (per TDD policy).

### Class A — filter-return marshaling silently drops values (highest impact)

`crates/pampa/src/lua/filter.rs` return handlers
(`handle_inline_return` L427, `handle_block_return` L457, and the
four `*_with_control` variants):

- ✅ A1. Returning a **bare string** (or number/bool) from a filter
  function is a silent no-op — the `_ =>` arm keeps the original.
  Pandoc coerces `return "hello"` into `Str "hello"` (via the fuzzy
  peekers).
- ✅ A2. Returning a **table containing non-userdata entries** (bare
  strings, nested plain tables) silently drops those entries — splice
  loops keep only `Value::UserData` items. Pandoc runs the whole
  return through `peekInlinesFuzzy`/`peekBlocksFuzzy`.
- A3. Audit remaining return paths (Meta filter returns, Pandoc filter
  returns, Inlines/Blocks list-function returns) for the same
  userdata-only pattern.

Fix shape: route all filter return values through the existing
`peek_*_fuzzy` machinery instead of ad-hoc userdata extraction.

### Class B — Attr argument shapes (silent empty attr)

`parse_attr` (`crates/pampa/src/lua/constructors.rs:759`):

- ✅ B1. Pandoc positional triple `{"id", {"c1","c2"}, {k="v"}}` →
  silently parses as **empty attr** (only named keys are read).
- ✅ B2. Pandoc HTML-like map `{id="x", class="a b", foo="bar"}` →
  silently **empty attr** (`id`/`class` keys ignored; no space-split
  of `class`; unknown keys not collected as attributes).
- ✅ B3. Q2 accepts a **q2-ism** `{identifier=…, classes=…,
  attributes=…}` named form that real Pandoc would *not* interpret
  this way (in Pandoc's HTML-like branch these keys would become
  ordinary attributes / errors). Decide: keep as extension or
  deprecate; either way document + test.
- ✅ B4. Non-string entries in `classes` are silently dropped
  (`filter_map(|r| r.ok())`); numbers should coerce (Lua tostring) or
  error loudly.
- B5. `pandoc.Attr()` constructor and `peekAttributeList` parity:
  list-of-pairs form `{{"k","v"},…}` vs named map; AttributeList
  integer indexing / `__pairs` / delete-by-nil semantics
  (Attr.hs:105–196). Overlaps bd-195t's proxy work.
- B6. Element virtual properties `el.identifier` / `el.classes` /
  `el.attributes` as aliases into `el.attr`, and `el.attr = "id"` /
  `el.attr = {…}` assignment re-running `peekAttr`.

### Class C — constructor signature gaps

`crates/pampa/src/lua/constructors.rs`:

- ✅ C1. **`pandoc.OrderedList(items, listAttributes)` discards its
  second argument** — binds `_list_attr`, hardcodes
  `(1, Default, Default)` (L637–645). `parse_list_attributes` exists
  but is dead on this path. Straight bug.
- ✅ C2. Missing constructors: **`pandoc.Pandoc`**, **`pandoc.Meta`**,
  `pandoc.MetaInlines/MetaBlocks/MetaList/MetaMap/MetaString/MetaBool`,
  `pandoc.SimpleTable`.
- ✅ C3. `pandoc.Citation(...)` returns a **plain Lua table**, and
  `pandoc.ListAttributes(...)` returns a positional plain table —
  Pandoc returns userdata with typed properties for both. Follow-on
  effects: equality, tostring, property assignment.
- C4. Strict scalar args: `pandoc.Str(5)` etc. — Pandoc's `peekText`
  accepts numbers (Lua string coercion); mlua `String` may already
  coerce — verify and pin behavior for `Str`, `Code*`/`Raw*` text,
  `Link`/`Image` target/title, `Header` level (string→int?).
- C5. Enum-ish args: `Quoted`/`Math` accept both `pandoc.SingleQuote`
  style constants and strings in Pandoc; `parse_alignment` and
  `parse_col_width` **default silently on garbage** instead of
  erroring (L989/1016) — the inverse problem (too lax, masks typos
  like `"AlignLeftt"`).
- C6. Optional-argument arity sweep: every constructor's optional
  args/defaults vs the table in the 2026-04-01 plan §2 + Caption/
  Cell/Row/TableBody fuzzy table forms (`peekCellFuzzy`,
  `peekRowFuzzy`, `peekTableBodyFuzzy`, `peekCaptionFuzzy`).

### Class D — property read/mutate persistence (co-top priority with A)

`types.rs` `get_field`/`set_field` implementations + bd-195t.
**Elevated 2026-07-13 by the worked example below**: reads of
container-valued properties (`div.content`, …) return *detached
copies*, so the idiomatic Pandoc pattern `div.content:insert(x)` (and
`table.insert(div.content, x)`, `div.content[1] = …`) mutates an
ephemeral table that is silently discarded. Only whole-property
assignment (`div.content = c`) persists.

- D0. **Content-mutation persistence.** Design decision needed:
  attr solved this with proxy *userdata* (`LuaAttr::BlockRef`,
  `LuaClassesProxy`, `LuaAttributesProxy` — the bd-195t approach),
  but `Blocks`/`Inlines` must remain plain tables with the List
  metatable (for `#`, `ipairs`, `table.insert`, Pandoc parity), so
  the same trick doesn't apply. Pandoc/hslua semantics are
  **property caching + readback**: the getter caches the pushed Lua
  table in the userdata's uservalue (repeated reads return the *same*
  table — aliasing included), and marshaling the element back re-reads
  cached properties. Candidate designs: (a) hslua-style cache+readback
  on `LuaBlock`/`LuaInline` (closest to Pandoc semantics, handles
  aliasing); (b) metamethod-proxy tables (`__index`/`__newindex`/
  `__len` forwarding into the `Rc<RefCell>`) — riskier: raw-access
  paths bypass metamethods. Evaluate against pandoc-lua-marshal
  test-block.lua:155–160 (read does not mutate) and :286–300
  (nested index mutation persists).
- D1. Audit all setters against Pandoc's rule "assignment re-runs the
  fuzzy peeker": `el.content = "text"`, `div.content = pandoc.Para(x)`
  (singleton wrap), `caption.long = "str"`, `cell.contents = …`, etc.
- D2. bd-195t: nested mutation through returned proxies
  (`el.attr.attributes[k] = v`, `el.content[1] = …`,
  `lineblock.content[1][1] = …` — pandoc-lua-marshal
  test-block.lua:227–258 pins nested LineBlock mutation). Verify how
  much the existing proxies already cover; the strand is still open.
- D3. Setting inapplicable properties: Pandoc's `possibleProperty`
  semantics (absent vs error) — decide + match or document divergence.

#### Worked example (verified 2026-07-13; the jumping-off case)

`t1.qmd` (div `a-div` + div `skip-this`) with filter:

```lua
function Div(div)
   if not div.classes:includes("a-div") then return {} end
   local s = pandoc.Str("hello")
   div.content:insert(pandoc.Div({ pandoc.Plain(pandoc.Inlines({ s })) }))
   return div
end
```

- `pandoc -f markdown t1.qmd -L t1.lua` → inserted `<div>hello</div>`
  present. `pampa t1.qmd -F t1.lua -t html` → **silently absent**.
- Isolated: with `local c = div.content; c:insert(...); div.content
  = c` pampa produces Pandoc-identical output — so the loss is
  entirely the ephemeral-copy read, **not** constructor coercion.
- All five constructor forms (`pandoc.Div(s)` through the fully
  explicit `pandoc.Div({pandoc.Plain(pandoc.Inlines({s}))})`) were
  verified OK in pampa once reassignment is used — the 2026-04-01
  fuzzy-peeker work holds.
- These six variants (5 constructor forms × insert idiom, plus the
  reassignment control) become the first Track-2 corpus cases.

### Class E — Blocks/Inlines/List semantics

`list.rs`:

- E1. `Blocks:clone`/`Inlines:clone` deep vs generic `List:clone`
  shallow; `__concat` result type; `__eq` across plain-table vs
  wrapped list; `insert/remove/sort` presence (HsLua.List adds more
  than we may have).
- E2. `walk` traversal order conformance (typewise bottom-up;
  topdown with `false` truncation) — pandoc-lua-marshal
  test-block.lua:559–684 pins this precisely.
- E3. `tostring()` output shapes (used by filter authors for
  debugging; low priority but cheap once harness exists).

### Class F — MetaValue coercions

- F1. Q2 has converters (`meta_value_to_lua` / `lua_to_meta_value`,
  types.rs:1431/1490) but no `pandoc.Meta*` constructors (→ C2) and
  the coercion rules (number→MetaString rendering, `__name`-based
  guessing, non-fuzzy inner peek) need conformance tests.
- F2. Note: q2 metadata is `ConfigValue`, not pandoc `Meta` — the
  mapping layer itself may be a structural divergence to document
  rather than erase.

### Class G — metamethod hooks

- G1. `__toinline` / `__toblock` (deferred in the 2026-04-01 plan).
  pandoc-lua-marshal test-inline.lua:481–528 / test-block.lua:692–740
  pin the semantics, including "ignored when not a function" and
  "non-Inline return ignored". Used by real-world libraries
  (e.g. pandoc's own `pandoc.layout` Doc values in some contexts).

### Class H — error quality (the Q2 advantage)

- H1. Today, coercion failures are bare `mlua::Error::runtime(...)`
  strings with a Lua traceback, not Q-coded diagnostics; no
  `SourceInfo` attached. Infrastructure exists to do better:
  `diagnostics.rs` (quarto.warn/error with `SourceInfo`, Q-11-1) and
  `filter_source_info` (types.rs:1813) already walk the Lua stack for
  provenance.
- H2. Define an error contract for the marshaling layer: every
  rejection names (a) which constructor/property, (b) expected vs got
  (with Lua type names), (c) filter file:line, (d) a Q-code with a
  catalog entry suggesting the fix. E.g.
  `Q-11-x: pandoc.Span: expected Inlines-like content (string, Inline,
  or list of Inlines), got function — at my-filter.lua:12`.
- H3. Divergence registry: a checked-in table
  (`claude-notes/research/lua-api-divergences.md` or a
  machine-readable YAML consumed by tests) listing every place we are
  *deliberately* stricter/different than Pandoc, each entry pointing
  at the test that pins our behavior and the diagnostic it raises.

## Testing infrastructure design (the feedback loop)

Two complementary harnesses plus a policy artifact. Both produce a
**scoreboard** so parity work is burn-down, not whack-a-mole.

### Track 1 — Ported Pandoc conformance suite (fast, hermetic, CI-always)

Port `pandoc-lua-marshal/test/test-*.lua` to run inside Q2's Lua
runtime:

1. Write a minimal **tasty.lua shim** (`test_case`, `test_group`,
   `assert.are_equal`, `assert.are_same`, `assert.is_truthy`, …) so
   the upstream files run with as few edits as possible. Vendor the
   suite under `crates/pampa/tests/lua-conformance/upstream/`
   (copied, per external-sources policy) with a script + README
   recording the upstream commit.
2. A Rust integration test (in `tests/integration/`, per the
   integration-test layout rule) executes each file in the pampa Lua
   runtime and collects per-test-case pass/fail.
3. An **expected-failures file** (`xfail.toml`: test id → reason /
   strand id / "deliberate divergence → registry entry"). CI fails on
   *unexpected* failures **and on unexpected passes** (ratchet). The
   xfail list is the scoreboard.

This immediately gives us hundreds of pinned behaviors for free and is
the primary feedback loop: fix → shrink xfail → repeat.

### Track 2 — Differential oracle harness (catches what Track 1 doesn't)

For behaviors not covered upstream, and for *our* regression corpus:

1. Corpus of small Lua snippets at
   `crates/pampa/tests/lua-conformance/cases/*.lua`. Each case is a
   filter applied to a tiny fixed document (or a snippet returning a
   `pandoc.Pandoc` once C2 lands); the observable is the resulting
   **Pandoc JSON AST** (normalized: strip q2 source-info).
2. Oracle results are **committed snapshots**, generated by a script
   (`scripts/` or an xtask: run `pandoc --lua-filter case.lua` on the
   fixture, emit JSON) with the pandoc version stamped in a header.
   CI never needs pandoc installed; regenerating is a local dev step
   when the corpus changes. Diffs are reviewable in PRs (matches our
   snapshot-test culture).
3. The q2 side runs the same case through `apply_lua_filter` and
   compares normalized JSON. Divergence-registry entries can mark a
   case as "expected divergence" with a pointer, same ratchet
   semantics as Track 1.
4. Error-path cases: for inputs where we *choose* strictness, the
   snapshot records the diagnostic (Q-code + message + location)
   instead of an AST — pinning error quality, not just error
   existence.

### Track 3 — divergence registry + docs

- The registry (H3) is the policy artifact both harnesses consult.
- User-facing summary of deliberate differences goes to `docs/`
  (rendered with q2, per repo policy) once behavior stabilizes.

### Why both tracks

Track 1 answers "do we match Pandoc where Pandoc's own tests look?"
hermetically and fast. Track 2 answers "do we match the *actual
binary* on cases we care about?" (catches doc-vs-implementation gaps,
version drift when we bump the pinned oracle, and covers the filter-
return boundary that pandoc-lua-marshal's suite exercises only
lightly). Both reuse the same corpus format where possible.

## Phases

### Phase 0 — infrastructure spike (first steps)

- [x] 0.1 Vendor pandoc-lua-marshal test suite + record upstream
      commit; get the suite executing under pampa with a pass/fail
      report. (Done 2026-07-13: vendored `test-{attr,inline,block}.lua`
      @ c2dc4e11 into `crates/pampa/tests/lua-conformance/upstream/`;
      no shim needed — hslua's `tasty.lua` is pure Lua and was
      vendored verbatim @ 82c983a9. `prelude.lua` replicates the
      upstream driver env: constructors as bare globals + enum
      constants as strings. Refactored `filter.rs` to extract
      `create_filter_environment()` so conformance runs against the
      production filter environment; all 4048 pampa tests pass after
      the refactor.)
- [x] 0.2 Wire as `tests/integration/lua_conformance.rs` with
      xfail list + ratchet (unexpected pass/fail both fail CI;
      both directions verified by fault injection). Plain-text
      `xfail.txt` (id `# comment` format), not toml — simpler and
      diff-friendlier.
- [x] 0.3 Initial xfail baseline committed: **133 cases, 11 pass,
      122 xfail** (attr 4/18, inline 4/54, block 3/61). The xfail
      file is the empirical catalog baseline. Scope per Decision 3:
      inline/block/attr now, more files later.
- [ ] 0.4 Build the Track-2 skeleton: case format, oracle-snapshot
      generation script (pinned pandoc 3.9.0.2), normalizer,
      comparator; seed with the Class-D worked-example variants
      (insert idiom × constructor forms) plus ~10 cases covering
      Class A and Class B items above.

### Phase 1 — catalog consolidation

- [ ] 1.1 Turn Phase-0 failures into a structured catalog
      (`claude-notes/research/2026-07-lua-api-mismatch-catalog.md`),
      classified A–H, each with: repro snippet, Pandoc behavior, Q2
      behavior, severity (silent vs loud), proposed disposition
      (match Pandoc / stay strict + diagnostic).
- [ ] 1.2 File child strands per class (or per coherent fix unit)
      under bd-grkrb9nj; link bd-195t into Class D.
- [ ] 1.3 Review disposition decisions with Carlos (esp. the
      strict-vs-match calls and the B3 q2-ism).

### Phase 2 — high-impact fixes (order by silent-error severity)

- [ ] 2.1 Class D0: content-mutation persistence (the worked
      example). Requires the cache+readback vs proxy design decision
      first; coordinate with / likely subsumes bd-195t.
- [ ] 2.2 Class A: filter-return values through fuzzy peekers
      (kills the other big silent-no-op class).
- [ ] 2.3 Class B: `parse_attr` full Pandoc shape support
      (positional triple, HTML-like map with class splitting,
      list-of-pairs attributes) + loud diagnostics for rejects.
- [ ] 2.4 C1: OrderedList listAttributes honored.
- [ ] 2.5 Class D1–D3 audit remainder.

### Phase 3 — breadth

- [ ] 3.1 C2/C3: missing constructors; Citation/ListAttributes as
      proper userdata.
- [ ] 3.2 Classes E, F, G per catalog priorities.
- [ ] 3.3 H2 error contract rollout across the marshaling layer
      (dedicated Q-code range for Lua marshaling errors).

### Phase 4 — steady state

- [ ] 4.1 Divergence registry complete; docs/ page on Lua API
      compatibility.
- [ ] 4.2 Oracle-bump procedure documented (new pandoc release →
      regenerate snapshots → triage diffs).

## Decisions (Carlos, 2026-07-13 review)

1. **Strictness policy**: match Pandoc **when Pandoc both accepts the
   parameters and honors the values**. Where Pandoc itself silently
   *ignores/drops* values by default, we should consider **erroring
   first** (with a Q-coded, source-located diagnostic) rather than
   replicating the silent drop. So there are three buckets:
   - Pandoc accepts + honors → we match.
   - Pandoc errors → we error, with better diagnostics.
   - Pandoc silently drops → candidate for a **deliberate divergence**:
     q2 errors (registry entry + diagnostic). Decided case-by-case in
     the catalog's disposition column.
2. **B3 q2-ism** (`{identifier=…, classes=…}` attr form): **keep for
   now**, as a documented extension alongside full support for
   Pandoc's positional-triple and HTML-like shapes. Revisit only if
   the ambiguity with Pandoc's HTML-like map bites in practice.
3. **Track 1 vendoring scope**: start with **inline/block/attr**
   (`test-inline.lua`, `test-block.lua`, `test-attr.lua`), grow later.
4. **Corpus location**: keep as much as possible **in pampa** —
   aspirationally pampa is a pure-Rust pandoc port that speaks with a
   heavy Quarto accent. (`quarto.*` API remains out of scope for this
   epic — q2-only, no Pandoc oracle.)
5. **Oracle pinning**: pin pandoc **3.9.0.2** exactly, stamp version
   in snapshot headers; bumps are deliberate PRs. Pandoc's Lua
   behavior is stable over time, so churn should be minimal.

## Source-of-truth references

| Topic | File |
|---|---|
| Fuzzy peekers (Q2) | `crates/pampa/src/lua/types.rs:1608-1801` |
| Constructors + parse_attr (Q2) | `crates/pampa/src/lua/constructors.rs` (`parse_attr` L759, OrderedList L635) |
| Filter-return handlers (Q2) | `crates/pampa/src/lua/filter.rs:427-662` |
| List metatables (Q2) | `crates/pampa/src/lua/list.rs` |
| Diagnostics + SourceInfo bridge (Q2) | `crates/pampa/src/lua/diagnostics.rs`, `types.rs:1813` (`filter_source_info`) |
| Pandoc fuzzy peekers | `external-sources/pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/{Inline,Block}.hs` |
| Pandoc Attr shapes | `.../Marshal/Attr.hs:83-253` |
| Pandoc content setters | `.../Marshal/Block.hs:292-335`, `Content.hs:61-77` |
| Pandoc MetaValue | `.../Marshal/MetaValue.hs:40-138` |
| Pandoc conformance tests | `external-sources/pandoc-lua-marshal/test/*.lua` |
| Documented contract | `external-sources/pandoc/doc/lua-filters.md` (Inlines §3374, Blocks §3035, List §4840) |

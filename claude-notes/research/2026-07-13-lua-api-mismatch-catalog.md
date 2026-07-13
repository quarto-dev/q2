# Lua API mismatch catalog (empirical baseline, 2026-07-13)

**Strand**: bd-grkrb9nj · **Plan**: `claude-notes/plans/2026-07-13-lua-api-pandoc-parity.md`

Source of data: the two conformance harnesses committed in Phase 0
(`crates/pampa/tests/lua-conformance/`), at their initial baselines:

- Track 1 (vendored pandoc-lua-marshal suite): 133 cases, **11 pass /
  122 xfail** (`xfail.txt` carries the per-test messages).
- Track 2 (differential vs pandoc 3.9.0.2): 8 cases, **2 pass /
  6 xfail** (`differential/xfail.txt`).

This note groups the baseline failures by root cause and records the
disposition for each. Dispositions follow Decision 1 in the plan:
match Pandoc wherever Pandoc accepts *and honors* a shape; where
Pandoc silently drops, consider erroring with a Q-coded diagnostic
(divergence-registry entry).

| # | Root cause | Evidence (baseline) | Classes | Disposition | Strand |
|---|---|---|---|---|---|
| 1 | **Element/list `__eq` missing.** `Str('a') == Str('a')` is false; every tasty `are_equal` on elements and every `are_same` deep-compare bottoming out at userdata fails. | ~68 of 122 Track-1 xfails (element-eq 26, deep-compare 42) | E | Match Pandoc (structural equality, ignoring source info) | bd-55mb0rjz |
| 2 | **`pandoc.Attr` / `parse_attr` argument shapes.** Positional triple and HTML-like map silently → empty attr in element constructors; `pandoc.Attr` rejects table-as-first-arg (8×"error converting Lua table to String"), rejects list-of-pairs / AttributeList-userdata attributes (6×"attributes must be a table"). `attr.classes` lacks the List metatable (test-attr:65). Keep the q2-ism named form per Decision 2. | ~17 Track-1 xfails + 2 Track-2 (B1, B2) | B | Match Pandoc; keep q2 named-key extension | bd-tzwcof0n |
| 3 | **Filter-return marshaling drops non-userdata.** Bare string return is a no-op; bare strings inside returned tables vanish. | Track-2 A1/A2 cases (confirmed against oracle); code: `filter.rs` return handlers | A | Match Pandoc (route returns through fuzzy peekers) | bd-23yvjfmm |
| 4 | **Content-mutation persistence.** Property reads return detached copies; `div.content:insert(x)` silently lost. Pandoc/hslua semantics: getter caches pushed value in uservalue (reads alias), marshal-out reads back. Decision: cache+readback design (not metamethod proxies). | Track-2 D0 case; bd-195t (attr variant, partially proxied already); several Track-1 nested-mutation tests inside cluster 1's counts | D | Match Pandoc (cache+readback) | bd-hitjclzp + bd-195t |
| 5 | **Property setters incomplete.** "cannot set field 'content'" on BulletList/OrderedList/DefinitionList-shaped blocks (Content re-projection missing), "cannot set field 'quotetype'/'mathtype'"; assignment must re-run fuzzy peekers. | 6 Track-1 xfails + part of cluster 1 | D1 | Match Pandoc (`setBlockContent` re-projection semantics) | bd-0g2yp61w |
| 6 | **`pandoc.List` module parity.** `List{…}`/`List(…)` not callable in q2 ("attempt to call a table value"); needed by walk-order tests too. | 7 Track-1 xfails | E | Match Pandoc (callable module, `List:new`, methods) | bd-1fjtodu8 |
| 7 | **Missing constructors.** `AttributeList` (3 xfails), `pandoc.Pandoc`, `pandoc.Meta*`, `pandoc.SimpleTable` (not exercised by vendored trio yet); `Citation`/`ListAttributes` return plain tables instead of userdata (surfaces via cluster 1 equality too). | 3 Track-1 xfails + code audit | C2/C3 | Match Pandoc | bd-sgfiiktn |
| 8 | **OrderedList discards ListAttributes.** `start`=1, style/delim Default regardless of argument. | Track-2 C1 case; Track-1 test-block:273 ('1' vs '4') | C1 | Straight bug fix | bd-0xghpvij |
| 9 | **Constructor arg peekers, misc.** `Cite` citations arg ("expected table of citations" — single citation / fuzzy forms), `Table` bodies ("expected table of TableBody" — single body), `Caption` ("expected Inline userdata…" — caption fuzzy forms). | 4 Track-1 xfails | C6 | Match Pandoc | bd-sgfiiktn |
| 10 | **`tostring` output shapes.** q2 prints `Inlines {Str(...)}`; pandoc prints `[Str "word"]`-style native repr with element payloads. | ~6 Track-1 xfails | E3 | Match Pandoc (cheap once repr helper exists) | bd-55mb0rjz |
| 11 | **`__toinline`/`__toblock` metamethods.** Not consulted by q2 coercion. | 4+ Track-1 xfails | G | Match Pandoc | bd-olz91r4v |
| 12 | **Error-message contracts.** Upstream tests pattern-match pandoc's error strings (e.g. `'Inline, list of Inlines, or string'`). q2 wants richer Q-coded diagnostics instead. | 2 Track-1 xfails | H | **Deliberate divergence candidate**: keep q2 messages, ensure they contain the expected *substance*; register + permanently xfail with `# DIVERGENCE` | bd-9p2686pc |

Progress log:

- **2026-07-13, bd-0xghpvij closed** (cluster 8): OrderedList honors
  ListAttributes. Differential xfail 6 → 5.
- **2026-07-13, bd-55mb0rjz closed** (clusters 1 + 10): element/list
  `__eq` (structural, source-info-ignoring, via the JSON writer's
  source-free serialization) and Haskell-show `tostring`
  (`crates/pampa/src/lua/show.rs`, formats probed against pandoc
  3.9.0.2). Track-1 xfail **122 → 64**, zero new failures. The
  post-fix residue clusters: Attr shapes ~17 (bd-tzwcof0n), List not
  callable 7 (bd-1fjtodu8), setters 6 (bd-0g2yp61w), missing
  constructors/peekers ~7 (bd-sgfiiktn), walk semantics ~6 (was
  masked by eq; revisit strand split when attacking it),
  content-mutation persistence ~5 (bd-hitjclzp), error-message
  contracts 2 (bd-9p2686pc), classes-proxy vs List table ~3
  (bd-tzwcof0n).

- **2026-07-13, bd-2j048yfm closed** (walk cluster): traversal engine
  rewritten as `crates/pampa/src/lua/walk.rs`, mirroring
  pandoc-lua-marshal Walk/SpliceList/Topdown: single children map
  (now covering Table cells/captions, DefinitionList, Citation
  prefix/suffix — all skipped by the old hand-rolled passes), subtree
  rule for `elem:walk` (fixes the `i:walk(filter), false` C-stack
  overflow), four-pass typewise order, topdown list-level stops +
  truncation. Track-1 **98 → 110 pass / 23 xfail** (all 12 walk
  xfails flipped); differential **17 → 19 cases, all pass**. One
  pre-existing q2 test updated per normative decision (pandoc compat
  over q2's old synthetic wrapper-list `Blocks` visit).

- **2026-07-13, bd-1fjtodu8 closed** (cluster 6 + the clone half of
  the old cluster-1 residue): pandoc.List module is callable
  (`List(t)`/`List{…}` in-place, `List()` empty, non-table → loud
  "table expected" error) via a metatable on the module table itself;
  the stray `__call` on the instance metatable was removed (instances
  are not callable in pandoc); `Inlines:clone`/`Blocks:clone` deep
  (generic `List:clone` stays shallow). Track-1 **90 → 98 pass / 35
  xfail** (5 BulletList-content + 2 deep-clone + 1 AttributeList
  flipped); differential **15 → 17 cases, all pass**. The walk-order
  tests that use `List` incidentally now fail on walk semantics
  proper — the walk cluster is unmasked and ready to refile.

- **2026-07-13, bd-23yvjfmm closed** (cluster 3): all filter-return
  paths (element, list, and the four topdown `*_with_control`
  handlers, plus the two ad-hoc typewise list-splice sites) route
  through `peek_inlines_fuzzy`/`peek_blocks_fuzzy`. Bare-string
  returns coerce like pandoc (word-split; Plain-wrapped in block
  position); non-userdata table entries coerce element-wise;
  number/boolean returns are loud errors naming the filter function
  and got-type (pandoc errors too — probes P1–P13 against 3.9.0.2).
  A3 audit: shortcode `classify_table_result` fixed the same way
  (was dropping non-userdata entries AND discarding inlines when a
  table mixed inlines+blocks); doc-level filter gap (`Pandoc`/`Doc`
  collected but never invoked; no `Meta` support) filed as
  bd-a9g50za2. Differential **8 → 15 cases, 15 pass, 0 xfail**;
  Track-1 unchanged at 90/133 (this cluster was Track-2-only).

- **2026-07-13, bd-hitjclzp closed** (cluster 4 + part of 5):
  hslua-style property cache+readback. `div.content:insert(x)` and
  friends persist; reads alias; flush happens at every marshal-out.
  Also added the list-shaped-block `content` setters and Figure/Table
  `caption` setters (cluster 5 partial). Track-1 xfail **64 → 60**,
  differential 5 → 4. Remaining known mutation gap:
  `attr.classes:insert` (classes proxy, → bd-tzwcof0n).

- **2026-07-13, bd-sgfiiktn S1** (part of clusters 7 + 9): Citation is
  typed userdata (`LuaCitation`: property cache on prefix/suffix,
  structural `__eq`, Haskell-show `__tostring`, deep `:clone`, eager
  validated setters); `pandoc.Cite` argument order flipped to Pandoc's
  `(content, citations)` (comment c-inqf5qlb) with the strict
  list-of-Citation-userdata peeker ("table expected, got Citation" /
  "Citation expected, got X"); `cite.citations` is an aliased
  pandoc-List of Citation userdata with cache+readback persistence.
  CitationMode constants added to the conformance prelude.
  Track-1 xfail **81 → 72** (7 test-citation + 2 Cite flips, zero new
  failures); differential **19 → 20 cases, all pass**
  (cite-construct-userdata; normalizer now strips `citationIdS`).
  E2e byte-identical to pandoc 3.9.0.2 through the real binary.

- **2026-07-13, bd-sgfiiktn S2** (rest of cluster 7's
  ListAttributes half + the OrderedList alias entries of cluster 9):
  ListAttributes is typed userdata (start/style/delimiter, eager
  validated setters, structural __eq, :clone); constructor and
  triple peeker validate loudly (garbage styles no longer silently
  default; partial triples error like pandoc's peekTriple);
  OrderedList gained listAttributes (cached, aliased, nested
  mutation persists) + delimiter, with start/style/delimiter as
  hslua-style aliases reading/writing through the cached value.
  Track-1 xfail **72 → 59** (13 flips, zero new failures);
  differential **21/21** (orderedlist-la-userdata-mutation). E2e
  byte-identical to pandoc 3.9.0.2.

- **2026-07-13, bd-sgfiiktn S3** (clusters 7/9 table-part halves +
  the Table-field-marshaling residue): Cell/Row/TableHead/TableFoot/
  TableBody/Caption are cache-backed typed userdata (properties,
  attr aliases through a cached LuaAttr, __eq/__tostring/:clone,
  Cell:walk + Row:walk); peekRowFuzzy/peekCellFuzzy fuzzy forms;
  Table head/foot/bodies/colspecs round-trips with nested-mutation
  persistence; bodies accepts a single TableBody; caption accepts
  bare block lists; Image caption alias. Arg-order parity fixes:
  TableBody(body, head, rhc, attr), Caption(long, short). Track-1
  xfail **59 → 30** (29 flips, zero new failures); differential
  **22/22** (table-parts-nested-mutation). E2e byte-identical to
  pandoc 3.9.0.2. Version skew recorded: the 3.9.0.2 binary lacks
  pandoc.TableBody and Cell:clone; contract is pandoc-lua-marshal
  @ c2dc4e11.

- **2026-07-13, bd-0g2yp61w closed** (cluster 5 remainder): element
  `attr` assignment re-runs `parse_attr` (bare string / triple /
  HTML-like map / flushed userdata all accepted on set, matching
  "assignment re-runs the fuzzy peeker"); `quotetype`/`mathtype`
  setters added with eager loud validation. Track-1 xfail **30 → 25**
  (5 flips, zero new failures); differential **23/23**
  (setter-repeek-attr-enums). E2e HTML identical to pandoc 3.9.0.2.

- **2026-07-13, bd-olz91r4v closed** (cluster 11): `__toinline`/
  `__toblock` metamethod hooks in all four fuzzy peekers, with
  hslua's recoverable-failure semantics (non-function metafield,
  call error, or wrong return type fall through to normal coercion).
  Track-1 xfail **25 → 21** (4 flips); differential **24/24**
  (toinline-toblock-hooks). E2e byte-identical to pandoc 3.9.0.2.
  Remaining 21 xfails: 17 Pandoc/Meta* (bd-2llqjsms), 2 SimpleTable
  (bd-d4wd6r3i), 2 error-message contracts (bd-9p2686pc).

Notes:

- Cluster 1 (`__eq`) masks finer-grained results: once equality works,
  some tests will flip to pass and others will reveal second-order
  mismatches. Expect a large xfail churn (in both directions) on that
  fix — that is the ratchet working as intended. (Confirmed: the fix
  flipped 58 tests with zero new failure ids.)
- The `walk`-related tests currently fail via clusters 1 and 6; do not
  file a separate walk strand until those land and the residue is
  visible.
- The authoritative child-strand list lives under the epic:
  `braid dep tree bd-grkrb9nj`.

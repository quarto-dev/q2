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

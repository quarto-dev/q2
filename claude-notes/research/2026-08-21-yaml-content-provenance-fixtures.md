# YAML content-provenance fixtures (measured)

Ground truth for Phase 2 of
`claude-notes/plans/2026-08-20-provenance-1-foundations.md`. Regenerated
2026-08-22 by running the committed generator
(`yaml-content-provenance-walker/walker.rs`) against `quarto-yaml` 0.1.2 as
checked out at `~/src/quarto-yaml`, so the `span` and `decoded value` columns
are **yaml-rust2's actual output**, not a hand derivation. Phase 2 should
transcribe these into tests rather than re-derive them — re-deriving risks
encoding the author's reading of the YAML folding rules instead of the behavior
the walker must match.

**32 shapes, zero desyncs, zero byte-mismatches.** Extended 2026-08-22 (Task 9)
with the seven cases the plan's Phase 2 checklist required but the initial
measurement pass didn't cover — see § Known gaps for what was added and why.
**40 shapes total, zero desyncs, zero byte-mismatches**, still against
`quarto-yaml` 0.1.2.

Extended again 2026-08-22 (Phase 2, fix round 2) with `plain multi-line,
trailing space before fold` — the shape that caught a real walker desync
under `strict-provenance`. **41 shapes total, zero desyncs, zero
byte-mismatches.** See § Rule 1's entry is style-conditional.

## How to read a row

- `span` is the node's existing `source_info` (`start..end`), absolute byte
  offsets into `source`. Unchanged by this work.
- `indent` is `marker.col()`, which the walker needs for block styles. Flow
  styles (plain, quoted) use **0** — folding strips all line-leading
  whitespace in a flow scalar, so there is no indent to skip.
- `expected pieces` are `content_range <- source_range`, with **absolute**
  source offsets. A `*` marks a **replacement**; unmarked pieces are
  **verbatim**, meaning the source range is *byte-identical* to the content
  range. That distinction is not cosmetic — see § The verbatim tag.
- A source range extending past `span`'s end is expected, not a bug: the walk
  is bounded by the decoded value, not by the span.
- A zero-width source range (`n..n`) with non-zero content length is
  synthesis: content with no source byte.
- Zero-*content* pieces **are** stored (`double-quoted escaped break` has one),
  because the piece list must tile its source contiguously or `preimage_in`
  yields no hull. Measured: dropping it turns `preimage_in` from `Some(4..14)`
  into `None`, with byte-identical offset mapping either way.

## The verbatim tag

`verbatim` is decided by **byte-identity**, never by length. A fold whose source
run is exactly `\n` and whose content is one space is 1→1 with different bytes;
tagging it verbatim would produce a piece claiming a byte-identical source range
it does not have — which any caller that treated `preimage_in`'s hull as a
byte-identity claim would then copy. The `root plain, col-0 continuation` row below is that case, and
it is the reason the tag exists. Only adjacent **verbatim** pieces coalesce.

## Fixtures

| shape | style | indent | source | span | decoded value | expected pieces (`content`<-`source`, `*` = replacement) | |
|---|---|---|---|---|---|---|---|
| `block | single-line` | block | 2 | `"k: |\n  aaa\n"` | 7..10 | `"aaa\n"` | `0..4`<-`7..11` | ok |
| `block | 3 lines` | block | 2 | `"k: |\n  aaa\n  bbb\n  ccc\n"` | 7..22 | `"aaa\nbbb\nccc\n"` | `0..3`<-`7..10` `3..4`<-`10..13`* `4..7`<-`13..16` `7..8`<-`16..19`* `8..12`<-`19..23` | ok |
| `block | no final newline` | block | 2 | `"k: |\n  aaa"` | 7..10 | `"aaa\n"` | `0..3`<-`7..10` `3..4`<-`10..10`* | ok |
| `block |- strip` | block | 2 | `"k: |-\n  aaa\n  bbb\n"` | 8..17 | `"aaa\nbbb"` | `0..3`<-`8..11` `3..4`<-`11..14`* `4..7`<-`14..17` | ok |
| `block |+ keep` | block | 2 | `"k: |+\n  aaa\n\n\n"` | 8..11 | `"aaa\n\n\n"` | `0..6`<-`8..14` | ok |
| `block | blank line inside` | block | 2 | `"k: |\n  aaa\n\n  bbb\n"` | 7..17 | `"aaa\n\nbbb\n"` | `0..3`<-`7..10` `3..5`<-`10..14`* `5..9`<-`14..18` | ok |
| `block | more-indented line` | block | 2 | `"k: |\n  aaa\n    bbb\n  ccc\n"` | 7..24 | `"aaa\n  bbb\nccc\n"` | `0..3`<-`7..10` `3..4`<-`10..13`* `4..9`<-`13..18` `9..10`<-`18..21`* `10..14`<-`21..25` | ok |
| `block |2 indicator` | block | 2 | `"k: |2\n    aaa\n    bbb\n"` | 8..21 | `"  aaa\n  bbb\n"` | `0..5`<-`8..13` `5..6`<-`13..16`* `6..12`<-`16..22` | ok |
| `block | trailing spaces on last line` | block | 2 | `"k: |\n  aaa\n  bbb   \n"` | 7..16 | `"aaa\nbbb   \n"` | `0..3`<-`7..10` `3..4`<-`10..13`* `4..11`<-`13..20` | ok |
| `block | CRLF` | block | 2 | `"k: |\r\n  aaa\r\n  bbb\r\n"` | 8..18 | `"aaa\nbbb\n"` | `0..3`<-`8..11` `3..4`<-`11..15`* `4..7`<-`15..18` `7..8`<-`18..20`* | ok |
| `block | tab in content` | block | 2 | `"k: |\n  a\tb\n"` | 7..10 | `"a\tb\n"` | `0..4`<-`7..11` | ok |
| `block | content starts with `|`` | block | 2 | `"k: |\n  |pipe\n"` | 7..12 | `"|pipe\n"` | `0..6`<-`7..13` | ok |
| `block | content is exactly `|`` | block | 2 | `"k: |\n  |\n"` | 7..8 | `"|\n"` | `0..2`<-`7..9` | ok |
| `block > fold + blank line` | block | 2 | `"k: >\n  aaa\n  bbb\n\n  ccc\n"` | 7..23 | `"aaa bbb\nccc\n"` | `0..3`<-`7..10` `3..4`<-`10..13`* `4..7`<-`13..16` `7..8`<-`16..20`* `8..12`<-`20..24` | ok |
| `block > more-indented (not folded)` | block | 2 | `"k: >\n  aaa\n    bbb\n"` | 7..18 | `"aaa\n  bbb\n"` | `0..3`<-`7..10` `3..4`<-`10..13`* `4..10`<-`13..19` | ok |
| `plain single-line` | plain | 0 | `"k: hello\n"` | 3..8 | `"hello"` | `0..5`<-`3..8` | ok |
| `plain multi-line` | plain | 0 | `"k: aaa\n  bbb\n  ccc\n"` | 3..18 | `"aaa bbb ccc"` | `0..3`<-`3..6` `3..4`<-`6..9`* `4..7`<-`9..12` `7..8`<-`12..15`* `8..11`<-`15..18` | ok |
| `plain multi-line, trailing space before fold` | plain | 0 | `"k: a \n  b\n"` | 3..9 | `"a b"` | `0..1`<-`3..4` `1..2`<-`4..8`* `2..3`<-`8..9` | ok |
| `plain multi-line CRLF` | plain | 0 | `"k: aaa\r\n  bbb\r\n"` | 3..13 | `"aaa bbb"` | `0..3`<-`3..6` `3..4`<-`6..10`* `4..7`<-`10..13` | ok |
| `single-quoted` | quoted | 0 | `"k: \'hello\'\n"` | 3..10 | `"hello"` | `0..5`<-`4..9` | ok |
| `single-quoted with ''` | quoted | 0 | `"k: \'it\'\'s\'\n"` | 3..10 | `"it\'s"` | `0..2`<-`4..6` `2..3`<-`6..8`* `3..4`<-`8..9` | ok |
| `single-quoted trailing ''` | quoted | 0 | `"k: \'its\'\'\'\n"` | 3..10 | `"its\'"` | `0..3`<-`4..7` `3..4`<-`7..9`* | ok |
| `single-quoted all-escape` | quoted | 0 | `"k: \'\'\'\'\n"` | 3..7 | `"\'"` | `0..1`<-`4..6`* | ok |
| `double-quoted \t` | quoted | 0 | `"k: \"a\\tb\"\n"` | 3..9 | `"a\tb"` | `0..1`<-`4..5` `1..2`<-`5..7`* `2..3`<-`7..8` | ok |
| `double-quoted \u00e9` | quoted | 0 | `"k: \"a\\u00e9b\"\n"` | 3..13 | `"aéb"` | `0..1`<-`4..5` `1..3`<-`5..11`* `3..4`<-`11..12` | ok |
| `double-quoted many escapes` | quoted | 0 | `"k: \"a\\\\b\\\"c\\td\"\n"` | 3..15 | `"a\\b\"c\td"` | `0..1`<-`4..5` `1..2`<-`5..7`* `2..3`<-`7..8` `3..4`<-`8..10`* `4..5`<-`10..11` `5..6`<-`11..13`* `6..7`<-`13..14` | ok |
| `double-quoted multi-line fold` | quoted | 0 | `"k: \"hello\n  world\"\n"` | 3..18 | `"hello world"` | `0..5`<-`4..9` `5..6`<-`9..12`* `6..11`<-`12..17` | ok |
| `double-quoted escaped break` | quoted | 0 | `"k: \"aaa\\\n  bbb\"\n"` | 3..15 | `"aaabbb"` | `0..3`<-`4..7` `3..3`<-`7..11`* `3..6`<-`11..14` | ok |
| `root plain, col-0 continuation (1-byte fold)` | plain (root) | 0 | `"aaa\nbbb\n"` | 0..7 | `"aaa bbb"` | `0..3`<-`0..3` `3..4`<-`3..4`* `4..7`<-`4..7` | ok |
| `k: ~` | plain | 0 | `"k: ~\n"` | 3..4 | `"~"` | `0..1`<-`3..4` | ok |
| `k: true` | plain | 0 | `"k: true\n"` | 3..7 | `"true"` | `0..4`<-`3..7` | ok |
| `quoted key` | quoted | 0 | `"'quoted key': v\n"` | 0..12 | `"quoted key"` | `0..10`<-`1..11` | ok |
| `flow collection item 0` | quoted | 0 | `"k: ['a b', \"c\\td\"]\n"` | 4..9 | `"a b"` | `0..3`<-`5..8` | ok |
| `flow collection item 1` | quoted | 0 | `"k: ['a b', \"c\\td\"]\n"` | 11..17 | `"c\td"` | `0..1`<-`12..13` `1..2`<-`13..15`* `2..3`<-`15..16` | ok |
| `tagged scalar` | quoted | 0 | `"k: !path 'x'\n"` | 9..12 | `"x"` | `0..1`<-`10..11` | ok |
| `double-quoted plain` | quoted | 0 | `"k: \"hello\"\n"` | 3..10 | `"hello"` | `0..5`<-`4..9` | ok |
| `double-quoted \n` | quoted | 0 | `"k: \"a\\nb\"\n"` | 3..9 | `"a\nb"` | `0..1`<-`4..5` `1..2`<-`5..7`* `2..3`<-`7..8` | ok |

| shape | style | indent | source | span | decoded value | expected pieces (`content`<-`source`, `*` = replacement) | |
|---|---|---|---|---|---|---|---|
| `empty value` | plain | 0 | `"k:\n"` | 3..3 | `""` | **none** | ok |
| `empty single-quoted` | quoted | 0 | `"k: \'\'\n"` | 3..5 | `""` | **none** | ok |
| `empty block scalar` | block | 3 | `"k: |\n"` | 3..4 | `"\n"` | `0..1`<-`4..5` | ok |
| `empty block scalar, next key follows` | block | 0 | `"k: |\nj: 1\n"` | 5..9 | `""` | **none** | ok |

The second table is the degenerate set. Three shapes produce **zero pieces**,
which is why `ProvenanceBuilder` needs an anchor offset independent of the
pieces (plan § The shared builder). All four derive successfully — see below on
why "empty" is not `None`.

`empty block scalar` (`k: |` alone) works via the **header-skip rule**: its span
covers the `|` header rather than any content, so the walk starts at the newline
ending the header line. The predicate is *not* "the span's first byte is `|` or
`>`" — that fires falsely on a block scalar whose **content** starts with a
pipe, which is valid YAML and measured above (`block | content starts with |`,
`block | content is exactly |`). It is that byte test **and** the decoded value
being empty or all-newlines.

`empty block scalar, next key follows` has a span pointing at **the next key**
(`j: 1`); harmless for provenance (the value is empty, so there are no pieces to
misplace) but wrong as a diagnostic span. The plan puts that one out of scope.

## Provenance is `Option`, and "empty" is not `None`

Every row in both tables derives **successfully** — they are `Some(...)`,
including the ones whose piece list is empty (`k:`, `k: ''`). `None` is reserved
for "the derivation desynced", which no shape here produces. When transcribing
these into tests, assert `Some` explicitly for the empty shapes; that
distinction is the one place a merged `None` could be misread. See the plan's
§ Desync policy.

## Known gaps

*Resolved 2026-08-22 (Task 9).* Seven cases the plan's Phase 2 checklist
requires had no row here, because the generator enumerated block-mapping
scalar values plus one root scalar, and none of those shapes is a non-string
scalar, a key, a flow-collection item, or a tagged scalar. `walker.rs` gained
`emit_node`, a variant of `emit` that takes an already-resolved node (a hash
key via `as_hash()`, a flow-collection item via `as_array()`) instead of
looking one up under `"k"`, plus a `val` override for the two scalars whose
decoded value isn't recoverable via `Yaml::as_str()`. The seven cases, now
measured in the table above:

- `k: ~` and `k: true` — non-string scalars, confirming "content" means the
  event's value string (`~`, `true`) and not `self.yaml`
- a quoted **key** (`'quoted key': v`) — `key_span` has the identical defect,
  and derives the same way as a value
- a **flow collection** (`k: ['a b', "c\td"]`) — two rows, one per item,
  since the generator's original `emit` reads `get_hash_value("k")` and only
  ever reaches the mapping value's own node
- a **tagged scalar** (`!path 'x'`) — confirms the tag needs no extra
  arithmetic, per the plan's note that the marker points at the value
- a **plain double-quoted** scalar with no escapes (`k: "hello"`) — every
  other double-quoted fixture above has at least one escape in it
- **`\n` as an escape** (`k: "a\nb"`) — same 3-piece shape as the `\t` row

One gap remains **unrecorded, deliberately**: no fixture here exercises the
break-region rule's "value at a tab" entry sub-case (a value tab at a break
yields a zero-content piece and re-enters the loop). The plan calls recording
it optional and is explicit that the rule itself must not change to
accommodate it; this pass left it out rather than reach for a rule change to
make it cheap to add.

## Rule 1's entry is style-conditional (fix round 2, 2026-08-22)

The `strict-provenance` feature (added after this note's first 40 shapes were
measured) found a real desync outside this table's coverage:
`key: a \n  b` (a plain scalar with a trailing space before a folded line
break). YAML strips that trailing space before folding, so it has no content
byte of its own and must be claimed by rule 1's break region — but the
original entry test (source cursor exactly at `\n`/`\r`) let rule 3 consume
it as a lone verbatim byte one iteration before rule 1 recognized the fold
was starting, stranding the walk. No shape above exercises this because none
of them has a *flow*-style trailing space immediately before a break.

The fix widens rule 1's entry **only for flow styles** (plain, single- and
double-quoted): source cursor at a whitespace run that *contains* a newline,
rather than at the newline itself. Block styles keep the original narrow
entry. This is not a per-shape special case — it mirrors `indent`, which
already varies by style for the same underlying reason (flow folding strips
line-leading whitespace; block styles don't). A first attempt widened entry
universally and desynced `block | trailing spaces on last line` above: a
block literal's trailing spaces are *content*, not stripped, so absorbing
them into a break-region piece hands the value-side cap
(`ve.min(vi + newlines.max(1))`) a run it can't size correctly — that cap
assumes entry starts at a real newline, which the universal widening broke
for block styles specifically.

**Probed, not added as a fixture: does `>` folding also need the wide
entry?** `k: >\n  a \n  b\n` (a `>` scalar with a trailing space before a
fold) derives correctly under the **narrow**, unwidened entry:

```
| `PROBE: block > trailing space before fold` | block | 2 | `"k: >\n  a \n  b\n"` | 7..13 | `"a  b\n"` | `0..2`<-`7..9` `2..3`<-`9..12`* `3..5`<-`12..14` | ok |
```

The oracle's reason: unlike plain scalars, `>` folding does not strip a
line's trailing whitespace — it only replaces the *line break* with a
folded separator (a space, here). So the source's trailing space is
preserved as its own literal content byte (matched by rule 3, coalesced into
the surrounding verbatim run: `0..2 <- 7..9` is `"a "` verbatim, i.e. `a`
plus the literal trailing space), and the fold's inserted separator space is
the *only* byte the break region needs to produce (`2..3 <- 9..12*`, 3
source bytes — the newline plus the 2-byte base indent — collapsing to that
1 separator byte). Not added to the table above because it doesn't exercise
anything the existing `block > fold + blank line` / `block > more-indented`
rows don't already cover, and the plan's ruling was explicit not to widen
`>` speculatively without a shape that needs it.

# Provenance, Plan 1 of 3: foundations (`quarto-source-map`, `quarto-yaml`)

**Epic:** `bd-mxa44voa` — *Nested-parse source mapping drifts when the inner
text was unescaped.*
**Siblings:** Plan 2 = `2026-08-20-provenance-2-consumers.md`, Plan 3 =
`2026-08-20-provenance-3-audit-and-fix.md` — both **alongside this file** and
current, since all three merged onto `feature/yaml-provenance` (integration
order 1 → 2 → 3). Read them here, not from a review worktree.

**Absorbed strands** — closed, their content is this plan's checklist:
`bd-jmquuiqh` (piecewise provenance for multi-line block scalars),
`bd-th2ah982` (the same for escaped quoted scalars).

## The bug class in one paragraph

A decoder returns a **decoded** string (quotes stripped, escapes resolved,
block-scalar indentation removed) paired with a `SourceInfo` describing the
**raw** source text. Callers then compute source positions as
`base + content_offset`. That is only valid when the decoded content is a
byte-identical, prefix-aligned slice of the raw span. It is not, for quoted
scalars (off by the opening delimiter), multi-line block scalars (off by the
stripped indentation, **compounding per line**), multi-line plain and quoted
scalars (off by each collapsed fold), or any escaped scalar (off by one byte
per escape). `SourceInfo::substring` composes a purely **affine** map and
validates nothing, so the error is silent.

Worked example — `_quarto.yml` line 7, file byte 67 onward:

```
byte:   67 …  80  81  82 …                        102 103 104  105 …
char:    ␣    ␣   '   <  s p a n   i d = " x " > A s k   A I ␣  ✨       <  /  s …
                  ↑   ↑                                       └ 3 bytes ┘
        opening quote │                          '</span>' truly starts here ─┘
                content starts at 82
```

`</span>` is at content offset 23. q2 computes `81 + 23 = 104`; the truth is
`82 + 23 = 105`. Byte 104 is two bytes inside `✨`, and the renderer sliced
there and aborted the whole render. (Re-verified byte-for-byte against the
fixture: quote at 81, content at 82, `✨` at 102..105, `</span>` at 105.)

## Why the fix belongs in `quarto-yaml` and not in the consumer

`quarto-yaml` is meant to be a **batteries-included, provenance-aware YAML
crate**. Content provenance for a scalar is exactly that crate's job, and the
derivation algorithm below is an *implementation detail* that pairs with the
current yaml-rust2 backend. A future implementation — a different parser, or
a decoder that emits pieces directly — can satisfy the same
`content_source_info()` contract by construction rather than by derivation.
That is the durable reason for the placement: the interface is stable, the
derivation is not.

Three supporting reasons, all of which also argue producer-side:

- **Style and indent are exact here.** `TScalarStyle` and `marker.col()` are in
  hand at the event. A consumer would have to re-derive the style, and scan
  *backwards* from the span start for a `|`/`>`/`|2` header that the span
  deliberately excludes.
- **The invariant is enforceable at construction**, where yaml-rust2's decoded
  `value: String` is in scope (`parser.rs:499-516`).
- **One implementation serves every consumer** — the markdown re-parse,
  `use_cmd/config.rs:229`, and anything later — instead of each re-deriving it.

Note that the reconstruction *is* possible at the consumer — the lockstep walk
needs only (raw span text, decoded value, block indent, escape mode), all of
which a consumer can obtain — so the argument above is about ownership and
exactness, not about impossibility. Three separate q2 authors have independently
hit this bug class and worked around it locally, each at a cost in capability —
see Plan 2's § Workarounds that collapse.

## Findings that correct the original diagnosis

Salvaged from the superseded plan, plus measurements from the 2026-08-21
review. All measured on real input, not inferred — and each one prevents a
plausible wrong turn.

**The scalar-style drift table.** The original table claimed plain scalars
drift by 0. That holds only for *single-line* plain scalars; folding makes
multi-line plain and multi-line quoted scalars drift like block scalars.
Measured span-vs-value byte counts:

| style | today | drift |
|---|---|---|
| plain, single-line | correct | 0 |
| plain, multi-line | wrong stride | one collapsed fold per line break (`aaa\n  bbb\n  ccc`: span 15, value 11) |
| single/double-quoted, single-line | 1 byte left | constant −1 |
| quoted with escapes | worse per escape | −1, then −1 per collapsed escape (`'it''s'`: span 7, value 4) |
| quoted, multi-line | wrong stride | −1 base **plus** one collapsed fold per break (span 15, value 11) |
| block, single-line | correct | 0 |
| block, multi-line | **reports the wrong line** | +indent per preceding line, accumulating (span 15, value 12) |

Quoted scalars are wrong at the *base*; folding and block indentation are
wrong in the *stride*. Single-line block scalars look fine only because they
never cross a newline. With a three-line block scalar, two warnings that both
belong on line 9 are reported at `8:10` and `9:14`.

**Content is not a subset of the node span.** Three measured shapes where the
decoded value needs source bytes at or past `source_info.end_offset()`:

| source | span | value |
|---|---|---|
| `k: \|`⏎`  aaa` (no final newline) | 3 (`aaa`) | 4 (`"aaa\n"`) — **no source byte exists** |
| `k: \|+`⏎`  aaa`⏎⏎⏎ | 3 (`aaa`) | 6 (`"aaa\n\n\n"`) — kept newlines are outside the span |
| `k: \|`⏎`  aaa`⏎`  bbb   `⏎ | 9 | 11 — `block_scalar_len` trims the last line |

This is the *default* case, not an edge case: every clip-chomped block scalar's
value ends in a `\n` whose source byte sits one past the span end. Consequences
in § Design.

**Do not "fix" the `length() - 1` fallback.** The original strand blamed the
end-offset fallback in `quarto-error-reporting`
(`map_offset(length())` failing over to `map_offset(length() - 1)`,
`diagnostic.rs:842-851`). It is not implicated. Instrumentation showed
`used_fallback=false` on both diagnostics, and for `Original`/`Substring` it is
*structurally incapable* of producing a mid-character offset: `map_offset`
returns `None` only when `offset > total_length`, so the fallback succeeds only
when `S + L - 1 == total_length` exactly — always a char boundary. The only
case where it could misbehave is a `Concat` with overlapping pieces, where
`length()` (sum of piece lengths) disagrees with
`last.offset_in_concat + last.length`. Anything built via `concat()` is
immune. **Leave it alone.**

**Why the bad offset survived to the renderer** — this is the *why* behind
Phase 1's `Location.offset` fix. `offset_to_location`
(`quarto-source-map/src/file_info.rs:85-127`) already floors mid-character
offsets, with a comment saying tree-sitter and Pandoc ranges "occasionally
produce offsets that land inside a multi-byte UTF-8 sequence… instead of
panicking" — almost certainly the k-330/k-328 fix. But it floors
`safe_offset` for the **column** computation and returns the **raw** offset:

```rust
let mut safe_offset = offset;
while safe_offset > line_start && !content.is_char_boundary(safe_offset) {
    safe_offset -= 1;                       // ← floors, for column only
}
let column = content[line_start..safe_offset].chars().count();
Some(Location { offset, row, column })      // ← raw, unfloored offset
```

So `Location` ships one sanitized field next to one unsanitized field, and
which you consume decides whether you crash. That asymmetry explains
everything: the JSON path reads `.column` and cannot panic; the ariadne path
reads `.offset` (`diagnostic.rs:873-878`) and did. It also means **k-330 was
never "markdown-only"** — it fixed the column computation, so no `.offset`
consumer was ever covered, on any path.

**There are three implementations of this conversion, and they disagree.** An
earlier draft claimed `FileInformation::offset_to_location` was the only
non-test `Location` construction site in `quarto-source-map`. That is wrong,
and it steered the audit away from the others:

| site | column for a mid-char offset | offset |
|---|---|---|
| `FileInformation::offset_to_location` (`src/file_info.rs:85`) | **floored** (6) | raw (7) |
| free `offset_to_location` (`src/utils.rs:8`, re-exported at `lib.rs:46`) | **overcounts** (7) — its loop `break`s only once `current_offset >= offset`, so the character containing the offset has already been counted | raw (7) |
| `offset_to_location_bytes` (q2 `quarto-parse-errors/src/error_generation.rs:330`, a documented "bytes-aware sibling") | its own rule | raw |

The same input yields columns differing by one *between two functions in the
same crate*. The free one is live in q2 **production**:
`pampa/src/pandoc/treesitter.rs:1463`, `:1464`, `:1485`, `:1486` and
`quarto-config/src/span_assert.rs:188`. And
`pampa/tests/integration/test_location_health.rs:448` asserts the two agree —
it feeds a `Location`'s own `offset` back through `utils::offset_to_location`
and compares row/column — so changing one without the other moves that test.
Phase 1's audit covers all three, plus q2 and `quarto-yaml`.

## Risks

- **Flooring `Location.offset` is an observable change** for any consumer that
  passes non-boundary offsets — caret positions shift. Measured scope: for a
  range wholly inside one character (`Original{7,8}` over `x = 'A✨B'`, emoji
  at 6..9), the raw `Location`s are `offset 7..8` with `column 6..6`, and
  after flooring `offset 6..6` — width zero. **The columns already collapse
  today**, so at the `Location` level flooring introduces no new hazard: it
  propagates existing `column` behavior to `offset`, which is the
  self-consistency this fix is for. The *rendered* result is a different
  question, and there the change is a regression — because the renderer ceils
  the **raw** offset, and flooring upstream destroys the input its ceil needs.
  Both statements are true of different layers; cost (a) below is the rendered
  one. **Decided 2026-08-21: floor anyway, accepting three costs.**
  (a) *A caret regression.* `quarto-error-reporting`'s
  `snap_span_to_char_boundaries` (`src/diagnostic.rs:671-686`) floors the start
  and **ceils** the end, so today `Original{7,8}` inside `✨` renders as `6..9`
  — the whole character highlighted. After flooring, both ends arrive as `6`,
  the ceil finds `6` already on a boundary, and the highlight becomes
  **zero-width**. A currently-useful full-character caret becomes a caret of no
  width. Accepted because mid-character offsets only arise where the mapping is
  already wrong and Plans 2-3 remove them — but it is a regression, not a pure
  fix, and the zero-width label must be tested in Plan 2, since both renderers
  live in `quarto-error-reporting`.
  (b) *A wire change.* `.offset` is not renderer-only: `pampa`'s JSON writer
  emits it as `"o"` (`src/writers/json.rs:550`, `:555`, `:2005`, `:2014`,
  `:2258`, `:2267`) and `quarto-core`'s TS engine reads it as `file_offset`
  (`src/engine/ts_engine.rs:689`). Expect JSON-writer snapshot churn for any
  q2 fixture carrying a mid-character offset — and note that neither consumer
  snaps, which is the affirmative case for flooring.
  (c) *A silent coverage loss upstream, found 2026-08-21 and not part of the
  original decision.* `quarto-error-reporting` is itself a
  `quarto-source-map = "0.1.0"` consumer, and its two regression tests **for
  this very crash** —
  `ariadne_span_starting_inside_multibyte_char_does_not_panic`
  (`src/diagnostic.rs:1601`) and its annotate-snippets twin (`:1635`) — build
  `SourceInfo::original(file_id, 21, 28)` where 21 is mid-`✨`. Once that crate
  picks up 0.1.2, `map_offset` floors 21→19 *before the renderer is reached*, so
  the snapping code under test is never exercised. The tests keep passing:
  their precondition `assert!(!content.is_char_boundary(21))` is about the
  fixture text, not the mapped offset. **Both tests must be re-anchored below
  the source-map layer** — feed the renderer a mid-character offset directly, or
  unit-test `snap_span_to_char_boundaries` — or this epic loses its only
  regression coverage for the panic that started it. Tracked in
  § Hand-off to Plan 2.
  Rejected alternatives: leave `offset` raw and document the asymmetry (keeps
  the wider caret, leaves the type self-inconsistent); or make the policy
  direction-aware in `map_range` (`src/mapping.rs:84`) so starts floor and ends
  ceil — architecturally the best answer, but it requires
  `quarto-error-reporting` to abandon its two separate `map_offset` calls, so it
  is more Plan 2 churn for a case that should stop occurring.
- **The `strict-provenance` assert is necessary, not sufficient.** Equal
  lengths do not prove a correct mapping: a `Concat` can tile the right total
  while pointing at wrong source ranges. Say so in the code, or a green CI
  step will be mistaken for verification. Under the lockstep derivation the
  length check is close to tautological (you consume the whole value or you
  fail), so the load-bearing checks are **desync detection** and the
  derivation-ran-implies-`Some` assert — see § Desync policy.
- **`Concat` degrades four accessors, in two different ways.** The benign pair
  signals failure: `resolve_byte_range()` returns `None` unconditionally
  (`source_info.rs:403`) and `preimage_in` returns `None` when gappy. Most
  production consumers degrade gracefully on that, and two name `Concat`
  explicitly (`navigation_href.rs:604-607`, `span_assert.rs:159`) — **but not
  all, and the exception matters.** `bind_source_candidates` opens with
  `info.resolve_byte_range()?` (`quarto-core/src/config_sources.rs:90`), so a
  diagnostic whose location is `Substring{parent: Concat}` — what the re-parse
  produces once it takes `content_source_info` as its parent — never registers
  `_quarto.yml`, and prints with **no source snippet at all**, where today the
  founding repro prints one with carets on the wrong line. Losing the snippet is
  worse than misplacing the caret. The renderer already gets this right: it
  takes the file id from `root_file_id()`, which sees through
  `Substring{parent: Concat}` (`source_info.rs:521-532`), at all four of its
  label sites (`quarto-error-reporting/src/diagnostic.rs:819`, `:925`, `:1022`,
  `:1077`). So binder and renderer disagree today about how to obtain the same
  value, and the binder picked the accessor that cannot see through a `Concat`.
  No API change needed here; Plan 2 fixes the binder.
  The other pair is worse still: `start_offset()` returns **0** and
  `end_offset()` returns the **content length**, so there is no `None` to check
  and a caller gets a plausible-looking wrong number — **and the same defect exists in
  TypeScript, one composition level up.** `resolveChain`'s `Substring` arm
  (`ts-packages/annotated-qmd/src/source-map.ts:301-315`) computes
  `range: [parentStart + localStart, parentStart + localEnd]` — affine
  composition over the parent's resolved range, which is exactly
  `preimage_in`'s bug in another language. `Substring{parent: Concat}` is
  precisely the shape these value spans take once provenance is correct, so that
  arm is where the work lands. A confident wrong answer, not a failure to
  resolve. Plan 2 Phase 4 owns it, and the fix is entirely on the TS side.
  Note what is *not* wrong: `resolveChain`'s `Concat` arm
  (`source-map.ts:317-375`) walks the pieces via `toMappedString(id)` and takes
  its range from `map(0)` and `map(len-1)+1`, consulting the serialized `r`
  only on error paths. A well-formed `Concat` resolves correctly today; the
  defect is one composition level up. (See § Reversed decisions, R6.)
  **This is not an argument for changing the Rust accessors.** `Option`
  returns would be breaking, and a `debug_assert` would fire on legitimate
  content-space uses; `start_offset()` returning 0 and `end_offset()` returning
  the content length are the *correct* answers in content space.
  Exposure is nonetheless low, and it is worth recording why so nobody
  re-derives it. The consumer that would hurt most is `qmd-syntax-helper`,
  which rewrites source files at `location.start_offset()`
  (`conversions/q_2_33.rs:74-75`, `attribute_ordering.rs:74`,
  `div_whitespace.rs:77` — three of **23 `start_offset()` sites across 22 files**
  in that crate) — but it is unreachable from here: its diagnostics
  come from its own `pampa::readers::qmd::read` call
  (`diagnostics/q_2_30.rs:58`, `syntax_check.rs:69`), i.e. tree-sitter spans
  produced before any of this provenance exists. `remap_file_ids` is
  variant-complete (`source_info.rs:485-511`), so new `Concat`s survive
  reconciliation; only the test-only `file_id_of`
  (`quarto-ast-reconcile/src/remap.rs:526`, which panics on non-`Original`)
  needs a touch. This is still the reason Plan 2 must keep
  `ConfigValue.source_info` contiguous; see § Hand-off to Plan 2.

## Design

**What Phase 1 actually changes, in code** — the sections below explain *why*,
and at ~700 lines they are not a prerequisite for starting:

```
quarto-source-map/src/file_info.rs  ~:122   Location{ offset } -> offset: safe_offset
quarto-source-map/src/utils.rs      ~:8     same floor, and stop the column loop early
quarto-source-map/src/mapping.rs    :64-70  last.length -> last.source_info.length()
quarto-source-map/src/source_info.rs :453   Substring arm: None when parent is Concat
quarto-source-map/src/source_info.rs :410   doc comment: locating, not copying
                                    (new)   ProvenanceBuilder — held for 0.1.3
```

**Which section gates which phase.** Phase 0 needs none of § Design. Phase 1
needs § `SourceInfo::Concat` is already the right shape and § `preimage_in`
composes affinely (the four fixes and the builder contract). Phase 2 needs
§ How the pieces are derived, § The shared builder and § `quarto-yaml`'s API,
plus the fixtures note. § Reversed decisions is reference material — read an
entry when you are about to propose the thing it retracts.

### `SourceInfo::Concat` is already the right shape

`Concat { pieces }` is a piecewise map: each `SourcePiece` carries
`offset_in_concat`, `length`, and its own `source_info`, and `map_offset`
locates the containing piece and recurses. Content provenance for a
multi-line block scalar is a `Concat` of one piece per line, each pointing at
that line's post-indent range.

Verified against the `quarto-source-map` 0.1.1 checkout, so this is
known-viable:

- `SourceInfo::concat(Vec<(SourceInfo, usize)>)` is **public**
  (`source_info.rs:203`); `SourcePiece`'s fields are all `pub`
  (`source_info.rs:130-138`). **No new enum variant, no upstream API
  change** — consistent with the ipynb design doc's finding that the closed
  `SourceInfo` enum need not change.
- `concat()` assigns `offset_in_concat` cumulatively, so pieces tile
  contiguously by construction. This also disposes of a latent wrinkle where
  `Concat::length()` sums piece lengths while `map_offset`'s exclusive-end
  branch tests `last.offset_in_concat + last.length`; they agree for anything
  built via `concat()`.
- A piece is `(SourceInfo, content_length)`, and the two need not match.
  `Concat::map_offset` (`mapping.rs:54-74`) computes
  `offset_in_piece = offset - piece_start` and recurses; the `Original` arm is
  a bare `start_offset + offset` with **no clamp against `end_offset`**. So a
  1-content-byte piece over a 2-source-byte escape maps offset 0 to the
  escape's start, which is the desired behavior.

**`map_offset(length())` is wrong for a `Concat`, and Phase 1 fixes it.** The
exclusive-end branch (`mapping.rs:64-70`) is

```rust
return last.source_info.map_offset(last.length, ctx);
```

where `last.length` is the piece's **content** length. For a verbatim piece
that equals its source length and the answer is the true source end; for a
replacement it does not. The comment above it claims the branch maps "to the
end of the last piece… like `Original`/`Substring`'s `map_offset(length)`" — and
for `Original` that *is* the end offset. So `Concat` is inconsistent with the
other variants in exactly the way `Location.offset` is inconsistent with
`Location.column`: the same class of defect, one variant over.

The fix is to use the piece's **source** length:

```rust
return last.source_info.map_offset(last.source_info.length(), ctx);
```

Measured on the three terminal shapes:

| last piece | today | fixed | truth |
|---|---|---|---|
| verbatim | `Some(9)` | `Some(9)` | 9 — unchanged |
| replacement (`''`→`'` over `Original{7,9}`) | `Some(8)` | `Some(9)` | 9 |
| synthesis (`Original{eof,eof}`, content length 1) | **`None`** | `Some(11)` | 11 (eof) |

The synthesis row is the one neither the earlier draft nor the proposal
predicted: today a `Concat` ending in a synthesized piece yields **no end
offset at all**, because `map_offset(content_length=1)` on `Original{11,11}`
computes 12 > `total_length`. That is a clip-chomped block scalar at EOF — an
ordinary shape — losing its caret's right edge entirely.

A deletion can never be the last piece, so the fix has no other terminal case —
but **not** because deletions are dropped (they are stored; see § The shared
builder). The reason is that the walk is bounded by the *value*: it terminates
as soon as the value is exhausted, before reaching any trailing source-only
region. Measured — `k: "aaa\`⏎`  "` (a trailing escaped break) yields the single
piece `0..3`←`4..7` and no trailing zero-content piece. So a stored zero-content
piece is always interior, which is what makes this fix's three terminal shapes
exhaustive.

**It does, however, change one existing production path — do not claim
otherwise.** q2 has exactly two non-test `SourceInfo::concat` producers.
`cell_options` (`quarto-core/src/cell_options/mod.rs:196-228`) pairs each piece
with a source range of matching length by construction — that is why the epic
calls it exemplary — so it is genuinely unaffected. But the QMD writer's
provenance concat (`pampa/src/writers/qmd.rs:2880-2903`) pairs each block's
`source_info()` with the number of bytes **written** (`buf.len() - start`),
which routinely differs from that block's source span whenever the writer
normalizes anything. For that `Concat`, `map_offset(length())` today resolves to
`last_block_source_start + bytes_written` — arithmetic across two coordinate
systems — and after the fix resolves to `last_block_source_end`. That is an
improvement, but it is an observable change in a production path and belongs in
the release notes.

This supersedes an earlier decision to accept the shortfall and document it.
Documenting it was the wrong call for two reasons: the caret is short by the
collapsed bytes for *every* value ending in an escape, and Plan 2's Phase 3
asserts exact end columns, so it would have had to work around a defect that a
one-liner removes.

### `preimage_in` composes affinely over a `Concat` parent, and must not

Found 2026-08-21 by the Plan 3 review session, verified here. `preimage_in`'s
`Substring` arm (`source_info.rs:453-456`) is

```rust
let parent_range = parent.preimage_in(target)?;
Some(parent_range.start + start_offset..parent_range.start + end_offset)
```

— affine, and therefore valid only when the parent is byte-identical to its
content. Over a `Concat` parent it is not. Measured, on a gap-free `Concat`
modelling `'it''s'` (content 4 bytes, source extent 1..6):

| | result |
|---|---|
| `A.preimage_in(fid)` where `A` is the `Concat` | `Some(1..6)` — correct hull |
| `C.preimage_in(fid)` where `C = substring(A, 0, 4)` | **`Some(1..5)`** — under by exactly the collapsed escape byte |
| `C.map_offset(0)` / `C.map_offset(4)` | 1 / 6 — both correct |
| `A.resolve_byte_range()` / `C.resolve_byte_range()` | `None` / `None` |

That is this epic's founding defect — off by one per collapsed escape —
reproduced *inside* the fix for it, and it fails silently where
`resolve_byte_range` at least returns `None`. The verbatim-tagging rule in
§ How the pieces are derived protects `preimage_in` at the **piece** level; this
is the **composition** level, which that rule does not reach.

**Why it belongs in 0.1.2 rather than being noted.** `Substring{parent: Concat}`
is the shape **every** AST node from a nested re-parse carries — `quarto-yaml`'s
`make_source_info` wraps its parent in exactly that (`parser.rs:344-361`), so it
is what Plan 2's config path, Plan 2 Phase 4's attribute path and Plan 3's
comrak path all produce. And `preimage_in` is the writer's verbatim-copy
decision across **26 production calls** — 20 in
`pampa/src/writers/incremental.rs` (blocks at `:171`, `:669`, `:1116`; inlines
at `:798`, `:1365`, `:1372`; table rows and cells at `:1253`, `:1264`; plus
`:421`, `:424`, `:672`, `:675`, `:746`, `:821`, `:826`, `:1299`, `:1306`,
`:1564`, `:1599`, `:1668`) and 6 in
`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs` (`:314`, `:315`, `:660`,
`:1817`, `:1823`, `:1828`), which documents at `:651-652` that it *relies* on
`preimage_in` handling a contiguous `Concat` correctly and at `:1850` that a
`None` there "corrupts provenance (wire-format …)" — that second one is the doc
of a `#[cfg(test)] mod`, not the production module doc. Those are line-classified calls,
not `grep -c` mentions (§ Reversed decisions, R7).

**What actually separates the safe accessors from the wrong ones — and it is not
the arithmetic.** All four **as shipped today** — the `preimage_in` row is what
this section changes:

| accessor | its `Substring` arm | the parent's `Concat` arm | result |
|---|---|---|---|
| `map_offset` | `parent.map_offset(start_offset + offset)` — **recurses** (`mapping.rs:49-53`) | piecewise | **correct** |
| `resolve_byte_range` | `parent_start + start_offset` (`source_info.rs:400-401`) | `None` (`:403`) | **refuses** |
| `preimage_in` | `parent_range.start + start_offset` (`:453-456`) | `Some(hull)` | **wrong answer** |
| TS `resolveChain` | `parentStart + localStart` (`source-map.ts:301-315`) | `Some(range)` from the pieces | **wrong answer** |

`resolve_byte_range` has **arithmetic identical to `preimage_in`'s** and is safe
only because the thing it composes over refuses to answer. So the defect is not
"someone wrote `+`" — it is "someone wrote `+` over a parent willing to hand back
a flattened range."

**That predicts where a fifth instance comes from, and it is a change someone
would make thinking they were improving things.** `resolve_byte_range`'s doc says
`Concat` "doesn't map cleanly to a single contiguous byte range" — while
`preimage_in`, three functions away, *does* return a hull for a contiguous
`Concat`. A reader who notices that asymmetry and "fixes" it by teaching
`resolve_byte_range` to return the hull turns 24 in-tree call sites from safe
into silently wrong in one commit, with no test failing.

**Do not make the `Concat` arms consistent with each other — in either
direction.** They differ because their callers differ:
`preimage_in`'s hull is an offset claim whose documentation must now say so
(Phase 1), and `resolve_byte_range`'s `None` is load-bearing. Both directions
arm the same downstream `Substring` arms:

- *Don't teach `resolve_byte_range` to return a hull* — the case above.
- *Don't teach `preimage_in` to return a hull where this plan makes it refuse.*
  This is the **more likely** of the two, and the harder to catch in review,
  because after 0.1.2 the git history will show that `preimage_in` "used to"
  answer for a `Substring{parent: Concat}` — so restoring it reads as undoing an
  over-conservative change rather than as proposing a new one. It is neither: it
  is the length-preserving refinement that was proposed and withdrawn during
  this epic (§ Rejected, above), and it is precisely the mechanism the
  membership test predicts. If the argument arrives as "we only return the hull
  when every piece is length-matched", that is the withdrawn proposal verbatim,
  and the counterexample is the 1→1 fold.

**Fix: return `None` from the `Substring` arm when the parent is a `Concat`.**
Honest failure, matching `resolve_byte_range`, and the fallback direction is
safe — writers decline to verbatim-copy and rewrite instead
(`postprocess.rs:317`/`:669`/`:1833` fall back to `combine()`). The
all-verbatim case needs no rescuing because `finish()` collapses it to an
`Original`/`Substring` before a parent ever sees a `Concat`.

**Rejected: a length-preserving refinement instead of `None`.** The Plan 3
session proposed, then argued for, keeping the affine path when every piece
satisfies `piece.length == piece.source_info.length()`. It is the natural idea
and it is **unsound for this function**, because it tests offset-affineness
while `preimage_in` needs *byte-identity* — it is the writer's "can I
Verbatim-copy these bytes?" check. Measured on the round-3 fold case, a root
plain scalar `aaa`⏎`bbb` whose pieces are verbatim `0..3`, a 1→1 **replacement**
`3..4` (source `\n`, content one space) and verbatim `4..7`:

| | result |
|---|---|
| passes `p.length == p.source_info.length()` for every piece | **true** |
| so the refinement returns | `Some(0..7)` |
| which licenses Verbatim-copying | source `"aaa\nbbb"` for content `"aaa bbb"` |

Length equality cannot distinguish that from a genuine verbatim run, and
`SourceInfo` carries no verbatim tag — the tag in § How the pieces are derived
lives in the *builder*, choosing which method to call, and does not survive into
the emitted value. So the refinement admits exactly the byte-substitution this
epic is about. `None` is the sound answer.

**And the regression it was meant to prevent is mostly not there.**
`cell_options`' pieces come from `option_content_ranges`, which returns ranges
*within* each line **excluding** the `#| ` prefix
(`cell_options/mod.rs:180-192`), so consecutive option lines leave a source gap
where the next prefix sits — and a gappy `Concat` already yields `None` today,
bare or through a `Substring`. Measured. Only a **single**-option cell produces
one piece and therefore a correct affine hull that `None` would take away, and
no `preimage_in` call site appears to reach cell-option nodes at all (they are
not `Block`/`Inline` source_infos). Phase 1 turns that inspection into a test
rather than leaving it load-bearing.

**Two things this does not fix, stated so nobody assumes otherwise.** First, no
path from any of this to wrong *output* bytes has been traced: the config path's
`Concat`s live in metadata rather than the body the incremental writer copies,
and while Plan 2 Phase 4 and Plan 3 put them on real body nodes, neither traced
that to `incremental.rs`. Second — and this is a new defect, not a caveat — the
**bare `Concat` arm has the identical byte-identity gap**: measured, the fold
shape above returns `Some(0..7)` from `preimage_in` *today*, with no `Substring`
involved. So `preimage_in`'s hull is an **offset** claim, not a byte-identity
claim, and any consumer using it to justify copying bytes needs more than it
offers once `Concat`s can contain length-matched non-identical pieces. That
cannot be fixed inside this function — it has no text to compare — so it is a
contract limitation to document here and an audit item for Plan 3, not a 0.1.2
change.

**Its own doc comment currently asserts the claim this retracts**, which is what
would mislead the next reader. `source_info.rs:410-413` opens:

> Byte range in `target` that this `SourceInfo`'s preimage covers, if any.
> This is the writer's "can I Verbatim-copy bytes from `target` for the node
> carrying this source_info?" check.

That second sentence is now false for a `Concat`. Phase 1 rewrites it to say
that a `Some(hull)` licenses **locating** a position, not **copying** bytes; that
for a `Concat` the hull is an offset claim only; and that the reason is the 1→1
fold — a piece whose source run and content run have equal length and different
bytes, which no length or contiguity check can detect. A caller that needs to
copy needs byte-identity, which this function cannot supply.

**Reachability: resolved as LATENT, and the safety is incidental.** Traced by
the Plan 3 session, verified here. The one confirmed *copy* site is
`incremental.rs:169-181`'s `KeepBefore` arm, whose `.get()` guard checks bounds
rather than identity and would therefore be defeated by a fold piece. It is safe
today for a reason that has nothing to do with `preimage_in`: its baseline is
`capture_untransformed_ast_json` (`quarto-core/src/pipeline.rs:1006-1022`),
which parses the document bytes afresh with **`parent_source_info: None`** and
runs **before any pipeline stage**. So the write-back AST is a second,
independent parse with its own source-info pool, and no content-provenance
`Substring` — config-derived or otherwise — can appear in it.

That closes the question I had left open ("does a config-derived Block or Inline
survive into the write-back AST"): it never gets the chance. Config-derived body
nodes with `Concat`-rooted provenance *are* real —
`parse_config_string_as_markdown` returns `PandocBlocks` as well as
`PandocInlines`, and their nodes carry `Substring{parent}` via
`location.rs:214-217` — but the incremental writer never sees them.

**The distinction worth keeping is that this is incidental, not structural.**
Nothing about `preimage_in` protects that call site; the shape of the preview
capture does. Thread a parent into that baseline parse, or baseline against a
transformed AST, and `incremental.rs:172` becomes a live wrong-bytes bug with no
diagnostic. Plan 3's Phase 1 therefore ships a **regression guard on that
invariant** — asserting `parent_source_info: None` and the pre-stage ordering,
with `incremental.rs:172` named in the failure message so whoever breaks it
lands on the copy site — rather than a fix. Recording it here because the
invariant now load-bears for this epic's correctness and lives in a file neither
of these two crates owns.

**One producer eliminated, with a caveat.** `combine()` cannot *introduce* a
fold piece: it pairs each piece with `piece.length()` (`source_info.rs:322-330`),
which for a whole `Original`/`Substring` is the source extent, so its output is
byte-identical by construction. That rules out the postprocess-coalescing family
of Block/Inline-level `Concat`s. But the guarantee is only "introduces none",
**not** "output is always clean": `combine` over an already fold-bearing
`Concat` propagates it, because `Concat::length()` returns the sum of *content*
lengths rather than a source extent (`source_info.rs:345`). Since combine
results demonstrably get re-fed — the doubled-self bug at
`postprocess.rs:1845-1852` is one — the weaker statement is the one to rely on.

**The trap this section exists to name, because four implementations fell into
it.** "Affine composition over a non-affine parent" appeared **four** times
during the design of this epic, twice as shipped code and twice as a proposed
fix — and three of the four were written or proposed by someone who had already
rejected the same idea elsewhere:

| # | instance | status |
|---|---|---|
| 1 | Rust `preimage_in`'s `Substring` arm (`source_info.rs:453-456`) | shipped; this section fixes it |
| 2 | TypeScript `resolveChain`'s `Substring` arm (`annotated-qmd/src/source-map.ts:301-315`) | shipped; Plan 2 Phase 4 fixes it |
| 3 | a length-preserving predicate proposed *for* #1 | proposed and withdrawn — see § Rejected, above |
| 4 | this plan's own `finish()` collapse rule | written, then corrected (§ The shared builder) |

**Not the same as the founding defect, though they rhyme.** It was suggested
that `SourceInfo::substring` belongs in this table as a third shipped instance,
since § The bug class opens by calling it "a purely affine map". It does not, and
the distinction is worth holding because the two need different fixes.
`substring` + `map_offset` composes **correctly** over a `Concat` parent —
measured: for the `'it''s'` fixture, `C.map_offset(0)` → source 1 and
`C.map_offset(4)` → source 6, both right, while `C.preimage_in` returns the wrong
`1..5`. The founding defect is a *coordinate-space* error — callers hand
`substring` offsets into the **decoded** string while the parent describes the
**raw** text — and its fix is to supply the right parent, which is this whole
plan. The four rows above are a *range-arithmetic* error: summing a resolved
source range with a content offset. Same family resemblance — assuming a linear
correspondence between two strings that differ — but one is fixed by changing
what you pass in and the other by refusing to answer.

Plan 3's § Accessor discipline records the **union** of both readings as a
five-row table with a status per instance, which is the right durable home — it
is what the sibling plans cite. Note that it lists the founding bug as row #1,
which this section declines to merge for the reason above; if you are reading
that table, apply the membership test it closes with (*are you deriving a source
range from a parent's range plus an offset?*) and note that row #1 does not
satisfy it. Same family, different fix shape.

The range-arithmetic idea is attractive because it is *almost* right: offsets
compose affinely over a piecewise parent, so every arithmetic check passes. What does not compose
is **byte-identity**, and no length or contiguity test can see the difference —
which is why #3 and #4 both survived review by people holding the counterexample.
The rule, stated once for all of them: **a range is only composable over a
parent that is byte-identical to its content, and no accessor on `SourceInfo`
can tell you whether it is.** If you find yourself deriving a source range from
a parent's range plus an offset, and the parent might be a `Concat`, the answer
is `None`.

**Cost of `None` that is real.** `postprocess.rs:659-661` calls `preimage_in` on
*element* source_infos and falls back to `combine()` on `None`, and the module
doc at `:1845-1852` records a historical bug where a `combine`-produced `Concat`
with `preimage_in() == None` "corrupts provenance (wire-format /
substring-invariant violations, attribution drops) and, on the
incremental-writer path, drove crashes / lossy fallbacks". That bug was a
doubled-self `Concat` from `combine(self, self)`, not any `combine` result — but
the direction of concern is legitimate, and it is the strongest argument against
this fix. Weighed against admitting a byte-substitution hull, `None` still wins:
a lossy rewrite is recoverable, wrong bytes in the output are not.

### How the pieces are derived: lockstep against yaml-rust2's value (decided)

`quarto-yaml` does **not** re-implement YAML scalar decoding. It walks
yaml-rust2's already-decoded value against the raw source, taking
*segmentation* from the grammar and *content lengths* from the value. The
decoded string is the oracle, not the output.

Piece-building needs two separable things: **segmentation** (where does a
transformation start, and how many source bytes does it consume) and
**evaluation** (what content bytes does it produce). Only segmentation is
needed. Reading evaluation off the value means the piece list tiles exactly
the string the consumer holds, by construction.

Four rules, **evaluated in this order**. The order is load-bearing and
counter-intuitive — verbatim is tried only *after* break and escape:

1. **Break region** — entry is **style-conditional** (corrected 2026-08-21
   during Phase 2 finishing — see below; superseded the single rule that
   follows this parenthetical for its first day). For **block** styles
   (`|`, `>`), the source cursor must be exactly at `\n`/`\r`, as originally
   stated. For **flow** styles (plain, single- and double-quoted), the source
   cursor must be at a **whitespace run that contains a newline** — which may
   start one or more bytes before the `\n` itself. Either way the value must
   also be at a space/newline/tab. Once entry fires, absorb the maximal source
   whitespace run (from wherever it started) and the value's run of
   **space-or-newline** — note the asymmetry, which is deliberate and matches
   the committed walker (`walker.rs:39` admits a tab as the *entry* condition,
   `:54` does not advance over one). A value tab at a break therefore yields a
   zero-content piece and re-enters the loop. Do not "tidy" the two byte sets
   into one; no measured shape distinguishes them, so nothing would catch it.
   Emit **one** piece. Two caps apply: the value run is capped at the number of
   newlines in the source run (`ve.min(vi + source_newlines.max(1))`), and for
   block styles the source run stops `indent` bytes after its last `\n`. Both
   caps exist to stop the walker eating a more-indented line's content-leading
   spaces — one cap per side of the correspondence.

   **Why the entry test is style-conditional.** YAML strips a plain or quoted
   scalar's trailing line whitespace *before* folding the break — those bytes
   belong to the break region and have no content byte of their own — but a
   block **literal** (`|`) keeps a line's trailing spaces as **content**,
   which is why the narrow entry (exactly at `\n`/`\r`) was already correct
   for every block fixture: rule 3 (verbatim) legitimately consumes those
   trailing spaces one byte at a time, and the real break is reached only at
   the line's true end, where it is a byte-identical newline run. A flow
   scalar has no such consumer for its trailing whitespace: under the narrow
   entry, rule 3 would consume a trailing space that belongs to the fold one
   iteration *before* rule 1 could recognize the fold was starting, stranding
   the walk on trivially valid YAML. Measured: `key: a `⏎`  b` (decoded
   `"a b"`) desyncs under the narrow entry, because rule 3 matches the
   trailing space (`' '==' '`) on the line before rule 1 ever sees the `\n`;
   under `strict-provenance` this is a CI panic, found on the feature's first
   CI run rather than by a fixture (see § Evidence, Phase 2 — the "no measured
   shape desyncs" claim below was true of the 32 shapes measured through
   Phase 1, and is incomplete for that reason, not wrong). This is not a new
   concept: rule 1 was **already** style-parameterized through `indent` (block
   passes `marker.col()`, flow passes `0`, because flow folding strips all
   line-leading whitespace). The entry test's style-conditionality is that
   same fact's **trailing-edge** counterpart — applied where the walk *enters*
   the region rather than where it stops absorbing it — implemented as a
   `wide_entry: bool` parameter threaded alongside `indent`, computed as
   `!block`.

   **Folded block scalars (`>`) do not need the wide entry — probed, not
   fixtured.** `k: >`⏎`  a `⏎`  b`⏎ derives correctly under the **narrow**
   entry: `>` folding replaces only the line-break byte itself with a folded
   separator and, unlike flow folding, does not strip a line's trailing
   whitespace — the trailing space is ordinary preserved content, the same
   shape as a block literal's trailing spaces, so the narrow entry already
   handles it. Recorded as a probe in the fixtures note, not added as a
   permanent fixture row or generator case, per instruction — nothing
   currently needs it.

   **Tag the piece `verbatim` iff the source run is byte-identical to the value
   run; otherwise `replacement`.** Never decide this by length. A fold whose
   source run is exactly `\n` and whose content is one space is 1→1 with
   *different bytes*; tagging it verbatim produces a piece claiming a
   byte-identical source range it does not have, which `preimage_in` — the
   writer's "can I Verbatim-copy these bytes?" check — would honour, emitting a
   newline where the content has a space. That is this epic's wrong-bytes
   failure, reintroduced by the fix for it. Reachable in the simplest possible
   document: `aaa`⏎`bbb` as a root-level plain scalar, measured in the fixtures
   note. Block-scalar line endings *are* byte-identical newline runs, so they
   legitimately tag verbatim and coalesce — which is why every shape measured
   before this rule existed happened to come out right, and why no fixture row
   caught it.
2. **Escape** — the source is at `\` (double-quoted) or at a `'` whose
   successor is also `'` (single-quoted): consume per the escape-length table
   and emit a replacement.
3. **Verbatim** — value byte equals source byte: extend the run.
4. **Synthesis** — the value has bytes left and the source is exhausted: if
   what remains is all newlines, emit one zero-width piece; otherwise desync.

Anything else is a **desync**, reported with both cursor offsets.

**Why verbatim must be last, measured.** Reordering the prototype to put
verbatim first — the intuitive reading — desyncs **9 of the 24 shapes then in
the set**, including the plainest case in it. Both later precedences are load-bearing for the same reason: the
bytes are *equal*, so verbatim consumes them 1:1 and strands the walker. A
source `\n` matching a value `\n` must be taken as a break region or the
following line's indent is never consumed (`k: |`⏎`  aaa`⏎`  bbb` desyncs at
the first indent byte). A source `'` matching a value `'` must be taken as an
escape or the doubled quote splits (`'it''s'` desyncs on the second `'`).

**Indent per style.** Block styles pass `indent = marker.col()`. Flow styles —
plain, single- and double-quoted — pass **0**, because folding in a flow scalar
strips all line-leading whitespace, so there is no indent to preserve and no
rewind to perform.

**Where the walk starts** (`raw`'s first byte):

| style | start | why |
|---|---|---|
| block, normal | span start | the marker is already on the first content byte |
| block, **empty body** | the newline ending the header line | the marker is on the `\|`/`>` **header**; see the predicate below |
| plain | span start | — |
| quoted | span start **+ 1** | skip the opening delimiter |

The empty-body row is the **header-skip rule**, and it is what makes `k: |`
work: its span is 3..4 (the `|`) while its decoded value is `"\n"`, so a walk
from the span start compares `|` against `\n` and desyncs on the first byte.
Starting after the header yields the correct single verbatim piece
`0..1`←`4..5`. **No span changes** — the node's `source_info` still covers the
header, which is the right thing for a diagnostic to underline.

**The predicate must not be a bare byte test.** `matches!(src[span_start], b'|' | b'>')`
alone is unsound: for a
block scalar whose *content* starts with a pipe the marker points at that
content byte, so the rule fires falsely and the walk desyncs — on valid YAML,
which under `strict-provenance` is a CI panic. Measured, both of these desynced
under the byte-only predicate:

```yaml
k: |          k: |
  |pipe         |
```

The predicate is that byte test **and** the decoded value being empty or
consisting only of newlines — an empty body is the only case where the marker
can sit on the header. Measured with that predicate: both shapes above derive
correctly (`0..6`←`7..13` and `0..2`←`7..9`), `k: |` still works, and all 32
shapes pass with zero desyncs. A line-based discriminator would also work —
yaml-rust2's `Marker` exposes `line()`, though `quarto-yaml` uses only `index()`
and `col()` today — but it needs no new API to compare against the value.

Why this and not a re-implementation:

- **Chomping stops being a concept.** The value already carries however many
  trailing newlines it has. `|`, `|-`, `|+` need no code, no indicator
  parsing, no knowledge that `block_scalar_len` excludes trailing blank lines.
- **Folding stops being a concept.** `>` semantics — fold-to-space, blank-line
  runs, more-indented lines not folded — are the nastiest corner of the spec
  and the most likely to diverge from yaml-rust2 on any future upgrade. The
  break-region rule infers all of it from the value.
- **`|2` needs nothing.** `marker.col()` is whatever indent applies to the
  scalar's first content line — auto-detected for a bare `|`, declared for
  `|2` — and either way it is the number of bytes to strip per line. Verified:
  `k: |2`⏎`    aaa` has its span start at column 2 and the value keeps two
  leading spaces. It is **not** literally "the declared indent": for `k: |`
  with an empty body `marker.col()` is 3, the column of the `|` itself, which
  is what the header-skip rule detects.
- **Failure is loud.** A re-implementation that diverges tiles a string nobody
  holds — silent misattribution, this epic's bug class. A desync names the
  byte. This is already the idiom in the file (`plain_scalar_len` ends with
  `if !tail.starts_with(ch) { return value.len(); }`).

Segmentation still needs three grammar facts, so this is not grammar-free: the
escape-length table (`\t`→2:1, `\uNNNN`→6:n, `''`→2:1, `\`+break→n:0), the
block indent, and "a break run is a source whitespace run containing a
newline."

**Every measured shape passes** with one ~90-line walker. The count and the
per-shape piece lists live in
`claude-notes/research/2026-08-21-yaml-content-provenance-fixtures.md`, which is
the authority; this plan does not repeat the number, because it was stale in six
places at one point. Two properties are checked per shape: the pieces tile the
value exactly, and every verbatim piece's source text equals the corresponding
value slice.

**The precision cost, which must be in the contract.** A break region is one
piece, so a content offset landing *inside* a multi-byte break maps to the
region's start rather than to its own newline. Every non-whitespace content
byte is exact. That is the right trade for carets, but it must be stated
rather than promised away.

**The walk is bounded by the value, not by the span.** It reads source past
`source_info.end_offset()` when the value asks for it. This is what turns the
three span-overflow shapes above into ordinary verbatim pieces —
`|+`'s kept newlines collapse to a single verbatim piece over `"aaa\n\n\n"`,
and the trailing spaces map to `bbb   \n` at their true offsets. The natural
implementation (slice the span, walk the slice) fails on all three.

**Desync policy (decided 2026-08-21, revised 2026-08-21, and once more during
Phase 2 finishing).** Desync is a bug, not a data condition — with the four
rules, the byte-identity tag, the header-skip rule *and its value-based
predicate*, and the **style-conditional break-region entry** (rule 1, above —
added after `strict-provenance`'s first CI run found a real desync) in place,
the walker does not desync on any of the **43** measured shapes. (Every clause
there is load-bearing, and each was established by a shape that desynced
without it: `k: |` without the header-skip rule, a block scalar whose content
starts with `|` without the value-based predicate, and `key: a `⏎`  b` without
the style-conditional entry. All three are trivially valid YAML, and under
`strict-provenance` a desync is a CI panic — so these are prerequisites of the
claim, not optimizations. The claim is "no measured shape desyncs", not a
proof over all YAML — and it was already true-but-incomplete once, for exactly
the reason it could be incomplete again: none of the 32 shapes measured
through Phase 1 happened to contain a trailing space before a fold, so their
absence of a desync was not evidence against one. `strict-provenance`'s CI
step is what actually found this one, on its first run, by parsing every
scalar in the existing test suite rather than only the dedicated provenance
fixtures — exactly the load-bearing check it exists to be, not merely a length
tripwire.)

- Under `strict-provenance`: **panic**, naming both cursor offsets.
- Otherwise: `content_source_info()` returns **`None`**.

**There is no `exact` flag.** A previous revision returned the node's
contiguous span plus a companion `content_provenance_is_exact()`. That is
withdrawn, for three reasons:

1. The premise was wrong. The flag existed so `None` would not have to carry
   two meanings — but "not a scalar" already has a dedicated public channel,
   `is_scalar()` (`yaml_with_source_info.rs:158`). A consumer needing the
   distinction asks the node its shape, which is clearer than inferring shape
   from a provenance accessor.
2. Nothing would branch on it. `quarto-yaml` has no diagnostic channel to
   report it through, and every consumer must do the same thing in both cases:
   **decline the sub-offset arithmetic**. That is already the established
   pattern — `use_cmd/config.rs:229` returns `None` rather than assume
   alignment. A public accessor whose documented contract is "treat this exactly
   as you treat the other failure" is not information.
   Note what this does **not** say: it does not say the desync case falls back
   to `source_info`. § Hand-off to Plan 2 permits that fallback *only* for
   non-YAML metadata, whose `Generated` provenance makes offset arithmetic yield
   `None` anyway. A desynced scalar is YAML-rooted with a real `Original` span,
   so falling back to it would resurrect exactly the drift this epic fixes.
3. It shipped the bug class as a documented mode: provenance present,
   correctly typed, and silently the old wrong base. That is what this epic
   exists to remove.

**What this costs, and the compensation.** `Option` inside the variant gives up
"the compiler enforces that no scalar can lack it" — already conceded when the
design went additive. Be honest about what is left: `Children` is private and
only `new_scalar` constructs it, so **no call site names the field**, and
forgetting `.with_content_provenance(…)` at some future production site is
exactly forgetful and invisible. The `strict-provenance` assert catches
"derivation ran but produced `None`"; it cannot catch "derivation never ran".
What does catch that is Phase 2's test matrix: it asserts `Some` for every
parsed shape, so a forgotten attach in the parser fails the suite. That is the
real backstop, and it is why those tests are not optional.

**The length invariant is unconditional**: if `Some(si)`, then
`si.length() == decoded.len()`.

**Not affected: zero-piece scalars stay `Some`.** `k:`, `k: ''` and an empty
block scalar followed by another key all *derive successfully* to a zero-length
`SourceInfo` at the anchor. "Empty but derived" is `Some`; only "could not
derive" is `None`. The test table in Phase 2 must keep that distinction
explicit — it is the one place the merged `None` could be misread.

### The shared builder

Lives in `quarto-source-map` so all decoders share the run-tiling machinery.
The *escape rules* are decoder-specific and stay with each decoder.

```rust
// `anchor` is the scalar's span start; it is what `finish()` uses when the
// piece list is empty. See "Both constructors take an anchor offset" below.
let mut p = ProvenanceBuilder::in_parent(parent_source_info, anchor); // Substring pieces
let mut p = ProvenanceBuilder::in_file(file_id, anchor);              // Original pieces
p.verbatim(src_range);              // n source bytes -> n content bytes
p.replacement(src_range, out_len);  // n source bytes -> out_len content bytes
                                    // out_len == 0 is a deletion
                                    // an EMPTY src_range with out_len > 0 is
                                    // synthesis: content with no source byte
let content_si = p.finish();        // -> Original/Substring if contiguous,
                                    //    Concat otherwise
```

**Two constructors are required.** `quarto-yaml`'s substring path has a parent
(`make_source_info` uses `SourceInfo::substring`, `parser.rs:344-361`), but its
original-file path has none — it emits `Original{FileId(0), s, e}` directly.
Without `in_file`, the crate would have to synthesise a whole-file
`Original{0, len}` parent and wrap every piece in a `Substring`, changing the
emitted shape for cases that are contiguous today.

**Both constructors take an anchor offset**, because `finish()` must be
total: an empty scalar produces **zero pieces** (three of the shapes in
§ `quarto-yaml`'s API), and with no pieces there is nothing to infer a position
from. `finish()` on an empty piece list returns a zero-length `Original` /
`Substring` at the anchor. Signature therefore
`in_file(file_id, anchor)` / `in_parent(parent, anchor)`, with the anchor being
the scalar's span start.

**The builder must never resolve absolute positions.** `in_parent`'s parent can
itself be a `Concat`: cell options builds `SourceInfo::concat(concat_pieces)`
and hands it to `quarto_yaml::parse_with_parent`
(`quarto-core/src/cell_options/mod.rs:227-229`). `substring()` composes over a
`Concat` parent correctly for `map_offset`, but `resolve_byte_range()` on it
returns `None` (`source_info.rs:403`). So the builder — including `finish()`'s
contiguous-collapse decision — must **never call `resolve_byte_range`**, on the
parent or on anything derived from it. (`in_file`'s ranges are absolute file
offsets and that is fine; the invariant is about resolving, not about which
coordinate space the caller works in.)
This is the one parent shape that is not an `Original`, and it is the case the
epic calls exemplary, so it is worth stating as a contract rather than
discovering.

**Coalescing is a builder contract, not a prototype detail.** The walker calls
`verbatim` one byte at a time, so the builder must merge adjacent **verbatim**
pieces whose source ranges abut — and only those. A `replacement` never
coalesces, however convenient its length. "Verbatim" means byte-identical
source and content, which is the caller's assertion when it picks that method
(see § How the pieces are derived, rule 1); the builder takes it on trust and
must not re-derive it from lengths. Without coalescing, Phase 1's own test —
"all verbatim must produce a contiguous `SourceInfo`, not a 1-piece `Concat`" —
cannot pass, and every plain scalar becomes an N-piece `Concat`.

**`finish()` collapses iff there is exactly one piece and it is `verbatim`.**
Nothing weaker is sound, and two weaker readings were tried and rejected:

- *"the pieces tile one source range"* — then `k: ''''` (value `'`, a single
  2→1 replacement) collapses to `Original{4,6}`, whose `length()` is 2,
  violating the unconditional invariant `si.length() == decoded.len()` and
  firing the `strict-provenance` assert on a trivially valid scalar.
- *"a single contiguous source range **and** equal totals"* — which this section
  said until 2026-08-21, and which is **the length-preserving refinement that
  § `preimage_in` composes affinely rejects, one layer down.** The fold shape
  (`root plain, col-0 continuation`: verbatim 0..3, a 1→1 **replacement** 3..4,
  verbatim 4..7) has 7 content bytes over a contiguous 7-byte source range, so
  it satisfies both clauses and collapses to `Original{0,7}` — whose
  `preimage_in` then licenses Verbatim-copying `"aaa\nbbb"` for content
  `"aaa bbb"`. And nothing catches it: `length()` is 7 and `decoded.len()` is 7,
  so the invariant passes and the assert stays quiet.

The rule is exact in both directions — **one verbatim piece ⇒ safe to collapse**,
and anything else ⇒ do not. After coalescing, two or more pieces implies either a
replacement or a source gap, and neither may collapse. Checked against the fixtures — `single-quoted` collapses
to `Original{4,9}`, `block |+ keep` to `Original{8,14}`, while
`single-quoted all-escape` stays a 1-piece `Concat` of length 1 and
`double-quoted escaped break` stays a `Concat`.

**The builder must therefore keep the verbatim tag internally**, even though it
does not survive into the emitted `SourceInfo`. Note the consequence for tests:
because the tag is invisible externally and a 1→1 fold satisfies
`piece.length == piece.source_info.length()`, **piece count is the only
observable** that distinguishes a collapsed run from a fold. Phase 1 carries the
fold shape as a builder test for exactly that reason — it must not wait for
Phase 2.

**Zero-content pieces are STORED, not dropped.** `Concat::map_offset`'s
containing-piece test is `offset >= piece_start && offset < piece_end`, so a
zero-content piece is unreachable through it and cannot affect the mapping —
storing one is measured-harmless. The reason to keep it is the **source-tiling
invariant**: a piece list that tiles its source contiguously is a testable
property, and dropping a deletion breaks it by leaving a gap exactly where the
deleted bytes were. `preimage_in`'s hull is the visible symptom of that gap
rather than the reason to care — that hull is an *offset* claim, and
§ `preimage_in` composes affinely makes it `None` for the
`Substring{parent: Concat}` shape these values actually take. Measured on the
escaped-break shape (`verbatim 4..7`, `deleted 7..11`, `verbatim 11..14`):

| | `preimage_in` | `map_offset` for content 0..6 |
|---|---|---|
| dropped | **`None`** — hull lost | 4,5,6,11,12,13,14 |
| stored | `Some(4..14)` | 4,5,6,11,12,13,14 — **identical** |

So storing costs nothing and buys the hull. The earlier rationale — "storing one
would route `map_offset(length())` into the *deleted* source's start" — was true
only against the **unfixed** exclusive-end branch; the fix above (use the last
piece's *source* length) resolves it to that range's end instead, which is
right. A decision justified by a defect, made before that defect was fixed.

**Therefore: the piece list tiles its source contiguously — an invariant, with
a test.** Every rule preserves it: the break-region rule absorbs the indent into
a replacement rather than dropping it, escapes are consecutive, synthesis is
zero-width at the end, and deletions are now stored. Note that § The shared
builder permits the operation that would break it (`out_len == 0` is legal), so
this needs asserting rather than assuming: one standalone dropped piece silently
turns every hull consumer into `None`. **Assert it builder-side** — a
`debug_assert` in `finish()` that the pieces tile their source contiguously —
*and* test it next to the `length()` test. Builder-side because three decoders
drive this and only one of them is in this plan.

**Synthesis must be legal.** Exactly one measured shape needs it: a
clip-chomped block scalar at EOF with no final newline, where the content's
trailing `\n` has no source byte. `replacement(eof..eof, 1)` expresses it —
a piece whose `source_info` is `Original{eof, eof}` with `length: 1` maps
content offset 0 to `eof`, and `offset_to_location` accepts
`offset == total_length`. Note this is *not* what chomping needs; chomping
needs nothing.

Shape check across the three decoders that must drive it:

| Decoder | verbatim | replacement | `out_len == 0` | synthesis |
|---|---|---|---|---|
| YAML (`quarto-yaml`) | yes | `''`→`'`, `\t`→TAB, folds | `\`+break→∅ (arrives via the escape rule) | clip newline at EOF |
| Div attributes (pampa) | yes | `\*`→`*` | — | — |
| CommonMark (comrak) | yes | `\*`→`*`, `&amp;`→`&` | — | — |

Note what this table corrects: **standalone deletion has no consumer.** YAML's
block-scalar indentation is always adjacent to a break, so it merges into the
break replacement; the escaped break in a double-quoted scalar is the only
genuine `out_len == 0` case, and it is also a break region. `out_len == 0`
must be *legal*; it does not need to be a named operation.

One note on the pampa row, decided in Plan 2: `AttrSourceInfo.attributes[i].1`
**changes meaning** to content provenance rather than gaining a sibling field,
because every production consumer wants content — `callout.rs:427` (re-parse
base), `theorem.rs:345` and `proof.rs:182` (a span for a `Str` whose text is
already decoded), and `llms.rs:374-379`/`:1082` only filters the vec in
parallel with `attr.2`. No *Rust* consumer needs the raw quote-inclusive span.
So the attribute path's output is a **public-semantics change**, not an additive
one — the only row in this table for which that is true.

**And the change crosses a language boundary.** That survey was Rust-only.
`AttrSourceInfo` derives `Serialize`/`Deserialize`, the JSON writer emits it as
`"a"` (`pampa/src/writers/json.rs:694-708`), and TypeScript reads the value
spans: `ts-packages/annotated-qmd/src/block-converter.ts:287` and
`inline-converter.ts:322` pull `attrSource.kvs[i]` and resolve them through a
`sourceReconstructor`. So quoted-value offsets shift by one for that consumer,
and a `Concat`-backed span may not resolve to a single range at all. Decided
2026-08-21 to keep the meaning change and give the TS side to Plan 2
(§ Hand-off to Plan 2, obligation 8) rather than widen `AttrSourceInfo`.

**Which side of the oracle boundary each decoder sits on** is the real
question for Phase 1's design review, and the answer differs. pampa's
attribute path *owns* `unescape_punctuation`, so it can emit pieces as it
decodes. comrak hands us decoded text with raw sourcepos and no access to its
escape handling, so it is *forced* into the lockstep form. The builder's
`verbatim`/`replacement` API is agnostic to that choice — which is the
strongest argument for putting it in `quarto-source-map`.

#### Design-review walkthroughs (Phase 1)

**1. A YAML break region.** `plain multi-line` (`k: aaa\n  bbb\n  ccc\n`, span
3..18, decoded `"aaa bbb ccc"`) has five pieces total, two of them
break-region replacements (the other three are verbatim literal runs); take
the first break-region piece, `3..4<-6..9*`. The walker enters rule 1 because
the source is at `\n` (offset
6) and the value is at a space (offset 3 in `"aaa bbb ccc"`); it absorbs the
source run `\n  ` (offsets 6..9, one newline plus the next line's two-byte
indent) against the value's run, capped at `nl.max(1) = 1` byte of content
(the fold's single space). Through the proposed API this is one call,
`p.replacement(6..9, 1)`, chosen over `verbatim` because the byte-identity
test fails outright (3 source bytes cannot equal 1 content byte) — length
alone would already settle it here, but the rule is byte-identity regardless,
per § How the pieces are derived. `finish()` on the scalar's full piece list
(`0..3<-3..6`, `3..4<-6..9*`, `4..7<-9..12`, `7..8<-12..15*`, `8..11<-15..18`)
returns a 5-piece `Concat`: two of the five pieces are replacements, so the
one-verbatim-piece collapse rule never engages. I did not find a measured
shape exercising the rule's other entry path — a source break where the
*value* is at a tab rather than a space or newline (rule 1's text: "a value
tab at a break therefore yields a zero-content piece and re-enters the
loop") — none of the 32 fixtures has a value-side tab at a break position;
`block | tab in content`'s tab is mid-content, not at a break, and produces
an ordinary verbatim piece. That path is asserted by the derivation rule's
prose, not demonstrated by a fixture row.

**2. A `\t` escape in a double-quoted scalar.** `double-quoted \t`
(`k: "a\tb"\n`, span 3..9, decoded `"a\tb"`) walks as verbatim `a`, an
escape, verbatim `b`: `0..1<-4..5`, `1..2<-5..7*`, `2..3<-7..8`. The calls are
`p.verbatim(4..5)`, `p.replacement(5..7, 1)`, `p.verbatim(7..8)`. Neither
verbatim call coalesces with the replacement between them (coalescing is
verbatim-adjacent-to-verbatim only), and the two verbatim pieces are not
adjacent to each other either — so `finish()` returns a 3-piece `Concat`.
This is the fold shape by another name: `crates/pampa`'s escape table differs
from YAML's (2:1 for `\t` vs. `\`+punctuation for pampa) but the call
sequence — verbatim, replacement, verbatim — and the resulting uncollapsed
3-piece `Concat` are identical in shape to what Phase 1's frozen "fold shape"
builder test exercises, which is why that one test stands in for both.

**3. A `''` escape in a single-quoted scalar.** `single-quoted with ''`
(`k: 'it''s'\n`, span 3..10, decoded `"it's"`) produces `0..2<-4..6`,
`2..3<-6..8*`, `3..4<-8..9`: calls `p.verbatim(4..6)`, `p.replacement(6..8,
1)`, `p.verbatim(8..9)`. Same shape as case 2 — verbatim/replacement/verbatim,
3-piece `Concat` from `finish()`, no collapse — driven by a different escape
rule (rule 2's single-quote branch: a `'` whose successor is also `'`, 2
source bytes to 1 content byte) than case 2's double-quote branch, which is
the point: the API does not need to know *which* escape rule produced the
replacement, only its source range and output length.

**4. The EOF-synthesis case.** `block | no final newline` (`k: |\n  aaa`,
span 7..10, decoded `"aaa\n"`) produces `0..3<-7..10`, `3..4<-10..10*`: calls
`p.verbatim(7..10)`, then `p.replacement(10..10, 1)` — an empty source range
at the source's end, with `out_len = 1` for the synthesized trailing newline
that has no source byte, per § The shared builder's synthesis note
(`replacement(eof..eof, 1)`). `finish()` returns a 2-piece `Concat`
(verbatim, then the synthesis replacement); it cannot collapse, both because
there are two pieces and because the second is not verbatim. No new
operation is needed — synthesis is expressed entirely by feeding
`replacement` a zero-width range, which the API sketch already documents as
legal.

**5. The zero-piece empty-scalar case.** `empty value` (`k:\n`, span 3..3,
decoded `""`) and `empty single-quoted` (`k: ''\n`, span 3..5, decoded `""`)
both walk to zero pieces — the `while vi < vb.len()` loop in the prototype
never executes because `vb.len() == 0`. No `verbatim`/`replacement` calls are
made at all; the builder is constructed (`ProvenanceBuilder::in_parent(...,
anchor)` or `in_file(...)`, with `anchor` set to the scalar's span start —
offset 3 for both fixtures) and `finish()` is called on an empty piece list.
`finish()` must still return *something* — the fixtures note is explicit that
these derive to `Some`, not `None` — and with no pieces there is no source
range to infer a position from, which is exactly why both constructors take
an anchor argument distinct from any piece: `finish()` on zero pieces returns
a zero-length `Original`/`Substring` positioned at that anchor rather than
failing or requiring a piece that doesn't exist. The third zero-piece shape
in the fixtures note, `empty block scalar, next key follows`, is the same
call shape (zero pieces, anchor-only `finish()`) with a span that happens to
point at the next key — harmless here because there is no content to
misplace, and out of scope per the fixtures note.

**6. A `\*` attribute escape on pampa's div-attribute path.** Reading
`unescape_punctuation` (`crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs:41-59`):
it iterates `chars()` and, on backslash followed by an ASCII-punctuation
character, pushes only the punctuation byte — the same 2-source-bytes,
1-content-byte shape as YAML's escape rule. Because pampa owns this loop
character-by-character rather than re-deriving it against an oracle, it
never needs synthesis or `out_len == 0`: a bare trailing backslash pushes
itself verbatim (1:1, byte-identical), and a backslash followed by a
non-punctuation character pushes both bytes verbatim (2:2, byte-identical,
so it would coalesce). For `x\*y`, the call sequence a builder-driven
rewrite would make is `p.verbatim(k..k+1)` (`x`), `p.replacement(k+1..k+3,
1)` (`\*`→`*`), `p.verbatim(k+3..k+4)` (`y`) — the identical
verbatim/replacement/verbatim, 3-piece, non-collapsing shape as cases 2 and
3, for offsets `k` relative to wherever the interior (quote-stripped) string
starts. What the API cannot paper over is that `unescape_punctuation` is
private with one caller, `extract_quoted_text` (`:28-36` in the same file),
which **returns a bare `String` and discards offsets entirely** — confirmed
by reading both functions — and `extract_quoted_text`'s own caller,
`key_value_value` at `crates/pampa/src/pandoc/treesitter.rs:1208-1212`,
currently takes `node_location(node)` (the whole quote-inclusive node span)
as the value's location, unadjusted for unescaping, which the
`key_value_key`/`key_value_value` match arms inside `key_value_specifier`
(the case itself starts at `:1214`; the arms are `:1223-1234`) then store
verbatim as `value_range`.
Threading a real piece list out therefore means changing the return type of
two private functions and the shape of `value_range`, and downstream,
`AttrSourceInfo.attributes[i].1`'s meaning change (already decided in this
section) and the TypeScript consumers named above. That is a caller-side
plumbing cost, not a builder deficiency: distinguishing it from an API
finding per the task brief's own test, `verbatim`/`replacement`/`finish`
already say everything `unescape_punctuation`'s loop needs to say. I did not
find, in either function, a case needing an operation absent from the
sketch.

**7. An `&amp;` entity on the comrak path.** Tracing comrak 0.52.0 (the
version pinned in `crates/comrak-to-pandoc/Cargo.toml:11`, checked against
the vendored source under `~/.cargo/registry/src/.../comrak-0.52.0/src/parser/inlines.rs:493-511`
and `src/parser/mod.rs:2396-2526`, not executed): `handle_entity` does create
its `NodeValue::Text` node with a sourcepos spanning the *entire* raw escape
(`self.scanner.pos - 1 - len .. self.scanner.pos - 1`, comrak's inclusive
convention) and content equal to the decoded entity — for `&amp;` alone,
sourcepos 5 bytes, content `"&"` 1 byte — but that atomic node is not what
`convert_inline` receives whenever the entity has adjacent literal text,
which is the ordinary case in prose (`x&amp;y`, `Ben &amp; Jerry's`), not a
corner case. `postprocess_text_nodes`'s `NodeValue::Text` arm
(`mod.rs:2413-2419`, inside the range cited above) calls
`postprocess_text_node_with_context` (`mod.rs:2491-2526`, not previously
cited): it walks `node.next_sibling()` in a loop and, for every adjacent
`NodeValue::Text` sibling, appends its content into `root` and extends
`sourcepos.end` to that sibling's end — unconditionally, with no option
gate (`escaped_char_spans` only gates the separate `Escaped`-node merge
discussed below). So for `x&amp;y` (source bytes `x`@0, `&amp;`@1..6, `y`@6,
7 bytes total), parsing first produces three sibling Text nodes — `"x"`
(0..1), `"&"` decoded from the entity (1..6), `"y"` (6..7) — and this merge
collapses them into **one** node, content `"x&y"` (3 bytes), sourcepos 0..7,
*before* `comrak-to-pandoc` ever sees the tree. That merged node — mixed
verbatim and replaced bytes within one node — is what the required case
("an `&amp;` entity," not "an isolated one") actually looks like; the
atomic single-escape node is the exception, reachable only when an entity
has no adjacent `Text` sibling (e.g. it is the sole content between two
non-`Text` nodes). Through the proposed API, the merged case's call
sequence is `p.verbatim(base+0..base+1)` (`x`), `p.replacement(base+1..base+6,
1)` (`&amp;`→`&`), `p.verbatim(base+6..base+7)` (`y`) — the same
verbatim/replacement/verbatim shape as cases 2, 3, and 6, so `finish()`
returns a 3-piece `Concat`, uncollapsed. Tracing `tokenize_text_with_source`
(`crates/comrak-to-pandoc/src/text.rs:91-157`) against the merged `"x&y"`
node itself: every char is non-whitespace, so the per-char loop (`:98-131`)
sets `current_word_start` once, at byte 0, and `'y'`'s own `abs_offset`
(computed at `:99`) is never read anywhere — the guard at `:125-127` only
assigns a word-start when none is already open. The node's single `Str`
instead comes from the trailing-word branch (`:136-141`), whose end is
`end_offset = base_offset + text.len()` (`:134`) — `0+3=3` — a
word-*length* miscalculation against the true consumed source length `7`,
not a per-character error at `'y'`. A fuller example exposes the mechanism
this paragraph's first draft wrongly attributed to `'y'` directly: for
`x&amp;y z` (9 source bytes; the same entity-merge mechanism above collapses
`"x"`, `"&"`, and `"y z"` into one node, content `"x&y z"` — 5 bytes,
sourcepos 0..9), the space at decoded byte 3 *does* reach the
whitespace-triggered branch (`:101-115`), whose end is `abs_offset` at
`:105` — `base_offset + 3 = 3`, again the decoded byte index used as a
source offset, not the true `7`. That wrong boundary is then carried
forward as the next word's start (`:114`, `:125-127`), so
`tokenize_text_with_source` emits `Str("x&y", 0..3)`, `Space(3..4)`,
`Str("z", 4..5)` against the true `0..7`, `7..8`, `8..9` — a uniform 4-byte
short-shift on every token after the entity, not only on the merged node's
own end. So there are two distinct proximate mechanisms, not one: a
word-length miscalculation at `:134`/`:136-141` when nothing follows the
merged run, and a decoded-index-as-source-offset conflation at
`:99`/`:105`/`:114` that then propagates additively once something does. This is a
real, traced-not-executed bug in the current code, independent of this
epic. Fixing it needs a genuine byte-cursor walk over the merged node's raw
source against its decoded text — the same verbatim/escape-mismatch shape
the YAML walker uses, with comrak's own escape/entity table
(`entity::unescape` for `&...;`, an ASCII-punctuation check for `\`) — not
just the one-classification-per-node shortcut this paragraph's first draft
assumed. `postprocess_text_node_with_context`'s own `spxv` (the per-original-node
sourcepos/length list it builds while merging, `mod.rs:2502-2515`) is
consumed internally for comrak's own line-processing and is not exposed to
`comrak-to-pandoc`, so that per-piece boundary information is gone by the
time `convert_inline` runs; nothing short of re-deriving it against the raw
source will recover it. Separately, and unrelated to entities:
`postprocess_text_nodes`'s `coalesce_escaped` flag (which gates a *different*
merge, of `NodeValue::Escaped` backslash-escapes into a neighbor) defaults to
`true` — q2 sets neither `parse.escaped_char_spans` nor
`render.escaped_char_spans` (confirmed by grep, no hits in
`crates/comrak-to-pandoc/`) — so under this crate's actual config, a
backslash-escape is *also* merged into an adjacent literal-run Text node
(traced from `mod.rs:2447-2479`), producing the identical interior-mismatch
shape by a second, independent mechanism. No q2 test currently exercises
either merge, so beyond the trace this is unverified. The `NodeValue::Escaped`
match arm at `crates/comrak-to-pandoc/src/inline.rs:96-100` is consequently
unreachable under today's default options — the merge already consumes the
node before `convert_inline` ever sees an `Escaped` variant — which is worth
recording because that arm's comment ("the actual character is in the
children as Text") reads as though the merge does not happen.

**API verdict: no decoder needs an operation the others do not.** All seven
walkthroughs above reduce to the same three primitives — `verbatim`,
`replacement` (including its `out_len == 0` and empty-source-range/synthesis
special cases), and `finish`'s one-verbatim-piece collapse rule — and the
two constructors already cover both shapes a decoder can be rooted in
(`in_file` for pampa's and comrak's file-rooted spans, `in_parent` for
YAML's substring path). The three decoders differ only in *which side of the
oracle boundary* they sit on, which is a decoder-design question, not a
builder-API one:

- **pampa** owns its decode loop and can emit pieces inline, but the finding
  in case 6 is entirely a plumbing cost — two private functions
  (`unescape_punctuation`, `extract_quoted_text`) discard offsets and return
  a bare `String`, and threading a piece list out crosses `value_range`,
  `AttrSourceInfo.attributes[i].1`'s already-decided meaning change, and a
  TypeScript consumer. Per the task brief's own distinction, this is caller
  awkwardness, not an API insufficiency, and I recommend no API change for
  it.
- **comrak** is forced into the lockstep form for the required case itself,
  not just for an aside next to it. Corrected from an earlier draft of this
  paragraph: case 7's entity is not, in general, an atomic single-node
  replacement — `postprocess_text_node_with_context` unconditionally merges
  an entity's decoded `Text` node with any adjacent literal-run `Text`
  siblings before `comrak-to-pandoc` ever sees the tree, so the ordinary
  shape for `&amp;` in prose is one node mixing verbatim and replaced bytes,
  and the per-original-node boundary is gone by the time the decoder runs.
  Recovering it needs a real byte-cursor walk over the merged node — the
  same verbatim/escape-mismatch shape the YAML walker uses, with comrak's
  own escape/entity table — not the one-call-per-node shortcut this
  paragraph first assumed. That walk still needs nothing beyond
  `verbatim`/`replacement`: the merged case's own call sequence
  (verbatim/replacement/verbatim) is expressible today, exactly like cases
  2, 3, and 6. If anything this is stronger evidence the two primitives are
  general enough for a lockstep decoder, having been designed against
  YAML's four-rule walker, than the narrower atomic-node reading first
  gave. I recommend no API change here either; Plan 3 owns writing that
  walk, now knowing it needs the walk rather than a per-node classification.

Neither check the brief asked to distinguish — "API insufficiency" vs.
"caller awkwardness" for pampa, and "does comrak need something YAML's
walker lacks" — turned up a widening, once case 7 was corrected to describe
comrak's actual merge behavior rather than the atomic-node reading an
earlier draft assumed. Both non-YAML decoders' outstanding work is confined
to their own crates (Plan 2, Plan 3), consistent with what § The shared
builder's "which side of the oracle boundary" paragraph and the shape table
above already state; this review adds concrete call sequences and one
traced bug (case 7) rather than revising either.

### `quarto-yaml`'s API

**Decided 2026-08-21: the change is additive, not breaking.** `Children` is a
**private** enum, so renaming its `None` variant and giving it fields is an
internal change. The two public constructors keep their signatures, and
provenance is attached afterwards — mirroring `with_tag`
(`yaml_with_source_info.rs:148-155`), whose own doc comment establishes the
idiom: "arrays and hashes have no tag-carrying constructor, so this is how the
parser attaches tags to them."

```rust
// private — internal rename, no API break
enum Children {
    Scalar { content_source_info: Option<SourceInfo> },   // was: None
    Array(Vec<YamlWithSourceInfo>),
    Hash(Vec<YamlHashEntry>),
}

// public, additive
impl YamlWithSourceInfo {
    /// Attach derived content provenance, replacing any existing value.
    /// The parser calls this immediately after `new_scalar*`; see
    /// § How the pieces are derived.
    pub fn with_content_provenance(mut self, si: SourceInfo) -> Self;
}

// UNCHANGED: new_scalar(yaml, source_info)
//            new_scalar_with_tag(yaml, source_info, tag)
// A scalar built without `with_content_provenance` has `None`.
```

**What the default buys, and what it costs.** Moving provenance into the
constructors would make it impossible for a scalar to lack — but measured, that
enforcement covers **one** production call site
(`parser.rs:513`, which is in fact the `new_scalar_with_tag` call; the
`Event::Alias` arm at `:634` is the only other) and costs
42 mechanical edits across three repos plus a coordinated minor — see
§ Third-party exposure. The default defuses the objection without the churn: a
scalar whose provenance was never derived returns `None`, which consumers must
already handle. The failure mode is an honest refusal rather than a
silently-wrong offset, which is the property this epic exists to establish. The
`strict-provenance` assert restores enforcement where it matters, by requiring
that any scalar whose derivation ran is `Some`.

```rust
/// Provenance of this node's **decoded scalar content**.
///
/// `self.source_info` describes the node's *source text* — including
/// delimiters for a quoted scalar, and the per-line indentation that decoding
/// strips from a block scalar. Adding a content offset to it is therefore
/// wrong. This is the value to add content offsets to.
///
/// "Content" means the **decoded scalar text, before type resolution** — i.e.
/// yaml-rust2's `Event::Scalar` value string, not `self.yaml`. So `k: ~` has
/// one content byte (`~`) and `k: true` has four, even though neither
/// resolves to a string.
///
/// `None` means no content provenance is available, for any of three reasons:
/// this node is not a scalar (ask [`is_scalar`] to tell that apart); no
/// derivation ran (the node was built by hand, e.g. in a test, or is an
/// unresolved alias); or the lockstep derivation desynced — which is a
/// `quarto-yaml` bug, and panics under `strict-provenance`. All three mean the
/// same thing to a consumer: decline sub-offset arithmetic. An *empty* scalar
/// is **not** `None` — it derives to a zero-length `SourceInfo`.
///
/// Contract: if `Some(si)`, then for every content byte offset `k`,
/// `si.map_offset(k, ctx)` resolves to the source position of content byte
/// `k`, and `si.length() == <decoded content>.len()`. An offset inside a
/// collapsed break region resolves to the start of that region; every
/// non-whitespace content byte is exact.
pub fn content_source_info(&self) -> Option<&SourceInfo>
```

A consumer that asks for provenance of something it believed was a string and
gets `None` has hit an invariant violation, not a precision shortfall, and
should say so loudly rather than degrade silently. Note that the two layers are
**not** symmetric: inside `quarto-yaml`, `None` means "not a scalar" or
"desynced"; at q2's `ConfigValue` layer it additionally means "not of YAML
origin" (CLI `-M`, Lua, defaults files), which is a legitimate everyday state
rather than a bug. So the loud-failure rule does not carry across the boundary —
see § Hand-off to Plan 2.

`Children::None` today covers **every** non-collection node — its own doc says
"scalars, Null, BadValue" — so the rename reaches `Yaml::Null`, `Boolean`,
`Integer`, `Real` and `BadValue` as well. Pinning "content" to the event's
value string is what makes the invariant hold for all of them.

#### Empty and degenerate scalars

Measured; full rows in the fixtures note.

| source | span | decoded value | content pieces |
|---|---|---|---|
| `k:` | 3..3 | `""` | none → zero-length at the anchor |
| `k: ''` | 3..5 | `""` | none → zero-length at the anchor |
| `k: ~` | 3..4 | `"~"` | one verbatim |
| `k: \|` | 3..4 (the `\|` **header**) | `"\n"` | `0..1`←`4..5`, via the header-skip rule |
| `k: \|` + a following key | 5..9 (points at **the next key**) | `""` | none → zero-length at the anchor |

Two consequences. Zero-piece scalars are common, so `finish()` needs an anchor
independent of the pieces (§ The shared builder). And a block scalar with an
empty body has its marker on the **header**, not on content — which the
header-skip rule in § How the pieces are derived handles without touching any
span.

The last row's span is a **pre-existing `compute_scalar_len` defect**: an empty
block scalar followed by another key spans that next key. It is harmless for
provenance (no pieces to misplace) but wrong as a diagnostic span. **Out of
scope here** — fixing it means changing a node span, which this plan otherwise
guarantees is unchanged. Record it and move on.

### Rejected: a content *span* plus a `verbatim` flag

An earlier draft had `quarto-yaml` return `content_span: SourceInfo` alongside
a `content_is_verbatim: bool`. Recorded here so it is not reproposed.

A *span* is a start and an end, and is therefore structurally incapable of
describing the content of a multi-line block scalar, which is runs of source
separated by the stripped indentation:

```
      line one\n      line two\n      <span…
      └─indent┘        └─indent┘      └─indent┘
              ^^^^^^^^^         ^^^^^^^^^
              content runs (newline included, indent excluded)
```

It can fix the first run and nothing after it. The `verbatim` flag existed
only to signal "the span cannot express this case" — an admission of the wrong
return type, not a feature. `SourceInfo` already expresses discontinuity, so
returning one removes the need for the flag entirely.

### Both crates ship as patches

**The principle: an API break gets a minor; a behavior fix or an additive API
gets a patch.** A version requirement cannot express "opt in to a bugfix," so a
behavior fix has nowhere to go but a patch.

**Decided 2026-08-21: the source-map release is SPLIT.** `ProvenanceBuilder` is
new API with three intended callers and none of them exercised until Phase 2, in
a different repo, after a publish. Phase 1's own first item concedes the API
"will likely need iteration" — and the round-4 review found exactly that (a
`finish()` collapse rule only Phase 2's fold fixture reveals). So:

| release | contents | when |
|---|---|---|
| **`quarto-source-map` 0.1.2** | the four behavior fixes only — **no new API** | end of Phase 1 |
| **`quarto-source-map` 0.1.3** | `ProvenanceBuilder` | after Phase 2's walker has driven it green against a path patch |

Two consequences worth stating. Plan 2 unblocks **early**: its obligation 5
gates on `Location.offset` flooring, which 0.1.2 delivers, so it no longer waits
for Phase 2. And `quarto-yaml`'s own release therefore depends on
`quarto-source-map` **0.1.3**, not 0.1.2 — see the requirement note below.

- **`quarto-source-map` 0.1.2.** **Four** behaviors change; none breaks a
  caller, and no public API is added. Decided 2026-08-21: ship
  them as one patch with **release notes that enumerate all four**, rather than
  splitting them across releases (they are one defect class — a
  coordinate-system inconsistency — and consumers would pick up both releases
  anyway) or forcing a minor (nothing fails to compile, so a manifest edit would
  buy deliberation and identical behavior). The four, with their known
  consequences:

  | change | known consequence |
  |---|---|
  | `Location.offset` floors to a char boundary | caret narrows for mid-char spans; JSON-writer `"o"` and `ts_engine`'s `file_offset` move; error-reporting's two crash regression tests silently stop testing (§ Risks cost (c)) |
  | free `utils::offset_to_location` aligns with `FileInformation` (it overcounts the mid-char column today) | `treesitter.rs` faded-prefix ranges and `span_assert` columns shift for mid-char inputs only |
  | `Concat`'s exclusive-end branch uses the last piece's source length | replacement-terminated lists reach the true source end; synthesis-terminated lists return `Some` instead of `None`; the QMD writer's provenance `Concat` end moves |
  | `preimage_in`'s `Substring` arm returns `None` over a `Concat` parent | writers decline to verbatim-copy such nodes and rewrite instead; `cell_options`' length-matched `Concat` parents lose an affine answer that was correct (believed unreached — see § `preimage_in` composes affinely) |

  The Phase 1 q2 smoke step exists to catch anything this table missed. **Three**
  crates declare `quarto-source-map = "0.1.0"` — q2 (`Cargo.toml:130`),
  `quarto-yaml` (`Cargo.toml:26`) and **`quarto-error-reporting`**
  (`Cargo.toml:28`), the last being the crate that actually hands
  `Location.offset` to ariadne (`src/diagnostic.rs:877-880`). All three pick
  0.1.2 up under `^0.1.0` with **no manifest edit** — a deliberate
  `cargo update` plus a lockfile change, which matters because CI builds
  `--locked`.
- **`quarto-yaml` 0.1.3.** Under the additive design (§ `quarto-yaml`'s API) no
  public signature changes, so this is a patch too, and **q2 needs no
  `quarto-yaml` edit** — its `quarto-yaml = "0.1.2"` pin accepts 0.1.3 under
  `^0.1.2`.
  **But `quarto-yaml` must bump its own `quarto-source-map` requirement to
  `"0.1.3"`.** `Cargo.toml:26` currently declares `"0.1.0"`, and `^0.1.0` is
  satisfied by 0.1.0 — which q2's lockfile pins today (`Cargo.lock:5371`). So a
  published `quarto-yaml` 0.1.3 whose code calls `ProvenanceBuilder` while
  declaring `"0.1.0"` fails to compile for any consumer whose lock predates the
  source-map release, and on docs.rs. `cargo update -p quarto-source-map` fixes
  *quarto-yaml's own* lockfile and CI; it does not make the published manifest
  correct. (§ Reversed decisions, R9.)

**Schedule consequence:** no coordinated minor across `quarto-yaml` +
`quarto-yaml-validation`, no q2 manifest edit for `quarto-yaml`, one PR per repo
instead of a two-repo dance. (An earlier draft argued for 0.2.0 — § Reversed
decisions, R8.)

### Third-party exposure: none, and nothing to update

Measured, not assumed. Under the additive design these counts are **evidence
that no edit is needed**, not a work estimate — they are what the rejected
breaking design would have cost.

- crates.io reverse dependencies of `quarto-yaml`: **exactly one** —
  `quarto-yaml-validation` v0.1.2 (19 downloads) — **in the same workspace**.
- `quarto-yaml`: 2,359 downloads, all recent, across 0.1.0/0.1.1/0.1.2 — the
  profile of a new crate whose traffic is CI and docs.rs, not adoption.
- `new_scalar` / `new_scalar_with_tag` call sites — all `#[cfg(test)]` except
  the two in `parser.rs`:

  | crate | sites | where |
  |---|---|---|
  | `quarto-yaml` itself | 5 test + **2 production** | `yaml_with_source_info.rs:279, 292, 294, 309, 311`; `parser.rs:513` (scalars), `:634` (alias) |
  | `quarto-yaml-validation` | 22 test | `schema/helpers.rs` 10, `validator.rs` 5, `schema/parsers/combinators.rs` 4, `tests.rs` 3 |
  | q2 | 15 test | `quarto-config/src/convert.rs`, `pampa/src/pandoc/meta.rs` |

  **Only the two `parser.rs` sites change** — they gain a
  `.with_content_provenance(…)` call. The other 42 compile untouched and keep
  reporting `None`, which is accurate: they never derived provenance.
- `quarto-yaml-validation` **struct-literal-constructs `YamlHashEntry`** at
  `validator.rs:837` and `:2135`. **No fields are added there**:
  `YamlHashEntry.key` and `.value` are `YamlWithSourceInfo` and already carry
  `content_source_info()`, so consumers read
  `entry.key.content_source_info()`. `key_span` / `value_span` keep their
  meaning (source text, delimiters included) and gain a corrected doc comment.

Both crates share one `[workspace.package] version`, so the 0.1.3 bump moves
`quarto-yaml-validation` with it; nothing in that crate needs editing.

### Out of scope, deliberately

- **ipynb.** Its design doc
  (`2026-07-20-ipynb-surface-syntax-design.md`, § "Why pointing into the
  .ipynb bytes is the wrong root") chose one ephemeral `SourceFile` per cell,
  so users see cell-relative positions. The builder does not serve it and
  should not be forced to. Do not spend time on the fit.
- **YAML aliases.** `parser.rs:629-636` leaves `Event::Alias` unresolved: it
  builds `new_scalar(Yaml::Null, make_source_info(&marker, 0))`. An alias
  node's source is `*name` while its value is the anchored node's, so
  "content provenance" has two defensible answers and the crate implements
  neither. **Decided: `None`** — an alias's content genuinely is not derivable
  here, and `None` is the accessor's word for that. Under the additive design
  this needs no code at all: the arm already calls `new_scalar`, which defaults
  to `None`. Document that an alias reads as a scalar with **no** content
  provenance — not as one whose provenance is "empty", which is the phrase this
  plan reserves for a zero-length `Some`. (§ Reversed decisions, R10.)

## Hand-off to Plan 2

Ten obligations. Plan 2's receipt table has been tracking this list as it grew,
so **check the count against that table rather than trusting either document
alone** — it has been out of step twice, in both directions. **When this section
was written none of them existed in Plan 2's checklist** — they were
identified while reviewing this plan, so someone must add them there before
Plan 2 is executed. Items 5-7 came from Plan 2's own author reconciling the two
plans on 2026-08-21.

**Obligations 3 and 10 are now live, not prospective** — this plan's own
releases (§ Evidence, Phases 1 and 2) made them so. `quarto-source-map` 0.1.2
published **2026-08-21T21:28:48Z**, so `quarto-error-reporting`'s two
char-boundary regression tests (obligation 3) have **already** silently
stopped exercising a mid-character renderer offset; this is not a risk to
plan around, it has already happened. And q2's `Cargo.toml`/`Cargo.lock` still
pin `quarto-source-map = "0.1.0"` (obligation 10) while CI builds `--locked`,
so as of this plan's completion none of 0.1.2, 0.1.3, or `quarto-yaml` 0.1.3
has reached q2 — the epic does not reach its own consumer until that refresh
lands.

1. **Thread content provenance through `ConfigValue`** — the substance below.
2. **Test the zero-width renderer label.** Flooring `Location.offset` makes a
   zero-width highlight reachable (§ Risks cost (a)); both renderers live in
   `quarto-error-reporting`, so the test cannot be written from Phase 1.
   Ariadne's `Report::build` anchor is already `start..start`, so it very likely
   renders — but "very likely" is not a test.
3. **Re-anchor the two upstream regression tests** at
   `quarto-error-reporting/src/diagnostic.rs:1601` and `:1635`. They stop
   exercising a mid-character renderer offset once that crate picks up
   `quarto-source-map` 0.1.2, and they stay green while doing it
   (§ Risks cost (c)). This is the epic's only regression coverage for the
   original panic.
4. **Release `quarto-error-reporting` 0.2.2**, carrying `4da3385`. The local
   checkout is still 0.2.1 with that commit untagged, so until this happens the
   fix is only reachable through a path override.
5. **Re-gate Plan 2's Phase 1 on Phase 1 of *this* plan publishing 0.1.2**, not
   merely on Phase 0's evidence capture. Obligation 2 above cannot be satisfied
   before then: the zero-width behavior *only exists* once `Location.offset` is
   floored, so as currently sequenced Plan 2 would release 0.2.2 before the
   behavior under test exists. Writing the test against a local
   `quarto-source-map` path override is the alternative.
6. **Absorb the two q2-side reactions to the floor**, which Phase 1 records but
   must not fix: `pampa/tests/integration/test_location_health.rs:448` (it
   asserts the two `offset_to_location` implementations agree) and the
   JSON-writer snapshots (`.offset` is emitted as `"o"` at
   `writers/json.rs:550`, `:555`, `:2005`, `:2014`, `:2258`, `:2267`, and read
   as `file_offset` at `ts_engine.rs:689`). Per CLAUDE.md the snapshot review
   needs a count, a summary and the file list — not a checkmark.
7. **Specify the desync report as warning-level and non-fatal.** Merging the two
   `None`s (§ Desync policy) means Plan 2's "a `None` on a value expected to be
   a string is an invariant violation" now also fires on a walker desync. The
   rejection of `Err` still stands — a walker bug must not turn a working render
   into a hard failure — so that report must be an internal diagnostic that does
   not fail the render. Nothing is needed in `quarto-yaml` beyond the
   `strict-provenance` panic it already has.

8. **Re-check `qmd-syntax-helper` after the `AttrSourceInfo` meaning change.**
   § Risks rules it out today by *reachability* — its diagnostics come from its
   own `pampa::readers::qmd::read` call, so it never sees this provenance. But
   its 23 `start_offset()` sites across 22 files **write to the user's files**,
   using the accessor that is silently 0 on a `Concat`, and Phase 4 changes what
   `AttrSourceInfo.attributes[i].1` *means*. The conclusion is expected to hold;
   nothing currently re-tests it, and the failure mode if it stops holding is
   corrupted source files rather than a bad caret.
9. **Carry the `AttrSourceInfo` meaning change into `annotated-qmd`.** Plan 2
   decided that `attributes[i].1` becomes content provenance rather than gaining
   a sibling field, on a Rust-only survey. There is a live TypeScript consumer:
   `ts-packages/annotated-qmd/src/block-converter.ts:287` and
   `inline-converter.ts:322` read `attrSource.kvs[i]` and resolve them through a
   `sourceReconstructor`, and `resolveChain`'s `Substring` arm composes affinely
   (§ Risks). Gordon has decided TypeScript moves to content semantics with a
   **0.2.0 bump on `@quarto/annotated-qmd`**. Two things to settle there: whether the reconstructor
   wants the content span (probably — it reconstructs source for an editor,
   where pointing at the opening quote is wrong, but **verify rather than
   assume**), and what it does with an id backed by a `Concat` that cannot
   resolve to one range. Also check for TS fixtures or snapshots encoding the
   current off-by-one offsets.

10. **Refresh q2's lockfile to both releases.** This is the item that makes the
    epic reach q2 at all, and it is currently recorded nowhere live: q2 declares
    `quarto-source-map = "0.1.0"` and its lockfile pins exactly 0.1.0
    (`Cargo.lock:5371`), while CI builds `--locked` — so neither
    `quarto-source-map` 0.1.2/0.1.3 nor `quarto-yaml` 0.1.3 reaches q2 without a
    deliberate `cargo update`. The only prior record of it named
    "`quarto-yaml` **0.2.0**", a version this plan superseded (§ Reversed
    decisions, R8). Name **0.1.3**, and expect the JSON-writer snapshot review
    (obligation 6) to land in the same commit.

One item in Plan 2 goes **stale** rather than being added: Phase 0 of this plan
now ends by removing the `[patch.crates-io]` override, so Plan 2's "drop the
override" item becomes a confirmation.

## Hand-off to Plan 3

Two obligations. Both were previously buried — one filed under § Hand-off to
Plan 2 despite its own text saying Plan 3 owns it, the other as a clause inside
a Phase 1 checklist bullet. They are at this altitude now so the deferral is as
visible as the decision to defer.

1. **Resolve the doc inconsistency the Phase 1 rewrite creates.** Phase 1
   retracts `preimage_in`'s "this is the writer's can-I-Verbatim-copy check"
   sentence, but the same claim is restated *in prose at a call site*:
   `pampa/src/writers/incremental.rs:162-168` reads "A kept block is
   Verbatim-copied out of `original_qmd`, so it must have a byte preimage in the
   target file", and the arm's `.get()` guard checks **bounds, not identity**, so
   a 1→1 fold defeats it. Once the upstream doc is corrected, that comment
   contradicts it, and a codebase asserting both readings is worse than one
   asserting the wrong one. Plan 3's Phase 1 audit owns the site.
2. **The third `offset_to_location` implementation.** Phase 1's audit item is
   read-only and explicitly does not ship fixes outside `quarto-source-map`, so
   `offset_to_location_bytes` (q2 `quarto-parse-errors/src/error_generation.rs:330`,
   whose mid-char behavior this plan has **not** examined) is handed to Plan 3's
   audit. Routing agreed with Plan 3: a **q2-side** disagreement is fixed there;
   a **`quarto-yaml`-side** one is out of Plan 3's scope — file a strand and
   notify this plan's owner, who owns that crate's release.

Plan 3 receives both at its current ref. Also noted from Plan 3's side, and
accepted here: it resolved the baseline regression guard as a **single**
assertion — every `SourceInfo` in the captured baseline pool must be an
`Original` rooted at the document's own `FileId` — rather than the two I
suggested, because "the capture precedes all pipeline stages" is a claim about
source order and not about a value. A threaded parent makes them `Substring`; a
transform-injected node carries `Generated` or a foreign file id. One assertion,
both failure modes, and it still names `incremental.rs:171`.

### Threading content provenance through `ConfigValue`

This is Plan 2's work, but the decision constrains this plan's API, so it is
recorded here.

**`content_source_info()` existing on the YAML node does not reach the
consumer.** `parse_scalar_string_in_place`
(`quarto-core/src/transforms/config_markdown.rs:283-290`) takes a
`ConfigValue`, not a `YamlWithSourceInfo`, and passes `&value.source_info`.
By then the YAML node is gone — config merging across front matter,
`_quarto.yml`, project files and CLI flags happened in between — and the
converter sets `source_info: yaml.source_info.clone()`, the node span.
Something must carry content provenance through the merge.

**The live converter is `pampa::pandoc::meta::yaml_to_config_value`**
(`crates/pampa/src/pandoc/meta.rs:162`, scalar arm at `:241`), consumed at
`:259` (`!md`), `:303` (annotated Markdown) and `:316` (`DocumentMetadata`
default), and reached from `project/mod.rs:169` for project config. An earlier
draft named `quarto-config`'s `config_value_from_yaml`
(`convert.rs:26`) as the setter; verified, that function has **no production
caller** — the only call sites outside its own recursion are its own tests, a
`#[cfg(test)]` use at `materialize.rs:495`, and two locally-shadowed test
helpers of the same name (`project_profile.rs:639`, `render_scripts.rs:712`)
whose bodies call the pampa converter. It is exported dead API. The diagnosis is
unchanged — both converters do `source_info: yaml.source_info.clone()` — but an
implementer following "set it in one place" to the old address lands in dead
code.

**The field is not single-purpose.** Front matter is in scope for Plan 2's
Phase 3, so the three *immediate* re-parse sites (`meta.rs:259`, `:303`, `:316`)
are in scope alongside the deferred config path. The threading is what the
deferred path needs and the binding test is a project-config fixture, but do not
read either as evidence that the field serves one context — an earlier narrowing
did, and § Reversed decisions R11 records why that stopped being true.

**Recommendation: `ConfigValueKind::Scalar { yaml, content_source_info: Option<SourceInfo> }`**,
not a fourth field on `ConfigValue`. Measured costs are comparable — 170 full
`ConfigValue` struct literals (sites with `merge_op:`) versus 206 sites naming
`ConfigValueKind::Scalar` across 49 files, both **excluding
`crates/*/tests/`**; including those directories the figures are 190 and 240 —
so the choice is semantic, not economic:

- **Provenance must not be separable from the value it describes.** A sibling
  field of `value` can be carried forward while `value` is replaced by a merge,
  producing a pair whose string came from one file and whose provenance points
  into another. That maps cleanly onto a real offset in the wrong file — the
  original bug with more confidence behind it.
- **`Option` is forced here** (CLI `-M`, Lua, defaults have no YAML origin),
  and inside the variant that reads as "a scalar may not know its content
  provenance," which is true. On `ConfigValue` it would read as "any config
  value may have content provenance," which is false for maps and arrays.
- **It mirrors the producer**, so `config_value_from_yaml` copies
  variant-to-variant and a shape mismatch is a compile error.
- **Keeping `ConfigValue.source_info` contiguous protects more than one
  hazard.** The Plan 3 session checked the `bind_*` call sites: seven non-doc
  sites exist (`project_resources.rs:1024`, `:1064`, `project/mod.rs:933`,
  `render_scripts.rs:593`, `theme_diagnostic.rs:66`,
  `website_post_render.rs:797`, `quarto/src/commands/render.rs:1194`), and those
  that bind on a `ConfigValue` span or a synthetic one are `Concat`-safe *only
  because* provenance is threaded alongside rather than replacing
  `source_info`. Record that here, or a later "simplification" reopens all of
  them at once.
- `ConfigValue` is `Serialize` and embedded in `DocumentProfile`
  (`categories_raw`, `extra`; `profile_version` equality is enforced at
  `document_profile.rs:842`), so a top-level field changes the serialized
  shape of every config node, not just scalars.

**Do not take the free option.** Replacing `ConfigValue.source_info` with
content provenance needs zero threading and is wrong: a `Concat` there makes
`bind_config_source` / `bind_source_candidates`
(`quarto-core/src/config_sources.rs:90`, `:145`) return `None`, so the
diagnostic loses its file binding and prints **no source snippet at all** —
for exactly the escaped and multi-line values this epic fixes. It also trips
`span_assert`'s existing `SpanProblem::Concat` (`quarto-config/src/span_assert.rs:159`)
across 13 assertion sites.

**Three constraints.** Set it in one place
(`meta.rs:241`, `yaml_to_config_value`'s scalar arm), read it in one place
(`parse_scalar_string_in_place`). A `None` may fall back to
`source_info` **only** because non-YAML metadata carries `Generated`
source_info, where offset arithmetic already yields `None`; document that
reason so the fallback is never extended to YAML-rooted values. On a `Concat`,
`map_offset` is the only safe accessor.

**Test seam.** A `_quarto.yml` **`page-footer.center`** as a three-line block
scalar containing raw HTML — the measured case where two `Q-2-9` warnings that
both belong on line 9 are reported at `8:10` and `9:14`; both fixtures are
transcribed in § Evidence. No unit test on either layer alone catches it. (An
earlier draft said "navbar `text:`" here, which was the *crash* repro's key and
cannot produce accumulating drift, being single-line and single-quoted.)

## Test seam spec

Frozen before any code is written. One row per test: the **named revert hunk**
whose removal reddens it, and whether the assertion actually **discriminates**.
Rows marked *gating* survive their own revert — they check shape, not behavior,
and must not be counted as evidence for the rule they sit next to.

There is only one tier here: in-crate Rust unit tests against pure logic, plus
two cross-repo runs (Phase 0's fixtures, Phase 1's q2 smoke) that are
observations rather than tests. Nothing is mocked; the unit under test is always
the real one.

### Phase 1 — `quarto-source-map`

| test | revert this hunk → RED | discriminates? |
|---|---|---|
| `FileInformation::offset_to_location(7)` on `x = 'A✨B'` returns `Location{offset: 6, column: 6}` | `offset: safe_offset` → `offset: offset` (`file_info.rs:~122`) | yes — 7≠6 before |
| free `utils::offset_to_location(src, 7)` returns the same | the floor + early loop break (`utils.rs:8`) | yes — column 7≠6 before |
| `Concat` exclusive end, **replacement**-terminated → `Some(9)` | `last.source_info.length()` → `last.length` (`mapping.rs:64-70`) | yes — `Some(8)` before |
| `Concat` exclusive end, **synthesis**-terminated → `Some(11)` | same hunk | yes — **`None`** before |
| `Concat` exclusive end, **all-verbatim** → `Some(9)` | same hunk | **no — *gating*.** 9 before and after. Keep for shape; never cite as coverage |
| `substring(concat, 0, 4).preimage_in(fid)` → `None` | the `Concat => None` guard in the `Substring` arm (`source_info.rs:453-456`) | yes — `Some(1..5)` before |
| bare `concat.preimage_in(fid)` → `Some(1..6)` | same hunk | **no — *gating*.** Unchanged by the fix; it pins that only the composition changed |
| `cell_options` multi-option shape → `None` | same hunk | **no — *gating*.** Gappy, so `None` either way |
| `cell_options` single-option shape **through `Substring`** → `None` | same hunk | yes — `Some(hull)` before. **This is the one row that binds the documented behavior change** |
| builder: all-verbatim → contiguous `SourceInfo`, not a 1-piece `Concat` | the abutting-verbatim merge in `push` | yes — N pieces before |
| builder: **fold shape** stays a 3-piece `Concat` | the collapse predicate, reverted to "contiguous + equal totals" | yes — collapses to `Original{0,7}` before. **The row that binds R1/R3** |
| builder: zero pieces → zero-length `SourceInfo` at the anchor | `finish()`'s empty-piece-list branch | yes — no other branch produces a position |
| builder: `out_len == 0` piece is **stored**, source tiling gap-free | re-add the `if out == 0 { return }` drop in `push` | yes — piece count drops and the contiguity `debug_assert` fires |
| builder: `in_parent` over a real `Concat` parent yields parent-relative pieces | make `finish()` consult `resolve_byte_range` | yes — `None` from the parent, so the builder cannot produce a result |

### Phase 2 — `quarto-yaml`

The 32 fixture rows are **not** 32 binding tests. Mapped against the two rules
that were reversed late:

| rule | rows that bind it | rows that are gating |
|---|---|---|
| **byte-identity verbatim tag** (not length) | `root plain, col-0 continuation` — **one row** | the other 31. Every block tail's break region *is* a byte-identical newline run, so reverting the tag leaves them unchanged. This is why the unsound rule survived 24 shapes undetected |
| **header-skip predicate** (byte test **and** empty-or-all-newlines value) | `empty block scalar`, `block \| content starts with \|`, `block \| content is exactly \|` — three rows | the other 29 |
| piece lists in general | all 32 | — |

So: "32 rows green" is evidence for the *derivation*, and evidence for neither
reversed rule beyond those four rows. Revert the byte-identity tag and 31 rows
stay green.

| test | revert this hunk → RED | discriminates? |
|---|---|---|
| every parsed shape has `content_source_info() == Some` | the `with_content_provenance` call at `parser.rs:513` | yes — `None` before, and this is the **only** backstop against a forgotten attach at a future production site |
| a non-scalar (`Hash`/`Array`) returns `None` | the `Children::Scalar`-only arm of the accessor | yes |
| the `Event::Alias` arm returns `None` | — | **accepted-untested-by-revert**: `None` is the constructor default, so there is no hunk to remove. Assert it anyway to freeze the contract |
| `strict-provenance` length assert | the `debug_assert_eq!` | **no valid input reddens it** — see below |

### Accepted untested, with rationale

- **The desync panic path.** By construction no valid YAML reaches it (that is
  the § Desync policy claim), so no fixture can redden it. The CI step proves
  the *length* invariant, not desync detection. To bind it you would have to
  inject a deliberately-broken walker; not worth a permanent test, but do not
  read a green `strict-provenance` job as evidence that desync handling works.
- **`preimage_in`'s doc rewrite** and the release notes. No test binds prose.
  The hand-off obligations exist because of this.
- **The read-only audit item.** An observation with an § Evidence artifact, not a
  test.
- **`Location.offset`'s effect on `pampa`'s JSON writer `"o"` fields and
  `ts_engine`'s `file_offset`.** Phase 1 records the movement and must not fix
  it; the binding test is Plan 2's (hand-off obligation 6). Untested *here* on
  purpose.
- **`test_location_health.rs:448` binds neither `offset_to_location` fix**, since
  it asserts the two implementations *agree* — it stays green whether both floor
  or both overcount. Do not count it.

## Phases

### Phase 0 — capture the crash evidence (partly time-sensitive)

`quarto-error-reporting` `4da3385` already fixes the char-boundary crash — but
it is **unreleased**: the local checkout is still `version = "0.2.1"`
(`~/src/quarto-error-reporting/Cargo.toml:11`) with that commit committed and
untagged. Plan 2 ships it as 0.2.2. Until then the only way to exercise the fix
from q2 is the path override below.

**Corrected framing:** `q2 0.24.0` is released and immutable, so the
*end-to-end panic* stays reproducible for as long as the fixture survives — the
first two bullets do not expire. **The fixture is not committed**, though: an
earlier draft claimed it was, but `.scratch/` is ignored via
`.git/info/exclude:20` — a *local, unshared* exclude — `git ls-files .scratch`
is empty, and the directory exists only in the worktree it was built in
(`.worktrees/workspace-1` at the time of writing, which may be gone). A
`git clean` or a worktree removal destroys it. Its full content is therefore
transcribed into § Evidence below, which also hands Plan 2's Phase 3 its
fixture. What expires is **per-hunk attribution**: proving a
single hunk of `4da3385` is independently load-bearing needs a q2 build whose
`Location.offset` is still unfloored *plus* a hand-patched dependency, which is
impossible against the released binary and impossible in-tree once Phase 1
lands. The last bullet is a unit test in the `quarto-error-reporting` repo and
never expires.

**Which binary each bullet uses matters.** Bullets 1-2 exercise q2 end to end.
Bullet 1 uses the **released** `q2` on `PATH` (`~/.local/bin/q2`, 0.24.0) —
that is the whole point, since it resolves `quarto-error-reporting 0.2.1` from
crates.io and is immutable. Bullet 2 needs a build of *this worktree*, so it is
`cargo run --bin q2 -- render`. **Bullets 3-4 need no q2 build at all**: both
are upstream unit-test experiments, runnable with `cargo test` in
`~/src/quarto-error-reporting` alone.

**The override is a three-line addition, not a whole section.**
`Cargo.toml:289` `[patch.crates-io]` and the `lua-src` / tree-sitter-language
entries beneath it are **committed and load-bearing**. Only the comment plus
`quarto-error-reporting = { path = … }` are the local addition. Remove those
lines, not the section. `Cargo.lock` is dirty too, so check both.

**What expires, precisely.** Not per-hunk attribution as such — those
experiments are upstream unit tests. What expires affects *both* per-hunk
bullets: once
`quarto-error-reporting` picks up `quarto-source-map` 0.1.2, its two
mid-character fixtures floor to boundaries before the renderer sees them, so
reverting a hunk stops changing the outcome. See § Risks cost (c) — those tests
need re-anchoring regardless.

- [x] Confirm the repro panics on stock `main`, with the released binary:
      `cd .scratch/ariadne-emoji-panic/repro && q2 render` → exit 101
      (recorded in § Evidence)
- [x] With the `[patch.crates-io]` override active, run
      `cargo run --bin q2 -- render` in the same directory → exit 0 with both
      `Q-2-9` warnings printed
- [x] **(expires when error-reporting takes 0.1.2)** Revert only the
      `Report::build` anchor hunk of `4da3385` → confirm
      `ariadne_span_starting_inside_multibyte_char_does_not_panic`
      (`diagnostic.rs:1601`) goes red. The commit message predicts a panic at
      `write.rs:267`; **that is a prediction, not an observation** — the only
      recorded panic is `write.rs:84:59`. A different line still proves the hunk
      load-bearing; record whatever you observe.
- [x] **(same expiry)** Revert only the **ariadne main-label** hunk — the
      `write.rs:84` site, and the only panic ever actually observed. With the
      anchor still snapped, this should panic at `write.rs:84` on its own.
      `4da3385` snaps at four sites; this bullet and the next two cover the
      other three.
- [x] **(same expiry)** Revert only the **ariadne detail-label** hunk — the
      fourth and last of `4da3385`'s snap sites. **Result: not attributed.**
      Reverting it left both regression tests green (§ Evidence) — neither
      target test's fixture builds a `DetailItem` with a location, so neither
      reaches the detail-label path. Three of the four hunks are attributed,
      by this phase's other three bullets; this one is a real gap in
      coverage, not a discharge of the bullet's original "attributes all
      four" framing, which this checkbox retracts.
- [x] **(same expiry)** Revert only the annotate-snippets clamp hunk
      (`diagnostic.rs:1031`) → confirm its twin at `:1635` goes red
- [x] Paste all observations into § Evidence

**Overlap with Plan 2, acknowledged in one direction only so far.** Plan 2's
Phase 1 reverts the same `4da3385` hunks and calls that "the only moment at
which those two tests can be shown to bind". These bullets do it earlier, one
hunk at a time, expecting the same two tests red. Both experiments are sound and
neither is wasted — this one attributes per hunk, Plan 2's confirms at release —
but the exclusivity claim was wrong. Plan 2 has since narrowed its version to
its actual delta — that the *ariadne* test goes red, which these bullets bind
only by panic location — and sequenced it to run in the same sitting as this
phase rather than as a second expedition. Do not drop either.
- [x] Publishing mechanism — **verified 2026-08-21, no human step exists.**
      Both repos carry a byte-identical `release.yml` that publishes to
      crates.io via **Trusted Publishing (OIDC)**, with no stored token and no
      approval gate (`release` environment, zero protection rules in both).
      It triggers on **any push to `main` whose workspace version is ahead of
      the registry** — so "release" means *merge a version-bump PR*, and there
      is no `cargo publish` for a human to run. Confirmed working: successful
      Release runs on `main` (2026-07-30 for `quarto-source-map`, 2026-08-08 for
      `quarto-yaml`) with tags `v0.1.1` and `v0.1.2`.
      Two consequences the checklists depend on: the workflow **hard-fails if
      the publishable workspace crates disagree on a version**, which is why
      `quarto-yaml` and `quarto-yaml-validation` must bump together; and it runs
      `cargo publish --locked`, so a committed, current lockfile is a release
      prerequisite, not a nicety.
- [x] **Remove the three added `[patch.crates-io]` lines** (the comment and the
      `quarto-error-reporting` path entry — *not* the section header or the
      `lua-src` entry), and confirm `git diff Cargo.toml Cargo.lock` is empty.
      Note the scope honestly: the override is an **uncommitted edit in
      the worktree it was made in**, so this cleans *that* worktree. It does not gate
      Plan 2, which runs on a different branch that is already clean — an
      earlier draft claimed the causal link "so Plan 2 starts from a clean
      manifest", which was illusory. Plan 2's matching item is a confirmation,
      not a dependency.

### Phase 1 — `quarto-source-map` 0.1.2

**Prerequisite reading**, because two of the three decoders this phase designs
for are not described anywhere else in this plan.

pampa's attribute path is `unescape_punctuation`
(`crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs:41`) — **private,
with exactly one caller**, `extract_quoted_text` at the same file's `:32`,
which is what `treesitter.rs:1207-1212` actually calls. Note that
`extract_quoted_text` returns a bare `String` and discards the offsets, and
both functions sit a layer below the caller — so § The shared builder's "it
can emit pieces as it decodes" is true in principle (we own the decoder) but
understates the work: the piece list has to be threaded back out through two
private functions.

The comrak path is the `NodeValue::Text` arm at
`crates/comrak-to-pandoc/src/inline.rs:49-52` feeding
`tokenize_text_with_source` (`src/text.rs:90-140`).

Their implementation work lives in Plan 2 and Plan 3 respectively.

Both upstream repos use plain `cargo test`, **not** `cargo nextest` — q2's
CLAUDE.md mandate does not apply outside q2, and neither crate has nextest
configured.

- [x] **Design-review `ProvenanceBuilder` against all three decoders on paper
      before writing it.** The question to answer per decoder is *which side of
      the oracle boundary it sits on* (§ The shared builder), then walk a YAML
      break region, a `\t` escape, a `''` escape, the EOF-synthesis case, the
      zero-piece empty-scalar case, a `\*` attribute escape, and an `&amp;`
      entity through the proposed API. If any needs an API the others don't,
      resolve it now — shipping a YAML-shaped builder means the other two
      hand-roll their own, which is the status quo with extra steps.
      **Done when the outcome is written back into § The shared builder** — one
      paragraph per walkthrough, seven in total, so the item has an artifact
      rather than a feeling;
      three later things (this phase's builder tests, Phase 2's walker, and the
      committed fixtures) assume the `verbatim`/`replacement`/`finish` shape and
      go stale silently if it changes.
- [x] Failing test first: **`FileInformation::offset_to_location`** (there are
      three implementations and they disagree — § Findings) with a mid-char offset must
      return a `Location` whose `offset` and `column` describe the **same**
      position. Observe red. — Done in T3 (`test_offset_to_location_floors_offset_field_too`,
      RED: `left: 7, right: 6`, per task-3-report.md).
- [x] Fix: `offset: safe_offset` (the floor loop already exists at
      `file_info.rs:116-120`; it currently floors `column` only) — Done in T3
      (commit `8e07717`).
- [x] Fix `Concat`'s exclusive-end branch (`mapping.rs:64-70`) to use the last
      piece's **source** length, not its content length (§ Design, with the
      three measured terminal shapes). Test all three: verbatim unchanged,
      replacement-terminated now reaching the true source end, and
      synthesis-terminated returning `Some(eof)` where it returns `None` today.
      — Done in T3 (commit `0c65d52`); all three RED values matched the brief
      exactly (`Some(8)`→`Some(9)`, `None`→`Some(11)`, gating row unchanged at
      `Some(9)`).
- [x] Fix `preimage_in`'s `Substring` arm (`source_info.rs:453-456`) to return
      `None` when the parent is a `Concat` (§ `preimage_in` composes affinely).
      Test the measured fixture: a gap-free `Concat` whose content is 4 bytes
      over source 1..6 yields `Some(1..6)` bare, and must yield `None` — not
      `Some(1..5)` — through a `substring(_, 0, 4)`. — Done in T3 (commit
      `0e900e2`); RED matched exactly (`Some(1..5)`→`None`).
- [x] **Rewrite `preimage_in`'s doc comment** (`source_info.rs:410-413`), whose
      "this is the writer's can-I-Verbatim-copy check" sentence is exactly the
      claim the byte-identity finding retracts. Wording is in § `preimage_in`
      composes affinely; Plan 3's consumer audit cites it, so get it in before
      the release rather than after. — Done in T3, alongside the Fix 4 commit
      (`0e900e2`).
- [x] Test the `cell_options` **shapes** — hand-modelled `Concat`s of the same
      geometry, not calls into `quarto-core`, since this phase's PR is in the
      `quarto-source-map` repo. Two cases, and note *which* call changes: a
      multi-option cell (pieces separated by `#| ` prefixes, so gappy) is `None`
      before and after; a single-option cell is one gap-free piece, so
      `concat.preimage_in()` still returns `Some` — **the change is only through
      the `Substring` composition**, `substring(concat, 0, n).preimage_in()`,
      which goes from a wrong answer to `None`. That single shape is the whole
      documented behavior change, and testing it is what stops the reachability
      inspection above from being load-bearing. — Done in T3
      (`test_preimage_in_cell_options_multi_option_shape_is_gating` and
      `..._single_option_shape_through_substring_returns_none`; the
      single-option RED was `Some(5..8)`, matching the brief's measured
      before-value).
- [x] Test that a piece list tiles its source **contiguously**, so
      `preimage_in` yields a hull: assert `Some` for the escaped-break shape
      (verbatim 4..7, stored zero-content 7..11, verbatim 11..14) and `None`
      when a piece is omitted. This is the test that stops a future
      "simplification" from re-dropping zero-content pieces. — Done in T3
      (`test_preimage_in_concat_contiguous_hull_with_zero_content_piece`); this
      one is not itself red under Fix 4's hunk (it guards a different,
      pre-existing invariant, per the T3 report), which is expected and noted
      there.
- [x] Apply the **floor fix** to the free `utils::offset_to_location`
      (`src/utils.rs:8`) too — failing test first, as above, since CLAUDE.md's
      TDD mandate applies to both: **floor, matching `FileInformation`** — return the
      floored offset, and stop the column loop *before* counting the character
      that contains a mid-char offset, so both functions agree. Only mid-char
      inputs change behavior; boundary offsets are already identical. Live
      production callers are `pampa/src/pandoc/treesitter.rs:1463-1464`,
      `:1485-1486` and `quarto-config/src/span_assert.rs:188`, and
      `pampa/tests/integration/test_location_health.rs:448` asserts the two
      agree. Whether it moves depends on where those `Location`s' row/column
      came from, which this plan has not established — so treat red *or* green
      there as **unknown until the smoke runs**, and record which you get
      rather than assuming either is the bug. — Done in T3 (commit `022f489`);
      RED matched exactly (offset 7 vs 6, the `.offset` assertion firing
      before the `.column` one). Whether it moved anything in q2: **green**,
      per Phase 1's q2 smoke evidence below — not evidence of general
      agreement, only that this suite's own `Location`s already sit on char
      boundaries.
- [x] Audit — **read-only, bounded, output goes in § Evidence.** The scope is
      "sites that can hand a non-boundary offset to a `Location`", not every
      construction site: there are ~155 `Location {` literals in q2's non-test
      sources and enumerating them is not this phase's job. Concretely: the
      third implementation `offset_to_location_bytes`
      (q2 `quarto-parse-errors/src/error_generation.rs:330`, whose mid-char
      behavior this plan has **not** examined), and `quarto-yaml`'s own
      `Location` uses. **Fixes outside `quarto-source-map` do not ship in this
      phase's PR** — record them and hand them to Plan 3's audit.
- [x] **(after the 0.1.2 bump below — on a branch off the 0.1.2 line, held for
      0.1.3)** Implement `ProvenanceBuilder` with `in_file(file_id, anchor)` and
      `in_parent(parent, anchor)`. **Tests and their named revert hunks are
      frozen in § Test seam spec** — do not invent a harness; the cases are: all-verbatim (must produce a
      contiguous `SourceInfo`, not a 1-piece `Concat`, which requires the
      coalescing contract), **zero pieces (must produce a zero-length
      `SourceInfo` at the anchor)**, one replacement, one deletion
      (`out_len == 0`, **stored**, so the source tiling stays gap-free),
      synthesis (empty src range, `out_len > 0`),
      adjacent replacements, replacement at offset 0, at the end, **the fold
      shape** (verbatim / 1→1 replacement / verbatim, which must stay a 3-piece
      `Concat` and must *not* collapse — see § The shared builder), and
      **`in_parent` over a real `Concat` parent**, since "the builder must never
      resolve absolute positions" is a stated contract and the only production
      caller of `parse_with_parent` hands it a `Concat`
      (`quarto-core/src/cell_options/mod.rs:227-229`). Without that last one a
      `finish()` reaching for `resolve_byte_range` passes every other test. —
      Done in T7 (branch `provenance-builder`, commit `545f50d`, cut from the
      0.1.2 line at `318ed77`): 124 tests (114 + 10 new), all five frozen rows
      individually revert-hunk-verified RED-then-GREEN, plus the five named
      extra cases (one replacement, synthesis, adjacent replacements,
      replacement at offset 0, at the end). Review clean, zero
      Critical/Important findings.

- [x] **Smoke the four behavior changes against q2 before releasing.** This
      phase's `PR → CI green` is *quarto-source-map's* CI, which cannot see the
      consumers; § Risks predicts JSON-writer snapshot churn and a caret
      regression, and the affected code is in q2 and
      `quarto-error-reporting`. In q2, add
      ```toml
      [patch.crates-io]  # LOCAL DEV ONLY — do not commit
      quarto-source-map = { path = "/Users/gordon/src/quarto-source-map" }
      ```
      **against the 0.1.2 branch** — smoking a tree that carries unreleased API
      is not smoking what ships — then `cargo nextest run --workspace` and
      **record what moved** in
      § Evidence. **A green run is not evidence of safety.** No existing q2 test
      reacts to the `Concat` exclusive-end change: the only production consumer
      of the QMD writer's provenance `Concat` is
      `quarto-core/src/stage/stages/engine_execution.rs:733`, and
      `crates/pampa/tests/integration/qmd_writer_source_info.rs` exercises only
      interior offsets — `concat_piece_lengths_sum_to_buffer_length` checks
      lengths, every `map_offset` call uses an interior position. Plan 2's
      Phase 2 adds an exclusive-end assertion there before the lock refresh, so
      the change is observed rather than asserted. Expect silence here on that
      change, and do not read it as coverage. Remove the patch afterwards. **Do not fix anything here**:
      Phase 1 works in the `quarto-source-map` repo and cannot touch a q2 test
      or snapshot. The two predicted reactions —
      `pampa/tests/integration/test_location_health.rs:448` and the JSON-writer
      snapshots — are Plan 2 hand-offs (§ Hand-off to Plan 2), and the snapshot
      one needs CLAUDE.md's count-summary-file-list treatment rather than a
      green checkmark.
- [x] **Write release notes enumerating all four behavior changes** and their
      known consequences (the table in § Both crates ship as patches). This is
      the mitigation the versioning decision rests on: three behaviors change in
      one patch that three crates pick up under `^0.1.0` with no manifest edit,
      so the notes are the only signal a consumer gets. — Done in T5 (commit
      `a097908`, amended to `e6a2394` then `318ed77` across two fix rounds: the
      first added the dropped "caret narrows for mid-char spans" clause, the
      second corrected "narrows" to the accurate zero-width case). Notes live
      in the commit message (no `CHANGELOG.md` in this repo).
- [x] Bump to `0.1.2` **with the four behavior fixes only — hold
      `ProvenanceBuilder` out of this release** (§ Both crates ship as patches).
      PR → CI green → **merge to `main`, which is what publishes** (Trusted
      Publishing fires on the version being ahead of the registry — see
      Phase 0). Plan 2 unblocks here. — Done: pushed, PR
      posit-dev/quarto-source-map#3, CI green on all four checks, **merged and
      published 2026-08-21T21:28:48Z, tag `v0.1.2`** (Gordon's explicit
      approval, "Push, PR, and merge now").
(The builder item above lands here in execution order: on a branch off the
0.1.2 line, unpublished until Phase 2's walker has driven it. Its unit tests run
locally; the release is Phase 2's last act, as `0.1.3`. Phase 2 **may revise**
the API and the seven design-review paragraphs — that is why the release is
split, so do not treat either as frozen.)

The zero-width-label test belongs to **Plan 2**, not here: both renderers live
in `quarto-error-reporting`, which `quarto-source-map` does not depend on (its
deps are serde, serde_json, smallvec), so the test is unwritable from this
phase. See § Hand-off to Plan 2.

### Phase 2 — `quarto-yaml` 0.1.3

Comprehensive from the start: no staged `None` for unsupported styles.

**This phase develops against an unpublished `ProvenanceBuilder`, and publishes
it at the end.** Under the split release (§ Both crates ship as patches), 0.1.2
carries only the behavior fixes; the builder ships as **0.1.3**, and this phase
is what drives it. So there is no publish to wait for — develop against the
local checkout throughout, and expect to change the builder API here rather than
regretting it after a release:

```toml
# ~/src/quarto-yaml/Cargo.toml — LOCAL DEV ONLY, do not commit
[patch.crates-io]
quarto-source-map = { path = "/Users/gordon/src/quarto-source-map" }
```

- [x] Add that `[patch.crates-io]` block to `~/src/quarto-yaml/Cargo.toml` before
      the walker item below — the stub item compiles without it, the walker does
      not, and a compile error is not a red test — Done (T8 onward; present and
      uncommitted throughout Phase 2, confirmed at each task boundary).
- [x] Drop the patch before opening the PR — Done in T15, step 1: the first
      build of the branch against the real registry rather than the local path
      checkout, after `quarto-source-map` 0.1.3 published.

When the walker is green, publish `quarto-source-map` **0.1.3** from its branch
(merge to `main`), then drop this patch, declare
`quarto-source-map = "0.1.3"` in `~/src/quarto-yaml/Cargo.toml`, and
`cargo update -p quarto-source-map` — quarto-yaml's CI builds `--locked`, so an
unupdated lockfile fails there rather than locally.

**Most expected values are measured and committed** at
`claude-notes/research/2026-08-21-yaml-content-provenance-fixtures.md`: source
text, style, indent, span, decoded value, and the expected piece list with
absolute source offsets. Transcribe those
rather than re-deriving them — re-deriving risks encoding your reading of YAML's
folding rules instead of the behavior the walker must match, which is the exact
failure the lockstep design exists to prevent.

**Seven required cases have no fixture row**, so for these you must derive and
then *record* the values: `k: ~`, `k: true`, a quoted **key**
(`'quoted key': v`), a **flow collection** (`k: ['a b', "c\td"]`, which needs a
per-item variant of the generator's `emit`), a **tagged scalar** (`!path 'x'`),
a **plain double-quoted** scalar (`k: "hello"` — the note has `\t`, `\u00e9`,
many-escapes, multi-line-fold and escaped-break, but no unescaped one), and
**`\n` as an escape**. The last two were named in the table above and omitted
from this list; count seven. The generator **is** committed —
`claude-notes/research/yaml-content-provenance-walker/walker.rs`, with its
Cargo.toml and the two easy-to-get-wrong rules in the sibling README — so
extending it is cheaper than hand-deriving five rows.

**Note the cross-repo path.** Phase 2's PR is in `~/src/quarto-yaml`, but the
fixtures note and the generator live in **q2**. Recording new rows therefore
means a second, separate commit on a q2 branch — **use the same branch that
carries this phase's § Evidence entry**, so the rows and the run that produced
them land together.

- [x] **First, land the API surface as a stub** so the tests below can compile
      and go *red* rather than failing to build: rename `Children::None` to
      `Children::Scalar { content_source_info: Option<SourceInfo> }`, add
      `with_content_provenance` and `content_source_info()`, and leave the
      parser attaching nothing yet (so every scalar reads `None`). Phase 1's
      "observe red" idiom worked because
      `offset_to_location` already existed; here the method does not, and a
      compile error is not a red test. — Done in T8 (commit `da2c560`), one
      file changed, no call site outside `yaml_with_source_info.rs` needed
      editing. Review Approved with one Important (a stub test's comment
      claimed a behavior the test can't exercise) and one deferred Minor
      (`with_content_provenance` silently no-ops on a non-`Scalar` node);
      both folded into T10 per Ruling R-F rather than run as a separate fix
      round, and both landed there.
- [x] Failing tests next, one per scalar style. **Assert the piece list, not
      `map_offset`, for the bulk of them.** `map_offset` needs a
      `&SourceContext`, and `quarto-yaml` has no `tests/` directory and
      constructs a `SourceContext` nowhere in `src/` — so routing 30 fixtures
      through one means writing an unbudgeted helper and registering each
      fixture's text as the context's first file. `SourceInfo::Concat`'s and
      `SourcePiece`'s fields are all public (`source_info.rs:128-138`), and the
      fixtures note records `content <- source` per piece, so asserting the piece
      list needs no context and matches the committed data's shape. Keep two or
      three `map_offset` tests for the contract itself. Every shape must also
      assert `content_source_info()` is `Some`. The shapes:

      | style | shape |
      |---|---|
      | plain, single-line | contiguous |
      | **plain, multi-line** | `Concat` (fold per break) |
      | single-quoted | contiguous, excludes quotes |
      | single-quoted with `''` | `Concat` |
      | single-quoted with a **trailing** `''` | `Concat`, exercises the end-offset contract |
      | double-quoted | contiguous, excludes quotes |
      | double-quoted with `\t`, `\"`, `\\`, `\uXXXX` | `Concat` |
      | double-quoted with `\n` | `Concat` — **no fixture row yet**, add it to the known-missing list below |
      | **double-quoted, multi-line** | `Concat` (fold per break) |
      | **double-quoted with an escaped break** (`\`+newline) | 3-piece `Concat` — the `out_len == 0` piece is **stored**, keeping the source tiling gap-free so `preimage_in` yields a hull (§ The shared builder) |
      | block `\|`, single-line | contiguous |
      | block `\|`, multi-line | `Concat`, one piece per line |
      | block `\|` with a **blank line inside** | `Concat`, break region spans both newlines |
      | block `\|` with a **more-indented line** | `Concat`, content-leading spaces preserved |
      | block `\|` with **trailing spaces on the last line** | `Concat` reaching past `end_offset` |
      | block `\|` with **no final newline at EOF** | `Concat` ending in a synthesized piece |
      | block `>`, multi-line, with a blank line | `Concat`, folds and breaks distinguished |
      | block `>` with a more-indented line | `Concat`, not folded |
      | chomping `\|-`, `\|+` | `Concat`, trailing-newline count correct |
      | **indentation indicator** `\|2` | `Concat`; `marker.col()` already yields the right per-line strip |
      | **CRLF** variants of the block and plain multi-line cases | `Concat`; the `\r` is **absorbed into the break replacement**, not a separate deletion — see the measured `block \| CRLF` row |
      | **empty value** (`k:`), **empty quoted** (`k: ''`) | `Some` of a zero-length `SourceInfo` at the anchor — **not `None`**; "empty but derived" must be distinguishable from "could not derive" |
      | **`k: ~`**, **`k: true`** | one piece; content is the event's value string (`~`, `true`) |
      | **empty block scalar** (`k: \|` alone) | one verbatim piece `0..1`←`4..5`, via the header-skip rule |
      | **block `\|` whose content starts with `\|`** | derives normally — the header-skip predicate must **not** fire (measured; it desyncs under a byte-only predicate) |
      | **all-escape scalar** (`k: ''''`) | one replacement piece; must **not** collapse to a contiguous `SourceInfo`, or the length invariant breaks |
      | **root plain scalar, column-0 continuation** (`aaa`⏎`bbb`) | 3 pieces, middle one a **replacement** — the case that proves the verbatim tag must key on bytes, not length |

      Done in T9 (quarto-yaml `9452e18`, 42 tests, all RED on `None`-vs-`Some`;
      73 pre-existing tests still pass): every named shape above got its own
      test. Two declared omissions, both judged acceptable in review: the
      break-region "value at a tab" entry sub-case (optional per the brief;
      recorded below and in the design-review walkthrough), and
      `empty block scalar, next key follows` (its span defect is out of
      scope; the empty→`Some` rule is exercised by `empty_value` and
      `empty_single_quoted` instead).
- [x] Cover every scalar **position**, not just block-mapping values:
      values, **keys** (`key_span` has the identical defect — verified:
      `'quoted key': v` yields `key_span` 0..12, quotes included), **flow
      collections** (verified: `k: ['a b', "c\td"]` yields spans `'a b'` and
      `"c\td"`, brackets excluded, quotes included), and **tagged scalars**
      (`!path 'x'`). Verified empirically: the node marker points at the
      VALUE, not the tag — `k: !path 'x/y'` gives span 9..14 = `'x/y'`
      (`parser.rs:405`, `:499`) — and anchors are excluded for the same
      reason, so tags need no extra arithmetic. — Done in T9: `quoted_key`,
      `flow_collection_item_0`/`_1`, and `tagged_scalar`, each its own test,
      plus the seven previously-missing fixture rows derived and recorded in
      the q2 fixtures note (commit `528038877`).
- [x] Implement the lockstep walker (§ How the pieces are derived), starting
      from the **committed** prototype at
      `claude-notes/research/yaml-content-provenance-walker/walker.rs` — it
      implements both corrected rules and generated the fixtures. Do not start
      from a code block in this plan; there no longer is one, for that reason. **The walk is bounded by the value and
      reads source past `end_offset`** — do not slice the span and walk the
      slice.

      **The node span is unchanged, and the walker is purely additive.** Do not
      delete or rewrite `plain_scalar_len` / `quoted_scalar_len` /
      `block_scalar_len`: all three still feed `compute_scalar_len` (`parser.rs:384-400`), which produces the
      node's `source_info`, and this plan guarantees that span keeps its
      current meaning ("`self.source_info` describes the node's *source
      text* — including delimiters"). The walker is **purely additive**. The
      natural shape is to widen the existing function rather than add a second
      traversal:

      ```rust
      fn compute_scalar_provenance(&self, marker: &Marker, value: &str,
                                   style: TScalarStyle)
          -> (usize, Option<SourceInfo>)
      //      span len ──┘    content provenance ──┘  (None iff the walk desynced)
      ```

      called from the `Event::Scalar` arm (`parser.rs:499-516`), which is where
      yaml-rust2's `value: String` is in scope. — Done in T10 (commit
      `8b8c05e`): `compute_scalar_provenance` widened as specified,
      `walk_scalar_provenance` feeding a `ProvenanceBuilder` directly (the
      builder's own coalescing replaces the prototype's hand-rolled merge
      step). 42/42 previously-red tests green; full workspace green; review
      confirmed no test expectation was edited and no branch keys on style
      rather than the four general rules. **Superseded in part**: rule 1's
      entry condition, ported unchanged from the prototype at this commit,
      was later found to desync a trivially valid shape and was corrected —
      see the style-conditional rewrite in § How the pieces are derived and
      the fix-round history below.
- [x] **Attach provenance at the two production construction sites** —
      `parser.rs:513` (scalars) and the `Event::Alias` arm at `:634` — now that
      the walker exists to compute it. **No constructor signature changes**
      (§ `quarto-yaml`'s API). The alias arm needs nothing beyond the default
      `None` (§ Out of scope). This item, the walker above and the header-skip
      rule below are **one change**: attaching without the walker is impossible,
      and the walker desyncs without the rule. — Done in T10: attached at the
      `Event::Scalar` arm only; `Event::Alias` needed no change (already
      constructs via bare `new_scalar`, defaulting to `None`).
- [x] **Implement the header-skip rule** (§ How the pieces are derived, "Where
      the walk starts"): begin the walk at the newline ending the header line
      when a block scalar's span starts on `\|` or `>` **and** its decoded value
      is empty or consists only of newlines. **Both** clauses — the bare byte
      test alone is unsound and desyncs on a block scalar whose *content* starts
      with a pipe, which is valid YAML and measured. This is what makes `k: \|`
      alone work — it is otherwise a desync on trivially valid YAML, and under
      `strict-provenance` that is a CI panic. **No span is changed.** Measured:
      the rule fixes `k: \|` and leaves every other shape at zero desyncs.
      The sibling defect — an empty block scalar followed by another key spans
      *that key* — is explicitly out of scope (§ Empty and degenerate scalars).
      — Done in T10 (`at_empty_block_header`, both clauses); review confirmed
      the predicate is correctly **suppressed** on the two
      content-starts-with-pipe shapes.
- [x] **Fix the misleading doc comments** on `value_span` **and `key_span`**
      (`yaml_with_source_info.rs:90-96`) — they carry the same sentence and the
      same defect. "Source location of just the value" is what invited the
      misuse; say each covers the source text *including delimiters* and point
      at `content_source_info`. Both fields are literally
      `key.source_info.clone()` / `value.source_info.clone()`
      (`parser.rs:581-582`), so this item is documentation only; no field is
      added (§ Third-party exposure). — Done in T11+12 (commit `d046467`).
- [x] Document that aliases read as scalars with **no** content provenance
      (`None`, not a zero-length `Some` — § Out of scope) — Done in T11+12:
      replaced the stub-sounding TODO on the `Event::Alias` arm with the
      design rationale (two defensible answers, neither implemented; no code
      change, `new_scalar` already defaults to `None`).
- [x] Return `None` from the walker on desync, and panic instead under
      `strict-provenance` (§ Desync policy). There is **no `exact` flag** — if
      you find yourself adding one, re-read that section. — Done in T11+12:
      the two asserts (unconditional length tripwire, feature-gated desync
      panic naming both cursor offsets) landed in the `Event::Scalar` arm.
      **This is also what surfaced the walker desync bug** fixed by the
      style-conditional rule-1 entry — see § How the pieces are derived and
      § Evidence, Phase 2.
- [x] Add the `strict-provenance` feature (off by default).
      **`crates/quarto-yaml/Cargo.toml` has no `[features]` table today**, so
      this creates one. No passthrough feature is needed in
      `quarto-yaml-validation`: it depends on `quarto-yaml` via
      `{ workspace = true }`, and `--features quarto-yaml/strict-provenance`
      from the virtual root enables it graph-wide. The feature gates the
      **desync response** (panic with both cursor offsets vs. contiguous
      fallback) and two asserts: `debug_assert_eq!(si.length(), decoded.len())`
      — now **unconditional**, since a `Some` is always byte-exact — and that
      any scalar whose derivation ran is `Some`, which is what replaces the
      compiler enforcement the additive design gave up. Both live in the
      `Event::Scalar` arm, where yaml-rust2's `value: String` is in scope;
      `new_scalar` only ever sees the resolved `Yaml` and cannot check either.
      — Done in T11+12.
- [x] **Add the CI step** — the feature alone is inert, because `ci.yml` runs
      no `--all-features` (verified: the `test` job runs
      `cargo test --workspace --locked` at `ci.yml:34`):
      ```yaml
      - name: Test with provenance invariant enforced
        run: cargo test --workspace --locked --features quarto-yaml/strict-provenance
      ```
      A new **step** in the existing `test` job (not a new job): the VM is
      already warm, and it inherits the OS matrix
      (`ubuntu`/`macos`/`windows`, `ci.yml:19`) — which matters, because CRLF
      handling makes this exactly the kind of invariant that catches a
      Windows-specific provenance bug. Confirm the
      `--features <pkg>/<feat>` form resolves from the virtual workspace root.
      Document in the code that the length half is a **tripwire, not a
      proof**: a `Concat` can tile the right total while pointing at wrong
      ranges, and under the lockstep derivation the length check is nearly
      tautological — desync detection is the load-bearing check. — Done in
      T11+12: a new step in the existing `test` job (inherits the OS matrix),
      wired in **while red** (Ruling R-H) — see § Evidence, Phase 2 for why
      that was the right call rather than holding the step back.
- [x] Add `--features quarto-yaml/strict-provenance` to the **clippy**
      invocation too (`ci.yml:54` is
      `cargo clippy --workspace --all-targets --locked -- -D warnings`), or the
      desync-panic branch ships unlinted — Done in T11+12.
- [x] **No call-site updates are needed** — `quarto-yaml-validation`'s 22 test
      sites, q2's 15 and `quarto-yaml`'s own 5 all compile untouched under the
      additive design (§ Third-party exposure). If you find yourself editing
      them, the design has drifted back to the breaking variant; stop and
      re-read that section. — Confirmed: no call-site edits anywhere, across
      T8, T9, T10 and T11+12; each task's review independently re-checked
      this by grep/diff, not by trusting the report.
- [x] **Reconcile Plan 2** — *discharged 2026-08-21.* Plan 2's owner rewrote it
      on what was then `review/provenance-plan-2` (now merged into
      `feature/yaml-provenance`) and confirmed taking the obligations then
      listed (eight at the time) and all three corrections. **The list has since
      grown to ten** — obligation 8 (`qmd-syntax-helper`) and obligation 10 (the
      q2 lock refresh) postdate that confirmation, so do not read this checkbox
      as "the hand-off is settled"; read it as "the reconcile pass happened".
      Reconcile the count against Plan 2's receipt table, not against this box (the `quarto-yaml 0.2.0` lock refresh, the
      15-call-site item, and the gate). Do **not** re-apply those edits from the
      line numbers an earlier draft of this item cited — they were read against
      the pre-rewrite file and now point at unrelated text. Verify against Plan
      2's current ref rather than a diff from `816f4ed47`.
- [x] Publish `quarto-source-map` **0.1.3** (the builder) first — it is this
      phase's dependency and Phase 1 deliberately held it back — Done in T14
      (commit `4ec38a4`, amended to `09ec6d1` for a wording fix): pushed
      `provenance-builder`, PR posit-dev/quarto-source-map#4, CI green,
      **merged and published 2026-08-21T22:57:16Z, tag `v0.1.3`** (Gordon's
      approval, "Do both releases now").
- [x] Declare `quarto-source-map = "0.1.3"` in `~/src/quarto-yaml/Cargo.toml`
      (not `"0.1.0"` — see § Both crates ship as patches; a published crate must
      require the version its code needs), then bump the shared workspace
      version to `0.1.3` for **both** publishable crates, since the release
      workflow hard-fails when they disagree; `cargo update -p quarto-source-map`
      and commit the lockfile (the workflow runs `cargo publish --locked`);
      **no q2 `quarto-yaml` edit** — `^0.1.2` already accepts it;
      PR → CI green → **merge to `main`, which is what publishes** — Done in
      T15 (commit `4734b46`): all five steps verified by the controller
      independently (not just the report) against the manifests themselves;
      `cargo package --workspace --locked` clean; pushed `content-provenance`,
      PR posit-dev/quarto-yaml#18, CI green (Windows run time consistent with
      the suite running twice — normal + `strict-provenance` — across the
      matrix), **merged at 2026-08-21T23:04:45Z**. Published: `quarto-yaml`
      0.1.3 (2026-08-21T23:05:40Z) and `quarto-yaml-validation` 0.1.3
      (23:05:44Z), tag `v0.1.3`. All three releases of this plan are live.

## Evidence

_A phase is not done until its evidence is here._

### Phase 0

**Bullet 1 — the repro panics on stock `main`.** Run 2026-08-21 against the
released binary (`/Users/gordon/.local/bin/q2`, `q2 (quarto 2) 0.24.0`), which
resolves `quarto-error-reporting = "0.2.1"` from crates.io — i.e. without
`4da3385`:

```
$ cd .scratch/ariadne-emoji-panic/repro && q2 render
… exit=101
thread 'main' (50946897) panicked at …/ariadne-0.6.0/src/write.rs:84:59:
end byte index 37 is not a char boundary; it is inside '✨' (bytes 35..38 of string)
```

Byte arithmetic re-derived from the fixture, confirming § The bug class:
opening quote at file byte 81, content starts at 82, `✨` occupies 102..105,
`</span>` begins at 105. Line 7 starts at 67, so the panic's line-relative
`37` / `35..38` are file offsets `104` / `102..105`. q2 computed `81 + 23 =
104`; the truth is `82 + 23 = 105`.

**The fixture, transcribed because it is not committed.** `.scratch/` is
ignored through `.git/info/exclude` (local, unshared), so this is the only
durable copy. Two files, in `.scratch/ariadne-emoji-panic/repro/`:

`_quarto.yml` —

```yaml
project:
  type: website
website:
  title: "T"
  navbar:
    left:
      - text: '<span id="x">Ask AI ✨</span>'
        href: index.qmd
```

`index.qmd` —

```qmd
---
title: "Index"
---

body
```

The `control/` sibling is identical except that the `✨` moves to the front of
the `text:` value (`'<span id="x">✨ Ask AI</span>'`), which renders clean at
exit 0 — the emoji's *position* relative to the span end is what matters, not
its presence.

Because the released binary is immutable and the fixture is now recorded here,
this observation is re-runnable indefinitely; see Phase 0's framing note.

**The canonical accumulating-drift fixture, measured 2026-08-21.** This is the
one the epic's cited numbers come from — `.scratch/blockscalar/_quarto.yml`,
transcribed here because `.scratch/` is not committed:

```yaml
project:
  type: website
website:
  title: "T"
  page-footer:
    center: |
      line one
      line two
      <span id="y">Footer</span>
```

`q2 render` exits **0** and reports two `Q-2-9` warnings at **`8:10`** and
**`9:14`** — the exact pair the epic quotes. Both tags are on **line 9**, at
1-based columns **7** and **26**. The drift is 2 preceding content lines × 6
bytes of stripped indent = **12**: column 26 − 12 = 14, and column 7 − 12 falls
before line 9 begins, so the first warning is attributed to **line 8**. Plan 2's
§ Evidence carries the same transcription.

**A four-warning variant, for the per-element arithmetic.** The epic's headline
symptom — a diagnostic attributed to the **wrong line** — had no reproducible
fixture in any of the three plans until now. The crash repro above cannot
produce it: its `text:` is a *single-line single-quoted* scalar, so its drift is
the constant −1, not the accumulating kind. Built and measured with the released
`q2 0.24.0`, at `.scratch/ariadne-emoji-panic/accum/`:

```yaml
project:
  type: website
website:
  title: "T"
  page-footer:
    center: |
      line one
      line two
      <span id="x">three</span> and <span id="y">four</span>
```

(plus the same trivial `index.qmd`). `q2 render` exits **0** and prints four
`Q-2-9` warnings. All four `<span>` tags are on **line 9**, at 1-based columns
**7, 25, 37, 54**. q2 reports them at:

| element | truth | reported | drift |
|---|---|---|---|
| `<span id="x">` | 9:7 | **8:10** — *wrong line* | −12 |
| `</span>` | 9:25 | 9:13 | −12 |
| `<span id="y">` | 9:37 | 9:25 | −12 |
| `</span>` | 9:54 | 9:42 | −12 |

The drift is exactly **2 preceding content lines × 6 bytes of stripped indent =
12 bytes**, which is the accumulating mechanism in § Findings stated as a
number. For the first element, 12 bytes back from column 7 lands before the
start of line 9 — hence `8:10`, a diagnostic pointing at the wrong line of the
user's file. That is why this bug class is worse than an off-by-one, and it is
now demonstrable from the repo rather than asserted.

Note the key: **`page-footer.center`** — the measured one, and what Plan 2 and
the epic use.

**Bullet 2 — the patched render succeeds.** Run 2026-08-21 against a build of
*this worktree* with the `[patch.crates-io]` override active (`cargo build
--bin q2`, then `target/debug/q2` — not the released binary):

```
$ cd .scratch/ariadne-emoji-panic/repro
$ /Users/gordon/src/q2/.worktrees/workspace-1/target/debug/q2 render
… exit=0
Warning: [Q-2-9] HTML element converted to raw HTML
  … /repro/_quarto.yml:7:15 …
Warning: [Q-2-9] HTML element converted to raw HTML
  … /repro/_quarto.yml:7:36 …
Rendered 1 of 1 files to …/_site — 2 warnings
```

Exit 0, both `Q-2-9` warnings printed, matching the phase's expectation
exactly.

**Bullets 3-6 — attribute each hunk of `4da3385` separately.** All four
experiments below are upstream unit-test experiments against
`~/src/quarto-error-reporting`, run with `cargo test --all-features` (needed
to compile both the ariadne and annotate-snippets target tests — confirmed
against the manifest's `[features]` block, `default = ["ariadne"]`,
`annotate-snippets = ["dep:annotate-snippets"]`). No q2 build is involved.
Baseline (clean checkout at `4da3385`) was confirmed green for both target
tests before starting. After each experiment, `git checkout --
src/diagnostic.rs` restored the checkout and `git status --short` was
confirmed empty before starting the next.

- **Site 1 — the ariadne `Report::build` anchor.** Edit: reverted only the
  anchor's span argument from `main_span.start..main_span.start` back to
  `start_mapped.location.offset..start_mapped.location.offset`, leaving the
  `main_span` computation (and its use in the main label below) snapped.
  Command: `cargo test --all-features -- ariadne_span_starting_inside_multibyte_char_does_not_panic annotate_snippets_span_starting_inside_multibyte_char_does_not_panic`.
  Result: the ariadne test goes red (annotate-snippets stays green), with

  ```
  thread '...ariadne_span_starting_inside_multibyte_char_does_not_panic' panicked at
  .../ariadne-0.6.0/src/write.rs:267:40:
  end byte index 21 is not a char boundary; it is inside '✨' (bytes 19..22 of string)
  ```

  This *does* land at `write.rs:267` — the commit message's prediction — which
  differs from the phase's framing that "the only recorded panic is
  `write.rs:84:59`" (that framing was about the *end-to-end q2 repro*, bullet 1,
  not this per-hunk experiment). Recorded as observed: this experiment's panic
  is at `write.rs:267`, not `:84`.

- **Site 2 — the ariadne main label, anchor still snapped.** Edit: shadowed
  `main_span` immediately before its use in the label (and `with_order`) with
  the raw, unsnapped `start_mapped.location.offset..end_mapped.location.offset`,
  added *after* the `Report::build` call so the anchor above still executes
  with the snapped span. Same command as site 1. Result: the ariadne test goes
  red, annotate-snippets stays green:

  ```
  thread '...ariadne_span_starting_inside_multibyte_char_does_not_panic' panicked at
  .../ariadne-0.6.0/src/write.rs:84:59:
  end byte index 21 is not a char boundary; it is inside '✨' (bytes 19..22 of string)
  ```

  Matches the phase's expectation exactly (`write.rs:84`).

- **Site 3 — the ariadne detail label.** Edit: reverted only `detail_span`'s
  computation from `Self::snap_span_to_char_boundaries(&content,
  detail_start.location.offset, detail_end.location.offset)` back to the raw
  `detail_start.location.offset..detail_end.location.offset`. Same command as
  above. Result: **both tests stay green** —

  ```
  test diagnostic::tests::annotate_snippets_span_starting_inside_multibyte_char_does_not_panic ... ok
  test diagnostic::tests::ariadne_span_starting_inside_multibyte_char_does_not_panic ... ok
  test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 67 filtered out
  ```

  Neither target test's fixture builds a `DetailItem` with a location, so
  neither test reaches the detail-label path — this is the "both tests stay
  green" outcome the phase called out as legitimate. It means these two tests
  do not attribute the detail-label hunk at all; that hunk's load-bearing-ness
  is unverified by this pair.

- **Site 4 — the annotate-snippets clamp closure.** Edit: reverted the
  `clamp` closure from `Self::snap_span_to_char_boundaries(&content, start,
  end)` back to the pre-`4da3385` `let s = start.min(content_len); let e =
  end.min(content_len).max(s); s..e` (reintroducing the removed `let
  content_len = content.len();` binding). Same command as above. Result: the
  annotate-snippets twin goes red, ariadne stays green:

  ```
  thread '...annotate_snippets_span_starting_inside_multibyte_char_does_not_panic' panicked at
  .../annotate-snippets-0.12.16/src/renderer/source_map.rs:71:13:
  end byte index 21 is not a char boundary; it is inside '✨' (bytes 19..22 of string)
  ```

  Matches the phase's expectation exactly (the `:1635` twin goes red).

Final state: `~/src/quarto-error-reporting` confirmed clean at `4da3385`
(`git status --short` empty, `git log -1` shows `4da3385`) after the last
experiment — nothing was committed or reverted-via-`git revert` in that repo.

**Local override removed.** The three added `[patch.crates-io]` lines
(comment ×2 + `quarto-error-reporting = { path = … }`) were deleted from q2's
`Cargo.toml`; `Cargo.lock` was regenerated to match. `git diff Cargo.toml
Cargo.lock` in the q2 worktree is empty.

### Phase 1

**The q2 smoke, run 2026-08-21.** `quarto-source-map` at
`/Users/gordon/src/quarto-source-map` confirmed on branch
`char-boundary-and-concat-fixes`, clean tree, tip `0e900e2` ("Return None
from `preimage_in`'s `Substring` arm over a `Concat` parent"), with the four
behavior-fix commits all present (`8e07717` floor, `022f489` free-function
agreement, `0c65d52` `Concat` exclusive-end, `0e900e2` `preimage_in`) and a
`grep -rn "ProvenanceBuilder"` across the repo returning nothing — the branch
carries no unreleased API, so this smokes what actually ships as 0.1.2.

In the q2 worktree, added to the existing `[patch.crates-io]` block:

```toml
[patch.crates-io]  # LOCAL DEV ONLY — do not commit
quarto-source-map = { path = "/Users/gordon/src/quarto-source-map" }
```

`cargo metadata` initially still resolved `quarto-source-map` to the
registry `0.1.0` — the committed `Cargo.lock` predates the patch and Cargo
does not re-resolve a locked package on its own. `cargo update -p
quarto-source-map` picked up the patch (`Adding quarto-source-map v0.1.1
(/Users/gordon/src/quarto-source-map)`), confirmed via `cargo metadata`
before the run: `source` empty (path dependency), `version` "0.1.1" — the
checked-out tree's own `Cargo.toml` version, one behind the 0.1.2 this phase
bumps to later, which does not affect what's being smoked since the four
fixes are commits, not a version bump.

Ran `cargo nextest run --workspace` from the worktree root, redirected to
`.scratch/task4-nextest.log` (not piped through `tail`, per this repo's
hang warning):

```
$ cargo nextest run --workspace > .scratch/task4-nextest.log 2>&1
     Summary [ 116.499s] 12859 tests run: 12859 passed (1 leaky), 198 skipped
```

**Nothing moved.** Zero failures (`grep -c FAIL` on the log: 0), zero
skipped-beyond-baseline, and `git status --short` after the run shows only
`Cargo.lock`/`Cargo.toml` changed (the patch itself) — no `.snap`, `.snap.new`,
or any other file touched. `cargo insta` is not an installed subcommand in
this checkout, so snapshot enumeration used `git status` instead, per the
brief's fallback; it is conclusive here because zero files changed at all.

**The two predicted reactions, individually:**

- **`crates/pampa/tests/integration/test_location_health.rs:448`** — the free
  `quarto_source_map::utils::offset_to_location(source, location.offset)`
  call inside `validate_offset_row_col_consistency`, invoked from every
  `test_core_properties_*` variant (including
  `test_core_properties_on_smoke_tests`, the broadest fixture sweep). All
  ran and passed:
  ```
  PASS [   0.027s] pampa::integration test_location_health::tests::test_core_properties_on_smoke_tests
  PASS [   0.025s] pampa::integration test_location_health::tests::test_core_properties_simple
  ```
  **Green**, not red. Per the brief's framing this is not evidence either
  way about where those `Location`s' row/column originate — it means this
  suite's existing `Location` values already sit on char boundaries with
  row/column consistent with a floored offset, not that the two
  implementations agree in general.
- **JSON-writer `"o"` snapshots** (`crates/pampa/src/writers/json.rs:550`,
  `:555`, `:2005`, `:2014`, `:2258`, `:2267`) — all `writers::json` unit
  tests passed, including the source-info-pool tests
  (`test_source_info_pool_concat`, `test_source_info_pool_substring_of_generated`,
  `test_source_info_pool_generated_with_invocation_anchor`, etc.), and
  `git status --short` shows no snapshot file touched at all. **Silence**,
  matching a "no fixture carries a mid-character offset" outcome rather than
  a churned-and-reverted one — there was nothing to revert.

**The `Concat` exclusive-end change: no reaction, and that is not coverage.**
`crates/pampa/tests/integration/qmd_writer_source_info.rs` ran in full
(`concat_piece_lengths_sum_to_buffer_length`,
`map_offset_resolves_block_in_single_file`,
`round_trip_code_block_offset_accuracy`,
`concat_covers_output_with_frontmatter`, `blocks_from_different_files_map_correctly`)
and stayed green, but as the brief predicts this file exercises only
interior `map_offset` positions — it asserts piece-length sums, not an
exclusive-end offset. `quarto-core/src/stage/stages/engine_execution.rs:733`,
the only production consumer of the QMD writer's provenance `Concat`, has no
dedicated exclusive-end assertion in this suite either. A fully green
workspace run is silence, not a demonstration that the exclusive-end fix is
safe for that call site; Plan 2's Phase 2 adds the assertion this run cannot
supply.

**Cleanup.** Removed the two added lines (comment + path entry) from q2's
`Cargo.toml`. `cargo update -p quarto-source-map` failed afterward
(`package ID specification 'quarto-source-map' did not match any packages`)
rather than re-resolving to the registry version — the Cargo.lock entry left
by the patch (`version = "0.1.1"`, no `source`/`checksum`, i.e. a path
dependency with no matching `[patch]` entry to resolve it) confused cargo's
package-spec matching for `update -p`. Restored `Cargo.lock` with `git
checkout -- Cargo.lock` instead (the diff was the isolated three-line
version/source/checksum stanza for this one package, against an
otherwise-clean pre-task tree, so a straight checkout is exact). Verified
with `cargo metadata` afterward: `quarto-source-map` resolves to `version:
"0.1.0"`, `source: "registry+https://github.com/rust-lang/crates.io-index"`,
no patch-related warnings. `git diff Cargo.toml Cargo.lock` in the q2
worktree is empty; `git status --short` is clean. The
`/Users/gordon/src/quarto-source-map` checkout was not modified — still on
`char-boundary-and-concat-fixes` at `0e900e2`, clean tree.

**The `Location` audit, run 2026-08-21.** Task 6's scope: the third
implementation `offset_to_location_bytes` (q2), and `quarto-yaml`'s own
`Location` uses. Read-only — no fixes anywhere; only a temporary scratch
`#[test]` was added to q2 to measure behavior, then reverted with `git
checkout --` before this write-up (confirmed via `git status --short`).

*Target 1 — `offset_to_location_bytes`, q2
`crates/quarto-parse-errors/src/error_generation.rs:330-350`.*

Read: the function walks `input[..offset]` byte-by-byte counting `\n` to
find `row`/`line_start`, then computes `column` as
`String::from_utf8_lossy(&input[line_start..offset]).chars().count()`, and
sets `Location.offset` to the raw `offset` parameter unconditionally — no
floor loop anywhere in the function.

Measured: added a temporary `#[test]` inside the file's existing
`#[cfg(test)] mod tests`, calling `offset_to_location_bytes` directly on
`"x = 'A✨B'"` (✨ = U+2728, 3 bytes at 6..9 — the same fixture and offset
this epic's comparison table uses), run via `cargo test -p
quarto-parse-errors scratch_task6_mid_char_offset_measurement --
--nocapture`, then reverted with `git checkout --
crates/quarto-parse-errors/src/error_generation.rs` (confirmed clean
afterward: `git status --short` empty). Results:

| input offset | position | `Location.offset` | `.column` |
|---|---|---|---|
| 6 | boundary (start of ✨) | 6 | 6 |
| 7 | mid-char (2nd byte of ✨) | 7 | 7 |
| 8 | mid-char (3rd byte of ✨) | 8 | 7 |
| 9 | boundary (start of 'B') | 9 | 7 |

Concluded: at the mid-character offsets (7, 8), `offset_to_location_bytes`
returns the **raw** offset (not floored to 6) and **overcounts** the column
(7, not 6) — the partial multi-byte sequence inside `input[line_start..offset]`
decodes via `from_utf8_lossy` to one `U+FFFD`, which `.chars().count()` counts
as a whole character rather than stopping before it. This **disagrees** with
both `quarto-source-map` implementations after Task 3's fix, which floor to
offset 6 / column 6 for the same input. At the boundary offsets (6, 9) all
three implementations agree, so the disagreement is specifically the
mid-character case this epic's bug class targets, not a distinct new issue.

On whether this is reachable in production (read, not measured — this is
the "is it exposed" question, which belongs to Plan 3's audit, not this
task's rule-comparison): all four production call sites
(`error_generation.rs:122,128,203,210`) pass offsets derived from
`calculate_byte_offset` (tree-sitter's own `row`/`column`) and
`advance_chars` (a codepoint-respecting walk that never stops mid-sequence),
plus ASCII-space trimming that moves only by whole single-byte characters.
For valid UTF-8 input, tree-sitter's lexer decodes whole codepoints while
scanning, so these call sites are not currently observed to hand this
function a mid-character offset. That is an emergent property of tree-sitter
and this crate's own byte-walking helpers, not an invariant
`offset_to_location_bytes` enforces itself, and this task did not construct
an adversarial tree-sitter parse to try to falsify it.

Routing: **q2-side disagreement → Plan 3's audit.** Plan 3
(`claude-notes/plans/2026-08-20-provenance-3-audit-and-fix.md:202-214`,
"Discharge Plan 1's hand-off") already has a checklist item shaped exactly
for this measurement; no braid strand filed, per this repo's rule that
in-scope plan work is never a tracker item.

*Target 2 — `quarto-yaml`'s own `Location` uses, repo
`/Users/gordon/src/quarto-yaml` (clean, `main` @ `c7b8a40`).*

Read, across both crates in the workspace:

- `crates/quarto-yaml/src/parser.rs` — the only production (non-test,
  non-doctest) `Location` literal is the fallback parent `SourceInfo` built
  in `parse_impl` (`:129-143`): `Location { offset: 0, row: 0, column: 0 }`
  and `Location { offset: content.len(), row: content.lines().count() - 1,
  column: <last line's length> }` — both boundary offsets (start-of-file,
  end-of-file) by construction. Every other `Location {` literal in the file
  is inside `#[cfg(test)] mod tests` or the `parse_with_parent` doc example.
- The parser never builds a `Location` for parsed *nodes* — `make_source_info`
  / `make_source_info_at_offset` (`:344-375`) build `SourceInfo::original`/
  `SourceInfo::substring` directly from byte offsets. The comment at
  `:353-355` states rows/columns are derived later, by `SourceContext`, "so
  there is nothing to compute here" — quarto-yaml deliberately does not
  reimplement offset→`Location`.
- Those byte offsets come from `byte_offset(&Marker)` → `byte_offset_of_char`
  (`:264-298`), converting yaml-rust2's *character* index
  (`Marker::index()`, mislabeled in its own docs per the comment at `:259`)
  to a byte offset by walking whole codepoints one at a time, testing
  `(b & 0xC0) != 0x80` for a continuation byte. It only ever stops on a
  genuine UTF-8 boundary — a node's start offset cannot land mid-character.
- `crates/quarto-yaml-validation/src/error.rs:373-380` (`with_yaml_node`) is
  the only place in the second crate that touches a `Location`: it reads
  `.row`/`.column` off `SourceInfo::map_offset(0, ctx)`'s result to populate
  its own **locally-defined** `SourceLocation` struct (`:520`) — not
  `quarto_source_map::Location`, and not a new offset→row/column rule.
  `map_offset` is `quarto_source_map::SourceInfo::map_offset`
  (`quarto-source-map/src/mapping.rs:17-44`), which for the `Original` case
  calls `file_info.offset_to_location(absolute_offset, &content)` —
  `FileInformation::offset_to_location`, the **first upstream implementation
  this epic's Task 3 already fixed to floor**. The offset it's called with
  is the literal `0` (relative to the node's own `SourceInfo`), which
  resolves to that node's start offset — itself always boundary-aligned per
  the point above.

Measured: nothing further needed a scratch experiment — neither crate
reimplements offset→`Location` logic; one avoids `Location` entirely
(byte-offset-only `SourceInfo`), the other delegates to the already-patched
upstream function on an offset that is structurally a boundary (`0`
relative to a node that itself starts on a boundary).

Concluded: **no disagreement found, for this reason** — `quarto-yaml` (both
crates) has no third implementation of offset→`Location` to disagree with
the two `quarto-source-map` implementations. The `Location` literals present
are either test/doctest illustration or boundary constants (`0`,
end-of-file).

Routing: nothing to route — no braid strand filed. The
`/Users/gordon/src/quarto-yaml` checkout was not modified (still on `main`
at `c7b8a40`, clean tree, verified again after this write-up).

### Phase 2

**Stub, then red, then green.** The API surface landed first as a stub
(`Children::Scalar { content_source_info: Option<SourceInfo> }`,
`with_content_provenance`, `content_source_info()`), with the parser
attaching nothing — every scalar read `None`. Against that stub, 42 tests
were written and confirmed **RED** (`content_source_info()` returning `None`
where the fixtures note says `Some`; 73 pre-existing `quarto-yaml` tests
stayed green throughout). The lockstep walker then turned all 42 **GREEN** in
one commit (`8b8c05e`), with the piece lists unedited from what the tests
already asserted — reviewed as the single most consequential check of the
phase, specifically because "42/42 green" is also what a walker that special-
cased its way to matching the fixtures would produce; the reviewer
independently re-derived several shapes by hand rather than trusting the
green run.

**The `strict-provenance` feature earned its keep immediately.** Once the
feature, its two asserts (an unconditional length tripwire, a feature-gated
desync panic naming both cursor offsets) and the CI step existed, running the
existing test suite under `--features quarto-yaml/strict-provenance` — not
just the 42 dedicated provenance tests, but every scalar parsed anywhere in
the workspace's tests — panicked in a pre-existing, unrelated span test
(`parser::tests::test_plain_scalar_spans`, fixture `"key: a \n  b"`, decoded
`"a b"`). Traced by hand: rule 3 (verbatim) matched the trailing space
(`' '==' '`) one iteration before rule 1 (break) could recognize the fold was
starting, because rule 1's entry test at the time required the source cursor
to already be *at* `\n`/`\r`; by the time the walk reached the real break, the
value cursor had nothing left to absorb it with, and rule 4 rejected the
non-newline remainder as a desync. This is the finding recorded and fixed in
rule 1 of § How the pieces are derived (the entry test became
**style-conditional**: wide for flow styles, narrow for block). **Ruling
R-H — wired the CI step in while it was red, deliberately**, rather than
holding it back until the fix landed: a red step naming a real bug is the
step working, and withholding it would have been the "green CI mistaken for
verification" failure the plan itself warns against. The branch was unpushed,
so no shared CI was affected by the interim red state.

**The fix took two rounds.** Round 1 tried the obvious universal widening and
was correctly **BLOCKED**: it fixed the reported case but desynced a
previously-green fixture (`block_pipe_trailing_spaces_last_line`), and two
narrower alternatives were tried and rejected in the same round, each
falsified by `block_pipe_more_indented_line` — see § Reversed decisions, R21.
Round 2 landed the sound fix, scoping the widening to flow styles only via a
`wide_entry: bool` parameter threaded alongside `indent` (`quarto-yaml`
`778225d`; q2 generator `eeb1abf57`, keeping the committed oracle in
lockstep so the walker and the fixtures note cannot drift apart). Also
performed, as instructed rather than added speculatively: a probe of folded
`>` scalars, which confirmed they do **not** need the wide entry, because `>`
folding replaces only the break byte and — unlike flow folding — does not
strip a line's trailing whitespace. Recorded as a probe in the fixtures note,
not as a fixture row.

**Final state, verified independently of the implementer's report at each
step:** **43** provenance tests green (42 original + 1 new,
`plain_multi_line_trailing_space_before_fold`), piece lists on the original 42
byte-for-byte unchanged. `cargo test --workspace --locked` and
`cargo test --workspace --locked --features quarto-yaml/strict-provenance`
both green (114+8 doctests, then 116 after the new test), including
`test_plain_scalar_spans` under the feature. `cargo clippy --workspace
--all-targets --locked --features quarto-yaml/strict-provenance -- -D
warnings` clean — CI's exact invocation, run locally before trusting CI to
say so.

**Publication, in dependency order, both approved 2026-08-21 ("Do both
releases now"):**

- `quarto-source-map` **0.1.3** (`ProvenanceBuilder`) — PR
  posit-dev/quarto-source-map#4, CI green, merged **2026-08-21T22:56:35Z**,
  published to crates.io **2026-08-21T22:57:16Z**, tag `v0.1.3`.
- `quarto-yaml` **0.1.3** and `quarto-yaml-validation` **0.1.3** (the content-
  provenance feature) — PR posit-dev/quarto-yaml#18, CI green on all four
  checks (Windows ran ~1m38s against ~40s elsewhere, consistent with the test
  job running the suite twice — plain and `strict-provenance` — across the OS
  matrix), merged **2026-08-21T23:04:45Z**, published
  **2026-08-21T23:05:40Z** / **23:05:44Z**, tag `v0.1.3`. Verified against the
  manifests directly before release: `Cargo.toml` declares
  `quarto-source-map = "0.1.3"` (not `"0.1.0"`), the shared workspace version
  is `0.1.3` for both publishable crates, and `Cargo.lock` resolves
  `quarto-source-map` from the real registry, not a path.

All three releases this plan promises are now live.

**Deferred minors and cross-plan findings, collected here so a future reader
finds them in one place** (the ledger has the full per-task list; these are
the substantive ones per the hand-off instruction):

- **The break-region "value at a tab" entry sub-case still has no fixture.**
  Flagged in the Phase 1 design-review walkthrough (walkthrough 1, above) when
  32 fixtures existed, and confirmed still true after Phase 2's 42: no shape
  in either set has a value-side tab at a break position. Optional per the
  brief; not fixtured in either phase.
- **Two comrak findings for Plan 3**, both already recorded in the Phase 1
  design-review walkthrough 7, above: `tokenize_text_with_source` misplaces
  every token after an entity (a uniform short-shift, traced against
  `x&amp;y z`), and the `NodeValue::Escaped` match arm
  (`comrak-to-pandoc/src/inline.rs:96-100`) is unreachable under this crate's
  comrak configuration, because backslash-escape coalescing is on by default
  and no q2 option turns it off.
- **q2's `offset_to_location_bytes` disagrees with both upstream
  implementations** after 0.1.2 (raw, unfloored offset; overcounted column).
  Already recorded in § Evidence, Phase 1 (Task 6's audit) and routed to
  Plan 3's existing checklist item
  (`2026-08-20-provenance-3-audit-and-fix.md:202-214`).

## Appendix — reversed decisions

Every position this plan held and abandoned, with what killed it. The body is in
the present tense and does not re-argue these; this is where a reader who wants
to propose one of them finds out it was already tried. Entries are ordered by
how likely someone is to re-propose them, not chronologically.

**R1 — `preimage_in` should keep the affine path when every piece is
length-matched.** Proposed twice: once as a refinement to the `Substring` fix,
once (by this plan) as `finish()`'s collapse rule. Killed by measurement: a 1→1
fold — source `\n`, content one space — is length-matched with *different
bytes*, so the predicate admits a hull that licenses copying the wrong bytes.
Reachable in `aaa`⏎`bbb` as a root plain scalar. **This is the single most
likely thing to be re-proposed**, and after 0.1.2 it will arrive disguised as
restoring an over-conservative change; see the do-not in § `preimage_in`
composes affinely.

**R2 — zero-content pieces should be dropped.** Held briefly. Killed because
dropping a deletion breaks the source-tiling invariant, leaving a gap where the
deleted bytes were. Its original justification — that storing one would route
`map_offset(length())` into the deleted source's start — was true only against
the *unfixed* exclusive-end branch, which this plan fixes. A rationale that
outlived the defect it cited.

**R3 — `finish()` collapses when the pieces tile one contiguous source range
with equal totals.** A restatement of R1. Killed the same way.

**R4 — `content_source_info()` returns a contiguous fallback plus an `exact`
flag.** Withdrawn because `is_scalar()` already carries "not a scalar", so
`None` was never overloaded; because nothing would branch on the flag (every
consumer must decline sub-offset arithmetic either way); and because it shipped
this epic's bug class as a documented mode — provenance present, correctly
typed, silently the old wrong base.

**R5 — the `quarto-yaml` change is breaking, so it needs 0.2.0.** Killed by
`Children` being a *private* enum plus the existing `with_tag` idiom: provenance
attaches after construction, no public signature changes, 42 call sites
untouched. See R8 for the version consequence.

**R6 — a `Concat` reaches TypeScript as a byte-0 range.** Asserted on inference
from the JSON writer emitting `(0, sum_of_piece_lengths)`, and written into
§ Risks before anyone opened the TS file. False: `resolveChain`'s `Concat` arm
walks the pieces. The real defect is its `Substring` arm composing affinely —
R1's error, in a second language.

**R7 — `preimage_in` has 33 production call sites.** 26. The 33 summed
`grep -c` lines for one file with call-*regions* for another: two units added
together.

**R8 — `quarto-yaml` needs 0.2.0 so a breaking change can't be swallowed by
`^0.1.2`.** Sound reasoning, dead premise: R5 removed the breaking change.
Now 0.1.3.

**R9 — `quarto-yaml` needs no dependency edit.** Wrong: it declares
`quarto-source-map = "0.1.0"`, `^0.1.0` is satisfied by 0.1.0, and q2's lockfile
pins exactly that — so a published crate whose code calls `ProvenanceBuilder`
would fail to compile downstream and on docs.rs. It must declare `"0.1.3"`.

**R10 — the alias arm should carry a zero-length `Some`.** Its rationale was
"the compiler will demand a value once the signature changes"; R5 removed the
signature change. `None` is the honest answer, and the two texts had picked
opposite values for an accessor whose `Some`/`None` distinction this plan calls
load-bearing.

**R11 — the `ConfigValue` content field serves exactly one context.** True while
only `ProjectConfig` deferred markdown-izing. Front matter is now in scope for
Plan 2's Phase 3, so the three immediate re-parse sites are in scope too. Do not
cite the narrowing to argue the field away.

**R12 — the prototype lives inline in this plan.** Two copies diverged; the
inlined one lost the verbatim tag and so implied the unsound length-based
coalescing. One committed copy now, and it is the one that generated the
fixtures.

**R13 — q2 "provably cannot" reconstruct the correspondence from a decoded
string plus a raw span.** False; the lockstep walk needs only (raw span text,
decoded value, block indent, escape mode), all obtainable at a consumer. The
placement argument is about ownership and exactness, not impossibility.

**R14 — the header-skip predicate is "the span's first byte is `|` or `>`".**
Desyncs on a block scalar whose *content* starts with a pipe — valid YAML, and
under `strict-provenance` a CI panic. Needs the byte test **and** an
empty-or-all-newlines value.

**R15 — the walker's four rules are evaluated verbatim-first.** Desyncs 9 of the
24 shapes then measured. Break and escape must precede verbatim, because the
bytes are *equal* in both cases and verbatim would consume them 1:1.

**R16 — `SourceInfo::substring` belongs in the affine-composition family.**
Declined: `substring` + `map_offset` composes *correctly* over a `Concat`
(measured). The founding defect is a coordinate-space error — the wrong parent
supplied — fixed by repointing, not by refusing. Plan 3 records the union of
both readings; § `preimage_in` composes affinely explains the non-merge.

**R17 — a content *span* plus a `content_is_verbatim` flag.** Structurally
incapable of describing a multi-line block scalar, which is runs of source
separated by stripped indentation. A span fixes the first run and nothing after
it; the flag existed only to admit that.

**R18 — the crash fixture demonstrates the accumulating drift.** It does not: it
is single-line and single-quoted, so its drift is the constant −1. The
accumulating case needs a multi-line block scalar; both fixtures are transcribed
in § Evidence.

**R19 — Phase 0's per-hunk attribution expires when Phase 1 lands.** It expires
when `quarto-error-reporting` picks up 0.1.2, which is a different and later
event, and it affects both per-hunk bullets rather than one.

**R20 — the `[patch.crates-io]` removal gates Plan 2.** It does not: the
override is an uncommitted edit in this worktree only, and Plan 2 runs on a
branch that is already clean.

**R21 — rule 1's break-region entry should widen universally (not
style-conditionally).** Tried first, during Phase 2 finishing, once
`strict-provenance` found the trailing-space-before-fold desync. Fixes that
shape but desyncs a previously-green one,
`block_pipe_trailing_spaces_last_line`: entering at the leading trailing space
makes the source run 4 bytes (`"   \n"`) while the value-side cap
(`vi + newlines.max(1)`) admits only 1 value byte, stranding 2. Two narrower
alternatives were also tried and rejected, both falsified by
`block_pipe_more_indented_line`: checking byte-identity against the
*uncapped* value run, and capping `ve` to `vi + (se - si)` instead of
`vi + newlines.max(1)` — each would merge a stripped base-indent range with a
preserved extra-indent range that happen to be byte-coincidentally equal,
which is exactly the "right length, wrong bytes" failure this epic exists to
prevent. The sound fix scopes the widening to flow styles only, leaving block
styles at the original narrow entry — see rule 1's current text.

## Appendix — the derivation prototype

**The prototype is committed, not inlined here.**
`claude-notes/research/yaml-content-provenance-walker/walker.rs`, with its
`Cargo.toml` and its two easy-to-get-wrong rules in the sibling README. Phase 2
starts from **that file**, not from this plan.

There is deliberately **one** copy. A previous inlined duplicate went stale in
exactly the way that matters — losing the verbatim tag, and so implying the
unsound length-based coalescing (§ Reversed decisions, R12).

**Inputs per style** — getting these wrong is the easiest way to manufacture a
false desync, and they live here because they are guidance rather than code:

| style | `raw` starts at | `indent` | `wide_entry` | `esc` |
|---|---|---|---|---|
| block (`\|`, `>`), normal | span start, running **to EOF** — the value can outrun the span | `marker.col()` | `false` | `None` |
| block, **empty body** | the newline ending the header line (§ Where the walk starts) | `marker.col()` | `false` | `None` |
| plain | span start | **0** | `true` | `None` |
| single-quoted | span start **+ 1** (skip the delimiter) | **0** | `true` | `Some('\'')` |
| double-quoted | span start **+ 1** (skip the delimiter) | **0** | `true` | `Some('\\')` |

Flow styles take `indent = 0` because folding strips all line-leading
whitespace in a flow scalar; only block styles have an indent to preserve. The
walk always terminates on the *value*, so the to-EOF `raw` for block styles is
an upper bound rather than an instruction to consume it.

**`wide_entry` added 2026-08-21, during Phase 2 finishing** (computed as
`!block`, threaded alongside `indent` in both the production walker and this
generator, so they cannot drift): widens rule 1's *entry* test from "the
source cursor is at `\n`/`\r`" to "the source cursor is at a whitespace run
that contains a newline" for flow styles only. See rule 1 in § How the pieces
are derived for the full rationale and § Reversed decisions, R21 for the
universal-widening attempt this rejected.

**Verification performed per shape:** the pieces tile the decoded value exactly
(`sum(out) == val.len()`), and every **verbatim** piece's source text equals the
corresponding slice of the value. Counts and per-shape piece lists live in the
fixtures note, which is the authority — this plan deliberately no longer repeats
the number.

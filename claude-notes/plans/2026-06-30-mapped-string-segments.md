# Plan 1b.1: MappedString `segments()` accessor — make piece provenance reachable

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Sequences:** landed after Plan 1b, **independently of Plan 1c.** It is a small,
self-contained correction to the Plan-2A §2aa `MappedString` library surface — a
**foundation fix, not a feature**: it makes piece provenance *reachable* (and deletes
the harness WeakMap hack) so that a **future** A′ (faithful converted→original
mapping) becomes a clean addition whenever a real producer/consumer is scheduled.
**Depends on:** Plan 2A §2aa (`@quarto/api/mappedString`), Plan 1b (the harness
`mapped-source.ts` serializer this corrects).
**Relationship to Plan 1c:** Plan 1c is **C′-only and does not depend on this** — its
A′ (faithful-mapping) work is deferred, so 1c carries **no A′ item** today. 1b.1
*unblocks* that future A′; it is **not** a gate on current 1c.
**Estimated sessions:** <1 (a unify step + one accessor across two TS packages + the
harness). **Status: landed — see "Status: COMPLETE" below.**

## Design decisions baked into this revision (read first)

Three choices were made up front; the spec below assumes them. They are recorded
here so the rationale survives.

1. **Unify the type, don't duplicate it.** `@quarto/types` and `@quarto/api` each
   carry their *own* `MappedString` interface today (structurally identical but
   nominally distinct). Adding `segments` to only one would leave the api builders
   typed against a copy that lacks it — an object literal returning `segments`
   would fail `tsc`'s excess-property check (`TS2353`). **Fix the root cause:**
   make `@quarto/types` the single owner and have `@quarto/api` import from it
   (§0). The dependency direction is already correct — `@quarto/api` declares
   `"@quarto/types": "*"` and `@quarto/types` is runtime-free.
2. **`segments` stays OPTIONAL (`segments?`).** We own every producer, but several
   build bare `{ value, map }` objects (the harness `rehydrateMappedString`, and
   `@quarto/api/markdownRegex`'s own raw substring/concat impl). Optional keeps the
   change purely additive — no producer is *forced* to grow `segments`, and no
   existing hand-built test fixture has to change. The cost is one well-defined
   fallback branch in the serializer (decision 3).
3. **`segments === undefined` ⇒ the serializer returns `[]`, meaning "provenance
   not provided (opaque)" — NOT "the string is synthetic."** An empty wire
   `sourceMap` is the *absence* of a structured map; a `source: null` *entry* is a
   *positive* assertion that a covered segment has no preimage. Keeping these two
   signals distinct matters for 1c: `source: null` stays reserved for
   *known-synthetic* text, and a producer that merely opted out of `segments` is
   never misreported as synthetic (its `.map()` may well resolve to a real file —
   e.g. a `markdownRegex` substring of a real source). This choice has **zero v1
   behavioral effect** (the response-side `sourceMap` is unconsumed on the Rust
   side today — `ts_engine.rs` only ever *sends* an empty map outbound), so we are
   freezing a *contract* for 1c, not changing runtime behavior.

## Why this exists (the hole)

Plan 1b surfaced a structural defect in the shared `MappedString` type, which the
plans had folded into the "A′ deferred future work" bucket. It does **not** belong
there: A′ (faithful original-file mapping) is a *feature* you legitimately defer
until a consumer exists; **this is a foundation defect that makes faithful
serialization *impossible* regardless of when A′ or any engine arrives.** It should
be fixed promptly and on its own, not gated behind A′.

**The defect.** `MappedString` is `{ value, fileName?, map }`
(`@quarto/types` `text.ts:17-34`; `@quarto/api` carries a duplicate at
`mappedString/index.ts:38-42`). `@quarto/api`'s builders return plain object
literals whose piece structure lives **entirely in the `map` closure**:
`mappedConcatInternal` (`mappedString/index.ts:200-230`) computes `offsets[]`
(prefix-sum boundaries) + captures `strings[]` (child segments) but exposes
**neither** — the returned literal is just `{ value, map }`.

**The consequence.** To serialize a `MappedString` to the wire `TsSourceMapEntry[]`
(one entry per piece, no coalescing) you must walk its pieces. There is no piece
list to walk, and probing `.map(i)` index-by-index **coalesces by construction** and
cannot recover the original boundaries. So the harness **cannot faithfully serialize
any `@quarto/api`-built `MappedString`** (e.g. one a real `markdownForFile`
conversion returns via `splitLines`/`mappedConcat`). The wire `source_map` field is
therefore structurally un-populatable from real engine output — a half-dead protocol
field.

**The 1b workaround (to be removed by this plan).** `mapped-source.ts` has its own
`mappedStringFromPieces` that conforms to the interface but stashes its
`SourcePiece[]` in a module-private `WeakMap` (`mapped-source.ts:81`,
`piecesRegistry`), and `serializeMappedString` reads that. This works **only** for
`MappedString`s the harness itself built; for an arbitrary `@quarto/api`-built one it
returns `[]` (the documented v1 echo case). That is exactly the gap that blocks A′.

## §0. Unify the `MappedString` type into `@quarto/types`

**Goal:** one nominal `MappedString` (plus its companions), owned by `@quarto/types`,
imported by `@quarto/api`. This is a prerequisite for §1–§2 to typecheck; it is also
a latent-bug fix in its own right (two structurally-equal-but-separate definitions
are an invitation to silent drift).

Concrete moves:

- **`@quarto/types/src/text.ts`** already defines `Range`, `MappedString`,
  `StringMapResult`, `EitherString`. **Add** `StringChunk` here
  (`export type StringChunk = string | MappedString | Range;`) because §4 promotes a
  public builder whose namespace-type signature references it — the public type
  surface must not leak an `@quarto/api`-local type. Also add the optional
  `segments` accessor to `MappedString` (spec in §1).
- **`@quarto/api/src/mappedString/index.ts`**: delete the local `MappedString`,
  `StringMapResult`, `EitherString`, `Range`, and `StringChunk` definitions
  (delete **by symbol**, ~lines 26–50 — and **keep `RangedSubstring`**, defined just
  below at `:52-55`; it stays local, so do not delete by raw line range). Replace with
  `import type { MappedString, StringMapResult, EitherString, Range, StringChunk } from "@quarto/types";`
  and **re-export them** so the package's existing public surface is preserved:
  ```ts
  export type { MappedString, StringMapResult, EitherString, Range, StringChunk } from "@quarto/types";
  ```
  This re-export is **load-bearing**: `@quarto/api/src/index.ts` does
  `export * from "./mappedString/index.js"`, and `markdownRegex/index.ts` imports
  `MappedString`/`EitherString`/`Range`/`RangedSubstring` **from `../mappedString`**.
  Without the re-export those imports break.
  - `RangedSubstring` stays **defined locally** in `mappedString/index.ts` (it is a
    line-splitting helper, not part of the core `MappedString` contract, and no
    other package imports it). Keep its existing `export`.
- **`@quarto/types/src/quarto-api.ts`** already does
  `import type { MappedString } from "./text.js";` — extend it to also import
  `StringChunk` (needed by §4's namespace member).
- **No `package.json` changes** and **no new dependency edge** — `@quarto/api`
  already depends on `@quarto/types`, and `@quarto/types` imports nothing from
  `@quarto/api` (no cycle risk; it is types-only).
- **Out of scope / untouched:** `@quarto/annotated-qmd` builds `MappedString`s via
  the *external, published* `@quarto/mapped-string` npm package — a different type
  universe that nothing in the 1b.1 surface (api / engine-host-deno / hub-client)
  imports. Do **not** touch it.

Verification gate for §0 alone: `npm run typecheck -w @quarto/types` and
`-w @quarto/api` clean **before** adding `segments` logic, so a type regression from
the unify is isolated from a logic regression in §2.

## §1. `@quarto/types` — add the optional accessor to the (now sole) interface

`ts-packages/quarto-types/src/text.ts`, on `MappedString`:
```ts
export interface MappedString {
  readonly value: string;
  readonly fileName?: string;
  readonly map: (index: number, closest?: boolean) => StringMapResult;
  /**
   * Flattened provenance: one entry per leaf-backed segment, in output order,
   * covering [0, value.length). Optional — consumers that don't need provenance
   * (engines) ignore it; `undefined` ⇒ provenance NOT PROVIDED (opaque), which the
   * serializer encodes as an empty wire map. `source: null` marks a segment with no
   * original file (KNOWN-synthetic / inserted text) — distinct from `undefined`.
   */
  readonly segments?: () => ReadonlyArray<{
    start: number;            // offset of this segment in `value`
    length: number;
    source: { file: string; fileOffset: number } | null;
  }>;
}
```
- **Optional** is load-bearing (decision 2): purely additive, no existing consumer
  changes, no producer is forced to implement it.
- It flattens to **leaf** segments (file + file-offset), which is exactly the wire
  `TsSourceMapEntry` shape — the serializer becomes a 1:1 map.

## §2. `@quarto/api/mappedString` — implement `segments()` in the three builders

`ts-packages/quarto-api/src/mappedString/index.ts` (the data is already computed; the
return types are now the unified `@quarto/types` `MappedString`, so adding `segments`
to the returned literals typechecks):
- **`fromString(text, fileName)`** (`:268`) → one segment:
  `[{ start: 0, length: value.length, source: fileName ? { file: fileName, fileOffset: 0 } : null }]`.
  (Identity leaf; `fileOffset` base is 0 because `fromString` is the terminus.)
- **`mappedSubstringInternal(source, start, end)`** (`:177`) → if `source.segments`
  is **present**, attach a `segments()` that slices/forwards `source.segments()` to the
  `[start, end)` window: rebase each segment's `start` by `-start`, and clip/split at
  the window edges (a segment straddling the boundary is truncated; its
  `source.fileOffset` shifts by the truncation). **If `source.segments` is `undefined`,
  do NOT attach a `segments` property at all** — the result stays opaque. Use a
  conditional property:
  `{ value, fileName, map, ...(source.segments ? { segments: () => clipRebase(...) } : {}) }`.
  The accessor's return type is `ReadonlyArray<…>`, **never `undefined`** — "propagate
  opacity" means *omit the accessor*, not implement one that returns `undefined`
  (which would be a type error and would break `serializeMappedString`'s `ms.segments?.()`
  call, which invokes the accessor and would get `undefined`, then `.map` throws).
- **`mappedConcatInternal(strings)`** (`:200`) → **opaque iff any child is opaque**
  (the opacity invariant below). If **every** child exposes `segments`, attach
  `segments()` = `strings.flatMap((s, i) => shiftSegments(s.segments!(), offsets[i]))`,
  where `shiftSegments(childSegs, off)` rebases each child segment's `start` by `+off`.
  If **any** child's `segments` is `undefined`, **omit the `segments` accessor entirely**
  (same conditional-property form as substring) — do **not** synthesize a `source: null`
  segment for the opaque child: that would misreport opaque-but-possibly-mapped text as
  *synthetic*, the exact thing decision 3 forbids. The empty-`strings` case (`:202`) is
  fully described and attaches `segments: () => []`. (In practice every `@quarto/api`
  child carries `segments`, so the omit branch fires only when a caller hands a *foreign*
  MappedString in as a chunk; mixing one foreign child sacrifices the faithful children's
  provenance — accepted, because the alternative is a synthetic lie.)
- `mappedStringFromChunks` (`:233`) and the public pure functions `normalizeNewlines`
  / `splitLines` all route through `mappedSubstringInternal` + `mappedConcatInternal`,
  so they inherit `segments()` for free — no separate implementation, but their
  existing tests are in the blast radius (must stay green; the change is additive).

**Opacity invariant (decision 3, made operational).** Opacity (`segments === undefined`)
**propagates outward and is never converted to a `source: null` (synthetic) claim.** A
foreign MappedString (no `segments`) can enter the builder graph at **exactly three
sites**, each preserving opacity rather than fabricating provenance:
1. `fromString(x)` where `x` is already a `MappedString` → returned as-is (`:287-289`
   passthrough); a foreign opaque input stays opaque (it gains no `segments`).
2. as the `source` of `mappedSubstringInternal` → result opaque (above).
3. as a chunk of `mappedConcatInternal` (incl. via `mappedStringFromChunks` passing a
   `MappedString` piece through unchanged, `:245-246`) → whole result opaque (above).
No `@quarto/api` builder ever **originates** opacity: `fromString(string)` always
attaches `segments`, and substring/concat of segment-bearing inputs do too. So within a
pure `@quarto/api` graph `segments` is always present; opacity only ever arrives from a
foreign input a caller supplied. (`source: null` stays reserved for *known-synthetic*
leaves — e.g. a bare-string chunk via `fromString(string)`, S4.)

- Invariants the implementer asserts in tests: segments are contiguous and cover
  `[0, value.length)` exactly; offsets are monotonic; a leaf with a `fileName`
  yields a non-null `source`.

## §3. Harness `mapped-source.ts` — serialize from `segments()`, drop the WeakMap

`ts-packages/quarto-engine-host-deno/src/mapped-source.ts`:
- `serializeMappedString(ms)` walks `ms.segments?.()` (one `TsSourceMapEntry` per
  segment: `{ start, length, source: seg.source }` — `source: null` passes through;
  **no coalescing**). **If `ms.segments` is `undefined`, return `[]`** — the
  "provenance not provided (opaque)" encoding (decision 3). Document the contract at
  the call site verbatim:
  > An empty `sourceMap` means **provenance was not provided** (opaque) — *not*
  > "provenance is empty/synthetic." A `source: null` entry is reserved for a
  > segment a producer **knows** is synthetic. The two are distinct signals.
- **Remove the entire WeakMap-era piece machinery as one unit** —
  `mappedStringFromPieces`, the `piecesRegistry` `WeakMap` (`:81`), **and** the
  now-orphaned `serializePieces` helper (`:340`) + `SourcePiece` interface (`:67`). All
  four exist only to make pieces reachable, which `segments()` now does natively. After
  the rewrite `serializeMappedString` walks `ms.segments()` **directly** — a segment is
  already `{ start, length, source }`, i.e. the `TsSourceMapEntry` shape (verified
  `types.ts:162`), so no piece→entry conversion is needed and `serializePieces` has no
  remaining caller. (Confirmed: `serializePieces`/`SourcePiece` are not imported by
  `host.ts` or `index.ts` — their only non-test users are the three functions being
  deleted.)
  **Migrate the tests.** `mapped-source.test.ts` splits into *Part A — Rehydration*
  (`describe` at `:74`, **kept**; S7 is added here) and *Part B — Serialization*
  (`describe "serializePieces / serializeMappedString"` at `:203`). Part B is the
  cluster that imports `SourcePiece`/`serializePieces` (`:22`/`:26`) and builds via
  `mappedStringFromPieces` (the **two** call sites, `:215` and `:278` — not four).
  **Rewrite the whole Part B block** as S5/S5b (serialize over a *real* `@quarto/api`
  MappedString, plus the opaque-vs-synthetic distinction); the no-coalescing guarantee
  Part B got from `serializePieces` is now owned by S5. The stub-base
  `.map().originalString.value === ''` behavior the old Part B asserted has **no
  equivalent** through the real builders (which carry real bases), so those assertions
  are re-expressed, not migrated 1:1.
- `rehydrateMappedString` (the Rust→Deno direction, T2) **gains a `segments()`**
  derived 1:1 from its wire `TsSourceMapEntry[]` (each entry *is* a segment). This is
  additive and cheap, and it closes the Rust→Deno→Rust **passthrough** round-trip: an
  engine that receives a rehydrated `MappedString` and returns it unchanged now
  re-serializes faithfully instead of degrading to `[]`. (No v1 consumer requires
  this, but with the `[]`-means-opaque contract it is the difference between a
  faithful and an opaque passthrough, so we implement it rather than leaving it to
  the fallback.)

## §4. `@quarto/api/mappedString` — expose a public multi-piece builder

`segments()` makes provenance *readable*; engines also need a public way to *build*
multi-piece provenance. Today `mappedConcatInternal` / `mappedStringFromChunks` /
`mappedSubstringInternal` are **internal** (`mappedString/index.ts:200/233/177`) — the
public namespace only exports `fromString`, `fromFile`, `splitLines`,
`normalizeNewlines`, `indexToLineCol`, so an engine's `markdownForFile` **cannot
construct a faithful multi-piece `MappedString`** through `quarto.mappedString.*`. Export
a public multi-piece builder and add it to the `quarto.mappedString` namespace type:
```ts
// @quarto/api/mappedString/index.ts — promote to public (add `export`):
export function mappedStringFromChunks(
  source: EitherString, pieces: StringChunk[], fileName?: string): MappedString;
// (StringChunk = string | MappedString | Range — now owned by @quarto/types per §0;
//  a Range slices `source` faithfully, a bare string is synthetic.)
```
Wiring:
- Add `mappedStringFromChunks` to the **inline `mappedString` object type** in
  `@quarto/types/src/quarto-api.ts` (`:100`). Note: there is **no named
  `MappedStringNamespace` interface** — it is an anonymous inline type; edit it in
  place. The signature uses `StringChunk` (added to `@quarto/types` in §0).
- Add `mappedStringFromChunks` to the harness's `mappedStringNs` assembly in
  `quarto-api.ts` (`:196`) and to its `@quarto/api/mappedString` import list (`:84`).
- **Caveat — the wiring is not type-checked end-to-end.** `buildQuartoAPI` returns
  `... as unknown as QuartoAPI` (`quarto-api.ts:231/241`), so a mismatch between the
  namespace member and the `QuartoAPI` type will **not** trip `tsc`. The round-trip
  test (S5 / the success-criteria round-trip) is the real guard — make sure it
  drives the *publicly-wired* `quarto.mappedString.mappedStringFromChunks`, not the
  internal function directly.

This is what would let a **future** A′ producer (a real `markdownForFile` conversion)
construct faithful multi-piece provenance through the public SDK. No engine in the
current epic produces one yet — Plan 1c's echo, Plan 3's jupyter, and Plan 4's Julia
all take the C′ path — so this is **enabling capability, not a current dependency**.

## Known opaque producers (intentional, documented — not work items)

With `segments` optional, these existing producers of the unified `MappedString` do
**not** implement it and therefore serialize as `[]` (opaque). This is acceptable for
v1 and explicitly in line with decision 3; flagged here so a future A′ author knows
where provenance currently stops:
- **`@quarto/api/markdownRegex`** carries its *own* raw substring/concat impl
  (`mappedStringFromSource` + inline `subMs` + `mappedSubstringOf`,
  `markdownRegex/index.ts:~600-730`) that builds bare `{ value, map }` literals.
  Strings produced by `breakQuartoMd` / `partitionCellOptions` are therefore opaque
  to `serializeMappedString` even though their `.map()` resolves to real files. If a
  later plan needs faithful provenance *through* `breakQuartoMd`, the clean fix is to
  refactor these raw builders to delegate to `mappedString`'s
  `mappedSubstringInternal` / `mappedConcatInternal` (which now carry `segments`),
  rather than duplicating the segment logic a third time. **Out of scope for 1b.1.**

## Test seams (write first; each bound to a named revert)

**Tier — single, for every seam: vitest Node (jsdom not required, not used).** Everything under test
here is pure, deterministic string/offset arithmetic over plain objects (`segments()` derivation,
`serializeMappedString`, `rehydrateMappedString`). There is **no layout/scroll/geometry surface** — no
`getBoundingClientRect`, no DOM render — so routing any of this to a browser tier would be wrong (slower
with zero added faithfulness). The **unit under test is never mocked**: each seam mounts the *real*
`@quarto/api` builder / real `serializeMappedString` / real `rehydrateMappedString` / real
`buildQuartoAPI`. The only mock is the harness's `SourceReader` (an in-memory `Record<path,string>`),
which is a genuine environment dep (file I/O) and is *not* the unit under test in S5/S5b/S6/S1–S4 (those
have no I/O at all). The mocks are environment deps only — the in-memory `SourceReader`
(`Record<path,string>`) in S7 (`rehydrate`) and the existing fake `PlatformHost` in S8 (`buildQuartoAPI`);
neither is the unit under test, and S1–S6, S5b, S9 touch no I/O.

### Frozen Test Seam Spec

Once green, each row's harness + assertions are **frozen** — never edited to go green.
All `:NNN` line anchors below are **pre-edit** (this branch's HEAD); §3's deletions and
§2's additions shift later lines, so **locate by symbol name**, not line number, once
edits start landing.

| ID | Real unit mounted (not mocked) | Seam: call → assertion surface | Mock boundary | Named revert hunk → which assertion reddens |
|----|--------------------------------|--------------------------------|---------------|---------------------------------------------|
| S1 | `fromString` (`mappedString/index.ts:268`) | `fromString("abc","f.qmd").segments()` → deep-equal `[{start:0,length:3,source:{file:"f.qmd",fileOffset:0}}]`; `fromString("abc").segments()[0].source` → `null` | none | In `fromString.segments()`, change the `source` expr `fileName ? {file:fileName,fileOffset:0} : null` → always `null` ⇒ the **file-segment** assertion (`source.file==="f.qmd"`) RED |
| S2 | `mappedConcatInternal` via `mappedStringFromChunks` (`:233`/`:200`) | `mappedStringFromChunks(fromString(src,"f.qmd"),[{start:0,end:5},{start:5,end:10}]).segments()` → **length 2**, entries `{start:0,len:5,fileOffset:0}`,`{start:5,len:5,fileOffset:5}` | none | Replace `mappedConcatInternal.segments()`'s `strings.flatMap(...)` with a single whole-value segment `[{start:0,length:value.length,source:firstChild}]` ⇒ `.length===2` RED (becomes 1) |
| S3 | `mappedSubstringInternal` via `mappedStringFromChunks` | substring window `[2,7)` of a single `"f.qmd"` leaf → `segments()` = `[{start:0,length:5,source:{file:"f.qmd",fileOffset:2}}]` | none | Remove the `+start` rebase on `source.fileOffset` in `mappedSubstringInternal.segments()` ⇒ `fileOffset===2` RED (becomes 0) |
| S3b | `mappedSubstringInternal.segments()` clip/split (`:177`) | two-segment source (`[0,5)`+`[5,10)` of `"f.qmd"`), then substring `[3,8)` **straddling** the boundary → **two clipped** entries `{start:0,length:2,fileOffset:3}`,`{start:2,length:3,fileOffset:5}` | none | Remove the window-edge clip in `mappedSubstringInternal.segments()` (forward whole child segments unclipped) ⇒ the clipped-`length`(2,3 not 5,5) assertion RED |
| S4 | `mappedConcatInternal` + `fromString` no-fileName branch | concat `[Range[0,3) of "f.qmd", bare-string "XX"]` → segment 0 `source.file==="f.qmd"`, segment 1 `source===null`, both spans intact | none | In `fromString.segments()` no-`fileName` branch, replace `source:null` with `source:{file:"?",fileOffset:0}` ⇒ the **bare-string** `segments()[1].source===null` assertion RED |
| S5 | `serializeMappedString` (`mapped-source.ts:434`) over a **real** `@quarto/api` multi-piece MS | build via `mappedStringFromChunks` (2 file-backed chunks); **exercised-guard** `ms.segments!().length===2` first; then `serializeMappedString(ms)` → deep-equal the 2-entry `TsSourceMapEntry[]`, no coalescing | none | Restore the pre-change body `return []` (ignore `segments()`) ⇒ `serialize(ms)` deep-equal (2 entries) RED. **This is the assertion that proves the hole is closed.** |
| S5b | `serializeMappedString` undefined-`segments` fallback | (a) bare `{value,map}` fixture (no `segments`) → `serializeMappedString` returns `[]`; (b) a fixture whose `segments()` returns one `source:null` whole-value entry → returns `[{start:0,length,source:null}]`; **assert (a) ≠ (b)** | none (hand-built fixtures) | Change the undefined-`segments` fallback from `return []` to synthesize `[{start:0,length:value.length,source:null}]` ⇒ the **`(a)!==(b)`** (opaque ≠ known-synthetic) assertion RED |
| S6 | `mappedConcatInternal.segments()` contiguity across a 3-child concat (incl. one empty-string child) | assert `segments` starts non-decreasing, each `start==prev.start+prev.length`, `last.start+last.length===value.length` (full cover, no gap/overlap) | none | Remove the `+offsets[i]` shift in `mappedConcatInternal.segments()`'s `shiftSegments` ⇒ child starts collapse toward 0 ⇒ the `start==prev.start+prev.length` contiguity assertion RED. (Distinct hunk from S2's flatMap; S6 binds the rebase arithmetic, S2 binds the no-coalesce count.) |
| **S7** | `rehydrateMappedString` (`mapped-source.ts:287`) **passthrough round-trip** | `serializeMappedString(rehydrateMappedString(value, wireEntries, reader))` → deep-equal the original `wireEntries` (rehydrate→serialize is identity on the wire) | in-memory `SourceReader` (`Record<path,string>`) | Remove the new `segments` property from `rehydrateMappedString`'s returned object ⇒ `serialize` hits the opaque fallback → `[]` ≠ `wireEntries` ⇒ round-trip-identity assertion RED |
| **S8** | `buildQuartoAPI` (`quarto-api.ts:115`) **public wiring** — add to the existing `quarto-api.test.ts` | `const q = buildQuartoAPI(makeFakeGlobal(), makeFakeHost()); expect(typeof q.mappedString.mappedStringFromChunks).toBe("function")`, then round-trip `q.mappedString.mappedStringFromChunks(...)` → `serializeMappedString` → faithful entries | **reuse the existing `makeFakeHost()`/`makeFakeGlobal()` in `quarto-api.test.ts` (`:20`/`:67`)** — do NOT hand-roll a `PlatformHost`; the ~12 existing pure-namespace tests already prove `buildQuartoAPI` is safe with this fake (it never eagerly touches the host during assembly) | Remove `mappedStringFromChunks` from the `mappedStringNs` literal (`quarto-api.ts:196`) ⇒ `q.mappedString.mappedStringFromChunks` is `undefined` ⇒ the `typeof==="function"` assertion RED. **tsc will NOT catch this** (the `as unknown as QuartoAPI` cast at `:231`); this runtime seam is the only guard. |
| **S9** | `mappedConcatInternal` opacity propagation (the C1 fix) | `mappedStringFromChunks` with **one file-backed `Range` chunk + one foreign opaque `MappedString` chunk** (a bare `{ value, map }`, no `segments`) → the result has **no `segments`** (`result.segments === undefined`), and `serializeMappedString(result) === []` — **not** a `[{…, source:null}]` synthetic claim | none (hand-built foreign opaque chunk) | Change the "any child opaque ⇒ omit `segments`" branch to synthesize a `source: null` segment for the opaque child ⇒ `result.segments` becomes defined / `serialize` returns a null-segment entry ⇒ the `segments === undefined` (opacity-preserved, decision 3) assertion RED |

### Refactor-induced vacuity check (decision-3 surface)

The one place a future refactor could silently collapse a discriminator is **S5 vs S5b vs S7**, all of
which can return `[]`:
- S5's revert makes the serializer return `[]` for a *segments-bearing* MS; the S5 input has
  `segments().length===2`, so `[] ≠ 2-entries` — discriminates. The **exercised-guard**
  (`ms.segments!().length===2` asserted *before* serializing) prevents the sibling trap where a
  single-segment MS would make `[]`-vs-faithful indistinguishable.
- S5b exists *because* `[]` is overloaded: it pins that opaque (`undefined segments → []`) and
  known-synthetic (`one null segment → [{…null}]`) stay **non-equal**. Do **not** "simplify" S5b by
  asserting both equal `[]` — that collapses exactly the signal decision 3 protects.
- S7's revert also yields `[]`, but S7 asserts equality to a **non-empty** `wireEntries`, so `[] ≠
  wireEntries` discriminates. If a later change ever makes the opaque fallback non-`[]`, re-confirm S7's
  `wireEntries` fixture still differs from that new fallback value.

### Missing-test pass (reasoned across the change; accepted-untested logged)

- **§0 unify — nominal single-owner of `MappedString`.** *Accepted-untested at runtime*, guarded by the
  **§0 tsc gate** instead. Rationale: TS types erase at runtime; no unit test can assert that
  `@quarto/types` and `@quarto/api` resolve to one nominal interface. The binding guard is "`npm run
  typecheck -w @quarto/api` clean after the api-local copies are deleted" — if the unify regresses, the
  builders (which return literals with `segments`) fail `TS2353` against a still-duplicated local type.
- **`markdownRegex` opacity (`breakQuartoMd`/`partitionCellOptions` → `[]`).** *Accepted-untested,
  deliberately.* Rationale: a test pinning "markdownRegex output serializes to `[]`" would be a
  change-detector that reddens the day someone makes those builders faithful — fighting the documented
  intended evolution (delegate to `mappedString`'s now-`segments`-bearing builders). The opacity is a
  documented v1 limitation, not a contract; pinning it would invert its meaning.
- **`splitLines` / `normalizeNewlines` inherit `segments()`.** *Covered by delegation* — both route
  through `mappedSubstringInternal` + `mappedConcatInternal`, which S3/S3b/S2/S6 bind directly. No
  separate seam; if a future change gives either its own segment logic, add a dedicated seam then.

## Verification

- **§0 gate first:** `npm run typecheck -w @quarto/types` / `-w @quarto/api` clean after the unify,
  **before** §2 logic lands (isolates type regressions from logic regressions).
- `npm run typecheck -w @quarto/types` / `-w @quarto/api` / `-w @quarto/engine-host-deno` — clean.
- `npm run test -w @quarto/api` (existing 33 `mappedString` tests + any `markdownRegex` tests stay green —
  the change is additive; `markdownRegex` consumes the re-exported types) +
  `-w @quarto/engine-host-deno` (S5/S5b/S7/S8 + the existing 105 host/mapped-source tests; the WeakMap
  removal must not regress any). The pure-builder seams S1–S4, S3b, S6, S9 live in `-w @quarto/api`.
- **Full `cargo xtask verify`** — `@quarto/types`/`@quarto/api` feed hub-client's bundle, so run the
  WASM/hub leg to confirm the additive change breaks nothing downstream. (Earlier grep found no direct
  hub-client `MappedString` consumer, so this is belt-and-suspenders, but the rule says run it for shared
  TS changes hub-client bundles.)

## Status: COMPLETE — all 7 criteria met, review-clean, full verify green (2026-06-30)

Landed on integration line `feature/ts-engine-extensions` as 4 sub-task commits
`94cc1f4b9` (T1 §0/§1) → `fc67af854` (T2 §2) → `9936f5069` (T3 §3) → `4ed32cffe` (T4 §4),
each individually spec+quality review-clean (sonnet), plus a whole-branch final review
(opus, `69268ab48..4ed32cffe`): **READY TO MERGE — YES**, no Critical/Important. The
decision-3 opacity invariant was traced end-to-end and the seam suite was confirmed
*genuinely bound* by re-applying the named reverts (S5b reds only S5b; S9 reds only S9 —
non-vacuous). Full `cargo xtask verify` = exit 0 (all 14 steps incl. WASM/hub leg).

**Accepted follow-up (NOT a blocker, confirmed not a live bug by both reviews):**
`shiftSegments` (api `index.ts`) and `serializeMappedString` (`mapped-source.ts`) propagate
`source: seg.source` by reference rather than cloning. `segments()` is read-only by contract,
leaves regenerate fresh source objects per call, and flatMap is 1:1 over distinct child
segments — so there is no intra-array or cross-call shared-mutable aliasing. Latent fragility
only; a defensive `{...seg.source}` clone would close it if a future change ever mutates a
`source` object. Left as-is to avoid over-building beyond the plan's scope.

## Success Criteria

- [x] **§0 unify:** `@quarto/types` is the sole owner of `MappedString`, `StringMapResult`,
  `EitherString`, `Range`, `StringChunk`; `@quarto/api/mappedString` imports + re-exports them; the
  api-local duplicates are deleted; `markdownRegex` (which imports from `../mappedString`) still
  typechecks; no new dependency edge / no cycle.
- [x] `MappedString.segments?()` exists on `@quarto/types` (optional, additive) with the
  `undefined` ⇒ opaque / `source: null` ⇒ known-synthetic contract documented inline.
- [x] `@quarto/api`'s `fromString` / `mappedSubstringInternal` / `mappedConcatInternal` implement it; the
  three builders' segments cover `[0, value.length)` contiguously; the substring straddle path is
  exercised (S3b); existing `mappedString` tests stay green.
- [x] `serializeMappedString` faithfully serializes a **real `@quarto/api`-built** multi-piece
  `MappedString` (one entry per segment, no coalescing) — S5 binds it — and returns `[]` (not a
  synthesized entry) for a `segments`-less `MappedString` — S5b binds it.
- [x] The WeakMap-era piece machinery — `mappedStringFromPieces`, `piecesRegistry`, `serializePieces`,
  and the `SourcePiece` type — is **removed** from `mapped-source.ts` (`serializeMappedString` walks
  `segments()` directly); Part B of `mapped-source.test.ts` is re-expressed as S5/S5b; no test depends on
  the removed machinery; `rehydrateMappedString` carries a `segments()` derived from its wire entries —
  the rehydrate→serialize passthrough round-trip is identity on the wire (S7 binds it).
- [x] **A public multi-piece builder** (`mappedStringFromChunks`) is exported from
  `@quarto/api/mappedString`, added to the inline `mappedString` namespace type in
  `@quarto/types/src/quarto-api.ts`, and wired in `buildQuartoAPI` — so a **future** A′ producer can
  construct faithful multi-piece provenance through the public SDK. A test builds a multi-piece string
  through the **publicly-wired** `quarto.mappedString.mappedStringFromChunks` and round-trips
  `segments()` → `serializeMappedString` to a faithful `TsSourceMapEntry[]` (S8 binds the wiring; tsc
  cannot, per the `as unknown as QuartoAPI` cast).
- [x] Full `cargo xtask verify` green.

## Sequencing note: separate plan vs. a 1c "Phase 0" prolog

This is written as a **separate plan** rather than a 1c prolog. The change is small,
additive-optional, and (per the blast-radius grep) narrowly consumed — so a
well-delineated 1c Phase 0 with its own gate would be *mechanically* fine. It is kept
separate for two reasons that survive the narrow blast radius: (1) **dependency
hygiene** — a *future* 1c A′ item would *depend on* this, and that reads more cleanly
as its own plan than as 1c's first phase (the more so now that A′ is deferred out of
1c — a 1c-gating "Phase 0" would have been actively wrong); and (2) **different
work-kind / review lens** — this is a shared-TS-library/type change (vitest + tsc +
the hub WASM leg), distinct from 1c's Rust pipeline-integration work, and deserves its
own reviewer attention on the `MappedString` surface. If minimizing plan count is
preferred, folding it into 1c as a gated Phase 0 is an acceptable alternative; the
tradeoff is the two points above.

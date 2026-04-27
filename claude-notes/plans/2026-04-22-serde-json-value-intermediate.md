# Eliminate `serde_json::Value` intermediate in pampa JSON writer

Status: **landed on `perf/2026-04-22-json-sourcemap` (commits 4e7a43ec, b3e15a47); browser-verified 2026-04-22**

Beads: bd-wgup. Previous session on the same file established the
`SourceInfoSerializer` hotspot (bd-h5l7,
`claude-notes/plans/2026-04-22-sourceinfo-eq-hotspot.md`); this is a
follow-up identified during Phase E browser validation of that fix.

## Context

The hub-client's `parse_qmd_to_ast` (wraps
`quarto_core::pipeline::parse_qmd_to_ast` and serializes its output via
`pampa::writers::json::write_with_config` with `include_inline_locations:
true`) is the primary read path in the preview render loop. After the
bd-h5l7 fix, Chrome profiling showed the remaining time dominated by
serde_json-related symbols:

```
<serde_json::value::Value as serde_core::ser::Serialize>::serialize
<alloc::collections::btree::map::BTreeMap<String, Value>>::insert
```

This plan establishes a reproducible native measurement of the same
code path, identifies the specific pattern responsible, and proposes a
fix direction validated against data.

## Native measurement setup

A new `crates/perf-harness/` crate houses drivers for native profiling
of hub-client entry points. The first driver, `parse-qmd-to-ast`, mirrors
the exact call chain in `crates/wasm-quarto-hub-client/src/lib.rs:757`:

1. `quarto_core::pipeline::parse_qmd_to_ast` (Parse → EngineExecution →
   MetadataMerge stages)
2. Build an `ASTContext` from the returned `SourceContext`
3. `pampa::writers::json::write_with_config` with
   `JsonConfig { include_inline_locations: true }`

```bash
# Build with debuginfo preserved for samply/atos symbol resolution
cargo build --profile=release-perf -p perf-harness

# One iteration (for large fixtures):
target/release-perf/parse-qmd-to-ast /tmp/q2-intern-bench/8x.qmd 1

# Many iterations (for small fixtures, so samply has samples):
target/release-perf/parse-qmd-to-ast /tmp/q2-intern-bench/1x.qmd 30
```

A new `[profile.release-perf]` in the workspace root `Cargo.toml`
inherits from `release` but sets `debug = true, strip = false` so
samply can resolve Rust symbols. Don't profile against plain `--release`;
the default release profile strips debuginfo and you get a useless
forest of raw addresses.

### samply workflow

```bash
# Record with presymbolication so we get resolved symbols offline
samply record -s -n --unstable-presymbolicate \
  -o /tmp/q2-perf-profiles/parse-qmd-8x-sym.json.gz -- \
  target/release-perf/parse-qmd-to-ast /tmp/q2-intern-bench/8x.qmd 3

# Inspect top self-time via the analyzer checked into the repo.
crates/perf-harness/scripts/analyze_profile.py \
  /tmp/q2-perf-profiles/parse-qmd-8x-sym.json.gz --top 30
```

The analyzer (`crates/perf-harness/scripts/analyze_profile.py`) merges
the profile's `threads[].stringArray` entries that look like `"0x..."`
with the sidecar syms table (`{rva, size, symbol}` triples per module)
and reports self-time per resolved symbol. stdlib-only Python; its
head docstring documents both the profile and sidecar formats.

## Findings

### Wall time scales ~linearly in the driver

| Size | JSON bytes | user CPU |  ratio |
|------|-----------:|---------:|-------:|
| 1×   |  1,170,632 |   0.11 s |   —    |
| 2×   |  2,382,298 |   0.23 s |   2.09× |
| 4×   |  4,831,709 |   0.54 s |   2.35× |
| 8×   |  9,826,609 |   1.37 s |   2.54× |

Growth per doubling is ~2.1–2.5×. Slightly super-linear in the tail,
consistent with allocator/cache effects as the output buffer grows
past L2/L3 cache sizes. No quadratic pathology — the work is genuinely
linear in AST node count, just with a high constant factor.

### Self-time distribution (symbolicated)

Top symbols at each fixture size (self-time % from 3–4k sample counts):

| Symbol (self-time) | 1× | 2× | 4× | 8× |
|---|---:|---:|---:|---:|
| `_platform_memmove` | 11.2% | 18.5% | 27.3% | **41.1%** |
| `indexmap::Core<String, Value>::insert_full` | 7.3% | 6.7% | 5.1% | 5.3% |
| `_nanov2_free` | 5.8% | 6.2% | 4.8% | 4.5% |
| `_malloc_zone_malloc` | 3.9% | 3.6% | 3.0% | 2.2% |
| `nanov2_malloc_type` | 3.2% | 3.3% | 3.1% | 2.7% |
| `tiny_malloc_from_free_list` | 3.3% | — | — | 2.3% |
| `__vfprintf` (format) | 3.8% | 3.8% | 3.6% | 2.3% |
| `hashbrown::entry` (indexmap) | 3.1% | — | — | 1.8% |
| `serde_json::Value::serialize` | 2.8% | — | 2.9% | 1.6% |
| `RandomState::hash_one::<&String>` | 2.7% | — | — | 1.5% |
| `Bucket<String, Value>::clone` | — | — | — | 0.7% |

Observations:

1. **`_platform_memmove` share rises with document size** — 11% → 41%
   across 1× → 8×. Pure memory copying. Something is moving increasing
   volumes of bytes per AST node as the document grows; constant
   overhead hypothesis is rejected.
2. **Allocator churn is a flat ~13–15% tax** regardless of size. Many
   small allocations (Values, Strings, IndexMap buckets) at a steady
   per-node rate.
3. **`indexmap::Core::insert_full` is ~5–7% on its own.** Every AST
   node builds a `Value::Object` backed by `IndexMap<String, Value>`;
   each insert hashes and inserts its key.
4. **Tree-sitter parsing is <2% at every scale.** The parser is not
   the bottleneck.
5. **`__vfprintf` at ~3%** — likely number formatting inside the
   serde_json output path (f64 / integer → decimal text). Not dominant
   but visible.

### Root cause

`pampa::writers::json::write_with_config` operates in two passes:

1. **Build pass**: constructs a `serde_json::Value` tree. Every AST
   node becomes a `Value::Object` (IndexMap<String, Value>) with
   freshly-allocated `String` keys (`"c"`, `"s"`, `"t"`, `"attrS"`,
   `"targetS"`, etc.) and `Value::*` leaves. For a document producing
   ~10 MB of JSON, this tree is on the order of tens of megabytes in
   memory, distributed across tens of thousands of allocations.

2. **Serialize pass**: `serde_json::to_writer(&mut buf, &value)` walks
   the tree and emits UTF-8 bytes to `Vec<u8>`, doubling the buffer on
   growth.

Both passes touch every byte. The build pass allocates and copies
string keys and IndexMap bucket contents; the serialize pass reads
every Value and memcpys its bytes into the output buffer; the output
buffer itself doubles in capacity as it grows past thresholds,
memcpying its entire current contents each time. Large documents
amortize this cost across a larger working set with poorer cache
locality — which is why `_platform_memmove`'s *fraction* climbs with
size.

### Why we do this today

The Value-tree pattern comes from the original writer design:
`write_pandoc` returns a `Value` which the top-level writer then
serializes. The intermediate form made it easy to rearrange field
order before output and compose pieces across helpers. It was never
chosen for performance reasons.

## Fix direction

**Primary (F1) — Stream JSON directly, skip the Value intermediate.**

Replace the `write_pandoc() -> Value` + `serde_json::to_writer(value)`
pattern with a direct serializer that writes UTF-8 to a `&mut dyn
io::Write` as it walks the Pandoc AST. Two implementation strategies:

1. **Custom `Serialize` impls on AST types.** Each `Inline`/`Block` gets
   a hand-written `Serialize` that calls `serializer.serialize_struct`
   / `serializer.serialize_map` with `&'static str` field names. No
   IndexMap, no String key allocations, no intermediate Value. Plays
   nicely with `serde_json::Serializer<&mut Vec<u8>>`.
2. **Direct byte writer.** Skip serde entirely. Write JSON bytes with a
   small writer (`write_all(b"{")`, escape helpers, number formatting
   via `itoa`/`ryu`). Maximum control, but reimplements JSON rules
   (escaping, number formatting, field ordering).

Recommend starting with (1). It preserves serde's correctness
guarantees around escaping and number formatting, drops the dominant
cost centers (Value tree alloc + IndexMap inserts + String key
allocs), and is straightforwardly verifiable against the existing
snapshot tests. (2) is a larger change with independent bugs; use it
only if (1) doesn't move the needle enough.

A known wrinkle: the current writer requires **deterministic field
ordering** (`c`, `s`, `t` alphabetical per comment in `json.rs:34`).
This is easy with a hand-written `Serialize` — you control the order
you emit fields. It's harder if you use `#[derive(Serialize)]` on
structs because serde serializes in declaration order, which forces
the struct fields themselves to be ordered. We can keep declaration
ordering or provide explicit serialize impls; both work.

**Secondary — interning the field-name strings.**

Even inside the current Value-tree pattern, replacing `String::from("c")`
/ `"c".to_string()` with `&'static str` where possible would cut
string allocations. But if F1 lands, this mostly goes away — direct
serializers emit `&'static str` keys naturally.

**Tertiary — investigate `__vfprintf` (~3%).**

Likely f64 number formatting inside serde_json's `fmt::Display for
Number`. Could be swapped for `ryu` if it's not already. Only worth
looking at if F1 leaves this as a relevant fraction.

## Validation plan

Each phase re-uses the perf-harness driver and the samply workflow.

### Phase 1 — Scaffolding ✅ done

- [x] Baseline profiles captured at bd-wgup investigation time; Findings
      section above stays authoritative.
- [x] Strategy decided (see "Selected strategy" section above):
      `JsonStreamWriter<W>` helper wrapping `CompactFormatter`; all
      `write_*` functions will take `&mut JsonStreamWriter<W>` in one
      cohesive cutover rather than an incremental dual-path rollout.
- [x] Implemented `crates/pampa/src/writers/json_stream.rs` with a
      per-level state machine (Array/Object, first-element + in_value
      flags) and unit tests. **12/12 pass**, including byte-identical
      escaping vs `serde_json::Value` for tricky strings.
- [x] Using the perf-harness driver for before/after numbers — no
      criterion bench needed beyond that.

### Phase 2 — Convert writer to streaming ✅ done

Strategy deviated from the plan slightly: the original incremental
"convert one variant at a time" approach was infeasible because
`write_inline` / `write_block` / helpers are coupled through the
`Value` return type — a single function changing its signature
forces all callers. Instead did a **cohesive cutover** of the entire
streaming implementation in one commit, alongside the legacy
Value-returning code (which stays for HTML writer callers that need
the Value for source-map building).

- [x] All `Inline` variants (22 of them including Custom) emit via
      streaming helpers.
- [x] All `Block` variants (18 of them including Custom) emit via
      streaming helpers.
- [x] `ConfigValue` / `Meta` variants.
- [x] Outer `Pandoc` + `ASTContext` envelope (emits `{blocks, meta,
      pandoc-api-version, astContext}` in that order — `astContext`
      last because `sourceInfoPool` is complete only after the walk
      finishes).
- [x] Snapshot churn: **62 snapshots updated**, all
      pool-resolution-equivalent per
      `crates/perf-harness/scripts/analyze_profile.py` workflow (i.e.
      canonicalized JSON structure unchanged — just key-order
      alphabetical now, and pool-ID shifts). Accepted via
      `cargo insta accept`.
- [x] Full pampa suite: **3734/3734 pass**.
- [x] JSON output byte counts identical to pre-Phase-2 on all 4
      fixture sizes.

**Perf result** (before / after, on the `parse-qmd-to-ast` perf harness,
user CPU):

| Size | Before  | After  | Speedup | JSON bytes  |
|------|--------:|-------:|--------:|------------:|
| 1×   | 0.11 s  | 0.05 s | 2.2×    | 1,170,632   |
| 2×   | 0.23 s  | 0.10 s | 2.3×    | 2,382,298   |
| 4×   | 0.54 s  | 0.27 s | 2.0×    | 4,831,709   |
| 8×   | 1.37 s  | 0.83 s | 1.65×   | 9,826,609   |

~2× speedup on small fixtures, tapering toward 1.6× at 8× — the
remaining cost becomes dominated by the output buffer's amortized
memcpy as it grows past cache (see next section).

**Profile shift (8×):**

| Symbol (self-time) | Before | After  |
|---|---:|---:|
| `_platform_memmove` | 41.1% | **68.9%** |
| `indexmap::Core::insert_full` | 5.3% | 0% (gone) |
| `_nanov2_free` | 4.5% | 0.5% |
| `nanov2_malloc_type` | 2.7% | gone |
| `__vfprintf` | 2.3% | 4.7% |
| `JsonStreamWriter::key` | — | 0.65% |
| `write_escaped_str` | — | 0.74% |
| tree-sitter parse | 1.7% | ~5% |
| allocator family (total) | ~13% | ~3% |

IndexMap and allocator churn are gone entirely. The **absolute**
memmove time dropped (40% less total wall time, even though memmove's
*share* rose to 68% of what's left), but now serialization is bottlenecked
on "move 10 MB of bytes into the output `Vec<u8>`" — the buffer's
amortized doubling copies ~20 MB total to produce 10 MB of output.

Tree-sitter parse as a % rose because the denominator shrank; absolute
parse time is unchanged. Parse is still not the bottleneck.

### Phase 3 — Snapshot canonicalization check ✅ done

- [x] Used the bd-h5l7 canonicalizer (`/tmp/check_snap_diffs2.py`)
      unchanged. For each `.snap.new` it parses the old and new JSON,
      walks every id-carrying field (`s`, `key_source`, `citationIdS`,
      `attrS.{classes,id,kvs}`, `targetS`, `captionS`), resolves each
      id through its own pool recursively, and compares the resulting
      pool-free structures for equality. Python dict equality is
      order-insensitive, so alphabetical-vs-legacy key order also
      folds away for free.
- [x] All 62 `.snap.new` files canonicalized identically to their
      `.snap` counterparts — zero structural changes.
- [x] Alphabetical key ordering documented inline in the streaming
      impl ("Alphabetical key order" in each helper's docstring) as
      the determinism contract. User constraint: not required to
      match legacy key order, but determinism required.

### Phase 4 — Re-profile ✅ done

- [x] Ran `samply record -s -n --unstable-presymbolicate` on the 8×
      fixture post-Phase-2 and analyzed with
      `crates/perf-harness/scripts/analyze_profile.py`. Full
      before/after table recorded under Phase 2's Findings section
      above — `indexmap::insert_full` gone, allocator churn collapsed
      from ~13% → ~3%, `_platform_memmove` share rose to 69% because
      its absolute cost dropped less than the total (the output
      `Vec<u8>` doubling now dominates). Total wall time on 8×
      dropped 1.37s → 0.83s (1.65×).

### Phase 5 — Full verification ✅ done

- [x] `cargo nextest run --workspace` — 7636/7636 pass.
- [x] `cargo xtask verify` — all verification steps passed (Rust
      build + hub-client WASM build + vitest + trace-viewer tests).
- [x] Hub-client browser cross-validation (2026-04-22): user Carlos
      loaded the canonical `test.qmd`, confirmed functionality is
      intact, and reported "performance feels markedly snappier."
      Matches the 1.65× native speedup at the same document size.

## Follow-ups (out of scope for bd-wgup)

With the Value tree gone, the post-Phase-2 profile is dominated by
`_platform_memmove` (69% on 8×). Root cause is no longer interning or
serialization — it's the output `Vec<u8>` growing via capacity
doublings to hold ~10 MB of JSON, which copies ~20 MB in aggregate.
Candidate directions for a future session (each worth its own beads
issue):

- **Pre-size the output buffer.** Estimate JSON size from AST node
  count (or a pre-walk) and `Vec::with_capacity`. Quick win if the
  caller (wasm-quarto-hub-client) can be involved — otherwise a
  heuristic inside `write_with_config`.
- **Avoid the `String::from_utf8` re-validation** in
  `crates/wasm-quarto-hub-client/src/lib.rs:826`. The streaming
  writer is already producing valid UTF-8 (serde_json's escaping
  rules guarantee it); a `String::from_utf8_unchecked` there would
  skip another full-buffer pass, at the cost of a clearly-documented
  invariant.
- **Write the JSON directly into JS-owned memory** via WASM memory
  views instead of going through an intermediate `Vec<u8>` + JS
  string. Biggest potential win but a bigger architectural change.
- **Second-tier symbols worth revisiting once memmove is addressed:**
  `__vfprintf` (~5%) likely from serde_json number formatting — could
  swap to `ryu`/`itoa` directly in the CompactFormatter wrapper.

## Reproducing this investigation

The raw samply profiles used above live in `/tmp/q2-perf-profiles/`
and the analyzer at `/tmp/analyze_profile2.py` — both are tmp and
won't survive a reboot. Reproduction recipe:

```bash
# 1. Build scaled fixtures from the canonical 50-paragraph lorem ipsum.
#    (Created during the bd-h5l7 session.)
mkdir -p /tmp/q2-intern-bench
cp /Users/cscheid/Desktop/daily-log/2026/04/22/test.qmd /tmp/q2-intern-bench/1x.qmd
for n in 2 4 8 16; do
  python3 -c "import sys; sys.stdout.write(open('/tmp/q2-intern-bench/1x.qmd').read() * $n)" \
    > /tmp/q2-intern-bench/${n}x.qmd
done

# 2. Build the driver with debuginfo
cargo build --profile=release-perf -p perf-harness

# 3. Record profiles with presymbolication
mkdir -p /tmp/q2-perf-profiles
for spec in "1 30" "2 15" "4 6" "8 3"; do
  read n iter <<< "$spec"
  samply record -s -n --unstable-presymbolicate \
    -o /tmp/q2-perf-profiles/parse-qmd-${n}x-sym.json.gz -- \
    target/release-perf/parse-qmd-to-ast /tmp/q2-intern-bench/${n}x.qmd $iter
done

# 4. Analyze — produces the tables above
for n in 1 2 4 8; do
  echo "=== ${n}x ==="
  crates/perf-harness/scripts/analyze_profile.py \
    /tmp/q2-perf-profiles/parse-qmd-${n}x-sym.json.gz --top 15
done
```

## Open questions — resolved (2026-04-22)

User provided constraints:

1. **Field ordering**: not a hard requirement, but **deterministic
   output is**. Pick any order you like; just emit the same bytes on
   every run. We'll preserve alphabetical ordering since that matches
   the current `#[derive(Serialize)]` behavior and minimizes snapshot
   churn.
2. **JSON shape**: cannot change. `sourceInfoPool` stays an array.
   Everything else structural stays the same.
3. **Whitespace**: free to change. The constraint is **structural
   equality of the parsed JSON value** (deep-equal under the
   `SameValue` terminal rule + recursive compound equality — JS's
   "deep equal"). Snapshots will churn on whitespace; we canonicalize
   and accept, same technique as bd-h5l7.

## Selected strategy (2026-04-22)

Introduce a `JsonStreamWriter<W: io::Write>` helper wrapping
`serde_json::ser::CompactFormatter` (serde_json's own byte-level
formatter). This gets correct string escaping and number formatting
for free without reimplementing JSON rules. The helper exposes
ergonomic methods: `begin_object`, `key`, `string`, `u64`,
`begin_array`, `end_array`, etc., each tracking comma state
internally.

Convert all `write_*` functions to take `&mut JsonStreamWriter<W>`
instead of returning `Value`. Single cohesive cutover — the caller
and callee are tied through the `Value` return type, so an
incremental or dual-path rollout would be messier than a clean
switch. Outer envelope (`PandocDocumentJson`) is already streamed via
`serde_json::to_writer` on `#[derive(Serialize)]`; we hand-roll the
equivalent emission so the whole writer is on the same abstraction.

Why not keep using `derive(Serialize)` with adapter types? The
adapter types would need mutable access to the `SourceInfoSerializer`
during `serialize()` (to intern sourceinfo IDs), which requires
either `RefCell` or a pre-computation pass. Both add complexity
without removing the fundamental cost — the point is to avoid
allocating the intermediate tree at all, not to hide it behind serde
adapter types.

Why not bypass serde entirely with a from-scratch JSON writer?
Reimplements escaping and float formatting. `CompactFormatter` gives
us exactly the primitives we need without that risk.

## Snapshot handling

Per constraint (3) above, output whitespace can change but structural
equality must be preserved. The canonicalizer from bd-h5l7
(`/tmp/check_snap_diffs2.py`) already does this for pool references.
For this session we may also need to handle:

- Key order within objects — if the new streaming writer happens to
  emit keys in a slightly different order than the old
  `serde_json::Value` output (e.g. because `IndexMap` ordering vs
  our explicit ordering diverge in edge cases).
- Whitespace — `CompactFormatter` emits no whitespace, which matches
  the current output (`serde_json::to_writer` with the default
  compact serializer).

Plan to verify: extend the canonicalizer to sort object keys before
comparing, then run it across all snapshots after each batch of
conversions.

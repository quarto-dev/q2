# Plan 7f — Prerequisites for Plan 7d

**Date:** 2026-05-29
**Branch:** feature/provenance (sibling to 7d / 7e)
**Status:** Ready for implementation. Ships before 7d.

## Overview

Plan 7d's algebraic refactor of `coarsen` → `plan_user_writes` depends on three pieces of producer-side hygiene that don't yet hold. Plan 7f lands them so that 7d's strict R5 trust point is meaningful and so that BP is not silently violated by upstream sloppiness.

Four workstreams, none of which involve the writer itself:

1. **Framework source_info preservation** — the React framework currently strips `s:` on rebuilt wrappers (Emph, Strong, Para, every passthrough except the top-level Ast). Fix the recursion to spread source_info forward.
2. **User-edit stamping** — a single reserved pool slot for `Generated{by: user_edit, …}`; the framework stamps it on user-constructed nodes (including those nested inside CustomNode slots).
3. **`SourceInfo::default()` audit** — replace test usages with explicit kinds; deprecate the `Default` impl.
4. **Production-residue cleanup** — handful of non-test `SourceInfo::default()` sites in `quarto-pandoc-types` and `quarto-yaml-validation`. Each gets a deliberate `By::` kind (four new constructors, including `By::unknown()` for the source-info-completing reader's placeholder). Refactors `InlineAttr::new` to require explicit source_info, eliminating the empty-AttrSourceInfo sentinel. Splits `json::read` into a strict variant for q2-internal paths and `read_completing_source_info` for callers that consume JSON from outside the source-tracked world (qmd-syntax-helper Pandoc subprocess output, CLI `--from json`, external filter binaries, Lua AST handoff).

Plus two minor cleanups bundled along for the ride: wire-format renames `attrS` → `a`, `sourceInfoPool` → `p`.

## Phase 1 — Audit `dispatch.tsx` for `s:`-stripping

Walk every renderer in `ts-packages/preview-renderer/src/framework/dispatch.tsx`'s `renderChildrenRegistry`. For each renderer whose `setLocalAst` closure reconstructs a wrapper, confirm whether it preserves the original node's `s:` field.

Spot test confirmed widespread; the list as of writing (per `dispatch.tsx:60-240`):

- ✓ `Ast` (preserves via spread)
- ✗ `Emph`, `Strong`
- ✗ `Underline`, `Strikeout`, `Superscript`, `Subscript`, `SmallCaps` (via `makeFlatInlineRenderer`)
- ✗ `Link`, `Image`, `Span`, `Quoted`
- ✗ `Para`, `Plain`, `Header`, `BlockQuote`, `Div`
- ✗ `BulletList`, `OrderedList`, `Figure`
- ✗ `CustomBlock`, `CustomInline` (via `renderCustomNodeChildren` — needs separate verification)

### Research finding (2026-05-30) — `renderCustomNodeChildren`

Verified at `ts-packages/preview-renderer/src/framework/dispatch.tsx:261-310`. The function preserves `s:` correctly already: its rebuild path at line 274 spreads the original node before overriding `slots:`

```ts
const setSlot = (next: Slot) =>
    setLocalAst({ ...customNode, slots: { ...customNode.slots, [name]: next } });
```

The spread copies every top-level field (including `s:`) from `customNode`; only `slots:` gets overridden. Both `CustomBlock` and `CustomInline` reach the same code path, so both are preserving. The audit row for them should flip from "needs separate verification" to "preserves."

This leaves Phase 1's `✗` list at: `Emph`, `Strong`, `Underline`, `Strikeout`, `Superscript`, `Subscript`, `SmallCaps` (all via `makeFlatInlineRenderer`), `Link`, `Image`, `Span`, `Quoted`, `Para`, `Plain`, `Header`, `BlockQuote`, `Div`, `BulletList`, `OrderedList`, `Figure`. `CustomBlock` and `CustomInline` move to `✓`.

Work items:

- [ ] Walk every entry in `renderChildrenRegistry`. Record a checklist row per renderer: "preserves" vs "strips."
- [ ] Verify `makeFlatInlineRenderer` separately (one helper, multiple renderers).
- [x] Verify `renderCustomNodeChildren` (custom-node generic walk). — preserves via spread (see finding above).

## Phase 2 — Apply the spread-fix

Mechanical pass over each `✗` row from Phase 1. The transformation:

```ts
// Before
setLocalAst({ t: 'Emph', c: newChildren });

// After
setLocalAst({ ...(node as EmphInline), c: newChildren });
```

The spread copies `s:`, `attr`, and any other top-level fields; the `c:` override replaces the children. For renderers that already override multiple fields (e.g. `Link` which keeps `c[0]` and `c[2]`), the spread happens first, then the explicit field overrides.

`makeFlatInlineRenderer` gets the spread internally; all six inline wrappers benefit at once.

Work items:

- [ ] Apply the spread pattern to every `✗` renderer.
- [ ] Apply the spread pattern inside `makeFlatInlineRenderer`.
- [ ] For each renderer, add a TS test: simulate a child edit, assert the rebuilt parent's `s:` matches the original.

## Phase 3 — User-edit stamping at `setLocalAst` boundary

Wrap the `<Node>` component's `setLocalAst` to stamp `Generated{by: user_edit}` on any subtree in the new node that lacks `s:`. The walker:

```ts
function stampUserEdits(node: BlockNode | InlineNode): BlockNode | InlineNode {
    const stamped = node.s === undefined
        ? { ...node, s: USER_EDIT_SOURCE_INFO_ID }
        : node;

    // Recurse into `c:` children (standard inline/block wrapper shape).
    if ('c' in stamped && Array.isArray(stamped.c)) {
        return {
            ...stamped,
            c: stamped.c.map(child =>
                typeof child === 'object' && child !== null && 't' in child
                    ? stampUserEdits(child as BlockNode | InlineNode)
                    : child)
        };
    }

    // Recurse into CustomNode `slots:` (Block::Custom / Inline::Custom shape).
    // Slots carry typed nested AST: `Block | Inline | Block[] | Inline[]` per slot key.
    if ('slots' in stamped && stamped.slots && typeof stamped.slots === 'object') {
        const newSlots: Record<string, unknown> = {};
        for (const [key, value] of Object.entries(stamped.slots)) {
            if (Array.isArray(value)) {
                newSlots[key] = value.map(v =>
                    typeof v === 'object' && v !== null && 't' in v
                        ? stampUserEdits(v as BlockNode | InlineNode)
                        : v);
            } else if (typeof value === 'object' && value !== null && 't' in value) {
                newSlots[key] = stampUserEdits(value as BlockNode | InlineNode);
            } else {
                newSlots[key] = value;
            }
        }
        return { ...stamped, slots: newSlots };
    }

    return stamped;
}
```

`<Node>` wraps the incoming `setLocalAst` and passes `(newNode) => setLocalAst(stampUserEdits(newNode))` to the child renderer. The walker only stamps subtrees lacking `s:`; preserved subtrees keep their existing source_info.

The constant `USER_EDIT_SOURCE_INFO_ID` is the reserved pool slot (Phase 4).

Work items:

- [ ] Implement `stampUserEdits` walker (both `c:` and `slots:` recursion).
- [ ] Wire into `<Node>` component's `setLocalAst` propagation.
- [ ] TS test: user component constructs a new Span via `setLocalAst({ t: 'Span', c: ... })`; assert the resulting node has `s: USER_EDIT_SOURCE_INFO_ID` after stamping.
- [ ] TS test: preserved subtree (rebuilt-wrapper case) keeps original `s:` after stamping passes through it.
- [ ] TS test: user component constructs a new CustomBlock via `setLocalAst({ t: 'CustomBlock', type_name: 'Callout', slots: {...}, ...})`; assert nested nodes inside slots are stamped recursively.

## Phase 4 — Reserved pool slots (user_edit and unknown)

The Rust JSON writer (`crates/pampa/src/writers/json.rs`) currently builds the `sourceInfoPool` as a used-only intern table during AST traversal. After Phase 4, the serializer pre-populates reserved slots before any intern operation runs.

Two reserved slots:

- **`USER_EDIT_SOURCE_INFO_ID`** — `Generated{by: By::user_edit(), from: smallvec![]}`. Referenced by the framework's `stampUserEdits` walker (Phase 3).
- **`UNKNOWN_SOURCE_INFO_ID`** — `Generated{by: By::unknown(), from: smallvec![]}`. Referenced by `json::read_completing_source_info` for nodes arriving without `s:` from outside the source-tracked world.

Layout pinned via named constants in `crates/pampa/src/writers/json.rs` alongside `SourceInfoSerializer`:

```rust
pub const USER_EDIT_SOURCE_INFO_ID: usize = 0;
pub const UNKNOWN_SOURCE_INFO_ID: usize = USER_EDIT_SOURCE_INFO_ID + 1;
// future reserved slots: UNKNOWN_SOURCE_INFO_ID + 1, etc.

impl SourceInfoSerializer {
    pub fn new() -> Self {
        let mut pool = Vec::new();
        // Push in declaration order; constants pin the layout.
        pool.push(serializable_for_user_edit());   // ID 0
        pool.push(serializable_for_unknown());      // ID 1
        // ...
        Self { pool, /* ... */ }
    }
}
```

A unit test next to the constants asserts the pool entries match the constants (`assert_eq!(serializer.pool[USER_EDIT_SOURCE_INFO_ID].kind(), "user-edit")` etc.), so adding or rearranging reserved slots breaks the test rather than silently shifting IDs at consumer sites.

Export TypeScript hand-mirror in `ts-packages/preview-renderer/src/types/sourceInfo.ts`:

```ts
export const USER_EDIT_SOURCE_INFO_ID = 0;
export const UNKNOWN_SOURCE_INFO_ID = 1;
```

A Rust-side CI test asserts parity with the TS file (read the TS source, parse the numbers, compare to the Rust constants) — same hand-mirror discipline as `ATOMIC_CUSTOM_NODES`.

The pool stays Rust-authoritative: the framework references slot IDs by name; it never allocates. The reserved slots exist in every JSON document the writer produces, regardless of whether any node references them.

### Research finding (2026-05-30) — pool intern deduplication

The current `SourceInfoSerializer::intern` (`crates/pampa/src/writers/json.rs:303-404`) allocates a fresh pool entry on every call. The one cache, `arc_parent_ids`, is keyed by `Arc::as_ptr` and fires only at parent edges — `Substring.parent` and `Generated.from` anchors. The top-level intern call for a node's own `source_info` field never consults this cache, and pool entries are never compared by value. The module comment at lines 297-302 makes this explicit: "Each call allocates a fresh pool entry. … Pool entries are not deduplicated by content."

Consequence for the reserved slots. When a node is parsed via `read_completing_source_info`, the reader clones pool slot 1's value into the node's `source_info` field, producing a `Generated{by: unknown, from: smallvec![]}`. On re-serialization the writer's intern call for that field creates a fresh pool entry — structurally equal to slot 1, but at a new ID. For N completed-on-read nodes that survive to write, the pool grows by N entries.

**Decision: accept the duplication (option a).** Reasons:

- The duplication is bounded by the number of completing-reader nodes that survive to write. The completing reader only fires at the four outside-world entry points; most documents see zero completing-reader nodes.
- The duplication is per-document, not cumulative: each fresh write rebuilds the pool from scratch.
- Adding a value-equality short-circuit for the two reserved kinds (`user-edit`, `unknown`) would add a branch on the intern hot path. `SourceInfo` and `By` derive `PartialEq`, so the check is cheap in principle, but the intern path is exercised once per AST node and is already known to be hot (the `QUARTO_PERF_STATS=1` gauge exists for it).
- The reserved slots' canonical purpose is *referencing* (the framework's `stampUserEdits` cites slot 0 by ID), not deduplication-at-serialize. R5 dispatch in the writer treats every `Generated{by: user_edit}` equivalently regardless of which pool slot supplies it.

Follow-up: if the q2-debug AST viewer ends up displaying the pool literally and the visual noise from duplicates is distracting, add display-side dedup there rather than serialize-side dedup. The `QUARTO_PERF_STATS=1` gauge already reports `pool_size`; monitor it during smoke tests for any unexpected blow-up.

Work item update: no fix needed. Add a one-line comment near `intern` noting the duplication is by design ("reserved-slot duplication is bounded and accepted; see plan 7f Phase 4 research finding 2026-05-30") so a future reader doesn't try to "fix" it.

### Research finding (2026-05-30) — per-caller verification for the reader split

Verified the five completing-reader callers in turn. All five consume `source_info` from the parsed AST downstream, so the placeholder choice matters (none are "ignored entirely" cases). Per-site detail:

| Site | Downstream use | Placeholder |
|---|---|---|
| `crates/pampa/src/json_filter.rs:221` | Filtered AST replaces the pre-filter AST in the main pipeline; downstream stages and the eventual writer consume `source_info`. | `By::filter(filter_path, 0)` would be more specific than `unknown`. `By::filter` already exists (`crates/quarto-source-map/src/source_info.rs:535`); reused by code-3 legacy reader at `readers/json.rs:305`. Recommend `By::filter`, with line `0` since we don't know which line in the filter produced each node. |
| `crates/qmd-syntax-helper/src/conversions/definition_lists.rs:182` | Parsed AST goes to `qmd::write(&pandoc_ast, ...)` to round-trip back to markdown. The qmd writer dispatches on source_info: `Original{FileId(0), 0..0}` (today's default) routes through R1 with empty range → emits nothing; `Generated{by: unknown, …}` routes through R5 (synthesize) → emits from structure. The change is the *correct* behavior here — the AST has no preimage. | `By::unknown` is the right placeholder. Flag in the commit: writer dispatch changes from R1-empty to R5 for these AST nodes; the new behavior is the correct one. |
| `crates/qmd-syntax-helper/src/conversions/grid_tables.rs:133` | Same shape as definition_lists.rs above. | Same: `By::unknown`. Same writer dispatch shift applies. |
| `crates/pampa/src/main.rs:290` | CLI `--from json`. The result flows through `transform_divs` and then into the standard render pipeline; downstream may consume `source_info` anywhere. | `By::unknown` is correct — the user passed JSON from outside, we genuinely don't know. |
| `crates/pampa/src/lua/readwrite.rs:447` | Result is exposed to Lua filters via `rust_pandoc_to_lua_table`. Whether a given Lua filter reads `source_info` is filter-dependent; can't be ruled out. | `By::unknown` is correct — we don't know what produced this JSON. |

Signature surfaced: `json::read_completing_source_info` should accept a placeholder, not bake `By::unknown` in. Two reasonable shapes:

```rust
// Option 1: parameterized placeholder
pub fn read_completing_source_info(input: ..., default_by: By) -> Result<(Pandoc, ASTContext)>;

// Option 2: caller overwrites after read
pub fn read_completing_source_info(input: ...) -> Result<(Pandoc, ASTContext)>;
// caller then runs a pass to overwrite Generated{by: unknown} with their kind.
```

Recommend **Option 1**. The placeholder is set once on read (cheap, simple); Option 2 requires an extra AST walk to overwrite, which both adds work and risks missing nodes. Option 1 also matches the named-parameter discipline already used by the Phase 4 design: the call site declares its provenance up front.

Concretely:

```rust
// json_filter.rs
let (filtered_pandoc, filtered_context) = readers::json::read_completing_source_info(
    &mut json_output.as_bytes(),
    By::filter(filter_path.to_string_lossy(), 0),
)?;

// the other three
readers::json::read_completing_source_info(&mut cursor, By::unknown())
```

Note: `By::filter` is atomic-kind (`is_atomic_kind()` returns `true` for `kind == "filter"` per `crates/quarto-source-map/src/source_info.rs:839`). That makes filter-produced nodes atomic from the framework's perspective. For external filters whose output gets composed back into the document, atomic semantics may be undesirable — flag this for review. If `By::filter` proves wrong, a new `By::filter_output` (non-atomic) constructor is a possible alternative.

Work items:

- [ ] Rust: define `USER_EDIT_SOURCE_INFO_ID` and `UNKNOWN_SOURCE_INFO_ID` constants alongside `SourceInfoSerializer` in `crates/pampa/src/writers/json.rs`. Chain via `+ 1` so future reserved slots derive predictably.
- [ ] Rust: `SourceInfoSerializer::new()` pre-pushes the user_edit and unknown entries in declaration order (matching the constant values).
- [ ] Rust: unit test asserting pool entries match the constants (`assert_eq!(serializer.pool[USER_EDIT_SOURCE_INFO_ID].kind(), "user-edit")` and same for unknown). Adding or rearranging reserved slots fails the test.
- [ ] Rust: adjust all `Vec<SerializableSourceInfo>` traversals that assume "pool starts empty" — they now start with `RESERVED_POOL_SLOTS` entries.
- [ ] Rust: grep tests for hardcoded pool indices (`sourceInfoPool[0]`, `pool[1]`, etc.); replace literal numbers with the named constants so future slot additions don't break call sites silently.
- [ ] Rust: add `JsonReadError::MissingSourceInfoRef { node_path: String }` variant to `crates/pampa/src/readers/json.rs:23`. `node_path` is a JSON-pointer-style string (e.g. `"blocks[3].c[0]"`) identifying the offending node for debugging.
- [ ] Rust: make `json::read` strict — reject missing `s:` with `Err(JsonReadError::MissingSourceInfoRef)`. Add `json::read_completing_source_info(input, default_by: By)` alongside, which fills missing `s:` with a reference to a pool entry constructed from `default_by`. Apply uniformly across Block, Inline, Cell, Row, Head, Body, Foot. (Open question: should the completing reader reuse `UNKNOWN_SOURCE_INFO_ID` when `default_by == By::unknown()`, or always allocate a fresh entry? Symmetric with the intern-dedup decision above; recommend "always allocate fresh" so the function has a single uniform path.)
- [ ] Rust: add `By::unknown()` constructor in `quarto-source-map` (`kind: "unknown"`, non-atomic). The reserved `UNKNOWN_SOURCE_INFO_ID` pool slot uses it.
- [ ] Rust: switch the five outside-world callers to `json::read_completing_source_info` with explicit placeholders per the table above:
  - `json_filter.rs:221` → `By::filter(filter_path, 0)` (flag the atomic-kind concern in the commit; revisit if it causes downstream issues).
  - `qmd-syntax-helper`'s `definition_lists.rs:182` and `grid_tables.rs:133` → `By::unknown()`. Note in commit message that writer dispatch shifts from R1-empty to R5-synthesize for these nodes; new behavior is correct.
  - `pampa/src/main.rs:290` → `By::unknown()`.
  - `pampa/src/lua/readwrite.rs:447` → `By::unknown()`.
- [ ] Rust: grep tests for hand-crafted JSON literals that omit `s:` (`serde_json::json!({"t": "Str", "c": "..."})` patterns, multi-line string-literal JSON used in reader tests). Tests exercising the strict path: update to include valid `s:` references. Tests exercising `read_completing_source_info`: assert nodes carry the expected `Generated{by, …}` after the read.
- [ ] WASM bridge: verify `MissingSourceInfoRef` propagates through `incremental_write_qmd` as `{success: false, error: "Missing source_info reference at <node_path>", diagnostics: ...}` cleanly. Manual test by patching out one stamping site in Phase 3, observing the error in the browser console, then restoring.
- [ ] Documentation: update `crates/pampa/src/readers/json.rs` module docs to explain the two-reader split — q2-internal paths use strict, outside-world paths use completing with explicit `default_by`.
- [ ] Documentation: add a one-line comment near `SourceInfoSerializer::intern` noting reserved-slot duplication is by design (cross-reference this Phase 4 finding).
- [ ] TS: export `USER_EDIT_SOURCE_INFO_ID = 0` and `UNKNOWN_SOURCE_INFO_ID = 1` as typed constants in `ts-packages/preview-renderer/src/types/sourceInfo.ts`. Add a Rust-side CI test that reads the TS file and asserts the values match the Rust constants (same hand-mirror discipline as `ATOMIC_CUSTOM_NODES`).
- [ ] Rust test: round-trip a hand-constructed AST through the WASM bridge; assert `sourceInfoPool[0]` decodes as `Generated{by: user_edit}`.
- [ ] Rust test: deserialize JSON with bare nodes (no `s:` field) and assert `json_read` returns `Err(JsonReadError::MissingSourceInfoRef)`.
- [ ] TS test (atomic-gate sanity): a node with `s: USER_EDIT_SOURCE_INFO_ID` is not flagged as atomic by `dispatch.tsx`'s atomic gate (the gate's lookup-by-ID resolves to `Generated{by: user_edit}`, which is non-atomic).

**Two readers — strict `json::read` for q2-internal JSON, `read_completing_source_info` for callers that need a fallback.** The current single `json::read` is consumed by both q2-internal paths (the WASM bridge's `incremental_write_qmd`, which reads q2-extended JSON with `s:` populated on every node) *and* by paths that consume JSON from outside the source-tracked world (`json_filter.rs` for external filter output, `qmd-syntax-helper` for Pandoc subprocess output, `pampa/src/main.rs` for CLI stdin, `lua/readwrite.rs` for Lua AST handoff). The outside-world paths produce JSON without `s:` because the upstream producer doesn't know about q2's extension; making the reader universally strict breaks them.

Split the reader, scoping leniency to specific call sites:

- **`json::read`** becomes strict: rejects nodes missing `s:` with `Err(JsonReadError::MissingSourceInfoRef { node_path })`. Used by the WASM bridge's `incremental_write_qmd` and any future q2-internal JSON consumer.
- **`json::read_completing_source_info`** fills missing `s:` with a reference to the reserved `UNKNOWN_SOURCE_INFO_ID` pool slot (Phase 4 pre-populates the slot with `Generated{by: By::unknown(), from: smallvec![]}`). Used by the four outside-world consumers above. Callers with more specific provenance (e.g. `filter_source_info` for filter output) overwrite the placeholder immediately; callers without keep `By::unknown` as the honest "we don't know" provenance.

The function name `read_completing_source_info` matches the surrounding `read_<thing>` convention in `readers/json.rs` (`read_inline`, `read_block`, `read_attr_source`, `make_source_info`) and says exactly what it does: read, then complete any missing source_info. There is no compatibility shim layer — the leniency is a property of the explicit call site, not of the wire format.

The strict-reader rule applies only to JSON under q2's source-tracking contract, and surfaces producer bugs there at the boundary rather than at the writer.

**Phase-ordering constraint.** The strict reader cannot ship before Phase 2 (spread-fix on rebuilt wrappers) and Phase 3 (stampUserEdits on new nodes) — those two together are what guarantee every TS-produced JSON has `s:` on every node. If the strict reader lands first, every incremental write fails. Implementation order is: Phases 1–3 land in sequence, then Phase 4 (which includes the strict-reader change) lands after Phase 3 is verified working end-to-end.

**Scope of the strict-reader rule.** Every JSON-wire-format struct that has an `s:` field must reject missing-`s:` on read. Per `crates/pampa/src/writers/json.rs:1010-1116`, the fields exist on: Block, Inline, Cell, Row, Head, Body, Foot. Apply the strict-reader rule uniformly to all of these in the reader update.

**Error variant.** `JsonReadError::ExpectedSourceInfoRef` exists today (`crates/pampa/src/readers/json.rs:30`) but fires when the field is *present but malformed*; its message ("Expected SourceInfo $ref, got inline SourceInfo") is wrong for the missing-entirely case. Add a new variant `MissingSourceInfoRef { node_path: String }` carrying the path-to-the-offender context. A JS-side debugger seeing this error in an `incremental_write_qmd` response should be able to find the responsible producer site immediately.

(Phase 4 work items are listed under the per-caller research finding above, which supersedes the earlier checklist.)

## Phase 5 — Wire-format renames

Two JSON top-level fields in `crates/pampa/src/writers/json.rs` get single-character names to match the rest of the wire format:

- `attrS` (currently camelCase from `attr_s: AttrSourceJson`) → `a`. Apply `#[serde(rename = "a")]` to the field.
- `sourceInfoPool` (currently camelCase from `source_info_pool: Vec<SourceInfoJson>`) → `p`. Same mechanism.

Multi-character fields inside `AttrSourceJson` (`classes`, `id`, `kvs`) stay — they're Pandoc-standard. `pandoc-api-version` stays — Pandoc-legacy.

**Snapshot regeneration.** The renames + reserved pool slot change *every* JSON snapshot the writer produces. Affected fixture trees include `crates/pampa/src/snapshots/` and any other crate using `insta`-style snapshots that serialize ASTs. Expect a large mechanical commit regenerating snapshots; do it as a separate commit from the rename itself so the snapshot churn doesn't bury the substantive change.

**Wire-format breaking change.** The renames are a breaking change to the JSON envelope. q2's wire format isn't a documented public contract, but anyone holding cached JSON (test fixtures committed to disk, debug-dump files, recorded session traces under `claude-notes/`) will see breakage. The new fields are byte-equivalent in meaning; only the key names change. No semantic regression, but consumer-side coordination is needed.

Work items:

- [ ] Rust: apply `#[serde(rename = "a")]` to the `attr_s` field; remove the camelCase fallback for it.
- [ ] Rust: apply `#[serde(rename = "p")]` to the `source_info_pool` field.
- [ ] Rust: update `crates/pampa/src/readers/json.rs` to read the renamed fields.
- [ ] TS: update `ts-packages/preview-renderer/src/types/`, `hub-client/src/types/wasm-quarto-hub-client.d.ts`, **`hub-client/src/components/render/q2-debug/`** (debug AST viewer/editor that decodes the same JSON), and **`q2-preview-spa/src/`** (SPA-side decode) to match.
- [ ] Test: round-trip the largest existing JSON fixture; assert byte-equivalent after the rename.
- [ ] Regenerate all `.snap` snapshot fixtures: `INSTA_UPDATE=always cargo nextest run --workspace` (or per-crate equivalent). Commit the regenerated snapshots separately from the rename itself.
- [ ] Grep `claude-notes/` for `attrS` / `sourceInfoPool`; update any design doc or research note that references the old names.
- [ ] Verify the hub server (`crates/hub/`) treats AST JSON as opaque blob and does not pattern-match on `attrS` / `sourceInfoPool` field names. If it does inspect specific fields, update accordingly.

## Phase 6 — Audit `SourceInfo::default()` in tests

Approximately 1,400 references across the workspace. Most are tests with one of three intents; replacements are mechanical.

Add a new constructor first:

```rust
// crates/quarto-source-map/src/source_info.rs
impl By {
    /// Producer kind for test scaffolding. Non-atomic; appears only in
    /// test code where source_info is required by a constructor but
    /// has no real provenance to record.
    pub fn test_scaffold() -> Self {
        Self {
            kind: "test-scaffold".to_string(),
            data: serde_json::Value::Null,
        }
    }
}

impl SourceInfo {
    /// Convenience for tests: produce a non-atomic Generated source_info
    /// that won't trigger soft-drop and won't be confused with real provenance.
    pub fn for_test() -> Self {
        SourceInfo::Generated {
            by: By::test_scaffold(),
            from: smallvec![],
        }
    }
}
```

Per-test replacement guidance:

| Test intent | Original use of `SourceInfo::default()` | Replacement |
|---|---|---|
| XML/YAML structural; source_info is scaffolding | `SourceInfo::default()` | `SourceInfo::for_test()` |
| Proptest generator; source_info is consistent but not meaningful | `SourceInfo::default()` | `SourceInfo::for_test()` |
| Integration test with known fixture bytes | `SourceInfo::default()` | `SourceInfo::original(FileId(0), start, end)` with the actual offsets |
| Simulating React user-edit | `SourceInfo::default()` | `SourceInfo::Generated { by: By::user_edit(), from: smallvec![] }` |
| Comparison against "no source info" sentinel | `&SourceInfo::default()` | Replace with an `is_default()` predicate or refactor to `Option<SourceInfo>` |

Files to audit (highest concentration first):

- `crates/quarto-xml/src/types.rs` — structural scaffolding case.
- `crates/quarto-yaml-validation/src/tests.rs` — structural scaffolding case.
- `crates/quarto-ast-reconcile/src/generators.rs` — proptest generators.
- `crates/quarto-core/tests/*.rs` (jupyter_integration, navigation_e2e, navigation_merge) — integration tests with fixture bytes.
- Test modules under `crates/pampa/`.

**Production residue is handled in Phase 6.5** (below). The replacement target is **not** `user_edit`. `user_edit` applies only to React-constructed content. Every other caller decides their own provenance kind.

**Behavior change in writer-exercising tests.** Today, `SourceInfo::default()` is `Original{FileId(0), 0, 0}`. Under the writer, that has `preimage_in(target=FileId(0))` returning `Some(0..0)` — an empty range — so R1 fires and emits zero bytes. After the audit, those tests use `SourceInfo::for_test()` which is `Generated{by: test-scaffold, from: smallvec![]}`. `preimage_in` returns `None` for this shape, so R5 fires (or R3, if the node is a container) — different rule, different output. Any test that asserted on the *specific byte output* of running the writer over hand-constructed AST with `SourceInfo::default()` will see different (correct) bytes after the swap. Expect a small batch of test-expectation updates alongside the audit.

Work items:

- [ ] Add `By::test_scaffold()` constructor in `quarto-source-map`.
- [ ] Add `SourceInfo::for_test()` convenience in `quarto-source-map`.
- [ ] Audit test-file usages of `SourceInfo::default()`; replace with one of the four patterns above.
- [ ] Update writer-exercising test expectations where switching to `for_test()` changes the dispatch rule (R1-empty-range → R5/R3) — the new output is the correct one.
- [ ] Verify: `cargo nextest run --workspace` passes after replacements.

## Phase 6.5 — Production-residue fix sweep

The non-test `SourceInfo::default()` usages turn out to be a small, well-characterized set after filtering out the `#[cfg(test)] mod tests` blocks. Per-site decisions follow; each gets a deliberate `By::` kind rather than the default sentinel. Add the three new `By::` constructors first, then apply each fix.

### New `By::` constructors

Add to `crates/quarto-source-map/src/source_info.rs`:

```rust
impl By {
    /// Empty-Map sentinel ConfigValue used during metadata merging when
    /// no value is present.
    pub fn config_default() -> Self {
        Self { kind: "config-default".to_string(), data: Value::Null }
    }

    /// Programmatic construction of ConfigValue via the WASM bridge
    /// (`ConfigValue::from_path`) — no source bytes exist for these.
    pub fn programmatic_config() -> Self {
        Self { kind: "programmatic-config".to_string(), data: Value::Null }
    }

    /// AST nodes synthesized by the reconciler during apply_reconciliation
    /// paths that don't correspond to either input AST.
    pub fn reconcile_synthesize() -> Self {
        Self { kind: "reconcile-synthesize".to_string(), data: Value::Null }
    }
}
```

All three are non-atomic (never match `is_atomic_kind`) and require no `Invocation` anchor.

### Per-site fixes

**`crates/quarto-pandoc-types/src/config_value.rs:415`** — `impl Default for ConfigValue`. The empty-Map sentinel used in metadata merging.

```rust
// Before
source_info: SourceInfo::default(),

// After
source_info: SourceInfo::Generated {
    by: By::config_default(),
    from: smallvec![],
},
```

**`crates/quarto-pandoc-types/src/config_value.rs:539`** — `ConfigValue::from_path`. WASM-bridge programmatic injection.

```rust
// Before
let source_info = SourceInfo::default();

// After
let source_info = SourceInfo::Generated {
    by: By::programmatic_config(),
    from: smallvec![],
};
```

**`crates/quarto-yaml-validation/src/schema/merge.rs:32, 51, 88`** and **`schema/mod.rs:256`** — `SchemaError::InvalidStructure { location }`. These describe bugs in the schema definition itself, not in the user's YAML. Change the variant's signature:

```rust
// In SchemaError enum
InvalidStructure {
    message: String,
    location: Option<SourceInfo>,   // None for schema-structure errors
}
```

Update the four call sites to use `location: None`. Update any pattern-matching consumers (probably diagnostic formatters) to handle `Option`. Single-crate change; no cross-crate ripple expected.

**`crates/quarto-pandoc-types/src/inline.rs:304-311`** — `InlineAttr::new`. The current `attr_source.combine_all().unwrap_or_default()` fallback is the source of the empty-AttrSourceInfo sentinel. Refactor the signature to require explicit source_info:

```rust
// Before
impl InlineAttr {
    pub fn new(attr: Attr, attr_source: AttrSourceInfo) -> Self {
        let source_info = attr_source.combine_all().unwrap_or_default();
        Self { attr, attr_source, source_info }
    }
}

// After
impl InlineAttr {
    pub fn new(attr: Attr, attr_source: AttrSourceInfo, source_info: SourceInfo) -> Self {
        Self { attr, attr_source, source_info }
    }

    /// Convenience: derive source_info from non-empty AttrSourceInfo.
    /// Panics if attr_source is empty (use new() with explicit source_info instead).
    pub fn new_from_attr_source(attr: Attr, attr_source: AttrSourceInfo) -> Self {
        let source_info = attr_source.combine_all()
            .expect("InlineAttr requires non-empty AttrSourceInfo; use new() with explicit source_info");
        Self { attr, attr_source, source_info }
    }
}
```

Then update every `InlineAttr::new` call site that uses `AttrSourceInfo::empty()` to provide explicit source_info. See the research finding below for the actual list (the line numbers cited in earlier drafts of this plan pointed to test scaffolding, not `InlineAttr::new` calls).

**Delete the obsolete test.** The `source_info_attr_empty` test at `crates/quarto-pandoc-types/src/inline.rs:1452-1463` asserts the fallback behavior we just removed. Delete it. Commit message should note: "removes test for empty-AttrSourceInfo sentinel; case is now structurally impossible after InlineAttr::new signature change."

### Research finding (2026-05-30) — reconciler/block-test "synthesis sites" are not InlineAttr::new sites

The earlier draft listed `crates/quarto-ast-reconcile/src/lib.rs:107, 116, 132, 322, 1178` and `crates/quarto-pandoc-types/src/block.rs:222, 235, 247` as `InlineAttr::new` call sites that needed the explicit-source_info update. Re-reading those line numbers shows the claim is wrong on two counts:

1. **None of those sites call `InlineAttr::new`.** They directly assign `attr_source: AttrSourceInfo::empty()` to a field of a `Block::Header` / `Block::CodeBlock` / `Block::Div` / `Inline::Code` / `Inline::Insert` struct. Those types each have their own `source_info: SourceInfo` and `attr_source: AttrSourceInfo` fields; the `combine_all().unwrap_or_default()` fallback in `InlineAttr::new` is never invoked through them.

2. **All eight sites are test code.** Lines 107-134 of `quarto-ast-reconcile/src/lib.rs` are inside the crate's `#[cfg(test)] mod tests` block (`make_header`, `make_code_block`, `make_div` test helpers). Line 322 is in `test_inline_code_replaced_with_result`. Line 1178 is in `make_insert_para`, a helper inside another `#[test]` function. Lines 222-247 of `quarto-pandoc-types/src/block.rs` are inside that file's `#[cfg(test)] mod tests` (`source_info_plain`, `source_info_paragraph`, `source_info_codeblock`). Phase 6.5 is production-residue cleanup; test sites belong to Phase 6.

**Where the real `InlineAttr::new` call sites live** (from a clean `grep -rn 'InlineAttr::new' crates/`):

| Site | Status | Treatment |
|---|---|---|
| `crates/quarto-pandoc-types/src/inline.rs:1455, 1474, 1491` | Test code (`#[cfg(test)] mod tests`). | Phase 6 — replace with explicit `source_info` once the new signature lands. |
| `crates/pampa/src/pandoc/treesitter.rs:559` | **Production** — tree-sitter intermediate → `Inline::Attr`. Passes a real `attr_source` from the parse. | Phase 6.5 candidate: pass explicit `source_info` derived from the surrounding parse node's range. The current `unwrap_or_default()` fallback can only fire if `combine_all()` returns `None` on a real-parser attr_source — likely impossible in practice, but the new signature forces the choice to be made consciously. |
| `crates/pampa/src/pandoc/treesitter_utils/caption.rs:50` | **Production** — caption_attr → `Inline::Attr`. Same as above. | Same treatment. |
| `crates/pampa/src/pandoc/treesitter_utils/paragraph.rs:30` | **Production** — paragraph attr inline → `Inline::Attr`. Same as above. | Same treatment. |
| `crates/pampa/src/filters.rs:1503, 1513, 2123` | Test code. | Phase 6. |
| `crates/pampa/src/writers/plaintext.rs:887` | Test code (the surrounding context is a `let inlines = vec![make_str("text"), ...]` test fixture). | Phase 6. |
| `crates/pampa/src/lua/types.rs:2932` | Test code (`#[test] fn test_lua_inline_tag_name_attr`). | Phase 6. |
| `crates/pampa/src/lua/filter.rs:2254` | Test code (assert inside a `#[test]`). | Phase 6. |

**None of the three production `InlineAttr::new` callers passes `AttrSourceInfo::empty()`** — they all pass a real `attr_source` from the parse. So the production-side migration of the `InlineAttr::new` signature is mechanical: change the call to `InlineAttr::new(attr, attr_source, source_info)` where `source_info` comes from the surrounding parse context (each call site has access to a range or `SourceInfo` from the tree-sitter node it's lowering — wire it through). The fallback path through `unwrap_or_default()` only existed for defensive completeness; the signature change makes the defense explicit at the call site.

**Reconciler-side synthesis is a separate concern.** The original prompt asked which of the five reconciler "synthesis sites" warrant a more-specific `By::` kind than `reconcile_synthesize`. The answer is **none** — those sites don't synthesize source_info today; they take a `source: SourceInfo` parameter from the caller (`make_header(level, text, source)` etc.) and copy it into both the block's `source_info` and the empty `attr_source`. The `By::reconcile_synthesize` placeholder is needed only if and when reconciliation grows a path that synthesizes new AST without an input `SourceInfo` to inherit from. Today that doesn't exist; the placeholder is a forward-looking primitive.

The recommended Phase 6.5 work-item update is:

- Keep `By::reconcile_synthesize()` as a constructor (Phase 6.5 still adds it). Document it as "reserved for future reconciler-synthesized paths; no producer uses it today."
- Drop the line-number list of "reconciler InlineAttr::new sites that need updating" — the list was incorrect.
- Add Phase 6.5 work items for the three real production `InlineAttr::new` sites in `pampa`'s tree-sitter lowering (`treesitter.rs:559`, `treesitter_utils/caption.rs:50`, `treesitter_utils/paragraph.rs:30`), each of which gets explicit source_info derived from the surrounding parse range.

### Work items

- [ ] Add `By::config_default()`, `By::programmatic_config()`, `By::reconcile_synthesize()` to `quarto-source-map`. Document `reconcile_synthesize` as reserved for future reconciler-synthesized paths (no producer uses it at 7f-landing time).
- [ ] Unit test: assert `By::test_scaffold()`, `By::config_default()`, `By::programmatic_config()`, `By::reconcile_synthesize()` all return `false` from `is_atomic_kind()`. Pins the property explicitly so a future producer-contract change can't accidentally promote one to atomic without updating the test.
- [ ] Apply `config_value.rs:415` (Default impl) fix.
- [ ] Apply `config_value.rs:539` (from_path) fix.
- [ ] Change `SchemaError::InvalidStructure::location` to `Option<SourceInfo>`; update four call sites and any pattern-matching consumers.
- [ ] Refactor `InlineAttr::new` signature; add `new_from_attr_source` convenience.
- [ ] Update the three **production** `InlineAttr::new` call sites in `pampa` to pass explicit source_info derived from the surrounding parse range:
  - `crates/pampa/src/pandoc/treesitter.rs:559`
  - `crates/pampa/src/pandoc/treesitter_utils/caption.rs:50`
  - `crates/pampa/src/pandoc/treesitter_utils/paragraph.rs:30`
- [ ] Update the **test-code** `InlineAttr::new` call sites (`quarto-pandoc-types/src/inline.rs:1455, 1474, 1491`; `pampa/src/filters.rs:1503, 1513, 2123`; `pampa/src/writers/plaintext.rs:887`; `pampa/src/lua/types.rs:2932`; `pampa/src/lua/filter.rs:2254`) to pass `SourceInfo::for_test()`. This is technically Phase 6 work, but it falls out of the signature change and should land in the same commit as the refactor.
- [ ] Delete `source_info_attr_empty` test at `inline.rs:1452-1463`.
- [ ] Audit `AttrSourceInfo::empty()` call sites (separate from `InlineAttr::new` callers): the eight reconciler-test sites at `quarto-ast-reconcile/src/lib.rs:107, 116, 132, 322, 1178` and the three block-test sites at `quarto-pandoc-types/src/block.rs:222, 235, 247` are test scaffolding for Block fixtures; they don't trigger any `SourceInfo::default()` path. Leave them as-is unless Phase 6 decides to rename `AttrSourceInfo::empty()` itself (e.g. to `test_scaffold()`).
- [ ] Decide whether `AttrSourceInfo::empty()` should be renamed (`empty()` → `test_scaffold()`) or kept as a clearly-documented test convenience. Recommend keep — the current name is honest ("an empty AttrSourceInfo") and the rename would touch every test fixture that builds a Block-with-attr.
- [ ] Verify: `cargo xtask verify --skip-hub-build` clean after all sites are updated.

## Phase 7 — Deprecate `SourceInfo::default()`

After Phase 6 brings test usages to the irreducible minimum:

```rust
#[deprecated(
    since = "0.x",
    note = "Use SourceInfo::for_test() in tests, or the appropriate Generated{by: <kind>} in production. See provenance-contract.md."
)]
impl Default for SourceInfo {
    fn default() -> Self {
        SourceInfo::Original {
            file: FileId(0),
            start_offset: 0,
            end_offset: 0,
        }
    }
}
```

The `#[deprecated]` attribute surfaces remaining call sites at compile time with a clear message. After Phases 6 and 6.5, every known production site has a deliberate replacement; the deprecation guards against new uses entering the codebase. CI's `-D warnings` strictness means the deprecation is effectively a hard ban on new callers; remaining test-side stragglers can be cleared during Phase 8 verification or, if any are truly load-bearing, get a clearly-commented `#[allow(deprecated)]`.

Removing the `Default` impl entirely is a follow-up after the deprecation has had time to surface any forgotten sites. `#[derive(Default)]` consumers of types that include a `SourceInfo` field need separate audit before removal is safe.

Work items:

- [ ] Add `#[deprecated]` to `impl Default for SourceInfo`.
- [ ] Verify: `cargo xtask verify --skip-hub-build` clean with no remaining `SourceInfo::default()` warnings.
- [ ] If any `#[allow(deprecated)]` survives the audit, add an inline comment explaining why and pointing to the relevant follow-up.

## Phase 8 — Verification

- [ ] `cargo xtask verify` (full, including hub-build) clean. 7f touches `quarto-pandoc-types`, `quarto-source-map`, and `quarto-yaml-validation` — all dependencies of `wasm-quarto-hub-client`. Plain `cargo build --bin q2` does *not* pick these up in `q2 preview`; the embedded SPA loads a stale WASM. Full verify rebuilds the WASM chain. After this lands, anyone testing the preview must run the full verify or follow the `q2 preview` rebuild instructions in CLAUDE.md.
- [ ] All existing tests pass.
- [ ] New tests from Phases 2, 3, 4 pass.
- [ ] `#[derive(Default)]` audit: grep workspace for `#[derive(.*Default.*)]` on structs containing `SourceInfo` fields. The deprecation warning will fire when these derives are exercised. Decide per-site whether to suppress (with a clear comment) or refactor to remove the derive.
- [ ] Manual smoke test of q2-preview: open a document with shortcodes, edit a paragraph, save, re-open; verify the shortcode tokens are preserved and the framework's `s:` is intact on rebuilt wrappers.
- [ ] Manual smoke test of q2-debug: open a document; verify the source_info pool display shows `[0] = Generated{by: user_edit, …}` as the reserved slot, and that documents without user edits still display correctly (pool entry 0 is always present even if unreferenced from any node).
- [ ] Coordinate with Plan 7b's open work: if 7b adds tests via hand-crafted JSON, those tests must rebase after 7f and use the strict-reader pattern (or use the lenient reader explicitly for Pandoc-compatible content).

## What 7f does not do

- **No CustomNode serialization.** Custom nodes (Callout, Theorem, etc.) remain broken on edit until 7e. Editing a callout body still results in the callout disappearing from source until 7e lands.
- **No writer changes.** `coarsen` keeps its flat shape; 7d does the algebra refactor.
- **No removal of `Default` impl.** Deprecation only; removal is a follow-up.

## References

- Design doc: [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md).
- Sibling plan (next): [`2026-05-26-q2-preview-plan-7d-algebraic-soundness.md`](2026-05-26-q2-preview-plan-7d-algebraic-soundness.md).
- Producer contract: [`provenance-contract.md`](../designs/provenance-contract.md).
- Playwright fixture convention: `claude-notes/instructions/testing.md` (post-`provenance-reactji-demo` merge).

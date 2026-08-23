# Provenance contract — emitting `SourceInfo` from a transform

**Status:** Active. Covers Plans 4–6 + 7f/7g provenance work on
`feature/provenance` (last revised 2026-06-05; see change log).
**Types:** `quarto_source_map::SourceInfo`, `By`, `Anchor`, `AnchorRole`
([`crates/quarto-source-map/src/source_info.rs`](../../crates/quarto-source-map/src/source_info.rs)).
**Plans:**
[Plan 4](../plans/2026-05-04-q2-preview-plan-4-source-info-types.md)
(types) ·
[Plan 5](../plans/2026-05-04-q2-preview-plan-5-wire-format.md)
(wire format) ·
[Plan 6](../plans/2026-05-04-q2-preview-plan-6-provenance-audit.md)
(this audit) ·
[Plan 8](../plans/2026-05-04-q2-preview-plan-8-include-roundtrip.md)
(include round-trip — abandoned; see tombstone).
**Audit report:** [`claude-notes/research/2026-05-22-plan-6-audit.md`](../research/2026-05-22-plan-6-audit.md).
**Consumer side:** what the incremental writer does with the `SourceInfo`
shapes this doc tells producers to emit — byte-copy boundaries, the
`preimage_in` walk, and the tiling precondition — is documented in
[Plan 7g](../plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md)
and the §"Tiling precondition" section at the end of this doc.

## Summary

Every `SourceInfo` a transform emits must accurately describe where the
node came from. The Plan 4 types give four physical shapes (`Original`,
`Substring`, `Concat`, `Generated`); this doc is the contract for which
shape to pick. The rule that follows replaces the historical "stamp
`SourceInfo::default()` and move on" pattern that Plan 6 audited out
of the transform layer.

## 1. Decision tree for new transforms

**Wire-format requirement.** Every AST node in the JSON wire format
carries an `s:` field referencing a valid entry in the source-info
pool — `astContext.p` (renamed from `sourceInfoPool` in Plan 7f Phase
5). The Rust JSON reader rejects bare nodes with
`Err(JsonReadError::MissingSourceInfoRef { node_path })`. There is no
fallback to `SourceInfo::default()` and no silent stamping; producers
are responsible for populating `s:` on every node, and the reader's
strictness keeps the contract honest by surfacing producer bugs at
the JSON boundary rather than at the writer.

**Pick the shape from where the emitted node's *bytes* come from, not
from how it was constructed.** Four branches:

| Source of the emitted node              | Shape                                                                                                                          |
|-----------------------------------------|--------------------------------------------------------------------------------------------------------------------------------|
| Corresponds to source bytes             | `Original` — `ctx.source_info.clone()`, or clone the input node's `source_info` field. Never construct an `Original` by hand.  |
| Pure synthesis with no preimage         | `Generated { by: By::<kind>(), from: smallvec![] }`                                                                            |
| Resolution of a user-written construct  | `Generated { by: By::<kind>(name), from: smallvec![Anchor::invocation(Arc::new(token_si))] }`                                  |
| **Mutation** of a node a filter received (e.g. `Str.text = upper(...)`) | Leave the input node's `Original` source_info untouched. Filter *mutations* are not classified atomic; do **not** rewrite to `Generated`. (The incremental writer then keeps the node's original bytes via the `Verbatim`/`InlineSplice` path rather than re-serializing.) |
| **Construction** inside a user Lua filter (e.g. `pandoc.Str("...")`) | Leave it alone — `filter_source_info` ([`crates/pampa/src/lua/types.rs:1813`](../../crates/pampa/src/lua/types.rs)) auto-attaches `Generated { by: filter, ... }` on the way out. |

If two branches feel equally applicable, pick the one with the longer
chain to source: the incremental writer and attribution
(`resolve_byte_range`) both prefer `Original` over `Generated{from:[]}`
and `Generated{from:[Invocation]}` over `Generated{from:[]}`.

## 2. `By::` constructor catalog

The known producer kinds, defined in
[`crates/quarto-source-map/src/source_info.rs`](../../crates/quarto-source-map/src/source_info.rs):

| Constructor                  | Line | `kind` string             | Purpose                                                                       | Atomic? |
|------------------------------|------|---------------------------|-------------------------------------------------------------------------------|---------|
| `By::filter(path, line)`     | 458  | `"filter"`                | Typed Inline/Block constructed inside a user Lua filter (auto-attached).      | yes     |
| `By::sectionize()`           | 470  | `"sectionize"`            | `SectionizeTransform`'s synthesized section `Div`.                            | no      |
| `By::user_edit()`            | 479  | `"user-edit"`             | **Dormant.** Was React-constructed edit content; the `stampUserEdits` stamping path was removed and the current write-back model (`target-incremental-writes.md`) does not stamp edits. Constructor retained, currently unused in production. | no      |
| `By::shortcode(name)`        | 494  | `"shortcode"`             | Result of resolving a `{{< name … >}}` token. **Requires an `Invocation`.**   | yes     |
| `By::include()`              | 505  | `"include"`               | **Dormant.** Was for a planned `IncludeExpansion` wrapper; that design (Plan 8) is abandoned — `IncludeExpansionStage` splices flat and includes round-trip without a wrapper (see Plan 8 tombstone). Constructor retained, currently unused. | n/a |
| `By::title_block()`          | 513  | `"title-block"`           | Title-block stage's synthesized title `h1`.                                   | yes     |
| `By::footnotes()`            | 521  | `"footnotes"`             | Footnotes stage's container `Div` chrome.                                     | no      |
| `By::appendix()`             | 529  | `"appendix"`              | Appendix-structure stage's wrapper `Div` and its helpers.                     | no      |
| `By::tree_sitter_postprocess()` | 538 | `"tree-sitter-postprocess"` | Parser-side synthetic Spaces (e.g. citation/suffix separator).             | yes     |
| `By::test_scaffold()`        | (7f) | `"test-scaffold"`         | Test fixtures that require a source_info but have no real provenance to record. Paired with `SourceInfo::for_test()`. | no      |
| `By::config_default()`       | (7f) | `"config-default"`        | Empty-Map sentinel `ConfigValue` used in metadata merging when no value is present.                | no      |
| `By::programmatic_config()`  | (7f) | `"programmatic-config"`   | WASM-bridge programmatic construction of nested `ConfigValue` (`ConfigValue::from_path`).         | no      |
| `By::unknown()`              | (7f) | `"unknown"`               | "We don't know" placeholder. Used by `json::read_completing_source_info` for nodes deserialized from JSON without `s:` (the call site is explicit about reading outside-world JSON; the placeholder is honest about not knowing). The completing reader takes a `default_by: By` parameter and allocates a fresh pool entry on each missing `s:` — no reserved pool slot. | no      |
| `By::raw(kind, data)`        | 552  | open                      | Escape hatch for extension-defined kinds.                                     | no      |

**Extension namespacing.** Third-party transforms going through
`By::raw` must namespace their kind as `ext/<extension>/<kind>` (e.g.
`ext/quarto-mermaid/diagram`). The `is_atomic_kind` set never matches
extension kinds — they are non-atomic by default in v1.

## 3. Adding a new `By::` kind

Worked example, using `bd-12vrr` (callout default-title synthesizer)
as the reference:

1. **Constructor.** Add `pub fn callout() -> Self` to
   [`crates/quarto-source-map/src/source_info.rs`](../../crates/quarto-source-map/src/source_info.rs)
   alongside the existing constructors. Pick a kebab-case `kind`
   string (`"callout"`); leave `data` as `Value::Null` unless the
   kind carries per-instance configuration.
2. **Atomicity decision.** Decide whether the new kind belongs in
   `is_atomic_kind` (line 570). Default: **no**. Yes only if the
   round-trip rule is "treat the entire subtree as one
   non-user-editable unit" (see §7). Document the decision in the
   beads issue.
3. **Fix the site.** Replace the `SourceInfo::default()` at the
   producer with
   `SourceInfo::Generated { by: By::callout(), from: smallvec![] }`
   (or with an `Invocation` anchor if the new kind resolves a
   user-written construct).
4. **Test.** Add a per-transform shape test next to the existing tests
   for that transform (e.g.
   `test_create_callout_title_has_generated_provenance`), asserting
   the produced shape directly.

The shape test is the per-kind contract — if it fails, the producer
broke the rule. The audit-completion sweep (Plan 6) catches *missing*
provenance; per-transform tests catch *wrong* provenance.

## 4. `from[]` vs. `by.data`

**Source-info pointers go in `from[]` as typed `Anchor`s. Per-instance
configuration that is not a source pointer goes in `by.data` as
JSON.** The two are not interchangeable:

```rust
SourceInfo::Generated {
    by: By {
        kind: "shortcode".to_string(),
        data: serde_json::json!({ "name": "meta" }),  // NOT a source pointer
    },
    from: smallvec![
        Anchor::invocation(Arc::clone(&token_arc)),    // source pointer — typed role
    ],
}
```

The defined `AnchorRole`s are `Invocation`, `ValueSource`, and
`Other(String)`. New roles are added as enum variants, not as `by.data`
fields. The canonical migration example is **bd-36fr9** (Lua filter
file registration in `SourceContext`): once Lua files have a
`FileId`, the `filter_path`/`line` pair currently living in
`by.data` migrates to a typed `Dispatch` anchor in `from[]`, and
`by.data` for `filter`-kind nodes shrinks to per-kind config only.
Treat that as the worked example whenever you're tempted to put a
path-or-range pair in `by.data`.

### Role-asymmetry — only `Invocation` drives byte-copy

**The writer walks `Invocation` only.** `ValueSource` and `Other(...)`
are diagnostic-only: attribution machinery may consult them, but the
writer's `preimage_in` skips past them and they never produce
verbatim-copy bytes. (`preimage_in` lives in
[`source_info.rs`](../../crates/quarto-source-map/src/source_info.rs);
the incremental writer consumes it in `pampa/src/writers/incremental.rs`.)

The producer-side implication: attaching `ValueSource` to a synthesized
node is fine for diagnostic richness (attribution will surface the
metadata range), but it will **not** make the writer copy bytes from
that range into the output. If you want a node's bytes to come from
a specific source range on round-trip, that range must be reachable
through `Invocation`. Extension authors writing custom attribution
via `Other("…")` get the same forward-compat guarantee: whatever they
point at will never be turned into rendered bytes by accident.

## 5. Enrichment-via-post-walk pattern

**When you wrap a dispatch and want to layer your own context on top
of provenance the dispatch already attached, walk the result, append
your anchor, and promote `by.kind` — preserving prior `by.data`
fields, renaming where the new context demands.** This is the
canonical pattern for "transform A constructed via transform B."

Reference implementation:
[`stamp_shortcode_anchors`](../../crates/quarto-core/src/transforms/shortcode_resolve.rs)
+ [`enrich_or_create`](../../crates/quarto-core/src/transforms/shortcode_resolve.rs)
at `crates/quarto-core/src/transforms/shortcode_resolve.rs:524` (entry
point) and `:774` (the promote/preserve helper). The relevant shape
of `enrich_or_create` is:

```rust
let by = match existing {
    SourceInfo::Generated { by, .. } if by.kind == "filter" => {
        // promote filter -> shortcode, rename filter_path -> lua_path
        let lua_path = by.data.get("filter_path").cloned();
        let lua_line = by.data.get("line").cloned();
        let mut data = serde_json::json!({ "name": name });
        if let Some(p) = lua_path { data["lua_path"] = p; }
        if let Some(l) = lua_line { data["lua_line"] = l; }
        By { kind: "shortcode".to_string(), data }
    }
    _ => By::shortcode(name),
};
SourceInfo::Generated {
    by,
    from: smallvec![Anchor::invocation(Arc::clone(token_arc))],
}
```

Three rules to apply when copying the pattern:

- **Append, don't replace.** New anchors join `from[]`; prior anchors
  stay.
- **Promote, don't downgrade.** `by.kind` moves to a more specific
  context (here: `filter` → `shortcode`). Going the other way drops
  information.
- **Preserve prior `by.data`, renaming for context.** Filter dispatch
  recorded `filter_path` / `line`; the shortcode context renames
  them `lua_path` / `lua_line`. Nothing is discarded.

The post-walk must also recurse into nested AST so every node in the
returned subtree gets the anchor — model the walk on
[`stamp_inline`](../../crates/quarto-core/src/transforms/shortcode_resolve.rs)
(`:546`) and
[`stamp_block`](../../crates/quarto-core/src/transforms/shortcode_resolve.rs)
(`:612`) rather than the narrower walkers in `callout.rs` /
`theorem.rs` (block-only — they miss `Image.alt` / `Note.content`).

## 6. `AttrSourceInfo` positional alignment + threaded-source pattern

**`AttrSourceInfo.attributes[i]` is the `(key_src, val_src)` pair for
the i-th entry of the parallel `Attr.2` (`LinkedHashMap`) in
insertion order.** Two preexisting parser paths break this invariant
(**bd-3aolj** duplicate-key handling, **bd-1e6a5** caption-attr merge
into table); see
[`crates/quarto-pandoc-types/src/attr.rs:28`](../../crates/quarto-pandoc-types/src/attr.rs)
for the full doc comment.

When a transform needs the value's source range — e.g. lifting an
attribute value into a typed Inline — thread `&div.attr_source` through
and index *before* mutating `attr.2`:

```rust
let name_idx = kvs.keys().position(|k| k == "name")?;
// Empty attr_source signals "no provenance" (the common test pattern).
// Only assert on a populated-but-misaligned attr_source — that's the
// bd-3aolj / bd-1e6a5 failure mode worth catching in dev.
debug_assert!(
    attr_source.attributes.is_empty()
        || kvs.len() == attr_source.attributes.len(),
    "AttrSourceInfo.attributes is out of sync with Attr.2 (bd-3aolj / bd-1e6a5)"
);
let value_source = if kvs.len() == attr_source.attributes.len() {
    attr_source.attributes[name_idx].1.clone()
} else {
    None
};
let name = kvs.remove("name")?;
// ... use value_source.unwrap_or_default() as the new node's source_info.
```

Reference:
[`crates/quarto-core/src/transforms/theorem.rs:314`](../../crates/quarto-core/src/transforms/theorem.rs)
(`extract_name_attr`), with a parallel implementation in
[`crates/quarto-core/src/transforms/proof.rs:162`](../../crates/quarto-core/src/transforms/proof.rs).

**The strict form is wrong.** `debug_assert_eq!(kvs.len(),
attr_source.attributes.len())` fires on the common
`AttrSourceInfo::empty()` test pattern (an `Attr` with non-empty `kvs`
constructed by hand without provenance) and panics every existing
theorem/proof test. The "empty OR equal" form is required so empty
provenance signals "unknown," not "bug." Future contributors will hit
this footgun if they copy the wrong form from a draft plan.

## 7. Atomic-kind set and consumer impact

**`is_atomic_kind()` controls how downstream consumers treat the
node, not whether the node carries an anchor.** The §2 catalog
above is the canonical enumeration of which kinds are atomic.

For producer authors: the rule is "new kinds default to **non-atomic**."
Promote to atomic only when the round-trip rule for nodes you emit
is "the entire subtree is one inseparable unit the user can't edit
in-place." Extension kinds (`ext/<extension>/<kind>`) are never atomic
in v1 — `is_atomic_kind` matches builtin kebab-case names only.

The consumer of `is_atomic_kind` today is the React framework gate
(`ATOMIC_KINDS` in `ts-packages/preview-renderer/src/framework/dispatch.tsx`),
which marks atomic subtrees as read-only DOM regions. This contract
just says "make the decision deliberately, default no."

## 8. Required-anchor invariants

**`by.kind == "shortcode"` always carries at least one `Invocation`
anchor.** The producer (the stamper in §5) enforces this. On the
consumer side, the writer (`pampa/src/writers/incremental.rs`) adds a
`debug_assert!` that catches an extension calling `By::raw("shortcode", …)`
without the anchor — distinguishing "missing `Invocation` is a bug"
(shortcode) from "missing `Invocation` is the normal shape" (filter /
title-block / tree-sitter-postprocess).

The pattern generalizes: when a new kind always has a source-side
preimage (e.g. a hypothetical `By::macro_expansion(name)`), declare
the invariant here, enforce it at the producer, and add the
corresponding consumer-side assert in the writer. Kinds that
*sometimes* have a preimage (sectionize wraps existing content; the
inner `Header` carries the original `source_info`, but the wrapper
`Div` doesn't) are not in this set — they emit `from: smallvec![]`
and don't require any anchor.

These "no source token of its own" wrappers (Generated, no
Invocation, block-container with source-bearing children — `sectionize`,
`appendix`, footnotes container, …) emit `from: []`; §2's catalog is
the producer-side enumeration.

## 9. Outliers — call-site threading vs. the stamper

**Two shortcode-related sites bypass the stamper because they don't
flow through the dispatch funnel:**

- [`make_error_inline`](../../crates/quarto-core/src/transforms/shortcode_resolve.rs)
  (`:1352`) — `?key` Strong wrapping the unknown-shortcode message.
- [`shortcode_to_literal`](../../crates/quarto-core/src/transforms/shortcode_resolve.rs)
  (`:1368`) — `{{</ … >}}` escaped-shortcode literal text.

Both branches consume their `shortcode_owned.source_info` directly
and emit an `Original` (the user-visible bytes belong to the token,
not to a synthesized replacement). `is_atomic_kind()` does
not fire on `Original`, so error/escaped regions round-trip
verbatim-copy as plain user content.

The pattern to recognize: **if the result variant is `Preserve` or
`Error` rather than `Inlines`/`Blocks`, the stamper does not run.**
Whenever you add a new `ShortcodeResult`-style enum variant that
short-circuits the dispatch funnel, thread the token's `source_info`
through the call sites and use it as the emitted node's
`source_info` — don't try to retrofit a `Generated{by:shortcode}`
shape onto content the user can edit directly.

## 10. Do-not list

- **Don't emit `SourceInfo::default()` for new synthesized nodes.**
  Use the four-branch decision in §1. `default()` survives in the
  Pandoc JSON reader ([`crates/pampa/src/readers/json.rs:80`](../../crates/pampa/src/readers/json.rs))
  by design (the source bytes genuinely don't exist there) and in
  test scaffolding; everywhere else it's a bug.
- **Don't put source-info pointers in `by.data`.** Add an
  `AnchorRole` variant and a typed `Anchor` in `from[]` instead. See
  §4 and the bd-36fr9 migration.
- **Don't drop existing `by.data` when enriching.** Promote /
  migrate. See §5.
- **Don't introduce a `CustomNode` wrapper for provenance alone.**
  The 2026-05-20 design discussion settled on `Generated` with
  typed anchors instead of `CustomNode("ShortcodeResolution")`-style
  wrappers because the anchor carries the structural information
  cheaply without forcing a new HTML-pipeline resolve transform, a
  React component, and a `qmd` writer arm. (Includes were once the
  one wrapper exception — Plan 8 — on cross-file `FileId` grounds;
  that design is abandoned. The node-edit architecture round-trips
  includes with no wrapper at all: the *untransformed* AST keeps the
  raw `{{< include >}}` token, so edits outside it preserve it
  verbatim and edits inside resolve to the included file and are
  read-only. See the Plan 8 tombstone.) Do not re-litigate.
- **Don't add a `test` arm to a `wasm32` cfg guard** when introducing
  new provenance code paths. See
  [`.claude/rules/wasm.md`](../../.claude/rules/wasm.md) — the
  `#[cfg(any(target_arch = "wasm32", test))]` pattern is prohibited
  because it forces native tests through the WASM-restricted Lua
  stdlib and fails on Windows.

## Follow-ups (named, not designed here)

- **bd-129m3** — `ValueSource` anchor stamping for `meta` / `var`
  shortcodes once the metadata loader threads per-key source-info
  through. Integration point is `enrich_or_create` (§5).
- **bd-36fr9** — `Dispatch` anchor for Lua-handler filter / shortcode
  source location, once Lua files are registered in `SourceContext`.
  Migration example for §4.
- **bd-12vrr** — Callout default-title synthesizer needs a `By::callout()`
  constructor + atomicity decision. The §3 worked example.
- **bd-1inj0** — Code-block decoration synthesizers
  (`code_block_generate` / `code_block_render`) — another small audit
  pass to bring into this contract.
- **bd-3aolj** / **bd-1e6a5** — Parser-side `AttrSourceInfo` /
  `Attr.2` alignment bugs that the §6 guard works around.

## Tiling precondition (Plan 7g — BP prerequisite)

This section is the **producer-side precondition** required for the
incremental writer's Byte Provenance (BP) guarantee to hold. The writer-side
work — the tiling auditor, the census, and the b43fadef boundary fix — is in
[Plan 7g](../plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md).

### P1 — Tight ranges

A node's `source_info` covers **exactly the bytes that constitute it**: its
own delimiters included (a code span includes its backticks), surrounding
whitespace excluded.

**Implemented** (2026-06-03, Plan 7g Phase 3): `code_span_helpers.rs`,
`citation.rs`, `quote_helpers.rs`, `postprocess.rs` math-with-attr Span.
Use `tight_source_info_for_node(node, ctx)` and
`leading_whitespace_source_info(&whole, &tight)` from `location.rs` when
peeling a leading `Space`.

**Also implemented** (2026-08-22, bd-1d6io): the attribute key path — the
`"key_value_key"` arm in `treesitter.rs`. It records a `Range`, not a
`SourceInfo`, so it uses `tight_node_location(node, input_bytes)` (the
`Range` counterpart, added alongside `node_location`). Reach for that helper
whenever a handler stores a `Range` over a whitespace-absorbing external
token; it trims with `str::trim` semantics so a caller that records
`node.utf8_text(..).trim()` keeps text and range in lockstep.

**Also implemented** (2026-08-22, bd-1d6io): the attribute key path — the
`"key_value_key"` arm in `treesitter.rs`. It records a `Range`, not a
`SourceInfo`, so it uses `tight_node_location(node, input_bytes)` (the `Range`
counterpart, added alongside `node_location`). Reach for that helper whenever a
handler stores a `Range` over a whitespace-absorbing external token; it trims
with `str::trim` semantics so a caller that records
`node.utf8_text(..).trim()` keeps text and range in lockstep.

### P2 — Whitespace ownership (producer obligation, not auditor-checked)

Inter-token ASCII whitespace belongs to a `Space` node (or block structure)
with its own tight range. This is a *producer obligation* discharged by the
Phase 3 shared helpers. The auditor enforces its observable consequence — P3.
A direct coverage check ("every inter-token whitespace byte is owned by some
`Space`") is deliberately not built; P3 + P4 pin down the load-bearing half.

### P3 — Symmetry

Trim **both** leading and trailing whitespace — not only leading. Today's
handlers trimmed only leading; the Phase 3 helpers apply `trim_all`.

### P4 — Tiling

Sibling leaf ranges are disjoint, and a parent's range contains its children's.
No source byte is claimed by two sibling nodes, qualified by two refinements:

1. **(Intra-node — NOT a sibling-disjointness exception) the `Concat` hull.**
   A `Concat`'s pieces tile internally and the node presents as one unit to its
   siblings — exactly one claim, never two. A *contiguous* `Concat` presents
   its hull; a *non-contiguous* one makes no contiguous claim (`preimage_in`
   → `None`). Use `contiguous_hull_for_run` (in `postprocess.rs`) to produce
   a tight `Original` hull when coalescing adjacent source-adjacent inlines.

2. **(Inter-node — the only genuine overlap exception) atomic N-to-1
   same-`Invocation` groups.** One source construct (e.g. a block shortcode
   `{{< lipsum 3 >}}`) expands to N sibling nodes, each stamped `Generated`
   with the same `Invocation` anchor. The writer coalesces same-`Invocation`
   runs and emits `R` once; the auditor partitions siblings by `Invocation`
   anchor identity before checking disjointness.

**Scope boundary**: P4 is *non-overlap*, not *gap-free partition*. Blank lines
between blocks, `> ` gutters, and list indentation are legitimately unowned;
the BP proof tolerates them (Deleted/gap categories).

### Semantic-ownership rule for `None`-resolving `Concat`

A non-contiguous `Concat` (`preimage_in` → `None`) must be classified:

- **All pieces resolve AND every inter-piece gap is space/tab only** → producer
  bug; fix with `contiguous_hull_for_run` (Phase 4b template).
- **Any gap has non-whitespace/newline, or a piece fails to resolve** → not
  auto-blessed; requires the Phase 2 World 1 / World 2 triage gate. The failure
  mode is mis-classifying genuine scatter as a fixable bug, so stop and report
  to the user before applying the auto-fix.

### CI enforcement

The tiling auditor (`audit_source_range_tiling` in
`crates/pampa/src/writers/incremental.rs`) runs as a property test in CI
(Phase 7, Plan 7g). The gate asserts zero `SiblingOverlap`,
`ContainmentViolation`, and `ScatteredConcat` findings across the pampa corpus.

## Change log

- **2026-05-25 — v1.** Initial version, written after Plan 6 landed
  on `feature/provenance` (2026-05-22). Documents the conventions
  that survived implementation:
  four-branch decision tree, `By::` catalog, enrichment pattern,
  `AttrSourceInfo` threading recipe (with the relaxed
  `debug_assert!` form), atomic-kind / required-anchor invariants,
  outlier call-site threading, and a do-not list. Plan-6 audit
  report lives separately at
  [`claude-notes/research/2026-05-22-plan-6-audit.md`](../research/2026-05-22-plan-6-audit.md).
- **2026-05-25 — v1.1.** Two substantive edits: §1 decision tree gains a
  row distinguishing filter *mutations* (keep input's `Original`) from filter
  *constructions* (auto-attached `Generated{by:filter}`); §4 documents the
  role-asymmetry — only `Invocation` drives the writer's byte-copy,
  `ValueSource` / `Other` are diagnostic-only.
- **2026-06-05 — v1.2.** Pruned the withdrawn Plan-7 write-back model
  (replacement: `target-incremental-writes.md`). Removed the cross-references to
  the deleted `incremental-writer-contract.md`; the consumer side now points at
  Plan 7g and §"Tiling precondition". Marked `By::user_edit()` dormant (its
  `stampUserEdits` stamping path was reverted). Made §2's catalog the canonical
  atomic-kind enumeration (§7/§8 no longer defer to a separate writer-contract
  doc). Dropped the reverted `Transparent`/`Omit`/soft-drop dispatch language.

# Provenance contract — emitting `SourceInfo` from a transform

**Status:** Active (Plan 6 landed 2026-05-22 on `feature/provenance`).
**Types:** `quarto_source_map::SourceInfo`, `By`, `Anchor`, `AnchorRole`
([`crates/quarto-source-map/src/source_info.rs`](../../crates/quarto-source-map/src/source_info.rs)).
**Plans:**
[Plan 4](../plans/2026-05-04-q2-preview-plan-4-sourceinfo-anchors.md)
(types) ·
[Plan 5](../plans/2026-05-04-q2-preview-plan-5-wire-format.md)
(wire format) ·
[Plan 6](../plans/2026-05-04-q2-preview-plan-6-provenance-audit.md)
(this audit) ·
[Plan 7](../plans/2026-05-04-q2-preview-plan-7-incremental-writer.md)
(writer / consumer) ·
[Plan 8](../plans/2026-05-04-q2-preview-plan-8-include-wrapper.md)
(include wrapper).
**Audit report:** [`claude-notes/research/2026-05-22-plan-6-audit.md`](../research/2026-05-22-plan-6-audit.md).
**Companion doc:** [`incremental-writer-contract.md`](incremental-writer-contract.md)
covers the *consumer* side — what the writer does with the `SourceInfo`
shapes this doc tells producers to emit. The two are designed in pairs:
if you change either contract, check the other.

## Summary

Every `SourceInfo` a transform emits must accurately describe where the
node came from. The Plan 4 types give four physical shapes (`Original`,
`Substring`, `Concat`, `Generated`); this doc is the contract for which
shape to pick. The rule that follows replaces the historical "stamp
`SourceInfo::default()` and move on" pattern that Plan 6 audited out
of the transform layer.

## 1. Decision tree for new transforms

**Wire-format requirement.** Every AST node in the JSON wire format
carries an `s:` field referencing a valid `sourceInfoPool` entry
(post-Plan-7f, after the wire-format renames, the field is `s:` and
the pool is `p:`). The Rust JSON reader rejects bare nodes with
`Err(JsonReadError::MissingSourceInfoRef { node_path })`. There is no
fallback to `SourceInfo::default()` and no silent stamping; producers
are responsible for populating `s:` on every node, and the reader's
strictness keeps the contract honest by surfacing producer bugs at
the JSON boundary rather than at the writer. The React framework's
`stampUserEdits` walker (Plan 7f Phase 3) is the TS-side mechanism
that satisfies this requirement for user-edit content.

**Pick the shape from where the emitted node's *bytes* come from, not
from how it was constructed.** Four branches:

| Source of the emitted node              | Shape                                                                                                                          |
|-----------------------------------------|--------------------------------------------------------------------------------------------------------------------------------|
| Corresponds to source bytes             | `Original` — `ctx.source_info.clone()`, or clone the input node's `source_info` field. Never construct an `Original` by hand.  |
| Pure synthesis with no preimage         | `Generated { by: By::<kind>(), from: smallvec![] }`                                                                            |
| Resolution of a user-written construct  | `Generated { by: By::<kind>(name), from: smallvec![Anchor::invocation(Arc::new(token_si))] }`                                  |
| **Mutation** of a node a filter received (e.g. `Str.text = upper(...)`) | Leave the input node's `Original` source_info untouched. Filter *mutations* are not classified atomic; do **not** rewrite to `Generated`. See [`incremental-writer-contract.md`](incremental-writer-contract.md) §"Filter mutations versus constructions" for the round-trip implications. |
| **Construction** inside a user Lua filter (e.g. `pandoc.Str("...")`) | Leave it alone — `filter_source_info` ([`crates/pampa/src/lua/types.rs:1813`](../../crates/pampa/src/lua/types.rs)) auto-attaches `Generated { by: filter, ... }` on the way out. |

If two branches feel equally applicable, pick the one with the longer
chain to source: the writer (Plan 7) and attribution
(`resolve_byte_range`) both prefer `Original` over `Generated{from:[]}`
and `Generated{from:[Invocation]}` over `Generated{from:[]}`.

## 2. `By::` constructor catalog

The known producer kinds, defined in
[`crates/quarto-source-map/src/source_info.rs`](../../crates/quarto-source-map/src/source_info.rs):

| Constructor                  | Line | `kind` string             | Purpose                                                                       | Atomic? |
|------------------------------|------|---------------------------|-------------------------------------------------------------------------------|---------|
| `By::filter(path, line)`     | 458  | `"filter"`                | Typed Inline/Block constructed inside a user Lua filter (auto-attached).      | yes     |
| `By::sectionize()`           | 470  | `"sectionize"`            | `SectionizeTransform`'s synthesized section `Div`.                            | no      |
| `By::user_edit()`            | 479  | `"user-edit"`             | React-constructed content reaching the AST through the q2-preview client.    | no      |
| `By::shortcode(name)`        | 494  | `"shortcode"`             | Result of resolving a `{{< name … >}}` token. **Requires an `Invocation`.**   | yes     |
| `By::include()`              | 505  | `"include"`               | `IncludeStage` expansion wrapper (Plan 8); most include children stay `Original`. | (Plan 8) |
| `By::title_block()`          | 513  | `"title-block"`           | Title-block stage's synthesized title `h1`.                                   | yes     |
| `By::footnotes()`            | 521  | `"footnotes"`             | Footnotes stage's container `Div` chrome.                                     | no      |
| `By::appendix()`             | 529  | `"appendix"`              | Appendix-structure stage's wrapper `Div` and its helpers.                     | no      |
| `By::tree_sitter_postprocess()` | 538 | `"tree-sitter-postprocess"` | Parser-side synthetic Spaces (e.g. citation/suffix separator).             | yes     |
| `By::test_scaffold()`        | (7f) | `"test-scaffold"`         | Test fixtures that require a source_info but have no real provenance to record. Paired with `SourceInfo::for_test()`. | no      |
| `By::config_default()`       | (7f) | `"config-default"`        | Empty-Map sentinel `ConfigValue` used in metadata merging when no value is present.                | no      |
| `By::programmatic_config()`  | (7f) | `"programmatic-config"`   | WASM-bridge programmatic construction of nested `ConfigValue` (`ConfigValue::from_path`).         | no      |
| `By::reconcile_synthesize()` | (7f) | `"reconcile-synthesize"`  | AST nodes the reconciler synthesizes during `apply_reconciliation` paths that don't correspond to either input. | no      |
| `By::raw(kind, data)`        | 552  | open                      | Escape hatch for extension-defined kinds.                                     | no      |

**On `By::user_edit`.** This is the **single** stamping kind for React-constructed content; q2 does not distinguish between different editor affordances (toggle-blockquote, add-list-item, wrap-in-callout, etc.) at the source_info level. The framework stamps `Generated{by: user_edit, …}` on every node a `setLocalAst` call creates, with no per-affordance specialization. This is intentional: Rust does not know about React render components, and a single kind keeps the producer contract simple and matches the framework's existing pattern.

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

**The writer walks `Invocation` only.** `ValueSource`, `Dispatch`
(when it lands), and `Other(...)` are diagnostic-only: attribution
machinery may consult them, but the writer's `preimage_in` skips
past them and they never produce verbatim-copy bytes. See
[`incremental-writer-contract.md`](incremental-writer-contract.md)
§"The role-asymmetry contract on `Generated.from`" for the rule
and rationale.

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
above marks which kinds are currently atomic; the canonical
enumeration plus the shortcode-only debug-assert table lives in
[`incremental-writer-contract.md`](incremental-writer-contract.md)
§"Atomic-kind `Generated` and the shortcode-only invariant."

For producer authors: the rule is "new kinds default to **non-atomic**."
Promote to atomic only when the round-trip rule for nodes you emit
is "the entire subtree is one inseparable unit the user can't edit
in-place." Extension kinds (`ext/<extension>/<kind>`) are never atomic
in v1 — `is_atomic_kind` matches builtin kebab-case names only.

Two consumers consult `is_atomic_kind` today: Plan 7's writer (round-
trip / soft-drop) and Plan 2A's React framework gate (read-only DOM
regions). The writer doc covers both behaviors; this contract just
says "make the decision deliberately, default no."

**Where the writer's internal shape is pinned:**
[`incremental-writer-contract.md`](./incremental-writer-contract.md)
§"`CoarsenedEntry` self-containment" documents the rule that
every emitted entry must be self-contained, and how the atomic-kind
decision flows into the choice of `Verbatim` (atomic with preimage)
vs `Omit` (atomic without preimage) vs `Rewrite` (non-atomic
catch-all) vs `Transparent` (non-atomic wrapper with source-bearing
children) at coarsen time.

## 8. Required-anchor invariants

**`by.kind == "shortcode"` always carries at least one `Invocation`
anchor.** The producer (the stamper in §5) enforces this; the writer
adds a consumer-side `debug_assert!` so an extension that calls
`By::raw("shortcode", …)` without the required anchor is caught. The
writer-side table that distinguishes "missing `Invocation` is a bug"
(shortcode) from "missing `Invocation` is the normal shape"
(filter / title-block / tree-sitter-postprocess) lives in
[`incremental-writer-contract.md`](incremental-writer-contract.md)
§"Atomic-kind `Generated` and the shortcode-only invariant."

The pattern generalizes: when a new kind always has a source-side
preimage (e.g. a hypothetical `By::macro_expansion(name)`), declare
the invariant here, enforce it at the producer, and add the
corresponding consumer-side assert in the writer doc. Kinds that
*sometimes* have a preimage (sectionize wraps existing content; the
inner `Header` carries the original `source_info`, but the wrapper
`Div` doesn't) are not in this set — they emit `from: smallvec![]`
and don't require any anchor.

**Sibling contract for these "no source token of its own" wrappers:**
see [`transparent-wrappers.md`](./transparent-wrappers.md). It names
the shape (Generated, no Invocation, block-container with
source-bearing children) and pins the *consumer* rule: any code
that asks "where do the user's source bytes live?" must descend
through transparent wrappers via `first_in_user_tree`, not read
`blocks[0]` directly. The producer side of that — what wrapper
kinds emit `from: []` — lives here in §2's catalog (`sectionize`,
`appendix`, footnotes container, …); the descent invariant lives
there. Adding a new `By::` kind that produces a block-container
wrapper should cross-reference both docs.

## 9. Outliers — call-site threading vs. the stamper

**Two shortcode-related sites bypass the stamper because they don't
flow through the dispatch funnel:**

- [`make_error_inline`](../../crates/quarto-core/src/transforms/shortcode_resolve.rs)
  (`:1352`) — `?key` Strong wrapping the unknown-shortcode message.
- [`shortcode_to_literal`](../../crates/quarto-core/src/transforms/shortcode_resolve.rs)
  (`:1368`) — `{{</ … >}}` escaped-shortcode literal text.

Both branches consume their `shortcode_owned.source_info` directly
and emit an `Original` (the user-visible bytes belong to the token,
not to a synthesized replacement). Plan 7's `is_atomic_kind()` does
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
  React component, and a `qmd` writer arm. Wrappers remain
  appropriate for the include case (Plan 8) because the cross-file
  `FileId` problem genuinely needs anchoring at the parent-file
  level. Do not re-litigate.
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
- **2026-05-25 — v1.1.** Cross-linked the consumer-side
  [`incremental-writer-contract.md`](incremental-writer-contract.md)
  that landed in parallel on Plan 7's review branch. Three
  substantive edits: §1 decision tree gains a row distinguishing
  filter *mutations* (keep input's `Original`) from filter
  *constructions* (auto-attached `Generated{by:filter}`); §4
  documents the role-asymmetry — only `Invocation` drives the
  writer's byte-copy, `ValueSource` / `Dispatch` / `Other` are
  diagnostic-only; §7 / §8 now defer the canonical atomic-kind
  enumeration and shortcode-only debug-assert table to the
  writer-contract doc rather than duplicating them.

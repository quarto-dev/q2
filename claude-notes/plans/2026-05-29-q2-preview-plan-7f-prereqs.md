# Plan 7f — Prerequisites for Plan 7d

**Date:** 2026-05-29
**Branch:** feature/provenance (sibling to 7d / 7e)
**Status:** Ready for implementation. Ships before 7d.

## Overview

Plan 7d's algebraic refactor of `coarsen` → `plan_user_writes` depends on three pieces of producer-side hygiene that don't yet hold. Plan 7f lands them so that 7d's strict R5 trust point is meaningful and so that BP is not silently violated by upstream sloppiness.

Three workstreams, none of which involve the writer itself:

1. **Framework source_info preservation** — the React framework currently strips `s:` on rebuilt wrappers (Emph, Strong, Para, every passthrough except the top-level Ast). Fix the recursion to spread source_info forward.
2. **User-edit stamping** — a single reserved pool slot for `Generated{by: user_edit, …}`; the framework stamps it on user-constructed nodes.
3. **`SourceInfo::default()` deprecation** — replace test usages with explicit kinds; deprecate the `Default` impl; surface real provenance decisions in production residue.

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

Work items:

- [ ] Walk every entry in `renderChildrenRegistry`. Record a checklist row per renderer: "preserves" vs "strips."
- [ ] Verify `makeFlatInlineRenderer` separately (one helper, multiple renderers).
- [ ] Verify `renderCustomNodeChildren` (custom-node generic walk).

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
    if ('c' in stamped && Array.isArray(stamped.c)) {
        return {
            ...stamped,
            c: stamped.c.map(child =>
                typeof child === 'object' && child !== null && 't' in child
                    ? stampUserEdits(child as BlockNode | InlineNode)
                    : child)
        };
    }
    return stamped;
}
```

`<Node>` wraps the incoming `setLocalAst` and passes `(newNode) => setLocalAst(stampUserEdits(newNode))` to the child renderer. The walker only stamps subtrees lacking `s:`; preserved subtrees keep their existing source_info.

The constant `USER_EDIT_SOURCE_INFO_ID` is the reserved pool slot (Phase 4).

Work items:

- [ ] Implement `stampUserEdits` walker.
- [ ] Wire into `<Node>` component's `setLocalAst` propagation.
- [ ] TS test: user component constructs a new Span via `setLocalAst({ t: 'Span', c: ... })`; assert the resulting node has `s: USER_EDIT_SOURCE_INFO_ID` after stamping.
- [ ] TS test: preserved subtree (rebuilt-wrapper case) keeps original `s:` after stamping passes through it.

## Phase 4 — Reserved pool slot for user_edit

The Rust JSON writer (`crates/pampa/src/writers/json.rs`) currently builds the `sourceInfoPool` as a used-only intern table during AST traversal. Change `SourceInfoSerializer::new()` to pre-push a `Generated{by: By::user_edit(), from: smallvec![]}` entry at index 0 before any traversal interns. All subsequent intern operations get IDs ≥ 1.

Export a TypeScript constant:

```ts
// ts-packages/preview-renderer/src/types/sourceInfo.ts
export const USER_EDIT_SOURCE_INFO_ID = 0;
```

The framework's stamping references this constant.

The pool stays Rust-authoritative: the framework only ever *references* pool IDs; it never allocates. The user-edit slot exists in every JSON document the writer produces, regardless of whether any node references it.

Work items:

- [ ] Rust: `SourceInfoSerializer::new()` pre-pushes the user_edit entry at index 0.
- [ ] Rust: adjust all `Vec<SerializableSourceInfo>` traversals that assume "pool starts empty" — they now start with one entry.
- [ ] TS: export `USER_EDIT_SOURCE_INFO_ID = 0` as a typed constant.
- [ ] Rust test: round-trip a hand-constructed AST through the WASM bridge; assert `sourceInfoPool[0]` decodes as `Generated{by: user_edit}`.

## Phase 5 — Wire-format renames

Two JSON top-level fields in `crates/pampa/src/writers/json.rs` get single-character names to match the rest of the wire format:

- `attrS` (currently camelCase from `attr_s: AttrSourceJson`) → `a`. Apply `#[serde(rename = "a")]` to the field.
- `sourceInfoPool` (currently camelCase from `source_info_pool: Vec<SourceInfoJson>`) → `p`. Same mechanism.

Multi-character fields inside `AttrSourceJson` (`classes`, `id`, `kvs`) stay — they're Pandoc-standard. `pandoc-api-version` stays — Pandoc-legacy.

Work items:

- [ ] Rust: apply `#[serde(rename = "a")]` to the `attr_s` field; remove the camelCase fallback for it.
- [ ] Rust: apply `#[serde(rename = "p")]` to the `source_info_pool` field.
- [ ] Rust: update `crates/pampa/src/readers/json.rs` to read the renamed fields.
- [ ] TS: update `ts-packages/preview-renderer/src/types/` and `hub-client/src/types/wasm-quarto-hub-client.d.ts` to match.
- [ ] Test: round-trip the largest existing JSON fixture; assert byte-equivalent after the rename.

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

**Production residue.** The non-test `SourceInfo::default()` usages — `crates/quarto-pandoc-types/src/config_value.rs`, `crates/quarto-yaml-validation/src/validator.rs`, `crates/quarto-yaml-validation/src/schema/*.rs`, `crates/quarto-doctemplate/src/*.rs` — are not test scaffolding. Each call site represents a real provenance decision that was deferred when Plan 6's audit went through library code but didn't cover ConfigValue / YAML / template helpers. The audit hands these to their respective module maintainers to decide:

- ConfigValue synthesis during merge: likely `Generated{by: By::raw("config-merge", _), from: smallvec![]}` or similar.
- YAML validator synthesized values: a similar `Generated` kind appropriate to the validator.
- Doctemplate eval-context defaults: per-site judgment.

The replacement target is **not** `user_edit`. `user_edit` applies only to React-constructed content. Every other caller decides their own provenance kind.

Work items:

- [ ] Add `By::test_scaffold()` constructor in `quarto-source-map`.
- [ ] Add `SourceInfo::for_test()` convenience in `quarto-source-map`.
- [ ] Audit test-file usages of `SourceInfo::default()`; replace with one of the four patterns above.
- [ ] For production residue (~20 sites), file as individual review items routed to each module's maintainer. Do not block 7f on full production cleanup; the deprecation in Phase 7 surfaces remaining sites.
- [ ] Verify: `cargo nextest run --workspace` passes after replacements.

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

The `#[deprecated]` attribute surfaces remaining call sites at compile time with a clear message. CI's `-D warnings` would block the build, so 7f keeps deprecation but does not enforce removal. Each newly-flagged site gets a deliberate fix; the impl can be fully removed in a follow-up after the dust settles.

Work items:

- [ ] Add `#[deprecated]` to `impl Default for SourceInfo`.
- [ ] Suppress the warning at the few remaining production residue sites with `#[allow(deprecated)]` and a TODO pointing to the module-maintainer review.
- [ ] Verify: `cargo xtask verify --skip-hub-build` (Rust-only) green with deprecation warnings tolerated; no fatal warnings break the build.

## Phase 8 — Verification

- [ ] `cargo xtask verify` clean.
- [ ] All existing tests pass.
- [ ] New tests from Phases 2, 3, 4 pass.
- [ ] Manual smoke test of q2-preview: open a document with shortcodes, edit a paragraph, save, re-open; verify the shortcode tokens are preserved and the framework's `s:` is intact on rebuilt wrappers.

## What 7f does not do

- **No CustomNode serialization.** Custom nodes (Callout, Theorem, etc.) remain broken on edit until 7e. Editing a callout body still results in the callout disappearing from source until 7e lands.
- **No writer changes.** `coarsen` keeps its flat shape; 7d does the algebra refactor.
- **No removal of `Default` impl.** Deprecation only; removal is a follow-up.

## References

- Design doc: [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md).
- Sibling plan (next): [`2026-05-26-q2-preview-plan-7d-algebraic-soundness.md`](2026-05-26-q2-preview-plan-7d-algebraic-soundness.md).
- Producer contract: [`provenance-contract.md`](../designs/provenance-contract.md).
- Playwright fixture convention: `claude-notes/instructions/testing.md` (post-`provenance-reactji-demo` merge).

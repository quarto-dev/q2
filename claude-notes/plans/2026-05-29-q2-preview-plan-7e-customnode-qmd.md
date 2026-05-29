# Plan 7e — CustomNode qmd serialization + edit-survival tests

**Date:** 2026-05-29
**Branch:** feature/provenance (sibling to 7d / 7f)
**Status:** Ready for implementation. Ships after 7d.

## Overview

q2's qmd writer has empty arms for `Block::Custom(_)` and `Inline::Custom(_)` (see `crates/pampa/src/writers/qmd.rs:2354`). `write_block_to_string` on a Callout produces zero bytes, which means any successful edit to a Callout — or to any other non-atomic CustomNode — silently deletes the entire `::: {.callout-…} … :::` block of source bytes. The same fate awaits Theorem, Proof, FloatRefTarget (Figure-as-CustomNode), and labelled display equations.

The bug is not caused by Plan 7d. It exists today on `feature/provenance` and on `main`. Nobody has noticed because no component-driven affordance has tried to edit a callout body yet — the framework's atomic gate doesn't fire on Callout (only `CrossrefResolvedRef` is in `ATOMIC_CUSTOM_NODES`), so the gate would let an edit through, but the qmd writer would then erase the callout on save.

Plan 7e implements the missing CustomNode arms and tests them with Playwright fixtures that simulate real edits.

## Why 7e ships after 7d (not before)

The CustomNode arms benefit from the shell-decomposition shape that 7d's Phase 1 establishes: `serialize_block_shell_open` + `serialize_block_shell_close` + slot rendering. Building the arms once, under 7d's shape, is less work than building them under today's monolithic `write_block` and re-shaping them later. 7d ships without CustomNode coverage — callout editing remains broken throughout 7d — and 7e closes the gap immediately afterward.

If urgency demands it, 7e could ship before 7d using today's monolithic shape, then be re-shaped during 7d Phase 1. That's strictly more work; the current ordering avoids it.

## Phase 1 — Enumerate CustomNode types and their qmd syntax

Catalog the non-atomic CustomNode types in production today:

| `type_name` | Producer | `plain_data` shape | Source qmd syntax |
|---|---|---|---|
| `Callout` | `crates/quarto-core/src/transforms/callout.rs` | `{ type, appearance, title?, collapse? }` | `::: {.callout-<type> appearance="…" title="…" collapse=…}` … `:::` |
| `Theorem` (and theorem-likes: Lemma, Proposition, Corollary, Conjecture, Definition, Example, Exercise, Remark) | `crates/quarto-core/src/transforms/theorem.rs` | `{ ref_type, kind, name? }` | `::: {.<ref_type> #<id> name="…"}` … `:::` |
| `Proof` | `crates/quarto-core/src/transforms/proof.rs` | `{ name? }` | `::: {.proof name="…"}` … `:::` |
| FloatRefTarget (e.g. `Figure`, `Table`, `Listing`) | `crates/quarto-core/src/transforms/float_ref_target.rs` | per ref-type-def fields | varies; figure/table-specific |
| Labelled display equations | crossref machinery | `{ id, label? }` | `$$ … $$ {#eq-id}` |

Atomic types (`CrossrefResolvedRef`, future `IncludeExpansion`) are out of 7e's scope — they keep their existing let-user-win path through R5-special.

- [ ] Confirm the production set above by grepping `type_name:` constructions across `crates/quarto-core/src/transforms/`.
- [ ] For each type, document the exact qmd syntax produced (open shell, slot mapping, close shell).
- [ ] Note: type names ending with "Block" vs without — verify writer dispatches on `Block::Custom` for block-level and `Inline::Custom` for inline-level types.

## Phase 2 — Implement shell helpers per CustomNode type

Add to `crates/pampa/src/writers/qmd.rs`:

```rust
fn serialize_callout_shell_open(plain_data: &Value, attr: &Attr) -> ShellOpen { … }
fn serialize_callout_shell_close() -> Bytes { … }

fn serialize_theorem_shell_open(plain_data: &Value, attr: &Attr) -> ShellOpen { … }
// … one pair per type
```

These produce `ShellOpen::Bytes(…)` (no per-line marker needed for CustomNodes — they're block-shell containers).

Update the `Block::Custom(_)` arm in `write_block` to dispatch on `type_name`:

```rust
Block::Custom(custom) => {
    match custom.type_name.as_str() {
        "Callout"  => write_callout(custom, buf, ctx)?,
        "Theorem" | "Lemma" | …  => write_theorem(custom, buf, ctx)?,
        "Proof"    => write_proof(custom, buf, ctx)?,
        "Figure" | "Table" | "Listing" => write_float_ref_target(custom, buf, ctx)?,
        // Atomic CustomNodes flow through let-user-win at the writer's
        // higher-level dispatch and don't reach this arm; debug_assert.
        other => {
            debug_assert!(false, "Unrecognized CustomNode type_name: {}", other);
        }
    }
}
```

Where `write_callout` (and siblings) emits:

```
shell_open + write_slots(custom.slots, ctx) + shell_close
```

For 7d's algebra, the `Block::Custom` arm is also exposed as shell-decomposed helpers that R3 can consume:

```rust
pub fn customnode_shell_open(custom: &CustomNode) -> ShellOpen { … }
pub fn customnode_shell_close(custom: &CustomNode) -> Bytes { … }
```

The dispatch by `type_name` inside `customnode_shell_open` mirrors the dispatch above.

- [ ] Implement shell helpers for each type from Phase 1.
- [ ] Wire up `Block::Custom` and `Inline::Custom` arms to dispatch on `type_name`.
- [ ] Expose `customnode_shell_open` / `customnode_shell_close` as public helpers for the algebra in `plan_user_writes`.

## Phase 3 — Round-trip tests per CustomNode type

For each type, a hand-authored qmd fixture that round-trips:

```rust
// crates/pampa/tests/customnode_roundtrip_tests.rs
#[test]
fn callout_roundtrips() {
    let source = "::: {.callout-note}\nBody content.\n:::\n";
    let (ast, _) = parse_qmd_to_ast(source.as_bytes()).unwrap();
    let mut out = String::new();
    write_pandoc_to_qmd(&ast, &mut out).unwrap();
    assert_eq!(out, source);
}
```

- [ ] Add a roundtrip test per type. Include variants: with/without title, with classes/attrs, collapsed/expanded callouts, named theorems, captioned figures.
- [ ] Verify all pass under `cargo nextest run -p pampa`.

## Phase 4 — Playwright fixtures for edit survival

Following the convention established by `provenance-reactji-demo` (`claude-notes/instructions/testing.md` §"Fixture organisation: smoke-all vs playwright-fixtures"):

### Fixture: callout body edit

`crates/quarto/tests/playwright-fixtures/q2-preview/callout-edit/`:

- `_quarto.yml` enabling q2-preview with render-components.
- `index.qmd` containing a callout block:
  ```
  ::: {.callout-note}
  Original callout body.
  :::
  ```
- `edit-callout-body.tsx` — a user component registered for `Callout` (via render-components):
  - Renders the callout body normally.
  - Exposes a click target (e.g. an "Edit" button) that calls `setLocalAst` with a modified Callout AST: same `type_name` and `plain_data`, modified content slot (e.g. append "Edited.").

### Spec: `hub-client/e2e/q2-preview-callout-edit.spec.ts`

Mirrors the structure of `q2-preview-render-components-write.spec.ts`:

1. Bootstrap the project via `bootstrapProjectSet` + `seedProjectInBrowser` (the existing helpers).
2. Load the page; wait for the callout to render.
3. Click the user component's edit affordance.
4. Wait for the round-trip (`incremental_write_qmd` → updated qmd → re-render).
5. **Assertion 1:** the rendered DOM still contains a `.callout-note` element. (The callout didn't disappear.)
6. **Assertion 2:** the persisted qmd (read via the WASM bridge) contains `::: {.callout-note}` ... `:::`. (The callout syntax survived.)
7. **Assertion 3:** the persisted qmd contains "Edited." in the callout body. (The user's edit took effect.)
8. **Assertion 4:** no `Q-3-43` warning was emitted. (The edit wasn't soft-dropped.)
9. **Assertion 5:** no `incrementalWriteQmd failed` console error. (No empty-qmd or hard-fail.)

### Fixture: theorem body edit

`crates/quarto/tests/playwright-fixtures/q2-preview/theorem-edit/` — analogous to callout-edit, but for a Theorem CustomNode (`::: {.theorem #thm-pythagoras name="Pythagoras"}` … `:::`).

### Spec: `hub-client/e2e/q2-preview-theorem-edit.spec.ts`

Same structure as the callout spec, asserting the theorem syntax and `#thm-pythagoras` ID survive the edit.

- [ ] Build the two fixtures (callout-edit, theorem-edit) under `playwright-fixtures/q2-preview/`.
- [ ] Build the two specs in `hub-client/e2e/`.
- [ ] Verify locally with `cd hub-client && npx playwright test q2-preview-callout-edit q2-preview-theorem-edit`.
- [ ] Add the specs to the playwright config's test set so CI picks them up.

## Phase 5 — Verification

- [ ] `cargo xtask verify` clean.
- [ ] All existing tests pass.
- [ ] New roundtrip tests (Phase 3) pass.
- [ ] New Playwright specs (Phase 4) pass.
- [ ] Manual smoke: open a qmd with a callout in q2-preview; click the edit affordance; save; reload; observe the callout body shows the edited content and the callout syntax is intact in source.

## What 7e does not do

- **Inline CustomNodes.** All currently-known production CustomNodes are block-level. If inline CustomNodes (e.g. inline ProofRef, inline FloatRef) materialize as a category, 7e extends to cover them; today's scope is block-level.
- **Atomic CustomNode handling.** `CrossrefResolvedRef` (and future `IncludeExpansion`) keep their let-user-win R5-special path via `plain_data` reading. No change.
- **AST extensions.** If a CustomNode shape requires AST changes (new fields, new slot types), those land in a separate plan. 7e works against the existing AST.

## References

- Design doc: [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md).
- Algebraic refactor (predecessor): [`2026-05-26-q2-preview-plan-7d-algebraic-soundness.md`](2026-05-26-q2-preview-plan-7d-algebraic-soundness.md).
- Prerequisites: [`2026-05-29-q2-preview-plan-7f-prereqs.md`](2026-05-29-q2-preview-plan-7f-prereqs.md).
- Playwright fixture convention: `claude-notes/instructions/testing.md` (post-`provenance-reactji-demo` merge).
- Reactji write example: `hub-client/e2e/q2-preview-render-components-write.spec.ts` on the `provenance-reactji-demo` branch — pattern to mirror.
- CustomNode producers: `crates/quarto-core/src/transforms/callout.rs`, `theorem.rs`, `proof.rs`, `float_ref_target.rs`, crossref machinery in `crates/quarto-core/src/crossref/`.

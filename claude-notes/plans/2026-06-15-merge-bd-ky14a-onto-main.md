# Merging bd-ky14a (PR #235, hash-based FileIds) onto current `main`

**Status:** context-gathering / awaiting go-ahead — do NOT start the merge yet
**Braid:** bd-bu5y34fp (`discovered-from` bd-ky14a)
**Implementation strand:** bd-ky14a
**PR:** [#235](https://github.com/quarto-dev/q2/pull/235)
**Branch:** `feature/bd-ky14a-pampa-hash-fileids` (this branch)
**Author of this note:** investigation on 2026-06-15
**Related in-flight PR:** [#286](https://github.com/quarto-dev/q2/pull/286)
(block-editing epic) — see §"Entanglement with #286 and the `d === 0`
assumption" and §"Recommended sequencing".

## TL;DR sequencing recommendation

**Wait for #286 (block-editing epic) to land before merging bd-ky14a.**
The root entanglement is not a merge conflict — it is a *semantic*
assumption, "FileId 0 = the document", that #260 wove into the q2-preview
block-editing source index (`buildSourceIndex` filters `entry.d === 0`;
lookups pass `d: 0`; the Rust siKey is `"0:{start}-{end}:{file_id}"`).
bd-ky14a turns `d` from `0` into a hash, which would **silently empty the
source index** and break block editing / targeted edits in q2-preview.
#286 is *actively extending* that same `d`-keyed machinery right now, so
the assumption is a moving target. Land #286 first, let the siKey surface
settle, then rebase bd-ky14a once and fix the `d`-scheme in one pass.
(Note: waiting does **not** simplify the hardest *merge conflict* —
`engine_execution.rs` — which comes from #260, already on `main`.)

## Why this document exists

PR #235 has been open since before #231 was superseded. #231 → #260
(`feat(provenance): targeted paragraph/header inline editing`) is now
**merged** (2026-06-10), so #235 is unblocked in the scheduling sense.
But it is **not** a clean fast-forward. cscheid asked for a written
context dump before any merge work begins, because the merge turned out
to be genuinely complex — the work that landed in #260 re-introduced, in
a new form, exactly the pattern bd-ky14a set out to remove.

**This note describes the situation. It is not a plan to execute yet.**

## What bd-ky14a does (recap)

pampa's `ASTContext` historically gave the primary document `FileId(0)`
(sequential), while `quarto_yaml` keyed files by `hash(filename)`. The
two schemes collide: the same `FileId` means different files depending
on which parser produced it. bd-ky14a makes pampa adopt
`quarto_yaml::file_id_for_filename(name)` so a `FileId` is globally
meaningful, and **removes** the `quarto_ast_reconcile::remap_file_ids`
workarounds in `include_expansion.rs` and `engine_execution.rs` plus the
parallel-`SourceContext` lockstep.

The PR is a **single commit** (`eb726334`) on top of merge-base
`65f13faf`. The user-visible effect: FileIds in the JSON wire format's
`sourceInfoPool.d` go from `0,1,2…` to 64-bit hashes.

## Branch topology

```
65f13faf  (merge-base: "bd-ky14a: clarify TDD intent in test strategy")
  │
  ├─ eb726334  feature/bd-ky14a-pampa-hash-fileids   ← PR #235 (1 commit)
  │
  └─ …235 commits…  origin/main (af21ccfc at time of writing)
```

`origin/main` has moved **235 commits** past the merge-base. A trial
`git merge origin/main` into the feature branch produces:

- **1 real source-code conflict:** `crates/quarto-core/src/stage/stages/engine_execution.rs`
- **61 snapshot conflicts:** `crates/pampa/snapshots/json/*.snap`
- Several files auto-merged but **need semantic review** (see §"Auto-merged but suspect").

`gh pr view 235` reports `mergeable: CONFLICTING`.

## The three things that make this complex

### 1. `engine_execution.rs` — architectural conflict (the real one)

This is the crux. When bd-ky14a was written, `EngineExecutionStage`
ran a **single** engine pass over a `doc_ast` and used hash-based
FileIds to register the engine's intermediate `<stem>.<engine>.rmarkdown`
file directly — no remap, because the intermediate's filename hashes to
its own `FileId` natively.

Since then, the provenance / multi-engine work (bd-5yff4, et al.) on
`main` **rewrote the stage into a multi-engine loop**:

```rust
// origin/main
let mut merged_context = ast_context;
…
for (run_index, (engine, engine_config)) in to_run.into_iter().enumerate() {
    …
    // Register this engine's intermediate as a NEW SEQUENTIAL slot:
    let new_slot = quarto_source_map::FileId(merged_context.filenames.len());
    …
    // Remap the executed AST's FileId(0) up into this engine's slot:
    quarto_ast_reconcile::remap_file_ids(&mut executed_ast,
        &|id| quarto_source_map::FileId(id.0 + new_slot.0));
    let (reconciled_ast, plan) = quarto_ast_reconcile::reconcile(ast, executed_ast);
    ast = reconciled_ast;
    …
}
```

That is **the same `FileId(0) + remap` workaround bd-ky14a deletes**, now
generalized to one sequential slot per engine and tracking a parallel
`merged_context.filenames: Vec<…>` whose `.len()` is the next FileId.

**The merge cannot keep both sides.** The hash-based design has to be
re-expressed inside the new multi-engine loop. The good news is that the
hash scheme makes the loop *simpler*, not harder:

- Each engine's intermediate name already includes the engine
  (`<stem>.<engine>.rmarkdown`), so `file_id_for_filename(intermediate_name)`
  is distinct per engine **natively** — no `new_slot` counter, no
  `remap_file_ids`, no `filenames.len()` bookkeeping.
- pampa on this branch reads the intermediate with `file_id: None`, which
  now derives `FileId(hash(intermediate_name))` automatically, so
  `executed_ast` already carries the right FileId before `reconcile`.
- `reconcile` already tolerates mixed-provenance FileIds (its keep/replace
  decision is by content), so dropping the remap is safe.

But this is a **re-implementation of a conflicted hunk against a changed
control-flow shape**, not a textual merge. It needs care and its own
tests (the existing bd-ky14a contract tests assume the single-engine
shape — see §"Tests").

**Open design question:** does anything else still depend on
`merged_context.filenames` as a dense, sequential, slot-indexed `Vec`
(e.g. a downstream consumer that indexes `filenames[file_id.0]`)? Under
hash FileIds, `file_id.0` is a 64-bit hash, not a dense index, so any
such consumer breaks. This needs a grep audit before re-implementing.

### 2. Semantic entanglement with #260 — `apply_node_edit.rs`

#260 added `crates/pampa/src/apply_node_edit.rs` (the targeted
inline-edit path used by q2-preview / hub-client). It contains:

```rust
// v1 assumption: single-file document → FileId(0).
let Some(path) = lookup_block(&a_u, &target_si, FileId(0)) else { … };
```

A literal `FileId(0)` as "the primary file" is exactly the invariant
bd-ky14a removes. **However**, the risk is smaller than it first looks:

- `lookup_block(ast, target, _file_id)` **ignores** the file-id argument
  (it's `_file_id`). It matches on full `block.source_info() == target`
  equality, where `target_si` comes from the frontend-serialized
  `SourceInfo` (the resolved value, including its own `d`/file_id). So the
  `FileId(0)` literal is effectively dead — it does not constrain the match.
- Correctness therefore depends on the **frontend and the AST agreeing on
  the FileId scheme**, not on the literal. After the merge both sides
  produce hash FileIds, so exact-match still holds.

Two things still need an explicit audit before declaring this safe:

- **`decode_compact_source_info`** reads `d` as
  `data.as_u64().unwrap_or(0) as usize`. `FileId(pub usize)` is **32-bit
  on `wasm32`** (the hub-client target) and 64-bit natively.
  `file_id_for_filename` likewise does `hasher.finish() as usize`, so the
  **same** truncation happens on both producer and consumer *within* wasm
  — consistent there. The danger is any path where a FileId computed
  **natively** (64-bit) is compared against one computed on **wasm**
  (32-bit truncation of the same hash). Confirm no such cross-target
  comparison exists (snapshots are native-only, so snapshot values will
  show the full 64-bit hash — expected).
- The dead `FileId(0)` argument and its "single-file document" comment
  should be cleaned up (or made real) so the next reader isn't misled.

> **The `apply_node_edit` literal is the *least* of it.** The serious
> version of this assumption lives on the **frontend**, in the q2-preview
> block-editing source index — see the next section. That is the one that
> silently breaks.

### 2b. Entanglement with #260 + #286 — the `d === 0` source-index assumption

This is the entanglement that motivates **waiting for #286** (see
§"Recommended sequencing"). It is broader than `apply_node_edit` and it
is not a merge conflict — it is a live, working assumption that bd-ky14a
silently violates.

**Root cause (already on `main`, from #260).** The q2-preview block
editor builds a source index keyed by a "siKey" string of the form
`"<t>:<r0>-<r1>:<d>"`, where `d` is the SourceInfo file_id. In
`ts-packages/preview-renderer/src/q2-preview/sourceIndex.ts`,
`buildSourceIndex` only indexes blocks whose pool entry has **`d === 0`**:

```ts
// sourceIndex.ts (origin/main, from #260)
const entry = pool[Number(s)];
if (entry?.t === 0 && entry.d === 0) {        // ← hardcoded "primary file is 0"
    const key = serializeSourceEntry(entry);  // "0:<r0>-<r1>:0"
    index.set(key, { sourceNode: block, … });
}
```

Under bd-ky14a, every `Original` block's `d` is `hash(filename) ≠ 0`, so
**no block satisfies `entry.d === 0` → the source index is empty →** every
hover/click/edit in q2-preview resolves to nothing. The targeted-edit
feature (#260) silently stops working. No test in pampa/quarto-core
catches this; it only shows up in a real q2-preview browser session.

**What #286 adds.** #286 (`feature/block-editing-improvements`, the
epic-closing PR) is **purely additive on the Rust side** (a new
`crates/pampa/src/regenerate_nested_buffers.rs` + a WASM binding + tests)
and does **not** touch the FileId machinery, `apply_node_edit`,
`node_lookup`, the JSON reader, or `engine_execution`. But it *extends the
same `d`-keyed siKey scheme* with more consumers that bake in `d = 0`:

- `regenerate_nested_buffers.rs` emits map keys as
  `format!("0:{start}-{end}:{file_id}")` with the comment
  *"d=file_id (always 0 for single-file docs)"*.
- Its Rust tests hardcode the expected key as `format!("0:{start}-{end}:0")`
  (`regenerate_nested_buffers_tests.rs:69,316`) — these **fail** the moment
  `file_id` becomes a hash.
- The frontend depth-cursor lookups pass a literal `d: 0`
  (`PreviewRoot.tsx:747`, `useBlockEditHover.tsx:88`:
  `serializeSourceEntry({ t: 0, r: [...], d: 0 })`), so even if the Rust
  map were re-keyed by hash, the frontend would still ask for `:0` and miss.

So #286 does not *create* the entanglement (it's #260's), but it **widens
the blast radius** and keeps the siKey surface in active flux.

**The clean fix (for the eventual bd-ky14a rebase).** Make the block
editor scheme-agnostic instead of assuming `0`:

- Drop the `&& entry.d === 0` filter in `buildSourceIndex` — it already has
  `entry.d` in hand; index every Original block under its real `d`.
- Replace the hardcoded `d: 0` at lookup sites with the real file_id the
  frontend already knows (the block's pool entry `d`).
- Re-key `regenerate_nested_buffers` by the real `file_id` (it already
  does — the bug is only in the *tests'* hardcoded `:0` and the frontend
  lookups), and update its tests.

This fix is small and well-contained, but it touches `sourceIndex.ts`,
`PreviewRoot.tsx`, `useBlockEditHover.tsx`, and the
`regenerate_nested_buffers` tests — **all files #286 is rewriting.** Doing
it before #286 lands guarantees a conflict and a re-audit. Hence the
sequencing recommendation.

### 3. The 61 snapshot conflicts are NOT mechanical

The PR description frames the snapshot churn as a pure `"d": 0 → "d": <hash>`
substitution. That **was** true against the merge-base, but `main` has
since changed the JSON writer's *format*, so the feature branch's
snapshots are now **doubly stale**:

Comparing the two conflict sides in e.g. `math-with-attr.snap`:

| Aspect | feature branch (HEAD) | origin/main |
|---|---|---|
| pool key | `"sourceInfoPool"` | `"p"` (renamed/compacted) |
| attribute key | `"attrS"` | `"a"` (renamed) |
| pool layout | one ordering | different dedup/ordering, different `id`/`s` indices |
| `source:` header | `crates/pampa/tests/test.rs` | `crates/pampa/tests/integration/test.rs` (bd-xvdop test move) |
| `d` value | `0` | `0` |

So you **cannot** take `main`'s snapshot and swap in the hash, and you
cannot take the feature branch's snapshot at all (wrong keys, wrong
layout, wrong source path). The only correct path is:

> Get the code merged and correct first, then **regenerate** all JSON
> snapshots with `cargo insta` and review the diff to confirm the only
> semantic change is `d: <small int> → d: <hash>` on top of `main`'s
> current format.

This also means the "62 snapshots reviewed" checkbox in the PR body is
**stale** and must be re-done after regeneration.

## Auto-merged but suspect (review before trusting)

These merged without conflict markers, but `main` changed them
substantially, so the three-way merge may be *clean-but-wrong*:

- **`crates/pampa/src/readers/json.rs`** — `main` rewrote this **+1041
  lines** (the provenance JSON reader/writer, incl. the `sourceInfoPool`→`p`
  and `attrS`→`a` renames). The feature branch's 24-line change touches
  how `d` (file_id) is read/written. Auto-merge may have silently dropped
  or mis-placed the feature change. **Read the merged result by hand.**
- **`crates/quarto-core/src/stage/stages/include_expansion.rs`** — `main`
  changed it (+7/−7) since base; the feature branch removes the
  remap/parallel-SourceContext workaround here. Confirm the removal still
  lands correctly on `main`'s version.
- **`crates/wasm-quarto-hub-client/src/lib.rs`** — feature branch +17;
  verify against `main`'s current shape (this is the WASM entry surface
  q2-preview/hub-client consume).
- **`crates/pampa/src/readers/qmd.rs`** — both sides changed (`main` +17,
  feature +11). Verify the `file_id` parameter plumbing survived.

### Files the feature branch changes that `main` did NOT touch (should apply cleanly)

`git diff --stat 65f13faf..origin/main` shows `main` did **not** modify
these, so the feature branch's edits should graft on without conflict —
but still build-check them:

- `crates/pampa/src/pandoc/ast_context.rs` (+127, the core of the change)
- `crates/quarto-source-map/src/context.rs` (+34, new
  `add_file_with_id` / `add_file_with_id_and_info`)
- `crates/quarto-core/src/transforms/attribution_render.rs` (+147/−…)
- `crates/quarto-lsp-core/src/document.rs`, `parse_document.rs`,
  `pipeline.rs`, `perf-harness`

## Tests

bd-ky14a shipped 5 contract tests (TDD anchors):

1. `pampa::pandoc::ast_context::tests::bd_ky14a_with_filename_uses_quarto_yaml_file_id`
2. `pampa::pandoc::ast_context::tests::bd_ky14a_source_context_indexed_by_hash_file_id`
3. `quarto_core::bd_ky14a_file_id_contract::bd_ky14a_pampa_qmd_read_uses_quarto_yaml_file_id`
4. `quarto_core::bd_ky14a_file_id_contract::bd_ky14a_sub_document_file_id_is_hash_based`
5. `quarto_core::bd_ky14a_file_id_contract::bd_ky14a_fresh_source_context_renders_pampa_source_info`

Note that test #3–#5 live in `crates/quarto-core/tests/bd_ky14a_file_id_contract.rs`
as a **top-level `tests/*.rs` file**, which now violates the
integration-test-layout rule (`.claude/rules/integration-tests.md`,
landed via bd-xvdop while this branch was open). On merge it should move
to `crates/quarto-core/tests/integration/bd_ky14a_file_id_contract.rs`
and be registered in `main.rs`.

**A new test is needed** for the multi-engine FileId behavior (§1): two
engines on one document must each get a distinct hash FileId with no
collision and no remap. The existing contract tests predate the
multi-engine loop and won't exercise it.

**A new test is wanted** for the §2 interaction: an `apply_node_edit`
round-trip on a document whose primary FileId is a hash (not 0), proving
the targeted-edit path still locates the destination block.

**A new e2e check is required** for §2b: a real q2-preview browser
session proving the block-editing source index is non-empty and a
targeted edit resolves when the primary file's `d` is a hash. This is the
only thing that catches the "empty source index" regression — no Rust or
vitest unit test does.

## Recommended sequencing

1. **Let #286 land first.** It is `MERGEABLE`, freshly rebased on `main`,
   and a draft. Its Rust side is purely additive; the bulk is q2-preview
   TS. Once it merges, the q2-preview siKey / source-index surface is
   stable and the full set of `d === 0` consumers is visible in one tree.
2. **Then rebase bd-ky14a onto the post-#286 `main`.** Do the `d`-scheme
   fix from §2b *once*, against settled code, instead of chasing a moving
   target.
3. What waiting does **not** buy you: the `engine_execution.rs` conflict
   (§1) is from #260/bd-5yff4 and already on `main`; it is unaffected by
   #286 either way. Plan to solve it regardless.

(If for some reason bd-ky14a must go first, the cost is that #286 will
have to absorb the `d`-scheme change mid-flight — re-keying its siKeys,
fixing its hardcoded `:0` tests, and re-running its browser e2e. That is
strictly more total work and lands on the #286 author. Not recommended.)

## Suggested merge strategy (for when the go-ahead comes)

1. Branch a working branch off `feature/bd-ky14a-pampa-hash-fileids`
   (keep the PR branch pristine until the merge is proven).
2. `git merge origin/main`. Expect the conflicts above.
3. Resolve `engine_execution.rs` by **re-implementing** the hash-based
   intermediate registration inside `main`'s multi-engine loop (drop
   `new_slot`/`remap_file_ids`/`filenames.len()` slot allocation). Do the
   `merged_context.filenames` consumer-audit grep first (§1 open question).
4. For the 61 snapshot conflicts: `git checkout --theirs` is **also
   wrong** (wrong `d`). Instead resolve them to *anything that compiles*,
   then **regenerate** with `cargo insta accept` after the code is correct,
   and review the diff (§3).
5. Hand-review the four "auto-merged but suspect" files (§"Auto-merged").
6. Address the `apply_node_edit` audit (§2) and clean up the dead
   `FileId(0)` arg.
7. Move the contract test file under `tests/integration/` (§Tests).
8. Add the two new tests (multi-engine FileId; apply_node_edit hash
   round-trip).
9. Full gate: `cargo build --workspace` → `cargo nextest run --workspace`
   → `cargo xtask verify` (full, **not** `--skip-hub-build` — this touches
   `quarto-core`/`pampa`/`quarto-source-map` which the WASM leg depends on).
10. End-to-end: `cargo run --bin q2 -- render <fixture>.qmd` and inspect
    the JSON pool; exercise a q2-preview targeted edit against the hash
    FileId (the §2 path) in a real browser session.

## Decisions needed from cscheid before starting

1. **Merge vs. rebase.** A single-commit branch could be `git rebase
   origin/main`'d instead of merged, giving a linear history and one
   clean commit to review. The conflict content is identical either way;
   the question is the history shape you want on the PR.
2. **Scope of the `apply_node_edit` cleanup** (§2): minimal audit + comment
   fix, or fold the "primary file id" into a real helper now?
3. **Whether to also tackle the lingering `FileId(0)` fallbacks** in
   `pampa/src/pandoc/location.rs`, `treesitter_utils/*`,
   `comrak-to-pandoc` (these are internal "no known file" sentinels, not
   primary-file assumptions — probably out of scope, but worth a yes/no).

## Appendix: reproduction

```bash
git fetch origin
git checkout -B trial origin/feature/bd-ky14a-pampa-hash-fileids
git merge --no-commit --no-ff origin/main      # observe conflicts
git diff --name-only --diff-filter=U            # 1 code + 61 snapshots
git merge --abort
```

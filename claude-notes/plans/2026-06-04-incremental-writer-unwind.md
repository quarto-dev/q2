# Incremental Writer Unwind

**Date:** 2026-06-04
**Branch:** feature/provenance
**Status:** Implementation-ready.

## Overview

Plan 7 added a write-back path to `feature/provenance`: a soft-drop reconciliation writer
(`CoarsenedEntry` State B), user-edit stamping (`stampUserEdits`), baseline-AST tracking
in the WASM bridge, and `setAst` wiring in the SPA. These are being withdrawn in favour
of a simpler future model (see `target-incremental-writes.md`) where user edits are
expressed as raw QMD content applied to the pre-pipeline AST rather than as provenance-
annotated modifications to the transformed AST.

This plan reverts all of that write-back machinery while preserving every improvement
unrelated to write-back: Plans 4–6 provenance stamping, 7f source-info API improvements
(strict reader, `By::` constructors, `SourceInfo::default()` deprecation, wire-format
renames), and 7g source-range tiling fixes.

## What we are NOT reverting

The following stay regardless:

- `preimage_in` on `SourceInfo` — essential bridge mechanism for the new write-back model; also required by the kept `b43fadef` crash fix (Phase 1)
- `is_atomic_kind` / `ATOMIC_CUSTOM_NODES` — useful concept, no harm to keep
- Wire-format renames (`attrS → a`, `sourceInfoPool → p`)
- Strict `json::read` / `read_completing_source_info` split
- All `By::` constructors and `SourceInfo` type improvements
- 7g source-range tiling fixes and tiling auditor
- Plans 4–6 provenance stamping in transforms
- Filter idempotence tests (Plan 3)
- Performance improvements (Pass-2 parallelisation, regex recompile fix)
- `b661b4e0` ariadne graceful degradation in WASM (unrelated robustness fix)
- The `pipelineKind` module move (`hub-client/src/utils/pipelineKind.{ts,test.ts}` → `ts-packages/preview-runtime/src/`, a 100% rename). This is an unrelated refactor, not write-back work. **Consequence for Phase 6:** `hub-client/src/utils/pipelineKind.ts` no longer exists, so `ReactPreview.tsx` must keep importing `pipelineKindForFormat` from `@quarto/preview-runtime`, not the old `'../../utils/pipelineKind'` path.

---

## Phase 1 — Rust: incremental writer core → State A

The target is State A: `CoarsenedEntry` with three variants (`Verbatim`, `Rewrite`,
`InlineSplice`), flat top-level `coarsen`, no soft-drop, no source-info-aware dispatch,
no baseline-AST tracking.

**One deliberate exception — keep the `b43fadef` crash fix.** Reverting
`incremental.rs` to a literal `git checkout main` is **not** safe: main's
`assemble_inline_splice` (incremental.rs:602) derives prefix/suffix boundaries
from `inline_source_span` → `start_offset()`, and `Concat::start_offset()`
returns the sentinel `0` (source_info.rs:189). A Concat-led first inline (e.g.
`Str "Table:"` parsed as `Concat[Original "Table" ++ Original ":"]`, common in
real input — table captions, links/images, anchor shorthands, math-with-attr)
then slices `original_qmd[block.start .. 0]`, a reversed range that panics. This
is a **pre-existing latent bug on main**, not a Plan-7 artifact; it was only
exercised once the provenance branch added a corpus panic-sweep. The unwind
keeps the WASM bridge's `incremental_write_qmd` (Phase 2, State A) which reaches
this path, so the crash would be live post-unwind. `b43fadef` fixed it by
deriving boundaries from `preimage_in(target_file_id)` with an `Option`/`Rewrite`
fallback — and that fix is corpus-tested. We keep it whole, including the
minimal `target_file_id` threading it depends on.

- [x] Revert `crates/pampa/src/writers/incremental.rs` toward its pre-Plan-7 shape, **except** the `b43fadef` crash fix (see exception above):
  - Remove `Transparent` and `Omit` variants from `CoarsenedEntry`
  - Revert `Verbatim.orig_idx` from `Option<usize>` back to `usize`
  - Revert `Rewrite` from `block_text: String` back to `new_idx: usize`
  - Remove soft-drop cascade (`coarsen_keep_before_block` Plan-7 arms)
  - Remove `is_editable_inside_inline` function and all callers within this file
  - Remove the `is_atomic_custom_node` import and the soft-drop/atomicity-dispatch uses of `target_file_id` (the per-arm source-info dispatch). **Keep** the `preimage_in` import and the `target_file_id` threading through `coarsen` → `coarsen_blocks` → `assemble_inline_splice` that `b43fadef` requires — this is the minimal Plan-7 plumbing the crash fix depends on, not the soft-drop machinery.
  - Remove multi-inline dedupe optimisation added in Plan 7 Phase 2+3a
  - Remove transparent-wrapper recursion (`coarsen_keep_before_block` wrapper descent)
  - Remove YAML-frontmatter wrapper preservation from `fcbb55dc`
  - Remove `descend wrappers when deriving target_file_id` from `d60119d9` (the wrapper-descent heuristic; the plain `target_file_id` derivation that feeds `assemble_inline_splice` stays)
  - Remove `recurse into non-atomic Generated wrappers` from `49748648`
  - Remove self-containment refactor from `6e3134ec` (was tied to baseline-AST tracking)
  - **Keep** the `b43fadef` `assemble_inline_splice` shape: `preimage_in`-based boundary computation, the `Option<String>` return, and the `None → CoarsenedEntry::Rewrite { write_block_to_string(new_block) }` caller fallback. After all the above removals, re-verify `assemble_inline_splice` still compiles against the State-A `CoarsenedEntry` (the `Rewrite` fallback now constructs the reverted `new_idx`-or-`block_text` shape — reconcile with whichever `Rewrite` variant the revert lands on).
- [x] Surgically revert `crates/pampa/tests/integration/incremental_writer_tests.rs` (do **not** `git checkout main` — that would delete the b43fadef regression tests). Remove the Plan-7-added test cases (soft-drop, Transparent/Omit, baseline-AST, multi-inline dedupe), but **keep** `inline_splice_concat_led_paragraph_does_not_panic` and `incremental_write_never_panics_on_pampa_corpus` (added by `b43fadef`, not present on main — they guard the kept crash fix). Verify the State-A test cases that existed before Plan 7 still pass.
- [x] Revert `crates/pampa/tests/integration/inline_splice_integration_tests.rs` to main — `git checkout main -- ...` (all three files exist on main and were modified, not created; revert, do not delete)
- [x] Revert `crates/pampa/tests/integration/inline_splice_property_tests.rs` to main
- [x] Revert `crates/pampa/tests/integration/inline_splice_safety_tests.rs` to main
- [x] `crates/pampa/tests/integration/incremental_writer_investigation.rs` — untouched by Plan 7; leave as-is
- [x] `crates/pampa/tests/integration/main.rs` — module registrations are identical to main (inline_splice modules were already registered); no changes needed
- [x] Delete `crates/quarto/tests/smoke-all/q2-preview/render-components-write/` directory (new on this branch; does not exist on main)
- [x] `cargo nextest run -p pampa` — all pampa tests pass (3877/3877)

## Phase 2 — Rust: WASM bridge → pre-Plan-7 signature

The key change in `cc582b80` was replacing an *internal re-parse* of `original_qmd`
with a caller-supplied `baseline_ast_json`. The commit message called this "lifting the
read-only guard" but on the Rust side there was no explicit guard — the State A function
just re-parsed the original QMD itself via `qmd_to_pandoc`. The revert restores that.

- [x] Revert `crates/wasm-quarto-hub-client/src/lib.rs` `incremental_write_qmd`:
  - Restore the 2-argument signature: `incremental_write_qmd(original_qmd: &str, new_ast_json: &str) -> String`
  - Replace the `baseline_ast_json` deserialization block with `qmd_to_pandoc(original_qmd.as_bytes())` to re-derive the original AST internally (State A approach)
  - Remove the soft-drop diagnostic collection and structured return (State A returns plain `String`)
  - Use `git show main:crates/wasm-quarto-hub-client/src/lib.rs` as a reference for the pre-Plan-7 shape of this function
- [x] `cargo build -p wasm-quarto-hub-client` — WASM Rust crate compiles (full wasm build runs in Phase 9). Note: native `cargo check` produces pre-existing `temporal_rs` errors (WASM-only crate); the Rust signature change compiles cleanly.

## Phase 3 — Rust: surgical json.rs cleanup

- [x] Remove `pub const USER_EDIT_SOURCE_INFO_ID: usize = 0` from `crates/pampa/src/writers/json.rs`
- [x] Remove pool pre-population (reserved slot 0) from `SourceInfoSerializer::new()`; pool now starts empty
- [x] Adjust the writer's intern-side pool-count assertions (tests that used `+ 1` offsets to account for the reserved slot) back to plain `N`; update "Slot 0 reserved" comments
- [x] Remove `test_reserved_slot_user_edit` and `test_user_edit_slot_id_matches_typescript_mirror` tests from `json.rs` (these were the pool[0] reserved-slot tests from `3c3492ac` — they lived in json.rs, not json_reader_smoke_tests.rs). `json_reader_smoke_tests.rs` had no reserved-slot tests to remove.
- [x] `cargo nextest run -p pampa` — 3873/3873 pass (snapshot updated: pool IDs shift down by 1, user-edit reserved entry removed)

## Phase 4 — TypeScript: framework write-back

- [x] **`ts-packages/preview-renderer/src/framework/dispatch.tsx`** (surgical):
  - Remove `import { USER_EDIT_SOURCE_INFO_ID }` at the top
  - **TRAP — do NOT revert the `ATOMIC_KINDS` import.** The diff shows `ATOMIC_SYNTHETIC_KINDS` renamed to `ATOMIC_KINDS` in both dispatch.tsx and sourceInfo.ts as part of Plans 4-6. The old name no longer exists in sourceInfo.ts; reverting this import would break the build. Keep `import { isAtomicSourceInfo, ATOMIC_KINDS } from '../utils/sourceInfo'` unchanged.
  - Remove the `stampUserEdits` function entirely (~30 lines; currently `dispatch.tsx:393-433`)
  - Remove the `stampedSetLocalAst` wrapper (currently `dispatch.tsx:484-485` plus its explanatory comment at `:480-483`) and restore the `effectiveSetLocalAst` ternary's non-atomic branch to plain `setLocalAst`. On main this line reads `const effectiveSetLocalAst = isAtomic ? NOOP_SET_LOCAL_AST : setLocalAst;` (main:411). **`NOOP_SET_LOCAL_AST` and `effectiveSetLocalAst` are pre-existing on main — keep them; only the `stampedSetLocalAst` indirection is removed.**
  - Revert each `setLocalAst` callback to its pre-7f-Phase-2 form by removing the `s:` preservation spreads. The actual pattern is a full node spread — `{ ...(node as EmphInline), c: newChildren }` — that implicitly preserved `s:` by carrying all fields through. Revert each back to the explicit `{ t: 'Emph', c: newChildren }` form (13+ sites: Emph, Strong, Link, Image, Span, Quoted, Para, Plain, Header, BlockQuote, Div, BulletList, OrderedList, Figure). The callbacks themselves stay; only the spread form changes back to explicit-t form.
  - Also revert `makeFlatInlineRenderer`: rename the parameter from `_tag` back to `tag`, and revert its inner `setLocalAst({ ...node, c: next })` back to `setLocalAst({ t: tag, c: next })`.
- [x] Delete `ts-packages/preview-renderer/src/framework/stampUserEdits.test.ts`
- [x] Delete `ts-packages/preview-renderer/src/framework/dispatch.test.tsx` (created entirely in Plan 7f Phase 2 for s: preservation tests)
- [x] **`ts-packages/preview-renderer/src/framework/Ast.tsx`** — no changes needed. The only diff on this branch is the `sourceInfoPool → p` wire-format rename, which is in the "keep" list. `stampedSetLocalAst` is not in this file; it lives in `dispatch.tsx` (addressed above). The pre-existing `setLocalAst`/`setAst` infrastructure stays intact — target-incremental-writes says it "still exists" after the unwind.
- [x] **`ts-packages/preview-renderer/src/types/sourceInfo.ts`** (surgical):
  - Remove `export const USER_EDIT_SOURCE_INFO_ID = 0`
- [x] **`ts-packages/preview-renderer/src/utils/sourceInfo.test.ts`** (surgical):
  - Remove the `"USER_EDIT_SOURCE_INFO_ID atomic-gate sanity"` describe block (three tests added in `fcd29383`)
  - Keep all other sourceInfo tests
- [x] Run `npm run test --workspace=ts-packages/preview-renderer` — 176/176 tests pass

## Phase 5 — TypeScript: write-back API types

- [x] **`ts-packages/quarto-sync-client/src/types.ts`** (surgical):
  - Revert `incrementalWriteQmd` callback signature from the Plan-7 three-argument structured-return form back to:
    `incrementalWriteQmd?: (originalQmd: string, newAst: unknown) => string`
  - Remove documentation about baseline AST and `warnings`
- [x] **`ts-packages/quarto-sync-client/src/client.ts`**: revert the corresponding caller changes to match the restored signature
- [x] **`ts-packages/preview-runtime/src/wasmRenderer.ts`** (surgical):
  - Revert `incrementalWriteQmd` from the three-argument form back to `(originalQmd, newAst) → string`
  - Remove the `IncrementalWriteQmdResult` interface
  - Remove `baselineAst` parameter handling and the soft-drop diagnostic parsing in the function body
  - Revert the error-handling/distinction logic to the pre-Plan-7 form
- [x] **`ts-packages/preview-runtime/src/wasm-quarto-hub-client.d.ts`**: reverted to 2-arg form; also removed `warnings?: AstDiagnostic[]` from `AstResponse`
- [x] **`hub-client/src/types/wasm-quarto-hub-client.d.ts`**: same; also removed `warnings` from `AstResponse`
- [x] **`q2-demos/hub-react-todo/src/types/wasm-quarto-hub-client.d.ts`**: had 3-arg signature; reverted to 2-arg
- [x] **`q2-demos/kanban/src/types/wasm-quarto-hub-client.d.ts`**: same

## Phase 6 — Hub-client: write-back UI and tests

- [x] **`hub-client/src/components/render/ReactPreview.tsx`** (surgical): revert the Plan-7 write-back additions. There is **no `AstDiagnostic` import** — ignore that earlier description. The actual additions to remove:
  - the two refs `pendingWriteWarningsRef` and `lastRenderDiagnosticsRef`
  - the soft-drop merge logic in `doRenderWithStateManagement` (the `pendingWriteWarnings` drain + `mergedDiagnostics` construction); restore the plain `onDiagnosticsChange(result.diagnostics)` and `diagnostics: result.diagnostics` shape
  - the `handleSetAst` body: revert to State A's read-only-guard form — `if (pipelineKindForFormat(format) === 'preview') { console.warn(...); return; }` then `const newQmd = incrementalWriteQmd(content, newAst)` (2-arg), and restore the `[content, onContentRewrite, format]` dependency array
  - **TRAP — do NOT revert the `pipelineKindForFormat` import line.** Keep `import { pipelineKindForFormat } from '@quarto/preview-runtime'`. The old `'../../utils/pipelineKind'` path was a 100% rename to preview-runtime (see "What we are NOT reverting") and no longer exists. State A's reverted `handleSetAst` still *calls* `pipelineKindForFormat`, so it must stay importable — from the new path.
- [x] **`hub-client/src/components/render/q2-debug/components.tsx`** (surgical): reverted Figure renderer from `{ ...args.node, c: [...] }` back to `{ t: 'Figure', c: [...] }`
- [x] Delete `hub-client/src/services/incrementalWrite.wasm.test.ts`
- [x] Delete `hub-client/e2e/q2-preview-render-components-write.spec.ts`
- [x] **`hub-client/src/test-hooks.ts`**: reverted to main
- [x] **`hub-client/e2e/helpers/projectFactory.ts`**: reverted to main
- [x] **`hub-client/e2e/helpers/testHooks.ts`**: reverted to main

## Phase 7 — SPA: write-back wiring

- [x] **`q2-preview-spa/src/PreviewApp.tsx`** — all changes are pure write-back with no kept refactors entangled, so `git checkout main -- q2-preview-spa/src/PreviewApp.tsx` is the safest approach and handles everything automatically (including the items below). If surgical editing is preferred instead, the following must all be covered:
  - Remove `handleSetAst`, `handleDismissWarnings`, `fnv1aHex`, `writeWarnings` state, `lastEmittedRef`, `activeFileRef`, `astJsonRef`, and their `useEffect` wiring
  - Remove echo-prevention logic in the `onFileContent` handler
  - **Restore `noopSetAst`** (Plan-7 deleted it; without it the `setAst={noopSetAst}` JSX site is broken): `const noopSetAst = () => { /* deliberately empty */ };`
  - Revert `setAst={handleSetAst}` back to `setAst={noopSetAst}` in the JSX
  - Remove `DiagnosticStrip` import and its JSX usage
  - Remove `useRef` from the React import (no longer needed)
  - Remove `getFileContent`, `updateFileContent`, `incrementalWriteQmd` from the sync-client import
- [x] Delete `q2-preview-spa/src/components/DiagnosticStrip.tsx`
- [x] `git checkout main -- q2-preview-spa/src/PreviewApp.integration.test.tsx` — the only diff vs main is a 3-line comment rewording (no test logic added or removed; the `expect(typeof props.setAst).toBe('function')` assertion is unchanged), so a clean checkout-main restores it

## Phase 8 — Demos: revert write-back usage

All four diffs vs main were verified to be **100% write-back-related** (only the
`incrementalWriteQmd` 2-arg → 3-arg signature change + the `{ qmd, warnings }`
structured return); no kept change is entangled. Wholesale `git checkout main` is
therefore safe and sufficient — no surgical judgment needed.

- [x] `git checkout main -- q2-demos/hub-react-todo/src/useSyncedAst.ts q2-demos/hub-react-todo/src/wasm.ts q2-demos/kanban/src/useSyncedAst.ts q2-demos/kanban/src/wasm.ts`

## Phase 9 — Verification

Run in this order: fast Rust feedback first, then the full stack.

- [x] `cargo nextest run --workspace` — 9708/9708 pass
- [x] `cargo xtask verify --skip-hub-build` — clean (lint + Rust build + tests with `-D warnings`)
- [x] `cd hub-client && npm run build:all` — succeeds (caught missed WasmModuleExtended 3-arg in wasmRenderer.ts; fixed in separate commit eb7ffe1f)
- [x] `cd hub-client && npm run test:ci` — 79/79 pass
- [x] `cargo xtask verify` — all verification steps passed

## Notes

- The `hub-client/changelog.md` entries for Plan 7 write-back work (q2-debug s: fix, WASM bridge baseline-AST) can be left in place as historical record.
- `preimage_in` keeps exactly one production caller after this revert: `assemble_inline_splice`, via the kept `b43fadef` crash fix (see Phase 1 exception). Beyond that single use it is otherwise dormant, preserved for the new write-back model described in `target-incremental-writes.md`.
- Plan 7a (user-filter idempotence), 7b (test consolidation), and 7c (orthogonal closure-gap phases) are unaffected by this revert and remain open.
- After Phase 9 passes, update the local `CURRENT.md` symlink: `ln -sf 2026-06-04-target-incremental-writes.md claude-notes/plans/CURRENT.md`. (`CURRENT.md` is gitignored, so this is a local session action only.)

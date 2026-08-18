# marimo + julia in `q2 preview`: two bugs (Bug A marimo hydration, Bug B julia-cell drop)

_Investigation 2026-07-23. Worktree `marimo-julia-preview` off `feature/ts-engine-extensions@869f2ac59` (includes the a30015c05 head-injection fix + 20 later commits). Fixtures: `~/src/mj-preview-debug/{mj-preview,mj-preview2}` (a marimo slider+numpy cell, a marimo md cell, one julia Plots cell; identical except engine order)._

## Symptoms (confirmed: user in browser + Playwright + screenshots)

| | marimo widget | julia code | julia plot |
|---|---|---|---|
| CLI `q2 render` (both orders; output byte-structurally identical) | ❌ empty | ✅ highlighted | ✅ shown |
| preview 7902 — order `[julia, marimo]` (julia **first**) | ❌ empty | ⚠️ raw, unhighlighted | ❌ dropped |
| preview 7903 — order `[marimo, julia]` (julia **last**) | ❌ empty | ✅ highlighted | ✅ shown |

Both engines execute + record captures server-side in both orders (`engines=julia-engine, marimo` / `engines=marimo, julia-engine`, no errors). These are display/splice/hydration bugs, not execution bugs.

## Bug A — marimo never displays (NOT a q2 bug; handoff premise was wrong)

The island markup, `__MARIMO_EXPORT_CONTEXT__`, and runtime scripts (`@marimo-team/islands@0.23.14`) are all present and correct in the preview pane. Hydration **crashes**:

```
[pageerror] import_humanize_duration.default.humanizer is not a function
```

**The CLI file render throws the identical error and also shows no marimo widget** (screenshot: empty "Interactive marimo (Python)" section). So the earlier handoff claim — "in a plain CLI render the marimo island + slider ARE present and correct" — is **false**: the slider never mounts in *either* path. Root cause is inside the `@marimo-team/islands@0.23.14` CDN bundle (its `humanize-duration` import), order- and path-independent. **This lives in the marimo extension's choice of islands bundle/version — out of q2 scope.** (A secondary preview-only `define is not defined` exists, but Bug A reproduces in the file render without it, so it is not the cause.) The a30015c05 head-injection fix works correctly.

## Bug B — julia plot only when julia runs last (real q2 preview bug; FIXED)

### Evidence
The raw captures are correct: for order `[julia, marimo]`, marimo's `result.markdown` (the last-folded capture) contains **both** julia's executed `.cell` + base64 png **and** the marimo islands. So the bug is purely in the multi-engine **fold/splice**, not recording.

### Root cause
`derive_cell_outputs_walk` (in `crates/quarto-core/src/engine/capture_splice.rs`) stalls on a **foreign engine's un-executed cell**. In order `[julia, marimo]`, julia runs first, so julia's capture `B1` still holds the raw `{python .marimo}` cells (marimo hasn't run). The walk treats every `{...}` code cell as an engine cell, looks for an output block in `B1`, finds none (still raw code), and **does not advance the `B1` pointer `j`**. At the next prose block (`## Julia` header) `structural_eq(a[header], b[stalled-marimo-cell])` fails and the walk **breaks entirely — before reaching julia's own `::: {.cell}` further down**. Julia's output is never mapped → its `{julia}` cell falls through to raw, unhighlighted source, no plot.

In order `[marimo, julia]` julia runs last; the marimo cells are already islands (`RawBlock`s that lock-step match as prose), so the walk sails past them, reaches julia's `.cell`, and maps it. Hence the order-dependence.

The permissive `cell_belongs_to_engine()` (returns `true` for any language) means the derive walk never restricts to the current engine's cells; a foreign un-executed cell derails it. The existing `two_engine_fold_splices_both_engines_cells` test passed only because its foreign cell is *last* (walk ends before any divergence).

### Fix
In the engine-cell arm's no-output branch of `derive_cell_outputs_walk`: when `B1[j]` is not an output block but is **structurally equal** to the A1 cell, advance `j` too (it's a passthrough — a foreign engine's un-executed cell, or a no-output cell). This keeps the walk aligned so this engine's own later cells are still reached and mapped. Only advances on a structural match; genuine capture drift still falls through to the conservative divergence handling. Generic across N engines; no lang→engine table needed. (Same spirit as the `bd-5m1ni9if` TODO already noted in the file.)

### Test
`julia_first_fold_preserves_julia_cell_after_foreign_marimo_cells` (pure-AST reproduction: marimo cells before julia cell + intervening prose, engine order `[julia, marimo]`). Fails before the fix (julia cell dropped), passes after. Paired with `two_engine_fold_splices_both_engines_cells` (foreign cell last) it pins the bug precisely.

## Verification method
- `q2` binary: fresh full preview chain from this worktree (`build:wasm` → `build-q2-preview-spa` → `cargo build --bin q2`).
- Browser: Playwright headless chromium (no extension needed) driving the preview iframe — DOM/console/network + screenshots. Harness in scratchpad (`inspect.cjs`, `dump.cjs`), run with `NODE_PATH=<worktree>/node_modules`.
- CLI ground truth: `q2 render mj.qmd`, opened `file://…/mj.html` in the same harness.

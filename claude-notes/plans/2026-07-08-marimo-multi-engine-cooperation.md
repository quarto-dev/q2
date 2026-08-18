# marimo file-claim vs Q2 multi-engine cooperation

## Overview

**Problem.** A `{python .marimo}` + `{r}` document renders the `{r}` cell as raw
code. Root cause: marimo's `claimsFile` returns true for any `.qmd`/`.md`
containing a marimo fence (`marimo-engine.ts:146`). That drives q2's
`EngineClaimsFileStage` → `ctx.claimed_engine_name = Some("marimo")` →
`resolve_engines`' CLAIMED SHORT-CIRCUIT (`resolution.rs:366`) → sequence
`[marimo]`, empty ownership, `engine_count=1`. The `{r}` cell is left to no
engine.

**Fix (subtractive, marimo-side only).** marimo already ships the correct
multi-engine claim: `_extension.yml` declares `python { whenClass: marimo }` →
`Primary(2)`, which the q2 tier resolver composes with knitr/jupyter. The
whole-file claim *defeats* marimo's own language claim. So: **make marimo's
`claimsFile` return false for `.qmd`/`.md`** and let `claimsLanguage` do the
selection. No q2-core change.

**Why marimo is the outlier** (engine survey): only `TsEngine` implements
`claims_file`; native knitr/jupyter/markdown don't. Other TS file-claimers
(julia, echo) claim whole-file *scripts* by extension (`.jl`, `.echo`), never
`.qmd`/`.md`. Design doc §8: "Multi-engine remains a `.qmd`-authoring feature;
converted non-`.qmd` files are single-engine."

**Empirical gate — PASSED** (committed `17a2f61f2`): with `claimed=None`,
`marimo + r` → `[marimo, knitr]`, `ownership{python→marimo, r→knitr}`,
`handled_languages_for("marimo") ∋ "r"`. Marimo-only and `engine: marimo` both
still select marimo. This confirmed scope = marimo-side only.

## Provenance of the `claimsFile` file-claim (git archaeology, 2026-07-08)

The whole-file `claimsFile` behavior this plan reverts is **not** original to
marimo's Q2 engine-API port. History (quarto-marimo repo):

- `2bd089c` (Gordon Woodhull, 2025-10-22, "convert from quarto filter to
  quarto external engine") — the Lua→TS engine-API port. It **introduced the
  `claimsFile` property as a deliberate no-op**: `claimsFile: (_file, _ext) =>
  { return false; } // Don't claim files automatically`. Selection was meant to
  run entirely through `claimsLanguage`. (Pre-port the extension was a pure
  Pandoc/Lua filter, `_extensions/marimo/marimo-execute.lua` — no file-claim of
  any kind existed.)
- `ab098d8` (Péter Gyarmati, 2026-04-14, "fix: engine routing logic and repo
  consistency (#62)") — **replaced `return false` with the content-sniff**
  (`containsMarimoFence`) that claims the whole file. This is the behavior that
  breaks Q2 multi-engine documents.
- `a0dcf30` (2026-07-08, this work) — **restores the original `return false`**
  and removes `containsMarimoFence`.

So the fix is a *restoration* of the port's original intent, not a redesign.

## Frozen Test Seam Spec

| # | Tier | Real unit mounted | Seam (mount · events · assertion surface) | Mock boundary | Named revert hunk |
|---|------|-------------------|-------------------------------------------|---------------|-------------------|
| 1 | Rust unit (`resolution.rs`) | `resolve_engines` (real resolver) | build registry [marimo,knitr,jupyter]; `resolve_engines(meta, ast, reg, None)`; assert seq⊇{marimo,knitr}, ownership{python→marimo,r→knitr}, `handled_languages_for("marimo")∋"r"` | `MockEngine` claim closures = env dep (registry); resolver is the unit | Remove `mock_marimo`'s `("python",Some("marimo"))→Primary(2)` arm → `ownership["python"]!="marimo"` RED (proven) |
| 2 | Rust unit (`resolution.rs`) | `resolve_engines` | same doc, `claimed=Some("marimo")`; assert seq==[marimo], ownership empty, r∉handled | as above | (characterization — documents the bug; no revert) |
| 3 | deno unit (`quarto-marimo/tests/`) | marimo engine's `claimsFile` | write temp `.qmd` with a marimo fence; call `engine.claimsFile(path, ".qmd")`; assert `=== false` | `Deno.readTextFileSync` real (temp file); function is the unit | Restore `claimsFile` body to `containsMarimoFence(...)` → assertion RED |
| 4 | deno unit (`quarto-marimo/tests/`) | sew-back cell loop in `execute` (guard, unchanged) | drive the cell loop over `[{r}, {python .marimo}]`; assert `{r}` sourceVerbatim survives unmodified | subprocess/`extract.py` mocked | Change the `else { push(cell.sourceVerbatim) }` branch to drop non-marimo cells → RED |
| 5 | q2 e2e (`q2-preview-spa/e2e/engine-capture-splice-marimo.spec.ts`) | full `q2 preview` render in browser | mixed marimo+knitr fixture; assert BOTH the marimo island AND the `{r}` output appear in the pane | none (real render; needs R + uv/marimo/deno + rebuilt fixture bundle) | Restore `claimsFile` in the fixture's bundled `marimo-engine.js` → `{r}` renders raw, output assertion RED |

## BRANCH DECISION (Gordon, 2026-07-08)

The q2 marimo fixture (`crates/quarto-core/tests/fixtures/extensions/marimo/`)
is **byte-identical to quarto-marimo's `q2-bare-sql-interop` branch** (all four
`src/marimo-engine.ts` + `lib/*.ts` files diff-clean), NOT `main`. That branch
is ahead of `main` with the handledLanguages-aware `cellOwnedByMarimo` sew-back
and the bare-sql interop logic (plan4c SC9/SC13/SC14/SC16; fixture provenance
recorded at `claude-notes/plans/2026-07-02-plan4c-marimo-validation.md:809`,
"SC19 fixture rebundle" from `q2-bare-sql-interop 2a2f312`).

**Gordon's call: fix lands on `main` only; Gordon merges it into
`q2-bare-sql-interop` and re-syncs the fixture himself.** I stop before
touching the fixture bundle. Consequences:

- quarto-marimo `main`: `claimsFile→false` + deno test — DONE, committed
  `a0dcf30` (NOT pushed).
- q2 fixture bundle + committed-fixture render/preview e2e: **deferred to
  Gordon** (needs the re-synced fixture; can't be verified against the current
  committed fixture, which still has the content-sniff `claimsFile`).

q2 work branch: `braid/marimo-multi-engine-cooperation` (off
`feature/ts-engine-extensions`). No braid strand was created for this work.

## Checklist

- [x] Empirical gate: resolution-tier tests (seams #1, #2) — q2 commit `17a2f61f2`
- [x] Seam #3: deno `claimsFile` test (RED-first proven) — marimo `a0dcf30`
- [x] Seam #4: sew-back — no change needed (isMarimoCell/cellOwnedByMarimo
      passthrough already leaves `{r}` verbatim; coincides exactly with the
      handledLanguages leave-alone set — proven by case analysis). Covered by
      proxy: is-marimo-cell.test.ts + the post-sync render test below.
- [x] Implement: `claimsFile → false` for `.qmd`/`.md` in marimo `main` `src/marimo-engine.ts`
- [x] marimo deno suite green (67/67); q2 `engine::resolution` green (36/36)
- [~] Seam #5 (mixed fixture + e2e): **deferred to Gordon** — ready-to-add
      render test below; add after re-syncing the fixture from
      `q2-bare-sql-interop`.
- [~] End-to-end real `q2 render`/`q2 preview`: **deferred to Gordon** (fixture
      re-sync gates it). Native resolution + deno unit tiers verified.
- [ ] Gordon: merge `main`→`q2-bare-sql-interop`, rebuild+re-sync fixture, add
      the render test below, run `QUARTO_SC21_LIVE=1` marimo preview e2e.

## For Gordon — merge + re-sync recipe

```bash
# 1. Bring the one-line fix into the fixture's source branch
cd ~/src/quarto-marimo
git switch q2-bare-sql-interop
git cherry-pick a0dcf30          # or hand-apply: claimsFile → return false, drop containsMarimoFence
deno test --no-check --allow-read --allow-write --allow-env   # expect green

# 2. Rebundle (per plan4c SC19 rebundle / compat doc §5 symlink workaround)
quarto call build-ts-extension   # leaves the real bundle at _extensions/marimo/marimo-engine.js

# 3. Sync into the q2 fixture (src + bundle; lib/command.py/extract.py unchanged)
cp src/marimo-engine.ts       <q2>/crates/quarto-core/tests/fixtures/extensions/marimo/src/marimo-engine.ts
cp _extensions/marimo/marimo-engine.js <q2>/crates/quarto-core/tests/fixtures/extensions/marimo/_extensions/marimo/marimo-engine.js
git checkout -- _extensions/marimo/marimo-engine.js   # restore the download-shim in the marimo repo
```

## Post-sync render test (seam #5-native) — add after re-sync

Add to `crates/quarto-core/tests/integration/marimo_engine_e2e.rs`. It renders
the mixed doc through the **committed** fixture (`setup_marimo_project`, NOT the
claims-less `setup_marimo_project_dynamic` workaround SC16 uses) — which only
works once the fixture's `claimsFile` is fixed. Named revert: restore
`claimsFile`'s content-sniff in the fixture bundle → the `{r}` cell renders raw
(`class="{r}"`) → RED (the 2026-07-03 SC16 annotation documents this exact raw
render firsthand).

```rust
/// marimo file-claim: the COMMITTED fixture (claimsFile fixed) renders a mixed
/// {python .marimo} + {r} doc with BOTH engines executing — no claims-less
/// fixture derivation needed (contrast sc16_e2e_*, which uses
/// setup_marimo_project_dynamic to sidestep the pre-fix claimsFile behavior).
#[test]
fn marimo_committed_fixture_mixed_coexistence() {
    if !deno_available() || !uv_available() || !rscript_available()
        || !knitr_r_package_available() { eprintln!("SKIP: tooling"); return; }
    let tmp = setup_marimo_project();               // COMMITTED fixture, unmodified
    let input = tmp.path().join("coexist.qmd");
    write_file(&input, COEXIST_PYTHON_R_DOC);
    let html = render_html(&input).expect("mixed render must succeed");
    assert!(html.contains("<marimo-cell-output>") && html.contains(">2<"),
        "marimo executed its python cell; got:\n{}", body_excerpt(&html));
    assert!(html.contains("[1] 2"),
        "knitr executed the {{r}} cell; got:\n{}", body_excerpt(&html));
    assert!(!html.contains("class=\"{r}"),
        "the {{r}} cell must NOT be raw/unexecuted; got:\n{}", body_excerpt(&html));
}
```

Then extend `q2-preview-spa/e2e/engine-capture-splice-marimo.spec.ts` with a
mixed marimo+knitr `stageMarimoProject(COEXIST_QMD)` case asserting both the
marimo island and `[1] 2` reach the pane (browser tier, `QUARTO_SC21_LIVE=1`).

## Notes / gotchas

- The committed `_extensions/marimo/marimo-engine.js` is a 38-line **download
  shim**; the q2 fixture commits the **real 722-line bundle**. `make build`
  restores the shim via `git checkout` — use `quarto call build-ts-extension`
  directly (Gordon's instruction) to keep the real bundle, then copy it into
  `crates/quarto-core/tests/fixtures/extensions/marimo/_extensions/marimo/marimo-engine.js`
  and update the fixture's reference `src/marimo-engine.ts`.
- Sew-back needs no logic change: `marimo-engine.ts` already passes non-marimo
  cells through verbatim. Seam #4 guards that; no rewrite.
- Do NOT `git checkout -- <file>` to undo a vacuity check on a file with
  uncommitted work — it wipes the additions. Commit first.

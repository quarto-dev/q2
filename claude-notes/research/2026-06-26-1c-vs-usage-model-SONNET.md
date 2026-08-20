# Plan 1c vs. All-Five-Engines Usage Model — Code Review (Sonnet lens)

**Reviewer:** claude-sonnet-4-6
**Date:** 2026-06-26
**Lens:** EQ1–EQ5 — claim/resolution/ownership coverage across all five engines, not only Julia

**Verdict:** The *core* resolution algorithm (`resolve_engines` in `resolution.rs`) and ownership enforcement machinery are correctly implemented against the design contract. The `first_class` path, multi-engine ownership, and the four tiers are all present and tested with coverage that matches (and in some places exceeds) the plan spec. However, three genuine gaps remain — all plan-spec gaps, not landed-code bugs — each shaped by a non-Julia engine behavior that the plan text does not fully cover:

1. **[GAP-1 — plan-spec] `claimsFile` content inspection path has no equivalent in the Rust plan** for jupyter's non-extension-only logic (`isPercentScript(file)` with no extensions arg), and the built-in knitr/jupyter `claims_file` is explicitly deferred ("Future Work" section of 1c) with no test stub.
2. **[GAP-2 — plan-spec] Marimo's `claimsFile: false` return (always false, marimo:219)** and its exclusive `firstClass`-gated selection pattern are not tested as a *negative* case in any plan test item — tests confirm `first_class` is *passed*, but no test guards against an engine that returns `false` from `claimsFile` still being selected (relevant to marimo's architecture).
3. **[GAP-3 — landed code] `KNOWN_ENGINES` and `is_known_engine()` still present in `detection.rs`** despite being slated for deletion in plan Phase 2 ("Remove the `KNOWN_ENGINES` constant…"). The plan item is unchecked. The top-level-key scan in `detect_engines` (lines 236-248) still iterates `KNOWN_ENGINES`, which means extension-engine top-level YAML keys (e.g. `marimo: {…}` or `julia: {…}`) are **never detected by the top-level scan path** — it only triggers for `knitr`/`jupyter`. This is a landed-code gap.

---

## EQ1 — Marimo's numeric score + `first_class`

**Covered. No new gap.**

The resolution contract normalises marimo's numeric return (`2`/`1`) at the TS protocol layer (design §3.2), so by the time a `LanguageClaim` crosses the Rust boundary it is already `Primary(2)` / `Primary(1)`. The Rust `resolve_engines` handles all numeric priorities correctly (kind dominates, integer breaks ties). The critical `first_class` path is:

- **`first_class` is extracted from the AST** in `resolution.rs:108-138` (`walk_block_for_langs` picks the first non-language, non-brace class from `cb.attr.1`) and is **passed to `claims_language`** in all four tier loops (T1/T2/T3/T4).
- `test_first_class_passed_to_claim` (resolution.rs:1101-1122) registers a mock engine that returns `Primary(1)` only for `{python .marimo}` and confirms it wins — directly testing the marimo selection pattern.
- The test uses `engine_cell_with_class("python", "marimo")` and verifies ownership is "marimo". This is an adversarial test; it is the only gap-closing test item in the spec that actually exercises a non-Julia engine claim shape.

**Caveat (EQ2-adjacent):** `first_class` is computed at first-occurrence per language (resolution.rs:137: `if !seen_set.contains(lang)`). A doc with `{python}` before `{python .marimo}` would record `first_class: None` for python, and marimo's engine would return `None` for plain python. This is Q1-faithful (§4.2: "a language has one owner") but the plan text does not call out the doc-order sensitivity. Not a new gap vs the exclusion list, but worth noting.

---

## EQ2 — Content-inspecting `claimsFile`

**GAP-1 (plan-spec): built-in `claims_file` with content inspection is deferred without a test stub.**

**Evidence:**

- `traits.rs:157`: `fn claims_file(&self, _file: &str, _ext: &str) -> bool { false }` — all built-in engines inherit this default.
- 1c plan "Future Work" section (plan1c.md:923-941): "Built-in engine percent/spin script support … No pipeline changes needed … This is out of scope for this plan." Jupyter's `isPercentScript(file)` (no extensions arg; calls into content) and julia's `isPercentScript(file, [".jl"])` (also content-inspecting) are both listed as future work.
- **The plan includes no test stub** for this path. The Phase 3 echo engine exercises `claims_file` extension-only (`.echo` is a pure extension check — no file content read), so the content-inspection branch of `claims_file` has zero E2E coverage in 1c. Plan 4 (Julia validation) is the first time a content-inspecting `claims_file` would run.
- Model Part A, `claimsFile` row: both jupyter and julia call `quarto.jupyter.isPercentScript` which reads file content (model:91, B.3:173).

**Severity (plan-spec):** Medium. The architecture supports it — `EngineClaimsFileStage` caches results and the trait signature is correct. The gap is that the plan makes no acknowledgement of the content-inspection case (no "will exercise via julia Plan 4" note, no stub) and 1c's echo engine can only prove extension-only logic.

**Recommended seam:** The plan's Phase 3 echo engine fixture could add a `.echo` file whose first line triggers content-inspection to prove the path (without requiring real Julia). One test item added: "Write test: `claims_file` that reads file content (not only extension) — use a mock engine with a content-sniffing predicate."

---

## EQ3 — `handled_languages` across real combos

**Covered for the key cases. One new gap (GAP-3, landed code).**

The `knitr`+`jupyter` combination (`[knitr, jupyter]`, `r`+`python` → `r→knitr, python→jupyter`) is tested at:
- `test_explicit_knitr_jupyter_r_python` (resolution.rs:759-777): confirmed T2 explicit Fallback preempts knitr's T3 Interop.
- `test_handled_languages_for` (resolution.rs:1001-1040): confirms knitr's leave-alone set contains `python` and `sql`, and jupyter's contains `r`.
- `test_explicit_knitr_jupyter_r_sql_t2_wins` (resolution.rs:785-800): T2>T3 for sql routing.

Knitr reticulate (implicit `{r}`+`{python}` → single `[knitr]`) is tested at `test_implicit_r_python_knitr_interop` with a vacuity guard.

**GAP-3 (landed code): `KNOWN_ENGINES` deletion not done; top-level key scan is extension-engine-blind.**

- `detection.rs:34`: `pub const KNOWN_ENGINES: &[&str] = &["markdown", "knitr", "jupyter"];`
- `detection.rs:89-91`: `pub fn is_known_engine(name: &str) -> bool { KNOWN_ENGINES.contains(&name) }`
- `detection.rs:236-248`: `detect_engines` loops `for engine_name in KNOWN_ENGINES` to handle top-level-key detection. This is the "Engine-specific top-level keys (e.g. `jupyter:` / `knitr:` with no `engine:` key)" path.

Plan 1c Phase 2 (plan1c.md:650-656): "Remove the `KNOWN_ENGINES` constant and `is_known_engine()` function from `detection.rs` … Replace usage with a query against the registry's engine names: `registry.engine_names()`."

This item has a `- [ ]` (unchecked). The code has not been changed. The consequence: a doc with a top-level `julia: 1.10` key (no explicit `engine:` key) will NOT trigger julia-engine selection via top-level key scan — the scan only checks `knitr` and `jupyter`. This differs from Q1 behavior (`engine.ts:161-169` scans all registered engines). It is a landed-code incompleteness relative to the plan's stated intent.

**Severity (landed-code gap):** Medium. It only affects the top-level-key shorthand (`julia: 1.10` with no `engine: julia`). Explicit `engine: julia` and language-claim paths work. The deletion is explicitly in-plan; the omission is traceable to the item being unchecked.

**Recommended fix:** In `resolve_engines`, after the `engine:` key check, add the registry-scanning equivalent of the top-level-key loop. Remove `KNOWN_ENGINES` (the plan already specifies this). The registry is now available in `resolve_engines` (`registry: &EngineRegistry`), so `registry.engine_names()` is the right replacement.

---

## EQ4 — Conversion seeding for real converters

**Covered by design; plan is internally consistent. No new gap.**

`resolve_engines` signature: `claimed: Option<&str>` (resolution.rs:323). When `claimed` is `Some(name)`:
- The seed engine is added to `explicit_with_seed` (resolution.rs:357-365).
- `is_implicit` is set to `false` (resolution.rs:370: `!has_explicit_engine_key && claimed.is_none()`), which disables T4 — the seed counts as explicit intent, preventing jupyter from silently capturing unclaimed languages.
- The seed engine wins T1 for any language it positively claims; other languages still resolve normally.
- `present` is seeded with the claimed engine (resolution.rs:386-392), enabling T3 Interop.

This matches design §8 exactly. A Julia-converted `.jl` file seeds julia as Primary; a secondary `{bash}` cell still falls through to jupyter via T2 (since julia is now explicit, T2 applies before T4). A generic python extension cannot steal a `.ipynb` (jupyter is the seed, so it wins T1 for python before the extension can claim T2/T4).

The echo engine Phase 3 test covers the seed path via the `.echo` file fixture (plan1c.md:805-819). Plan item: "`claimed_engine_name` propagates from the pre-parse stage and seeds `resolve_engines` as the converted content's Primary owner (resolution still runs)."

The `percentScriptToMarkdown` converter is a `quarto.jupyter` API concern (model B.3), not a `resolve_engines` concern — conversion happens in `EngineClaimsFileStage` before `resolve_engines` runs. The seed result is correct regardless of which API the engine calls internally.

---

## EQ5 — Test representativeness

**Coverage for marimo `firstClass` and knitr+jupyter ownership: present but narrower than all-five-engines ground truth suggests.**

Tests that cover non-Julia, non-markdown shapes:
- `test_first_class_passed_to_claim` — marimo `firstClass` pattern (covered).
- `test_explicit_knitr_jupyter_r_python` and `test_explicit_knitr_jupyter_r_sql_t2_wins` — knitr+jupyter ownership (covered).
- `test_handled_languages_for` — knitr's leave-alone set under explicit `[knitr, jupyter]` (covered).
- `test_implicit_r_python_knitr_interop` + vacuity guard — reticulate Interop (covered).
- `test_fallback_priority_beats_registration_order` — marimo-style higher-priority Fallback (covered).

**Missing test in the plan (not a landed-code gap, a plan-spec gap):**
Plan Phase 2 (plan1c.md:706-708): "Write test: implicit `{r}`+`{python}` → `[knitr]` (knitr `Interop` python; reticulate preserved)" — ✓ present.
Plan Phase 2 (plan1c.md:707-708): "Write test: explicit `engine: [knitr, jupyter]`, `{r}`+`{python}` → `[knitr, jupyter]` with `ownership` = {r→knitr, python→jupyter}" — ✓ present.
Plan Phase 2 (plan1c.md:712-713): "Write test: file-claim seed — a claimed `.echo`/`.jl` file makes the claimer `Primary`, and a second-language cell still resolves to its own owner (secondary)" — `- [ ]` in plan, not yet landed.

The missing file-claim seed test is the only spec-level test gap directly named in the plan. It does not affect the landed code's correctness (seed logic is in `resolve_engines` and is code-covered via mock), but the E2E path (EngineClaimsFileStage → claimed_engine_name → resolve_engines) is covered only by the echo Phase 3 test (also not yet landed — it's a Phase 3 plan item).

**GAP-2 (plan-spec): No negative `claimsFile: false` test for marimo's always-false pattern.**

Model Part A: marimo:219 returns `false` from `claimsFile` (always). No plan test verifies that a `.qmd` file is NOT claimed by an engine that returns `false`. This is a minor gap: the default implementation is `false`, so the correctness is inherited, but no test pins this for marimo's explicit case.

---

## What the plan DOES cover well (confirming, not finding)

- Four-tier algorithm (T1/T2/T3/T4), kind-dominates-priority, presence-gating: fully implemented in `resolution.rs` with all §4.4 worked cases as unit tests.
- `LanguageClaim` enum with `Primary`/`Interop`/`Fallback`/`None`: landed in `engine/mod.rs`, used by `resolve_engines`.
- `handled_languages_for` (§5): correct — `HANDLED_LANGUAGES ∪ { lang: owner ≠ engine }`, sorted, deterministic.
- Multi-engine loop in `engine_execution.rs:311-511`: threads AST, derives `handled_languages` per engine from `resolution.handled_languages_for(engine.name())` (line 328).
- `first_class` extraction in `computational_languages`: extracted from the first non-brace class on the attr class list (resolution.rs:125-136), correctly passed to all four tier calls.
- `EngineResolution` stashed on `StageContext` (engine_execution.rs:242-243): `ctx.engine_resolution = Some(resolution.clone())`.
- Replay drives from `engine_captures` not re-resolution: the plan specifies this (plan1c.md:479-482); `EngineRegistry::with_replay_many` exists.
- Static-claim forms (`claims:`, `claims-files:` in `_extension.yml`): design §3.3 — documented in the design contract as a future possibility; plan1c correctly scopes to dynamic claims only.
- `HANDLED_LANGUAGES` constant exclusion from computational language scan: correctly implemented (resolution.rs:118-120).

---

## Summary of new findings

| Key | Type | Severity | Short description |
|---|---|---|---|
| GAP-1 | plan-spec gap | Medium | `claimsFile` content-inspection path (jupyter/julia `isPercentScript`) is deferred with no test stub; echo engine only covers extension-only logic |
| GAP-2 | plan-spec gap | Low | No negative `claimsFile: false` test for marimo's always-false pattern |
| GAP-3 | landed-code gap | Medium | `KNOWN_ENGINES` deletion not done; top-level-key scan (`detect_engines` loop) is blind to extension engines; `julia: 1.10` shorthand won't select julia engine |

---

## What I read

| File | Lines / note |
|---|---|
| `claude-notes/research/2026-06-26-engine-api-usage-model.md` | Full — Parts A/B/C/D |
| `claude-notes/plans/2026-04-16-plan1c-extension-integration.md` | Full — all phases + success criteria |
| `claude-notes/designs/engine-resolution.md` | Full — §1-§13 |
| `crates/quarto-core/src/engine/detection.rs` | Full — 628 lines including KNOWN_ENGINES constant and detect_engines top-level-key scan |
| `crates/quarto-core/src/engine/registry.rs` | Full — 358 lines |
| `crates/quarto-core/src/engine/resolution.rs` | Full — 1149 lines including all §4.4 tests |
| `crates/quarto-core/src/engine/traits.rs` | Full — 266 lines; confirmed claims_language, claims_file, markdown_for_file signatures |
| `crates/quarto-core/src/stage/stages/engine_execution.rs` | Lines 1-522 (run() + tests through multi-engine) — confirmed resolve_engines call at line 242, handled_languages_for at line 328 |

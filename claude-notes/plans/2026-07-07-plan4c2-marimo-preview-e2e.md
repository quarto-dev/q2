# Plan 4c.2: Marimo through `q2 preview` — capture-splice fix + full browser e2e

**Status:** plan (2026-07-07). Driving strand: **bd-5jxcio5d** (P2, bug —
"q2 preview capture-splice cannot splice engines that emit unwrapped output").
**Sequence:** continuation of Plan 4c (marimo validation), which completed the
`q2 render` tier (15 tests green) and *pinned* the preview gap with the
SC21-NEG limitation canary. This plan **fixes** the gap and adds the positive
browser-level e2e coverage Plan 4c deferred — the marimo equivalent of Plan 4's
julia preview validation.
**Depends on:** Plan 4c (fixture, engine, the frozen SC1–SC21 seams); the
q2-preview capture→splice delivery chain (bd-h4rhohhy, PC5/PC6, already
merged). Plus `deno`+`uv` present (marimo runs via `uv run --with`).
**Model:** Plan 4 / bd-h4rhohhy's julia preview e2e —
`q2-preview-spa/e2e/engine-capture-splice-julia.spec.ts` (PC6 minimal + PC6-FIG
figure) and the native seam file
`crates/quarto-core/tests/integration/capture_splice_seam.rs`. This plan mirrors
both tiers for marimo.

## Overview

Plan 4c established that **marimo renders fully via `q2 render`** but does **not**
reach the `q2 preview` pane — FINDING #5. The SC21-NEG canary asserts the
limitation (pane shows inert source) and is contracted to redden and flip to a
positive test once the fix lands. This plan:

1. **Fixes bd-5jxcio5d** so marimo's captured output splices into the pane.
2. **Flips SC21-NEG → positive** and adds full preview e2e (minimal, widget,
   sql-interop), opt-in-gated exactly like the julia PC6 spec.

## The bug, at code level (precise — this is the RED the fix must turn GREEN)

`q2 preview` records engine execution as a capture and, on later edits, splices
the recorded output onto the live AST via
`crates/quarto-core/src/engine/capture_splice.rs`. The algorithm
(`derive_cell_outputs`) walks the capture's pre-engine AST `A1` and post-engine
AST `B1` in lockstep: **each engine cell in `A1` is matched to the next
`is_cell_wrapper` block in `B1` — a `Div` whose class list contains `"cell"`**
(`capture_splice.rs:89`, the `while … !is_cell_wrapper` loop at `:195`).

Julia and the other engines produce `::: {.cell}` wrappers (via the shared
engine-host's `mdFromCodeCell`, `ts-packages/quarto-api/src/jupyter/to-markdown.ts`).
**Marimo does not.** Its `execute()` builds `processedMarkdown` directly, where
each executed cell is a bare `{=html}` RawBlock island
(`<marimo-island>`/`<marimo-cell-output>`), zero `.cell` Divs. So for a marimo
capture, `is_cell_wrapper` is false for every `B1` block, the match loop finds
nothing, `derive_cell_outputs` records no entry, and every marimo cell falls
through to raw source in the pane. The capture records server-side (verified in
4cH: `recorded engine capture(s) engines=marimo`) — the break is purely in
`B1`-side matching.

## Phase A — Fix bd-5jxcio5d (the splice must handle unwrapped engine output)

### A0. Characterize the real marimo `B1` (research; produces the RED evidence)
- [x] Record a real marimo capture and dump `capture.result.markdown` (`B1`)
      and its parsed block structure. Marimo has no shared daemon — a scratch
      `q2 preview` against a copy of the marimo fixture + reading the captures
      dir (as 4cH did) is enough; OR add a throwaway dump in a scratch test.
      Answer concretely: is each executed cell **one** RawBlock island at the
      cell's position? Are there prose/heading blocks interleaved as in `A1`?
      Where does the `__MARIMO_EXPORT_CONTEXT__`/`<marimo-code>` header sit
      (it flows through `include-in-header`, NOT the markdown body — confirm it
      is absent from `B1.blocks`)? Record the block-shape verbatim in the plan
      + `capture_splice_seam.rs`'s doc comment; it defines SC22's synthetic-but-
      faithful `B1`.
- [x] Write the **RED** first: add the SC22 native seam (below) with a
      marimo-shaped `B1`; confirm it fails today (marimo cell → raw source).

**A0 findings (recorded 2026-07-07 from a REAL marimo capture — `record_capture`
against the committed fixture, doc = `# SC22 heading` + `{python .marimo}` cell
`40 + 2`; full dump in `.superpowers/sdd/a0-report.md` / `a0-dump.log`):**
1. **One RawBlock per cell.** Each executed marimo cell is exactly ONE
   `RawBlock{format:"html"}` at the cell's position — the `<marimo-island>`.
   No splitting, no `.cell` Div anywhere.
2. **Heading passes through lockstep.** `B1.blocks == [Header("SC22 heading"),
   RawBlock(html island)]`, matching `A1.blocks == [Header, CodeBlock{python
   .marimo}]` position-for-position. So the existing prose/heading lockstep
   walk pairs `A1[0]↔B1[0]` (structural Header match) and advances both to the
   cell position `[1]`, where the `{python .marimo}` cell meets the island.
3. **Header markers absent from the body.** `__MARIMO_EXPORT_CONTEXT__` and
   `<marimo-code` are BOTH absent from `B1.blocks` (grep=false) — they flow via
   `include-in-header` into the HTML `<head>`, never the markdown body. (The
   distinct `<marimo-cell-code hidden>` tag DOES appear *inside* the island
   RawBlock text — the per-cell hidden source — but that is content of the
   output block, not a separate block.)
4. **The RawBlock, verbatim:** `format = "html"`; `text` =
   `<marimo-island … data-reactive="true"> <marimo-cell-output> <pre
   class='text-xs'>42</pre> </marimo-cell-output> <marimo-cell-code
   hidden>import%20marimo%20as%20mo%0A40%20%2B%202</marimo-cell-code>
   </marimo-island>`.

**RED confirmed.** `marimo_shaped_capture_splices` (SC22) FAILS today: the
`{python .marimo}` cell falls through to raw source, so `out.blocks[1]` is a
`CodeBlock` (classes `["{python}","marimo"]`, text `import marimo as mo\n40 + 2`)
— the cell survived, no island. Companion guards `cell_wrapped_capture_splices`
+ `real_echo_capture_splices` stay GREEN (3 passed, 1 failed). `capture_splice.rs`
is UNMODIFIED — the fix is Phase A2, gated on the A1 design decision.

**Root cause pinned for A1:** at the paired cell position, `B1[1]` is a
`RawBlock`, so `is_cell_wrapper(B1[1])` is false → the `while … !is_cell_wrapper`
loop breaks immediately → no map entry → fall-through. The fix must let an `A1`
engine cell pair with a non-`.cell` `B1` block **at its lockstep position**.

### A1. DESIGN DECISION — how to pair an unwrapped `B1` block to an `A1` cell
**STOP and get Gordon's ratification before implementing.** Two approaches; a
recommendation follows.

- **Option (a) — splice-side generalization (RECOMMENDED).** Relax the
  `derive_cell_outputs` matcher so an `A1` engine cell can pair with its
  lockstep-positioned `B1` block even when that block is not a `.cell` Div
  (e.g. a RawBlock island). Keep the fail-soft prose-lockstep guard; the
  generalization is "at the paired position, an engine cell consumes the
  engine's output block(s) there," not "consume anything." Rationale: fixes it
  in **engine-agnostic core**, consistent with the repo's no-per-engine-special-
  casing philosophy; leaves marimo's **validated render output unchanged** (all
  15 Plan-4c render tests + the render DOM stay byte-identical — Option (b)
  would change them and force re-validation). Risk to manage in design: never
  mis-pair a prose block as output — the A0 characterization pins exactly which
  `B1` positions are engine output.
- **Option (b) — make marimo emit `.cell` wrappers.** Change the marimo engine
  (upstream) so each island is wrapped in a `::: {.cell}` Div, so the existing
  splice works unchanged. Rejected as the default: changes marimo's render
  output (re-validation of the whole render tier), grows the upstream diff, and
  pushes engine-shape knowledge into the engine instead of keeping the core
  general. Keep on file as the fallback if (a)'s matcher proves unsafe.
- [x] Present (a) vs (b) with the A0 evidence; implement the ratified option.
      **Gordon ratified Option (a)** — splice-side generalization in
      `capture_splice.rs` (engine-agnostic; marimo render output unchanged).

### A2. TDD the fix
- [x] With SC22 RED (A0), implement the ratified matcher change in
      `capture_splice.rs`. Green SC22. The **named revert** is exactly this
      hunk: reverting it returns the marimo cell to raw source → SC22 RED.
      Implemented as `is_engine_output_block(block) = is_cell_wrapper(block) ||
      matches!(block, Block::RawBlock(_))`, swapped into the two `is_cell_wrapper`
      uses in `derive_cell_outputs_walk`'s engine-cell branch. Named-revert
      confirmed: reverting the swap → SC22 RED (`out.blocks[1]` is a `CodeBlock`).
- [x] Regression guard (refactor-vacuity): the existing wrapped-cell seams
      (`cell_wrapped_capture_splices`, `real_echo_capture_splices`, the julia
      figure-nesting tests) must stay GREEN — the generalization must not break
      `.cell`-wrapper matching. Assert this explicitly in SC22's companion.
- [x] `cargo nextest run -p quarto-core` green (splice + all engine e2e rows).
      2656 passed, 0 failed, 34 skipped.

### A3. End-to-end confirm (the real proof)
- [x] Rebuild the preview binary fresh (the `include_dir!` chain —
      `q2-preview-spa/e2e/README.md` / `preview-spa-rebuild.md`). Verified
      `target/debug/q2` mtime (09:48) postdates the fix commit `4cfc3b1ae`
      (09:42) — no rebuild was needed.
- [x] Flip **SC21** NEG → positive (Phase B / SC21) and run it live: the
      executed `42` + `<marimo-cell-output>` appear in the pane without reload.
      This is the binding real-engine proof that A2's synthetic `B1` was
      faithful. GREEN in 4.2–6.3s across live runs; pane HTML observed:
      `<marimo-island …><marimo-cell-output><pre class="text-xs">42</pre>
      </marimo-cell-output><marimo-cell-code hidden>…</marimo-cell-code>
      </marimo-island>` — the literal source survives only URL-encoded
      (`40%20%2B%202`) inside the hidden `<marimo-cell-code>`, never as the
      literal `40 + 2` string.

## Phase B — Full preview e2e (modeled on julia PC6, opt-in gated)

All specs live in `q2-preview-spa/e2e/engine-capture-splice-marimo.spec.ts`
(SC21's file), gated on `deno`+`uv` presence AND opt-in **`QUARTO_SC21_LIVE=1`**
(mirrors julia's `QUARTO_PC6_LIVE`; skips by default so CI stays fast). Marimo
has **no shared daemon / no transport file** — the per-test isolation is just a
temp project copy (no `isolateJuliaProject`/HOME override needed; state this in
the spec header, as SC21's frozen row already notes).

- [x] **SC21 (minimal — the flip).** `{python .marimo}` `40 + 2` doc → pane
      shows `marimo-cell-output` AND `42` without reload; literal `40 + 2`
      absent from the pane **body** (scope excludes the head `notebookCode:`
      script — the corrected premise in SC21's own flip sketch). GREEN live
      (4.2-6.3s); see A3 evidence above.
- [x] **SC23 (widget markup delivery).** `mo.ui.slider(...)` doc → the widget
      island markup (`<marimo-island>` + `<marimo-ui-element>`/`<marimo-slider>`)
      reaches the pane. **Assert markup delivery, NOT interactivity** — see the
      hydration decision below; this seam proves the *splice* delivers the
      widget, not that marimo's client runtime hydrates it in the preview
      sandbox. GREEN live (4.4s); pane HTML observed:
      `<marimo-island …><marimo-cell-output><marimo-ui-element …><marimo-slider
      data-start="1" data-stop="10" data-initial-value="5" …></marimo-slider>
      </marimo-ui-element></marimo-cell-output>…</marimo-island>` — markup
      delivered, no attempt made to assert hydration/interactivity.
- [x] **SC24 (sql-interop in preview — the 4cH accepted-untested item).**
      `{python .marimo}` + bare `{sql}` doc (with the pyproject deps block) →
      the executed sql island (`data-data='[{"x":2}]'` markup) reaches the pane.
      Binds the same splice fix over the interop path end-to-end in the browser.
      GREEN live (5.5s — deps were warm from earlier render-tier runs, not a
      cold `uv` resolve); pane HTML observed BOTH islands: the python cell's
      `<marimo-island>…<pre class="text-xs">2</pre>…</marimo-island>` AND the
      sql cell's `<marimo-island>…<marimo-table data-data="&quot;[{\&quot;x\&quot;:2}]&quot;"
      …></marimo-table>…</marimo-island>` — innerHTML re-serializes the
      attribute's embedded quotes as `&quot;` (not a decode), so the `x":2`
      payload is present just re-escaped, not raw-decoded as originally
      assumed; the test asserts on `marimo-table` + the literal `2` rather
      than a brittle exact-attribute match.

### Open decision (surface to Gordon; may spawn a follow-up strand)
Marimo widgets **hydrate client-side** via an external islands script
(`https://cdn.jsdelivr.net/npm/@marimo-team/islands@.../main.js`). Static
executed output (`42`, sql tables) is baked into the server-rendered island
markup and shows **without JS** — SC21/SC24 are robust. Interactive widgets
(SC23) need that script to load and run **inside the preview pane's sandboxed
iframe** (CSP / external-host constraints may block it). SC23 therefore asserts
markup delivery only; whether the islands runtime executes in the preview
sandbox is a **separate question** — decide whether to (i) accept markup-only
as the SC23 contract and file a strand for pane-hydration, or (ii) fold a
hydration probe in. Record the decision; do not silently claim interactive
widgets work in preview.

## Phase C — Bookkeeping
- [x] Rewrite SC21's row: NEG canary → positive splice test (dated annotation,
      per the frozen-seam correction convention; the row's own flip sketch is
      the authority). **Landed 2026-07-07:** SC21 flipped NEG→positive in
      `engine-capture-splice-marimo.spec.ts` (commit `29476fe8`), live GREEN
      (pane shows `<marimo-cell-output>42</marimo-cell-output>` without reload);
      the Test Seam Spec SC21 row below is the positive contract it now meets.
- [x] Compat doc (`claude-notes/research/2026-07-02-marimo-engine-q2-compat.md`)
      + migration guide: FINDING #5 RESOLVED — marimo now splices in preview;
      note the matcher generalization and the widget-hydration disposition.
      **Done:** FINDING #5 RESOLVED block added at the top of the finding
      (matcher generalization `is_engine_output_block`; hydration disposition
      recorded as pending Gordon's (a)/(b) decision — see Open decision above).
- [x] `cargo nextest run --workspace` + full `cargo xtask verify` (splice is in
      `quarto-core` → the WASM/preview leg matters). **GREEN 2026-07-07:** full
      `cargo xtask verify` — Rust workspace 10617 tests pass, ts-packages,
      hub-client (WASM + build + tests), q2-preview-spa build all green
      (`✓ All verification steps passed!`). NOTE: this worktree needed a
      `npm install` first (no root `node_modules`) — the initial ts-packages
      leg failure was that env gap, not a code regression.
- [x] Close bd-5jxcio5d with the fix commit + the green live SC21 run.
      **CLOSED 2026-07-07** (fix `4cfc3b1ae`; live SC21 green). Final whole-plan
      review: Ready to merge. Follow-up `bd-5m1ni9if` filed (narrow
      RawBlock-misconsume edge, doc-tightened). Branch unpushed pending Gordon's
      cumulative push approval. Open: SC23 widget-hydration disposition — Gordon's
      (a) accept markup-only + file strand / (b) fold hydration probe.

## Test Seam Spec (frozen once green — prevalidated 2026-07-07)

One row per test: **tier · real unit (never mocked) · seam → assertion surface ·
mock boundary · named revert hunk → RED**. Tiers: `int-rs` (native AST, no
subprocess/browser), `e2e-pw` (real `q2 preview` binary + chromium + real
uv/marimo, opt-in `QUARTO_SC21_LIVE=1`).

| ID | Phase | Tier | Real unit | Seam → assertion | Mock boundary | Revert hunk → RED |
|----|-------|------|-----------|------------------|---------------|-------------------|
| SC22 | A | int-rs | `derive_cell_outputs`+`splice_cells` (real `capture_splice.rs`) | `A1=[heading,{python .marimo}cell]`, `B1=[heading, RawBlock(html, "<marimo-island>…<marimo-cell-output>42</marimo-cell-output>…")]` (marimo-shaped per A0), `A2=A1` → spliced block for the cell is the island (contains `42`/`marimo-cell-output`), NOT the raw CodeBlock | none (pure AST; synthetic-but-faithful `B1`) | Revert the A2 matcher generalization in `capture_splice.rs` → marimo cell falls through to raw source (`out.blocks[1]` is a `CodeBlock`, not the island) RED. **Companion (refactor-vacuity guard):** `cell_wrapped_capture_splices` + `real_echo_capture_splices` stay GREEN under the fix — the generalization must not break `.cell` matching |
| SC21 | A/B | e2e-pw | full preview delivery chain + real marimo | `{python .marimo}` `40 + 2` doc via real `q2 preview` → pane (no reload) contains `marimo-cell-output` AND `42`; literal `40 + 2` absent from pane **body** (excl. head `notebookCode:` script) | none (real binary+chromium+uv) | Same fix hunk as SC22 → marimo island never reaches the pane → `42`/`marimo-cell-output` absent RED. (This is SC21's own frozen flip: NEG→positive.) |
| SC23 | B | e2e-pw | preview delivery of widget markup | `mo.ui.slider(...)` doc → pane contains `<marimo-island>` + `<marimo-ui-element>`/`<marimo-slider>` markup (delivery, not interactivity) | none; **hydration explicitly out of scope** (decision above) | Fix hunk → widget island markup absent from pane RED |
| SC24 | B | e2e-pw | preview delivery over the sql-interop path | `{python .marimo}`+bare `{sql}` doc (pyproject deps) → pane contains the executed sql island (`data-data='[{"x":2}]'` / `marimo-table` markup) | marimo/uv (env skip) | Fix hunk → sql island absent from pane RED |

**Vacuity notes (traps this spec closes):**
- **SC22 must assert the OUTPUT reached the cell position** (island/`42` present,
  the block is no longer a `CodeBlock`), not merely "no panic" — and its
  companion must prove the `.cell` path still works, or the generalization could
  silently over-match. SC22 (fast, unconditional) and SC21 (real engine) **bind
  as a set**: SC22 guards the matcher logic; SC21 guards that the synthetic `B1`
  matched reality. Neither alone is sufficient — a wrong synthetic `B1` passes
  SC22 while real marimo still fails, which only SC21 catches.
- **SC21's literal-absent check is scoped to the pane BODY**, excluding the head
  `notebookCode:` script — the original SC21-NEG premise ("source only
  URL-encoded") was wrong (the literal survives in the header script); the flip
  sketch already records this.
- **SC23 asserts markup delivery, not interactivity** — a "widget works" browser
  assertion could pass or fail for reasons unrelated to the splice (islands JS
  loading in the sandbox). Keeping SC23 to delivery keeps it bound to the splice
  fix; hydration is a separate, explicitly-logged question.
- **Skip-without-flag is BY DESIGN** (opt-in live tier). The deliverable
  includes one recorded LIVE run (flag set) per e2e seam with timing — a
  skip-only green does not close a phase.

**Missing-test pass (accepted-untested, logged):**
- **Marimo widget client-side hydration in the preview pane** — deferred per the
  Phase-B decision; if markup-only is accepted for SC23, file a strand for
  pane-hydration rather than leaving it implied.
- **Multi-cell / reorder splice for marimo** — the `(hash, occurrence)` keying is
  engine-agnostic and already covered by the existing splice unit tests; SC22
  need not re-test occurrence keying, only the unwrapped-`B1` matcher.

## Model references
- Julia preview e2e (the template): `q2-preview-spa/e2e/engine-capture-splice-julia.spec.ts`
  (PC6 minimal, PC6-FIG figure; opt-in `QUARTO_PC6_LIVE=1`; per-test isolation).
- Native splice seam (the fix's binding tier):
  `crates/quarto-core/tests/integration/capture_splice_seam.rs`.
- The gap's origin + the flip contract: Plan 4c Phase 4cH / seam SC21-NEG in
  `claude-notes/plans/2026-07-02-plan4c-marimo-validation.md`, and the SC21 spec
  header's flip sketch.
- Preview build chain + test invocation: `q2-preview-spa/e2e/README.md`.

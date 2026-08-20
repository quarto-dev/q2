# Plan 8 — HANDLED_LANGUAGES → claiming engines: absorb #241 (mermaid) + graphviz TS extension

> # ⛔ TOMBSTONED — 2026-07-24
>
> **This plan is obsolete and will not be implemented.** Its founding premise —
> that diagram languages (`mermaid`, `dot`) should be handled by *engines* that
> *claim* those languages — has been overturned by work on `main`.
>
> **The decision on `main`: diagrams are NOT engines.** Mermaid was pivoted away
> from the engine model to a "regular" rendering feature: a format-gated **AST
> transform** (`crates/quarto-core/src/transforms/mermaid.rs`) that rewrites
> ` ```mermaid ` fenced blocks into `RawBlock(HTML, …)` for `q2 render`, plus a
> built-in React `CodeBlock` override for `q2 preview`. See
> [`2026-07-20-mermaid-regular-rendering.md`](2026-07-20-mermaid-regular-rendering.md)
> ("Mermaid diagrams as a *regular* rendering feature (non-engine)"; braid
> bd-5m4ga0s1, epic bd-je48v). The engine-model branch `feature/mermaid-engine`
> (PR #241) was **never merged to `main`**.
>
> Consequences, part by part:
> - **Part A (mermaid as an implicitly-claiming built-in engine)** — dead.
>   `MermaidEngine`/`feature/mermaid-engine` never landed; mermaid is a transform,
>   not an engine, and there is no `HANDLED_LANGUAGES` constant left to drain (it
>   no longer exists on `main`).
> - **Part B (graphviz `dot` as a TS engine-extension "proving ground")** — the
>   diagrams-are-engines premise is gone, so `dot`, like mermaid, belongs on the
>   transform path, not an engine claim. If a TS-engine proving ground is still
>   wanted, choose a genuine *computation* engine (not a diagram renderer) and
>   file a fresh plan.
> - **"Enables Plan 6 Q4 (drain `HANDLED_LANGUAGES`)"** — moot: the constant is
>   already gone from `main`.
>
> Original content is preserved below for historical reference only.
> **Do not implement any of it.**

**Status:** ⛔ **TOMBSTONED 2026-07-24** — obsoleted by the "diagrams are not engines" decision on `main` (see banner). ~~plan — ready to implement (two workstreams).~~ **Created:** 2026-07-02.

> **Plan 10 opportunity (noted 2026-07-04, bd-4qflzhwh):** once Plan 10 lands,
> any TS engine implementing `checkInstallation` automatically becomes a
> `q2 check <name>` target. Part B's graphviz extension should implement a
> *real* `checkInstallation` (probe the `dot` binary + version) — it would be
> the first non-stub TS-engine check and a strong e2e validation of the Plan 10
> wire path. See `claude-notes/research/2026-07-03-plan10-check-installation-research.md`.
**Sequence:** post-Plan-1c (needs static-claim resolution + the TS-engine
extension stack for Part B). Part A (mermaid) is independent of the TS-engine
subprocess and can land first; Part B (graphviz) needs Plans 1a–c + 1b + 2A.
**Depends on:** Part A — `claims_language` / `LanguageClaim` + `resolve_engines`
(plan1a-engine, landed) and PR **#241** (`feature/mermaid-engine`). Part B — the
full TS-engine extension path (plan1a-*, 1b, 1c, 2A) + `q2 build-ts-extension`.
**Enables:** Plan 6 Q4 (draining `HANDLED_LANGUAGES` so languages are not
hard-coded).

## Overview

An epic goal is that **computational languages are no longer hard-coded
anywhere** — no `HANDLED_LANGUAGES` allow/leave-alone constant, just engines that
*claim* the languages they handle (`engine-resolution.md` §3–4). Today
`HANDLED_LANGUAGES = ["ojs", "mermaid", "dot"]` (`engine/mod.rs:123`, mirrored in
`jupyter/text_execute.rs`) is the last hard-coded language list. Plan 8 drains
two of its three entries by turning their handlers into **claiming engines**:

- **Part A — `mermaid`:** absorb PR #241's `MermaidEngine` (a native Rust engine)
  and make it *implicitly claim* the `mermaid` language, removing `mermaid` from
  `HANDLED_LANGUAGES`. Always-available built-in ⇒ a clean, unconditional removal.
- **Part B — `dot`:** port Quarto 1's graphviz cell handler
  (`core/handlers/dot.ts`) to a **TypeScript engine extension** that statically
  claims `dot` in its `_extension.yml`, and bundle it. This is deliberately a
  **proving ground for the TS-engine extension system end-to-end** — build →
  bundle → subprocess load → static-claim resolution → `execute()` → HTML output
  — using a real external WASM dependency (`@hpcc-js/wasm`). **A native Rust port
  is possible but explicitly out of scope**; TS is chosen only to exercise the
  extension path.

After Plan 8, only `ojs` remains in `HANDLED_LANGUAGES` — and it "won't be far
behind" (its own follow-up, not this plan).

Both parts follow the established **B1 direct-HTML-emission** pattern (mermaid's
precedent) — emit a ` ```{=html} ` raw block — with `bd-mqk49` (engine → stage
extension API) tracking the eventual marker-`Div` + format-conditional AST-pass
refactor. Q2 is HTML-only today, so format-locked emission is acceptable.

---

## Part A — Absorb #241: mermaid as an implicitly-claiming built-in engine

PR #241 already landed `MermaidEngine` (native, in-process, registered in both
native + WASM registries; text-level fence scanner; B1 emission; preview
RawBlock script-re-execution shim). See the earlier analysis: it is selected
today **only** by explicit `engine: mermaidjs` because `mermaid` is a
`HANDLED_LANGUAGE` (excluded from computational-language detection), so
`resolve_engines` never selects it implicitly. Part A closes that gap.

### Checklist (TDD — tests first)

- [ ] **Failing test:** a doc with a bare ` ```{mermaid} ` cell and no `engine:`
      key resolves to `[mermaidjs]` (currently resolves to markdown/jupyter).
      Add to `resolution.rs` tests with a registry including `MermaidEngine`.
- [ ] **Failing test:** `{mermaid .foo}` (attributed fence) is both *claimed*
      and *transformed* — proves the scanner/claim predicate agreement (see
      scanner fix below).
- [ ] **`claims_language` on `MermaidEngine`** (`engine/mermaid.rs`):
      `claims_language("mermaid", _) → Primary(1)`, else `None`. Native trait
      impl (no subprocess).
- [ ] **Remove `mermaid` from `HANDLED_LANGUAGES`** (`engine/mod.rs:123` →
      `["ojs", "dot"]`; update the assertion at `mod.rs:308`). Safe because
      `MermaidEngine` is *always registered* in both native and WASM, so
      `mermaid` always gets an owner → knitr/jupyter still cede it via the
      ownership projection (`handled_languages(k) = … ∪ {lang : ownership[lang]
      != k}`, §5), not via the constant.
- [ ] **Loosen the fence scanner** (`render_mermaid_cells` in `mermaid.rs`): the
      strict `info.trim() == "{mermaid}"` misses `{mermaid .cls}` / attributes.
      Match `{mermaid}` + optional attribute list so the *transform* predicate
      matches the *claim* predicate (which is language-only). Without this, an
      attributed cell would be selected but silently passed through unrendered.
- [ ] **Update the hard-coded lists in `jupyter/text_execute.rs`** (lines ~415,
      451, 496, 558) that repeat `["ojs","mermaid","dot"]` — route them through
      the `HANDLED_LANGUAGES` constant (or the ownership projection) so there is
      one source of truth, not three. (These are the leave-alone lists jupyter
      re-emits; they must track the constant.)
- [ ] **Verify** `mermaid` still works via explicit `engine: mermaidjs` and in
      `engine: [knitr, mermaidjs]` sequences (knitr cedes `{mermaid}`).
- [ ] End-to-end: `q2 render` a `{mermaid}`-only doc with no `engine:` key →
      `<pre class="mermaid">` + script present. `q2 preview` likewise (the
      RawBlock shim from #241).

### Notes
- This is the model for every `HANDLED_LANGUAGES` graduation: *a handler becomes
  an engine that claims its language; the constant shrinks.*
- #241's honest gaps (`bd-mqk49` stage-extension API; `bd-cp3em` CaptureSplice
  aux-field drop) are carried as cross-refs, **not** Part A scope.

---

## Part B — Graphviz `dot` as a TS engine extension (proving ground)

Port Quarto 1's `core/handlers/dot.ts` to a q2 **TypeScript engine extension**.
Q1's handler dynamically imports a graphviz WASM (`@hpcc-js/wasm`, via the
vendored `resources/js/graphviz-wasm.js`) and calls
`graphviz().layout(source, "svg", graph-layout)` to render each `{dot}` cell to
SVG server-side, then emits ` ```{=html} `-wrapped SVG for HTML output (and a
PNG-rasterized figure for non-HTML). q2's port keeps the **HTML/SVG path only**.

### Why a TS extension (the proving-ground rationale)

There is no engine-specific reason to use TS here — the point is to exercise the
**entire extension path** with a realistic engine that pulls an external WASM
dependency:
- author an engine module against `@quarto/api` / `@quarto/types`;
- **static `_extension.yml` claiming** of the `dot` language (zero-load
  resolution, §3.3) — the first non-Julia validation of static claims;
- `q2 build-ts-extension` **bundling** (`deno bundle`, with `@hpcc-js/wasm`
  inlined/fetched);
- subprocess **load → LaunchEngine → execute** round-trip;
- HTML output flowing back through the protocol.

It complements the Julia benchmark (which validates a *daemon/kernel* engine);
graphviz validates a *pure-transform* engine with a bundled WASM asset and
server-side rendering — closer in shape to mermaid but exercised through the
subprocess rather than as a built-in.

### Engine shape

- **`execute()`** (Deno subprocess): scan `{dot}` executable cells; for each,
  `graphviz().layout(cellSource, "svg", options["graph-layout"] ?? "dot")`; emit
  ` ```{=html}\n<svg …>\n``` `. Non-`{dot}` content passes through verbatim
  (surgical, like mermaid). Server-side SVG ⇒ **no browser runtime** needed
  (contrast mermaid's client-side `mermaid.run()`).
- **`_extension.yml`** (static claims):
  ```yaml
  contributes:
    engines:
      - path: graphviz-engine.js
        name: graphviz
        claims:
          dot: { kind: primary, priority: 1 }
        # no file-extensions / claims-files: dot is a fenced language, not a file type
  ```
- **Options parity (scoped to HTML):** honor `graph-layout` (dot/neato/…),
  `fig-width`/`fig-height`/`fig-responsive`, `fig-align`, `fig-cap`, `label`.
  **Out of scope:** the PDF/typst/ipynb PNG-rasterization branch
  (`createPngsFromHtml`) — Q2 is HTML-only (matches mermaid's format-lock).
- **Nice-to-have (not required):** Q1's graphviz "syntax error in line N" →
  source-file/line remap, using the `source_map` in `TsExecuteOptions`.

### The `HANDLED_LANGUAGES` / distribution decision (must settle)

To make the extension's `dot` claim fire, **`dot` must leave
`HANDLED_LANGUAGES`** (otherwise resolution excludes `{dot}` from computational
languages and the claim never runs). But unlike mermaid (an always-available
built-in), a TS *extension* is only present if installed. So removing `dot` from
the constant without guaranteeing the extension is available is a **regression**:
a base-q2 `{dot}` cell would become an unclaimed computational language →
jupyter fallback (implicit T4) → §10 case-4 loud failure (no `dot` kernel).

**Recommended:** ship the built graphviz extension as a **default-bundled
extension** so `dot` stays universally handled after it leaves
`HANDLED_LANGUAGES` — no regression, and it doubles as the reference bundled
extension. Alternatives to weigh: (b) keep `dot` in `HANDLED_LANGUAGES` and treat
the extension as opt-in — **rejected**, because then the claim can never fire;
(c) accept base-q2 dot as unsupported pending a Rust port — a real regression,
not recommended. **This is the one blocking design decision for Part B.**

### Checklist (TDD — tests first)

- [ ] **Failing test:** an installed graphviz extension + a `{dot}` doc (no
      `engine:` key) resolves to `[graphviz]` via static `_extension.yml` claim,
      **without loading** the subprocess (zero-load resolution assertion).
- [ ] **Failing test:** `q2 build-ts-extension` produces a loadable
      `graphviz-engine.js` bundle that includes `@hpcc-js/wasm`.
- [ ] Author `graphviz-engine.ts` against `@quarto/api` / `@quarto/types`;
      implement discovery (static claims mirror `_extension.yml`) + `execute()`.
- [ ] `_extension.yml` with the static `claims: { dot: primary }`.
- [ ] Remove `dot` from `HANDLED_LANGUAGES` (+ `mod.rs:308` assertion +
      `text_execute.rs` lists) — gated on the distribution decision above.
- [ ] End-to-end: `q2 render` a `{dot}` doc → inline `<svg>` in the HTML; inspect
      the output. `q2 preview` → SVG renders (no client runtime needed).
- [ ] Verify `graph-layout`, `fig-width`/`fig-height`, `fig-cap`/`label` map
      through to the emitted figure.

---

## HANDLED_LANGUAGES endgame

| language | Plan 8 outcome | resulting `HANDLED_LANGUAGES` |
|---|---|---|
| `mermaid` | built-in `MermaidEngine` claims it (Part A) | removed (unconditional) |
| `dot` | graphviz TS extension claims it (Part B) | removed (**gated on default-bundling**) |
| `ojs` | out of scope | remains — own follow-up |

After Plan 8, `HANDLED_LANGUAGES` is `["ojs"]` (or `["ojs","dot"]` if the dot
extension ships opt-in). This is the concrete payoff for Plan 6 Q4.

## Open questions

1. **Default-bundle vs opt-in for the graphviz extension** (the Part B blocker
   above). Recommend default-bundle.
2. **Where the reference extension lives** in the repo (an `extensions/`
   examples tree? alongside the echo engine from plan1c Phase 3?) and how
   `q2 build-ts-extension` is wired into CI so the bundle stays fresh.
3. **`@hpcc-js/wasm` in a `deno bundle`** — confirm the WASM asset inlines/loads
   cleanly under the subprocess `deno run --allow-all` model (no import map);
   this is part of what Part B proves.
4. **ojs** — note the path (a third graduation) without scoping it here.

## Relationship to other plans

- **Plan 6** — Part A/B are the concrete mechanism behind Plan 6's Q4 answer
  (drain `HANDLED_LANGUAGES`); Plan 6 references this plan as the prerequisite.
- **plan1c** — Part B is the second real exercise of static `_extension.yml`
  claiming + the `build-ts-extension` path (after the echo engine).
- **Plan 4 (Julia)** — complementary validation: Julia = daemon engine; graphviz
  = pure-transform engine with a bundled WASM asset.

# Plan 7 (root) — Native content-processor architecture for non-qmd engine inputs

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Date:** 2026-06-27 (reframed 2026-07-08 as the 7-series root)
**Status:** ARCHITECTURE ROOT — this file no longer holds an execution checklist. The percent/spin
work moved to **Plan 7b**; ipynb is **Plan 7c**; the withdrawn arbitrary-regex claim mechanism is
**Plan 7a (tombstone)**.

> **History.** This plan originally scoped "native percent/spin conversion + precise SourceInfo" with
> **two conversion paths** (native for built-ins, wire/Deno-side for TS engines via
> `percentScriptToMarkdown`). That split was the root of a Pass-1 performance bug: admitting a
> TS-engine percent/spin script into a project forced its Pass-1 `markdown_for_file` conversion over
> the wire, **launching Deno (or, for knitr spin, Rscript) in the indexing pass** — violating the
> grand plan's own "no engine load in Pass-1" principle (L82). The 2026-07-08 "spin-parse-rust"
> session reorganized this into a **7-series** around a single native, engine-agnostic
> **content-processor registry**. Full diagnosis + code evidence: Plan 7b § Context.

## The architecture (the contract every 7-x conforms to)

A **content processor** owns *sniff + convert + A+ SourceInfo* for one non-qmd input format
(`percent`, `spin`, later `ipynb`). It is **not an engine** — an engine (built-in or TS) merely
*names* the processor it wants on a `claims-files` entry. One `percent` processor therefore serves
jupyter, julia, and marimo alike; the registry holds no extension→language knowledge (each engine
supplies its own params).

Invariants:
- **Data, not launch.** Pass-1 discovery reads processor *names + params* (static data) and runs the
  processor's `sniff` **natively**; it never constructs an engine or spawns a subprocess. **Pass-1 is
  100% launch-free** — a file claimed only via a *dynamic* `claims_file` (no processor) is excluded
  from project discovery (still explicit-single-file renderable in Pass-2).
- **One conversion path.** `markdown_for_file` dispatches to the named processor natively (no engine
  object needed). The wire `markdownForFile`/`ClaimsFile` verbs survive only as the residual dynamic
  fallback.
- **One predicate, two sites.** The same `sniff` decides discovery admission and the claim stage.
- **A+ SourceInfo, uniform.** Every processor produces `Concat`/`Original` provenance back to the
  original bytes — no per-engine wire remap (Plan 7's old "A′ over the wire" is deleted).
- **Forward-compatible seams:** name-keyed registry; open `processor:` schema; the general Plan-0
  `SourceInfo` enum; a versioned sidecar envelope (`plain_text | jupyter_notebook`); a
  `ProcessorContext` on `convert` (so an asset-writing ipynb processor needs no trait change).

Registry lives at `crates/quarto-core/src/engine/content_processors/`, engine-agnostic.

## Sub-plans (the 7-series)

| Plan | Scope | Status |
|------|-------|--------|
| **7 (this file)** | Architecture root — the registry contract + invariants + forward-compat seams | root doc |
| [**7a**](2026-07-07-plan7a-static-content-pattern-claims.md) | *Tombstone.* Arbitrary static-regex content claims — **withdrawn**; surviving design points (discovery admission, one-predicate-two-sites coherence, built-ins-as-data, the Q6 membership-cache contract) migrated into 7b | superseded |
| [**7b**](2026-07-08-plan7b-native-content-processors.md) | **percent + spin** processors (native; `tree-sitter-r` for spin `matchable`), built-in + TS routing, zero Pass-1 launch, A+ SourceInfo, `.jl` validation flip | PLAN (this session) |
| [**7c**](2026-07-08-plan7c-ipynb-content-processor.md) | **ipynb** processor — `jupyterToMarkdown`, `SourceInfo::NotebookCell`, the `jupyter_notebook` sidecar arm; additive over 7b's seams | placeholder |

## References
- Finalized SourceInfo design: `2025-12-15-source-info-for-structured-formats.md`.
- Governing mandate: `claude-notes/designs/engine-api-surface.md` § "carry the whole Q1 engine surface".
- Complementary output-side provenance: `2026-06-18-qmd-per-line-provenance.md`.

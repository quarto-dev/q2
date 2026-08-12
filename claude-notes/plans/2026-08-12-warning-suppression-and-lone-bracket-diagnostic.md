# Warning suppression, and the lone-bracket diagnostic that motivates it (bd-lone-bracket-diagnostic-mxu41qbt)

**Date:** 2026-08-12
**Braid:** bd-lone-bracket-diagnostic-mxu41qbt
**Branch:** `braid/bd-lone-bracket-diagnostic-warning-suppression`, based on `main` @ `593f2785` (no worktree — investigation ran in the room-3 checkout)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design, with a sequencing recommendation the strand already anticipated: build the suppression mechanism first, then the diagnostic.** Two findings from the code sharpen the discussion rather than change its shape — (1) there is no suppression machinery of any kind today, but there *is* a directly reusable seam, because `--strict` (bd-yjs54ptg) already solved the mirror-image problem of applying a *severity policy* to every structured diagnostic in one place; and (2) the "someone might have written `[text]` on purpose" objection is weaker than the module docs claim, because q2 renders a lone bare span as `<span>text</span>` — semantically inert HTML.

## Pre-flight: HEAD state

`cargo xtask verify --skip-hub-build` at `593f2785`: **Rust legs green** (workspace build + `cargo nextest run --workspace` both passed). The hub-client test leg — which `--skip-hub-build` skips the *build* of but still *runs* — reported one failure:

```
mermaid/basic.qmd [html]: ensureFileRegexMatches: expected pattern
"<script src=\"[^\"]*mermaid\.min\.js\"></script>" to match
  src/services/smokeAll.wasm.test.ts  (1 failed | 130 passed)
```

This was the documented **stale-WASM trap**, not a HEAD regression. `hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm` was dated Aug 11 10:30, while the last commit touching `crates/quarto-core` / `crates/pampa` (`593f2785`) landed Aug 11 15:39 — so the WASM leg was running pre-`593f2785` Rust. Confirmed by rebuilding and re-running:

```
$ cd hub-client && npm run build:wasm && npx vitest run --config vitest.wasm.config.ts
 Test Files  22 passed (22)
      Tests  131 passed (131)
```

**HEAD is green.** Worth remembering for Phase 1: `--skip-hub-build` skips the WASM *build* but still *runs* the hub-client tests against whatever artifact is on disk, so a stale artifact reads as a test failure.

## Issue context

Filed 2026-08-12 by Claude (q2-connect-docs), type `feature`, priority 2, label `markdown`, status `open`. Explicitly opened as a **design discussion**, not a defect report.

A lone bare bracket group — `[Version TBD]`, `[1]`, `[Posit Connect]` — parses as the bracket half of span syntax, finds no attribute block, and renders as a bare `<span>` with the brackets discarded. Quarto 1 renders all of these literally. Nothing is reported. Q-2-45 and Q-2-46 cover the *other* two shapes of reference-link breakage (`[label][ref]` and `[ref]: url`); this third shape stays silent.

`crates/quarto-core/src/transforms/reference_link_diagnostics.rs` documents the gap as deliberate:

> A **lone** bare bracket group … is not diagnosed, even though its brackets are silently deleted too. There is no way to tell it apart from a deliberate `[text]` span, so warning on it would fire on legitimate documents.

The strand accepts that reasoning *given today's constraints* and questions one of the constraints: **a project that legitimately uses bare spans has no way to say so**, so "it would fire on legitimate documents" is currently unanswerable. Given suppression, the calculus flips — one opt-out in one place for the few projects that mean it, versus silently wrong output for every reader of every other project.

Real-world cost: two Posit Connect pages documented the *wrong* default mail subject prefix (`[Posit Connect]` → `Posit Connect`) for the duration of the port. The docs-side worklist has since been fixed by hand, so the corpus is clean — which is exactly the point: nothing stops the next author, and nothing will tell them.

## Dependency graph

**Empty.** `braid dep tree` shows the strand alone; `braid dep list` shows no edges in either direction. The origin strand (`br-e7mco18f`, related `br-raalju6n`) lives in the *q2-connect-docs* skein, not this one, so the `discovered-from` chain does not cross over.

No incoming pressure — nothing is blocked on this. That argues for doing it properly (suppression first) rather than shipping a narrow diagnostic under deadline.

Neighbors found by searching the skein rather than the graph (no edges exist yet; **worth linking** — see Work items):

- **bd-m2w7a** — *"Migrate remaining unstructured `DiagnosticMessage` warnings in transforms to builder API"* (open). Directly load-bearing: code-keyed suppression can only reach diagnostics that *have* codes. bd-m2w7a is the backfill for `theorem.rs`, `crossref_resolve.rs` (×2), `crossref_index.rs`, `attribution_render.rs`. See "The uncoded-warning gap" below.
- **bd-yjs54ptg** — `--strict` / warnings-as-errors, GH #220, **closed and shipped**. Plan: `claude-notes/plans/2026-07-02-strict-mode-warnings-as-errors.md`. This is the single most useful piece of prior art: it is the *same problem with the opposite sign*, and its design decisions should be reused rather than re-litigated.
- **k-lckc** — "Use quarto-error-reporting crate uniformly for render pipeline errors and warnings" (open). Same underlying theme: warnings that bypass the structured system are invisible to any policy layer.

## What the code looks like today

Everything the strand cites still exists at `593f2785` and has the shape described.

### The diagnostic gap is where the strand says it is

`crates/quarto-core/src/transforms/reference_link_diagnostics.rs` — `scan_inlines` (:131) fires only on two adjacency patterns:

- bare span at line start immediately followed by a `Str` starting with `:` → **Q-2-46** (definition line);
- reference label (bare span **or** empty-`src` image) immediately followed by a bare span → **Q-2-45** (reference use).

A lone bare span matches neither. `is_bare_span` (:82) — `Inline::Span` with `is_empty_attr(&span.attr)` — is already the exact predicate a third trigger would need. **The detection is a one-branch addition; the whole difficulty is policy, not detection.**

### A lone bare span renders to inert HTML — verified

`crates/pampa/src/writers/html.rs:996`:

```rust
Inline::Span(span) => {
    write!(ctx, "<span")?;
    write_attr(&span.attr, ctx)?;          // empty attr → writes nothing
    write_inline_source_attrs(inline, ctx)?;
    write!(ctx, ">")?;
    write_inlines(&span.content, ctx)?;
    write!(ctx, "</span>")?;
}
```

With an empty attr the output is `<span>text</span>` — no class, no id, no attributes. **A deliberately-authored lone `[text]` therefore accomplishes nothing** in the rendered document. That materially weakens the "it might be intentional" objection recorded in the module docs. The residual legitimate uses are narrow and worth naming explicitly in the discussion:

1. **A Lua filter that hooks bare spans.** Filters see the AST, so `[foo]` is a usable q2-specific marker for a filter to rewrite. This is a real pattern and the strongest counter-example.
2. **Work in progress** — an author who has typed `[text]` and has not yet typed `{.class}`.
3. **Generated/round-tripped content** where a bare span survives a pipeline.

(1) is the case a suppression mechanism has to serve well.

### There is no warning-suppression machinery of any kind

Confirmed by search across `crates/quarto-config/src`, `crates/quarto-core/src`, and `crates/quarto-error-catalog/src`: no per-code silencing, no allow-list, no severity override. Every hit for "suppress" is a *feature* being disabled (`toc: false`, `sidebar: false`), never a diagnostic.

`DiagnosticMessage` (`quarto-error-reporting` 0.2.1, `src/diagnostic.rs:224`) carries `pub code: Option<String>` and `kind: DiagnosticKind`. Severity is a plain field; the crate is deliberately policy-free (Decision D1).

### But the seam already exists — `--strict` built it

`claude-notes/plans/2026-07-02-strict-mode-warnings-as-errors.md` is the map. Its findings, re-verified at HEAD:

- **No emission chokepoint.** Warnings are pushed into `StageContext.diagnostics`, `RenderContext.diagnostics` (`render.rs:233`), pampa's `DiagnosticCollector`, ~15 transforms taking a bare `&mut Vec<DiagnosticMessage>`, and Lua `quarto.warn()` harvesting. Filtering at emission would be unsustainable.
- **Everything converges on `ProjectRenderSummary`** (`crates/quarto-core/src/project/orchestrator.rs:481`) through exactly four fields: `pass1_failures[].diagnostics`, `pass2_failures[].diagnostics`, `project_diagnostics`, and per-output `outputs[].render_output.diagnostics` (via the `OutputDiagnostics` trait, `:546`).
- **The policy hook is already written and shipped**: `ProjectRenderSummary::promote_warnings_to_errors()` (`orchestrator.rs:617`) walks all four sources post-run, pre-print. `OutputDiagnostics::diagnostics_mut()` exists specifically so policies can rewrite diagnostics in place. `should_exit_nonzero` (`render.rs:1118`) is the one exit gate.

**Suppression is the same shape with a filter instead of a mutation.** The recommended design below is deliberately parasitic on this.

### Two facts that complicate a naive design

**1. The uncoded-warning gap.** Code-keyed suppression can only reach diagnostics that carry a code. Measured at HEAD (AST-ish scan of `DiagnosticMessage::warning(` construction statements in non-`tests/` files):

| | count |
|---|---|
| `DiagnosticMessage::warning(` construction sites | 42 |
| …with no `.with_code(` in the same statement | 37 |

That 37 overcounts somewhat (it includes in-file `#[cfg(test)]` blocks and sites where the code is attached later through a helper), but the real figure is on the order of **25–30 user-visible warnings with no code at all** — including the two `crossref_resolve.rs` warnings that the strict-mode plan itself used as its canonical fixture. Those are exactly bd-m2w7a's worklist. Consequences:

- Full suppression coverage **depends on** bd-m2w7a (or a superset of it).
- A `warnings:` key that silently fails to suppress an uncoded warning is a bad user experience, so v1 needs a story: either backfill first, or make the failure legible.
- Going forward, an xtask lint ("every `DiagnosticMessage::warning`/`::error` must carry a code") would keep the gap from reopening — the same sustainability argument the strict-mode plan made.

**2. Not every user-visible diagnostic flows through the summary.** `crates/quarto/src/commands/render.rs:897-905` prints `underscore_typo_diagnostics`, `project_kind_diagnostics`, and `project.config.config_diagnostics` with a bare `eprintln!("{}", diagnostic.to_text(None))` — **before and outside** `print_render_diagnostics`. A summary-boundary filter would not see them. Either route them into `project_diagnostics` (arguably correct independent of this work) or declare them out of scope in v1. Separately, the strict-mode plan's count of ~552 `eprintln!` / ~63 `tracing::warn!` sites remains true and remains structurally out of reach.

**3. There is no config schema layer.** `render.rs:894` says it outright: *"Q2 has no schema layer; unknown keys are otherwise silently ignored."* Good news: a new `_quarto.yml` key costs no schema work. Bad news: a typo in a suppression list is silent unless we validate it ourselves — which makes the "unknown code" validation in the design below load-bearing rather than a nicety.

### Repro

The strand's repro exists at `/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/reference-links-unsupported/` (`_quarto.yml`, `index.qmd`). Not re-run during this investigation — the diagnostic gap was confirmed by reading `scan_inlines`, which cannot fire on a lone span. Re-running it is Phase 0 work.

## Recommended design (draft — for discussion, not settled)

### Part A — the suppression mechanism

**A1. Apply the policy at the summary boundary, exactly where `--strict` applies its own.**

Add `ProjectRenderSummary::apply_diagnostic_policy(&DiagnosticPolicy)` beside `promote_warnings_to_errors()`. It walks the same four sources through the same `OutputDiagnostics` trait. Every structured diagnostic added anywhere tomorrow participates with zero per-call-site work — the property the strict-mode plan called the sustainability requirement.

**A2. Resolve the policy per-document, apply it centrally.**

The wrinkle `--strict` did not have: suppression should be configurable in **document front matter** as well as `_quarto.yml`, and the summary boundary has no merged per-document metadata. Recommendation: resolve the policy *during* the document render (merged metadata is available after `MetadataMergeStage`, so project + document precedence comes free from the existing merge — including `!prefer`-style tags, with no new precedence machinery), stash the resolved policy on `RenderOutput`, and **apply** it at the boundary. That keeps one application seam, and keeps the diagnostic alive until policy runs, which is what makes `--show-suppressed` and unused-suppression reporting possible.

The alternative — filter inside the render pipeline as a final stage — splits the seam in two and forecloses those features. Recommend against.

**A3. Ordering: suppress, then promote.**

Under `--strict`, a suppressed warning must stay suppressed rather than become an error. Suppression is an author's statement about *what is a problem*; strict is a statement about *what to do with problems*.

**A4. Config shape.** Sketch, with the per-code map preferred:

```yaml
# _quarto.yml (or document front matter)
diagnostics:
  Q-2-49: off        # or: a reason string
```

with reasons encouraged:

```yaml
diagnostics:
  Q-2-49:
    level: off
    reason: "bare spans are hooks for our `annotate.lua` filter"
```

Why a **per-code severity map** rather than a flat `suppress: [Q-2-49]` list: it generalizes to `error` / `warning` / `info`, so it subsumes per-code strictness later without a second config key, and it matches the shape users already know from clippy / ESLint / tsconfig. Why **reasons**: the strand's own framing is that a project has "no way to say so" — a code list lets a project say *that* it dissents, a reason lets it say *why*, which is what makes the entry reviewable a year later. See open question 2.

**A5. Errors are not suppressible.** Recommend refusing to silence `DiagnosticKind::Error` and reporting the ignored entry. Suppressing an error means producing broken output silently, which is the failure mode this whole strand is about.

**A6. Validation and rot control** — the parts that keep a suppression list from becoming a graveyard:

- **Unknown code** → warn at config load (needs its own new Q-code). Load-bearing because there is no schema layer to catch the typo.
- **Unused suppression** → after a full-project render, a code that is suppressed but never fired gets an info diagnostic ("remove it"). Must be **project-render-only**: on a single-file render a project-wide suppression legitimately will not fire.
- **`--show-suppressed`** (or `--no-suppress`) → CLI escape hatch that reports everything, for audit and CI.

**A7. Scope: preview and hub.** `--strict` deliberately excluded `q2 preview` / hub-client (Decision D1: preview is lenient by design). Suppression probably should *not* be excluded — an author who has declared a construct legitimate should not be nagged in the editor. This is the one place where copying strict mode's scope decision is likely wrong. See open question 5.

### Part B — the lone-bracket diagnostic

Once A ships: add a third trigger to `reference_link_diagnostics.rs` for a lone `is_bare_span`, under a new code (**Q-2-49** — Q-2-48 is the current maximum in `error_catalog.json`), pointing at `\[`/`\]` escaping as the fix. Then rewrite the module docs, whose central claim ("no way to tell it apart from a deliberate span") is what A dissolves.

Downstream: `qmd-syntax-helper`'s `literal-brackets` rule (`crates/qmd-syntax-helper/src/conversions/literal_brackets.rs`) can become a diagnostic-code-keyed `q_2_49.rs` rule like its siblings, instead of the run-`check`-first special case it is today. It should probably **stay** opt-in for `convert -r all` regardless — the rule's own header explains why (an escape is a source edit that cannot afterwards be distinguished from author intent), and that reasoning is independent of the diagnostic.

Before shipping B, **measure the noise**: run the detection over a real corpus (`docs/`, the Connect docs, `crates/pampa` fixtures) and count how many lone bare spans exist in documents nobody considers broken. If the count is near zero, B is uncontroversial. If it is large, the count itself is the argument for how good A's ergonomics have to be.

A **preliminary text-level probe over `docs/`** is recorded in `lone-bracket-diagnostic-and-warning-suppression-investigation/noise-probe.md`: 8 regex candidates, **all 8 false positives** (mermaid node syntax and Python inside nested fences, one YAML flow sequence in front matter) — so the true count of AST-level lone bare spans in q2's own docs is **zero**. Encouraging but not decisive: `docs/` is written by people fluent in qmd span syntax. The corpora that matter are ones ported from Pandoc / Quarto 1, where reference-link habits survive, and the real measurement should go through `qmd-syntax-helper check -r literal-brackets` rather than a regex.

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD).** Re-run the strand's repro and capture current output. Failing tests for: policy resolution from merged metadata (project-only, doc-only, doc-overrides-project); `apply_diagnostic_policy` across all four summary sources; suppress-then-promote ordering under `--strict`; error-not-suppressible; unknown-code warning; unused-suppression info; `--show-suppressed`.
- **Phase 1 — Policy type + resolution.** `DiagnosticPolicy` in `quarto-core`, parsed from merged metadata. No application yet.
- **Phase 2 — Application at the summary boundary.** `apply_diagnostic_policy` beside `promote_warnings_to_errors`; `RenderOutput` carries the resolved policy; CLI wiring and ordering vs `--strict`.
- **Phase 3 — Validation + rot control.** Unknown-code warning (new Q-code), unused-suppression reporting, `--show-suppressed`.
- **Phase 4 — Coverage.** Decide the bd-m2w7a relationship; optionally an xtask lint requiring codes on new warnings.
- **Phase 5 — Preview / hub scope.** Whatever open question 5 decides.
- **Phase 6 — Noise measurement for the lone-bracket rule.**
- **Phase 7 — Q-2-49 diagnostic** + `docs/errors/markdown/Q-2-49.qmd` (the `error-docs-page-missing` lint requires the page in the same commit) + module-doc rewrite + `qmd-syntax-helper` rule re-keying.
- **Phase 8 — User docs.** Suppression belongs alongside the "Rendering in CI" section that strict mode added to `docs/guides/publishing/index.qmd`.

## Open design questions for the user

1. **Sequencing.** Confirm: suppression fully first (A, phases 1–5), then the diagnostic (B, phases 6–7)? Or interleave — ship a minimal `off`-only suppression and Q-2-49 together, deferring validation/rot-control to a follow-up?

2. **Config shape.** Per-code severity map (`diagnostics: {Q-2-49: off}`, generalizes to `error`/`warning`, clippy/ESLint-shaped) versus a flat list (`diagnostics: {suppress: [Q-2-49]}`, simpler)? And: should a **reason** be *encouraged*, *required*, or *unavailable*? Requiring one is unusual but makes every suppression self-documenting, which is precisely the "a project has no way to say so" gap the strand identifies.

3. **The uncoded-warning gap.** ~25–30 warnings carry no code and would be silently unsuppressible. Options: (a) make bd-m2w7a a hard prerequisite; (b) ship suppression first and let uncoded warnings be unsuppressible, tracked as follow-up; (c) ship a lint requiring codes so the gap stops growing while bd-m2w7a drains it. Recommendation: (c) plus (b), with bd-m2w7a linked as `related`.

4. **Granularity in v1.** Project-wide + per-document (free from the metadata merge) only? Or also per-path globs (`Q-2-49: {level: off, files: ["legacy/**"]}`)? Per-*line* suppression (a `<!-- quarto: allow Q-2-49 -->` comment) is the ergonomic ideal but needs comment-to-node attachment that does not exist — propose deferring, with a note that HTML comments now survive as `RawInline`, so it is reachable later.

5. **Preview / hub scope.** `--strict` excluded them deliberately. Should suppression apply in `q2 preview` and hub-client? Recommendation: **yes** — otherwise a project that has legitimately opted out still gets nagged in the editor, which is where authors actually live.

6. **The general rule the strand asks about.** Beyond this one diagnostic: is "q2 may warn on an ambiguous-but-usually-wrong construct, because suppression exists" now the standing policy? If yes, it belongs in `CLAUDE.md` or a design note, and it is worth auditing which *other* diagnostics were scoped down for the same reason so they can be revisited under the new constraint.

7. **Branch placement.** This investigation committed to a local `braid/…` branch off `main` rather than to `main` itself; there is no worktree. Confirm that is where the work should continue, or say where to move it.

## Risks / tradeoffs (draft)

- **Suppression is a load-bearing safety feature, not a convenience.** Once shipped, every future decision to *not* add a diagnostic loses its best excuse — which is the point, but it also means the mechanism has to be genuinely good (discoverable, validated, self-documenting, rot-resistant) or it becomes a way to silence real problems. A6 is not optional polish.
- **The uncoded-warning gap makes v1 partial by construction.** Whatever we ship, some warnings will not be suppressible. That must be visible to users rather than mysterious.
- **`--strict` interaction is subtle** and is where a bug would hide. Ordering (A3) needs an explicit test, not an inferred one.
- **The lone-bracket diagnostic's noise profile is unmeasured.** Phase 6 exists because the honest answer today is "we do not know how many legitimate bare spans exist in the wild."
- **The config-diagnostic `eprintln!` path (`render.rs:897`) bypasses the summary.** Fixing it is arguably right independent of this work, but it widens the diff.

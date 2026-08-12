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

## Work items

Settled after the decisions below. TDD throughout: tests written and observed failing before each implementation step.

### Phase 1 — `DiagnosticPolicy` type + resolution ✅

- [x] `crates/quarto-core/src/diagnostic_policy.rs`: `DiagnosticPolicy`, `PolicyEntry { level, reason, source }`, `PolicyLevel::Off`.
- [x] `DiagnosticPolicy::from_metadata(&ConfigValue) -> (Self, Vec<DiagnosticMessage>)` — parses the `diagnostics:` key in both short and long form; malformed entries produce a diagnostic rather than being ignored (there is no schema layer to catch them).
- [x] `DiagnosticPolicy::apply(&self, &mut Vec<DiagnosticMessage>)` — drops suppressed diagnostics. **Never drops `DiagnosticKind::Error`** (A5): silencing an error means shipping broken output silently, which is the exact failure this strand is about.
- [x] 12 unit tests: short form, long form, reason captured, unknown level rejected, non-map value rejected, empty policy is a no-op, errors survive suppression, uncoded diagnostics survive, one bad entry does not void its neighbours.
- [x] **Mutation-checked**: deleting the `DiagnosticKind::Error` guard makes `errors_are_never_suppressed` fail, confirming the safety test is not vacuous.

**One non-obvious case worth recording:** YAML 1.1 resolves a bare `off` to boolean `false`, so `parse_level` accepts `Scalar(Yaml::Boolean(false))` as well as the string `"off"`. Without that, the documented spelling would have been rejected as invalid. Level parsing also goes through `as_plain_text()` rather than `as_str()` — a bare YAML string in front-matter context is stored as `PandocInlines`, for which `as_str()` returns `None` (this is what the `metadata-as-str` xtask lint exists to catch).

### Phase 2 — Wire it into the single seam ✅

- [x] `StageContext.diagnostic_policy` field (defaults empty).
- [x] `MetadataMergeStage` resolves the policy from merged metadata after `activate_trace_from_metadata` and stores it on the context; malformed-entry diagnostics go into `ctx.diagnostics`.
- [x] `run_pipeline` applies the policy to `stage_ctx.diagnostics` immediately before returning.
- [x] 6 wiring tests in `pipeline.rs`, including a **baseline test** (`reference_link_warning_fires_without_suppression`) so a suppression test cannot pass merely because the warning never fired, and a preview-pipeline test covering decision 3.

### Phase 3 — Q-2-49, the lone-bracket diagnostic ✅

- [x] Catalog entries **Q-2-49** (`markdown`) and **Q-5-27** (`project`, "Invalid `diagnostics:` Entry") + `docs/errors/markdown/Q-2-49.qmd` and `docs/errors/project/Q-5-27.qmd` in the same commit (`cargo xtask lint` passes: 957 files).
- [x] Third trigger in `reference_link_diagnostics.rs`. `scan_inlines` now tracks a `claimed` bitmap so a span consumed by Q-2-45/Q-2-46 is **not** also reported as Q-2-49 — one mistake, one diagnostic.
- [x] Module docs rewritten: the old "no way to tell it apart from a deliberate span" section is replaced by an account of which premise changed and why.
- [x] 17 tests in the transform (13 pre-existing, 4 new), all passing.

**One pre-existing test changed, deliberately.** `does_not_treat_a_mid_line_colon_span_as_a_definition` (`Text before [label]: after`) asserted *no diagnostic at all*. That was never the claim it was named for — the claim is "this is not a *definition*" — and the silence it relied on was precisely the gap Q-2-49 closes, since `[label]` mid-sentence does lose its brackets. It now asserts `vec![CODE_LONE_BRACKETS]`. No other existing test changed; the full workspace suite passes at **11700 tests**.

### Phase 4 — End-to-end verification + docs ✅

- [x] User docs: a "Suppressing diagnostics" section in `docs/guides/publishing/index.qmd`, beside the `--strict` section. Rendered with Q2 (`cargo run --bin q2 -- render docs/…`) and the output inspected.
- [x] `cargo xtask lint` clean; full workspace tests green.

**End-to-end, real binary, output inspected** (`/tmp/q2repro`, a copy of the strand's repro):

```
$ q2 render
Warning: [Q-2-45] `[the RedHat documentation][gcc-toolset]` looks like a reference-style link …
Warning: [Q-2-45] `[noexec][noexec]` …
Warning: [Q-2-49] `[Version TBD]` has no attribute block, so it renders as an empty span and the
                 brackets are discarded — the reader sees `Version TBD`. Write `\[Version TBD\]` …
Warning: [Q-2-49] `[1]` …
Warning: [Q-2-49] `[2]` …
Warning: [Q-2-49] `[Posit Connect]` …
Warning: [Q-2-46] `[gcc-toolset]:` …
Warning: [Q-2-46] `[noexec]:` …
```

Section B — silent through 0.19.0 — now reports all four cases with precise spans, and Section A is unchanged (no double-reporting). Then, with `diagnostics: {Q-2-49: {level: off, reason: …}}` appended to `_quarto.yml`:

```
$ q2 render 2>&1 | grep -oE "\[Q-2-[0-9]+\]" | sort | uniq -c
   2 [Q-2-45]
   2 [Q-2-46]
```

Q-2-49 gone, the others untouched. With all three codes suppressed, `q2 render --strict` prints no diagnostics and **exits 0** — confirming suppress-then-promote ordering. Corrupting one entry to `Q-2-45: shout`:

```
   2 [Q-2-45]
   1 [Q-5-27]
```

— the malformed entry is reported once and suppresses nothing.

- [x] `cargo xtask verify` (full, all 14 steps including the WASM build and hub-client tests): **passed**.
- [ ] **Not done, stated plainly:** suppression has not been exercised in a live `q2 preview` browser session. The preview *pipeline* is covered by a unit test (`suppression_applies_in_the_preview_pipeline`, which drives `render_qmd_to_preview_ast` and asserts the code is gone), and the seam it relies on is the same one the CLI uses — but that is not the same as watching a browser. Anyone picking this up should run the `npm run build:wasm` → `cargo xtask build-q2-preview-spa` → `cargo build --bin q2` chain and confirm visually.

### Determinism note

`DiagnosticPolicy` stores entries in a `LinkedHashMap`, not a `HashMap`. They are lookup-only today, so either would be correct — but the deferred unused-suppression report (bd-91rgxmav) will iterate them to produce user-visible output, and author-config order is the order that report wants. Cheaper to get right now than to debug as nondeterministic output later; the repo's own guidance is "when in doubt, use `LinkedHashMap`."

### Merge with `main`: the invalid-entry code moved to Q-5-27

The branch originally used **Q-5-23** for the invalid-`diagnostics:`-entry
warning; it was free when the code was allocated. While the PR was open,
main's `aliases:` work claimed Q-5-23 through Q-5-26 — Q-5-23 is now
"Alias Would Overwrite a Rendered Page". Merging main surfaced this as an
add/add conflict on `docs/errors/project/Q-5-23.qmd`, which is the useful
kind of conflict: a silent textual merge would have left two different
diagnostics sharing one code and one docs URL.

Resolution: main keeps Q-5-23 (its page taken verbatim); this branch's
warning moved to **Q-5-27** (main's Q-5 maximum is 26), with its page at
`docs/errors/project/Q-5-27.qmd`. Q-2-49 was re-checked and is still
free, so the lone-bracket code is unchanged.

**Worth knowing for anyone allocating a code on a long-lived branch:**
the catalog is a shared numeric namespace with no reservation mechanism,
so "free when I looked" decays. Re-check immediately before merge. The
`error-docs-page-missing` lint catches a code with no page, but nothing
catches two branches picking the same number — the add/add conflict is
the only signal, and only because each side wrote a docs page.

### Follow-up strands filed

- **bd-91rgxmav** — warning-suppression v1 follow-ups (validation, rot control, `--show-suppressed`, globs, per-line, project-scoped coverage, the codes lint, additional levels).
- **bd-cljk1g5p** — re-key `qmd-syntax-helper`'s `literal-brackets` rule to the `q_2_49.rs` convention.

Both linked `discovered-from` this strand.

### Deferred to a follow-up strand

Unknown-code validation, unused-suppression reporting, `--show-suppressed`, per-path globs, per-line suppression, project-scoped-diagnostic coverage, and the xtask lint requiring codes on new warnings.

**Also deferred: re-keying `qmd-syntax-helper`'s `literal-brackets` rule to the `q_2_NN.rs` convention.** Now that Q-2-49 exists the rule *could* become `q_2_49.rs` like its siblings, which was one of the strand's stated motivations. It is deliberately not part of this change: the rename touches a user-visible CLI surface (`-r literal-brackets`, which appears in the `Q-2-46` docs page, the rule's own header, and the new `Q-2-49` page), and the sibling rules derive violations from parse errors while this one derives them from its own `bracket_analysis` — so it is a real refactor rather than a rename. The rule works as-is; `-r literal-brackets` remains the correct invocation everywhere it is documented.

The rule should also **stay opt-in** for `convert -r all` regardless of the re-keying, for the reason its own header gives: an escape is a source edit that cannot afterwards be distinguished from an author's intent.

## Decisions (Carlos, 2026-08-12)

1. **Config shape:** per-code map, reason *encouraged* (short form `Q-2-49: off`, long form `{level:, reason:}`). Chosen over a flat `suppress:` list so per-code severity (`error`, `warning`) is reachable later without a second key.
2. **Sequencing:** minimal suppression (`off` only) **plus** Q-2-49 ship together. Unknown-code validation, unused-suppression reporting, and `--show-suppressed` are deferred to a follow-up strand.
3. **Scope:** suppression applies **everywhere**, including `q2 preview` and hub-client — deliberately diverging from `--strict`'s Decision-D1 exclusion, because an author who has declared a construct legitimate should not be nagged in the editor.
4. **Uncoded warnings:** ship anyway; the ~25–30 uncoded warnings are simply unsuppressible in v1, documented as such, with bd-m2w7a linked as `related`. No xtask lint in v1.

### What decision 3 changes about the design

The summary-boundary seam (A1) is **CLI-only** — `q2 preview` and hub-client never build a `ProjectRenderSummary`. Applying the policy there would have excluded exactly the surface decision 3 requires.

The seam that satisfies decision 3 is **`run_pipeline`** (`crates/quarto-core/src/pipeline.rs:717`), whose single tail expression

```rust
.map(|d| (d, stage_ctx.diagnostics))
```

(:811) is the one place every per-document diagnostic passes through, for *every* frontend: `render_qmd_to_html` (:920), `parse_qmd_to_ast`, and `render_qmd_to_preview_ast` (:998) all funnel through it. Filtering there covers CLI single-doc, CLI project (per-page), preview, and WASM in one edit — and, because it happens inside the render, it lands strictly *before* `--strict`'s promotion at the CLI boundary, so the suppress-then-promote ordering of A3 falls out for free rather than needing to be enforced.

Resolution and application are split:

- **Resolve** in `MetadataMergeStage`, right after `activate_trace_from_metadata` (`metadata_merge.rs:420`) — the point where merged metadata exists. Project → directory → document precedence comes free from the existing merge, so `_quarto.yml` and front matter both work with no new precedence machinery.
- **Apply** in `run_pipeline`'s tail, via a new `StageContext.diagnostic_policy` field.

**Known v1 gap, accepted:** *project-scoped* diagnostics (`project_diagnostics` in `ProjectRenderSummary`, plus the `eprintln!` config-diagnostic path at `render.rs:897`) do not pass through `run_pipeline` and are therefore not suppressible in v1. Per-document diagnostics — which is what Q-2-49 is — are fully covered, including when the suppression is written in `_quarto.yml`, because project config is merge layer 1.

## Open design questions for the user

**Questions 1–3 and 5 are answered above.** Questions 4, 6, and 7 remain open; 4 is assumed as stated (project + document only, globs and per-line deferred) unless Carlos says otherwise.

1. ~~**Sequencing.**~~ *Answered: decision 2.*

2. ~~**Config shape.**~~ *Answered: decision 1.*

3. ~~**The uncoded-warning gap.**~~ *Answered: decision 4.*

5. ~~**Preview / hub scope.**~~ *Answered: decision 3.*

<details><summary>Original wording of the answered questions</summary>

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

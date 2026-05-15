# Plumb doctemplate diagnostics through `quarto render`

**Issue:** bd-xdnk
**Status:** Implementation complete — awaiting user review for commit/push

## Discovered: pre-existing `template:` YAML bug

Phase 3 end-to-end testing uncovered a separate bug that this fix
also resolves. `apply_template.rs:170` looked up the `template:`
metadata key with `as_str()`. Real qmd front-matter parses
`template: custom.html` as `ConfigValueKind::PandocInlines` (the
parser treats unquoted scalar metadata as inline content), and
`as_str()` only matches `String` / `Path`. As a result, the
`template:` YAML key was silently ignored under `quarto render`
— the renderer always fell back to a built-in template even when
the document specified a custom one. The fix uses
`as_plain_text()`, which extracts text from inlines as well. Same
for `template-partials` array entries. Regression test:
`apply_template::tests::test_custom_template_path_from_pandoc_inlines`.

## Overview

The doctemplate engine (`quarto-doctemplate`) attaches accurate source
locations to every variable reference, conditional, partial, and for-loop
body it parses. When evaluation encounters an undefined variable, an
unresolved partial, or any other recoverable problem, it emits a
`DiagnosticMessage` with that location. `pampa` already surfaces those
diagnostics through ariadne (yellow `Warning: [Q-10-2] …` with a caret
under the offending `$var$` and an OSC 8 hyperlink to the template file).

The `quarto render` orchestrator does not. Its template stage calls the
diagnostic-discarding `template.render(&ctx)` API and throws the
warnings away. This plan plumbs the diagnostics from the doctemplate
evaluator out through the stage pipeline so they reach the renderer's
existing `print_render_diagnostics` sink.

### Two call sites at issue

```
crates/quarto-core/src/template.rs:412
    template.render(&ctx)            ← drops diagnostics

crates/pampa/src/template/render.rs:184
    template.render_with_diagnostics(&template_ctx)   ← keeps them
```

The fix is to make the `quarto-core` site behave like the `pampa` site
and ferry the diagnostics through the stage pipeline.

### Pipeline layout

```
ApplyTemplateStage (crates/quarto-core/src/stage/stages/apply_template.rs)
  └─ quarto_core::template::render_with_compiled_template
        └─ Template::render(&ctx)            ← change to render_with_diagnostics

orchestrator (crates/quarto-core/src/project/orchestrator.rs)
  └─ produces RenderOutput { diagnostics, source_context, … }

quarto render command (crates/quarto/src/commands/render.rs:674)
  └─ for diag in render_output.diagnostics:
        eprintln!("{}", diag.to_text(Some(&render_output.source_context)));
```

The CLI already prints `RenderOutput::diagnostics` with ariadne. The
gap is everything between the doctemplate evaluator and that vector.

## Test plan (TDD — write these first)

We follow the project's "test fails first, then implement" rule.

- [ ] **End-to-end test in `crates/quarto/tests/`** — drive the real
      `quarto render` binary (or its in-process equivalent that the
      other render tests use) on a fixture project containing:
        - `post.qmd` with a `template: post.html` YAML key
        - `post.html` referencing `$author-greeting$` (undefined)
      Assert that:
        - the render succeeds (warnings ≠ errors);
        - the resulting `RenderOutput.diagnostics` (or captured stderr,
          depending on what the harness exposes) contains a
          `Q-10-2` `DiagnosticKind::Warning`;
        - the diagnostic's primary location resolves to `post.html`
          at the byte range of `$author-greeting$`.
- [ ] **Stage-level unit test in
      `crates/quarto-core/src/stage/stages/apply_template.rs`** — feed
      a synthetic `RenderedDocument` and metadata through
      `ApplyTemplateStage` with a custom template that has an
      undefined variable, and assert that the stage produces at least
      one `DiagnosticMessage` with code `Q-10-2`. Today there is no
      hook for this because the stage discards them.
- [ ] **Library-level unit test in `crates/quarto-core/src/template.rs`**
      — call `render_with_compiled_template` directly with an undefined
      variable in the template and assert the returned
      `(String, Vec<DiagnosticMessage>)` carries the warning. (This
      requires the API change in Phase 1 below.)
- [ ] **Existing snapshot/regression sweep** — `cargo nextest run
      --workspace` and `cargo xtask verify` to confirm we did not
      change the rendered HTML for documents whose templates have no
      undefined variables.

The end-to-end test is the load-bearing one per
`claude-notes/plans/2026-04-20-end-to-end-verification-process.md` —
unit tests alone are not enough.

## Implementation phases

### Phase 1: change the library API in `quarto-core`

Currently:

```rust
// crates/quarto-core/src/template.rs:331
pub fn render_with_compiled_template(
    template: &Template,
    body: &str,
    meta: &ConfigValue,
    css_paths: &[String],
    script_paths: &[String],
) -> Result<String> { … template.render(&ctx) … }
```

Change return type to `Result<(String, Vec<DiagnosticMessage>)>` and
call `template.render_with_diagnostics(&ctx)`. Update the two
internal callers in the same file (`compile_builtin_template_with_partials`
and `select_template` wrappers — lines 463/478) to thread the
diagnostics through. Test helpers (`tests` module from line 1900+)
need their `.unwrap()` adjusted to `(html, _)` destructuring.

### Phase 2: thread diagnostics through `ApplyTemplateStage`

`crates/quarto-core/src/stage/stages/apply_template.rs:235/255/269`
currently does:

```rust
let html = template::render_with_compiled_template(...).map_err(...)?;
```

Change each of the three branches to capture
`(html, template_diags)` and merge `template_diags` into the
stage's diagnostic stream. Need to confirm the stage's mechanism for
emitting diagnostics — investigation item below.

- [ ] **Investigate** how other stages report diagnostics
      (`StageContext` API? `PipelineData` variant? a `diagnostics`
      vec inside `RenderedDocument`?). Pattern-match on whatever
      `MetadataMergeStage` does for its merge warnings.

### Phase 3: surface diagnostics in `RenderOutput`

Confirm the diagnostic flow from `ApplyTemplateStage` ↔ orchestrator
↔ `RenderOutput.diagnostics`. The orchestrator already aggregates a
`Vec<DiagnosticMessage>` per file (the renderer prints them at
`crates/quarto/src/commands/render.rs:674`), so this is mostly about
making sure the stage's diagnostics land in that bucket and that the
file IDs in their `SourceInfo` resolve against the
`RenderOutput.source_context`.

- [ ] **Investigate** whether template-file source IDs are registered
      in the document's `SourceContext` along the orchestrator path
      the same way they are in pampa's `ASTContext::source_context`.
      The pampa render path (`crates/pampa/src/template/render.rs:147`)
      compiles the template into the same source context the AST uses;
      if `ApplyTemplateStage` does not, the ariadne renderer will
      have nothing to slice from and we will get
      "<unknown source>" output instead of a caret.

### Phase 4: end-to-end verification

Per `CLAUDE.md` "End-to-end verification before declaring success":

- [ ] Run `cargo run --bin q2 -- render <fixture>/post.qmd` (or
      whatever the canonical CLI invocation is) and capture stderr.
- [ ] Confirm the captured stderr contains the ariadne-rendered
      warning identical in shape to the `pampa` output we already saw
      in this session.
- [ ] Paste the captured invocation + stderr snippet into this plan
      document under a "Verification" section before closing bd-xdnk.

## Resolved questions

1. **Surface scope.** All template diagnostics (Q-10-2, Q-10-5, any
   future codes). The API change is one-shot.
2. **Strict mode.** Out of scope here. Likely better handled as a
   global "warnings are errors" flag in a follow-up, not template-
   specific.
3. **Built-in templates.** Plumbing diagnostics may produce noise on
   built-in templates whose `$var$` references aren't guarded by
   `$if(...)$`. Tweaking those templates is the right tradeoff and is
   in scope. Before Phase 1, grep the built-in template set
   (`crates/pampa/src/template/builtin*` and
   `crates/quarto-core/src/template.rs::select_template`) for
   unguarded references and either guard them or document them as
   expected to be defined. Track each tweak in this plan's work-items.
4. **Hub-client.** In scope. `wasm-quarto-hub-client` already routes
   structured diagnostics through `JsonDiagnostic` /
   `diagnostics_to_json` (`crates/wasm-quarto-hub-client/src/lib.rs`,
   ~lines 600/756/952/1102/1225). Once template diagnostics land in
   `RenderOutput.diagnostics`, they ride those existing rails to
   Monaco markers and the in-app diagnostics panel — no shim needed,
   just the API update plus a hub-client smoke test.

## Out of scope

- Changing the doctemplate evaluator itself.
- New diagnostic codes.
- The pampa rendering path (already correct).
- Fancier formatting / grouping of warnings — we use whatever
  `print_render_diagnostics` already does.
- Strict-mode opt-in (see #2).
- The `$variable?$` short-circuit syntax (see Follow-up below).

## Follow-up: `$variable?$` short-circuit syntax

The user raised this as a feature worth studying. Goal: make

```
$variable?$
```

equivalent to

```
$if(variable)$$variable$$endif$
```

so users can suppress "undefined variable" diagnostics inline without
the verbosity of an `$if$` guard. This is **not** in scope for
bd-xdnk; opening a separate beads issue if/when we want to pursue it.

### Feasibility notes (for the followup issue)

The doctemplate grammar already uses `/` as the pipe separator for
post-fix transformations:

```
$variable/uppercase$
$variable/left 20 "| "$
```

(see `crates/tree-sitter-doctemplate/grammar/grammar.js`, the `pipe`
rule and the `/` repetitions on lines ~84–86).

`?` is currently unused as a sigil and would not collide with any
existing pipe name (`pairs`, `first`, `last`, `rest`, `allbutlast`,
`uppercase`, `lowercase`, `length`, `reverse`, `chomp`, `nowrap`,
`alpha`, `roman`, `left`, `center`, `right`). Two plausible spellings:

- **`$variable?$`** — terminal `?` immediately before the closing
  `$`. Grammar-wise: add an optional `"?"` token after
  `variable_name` (and pipe chain) in the same production as the
  literal-separator `[…]` clause. Eval-wise: in `render_variable`,
  if the optional flag is set and `resolve_variable` returns `None`,
  skip the `warn_or_error_with_code` call and return `Doc::Empty`
  silently.
- **`$variable/?$`** — treat `?` as a degenerate pipe in the existing
  `repeat(seq("/", $.pipe))` chain. Slightly more uniform with the
  rest of the syntax, but `?` is not really a pipe (it changes
  resolution semantics, not value transformation).

The first spelling is closer to user expectation ("optional variable")
and keeps pipes purely value-to-value. Implementation cost is
roughly: one grammar token, one bool field on `VariableRef`, one
branch in `render_variable`. Behavioral parity with
`$if(var)$$var$$endif$` is exact for scalar values; for lists/maps
the current `$if$` truthy semantics already match `is_truthy()`, so
no surprise there.

Open question for the followup: should `?` propagate to applied
partials (`$var?:partial()$`) and conditionals? Probably yes for
applied partials (same "skip silently if missing" intent); not
applicable to `$if$` itself.

This study is enough to motivate the followup but does not commit us
to syntax — the followup issue should reconfirm before
implementation.

## Work items

### Phase 0: pre-flight

- [x] Grep built-in templates for unguarded `$var$` references.
- [x] Confirm whether `ApplyTemplateStage` registers the template
      file in the document's `SourceContext`.
- [x] Trace the `SourceContext` flow from `DocumentAst` →
      `RenderedOutput` → `RenderOutput` and identify the gap.

#### Findings

**Built-in templates (`quarto-core/src/template.rs`)**: clean. Both
`MINIMAL_HTML_TEMPLATE` and `FULL_HTML_TEMPLATE` guard every `$var$`
with `$if(...)$` except for variables that are always populated by
`render_with_compiled_template`:
- `body` (always inserted, line 339)
- `version` (always inserted, line 393)
- `page-layout` (default-set if missing, line 397)

`pampa`'s built-in `main.html` has unguarded `$lang$` and
`$pagetitle$` (lines 2 / 19), plus unguarded `$idprefix$` and
`$abstract-title$` inside outer guards. These are only used by the
`pampa` CLI, which already calls `render_with_diagnostics`. Out of
scope for this fix.

**SourceContext registration**: `ApplyTemplateStage` calls
`Template::compile_with_resolver` (parser.rs:253), which creates a
fresh internal `SourceContext` for the template (parser.rs:259).
File IDs from that context never reach `RenderOutput.source_context`,
so ariadne would have nothing to slice from.

**SourceContext flow gap**: `pipeline.rs:660-668`
(`render_qmd_to_html`) builds `RenderOutput.source_context` fresh,
registering only the input file by name. The document's
`DocumentAst.source_context` (which `IncludeExpansionStage`
populates with included files) is dropped when `RenderHtmlStage`
converts `DocumentAst` → `RenderedOutput` (render_html.rs:108-119:
`RenderedOutput` has no `source_context` field). Existing include
warnings happen to render because their `SourceInfo` references the
input file's `FileId 0`, which `pipeline.rs:660` re-registers in the
same slot. Template file IDs would be slot 1+ in the doctemplate's
internal context and would not exist in the rebuilt context.

**Diagnostic emission pattern**: stages push to `ctx.diagnostics`
(`StageContext.diagnostics`, context.rs:84). The pipeline returns
this vector unchanged to `RenderOutput.diagnostics`
(pipeline.rs:556, 666). This is the right channel for template
diagnostics — no new mechanism needed.

#### Design decision: surgical SourceContext plumbing

Rather than introduce `StageContext.source_context` as global state
(a broader cross-cutting refactor), thread the SourceContext through
the existing `DocumentAst → RenderedOutput → RenderOutput`
boundary that currently drops it:

1. Add `source_context: SourceContext` field to `RenderedOutput`
   (data.rs:388).
2. `RenderHtmlStage` copies `doc.source_context` into
   `RenderedOutput.source_context` (render_html.rs:108).
3. `ApplyTemplateStage` calls
   `Template::compile_with_resolver_and_context(..., &mut
   rendered.source_context)` so template file IDs land in the
   document's context.
4. `pipeline.rs::render_qmd_to_html` reads
   `rendered.source_context` into the final
   `RenderOutput.source_context` instead of rebuilding fresh.

This fixes the cross-file diagnostic gap for both templates and
includes (existing include warnings already render correctly only
by accident of FileId-0 collision), without imposing a new
`StageContext` field on every stage. A future cleanup could
consider `StageContext.source_context` if more stages emit
diagnostics referencing non-input files; tracked as a follow-up,
not blocking this fix.

### Phase 1: failing tests

- [x] Library-level test
      `quarto_core::template::tests::test_undefined_variable_emits_diagnostic`:
      asserts the new `(String, Vec<DiagnosticMessage>)` return shape
      and that a `Q-10-2` warning is in the vec.
      *Failure signal:* fails to compile; current API returns
      `Result<String>`. ✅
- [x] Stage-level test
      `apply_template::tests::test_custom_template_undefined_variable_emits_diagnostic`:
      runs `ApplyTemplateStage` with a custom template containing
      `$author-greeting$`; asserts `ctx.diagnostics` carries a
      `Q-10-2` warning whose `SourceInfo` resolves through
      `RenderedOutput.source_context` to the template file.
      *Failure signal:* fails to compile; `RenderedOutput` has
      no `source_context` field. ✅
- [x] End-to-end CLI test in
      `crates/quarto/tests/render_cli_e2e.rs::custom_template_undefined_variable_emits_warning_on_stderr`:
      spawns the real `q2 render` binary on a project containing
      a `post.qmd` with `template: custom.html` and an undefined
      `$author-greeting$`. Asserts: zero exit, output HTML
      created with body content, and stderr contains `Q-10-2`,
      `Undefined variable`, `author-greeting`, and `custom.html`.
      *Failure signal:* compiles; will fail at runtime because
      production code drops the diagnostic before stderr.
- [~] Hub-client smoke test: **scope adjusted.**
      `wasm-quarto-hub-client` has no Rust-side `tests/` dir and
      its WASM-bound `render_qmd_content` requires a JS test
      harness to exercise. The crate already routes
      `RenderOutput.diagnostics` through `diagnostics_to_json`
      into the JSON `warnings` field (lib.rs:1102, 1225) — that
      path is already exercised by Q-5-x include warnings. As long
      as the doctemplate diagnostic lands in
      `RenderOutput.diagnostics` (covered by the library and
      e2e tests above), it will ride the same rails to the
      hub-client UI. A dedicated WASM smoke test is recorded as
      a follow-up rather than a blocker for bd-xdnk.

### Phase 2: implementation

- [x] Add `source_context: SourceContext` field to `RenderedOutput`.
- [x] `RenderHtmlStage`: copy `doc.source_context` into the new
      field on construction.
- [x] Update all `RenderedOutput { … }` literal sites (test
      fixtures in `apply_template.rs`) to set the new field.
- [x] Change `render_with_compiled_template` to return
      `Result<(String, Vec<DiagnosticMessage>)>` and call
      `template.render_with_diagnostics(&ctx)`. Updated both
      internal wrappers (`render_with_resources`, `render_with_format`).
- [x] Updated `render_with_template` similarly.
- [x] `ApplyTemplateStage`: call
      `Template::compile_with_resolver_and_context` for both
      no-partial and with-partial branches, threading
      `&mut rendered.source_context`. Updated
      `compile_builtin_template_with_partials` to take
      `&mut SourceContext`. Captured `(html, template_diags)` and
      extended `ctx.diagnostics`.
- [x] `pipeline.rs::render_qmd_to_html`: forwarded
      `rendered.source_context` directly into
      `RenderOutput.source_context` (no fallback needed —
      `ParseDocumentStage` always registers the input file).
- [x] Fixed pre-existing `template:` YAML lookup bug
      (`as_str()` → `as_plain_text()`).

### Phase 3: callsite + downstream updates

- [x] Updated `crates/quarto-core/tests/navigation_e2e.rs`
      (two `render_with_compiled_template().unwrap()` sites).
- [x] Updated `crates/quarto/tests/render_integration.rs`
      (`render_with_resources` site).
- [x] Updated all in-module test callers in
      `crates/quarto-core/src/template.rs` (`render_with_template`,
      `render_with_resources`, `render_with_format`,
      `render_with_compiled_template`).
- [x] No `wasm-quarto-hub-client` callers needed updating — the
      crate consumes `RenderOutput.diagnostics` and
      `RenderOutput.source_context` via the existing
      `diagnostics_to_json` rails. Template warnings now flow
      through automatically.
- [x] No built-in-template tweaks needed (Phase 0 found them clean).
- [x] Added regression test
      `test_custom_template_path_from_pandoc_inlines` for the
      discovered `as_str` → `as_plain_text` fix.

### Phase 4: verification

- [x] Three new tests pass
      (`test_undefined_variable_emits_diagnostic`,
      `test_custom_template_undefined_variable_emits_diagnostic`,
      `test_custom_template_path_from_pandoc_inlines`,
      `custom_template_undefined_variable_emits_warning_on_stderr`).
- [x] `cargo nextest run --workspace`: 8407 tests passed,
      0 failed, 195 skipped. No regressions.
- [x] Full `cargo xtask verify` (Rust + hub-client +
      trace-viewer): all steps passed.
- [x] Real `q2 render` invocation captured below.
- [~] Hub-client browser smoke-test: deferred. The structured
      diagnostic flows through `RenderOutput.diagnostics` /
      `diagnostics_to_json` / `JsonDiagnostic.warnings` (existing
      rails), so no new code-path on the WASM side. A live UI
      smoke would still be valuable; recorded as follow-up.
- [ ] `hub-client/changelog.md` update — deferred until first
      commit lands (need its hash).
- [x] `$variable?$` follow-up beads issue: bd-x5r4
      (linked via `discovered-from`).
- [x] Hub-client UI smoke follow-up: bd-khuj
      (linked via `discovered-from`).

#### Verification: real CLI invocation

```
$ cd /tmp/xdnk-render-test
$ ls
_quarto.yml  custom.html  post.qmd
$ cat post.qmd
---
title: Source-tracked template diagnostics
template: custom.html
---

Body content.
$ cat custom.html
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>$title$</title></head>
<body>
<header>by $author-greeting$</header>
<main>$body$</main>
</body>
</html>
$ q2 render post.qmd
Warning: [Q-10-2] Undefined variable: author-greeting
   ╭─[ /private/tmp/xdnk-render-test/custom.html:5:12 ]
   │
 5 │ <header>by $author-greeting$</header>
   │            ────────┬────────
   │                    ╰────────── Undefined variable: author-greeting
───╯
$ echo "exit=$?"
exit=0
$ head -3 post.html
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Source-tracked template diagnostics</title></head>
```

Confirmed:
- Custom `template:` YAML key resolves the chosen template (was
  silently ignored before this fix — the `as_str()` /
  `PandocInlines` mismatch).
- Doctemplate evaluator emits the `Q-10-2` warning with accurate
  source location (line 5, column 12 — the `$author-greeting$`
  position in `custom.html`).
- Ariadne renders a caret under the offending variable, with an
  OSC 8 hyperlink (the `]8;;file://...` markup wraps the path).
- Render exits zero (warning, not error) and `post.html` is
  produced with the body content rendered through the custom
  template.

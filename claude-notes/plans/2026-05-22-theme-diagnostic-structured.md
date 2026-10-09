---
title: 'Structured diagnostic for theme-config errors'
date: 2026-05-22
description: 'Turns a theme configuration error into a structured diagnostic with a source span pointing into `_quarto.yml`, instead of a plain error line that loses its location.'
status: draft  # drafting — pending user review
braid:
  strand: bd-pgczr
  priority: P2
  labels: [diagnostics, sass, theme]
---

**Parent:** [theme-diagnostic epic](2026-05-22-theme-diagnostic-epic.md)

## Goal

Make the theme-config error reach the CLI as a `DiagnosticMessage` with
an ariadne-rendered source span, the way `[Q-2-39]` and friends do
today. Stop emitting it as a plain `error: <path>: <msg>` line.

This issue does *not* address the cross-page duplication; it just makes
each emission well-formed. Coalescing is the sibling issue.

## Current path (problem)

```
ConfigValue { theme: <map> }    ← in merged metadata
        │
        ▼  crates/quarto-sass/src/config.rs:440  extract_theme_specs()
SassError::InvalidThemeConfig { message: "theme must be …" }
        │  (source_info discarded by .to_string() at next step)
        ▼  crates/quarto-core/src/stage/stages/compile_theme_css.rs:272-273
PipelineError::stage_error("compile_theme_css", "…")
        │
        ▼  crates/quarto-core/src/project/orchestrator.rs:337  file_failure_from_error
FileFailure { error: "<plain string>", diagnostics: vec![], source_context: None }
        │   (the diagnostics/source_context arms only fire for QuartoError::Parse)
        ▼  crates/quarto/src/commands/render.rs:716
eprintln!("error: {}: {}", path, error_string)
```

The two places that throw away structured information are:

- `crates/quarto-sass/src/config.rs:454,463` — `SassError::InvalidThemeConfig`
  is constructed with just a `String`; the `ConfigValue::source_info` we
  could attach is right there in scope (`value.source_info`).
- `crates/quarto-core/src/project/orchestrator.rs:337-348` —
  `file_failure_from_error` only extracts diagnostics from
  `QuartoError::Parse`. Sass errors get the empty-diagnostics fallback.

## Target path

```
ConfigValue { theme: <map>, source_info: SI }
        │
        ▼  extract_theme_specs() — pass the value's source_info through
SassError::InvalidThemeConfig { message, location: Some(SI) }
        │
        ▼  CompileThemeCssStage::run — convert to a structured QuartoError
QuartoError::ConfigError(ConfigDiagnostic {
    diagnostic: DiagnosticMessage {
        code: Some("Q-14-1"),     // see "code allocation" below
        kind: Error,
        title: "Invalid theme configuration",
        location: Some(SI),
        problem: Some(MessageContent::from(msg)),
        …
    },
    source_context: SourceContext,
})
        │
        ▼  file_failure_from_error — add an arm for ConfigError
FileFailure { error: <unchanged display>, diagnostics: vec![diag], source_context: Some(ctx) }
        │
        ▼  print_render_diagnostics in render.rs
diag.to_text(Some(&source_context)) is printed as ariadne report
```

## Test plan (TDD — write first)

Per CLAUDE.md the test goes before the implementation. Order:

1. **Unit test in `quarto-sass`.** Construct a `ConfigValue` whose
   `theme` key holds a `Mapping` (not string / not array). Call
   `extract_theme_specs`. Assert the returned `SassError` carries the
   value's `source_info` and the expected message. This will *fail*
   today because the variant doesn't have a `location` field.

2. **Unit test in `quarto-core`** for `file_failure_from_error`. Feed
   it a `QuartoError::ConfigError(...)` and assert the resulting
   `FileFailure` has the diagnostic copied across.

3. **End-to-end test** (drives the CLI surface): point
   `render_document_to_file` at a fixture project whose
   `_quarto.yml` has the offending `theme: {light: […], dark: […]}`
   block, render, and assert that the resulting failure's
   `diagnostics[0]` is the structured one with the right code and
   location. The fixture can be a stripped 1-page project.

4. **Render-the-binary check** (per CLAUDE.md end-to-end policy). Run
   `cargo run --bin q2 -- render external-sources/quarto-web 2>&1 | head -60`
   manually and confirm: each theme error is rendered with ariadne,
   code header, and a source-span pointer into `_quarto.yml`. Record
   the snippet in this plan when done.

## Implementation outline (after tests are red)

### Step 1: Extend `SassError::InvalidThemeConfig` to carry a location

`crates/quarto-sass/src/error.rs:35-36`:

```rust
#[error("Invalid theme configuration: {message}")]
InvalidThemeConfig {
    message: String,
    location: Option<SourceInfo>,   // NEW
},
```

Update the call sites at `config.rs:454` and `config.rs:463` to pass
`Some(value.source_info.clone())`. (Other internal call sites that
don't have a `ConfigValue` in scope — e.g. `:206`, `:355`,
brand-layer wrappers — pass `None` for now; they're out of scope.)

### Step 2: Add a `ConfigError` variant to `QuartoError`

`crates/quarto-core/src/error.rs` (verify exact path during impl):

```rust
pub enum QuartoError {
    …existing variants…,
    Config(ConfigDiagnostic),
}

pub struct ConfigDiagnostic {
    pub diagnostic: DiagnosticMessage,
    pub source_context: SourceContext,
}
```

The `Display` impl can produce the legacy plain-text form so call
sites that still do `e.to_string()` continue to look reasonable in
logs.

### Step 3: Build the diagnostic in `CompileThemeCssStage`

`crates/quarto-core/src/stage/stages/compile_theme_css.rs:272-273`:

```rust
let theme_config = match ThemeConfig::from_config_value(&doc.ast.meta) {
    Ok(c) => c,
    Err(SassError::InvalidThemeConfig { message, location }) => {
        let diag = DiagnosticMessageBuilder::error("Invalid theme configuration")
            .with_code("Q-14-1")
            .with_location_opt(location)
            .problem(message)
            .build();
        return Err(QuartoError::Config(ConfigDiagnostic {
            diagnostic: diag,
            source_context: ctx.source_context.clone(),  // confirm this field exists
        })
        .into());
    }
    Err(other) => return Err(PipelineError::stage_error(self.name(), other.to_string())),
};
```

(`with_location_opt` may need to be added; trivial helper.)

Caveat: `StageContext` needs to expose a `SourceContext` for the
project's `_quarto.yml`. If it doesn't already, this step grows; verify
during implementation. The parse-error path
(`crates/pampa/src/qmd.rs:100-103`) already populates one — confirm we
can reuse the same mechanism here.

### Step 4: Extend `file_failure_from_error`

`crates/quarto-core/src/project/orchestrator.rs:337-348`:

```rust
let (diagnostics, source_context) = match &e {
    QuartoError::Parse(pe)   => (pe.diagnostics.clone(),     Some(pe.source_context.clone())),
    QuartoError::Config(cd)  => (vec![cd.diagnostic.clone()], Some(cd.source_context.clone())),
    _ => (Vec::new(), None),
};
```

### Step 5: Render the structured failure at the CLI

`crates/quarto/src/commands/render.rs:715-717`. Today it prints the
plain string. Change the loop to:

- if `failure.diagnostics` is non-empty, render each with
  `to_text(failure.source_context.as_ref())` (the existing per-page
  diagnostic path already does exactly this — line 723–729);
- otherwise fall back to the legacy `error: <path>: <err>` plain form.

This is a 1:1 lift of the existing per-page render block — no new
formatting code.

## Code allocation

Add a new subsystem entry to
`crates/quarto-error-reporting/error_catalog.json`. Existing
subsystems: `internal, yaml, markdown, writer, project, cli, …, lua,
listing, navigation, template, xml`. We have **no** subsystem for sass
/ theme today.

Subsystem: `"theme"` (decided 2026-05-22 — names the user-facing
concept, not the implementation, so a future move away from sass
doesn't force a rename). First code `Q-14-1` (next free major;
`Q-13-X` is the last used).
Entry:

```json
"Q-14-1": {
  "subsystem": "theme",
  "title": "Invalid theme configuration",
  "message_template": "{message}",
  "docs_url": "https://quarto.org/docs/errors/Q-14-1",
  "since_version": "99.9.9"
}
```

## Implementation revisions discovered during the work

1. **Reuse `QuartoError::Parse(ParseError)`** instead of adding a new
   `QuartoError::Config(ConfigDiagnostic)` variant. `ParseError` is
   already the project's "diagnostics + source-context envelope"
   (precedent: `resource_error_to_parse_error` for Q-5-1..Q-5-3).
   Cleaner, smaller blast radius.

2. **Add `PipelineError::Structured(ParseError)` variant.** The
   existing bridge in `pipeline.rs:700-715` synthesizes a
   `SourceContext` from the *document content*. That doesn't contain
   `_quarto.yml`, so a diagnostic whose `location` points there
   wouldn't render an ariadne snippet. The new variant is passed
   through the bridge verbatim (`Structured(pe) => QuartoError::Parse(pe)`)
   so the cross-file `SourceContext` survives. Display delegates to
   the wrapped `ParseError`, which is `Display` via its full ariadne
   render.

3. **`file_failure_from_error` did not need changes** — it already
   extracts `QuartoError::Parse`\'s diagnostics + source_context onto
   `FileFailure`.

## Work items

- [x] Investigate `SourceContext` plumbing — resolved by reading
      `_quarto.yml` from disk in the converter, mirroring
      `resource_error_to_parse_error`.
- [x] Write red tests (sass-level + theme-diagnostic-level).
- [x] Add `location: Option<SourceInfo>` to `SassError::InvalidThemeConfig`
      and propagate at `config.rs:454` + `:463` (plus null-`location`
      at the internal brand sites: `config.rs:206/355/394`,
      `brand_layer.rs` two sites, `themes.rs` two sites).
- [x] Allocate `Q-14-1` in `error_catalog.json` (subsystem `theme`).
- [x] Build `crates/quarto-core/src/theme_diagnostic.rs` with
      `sass_error_to_parse_error(err, source_file)`. Mirrors
      `resource_error_to_parse_error`.
- [x] Add `PipelineError::Structured(ParseError)` variant + Display
      delegation + pass-through in the pipeline-to-QuartoError bridge.
- [x] Convert the throw site in `CompileThemeCssStage::run`.
- [x] Update the CLI failure-rendering loop in
      `print_render_diagnostics` to use structured diagnostics from
      `pass2_failures` when present (legacy `error: <path>: <err>`
      fallback preserved for non-structured errors).
- [x] Run `cargo nextest run --workspace` — 9395 tests pass (5 net
      new from this branch; baseline was 9390).
- [x] Run end-to-end against the user's `external-sources/quarto-web`
      fixture and record output below.

## Verification

```bash
NO_COLOR=1 cargo run --bin q2 -- render \
  /Users/cscheid/rooms/room-2/q2/external-sources/quarto-web
```

One representative emission (there are 345 identical copies — bd-9hlja
coalesces them):

```
Error: [Q-14-1] Invalid theme configuration
     ╭─[ /…/external-sources/quarto-web/_quarto.yml:686:15 ]
     │
 686 │       light: [cosmo, theme.scss]
     │               ──┬──
     │                 ╰──── theme must be a string or array of strings
─────╯
```

`grep -c "Q-14-1"` ⇒ **345** structured diagnostics.
`grep "^error:"` ⇒ **0** legacy plain-text emissions for the theme
case (was hundreds before this branch). The ariadne renderer also
emits a working hyperlink to `file:///…/_quarto.yml#686:15`.

The remaining duplication is by design for this issue; bd-9hlja
collapses it.

## Risks (resolved)

- ~~`SourceContext` for `_quarto.yml` may not live on `StageContext`~~ —
  not needed; the converter reads YAML from disk on demand.
- ~~`QuartoError` exhaustive matches would force changes everywhere~~ —
  avoided by reusing `QuartoError::Parse`.
- New `PipelineError::Structured` variant: grep'd for `PipelineError`
  matches; the only non-trivial exhaustive site was the bridge in
  `pipeline.rs`, which we updated. `match` arms elsewhere either use
  `_` fallthrough or pattern-match a single variant.

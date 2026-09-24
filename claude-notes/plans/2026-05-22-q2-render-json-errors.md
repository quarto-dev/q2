# Plan: `--json-errors` for `q2 render`

**Status:** Implementation complete on `feature/q2-render-json-errors`;
awaiting review before merge to `main`. Beads issue still `in_progress`
until then.
**Beads issue:** bd-iey8o
**Related issues:** bd-creo (strict pass-1 exit policy), k-lckc (uniform
quarto-error-reporting in render pipeline), bd-rqba (JsonPass1Failure)
**Related plans:**
- `claude-notes/plans/2025-10-21-json-diagnostics-output.md` — pampa's
  original `--json-errors` design (text-vs-JSON output mode enum).
- `claude-notes/plans/2025-11-21-k-378-json-writer-diagnostics.md` —
  threading diagnostics through the JSON writer.

## Goal

Add `--json-errors` to `q2 render` so that agents and other programs
driving the binary can consume diagnostics (errors, warnings, info) in
a structured, machine-readable form. The first consumer is Claude
Code itself, which currently has to scrape ariadne text out of
stderr.

## What exists today

### pampa (the lower-level binary)

`pampa --json-errors` is wired through `crates/pampa/src/main.rs` and
covered by `crates/pampa/tests/test_json_errors.rs`. Conventions:

- Each diagnostic is emitted as **one JSON object per line** (NDJSON).
- **Warnings** go to **stderr**; **errors** that abort the run go to
  **stdout** (because in pampa, the rendered AST normally takes
  stdout — when the render fails, errors take its place).
- Schema is the in-place `DiagnosticMessage::to_json()` output
  (`{kind, title, code, problem: {content}, hints, details, location}`
  — see `crates/quarto-error-reporting/src/diagnostic.rs:495`).

### `quarto-error-reporting` already has a richer JSON shape

`crates/quarto-error-reporting/src/json.rs` defines `JsonDiagnostic`
(introduced by bd-b9kzg and used by hub-client + the `q2 preview`
`/api/preview/diagnostics` endpoint):

```rust
pub struct JsonDiagnostic {
    pub kind: String,                    // "error" | "warning" | "info" | "note"
    pub title: String,
    pub code: Option<String>,
    pub problem: Option<String>,
    pub hints: Vec<String>,
    pub start_line: Option<u32>,         // 1-based, Monaco convention
    pub start_column: Option<u32>,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
    pub source_file: Option<String>,     // for sibling-page failures (bd-rqba)
    pub details: Vec<JsonDiagnosticDetail>,
    pub rendered: Option<String>,        // pre-rendered ariadne snippet
}

pub struct JsonPass1Failure {
    pub source_file: String,
    pub error: String,
    pub diagnostics: Vec<JsonDiagnostic>,
}
```

The free function `diagnostic_to_json(&DiagnosticMessage,
&SourceContext) -> JsonDiagnostic` does the conversion (resolves byte
offsets to 1-based line/column, pre-renders the ariadne snippet for
located diagnostics, etc.).

**Per the scope decision: we use this shape for `q2 render`.** Pampa
keeps its existing shape; harmonization is out of scope here.

### `q2 render` today

`crates/quarto/src/commands/render.rs`:

- **No `--json-errors` flag.** `RenderArgs` has no field for it; the
  clap definition in `main.rs` doesn't expose it.
- `print_render_diagnostics` (line 704) emits everything as text:
  - `pass1_failures` → `eprintln!("warning: profile-pass skipped …")`
    (a string-matched legacy line — bd-creo wants this gone anyway).
  - `pass2_failures` → coalesced via `coalesce_by_source(…)` then
    `group.to_text()`. The structured `DiagnosticMessage`s and
    `SourceContext`s are already attached to each `FileFailure`.
  - `project_diagnostics` → `diagnostic.to_text(None)` (locationless).
  - `outputs[i].render_output.diagnostics` → `diagnostic.to_text(Some(&source_context))`.
- `QuartoError::Parse(parse_error)` arms in `execute_single_doc` /
  `execute_project` `eprintln!("{}", parse_error)` and `exit(1)`.
- `DispatchError` from `classify_inputs` is rendered through its
  `Display` impl and bubbled out as `anyhow::Error`.

Everything we need (`DiagnosticMessage` + `SourceContext`) is already
on the summary — we just need a JSON-emitting branch that uses
`diagnostic_to_json`.

## Design

### CLI surface

Add to `Commands::Render` in `crates/quarto/src/main.rs`:

```rust
/// Emit diagnostics as one JSON object per line on stderr instead
/// of human-readable text. Intended for agents and other programs
/// that drive `q2 render`. Schema: `JsonDiagnostic` from
/// quarto-error-reporting (same shape served by the preview
/// `/api/preview/diagnostics` endpoint).
#[arg(long = "json-errors")]
json_errors: bool,
```

Thread it through `RenderArgs::json_errors` and into
`print_render_diagnostics` (renamed signature; see below).

### Output channel & format

- **All diagnostics** (errors, warnings, info, project-level) emitted
  to **stderr** as NDJSON — one JSON object per line.
- Rendered HTML still goes to disk (or stdout when `--output -`); the
  json-errors mode does **not** put anything on stdout. (Differs
  from pampa, which routes errors to stdout because pampa's stdout
  normally carries the rendered AST. For `q2 render`, stdout is
  already free.)
- Exit codes are unchanged: `should_exit_nonzero(&summary)` still
  drives `exit(1)`; the strict pass-1 contract from bd-creo is
  unaffected.

NDJSON (not a single JSON array) so partial output during a long
project render is still consumable line-by-line, matching the
established `quarto publish --json` convention ("NDJSON events on
stderr").

### Per-diagnostic schema

`JsonDiagnostic` from `crates/quarto-error-reporting/src/json.rs`,
serialised verbatim — one JSON object per line.

**Pass-1 sibling failures** (bd-rqba's `JsonPass1Failure` shape:
`source_file`, top-level `error`, nested `diagnostics`) are emitted
verbatim, *as their own line shape*, distinct from `JsonDiagnostic`.

**Decided (2026-05-22):** mixed line shapes are explicitly fine on
the stderr stream. Consumers discriminate by structure (and
eventually by `$schema` / JSON Schema — see Documentation below).
Forcing one shape onto every line would lose information (the
pass-1-failure container shape exists precisely to keep grouped
diagnostics grouped) and constrain future additions as the system
grows.

### Render outcome

**Decided (2026-05-22):** NDJSON-only, no final summary line on
stdout. Consumers infer success from the exit code and can reconstruct
the list of outputs from `.quarto/render-manifest.json` or by walking
the output dir. Summarising a stream is the consumer's job, not the
producer's.

This differs from `quarto publish --json`, which emits a final
`PublishOutcome` on stdout — publish has a single discrete result
("did the upload succeed, what's the URL"), whereas render's "result"
is the set of files on disk, already enumerated elsewhere.

### Renaming `print_render_diagnostics`

Replace the body with a branch on `args.json_errors`:

```rust
fn print_render_diagnostics(
    summary: &ProjectRenderSummary,
    args: &RenderArgs,            // pass the full args, not just `quiet`
) {
    if args.json_errors {
        print_render_diagnostics_json(summary);
    } else {
        print_render_diagnostics_text(summary, args.quiet);
    }
    // perf-stats output stays unchanged at the end
}
```

`print_render_diagnostics_text` is the existing body verbatim.
`print_render_diagnostics_json` walks the same four sources
(pass1_failures, pass2_failures, outputs[..].render_output.diagnostics,
project_diagnostics) but emits `diagnostic_to_json(diag, &ctx)` as
NDJSON.

The `QuartoError::Parse(parse_error)` arms in `execute_single_doc` and
`execute_project` also need a JSON branch: today they print the error
as text and exit; under `--json-errors` they should emit each
diagnostic on `parse_error.diagnostics` through `diagnostic_to_json`
against `parse_error.source_context`, then exit.

`DispatchError` (path-not-found, multi-project, etc.) — these are
CLI-level errors emitted before any render runs. Under
`--json-errors`, we synthesise a minimal `JsonDiagnostic`
(`kind: "error"`, `title`, `problem` from the `Display` impl, no
location) and exit. The `cli` subsystem already exists in
`crates/quarto-error-reporting/error_catalog.json` as `Q-7-*` (only
`Q-7-1` "Missing Newline at End of File" is taken today), so each
`DispatchError` variant gets a new code in that range — see
**Open question #2** for the proposed assignments.

## Open questions for the user

1. **`DispatchError` error codes.** The `cli` subsystem already
   exists at `Q-7-*` (only `Q-7-1` is taken — "Missing Newline at End
   of File", emitted by pampa). Proposed assignments, one per
   `DispatchError` variant in `crates/quarto/src/commands/render.rs`:

   | Code  | Variant                 | Meaning |
   |-------|-------------------------|---------|
   | Q-7-2 | `PathNotFound`          | Input path does not exist |
   | Q-7-3 | `NoInputAndNoProject`   | No args and no `_quarto.yml` upstream |
   | Q-7-4 | `MultiArgNonProject`    | Multiple paths outside any project |
   | Q-7-5 | `MultiProjectArgs`      | Inputs span two projects |
   | Q-7-6 | `NotInRenderList`       | File excluded by `project.render` / underscore/hidden rules |
   | Q-7-7 | `NoRenderableMatches`   | Directory expanded to zero renderable files |
   | Q-7-8 | `Discover`              | Project discovery failed (parse error in YAML, I/O, …) |

   Each requires a matching entry in
   `crates/quarto-error-reporting/error_catalog.json` (title,
   message_template, docs_url under `https://quarto.org/docs/errors/Q-7-N`).
   Per `CONTRIBUTING-ERRORS.md` the catalog entry is mandatory for
   any user-facing code.

   The alternative (a) is to leave `code` as `None` for these and add
   it later when a consumer needs to discriminate — cheaper now, but
   the catalog entries are small and assigning codes up-front means
   the agent integration can rely on them. **Recommended: assign all
   seven up-front.**
2. **Documentation for agents.** See **Agent-readable documentation**
   below — the plan now treats this as a substantive design decision
   rather than an afterthought, because the user explicitly raised it
   on 2026-05-22.

## Agent-readable documentation

The `--json-errors` output is a contract between `q2 render` and any
program (agents included) consuming the binary. The contract needs to
be discoverable *without* prior knowledge of the Quarto docs — an
agent should be able to read one stderr line and orient itself.

Layered approach, in order of load-bearingness:

### 1. JSON Schema documents (load-bearing)

Publish one schema per line shape at a stable URL:

- `https://quarto.org/schemas/json-diagnostic.json` — `JsonDiagnostic`
- `https://quarto.org/schemas/json-pass1-failure.json` — `JsonPass1Failure`
- (new shapes added later get their own URLs.)

Source-of-truth lives in the repo (probably under
`crates/quarto-error-reporting/schemas/`) and is published to the
docs site as static assets. Two ways to keep the schema in sync with
the Rust types:

- **(a) Generate via `schemars`** — add `schemars` as a dependency
  (not currently in the workspace), `#[derive(JsonSchema)]` on
  `JsonDiagnostic` / `JsonPass1Failure`, and a small `xtask` that
  regenerates the JSON files. Schema can't drift; cost is one new
  dependency and a generation step.
- **(b) Hand-write the schemas, pin them with a round-trip test** —
  no new dependency; a test serialises a representative diagnostic
  and validates it against the checked-in schema using `jsonschema`
  (already in the workspace? — TBD; otherwise pick a tiny crate).
  Drift is detected at test time, not at write time.

Either is fine for two schemas. Recommendation: **(a)** if we expect
the wire shape to evolve (more line shapes coming as the system
grows, per the user's note on 2026-05-22); **(b)** if we'd rather
keep the dependency surface tight.

**Decided (2026-05-22):** path (a), schemars-derive. Rationale + the
coexistence analysis with `quarto-yaml-validation` below.

#### Coexistence with `quarto-yaml-validation`

Quarto already has a YAML-validation system —
`crates/quarto-yaml-validation` — that consumes a custom schema
dialect authored in YAML, with mandatory source-location tracking
via `quarto-yaml` + `quarto-source-map`. We need to verify that
adopting `schemars` for the CLI wire format does not create a path
toward accidentally using `schemars`-generated JSON Schema for
user-facing YAML config.

Findings:

- **`schemars = "1.2.1"`** is already declared in workspace root
  `Cargo.toml:44`, added 2025-03-10 by commit `aa851a35` for the
  now-removed `biome_ungrammar` / xtask code-generation tree. **No
  current crate imports it** — `grep -rn "use schemars\|schemars::"
  --include="*.rs"` returns nothing. It's a dormant dep with no
  incumbent consumer to conflict with.

- **`quarto-yaml-validation`** self-describes (`src/lib.rs:4`) as a
  "simplified JSON Schema subset" but has real Quarto-specific
  divergences: `arrayOf`, `maybeArrayOf`, `record`, `schema`
  wrapper, `ref` (not `$ref`), short forms (bare `string` /
  `number`), inline-array enums, `required: all` auto-expansion,
  completion annotations. Its schemas are parsed from
  `YamlWithSourceInfo`, never from JSON.

- **`schemars`** generates standard JSON Schema (Draft 7 by default)
  from Rust types via `#[derive(JsonSchema)]`. No Quarto extensions,
  no source-map provenance.

Incompatibility analysis:

1. **Direct interchange does not work.** A schemars-generated JSON
   Schema fed into `quarto-yaml-validation` would mostly be rejected
   (different combinator keywords, different object shape, different
   input medium). A Quarto YAML schema is not something schemars
   reads at all. They are not swappable as input.

2. **The dangerous accidental usage** is a future contributor seeing
   `#[derive(JsonSchema)]` on a *render-output* type and reaching
   for the same pattern on a *user-configuration* type — then
   exporting the resulting JSON Schema to validate user
   `_quarto.yml`. That would bypass `quarto-yaml-validation`
   entirely, lose source-location-aware errors, and miss the Quarto
   dialect features users rely on. This is the failure mode worth
   guarding against; it's a coding-discipline problem, not a
   technical incompatibility.

3. **Coexistence is fine when scoped to declared roles:**

   | Concern                     | Tool                       | Audience |
   |-----------------------------|----------------------------|----------|
   | Wire formats (CLI JSON, RPC, HTTP) | `schemars`          | Programs |
   | User-authored YAML config   | `quarto-yaml-validation`   | Humans   |

   The two produce different artifacts (a JSON Schema file vs. a
   `Schema` value), parsed by different code, for different
   audiences. They never need to interact.

Guardrails:

- **Scope the dependency.** Add `schemars` only to
  `crates/quarto-error-reporting/Cargo.toml`. Do not promote it to a
  habitual addition.
- **Document the boundary** in both directions: a note in
  `crates/quarto-yaml-validation/README.md` ("for user-facing YAML
  config; do NOT replace with schemars-generated JSON Schema") and a
  parallel rustdoc on `JsonDiagnostic`\'s schema export ("schemars is
  used here for wire-format documentation only; YAML config
  validation lives in `quarto-yaml-validation`").
- **Optional follow-up:** an `xtask lint` rule
  `schemars-outside-allowlist` flagging `#[derive(JsonSchema)]` in
  any crate other than `quarto-error-reporting`. The lint infra
  already exists (`crates/xtask/src/lint/`). Defer until we see
  drift, but record it as a possible safeguard.

This issue does items 1 and 2. Item 3 is filed as a follow-up
candidate, not a blocker.

This is the language-agnostic contract. IDEs, validators,
typegen tools, and agents all speak JSON Schema.

### 2. Embed `$schema` in every emitted line (also load-bearing)

Each emitted JSON object gets a `$schema` field pointing at its
schema URL. An agent reading even one stderr line can discover the
schema. Cost is one extra string per line; payoff is self-describing
output that doesn't require docs at all to bootstrap.

`JsonDiagnostic` and `JsonPass1Failure` both gain a
`#[serde(rename = "$schema")] pub schema: &'static str` field with a
const default. Schema URL is set at the point of serialisation (or as
a serde default), not by every call site.

### 3. `llms.txt` on the docs site (discovery)

[llmstxt.org](https://llmstxt.org/) — a `/llms.txt` at the docs site
root, curated Markdown index pointing agents at the most useful docs.
Optional companion `/llms-full.txt` inlines the full text.

Quarto doesn't have one yet. The JSON-output contract is a natural
early entry — agents driving `q2 render` are exactly the audience
llms.txt was designed for. **Out of scope for this beads issue** (it
needs a project-wide decision about which docs are curated for LLM
consumption, plus the docs-site plumbing to serve it). Filed as a
follow-up; this issue just makes sure the schemas exist so they're
ready to be linked.

### 4. `quarto schema` subcommand (offline discovery)

Future: `q2 schema render` (or `q2 render --print-schema`) emits the
JSON Schema documents on stdout, so sandboxed/offline agents can
discover the contract without network. Pattern from `cargo --list`,
`kubectl explain`, `gh api`. **Out of scope for this beads issue** —
listed here so the JSON Schema work in this issue is sized for it
(schemas as repo artifacts, not just static site assets).

### What this issue actually does

- Publish the schema files (item 1).
- Add `$schema` to every emitted line (item 2).
- Items 3 and 4 are follow-ups, not blockers.

## Test strategy (TDD)

Tests live in `crates/quarto/tests/json_errors.rs` (new file). Each
test follows the same template as `crates/pampa/tests/test_json_errors.rs`:
write a fixture qmd, invoke `cargo run --bin q2 -- render <fixture>
--json-errors -o <tmp>`, inspect stderr, parse each line as JSON,
assert structure.

### Phase 0 — foundations (catalog + schemars + schema files)

- [x] Add Q-7-2..8 catalog entries in `error_catalog.json` (one per
  `DispatchError` variant).
- [x] Add `schemars` to `quarto-error-reporting`\'s deps, derive
  `JsonSchema` on `JsonDiagnostic` / `JsonDiagnosticDetail` /
  `JsonPass1Failure`.
- [x] Add `$schema` field (with `#[serde(rename = "$schema")]`) and
  `SCHEMA_URL` consts on both wire shapes; route construction
  through `JsonPass1Failure::new`.
- [x] Lock in `$schema` behavior with unit tests
  (`diagnostic_carries_schema_url`,
  `diagnostic_serializes_schema_field_as_dollar_schema`,
  `pass1_failure_carries_schema_url`).
- [x] Generate `schemas/json-diagnostic.json` and
  `schemas/json-pass1-failure.json`; add `tests/schema_drift.rs`
  which detects drift and self-regenerates under
  `QUARTO_REGEN_SCHEMAS=1`.
- [x] Verified the drift test fires on perturbation and passes after
  restoration.

### Phase 1 — write the failing tests

All seven tests are now in `crates/quarto/tests/json_errors.rs` and
fail in expected ways before Phase 2 lands (6 of 7 fail with
"unexpected argument '--json-errors'"; the 7th, the text-mode
regression check, already passes — the unflagged text path is
untouched).

- [x] `json_errors_flag_exists` — written, fails (flag missing).
- [x] `single_doc_parse_error_json` — written, fails (flag missing).
- [x] `single_doc_warning_json` — written, fails (flag missing).
- [x] `project_pass1_failure_json` — written, fails (flag missing).
- [x] `project_diagnostic_json` — written, fails (flag missing).
- [x] `dispatch_error_json` — written, fails (flag missing).
- [x] `text_mode_unchanged_regression` — written, **already passes**
  (regression guard for the unmodified text path).

All tests must fail before the implementation lands; we confirm that
by running each one immediately after writing it.

### Phase 2 — implementation

- [x] Add `--json-errors` to clap, plumb through `RenderArgs`.
- [x] Split `print_render_diagnostics` into text + JSON branches.
- [x] Implement `print_render_diagnostics_json` for the four
  diagnostic sources on the summary.
- [x] JSON branch for `QuartoError::Parse` in `execute_single_doc`
  and `execute_project`.
- [x] JSON branch for `DispatchError` via
  `dispatch_error_to_diagnostic` (Q-7-2..8 mapping).
- [x] All seven Phase-1 tests now pass.
- [x] Full `quarto` crate suite green (108/108).
- [x] Full workspace suite green (9425/9425) — including the
  schema drift test under workspace context (required a
  `canonicalize_keys` helper so output is independent of
  `serde_json/preserve_order` activation; the schemas are now
  alphabetically-sorted JSON).

### Phase 3 — end-to-end verification (CLAUDE.md non-negotiable)

- [x] Real binary invocations on four fixtures (error / warning /
  clean / dispatch-error), all confirmed to behave correctly.
  Snippets recorded in **End-to-end verification** below.
- [x] `cargo xtask verify --skip-hub-build` is clean: 12/12 steps
  pass.
- [x] pampa's existing `--json-errors` tests still green (7/7) — no
  cross-binary regression.
- [x] Workspace `cargo nextest run --workspace` green
  (9425/9425 pass, 196 skipped, 0 fail).

## Files likely to change

- `crates/quarto/src/main.rs` — clap definition, wire to `RenderArgs`.
- `crates/quarto/src/commands/render.rs` — `RenderArgs` field, the
  split of `print_render_diagnostics`, JSON branches in error arms.
- `crates/quarto/tests/json_errors.rs` (new) — integration tests.
- `crates/quarto-error-reporting/error_catalog.json` — seven new
  `Q-7-*` entries (per Open question #1, if the "assign all" path
  wins).
- `crates/quarto-error-reporting/src/json.rs` — `$schema` field on
  `JsonDiagnostic` and `JsonPass1Failure`, with const URL defaults.
- `crates/quarto-error-reporting/schemas/` (new) — generated JSON
  Schema docs for both shapes, plus a small test that round-trips a
  fixture diagnostic through the schema validator so the schema
  can't drift.
- `docs/` — wherever the static schemas get served from. (Path TBD;
  the docs site already has a static-assets surface — exact location
  is a docs-site concern.)
- Possibly `docs/cli/` — one-paragraph mention for users.

No changes expected to `quarto-core`: the summary already carries
everything we need.

## Risks / non-issues

- **Risk: emitted JSON drifts from the hub-client / preview-API
  shape.** Mitigated by using `diagnostic_to_json` directly (no parallel
  serialisation code in q2). If `JsonDiagnostic` changes, all three
  consumers change together.
- **Risk: stderr noise overwhelms agents on large project renders.**
  NDJSON is line-buffered and each line is a self-contained object;
  this is the same shape `quarto publish --json` already produces and
  has held up.
- **Non-issue: backwards compatibility.** The flag is new; default
  behavior is unchanged.

## End-to-end verification

Performed 2026-05-22 against `target/debug/q2` built from
`feature/q2-render-json-errors`. All four real invocations behaved as
designed.

### Case 1: parse error (single doc)

```text
$ echo '---
title: Bad
---

```{python
' > /tmp/json-errors-e2e/err/bad.qmd

$ q2 render --json-errors -o /tmp/json-errors-e2e/err/bad.html bad.qmd
# exit: 1
# stderr (one NDJSON line, abbreviated):
{
  "$schema": "https://quarto.org/schemas/v1/json-pass1-failure.json",
  "source_file": ".../bad.qmd",
  "error": "<ariadne text>",
  "diagnostics": [
    {
      "$schema": "https://quarto.org/schemas/v1/json-diagnostic.json",
      "kind": "error",
      "code": "Q-2-36",
      "title": "Old-style knitr chunk options are not supported",
      "start_line": 5,
      "start_column": 1,
      "end_line": 5,
      "end_column": 11,
      "source_file": ".../bad.qmd",
      ...
    }
  ]
}
```

Pass-1 wrapper carries the file-level error string; nested
`JsonDiagnostic` carries the typed diagnostic with 1-based location.

### Case 2: metadata warning (single doc)

```text
$ echo '---
title: Warn
description: "[incomplete link"
---

Body.
' > /tmp/json-errors-e2e/warn/warn.qmd

$ q2 render --json-errors -o /tmp/json-errors-e2e/warn/warn.html warn.qmd
# exit: 0
# stderr (one line):
{
  "$schema": "https://quarto.org/schemas/v1/json-diagnostic.json",
  "kind": "warning",
  "code": "Q-1-20",
  "title": "Failed to parse metadata value as markdown",
  "start_line": 3,
  ...
}
```

Non-fatal warning, render still produced output to disk.

### Case 3: clean render (no diagnostics)

```text
$ q2 render --json-errors -o /tmp/json-errors-e2e/ok/ok.html ok.qmd
# exit: 0
# stderr JSON lines: 0
```

Stderr is empty (modulo any tracing output the user enables via
`-v`); the contract is "no diagnostics produces no diagnostic
lines."

### Case 4: dispatch error (path not found)

```text
$ q2 render --json-errors does-not-exist.qmd
# exit: 1
# stderr (one line):
{
  "$schema": "https://quarto.org/schemas/v1/json-diagnostic.json",
  "kind": "error",
  "code": "Q-7-2",
  "title": "Input Path Not Found",
  "problem": "Input path does not exist: .../does-not-exist.qmd"
}
```

CLI-level `DispatchError` mapped to its `Q-7-N` code, emitted before
any render runs.

### CI parity

- Workspace tests: `cargo nextest run --workspace` → 9425/9425 pass.
- Quarto crate (where the CLI lives): 108/108 pass; the seven new
  `json_errors` tests all green.
- Schema drift test: passes under both `cargo nextest run -p
  quarto-error-reporting --test schema_drift` and the workspace
  context (was initially mis-canonicalized due to
  `serde_json/preserve_order` ambiguity; fixed by sorting object
  keys in the test).
- `cargo xtask verify --skip-hub-build` invoked at end of Phase 3
  — all 12 steps passed (`✓ All verification steps passed!`).

---
title: 'Declare the Quarto major version in `_quarto.yml` (bd-gw8fmaz3)'
date: 2026-11-17
description: 'Choose a project-level marker that says a project targets Quarto 2, make q2 warn when an explicit `_quarto.yml` lacks it, and pick a marker that released Quarto 1 versions already refuse.'
status: in-progress  # approved 2026-11-17; implementation on braid/bd-gw8fmaz3-quarto-version-marker
braid:
  strand: bd-gw8fmaz3
  priority: P1
  labels: [project-config]
---

## Overview

Quarto 2 is not fully backwards compatible with Quarto 1. A project written
for one and rendered by the other fails in confusing ways, or worse, renders
with subtly different output. We want both programs to reason about which
major version a project targets:

- **q2 (this work):** when a project has an explicit `_quarto.yml` that does
  not declare it targets Quarto 2, emit a warning. Because released Quarto 1
  versions cannot be changed, an unmarked `_quarto.yml` must be presumed to
  be a Quarto 1 project.
- **Quarto 1 (follow-up, in quarto-cli):** the next Quarto 1 release refuses
  to render a project that carries the Quarto 2 marker, with a message that
  points at q2.

The choice of marker matters most for the *already released* Quarto 1
versions, which we cannot change. This note records what those versions do
with each candidate, read from the source in `external-sources/quarto-cli`
(at `cda410214`, 2026-11-17) and probed against a Quarto 1 dev build
(`99.9.9`, the same checkout). It then recommends one.

Single-file renders with no `_quarto.yml` are out of scope: the warning is
only for projects with an explicit `_quarto.yml`.

## What Quarto 1 already has

### `quarto-required` (the existing version-range key)

Quarto 1 has a key for exactly this purpose, `quarto-required`, a **string**
holding an npm-style semver range (`>= 1.1.0`, `1.*`). Its schema entry is
`src/resources/schema/document-options.yml:373`, so it is a document option:
it is valid in document front matter, in `_metadata.yml`, and at the top level
of `_quarto.yml` (which merges into every document's metadata). The same key
name, with the same range syntax, exists in `_extension.yml`
(`src/resources/schema/extension.yml:14`) and for engine extensions
(`src/execute/engine.ts:63`).

The project/document check is in `renderExecute`
(`src/command/render/render-files.ts:127`):

```ts
const versionConstraint = context
  .format.metadata[kQuartoRequired] as (string | undefined);
if (versionConstraint) {
  const ourVersion = quartoConfig.version();
  ...
  if (!satisfies(ourVersion, versionConstraint)) {
    throw new Error(`in file ${context.target.source}:\nQuarto version ${ourVersion} does not satisfy version constraint ${versionConstraint}.`);
  }
}
```

It runs once per rendered document, before execution, and the thrown error
aborts the render. History matters here:

| Quarto 1 release | Key the document check reads | Notes |
|---|---|---|
| up to 1.1.x | none | No document/project check exists. |
| 1.2.69 – 1.4.184 (Sep 2022 – Jul 2023) | `quarto-version` | The feature (`997f6bbc5`, #2322) documented `quarto-required` but read `quarto-version` by mistake. |
| 1.4.185 and later (Jul 2023 onward; 1.4 stable shipped 2024-01-24) | `quarto-required` | Fixed in `ec5a91713` (#6073). |

So `quarto-required` is enforced by Quarto 1.4 and every later release, and
silently ignored by 1.3 and earlier.

### `quarto-version` (an internal key, not a user setting)

`quarto-version` is **not** a user-facing setting. Quarto 1 injects it into
the default metadata of every HTML-family format with the running version
(`src/format/formats-shared.ts:110`), uses it for the
`<meta name="generator" content="quarto-$quarto-version$">` tag
(`src/resources/formats/html/pandoc/metadata.html:2`, the revealjs template
likewise), and strips it from printed metadata (`src/command/render/pandoc.ts:432`).
In 1.2–1.3 the same key was (accidentally) also the version constraint.

### The `project:` object is closed

Since 1.1.117 (`290af1121`, 2022-08-24, "close schemas that can be closed")
the `project:` object in `_quarto.yml` is validated as a closed schema
(`src/resources/schema/project.yml:5`), and a `_quarto.yml` that fails
validation aborts the render (`readAndValidateYamlFromFile` throws
`ValidationError`, `src/core/schema/validated-yaml.ts:50`). Unknown
*top-level* keys, by contrast, pass validation and flow into document
metadata.

## Probe against Quarto 1

Each case is a two-file project (`_quarto.yml` with `project: type: default`
plus the line shown, and a trivial `index.qmd`), rendered with `quarto render`
using the Quarto 1 dev build (`99.9.9`). Probe directories:
`/private/tmp/claude-501/-Users-cscheid-rooms-room-3-q2/52e1afab-2afa-44fc-8b6b-e03b0246938b/scratchpad/q1probe/`.

| Marker in `_quarto.yml` | Result on Quarto 1 HEAD |
|---|---|
| `quarto-required: "^2.0.0"` | **Refuses** (exit 1): `ERROR: in file …/index.qmd: Quarto version 99.9.9 does not satisfy version constraint ^2.0.0.` |
| `quarto-required: 2` (a number) | **Refuses** (exit 1) with a schema error: `Field "quarto-required" has value 2, which must instead be a string`. |
| `project:` / `quarto: 2` | **Refuses** (exit 1) with a schema error: `property name quarto is invalid … location project:quarto`. |
| `quarto-version: 2` | **Renders**, and stamps `<meta name="generator" content="quarto-2">` into the output. |
| top-level `quarto: 2` | **Renders**; `quarto: 2` leaks into document metadata. |

The probe only exercises Quarto 1 HEAD. The per-release behaviour in the next
section comes from reading the tagged sources; before we commit to a marker
we should confirm it by running the actual 1.0, 1.3 and 1.4 release binaries
(checklist item below).

One caveat specific to dev builds: a Quarto 1 dev build reports `99.9.9`,
which satisfies `>=2` but not `^2`. Only the caret form makes dev builds
refuse too.

## Options

### Behaviour of released Quarto 1 versions per candidate

| Candidate | 1.0.x | 1.1.117 – 1.1.x | 1.2 – 1.3 | 1.4 and later |
|---|---|---|---|---|
| A. `quarto-required: "^2"` | renders | renders | renders | **refuses**, clear message |
| B. `project: quarto: 2` (any new key under `project:`) | renders | **refuses**, schema error | **refuses**, schema error | **refuses**, schema error |
| C. `quarto-version: 2` | renders | renders | refuses (reads it as a range) | renders, corrupts the generator tag |
| D. new top-level key (`quarto: 2`, `quarto-language: 2`, …) | renders | renders | renders | renders |

"Renders" means the old Quarto 1 version silently renders a Quarto 2 project,
which is the failure we are trying to prevent.

### A. Reuse `quarto-required` with a 2.x range

The project declares `quarto-required: "^2"` (or `">=2"`) at the top level of
`_quarto.yml`.

- Every Quarto 1 since 1.4 (July 2023 dev builds; January 2024 stable), which
  is most of the installed base by now, already refuses, with a message that
  names the requirement. No Quarto 1 change is needed for those; the planned
  Quarto 1 follow-up only improves the message ("this is a Quarto 2 project;
  render it with q2").
- It reuses a documented key whose meaning ("which Quarto versions may render
  this") is the meaning we want, with syntax users of Quarto 1 extensions
  already know.
- It also works in document front matter, so a single `.qmd` can carry it
  when we want that later.
- Projects that already declare a 1.x range (`quarto-required: ">=1.4"`) are
  *explicitly* Quarto 1 projects. q2 can say so with a stronger message than
  the generic "no marker" warning.
- **Cost on the q2 side:** q2 reports its real version, currently `0.46.0`
  (`crates/quarto-util/src/version.rs`, decision bd-c6l13j79), which satisfies
  no 2.x range. q2 therefore cannot just evaluate the range against its own
  version until it ships as 2.x. The rule has to be "the range admits some
  2.x version and excludes every 1.x version" (an open question below).
- Gap: Quarto 1.0 – 1.3 render a marked project silently. RStudio releases
  from 2022–2023 bundled 1.2/1.3, so the gap is real, if shrinking.
- Needs an npm-semantics range parser in Rust (the `semver` crate uses Cargo
  semantics: no `||`, no hyphen ranges). bd-3z5bubrf (honour
  `quarto-required` in `_extension.yml`) needs the same parser, so the two
  should share one helper.

### B. A new key under `project:`

The project declares, e.g., `project: { quarto: 2 }` (spelling open).

- Widest coverage: every Quarto 1 since 1.1.117 refuses, because the closed
  `project:` schema rejects the unknown key.
- The refusal message is a schema error ("property name quarto is invalid")
  that says nothing about versions. We can improve it only in future Quarto 1
  releases. Editor tooling built on the Quarto 1 schema (the VS Code
  extension, RStudio) also flags the key as invalid.
- The semantics are cleaner: it declares the project's dialect, not a range of
  binaries.
- Nothing to evaluate on the q2 side: present-and-equal-to-2 or not.

### C. `quarto-version: 2` — rejected

It collides with Quarto 1's internal key: Quarto 1.4+ renders the project and
writes `quarto-2` into the generator meta tag, and 1.2–1.3 reinterpret it as
a version range. It protects no current Quarto 1 user and corrupts output.

### D. A new top-level key — rejected

No released Quarto 1 notices it, so it protects nobody until the Quarto 1
follow-up ships and is installed. It also leaks into document metadata.

### E. Combine A and B

Ask projects to declare both. This gets the 1.1.117+ coverage of B and the
clear 1.4+ message of A (Quarto 1 validates the schema before it renders, so
on 1.4+ the user would see B's schema error first, not A's message). Two
markers to explain, scaffold and keep in agreement for one fact; not
recommended unless the 1.1–1.3 gap turns out to matter.

## Decision (2026-11-17)

**Option A: `quarto-required: "^2"` at the top level of `_quarto.yml`.** Every
q2 scaffold writes it and the docs recommend it.

It works today on every Quarto 1 people actually use: in practice users are
on 1.7 or later, all of which refuse a `^2` project with a message that names
the requirement. The 1.0–1.3 gap does not matter. The caret form also stops
Quarto 1 dev builds (`99.9.9`), which `>=2` would let through.

## q2 behaviour

### Only the base `_quarto.yml` counts

q2 reads the marker **only from the project's own `_quarto.yml` (or
`_quarto.yaml`)**, not from profile overlays (`_quarto-<profile>.yml`),
`_quarto.yml.local`, `_metadata.yml`, document front matter or `-M`.

The reasons:

- "This project targets Quarto 2" is a property of the project, so it
  should be a property of one file. If documents could carry the marker, the
  project's status would depend on every document, and adding one document
  could change it.
- It closes the obvious workarounds (`q2 render -M'quarto-required: "^2"'`,
  `--profile q2`) that would teach people to silence the check instead of
  declaring the version.
- Nothing in the Quarto 1 source argues otherwise. Quarto 1 checks the
  *merged* per-document metadata (`render-files.ts:127`), so in Quarto 1 a
  document's front matter or `-M` overrides the project's value. Our
  narrower reading is stricter than Quarto 1's, never looser, and it is the
  rule the Quarto 1 follow-up should adopt for its refusal as well.

A `quarto-required` in document front matter or `_metadata.yml` stays inert
in q2, as today.

Implementation consequence: `ProjectConfig.metadata` (`mod.rs:1176`) is the
*merged* config, so the check must look at the base file's parse
(`parse_config`, `mod.rs:2321`) before `apply_project_profiles`
(`mod.rs:2489`) runs, and record the result (value plus source span) on
`ProjectConfig`.

### Which projects are checked

`ProjectConfig.config_path` (`mod.rs:1183`) is `Some` only when a real
`_quarto.yml` / `_quarto.yaml` was parsed; it is `None` for single-file
renders and synthesized directory configs. That is the gate. A
`_quarto.yml` without a `project:` key is still an explicit project.

### Classifying a range

Parse the value as an npm-style range and ask two questions: does it admit
**some 1.x version**, and does it admit **some 2.x version**? Prerelease
versions count by their major number (`2.0.46-prerelease` is a 2.x version).

| Admits 1.x | Admits 2.x | Examples | Result |
|---|---|---|---|
| no | yes | `^2`, `>=2`, `2.3.4`, `>=2.0.0 <3` | Quarto 2 project: silent |
| yes | yes | `>=1.4`, `*`, `>=2.5 \|\| 1.9` | open range spanning both majors: Q-5-54 |
| yes | no | `~1.7`, `^1.7`, `>=1.7 <2.0.0` | declared Quarto 1 project: Q-5-55 |
| no | no | `^3`, `<1` | does not admit Quarto 2: Q-5-55 |

Libraries get the boundary wrong, so we implement the two questions
ourselves. With Node's `semver` (the reference implementation of these
ranges), `Range.intersects` gives contradictory answers at the 1/2 boundary
depending on how the probe range is spelled: with
`>=1.0.0-0 <2.0.0-0` / `>=2.0.0-0 <3.0.0-0` as the probes it reports that
`1.*` and `^1.7` admit 2.x; with `>=1.0.0 <2.0.0` it reports that
`2.0.0-0 - 2.0.0` admits 1.x. Both are wrong under the definition above. So
the helper walks the parsed comparator sets itself. Each set is an interval
of versions; it admits major `M` iff the interval meets
`[M.0.0-0, (M+1).0.0-0)`. The `nodejs-semver` crate (v5, already used by
`xtask`) parses ranges and exposes comparator sets. The helper lives in
`quarto-util` and is shared with bd-3z5bubrf. Tests pin every row of the
table plus the boundary cases above.

### Prerelease version strings (`2.0.46-prerelease`)

The classification does not look at q2's own version, so it works the same
whether q2 reports `0.46.0` or `2.0.46-prerelease`. Moving q2 to 2.x-style
versions needs no change here.

There is one trap for later, when we also check the range against q2's own
version as Quarto 1 does. Under npm semantics a prerelease satisfies a range
only if the range itself names a prerelease of the same `major.minor.patch`.
Verified with Node's `semver`: `satisfies("2.0.46-prerelease", "^2")` is
**false** by default and true with `includePrerelease`. That check must
therefore use `satisfies_with_prerelease(.., true)`. It is out of scope for
this work, but the helper's doc comment should say so.

(Ordering is fine: `2.0.46-prerelease` sorts above `2.0.0` and every 1.x,
and below `2.0.46`.)

### Diagnostics (subsystem 5, "project"; next free code is Q-5-53)

All are warnings, so `--strict` turns them into failures.

| Code | When | Anchor |
|---|---|---|
| Q-5-53 | explicit `_quarto.yml` with no `quarto-required` | none (names the file, like Q-5-11); the hint shows the line to add |
| Q-5-54 | range admits both 1.x and 2.x | the value's span |
| Q-5-55 | range admits no 2.x (a declared Quarto 1 project, or neither major) | the value's span |
| Q-5-56 | value is not a string, or not a valid range | the value's span |

Suggested Q-5-53 text: "`_quarto.yml` does not declare a Quarto version, so
it is treated as a Quarto 1 project. If it is meant for Quarto 2, add
`quarto-required: \"^2\"`; Quarto 1 will then refuse to render it."

Each code needs its `docs/errors/project/Q-5-5x.qmd` page and sidebar entry
in the same commit (lint rules error-docs-page-missing /
error-docs-sidebar-unlisted).

### One producer, every host

Project-level warnings are produced once per run but printed through three
paths today:

- the CLI "once per run" block for `config_diagnostics`
  (`crates/quarto/src/commands/render.rs:1229`, copied in
  `crates/quarto-preview/src/lib.rs:604`);
- the orchestrator's `initial_diagnostics()`
  (`crates/quarto-core/src/project/orchestrator.rs:2170`), which reaches the
  hub through `summary.project_diagnostics`;
- nothing in the LSP.

The hub does not show `config_diagnostics` today, and hub users are the most
likely to have q2-only projects. So the warning goes through the
orchestrator, and `q2 render`, `q2 preview` and the hub all get it from one
function, `version_marker_diagnostics(&ProjectConfig)`.

As implemented, the hook is not `initial_diagnostics()` (several book
dispatches never call it) but `finish_project_diagnostics` in
`orchestrator.rs`, which both entry points (`ProjectPipeline::run`, used by
the hub, and `run_with_book_support`, used by the CLI) call exactly once at
the end of every render, dry runs included. It prepends the marker warnings
to `summary.project_diagnostics` and applies the project's `diagnostics:`
policy.

No hub dedup guard: the hub returns diagnostics with each page render, and
its diagnostics panel shows the latest render's list. A guard that reported
the warning only on the first render would make it vanish from the panel on
the next keystroke. Q-5-12's process-global guard is a different case (it
describes the session, not the project's configuration). Publishing it in
the LSP on `_quarto.yml` is a follow-up.

### Opt-out through `diagnostics:`

```yaml
diagnostics:
  Q-5-53:
    level: off
    reason: "a Quarto 1 project we also render with q2 for comparison"
```

This needs new plumbing. `crates/quarto-core/src/diagnostic_policy.rs`
documents that **project-scoped diagnostics are out of reach in v1**: policy
is resolved per document in `MetadataMergeStage` and applied in
`run_pipeline`, and `ProjectRenderSummary::project_diagnostics` never passes
through it. So this work extends the policy to project diagnostics. Build a
`DiagnosticPolicy` from the project config's `diagnostics:` key (the merged
project config, since profiles and `.local` are legitimate places for a
policy), apply it to `project_diagnostics` before the CLI's `--strict`
promotion (the same ordering as per-document policy), and update that
module's "deliberate limits" doc. This lifts the limit for every
project-scoped code, not just these.

A policy in document front matter cannot silence a project-scoped warning,
by construction.

### Blast radius in this repository

The warning fires on every existing `_quarto.yml` in the tree (205 tracked
files). The ones that matter:

- `claude-notes/_quarto.yml` and `docs/_quarto.yml`: CI renders both with
  `--strict`, where a warning fails the run. They get the marker in the same
  commit as the warning.
- `crates/quarto/tests/` (68 files), `crates/quarto-core`,
  `crates/quarto-preview` and `crates/quarto-hub` fixtures: any test that
  asserts "no warnings" or snapshots stderr will change. Add the marker to
  the fixtures rather than accepting new snapshot noise. Keep a few unmarked
  fixtures on purpose, to test the warning.
- `examples/` (about 50 projects): add the marker.
- The nested repro projects under `claude-notes/plans/*-investigation/`:
  check whether the strict render visits them (nested projects are skipped,
  Q-5-31/32). If it does not, leave them alone.
- Scaffolds in `crates/quarto-project-create/resources/templates/` (10
  `_quarto.yml` files, used by `q2 create project` and the hub's
  create-project) write the marker, so new projects never warn.

## Checklist

### Phase 0: settle the design

- [x] Decide the marker: option A, `quarto-required: "^2"`.
- [x] Decide classification, scope (base `_quarto.yml` only), severities
  (all warnings) and opt-out (`diagnostics:`, needs project-level plumbing).
- [x] Confirm the refusal against a real Quarto 1 release (1.7+ and the
  current stable), not just the dev build. Both refuse, exit status 1, with
  the release binaries from GitHub on a two-file project carrying
  `quarto-required: "^2"`:
  - 1.7.34: `ERROR: in file …/index.qmd: Quarto version 1.7.34 does not
    satisfy version constraint ^2.`
  - 1.11.9 (current stable, 2026-10-30): `ERROR: in file …/index.qmd:
    Quarto version 1.11.9 does not satisfy version constraint ^2.`
- [x] Go-ahead from Carlos (2026-11-17).

### Phase 1: tests first

- [x] Range helper: every row of the classification table; the boundary
  cases (`1.*`, `^1.7`, `2.0.0-0 - 2.0.0`, `<2`); a malformed range; a
  non-string value; a doc test noting the prerelease trap.
- [x] `version_marker_diagnostics`: no marker (Q-5-53), `^2` (silent),
  `>=1.4` (Q-5-54), `~1.7` (Q-5-55), malformed (Q-5-56); marker only in a
  profile overlay or `_quarto.yml.local` (still Q-5-53); marker only in
  `_metadata.yml` (still Q-5-53); `_quarto.yaml` spelling; single-file render
  and synthesized directory project (silent).
- [x] Policy: `diagnostics: {Q-5-53: off}` in `_quarto.yml` silences it, and
  the warning stays silent under `--strict`; the same key in document front
  matter does not silence it.
- [x] End-to-end through the binary (`q2 render` on a multi-document fixture
  project): the warning is printed once, and `--strict` fails.
- [x] Hub/WASM: the warning surfaces on every hub and preview render
  (`hub-client/src/services/versionMarkerWarning.wasm.test.ts`).
- [x] Run them; confirm they fail.

### Phase 2: implementation

- [x] Range helper in `quarto-util` (add `nodejs-semver` as a workspace
  dependency).
- [x] Record the base file's `quarto-required` on `ProjectConfig` before the
  profile merge.
- [x] `version_marker_diagnostics`, wired into `finish_project_diagnostics`
  at the end of both orchestrator entry points (no hub dedup; see above).
- [x] Project-level `diagnostics:` policy applied to `project_diagnostics`;
  update `diagnostic_policy.rs` docs.
- [x] Catalog entries Q-5-53..56, docs error pages, sidebar entries.
- [x] Add the marker to the scaffolds, `docs/`, `claude-notes/`, `examples/`
  and the test fixtures; audit snapshot changes.
- [x] User docs: `docs/guides/projects/quarto-version.qmd` (the key, why
  only `_quarto.yml`, what Quarto 1 does) and the four error pages.
  Fixtures: the tracked `_quarto.yml` files under `crates/` got the marker
  appended at the end (keeping existing source spans stable); `docs/`,
  `claude-notes/`, `examples/` and the scaffolds got it at the top. Tests
  that write a `_quarto.yml` inline and expect no project diagnostics got
  it prepended. No snapshot file changed.

### Phase 3: verify

- [ ] `cargo nextest run --workspace`; `cargo xtask verify` (full: the WASM
  leg is affected).
- [x] End-to-end: `q2 render` an unmarked project, a `^2` project and a
  `>=1.4` project; render the `^2` one with Quarto 1 too (Quarto 1 results
  are under Phase 0). Output inspected; `NO_COLOR=1 q2 render` in a
  two-file default project with no `quarto-required`:

  ```default
  Warning [Q-5-53]: `_quarto.yml` does not declare a Quarto version
  Without `quarto-required`, `_quarto.yml` is presumed to belong to a Quarto 1 project, and Quarto 1 will render it without complaint.
  ℹ If this project is meant for Quarto 2, add `quarto-required: "^2"` to `_quarto.yml`?

  Rendered 1 of 1 files to … — 1 warning
  ```

  The same project with `quarto-required: ">=1.4"`:

  ```default
  Warning: [Q-5-54] `quarto-required: ">=1.4"` admits both Quarto 1 and Quarto 2
     ╭─[ …/_quarto.yml:3:18 ]
   3 │ quarto-required: ">=1.4"
     │                  ───┬───
     │                     ╰───── The range in `_quarto.yml` does not say which major version this project is written for, so both Quarto 1 and Quarto 2 will render it.
  ℹ For a Quarto 2 project, write `quarto-required: "^2"`; for a Quarto 1 project, a range below 2, such as `"^1.7"`?
  ```

  With `quarto-required: "^2"` the render prints no Q-5-5x warning and
  passes `--strict` (pinned by `quarto_version_marker_cli.rs`).
- [ ] Hub: confirm the warning in a browser session. Not done; the WASM
  entry points are covered by `versionMarkerWarning.wasm.test.ts`.

### Follow-ups (separate strands)

- [ ] Quarto 1 (quarto-cli): refuse `^2` projects with a message that points
  at q2; warn on open ranges spanning 1.x and 2.x (Q-5-54's counterpart);
  read the marker from `_quarto.yml` itself rather than the merged
  per-document metadata.
- [ ] LSP: publish project-level diagnostics on `_quarto.yml`.
- [ ] bd-3z5bubrf: reuse the range helper for `_extension.yml`.
- [ ] When q2 reports 2.x versions: also enforce the range against q2's own
  version, with prerelease inclusion.

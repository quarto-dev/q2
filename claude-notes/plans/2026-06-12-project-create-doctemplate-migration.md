---
title: 'Migrate quarto-project-create from EJS to quarto-doctemplate'
date: 2026-06-12
description: 'Moves `quarto-project-create` from EJS templates rendered through the JavaScript runtime to `quarto-doctemplate` `$title$` placeholders, making scaffolding synchronous and removing the last JavaScript template consumer.'
---

**Strand:** bd-kuxzj8su (blocks bd-3e3sam51, discovered-from bd-3e3sam51)
**Created:** 2026-06-12
**Status:** planned

## Overview

`quarto-project-create` renders its project-scaffolding templates through
`SystemRuntime::render_ejs()`. That JS template path is wildly oversized for
what it does:

- **Native:** `render_ejs` is implemented by `js_native.rs` via embedded
  deno_core/V8 — pulling rusty_v8 (\~100MB prebuilt downloads, no musl
  prebuilts) into every native binary. **No native code path ever calls it**
  (`quarto-project-create`\'s only dependent is `wasm-quarto-hub-client`).
- **WASM:** `render_ejs` goes through `wasm_bindgen` externs
  (`crates/quarto-system-runtime/src/wasm.rs`) into hub-client's
  `src/wasm-js-bridge/template.js`, backed by an esbuild EJS bundle
  (`crates/quarto-system-runtime/js/dist/ejs-bundle.js`).
- **The templates use exactly one EJS construct:** `<%= title %>`, in three
  files.

`quarto-doctemplate` (the Pandoc-compatible template engine we already use
for document rendering) handles this with `$title$`, is pure Rust, builds on
both native and wasm32 (pampa depends on it), and renders synchronously.

Migrating removes the **last live consumer** of the JS template path on every
target, which unblocks bd-3e3sam51 (drop deno_core/rusty_v8 from
`quarto-system-runtime`) and, downstream, the static-musl release targets
(see PR #280 / bd-c6l13j79). It also means a future native `q2 create`
command can use this crate with no JS engine at all.

## Current state (verified 2026-06-12)

Templates (`crates/quarto-project-create/resources/templates/`):

| File | EJS constructs used |
|---|---|
| `default/_quarto.yml.ejs` | `<%= title %>` |
| `website/_quarto.yml.ejs` | `<%= title %>` |
| `website/index.qmd.ejs` | `<%= title %>` |

Call sites of the JS path in `quarto-project-create/src/lib.rs`:
- `create_project()` — legacy API: `js_available()` gate + `render_ejs()` loop
- `create_project_from_choice()` — scaffold API: same pattern for
  `ScaffoldContent::Template` entries

Template data passed: `{ "title": ..., "projectType": ... }` — only `title`
is referenced by any template.

Consumers of `quarto-project-create`: **only** `wasm-quarto-hub-client`
(`create_project`, `create_project_from_choice` wrappers around
lib.rs:1955–2110, plus `js_available` gates).

## Design decisions

- **Template syntax:** doctemplate `$title$`. None of the templates contain a
  literal `$`, so no escaping issues in the existing content. Rename files
  `*.ejs` → `*.template`.
- **API change:** `create_project()` / `create_project_from_choice()` drop the
  `runtime: &dyn SystemRuntime` parameter and become **synchronous** —
  doctemplate rendering is pure CPU. `quarto-project-create` drops its
  `quarto-system-runtime` dependency entirely. The wasm wrappers stay
  `async`-shaped toward JS (they return Promises) but no longer await
  anything for template rendering.
- **Escaping behavior change (deliberate):** EJS `<%= %>` HTML-escapes
  interpolations, which was a latent bug for YAML/qmd output (a title
  containing `&` rendered as `&amp;` in `_quarto.yml`). Doctemplate inserts
  the value raw, fixing that. Titles containing `"` break the YAML
  double-quoted string in **both** engines; this plan adds a
  YAML-double-quote escaping helper at the data-construction site (escape
  `\` and `"`) so quoted titles are safe. Test specs below cover both cases.
- **Scope boundary:** this strand stops at "no caller of the JS template
  path remains". Removal of the machinery itself stays in bd-3e3sam51:
  - `deno_core`/`serde_v8`/`js_native.rs` and the `render_ejs` /
    `js_render_simple_template` / `js_available` trait surface in
    `quarto-system-runtime`
  - the `wasm.rs` extern shims and hub-client `src/wasm-js-bridge/template.js`
  - `crates/quarto-system-runtime/js/` (esbuild EJS bundle)
  - `claude-notes/plans/js-execution-performance.md` (obsolete after removal)
  - the wasm test wrappers `test_js_available` / `test_render_simple_template`
    / `test_js_ejs` in `wasm-quarto-hub-client` and their TS typings

## Work Items

### Phase 1: Tests first (TDD)

- [x] In `quarto-project-create`, write tests for the new sync API
      (they will not compile/pass until Phase 2 — that is the expected
      failure mode for an API migration):
  - [x] `create_project(Website, "My Website")` produces `_quarto.yml`
        containing `title: "My Website"` and `index.qmd` with the title in
        front matter — no `$title$` or `<%` residue in any output.
  - [x] Same for `Default` project type.
  - [x] Special characters: title `R & D "quoted" \ backslash` renders
        **unescaped** `&` (no `&amp;`) and YAML-escaped `\"` / `\\`;
        newline-in-title test; `yaml_escape_double_quoted` unit tests.
  - [x] `create_project_from_choice()` equivalents for the scaffold path.
  - [x] Update `test_templates_are_valid_ejs` → `test_templates_are_valid_doctemplate`:
        every template compiles with `Template::compile` and contains `$title$`.
- [x] Run `cargo nextest run -p quarto-project-create` and record the failures.
      **Recorded 2026-06-12:** 28 compile errors — new sync signatures
      (`create_project(options)` without runtime), missing
      `yaml_escape_double_quoted`, `quarto_doctemplate` not yet a dependency.
      Expected failure mode for an API migration; old `integration_tests`
      module replaced by platform-independent `render_tests`.

### Phase 2: Migration

- [x] Rename `resources/templates/**/*.ejs` → `*.template`; change
      `<%= title %>` → `$title$`. Update `include_str!` paths and doc
      comments in `templates.rs` / `scaffold.rs`.
- [x] Replace `runtime.render_ejs(template, &data)` with
      `Template::compile(template)` + `TemplateContext` (insert `title`,
      `projectType`, optional `template` as `TemplateValue::String`); added
      `yaml_escape_double_quoted` helper applied to `title` (the only
      user-controlled value; templates interpolate it inside double-quoted
      YAML strings only — documented on the helper).
- [x] Make `create_project` / `create_project_from_choice` /
      `create_scaffolded_files` sync; removed the `runtime` parameter and
      `js_available()` gates. `CreateError::TemplateRender` retained for
      compile/render errors.
- [x] Dropped `quarto-system-runtime`, `async-trait`, `serde_json`, and the
      `pollster` dev-dep from `quarto-project-create/Cargo.toml`; added
      `quarto-doctemplate`.
- [x] `cargo nextest run -p quarto-project-create` — 31/31 pass
      (2026-06-12).

### Phase 3: hub-client call sites

- [x] Update `wasm-quarto-hub-client/src/lib.rs` wrappers: `create_project`
      is now a **sync** `pub fn` (no runtime, no `js_available` gate, no
      await). `test_js_*` wrappers and the wasm-js-bridge left untouched
      (bd-3e3sam51's scope).
- [x] hub-client TS: no TS code calls `create_project` yet — only the
      `.d.ts` typing existed; updated it to return `string` instead of
      `Promise<string>`.
- [x] Added `hub-client/src/services/projectCreate.wasm.test.ts` (vitest
      WASM harness, runs in `npm run test:wasm`): 6 tests covering choices,
      website/default scaffolds, sync return, YAML escaping, unknown id.
      All pass against the real WASM module (2026-06-12). Note: the
      standalone `test-wasm.mjs` pattern no longer loads in plain Node
      (bridge `raw_module` imports); the vitest harness is the supported
      path.

### Phase 4: Verification

- [x] `cargo build --workspace` + `cargo nextest run --workspace` —
      9965/9965 pass (2026-06-12).
- [x] Full `cargo xtask verify` — all steps pass (2026-06-12), including
      `npm run build:all` (WASM + production build) and hub-client
      `test:ci`.
- [x] End-to-end: exercised through the **real WASM module** via the vitest
      WASM harness (`npm run test:wasm` path, the project-sanctioned
      Node-based e2e per `claude-notes/instructions/testing.md`). Honest
      scope note: there is **no browser UI wired to `create_project` yet**
      (no TS caller exists — only the `.d.ts` typing), so a browser session
      would exercise nothing beyond what the WASM harness already covers.
- [x] E2E record: invocation
      `npx vitest run --config vitest.wasm.config.ts src/services/projectCreate.wasm.test.ts`
      → 6/6 pass. Observed output (website project): `_quarto.yml` contains
      `title: "My Website"` + `type: website`; `index.qmd` front matter
      contains `title: "My Website"`; special-char title
      `R & D "quoted" \ backslash` renders as
      `title: "R & D \"quoted\" \\ backslash"` with no `&amp;`. Output
      inspected via test assertions on exact rendered content.

**Discovered work:** bd-5wky1mq2 — pre-existing `cargo clippy` breakage
workspace-wide (`deprecated(since = "0.x")` in `quarto-source-map`).

### Phase 5: Handoff

- [x] bd-kuxzj8su closed with results (commits `3a35de43` + `67b61ef3`
      on branch `beads/bd-kuxzj8su-migrate-quarto-project-create`).
- [x] `braid comment bd-3e3sam51` noting the blocker is cleared and listing
      the now-dead machinery enumerated under "Scope boundary" above.

## Follow-on (not in this strand)

- bd-3e3sam51: remove deno_core/rusty_v8 + the whole JS template surface;
  measure build-time/binary-size win; revisit static-musl release targets.
- A native `q2 create` command consuming `quarto-project-create` directly
  (no JS engine needed after this migration).

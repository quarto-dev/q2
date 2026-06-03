# Plan: `q2 get-config` — emit merged document config as JSON

- **Beads:** bd-xoaic
- **GitHub:** quarto-dev/q2#256
- **Status:** IMPLEMENTED — all phases (0–4) complete; full workspace tests
  (9542 pass) + `cargo xtask verify --skip-rust-tests` (WASM/hub build) green.
  Awaiting user review. One item flagged below for confirmation (D3 default
  format = `html`).
- **Date:** 2026-06-02

## Overview

Add a CLI subcommand that returns the **fully-merged document
configuration** as JSON:

```
q2 get-config <file> [yaml-path]
```

The point is that external tool authors (editors, linters, build
wrappers) can ask Quarto 2 "what is the effective value of key `X` for
this document?" and get back the result *after* all of Q2's metadata
resolution semantics — project `_quarto.yml`, directory
`_metadata.yml` layers, document frontmatter, format flattening
(`format.<fmt>.*`), and merge-operator semantics (`!prefer` /
`!concat`). Tool authors should not have to reimplement any of that.

- `q2 get-config foo.qmd key.value.subvalue` → JSON value at that path.
- `q2 get-config foo.qmd` (no path) → the entire merged metadata as JSON.

## Why this is cheap (the efficiency claim in #256)

The merged metadata is produced at the **document-profile checkpoint**,
which only requires `ParseDocumentStage` + `MetadataMergeStage` (plus,
optionally, `IncludeExpansionStage` + `DocumentProfileStage`). It runs
**no engines, no user filters, no rendering**. This is the same work
Pass-1 already does per document.

`crates/quarto-core/src/project/orchestrator.rs:1668`
(`pass1_profile_single_file_live`) is the existing template — it builds
exactly this stage list and runs it via `run_pipeline`, producing a
`PipelineData::AtProfile { profile, ast }`. The merged metadata we want
is `ast.meta` (a `ConfigValue`), which the live computation already has
in hand.

**Important correction to the issue's framing:** the *Pass-1 cache*
(`crates/quarto-core/src/project/profile_cache.rs`) does **not** help
here. It stores only the lossy, typed `DocumentProfile` (title as a
flattened `String`, authors as `Vec<String>`, etc.) — not the full
merged `ConfigValue`. So a v1 `get-config` cannot be served from the
existing cache for arbitrary keys; it must run the (cheap) parse+merge
pipeline live. Caching the full merged metadata is possible as a
follow-up (see "Future work"), but is not required for correctness and
is out of scope for v1.

## Key facts established from the source

- **CLI shape** (`crates/quarto/src/main.rs`): clap `Commands` enum +
  per-command module in `crates/quarto/src/commands/`. Model to copy:
  `trace show` — `commands/trace.rs` splits a testable
  `show_value(&args) -> serde_json::Value` from a thin `execute(args)`
  that prints `serde_json::to_string_pretty`.
- **Merged metadata type**: after `MetadataMergeStage`, `ast.meta` is a
  `ConfigValue` (`crates/quarto-pandoc-types/src/config_value.rs`),
  always a top-level `Map`. Prose values
  (`title: Hello _world_!`) are stored as
  `ConfigValueKind::PandocInlines([Str, Space, Emph[Str "world"], Str"!"])`
  because document-metadata strings are parsed as markdown at parse time.
  Project `_quarto.yml` strings stay literal `Scalar` unless tagged `!md`.
- **Path navigation already exists**: `ConfigValue::get_nested("a.b.c")`
  / `get_path(&["a","b","c"])`. **Caveat:** these traverse *maps only* —
  there is no array-index support (`authors.0.name` does not work today).
- **`ConfigValue`'s default serde output is NOT suitable** for this
  command. It emits an internal tagged form, e.g.
  `{"PandocInlines":[{"t":"Str",...}]}`, with merge-op/source wrappers.
  `get-config` needs its **own clean JSON projection** (a small
  `config_value_to_output_json(value, mode)` function), not
  `serde_json::to_value(&ast.meta)`.
- **Format-dependence**: the merge flattens `format.<fmt>.*` into the
  top level (`resolve_format_config`). **The merged result therefore
  depends on the target format** — `title`/`toc`/etc. can differ per
  format. `get-config` must pick a format (see Open Questions Q3).

## Resolved decisions (review round 1, 2026-06-02)

- **D1 (was Q1) — `value`-mode prose = faithful source text**, not
  lossy plain text. Two acceptable implementations; either is fine:
  - **markdown round-trip** of the `PandocInlines` via the qmd writer
    (`crates/pampa/src/writers/qmd.rs`) → `"Hello _world_!"`. Robust:
    self-contained, works even when a value was merged in from another
    file. **Recommended primary.**
  - **source-map slice**: read the `SourceInfo` spans on the inlines and
    pull the original substring. Most faithful (exact whitespace /
    escapes), but trickier here: merged metadata mixes values that
    originate in *different* source files (`_quarto.yml`, each
    `_metadata.yml`, the doc), so the slicer must resolve each value's
    originating file + bytes. Acceptable as a refinement if the qmd
    writer's output proves not faithful enough.
- **D2 (was Q2) — missing path** prints `null` and exits 0 by default;
  add `--strict` to exit non-zero with a diagnostic when the path does
  not exist. (Real errors — file not found, parse failure — are always
  non-zero regardless of `--strict`.)
- **D3 (was Q3) — single merge code path.** `get-config` must NOT
  reimplement merging. It runs the same `MetadataMergeStage` the render
  pipeline runs and reads `ast.meta`. The target format defaults to the
  **document's first declared format** (fallback `html`) — obtained by
  reusing render's existing format-determination so `ctx.format` is
  built identically — and `--to <fmt>` overrides. More generally: the
  merge result must equal what rendering under the same invocation
  conditions would have produced.
- **D4 (was Q7) — array-index paths** like `authors.0.name` are
  supported in v1: extend path navigation so a numeric segment indexes
  into an `Array` (current `get_path` is map-only).
- **D5 (was Q4) — JSON formatting:** pretty-printed by default;
  `--compact` emits single-line JSON for piping.
- **D6 (was Q5) — `!path`/`!glob`/`!expr` in `value` mode:** ignore the
  tag distinction for v1 — emit the underlying string as a plain JSON
  string (`!expr foo` ⇒ `"foo"`). No tagged-object wrapper.
- **D7 (was Q6) — `pandoc`-mode JSON:** self-contained Pandoc fragment;
  source locations are dropped (no `s` pool / no `SourceInfo`).
- **D8 (was Q8) — command name:** `get-config`.

## Proposed CLI surface

```
q2 get-config <file> [yaml-path] [--to <format>] [--output <mode>] [--strict] [--compact]
```

| Arg/flag        | Meaning                                                              | Default |
|-----------------|---------------------------------------------------------------------|---------|
| `<file>`        | path to a `.qmd`/`.md` document                                     | (required) |
| `[yaml-path]`   | dot-separated key path (numeric segment ⇒ array index, D4); empty ⇒ entire merged metadata | empty |
| `--to <fmt>`    | target format whose `format.<fmt>.*` overrides are flattened in     | document's first declared format, else `html` (D3) |
| `--output <m>`  | prose representation: `value` \| `pandoc`                           | `value` (D1) |
| `--strict`      | exit non-zero if `yaml-path` does not exist (else print `null`, exit 0) | off (D2) |
| `--compact`     | single-line JSON for piping (else pretty-printed)                   | off (pretty default) (Q4) |

### `--output value` (default)

Clean, idiomatic JSON. Scalars map to JSON scalars. Maps/arrays map to
JSON objects/arrays. **Prose values** (`PandocInlines`/`PandocBlocks`)
are rendered back to a **faithful string** (D1 — markdown round-trip via
the qmd writer, with source-map slicing as an optional refinement).
`as_plain_text` (lossy) is explicitly rejected.

`!path` / `!glob` / `!expr` tagged values: v1 ignores the tag and emits
the underlying string as a plain JSON string (D6).

### `--output pandoc`

Prose values are emitted as **Pandoc AST JSON** (`Emph [Str "world"]`
shape), using the existing pampa JSON writer machinery
(`crates/pampa/src/writers/json.rs`), as a **self-contained fragment
with source locations dropped** (D7). Scalars/structure still map to
plain JSON; only `PandocInlines`/`PandocBlocks` leaves switch to AST.

## Design / implementation steps (TDD)

Per CLAUDE.md, tests first.

### Phase 0 — projection function + tests (pure, no pipeline) ✅ DONE
- [x] Add projection `config_value_to_json(&ConfigValue, ProseMode, &ASTContext)
      -> serde_json::Value`. Landed in **`crates/pampa/src/config_json.rs`**
      (pampa owns both the qmd + json writers it needs; `ConfigValue` is in
      `quarto-pandoc-types`, a pampa dep). Strips merge-op / source-info; renders
      prose per `ProseMode::{Value, Pandoc}`.
- [x] Supporting writer additions:
      - `pampa::writers::qmd::write_inlines` — public inline-run writer (shared
        context, no trailing newline) for the markdown round-trip (D1).
      - `pampa::writers::json::{inlines_to_source_free_json,
        blocks_to_source_free_json}` — reuse the maintained `write_inlines`/
        `write_blocks` match and strip `s`/`l`/`attrS` keys for a self-contained,
        source-free Pandoc fragment (D7).
- [x] Unit tests over hand-built `ConfigValue`s: scalar string/int/bool/null/
      float, nested map, array, `PandocInlines` in both modes (value ⇒
      `"Hello *world*!"`; pandoc ⇒ source-free AST), deferred tags ⇒ string (D6).
- [x] `navigate(&ConfigValue, path)`: empty ⇒ root; `a.b.c` ⇒ nested map keys;
      numeric segment ⇒ array index (D4); missing/oob ⇒ `None`. Tests for each.
- **Result:** 12 tests pass (`cargo nextest run -p pampa config_json`).
- **Note (D1):** the qmd writer normalizes emphasis to `*`, so
      `title: Hello _world_!` round-trips to `"Hello *world*!"` — semantically
      faithful, not byte-identical. Exact-source-text via source-map slicing is
      a documented future refinement.

### Phase 1 — pipeline reuse to get merged metadata ✅ DONE
- [x] `quarto_core::get_config::merge_document_metadata(runtime, project,
      format, doc_info, source_bytes) -> Result<(ConfigValue, ASTContext)>`
      in **`crates/quarto-core/src/get_config.rs`**. Runs
      `[ParseDocumentStage, MetadataMergeStage]` via `run_pipeline` and returns
      `doc.ast.meta` + `doc.ast_context`. Modeled on
      `pass1_profile_single_file_live` but returns the full merged metadata, not
      the lossy profile. Include-expansion is omitted (includes don't affect
      `meta`); the merge is entirely `MetadataMergeStage` — the single shared
      merge path (D3).
- [x] Returns the document's `ASTContext` so `--output pandoc` can serialize
      prose; value mode ignores it.
- [x] Integration test `crates/quarto-core/tests/integration/get_config_merge.rs`
      (registered in `main.rs`): fixture with `_quarto.yml` +
      `sub/_metadata.yml` + doc frontmatter. Asserts: `format.html.toc`
      flattening overrides top-level `toc` (`true`); directory `_metadata.yml`
      `description` overrides project (`"from dir"`); frontmatter prose `title`
      round-trips (`"Hello *world*!"`); navigation (empty/key/missing). 4 tests
      pass.
- **Deferred:** single-file (no `_quarto.yml`) path — exercised in Phase 3 e2e
      rather than a separate unit test, since `ProjectContext::discover` handles
      both and the CLI is the real entry.
- **D3 adjustment (flagged for review):** default format is **`html`**, matching
      render's actual `--to` default (render only supports native/html today and
      *bails* on other formats). Implementing "first declared format" now would
      make `get-config` *inconsistent* with render — the opposite of D3's intent.
      `--to <fmt>` overrides and (unlike render) `get-config` will happily merge
      for any format id since it never renders. Revisit "first declared format"
      once render itself honors it, so the two stay consistent.

### Phase 2 — wire the CLI subcommand ✅ DONE
- [x] `crates/quarto/src/commands/get_config.rs`: `GetConfigArgs`, `OutputMode`
      (clap `ValueEnum` → `ProseMode`), testable
      `get_config_value(&args) -> Result<Option<serde_json::Value>>` (`None` =
      path absent, distinct from real errors), thin `execute(args)` that prints
      (compact/pretty; `null`+exit-0 vs `--strict` error).
- [x] Registered `GetConfig` in `Commands` enum (`#[command(name="get-config")]`)
      + `commands/mod.rs` + dispatch in `main()`.
- **Layering note:** `pampa` is only a *dev*-dependency of the `quarto` crate,
      and commands are meant to delegate to `quarto-core`. So the projection
      helpers (`config_value_to_json`, `navigate`, `ProseMode`) are re-exported
      from `quarto_core::get_config` and the CLI imports them from there — it
      never depends on `pampa` directly.

### Phase 3 — end-to-end verification (CLAUDE.md mandate) ✅ DONE
Exercised the real `q2` binary against an on-disk fixture (project with
`_quarto.yml` + `sub/_metadata.yml` + `sub/doc.qmd`, plus a standalone doc).
**Observed output (inspected):**

| Invocation | Output |
|---|---|
| `get-config sub/doc.qmd` | full merged JSON: `description:"from dir"`, `toc:true`, `title:"Hello *world*!"`, `authors:[{name:Alice},{name:Bob}]`, `format:"html"` |
| `… title` | `"Hello *world*!"` |
| `… title --output pandoc` | `[{"t":"Str","c":"Hello"},{"t":"Space"},{"t":"Emph","c":[{"t":"Str","c":"world"}]},{"t":"Str","c":"!"}]` (no `s` keys) |
| `… toc --to html` / `--to pdf` | `true` / `false` |
| `… documentclass --to pdf` | `"article"` |
| `… authors.1.name` | `"Bob"` |
| `… --compact` | single-line JSON |
| `… does.not.exist` | `null`, exit 0 |
| `… does.not.exist --strict` | error, exit 1 |
| standalone doc (no project) | merged frontmatter only |
| missing input file | error, exit 1 |

- [x] Codified as automated CLI e2e tests driving `CARGO_BIN_EXE_q2`:
      `crates/quarto/tests/integration/get_config_cli.rs` (11 tests, all pass),
      registered in `tests/integration/main.rs`.

### Phase 4 — docs ✅ DONE
- [x] `docs/guide/get-config.qmd` — usage-focused page (paths, `--to`, `--output`,
      `--strict`, `--compact`, `jq` piping). Linked in the guide sidebar
      (`docs/guide/index.qmd`).
- [x] Verified it renders with q2: `q2 render docs/guide/get-config.qmd` → exit 0,
      output `docs/_site/guide/get-config.html` contains the expected content
      (`_site` is git-ignored).

## Open questions

All resolved — see "Resolved decisions" (D1–D8). No design questions
remain blocking implementation.

## Future work (out of scope for v1)

- Cache the full merged `ConfigValue` (or a get-config-shaped
  projection) in the project Pass-1 cache so repeated queries during an
  interactive session are near-instant. Requires either a parallel
  cache namespace or extending `DocumentProfile` (profile_version bump)
  — deliberately deferred.
- `--all-formats` mode returning a `{format: merged}` map.
- Watch mode / batch mode (multiple files / multiple paths in one call).

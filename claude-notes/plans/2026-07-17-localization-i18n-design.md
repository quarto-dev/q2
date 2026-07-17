# Localization / internationalization for Quarto 2

**Braid strand:** bd-llhlzd7p (epic)
**Status:** design draft — iterating with Carlos before execution
**Related strands:** bd-99ru (listing category sidebar labels), bd-fod3 (sidebar language selector), bd-apudk (citeproc locale silent-degrade)
**Follow-up strand:** bd-xzaiqpjq (Lua exposure of `quarto.language`; blocked by bd-2llqjsms + bd-a9g50za2 — no `pandoc.Meta` support in Lua filters yet)

## Overview

Bring Quarto 1's localization model to Q2. The guiding principle is unchanged
from Q1: **Quarto's own messages (CLI output, errors, logs) stay in English;
rendered documents localize.** Localization is driven by:

1. **`lang`** — a document/project option holding a BCP 47 tag (`fr`, `pt-BR`,
   `de-CH`). Flows to Pandoc-style `<html lang="…">` and selects the term set.
2. **Shipped term files** — `_language.yml` (English defaults, ~111 keys) plus
   `_language-<tag>.yml` per language (34 files in Q1), copied into this repo
   and embedded in the binary.
3. **`language:`** — user-facing metadata key for overrides: an inline flat map
   of terms, per-language subkeys (`language: { fr: { … } }`), or a path to a
   custom YAML file. A `_language.yml` at the project root is picked up
   automatically.

We deliberately keep **key-level compatibility with Q1** (`crossref-fig-title`,
`callout-note-title`, `title-block-published`, `section-title-abstract`, …) so
existing documents, custom translation files, and the community's translation
contributions carry over unmodified.

### Why Q2 gets to be simpler than Q1

Q1 delivers the resolved language table through four distinct channels (Lua
filter params, explicit copies into Pandoc metadata, direct TS reads into the
DOM, and the `$quarto.language.*$` defaults-file template namespace — see
`external-sources/quarto-cli/llm-docs/localization-architecture.md`). These
exist because Q1 straddles TS, Pandoc templates, and Lua with no shared memory.

Q2 has a single render pipeline in one process. We can have **one resolved
term table** with exactly two consumer surfaces:

- **Rust transforms/writers** read it through a typed accessor on the stage
  context.
- **Templates** read it as the `$quarto.language.<key>$` dotted namespace
  (quarto-doctemplate already resolves dotted paths — zero engine changes).

A third surface (the Lua filter API, mirroring Q1's `param("language")`) is
planned but deferred to a later phase.

## Current state of Q2 (survey, 2026-07-17)

No language machinery exists; all user-visible strings are hardcoded English.
High-leverage change points found in the survey:

| Feature | Where | Today |
|---|---|---|
| Crossref display names | `crates/quarto-core/src/crossref/registry.rs:78-105` (`BUILTINS`) | English literals ("Figure", "Table", theorem family, callout-as-crossref) |
| Theorem sugar names | `crates/quarto-core/src/transforms/theorem.rs:62-67` | **Second copy** of the same display names |
| Crossref rendering | `crates/quarto-core/src/transforms/crossref_render.rs:695,730-731` | `format!("{kind}\u{a0}{n}")`, hardcoded `: ` title delimiter |
| Callout titles | `crates/quarto-core/src/transforms/callout_resolve.rs:250,272` | `capitalize(callout_type)` — mechanical, not a lookup |
| TOC title | `crates/quarto-core/src/transforms/toc_generate.rs:128` | `"Table of Contents"` literal |
| Title-block labels | `crates/quarto-core/src/template.rs:227,232,240` | "Author"/"Published"/"Abstract" hardcoded **in the template string** |
| `<html lang>` | `crates/quarto-core/src/template.rs:81,146` | Works (generic metadata pass-through, no validation) |
| Revealjs | `crates/quarto-core/src/revealjs/assemble.rs:408` | **Bug:** hardcodes `<html lang="en">` |
| LaTeX/Typst | — | No writers exist yet; babel/polyglossia is out of scope until they do |
| Code-fold label | — | Not implemented yet ("Show the code" doesn't exist in q2) |
| Search/listing UI | — | Not emitted from Rust yet (listing labels tracked in bd-99ru) |

Metadata flows through `MetadataMergeStage`
(`crates/quarto-core/src/stage/stages/metadata_merge.rs`), producing fully
merged `doc.ast.meta` as `ConfigValue` trees. The template context is populated
by walking that tree (`crates/quarto-core/src/template.rs:590,627`), and dotted
namespaces like `$navigation.toc.title$` already work.

## Design

### D1. Term files ship in-repo, embedded

- New directory `resources/language/` containing `_language.yml` +
  `_language-<tag>.yml`, **copied** from
  `external-sources/quarto-cli/src/resources/language/` (one-time copy per the
  external-sources policy; never referenced in-place). Add a `README.md` noting
  provenance + the upstream commit, and that updates are re-copies.
- Embedded via `include_dir!` in `quarto-core` (same pattern as
  `resources.rs`'s knitr bundle), parsed on demand with `quarto-yaml`.
- Keys we don't consume yet (e.g. `listing-page-*`, `search-*`) still ship and
  still resolve — they're inert until their features land, and user templates
  can already reference them via `$quarto.language.*$`.

### D2. Resolution semantics (Q1-compatible)

A pure function in a new `quarto-core` module (working name
`crates/quarto-core/src/language.rs`):

```
resolve_language(lang_tag, embedded_defaults, project_root_file,
                 user_language_value) -> LanguageTerms
```

Merge order (lowest to highest precedence), matching Q1's `formatLanguage`:

1. Embedded `_language.yml` (English base).
2. Embedded `_language-<subtag-prefix>.yml`, walking BCP 47 subtags most
   general first: `pt-BR` merges `_language-pt.yml` then `_language-pt-BR.yml`.
3. Project-root `_language.yml`, if present (auto-detected, same subtag walk
   for sibling `_language-<tag>.yml` files).
4. User `language:` value: a **string** is a path to a YAML file (error if
   missing); a **map** is used directly. In either case, top-level plain keys
   apply unconditionally; per-language subkeys (`en:`, `fr:`, `fr-CA:`) apply
   only when they match the subtag walk of `lang`, most general first.

`lang` itself resolves as: CLI `--metadata lang=…` > document/project merged
metadata `lang` > default `"en"`. (Since resolution runs after
`MetadataMergeStage`, the standard six-level precedence handles most of this
for free.)

Term keys are accepted leniently like Q1: known keys from the catalog, plus
`crossref-*-title` / `crossref-*-prefix` patterns for custom crossref types.
Unknown keys under `language:` produce a **warning diagnostic** (Q1 silently
validates against a schema; we can do better) but are still carried, so users
can reference custom terms from their own templates.

Crossref-specific fallback, also Q1-compatible: `crossref-{type}-prefix`
defaults to `crossref-{type}-title` when omitted.

### D3. One resolved table, two surfaces

- **New pipeline stage** `LanguageResolveStage`, immediately after
  `MetadataMergeStage` (before the DocumentProfile checkpoint, so profile
  extraction and everything downstream can see it).
- The stage produces a `LanguageTerms` value (a thin newtype over
  `BTreeMap<String, String>` with accessor helpers like
  `terms.get("callout-note-title")` and
  `terms.crossref_title("fig")` / `terms.crossref_prefix("fig")` with the
  prefix→title fallback built in).
- Storage: written into `doc.ast.meta` as a `ConfigValue` map under the
  reserved key `quarto.language` (marked as system-injected). **Only the
  `quarto.language` subtree is claimed** — we do not reserve the whole
  top-level `quarto.*` namespace (decided 2026-07-17; that's more real estate
  than this feature needs). Because the template context is built by walking
  `meta`, this makes `$quarto.language.<key>$` work with **no doctemplate
  changes**.
  Transforms access it through a `LanguageTerms::from_meta(&meta)` helper (or a
  field on `StageContext` — decision point, see Open Questions).
- **DocumentProfile**: not added in v1. If a project-scoped feature later needs
  terms without re-running the pipeline, we add a field + `profile_version`
  bump per the profile contract.

### D4. Consumers converted in v1

Each conversion replaces a hardcoded literal with a term lookup, keeping the
current English value as the built-in default (which is what `_language.yml`
contains anyway):

1. **Callouts** — `callout_resolve.rs`: `callout-{note,tip,warning,important,caution}-title`
   replaces `capitalize()` for the five known types (unknown/custom callout
   types keep the capitalize fallback).
2. **Crossref + theorems** — seed `RefTypeRegistry::builtin()` display names
   from `crossref-<type>-title` at registry construction, so localized `kind`
   flows through `plain_data.kind` and both `crossref_render.rs` and
   `theorem.rs` inherit it. This also **de-duplicates** the theorem-name table
   (theorem.rs keeps only keyword→prefix mapping; display name comes from the
   registry). References use `crossref-<type>-prefix` (with title fallback),
   captions use `crossref-<type>-title` — matching Q1's split.
   `environment-proof-title` (+ solution/remark) covers `render_proof`.
3. **TOC** — `toc_generate.rs`: default from `toc-title-document`
   (`toc-title-website` reserved for the website "On this page" TOC when that
   distinction lands).
4. **Title block** — replace the three hardcoded labels in
   `FULL_HTML_TEMPLATE` with `$quarto.language.title-block-author-single$`
   (single/plural chosen by author count — needs a small context flag),
   `$…title-block-published$`, `$…section-title-abstract$`.
5. **Revealjs** — fix `assemble.rs:408` to emit the document's `lang` instead
   of hardcoded `"en"`.
6. **`<html lang>`** — already works; add a regression test.

Explicit **non-goals for v1** (each stays on its own strand / future phase):
CLI/log message localization (never a goal); LaTeX babel/polyglossia and Typst
`set text(lang:)` (no writers yet — but the design reserves nothing that would
block them: the writer just reads `lang` + terms); listing labels (bd-99ru
becomes a consumer of this table); search UI strings (client JS, needs a
JSON-blob channel like Q1's 2c — design when search lands); code-fold label
(feature not implemented); citeproc locales (separate CSL mechanism, bd-apudk);
`crossref.title-delim` and friends (Q1 crossref *options* are a separate
surface from language terms; tracked separately in crossref work).

### D5. Lua filter API (deferred — follow-up strand bd-xzaiqpjq)

Q1 exposes the table to Lua as `param("language")` plus per-key params. Q2
cannot do the equivalent yet: **Lua filters have no `pandoc.Meta` support
today** — Meta↔ConfigValue marshaling design is bd-2llqjsms, and doc-level
filter invocation (the filters that would receive Meta) is bd-a9g50za2.

This epic therefore only guarantees the *precondition*: `quarto.language` is
injected into `doc.ast.meta` before user filters run, so when Meta support
lands, the table is already in place. The exposure work itself —
verifying Meta visibility, choosing the blessed accessor, conformance tests
alongside the bd-grkrb9nj parity harnesses — is **bd-xzaiqpjq**
(discovered-from this epic, blocked by bd-2llqjsms and bd-a9g50za2) and is
*not* part of this epic's scope or closeout.

### D6. Schema & docs

- Add `lang` (string) and `language` (string-or-map) to the metadata schema
  surface so `q2` validation/completions know them.
- New user-facing docs page `docs/authoring/language.qmd` mirroring Q1's,
  rendered with `q2` itself (per repo policy).

## Test plan (TDD — written first, per phase)

Unit tests (phase 2, in `crates/quarto-core`, `tests/integration/language.rs`
registered in `main.rs` per the integration-test layout rule):

- [ ] Merge order: base → `pt` → `pt-BR` subtag walk (assert a key overridden
      at each level).
- [ ] `de-CH`, `fr-CA`, `sr-Latn`, `zh-TW` variant files resolve (real shipped
      files, not fixtures).
- [ ] User inline flat map overrides shipped translation.
- [ ] Per-language subkeys: `language: { fr: {…}, en: {…} }` — only the
      matching branch applies; `fr-CA` doc picks up `fr:` subkey.
- [ ] `language: custom.yml` file form; missing file is a hard error with a
      source-located diagnostic.
- [ ] Project-root `_language.yml` auto-detection.
- [ ] `crossref-*-prefix` falls back to `crossref-*-title`.
- [ ] Unknown key under `language:` warns but is preserved and reachable from
      a template.
- [ ] Every shipped `_language-*.yml` parses and only contains known keys /
      known patterns (catalog integrity test).

End-to-end render tests (phase 3-4, driving the real binary path per the
end-to-end verification policy; use languages where the term visibly differs
from English — `es` "Figura"/"Tabla", `de` "Abbildung", `fr` "Table des
matières"):

- [ ] `lang: es` → callout renders "Nota", crossref renders "Figura 1",
      caption "Figura 1: …", TOC title "Tabla de contenidos" (exact strings
      from `_language-es.yml`).
- [ ] `lang: pt-BR` → a key that differs between `pt` and `pt-BR` renders the
      `pt-BR` value.
- [ ] `language: { crossref-fig-title: "Figura" }` without `lang` → override
      applies on English base.
- [ ] Title-block labels localized (`title-block-published`).
- [ ] `$quarto.language.<key>$` resolves in a user-provided template
      (including a custom/unknown key).
- [ ] `<html lang="es">` emitted in HTML output; revealjs output no longer
      hardcodes `lang="en"`.
- [ ] `lang` set at project level (`_quarto.yml`) localizes a document with no
      front matter.

## Phases / work items

### Phase 0 — resources
- [x] Copy `_language*.yml` (35 files) + provenance README into
      `resources/language/`; embed via `include_dir!`.
- [x] Catalog integrity test (parses, known keys).

Phase 0 findings (2026-07-17):
- Upstream stray keys kept verbatim, allowlisted in the integrity test:
  `_language-lt.yml` `search`, `_language-sv.yml` `callout-danger-title`.
- `_language-sr-Latn.yml` has **no** `_language-sr.yml` parent — the subtag
  walk must tolerate missing intermediate layers (unit-tested in phase 2).
- quarto-yaml parses `key: ""` as Null (filed bd-gutochbq, upstream crate);
  `parse_term_file` reads Scalar(Null) as the empty string.

### Phase 1 — test skeletons
- [x] Write the unit-test suite above (failing / `#[ignore]`-staged as needed).
      (`tests/integration/language_resolve.rs`, 18 tests, verified failing
      against the missing API before implementation.)

### Phase 2 — resolution core
- [x] `crates/quarto-core/src/language.rs`: `LanguageTerms`, subtag walk,
      merge, file/inline/subkey handling, diagnostics.
- [x] Unit tests green. (21 language tests + full quarto-core suite, 2473
      passed.)

Phase 1-2 notes:
- `resolve_language(lang, extra_layers)` has no diagnostics param: warnings
  are emitted when layers are *built* (`structured_layer_from_config`,
  `parse_language_file`), where key source locations are at hand.
- Unknown-key warning includes a note that the key remains reachable as
  `$quarto.language.<key>$` (tested: warning carries the key's location).
- Custom files are strict (non-string term value = hard error);
  inline metadata is lenient (warn + skip the entry).

### Phase 3 — pipeline integration
- [x] `LanguageResolveStage` after `MetadataMergeStage`; inject
      `quarto.language` into `meta`; template namespace works.
- [x] Project-root auto-detection wired through project context.

Phase 3 notes:
- Stage inserted in three builders: `build_html_pipeline_stages_with_options`
  (covers native render + q2-preview), `build_wasm_html_pipeline`, and
  `build_analysis_pipeline`. Deliberately **not** in `get_config`'s pipeline
  (that surface shows user config, not derived state) nor the Pass-1 profile
  pipeline in `orchestrator.rs` (profile doesn't carry terms in v1).
- `language: <file>.yml` resolves against the document dir, then the project
  root; missing file = source-located **error** diagnostic, render continues
  on shipped terms.
- `LanguageTerms::from_meta(&meta)` is the transform-side accessor;
  round-trip is tested. 10 pipeline tests in
  `tests/integration/language_pipeline.rs`.

### Phase 4 — consumers
- [x] Callout titles (TDD: es/fr render tests first).
- [x] Crossref/theorem registry seeding + theorem-table dedupe.
- [x] TOC title.
- [x] Title-block template variables (author single/plural flag).
- [x] Revealjs `lang` fix.
- [ ] Full-workspace verify (`cargo xtask verify` — WASM leg affected:
      quarto-core changes). *(Deferred to session end, before push.)*

Phase 4 notes:
- 7 smoke-all fixtures in `crates/quarto/tests/smoke-all/localization/`
  (written first, all failing on exactly the localized strings, then green).
- Crossref: `RefTypeRegistry::localize_builtin_display_names` runs in
  `PreEngineSugaringStage` *before* `extend_from_metadata`, so
  `crossref.custom` display names win over locale defaults. Reference text
  additionally prefers `crossref-<type>-prefix` (Q1 prefix→title fallback)
  at render time; captions use the localized `kind`. Proof labels use
  `environment-proof-title`.
- Theorem sugar now takes its display name from the registry (localized),
  keeping `THEOREM_CLASSES`' English column only as the registry-less
  test fallback — the duplicate display-name table is effectively gone.
- Title block: `labels.{author,published,abstract}` computed in Rust
  (author single/plural by author count, Q1 `computeLabels` parity),
  inserted before the metadata walk so a user `labels:` key wins.
- End-to-end record (policy §3): `cargo run --bin q2 -- render completo.qmd`
  (es doc with toc/title-block/table+ref/theorem/proof/callout) — output
  inspected: `<html lang="es">`, "Tabla de contenidos", "Autor/a",
  "Fecha de publicación", "Resumen", "Tabla 1: Una tabla", "Teorema 1
  (Pitágoras)", "Demostración.", "Advertencia". Revealjs scaffold verified
  via `render_to_file` integration test (`lang="es"` / default `"en"`).
- Discovered (filed): bd-51k5yz4e — caption-form pipe tables
  (`: Caption {#tbl-N}`) never get a numbered caption prefix (pre-existing,
  language-independent; related bd-uwv2eec2).

### Phase 5 — docs & closeout
- [x] Docs page: `docs/guides/authoring/document-language.qmd` (rendered
      with q2; added to the site sidebar).
- [x] Schema entries for `lang`/`language`: **N/A for now** — q2 has no
      document-metadata schema/completion surface yet (no analogue of Q1's
      `definitions.yml`); when one lands, `lang` and `language` should be
      added to it.
- [x] Changelog: no hub-client/ source changes in this epic, so the
      hub-client changelog rule is not triggered.
- [ ] Full `cargo xtask verify` + strand closeout.
- (Lua visibility of `quarto.language` moved out of this epic → bd-xzaiqpjq,
  blocked on Meta support: bd-2llqjsms, bd-a9g50za2.)

Phase 5 notes:
- Visible side effect worth knowing: the default TOC title changed from
  q2's old hardcoded "Table of Contents" to the catalog's
  "Table of contents" (lowercase c) — this is Quarto 1's exact string, so
  it's a parity gain, but it shows up in every default-English document.

## Decisions (resolved with Carlos, 2026-07-17)

1. **Namespace**: claim only `quarto.language.<...>` in metadata, not the
   whole top-level `quarto.*`. Transport is meta injection; a `LanguageTerms`
   accessor wraps reads so transforms never do stringly `ConfigValue` walks.
2. **Ship all 34 Q1 language files from day one.**
3. **Author single/plural**: compute the chosen label string in Rust (Q1
   parity with `computeLabels` in authors.lua) and expose the resolved string
   to the template — no template-side count logic.
4. **Unknown `language:` keys**: emit a warning diagnostic, with q2's
   source-location infrastructure pointing at the offending key (improvement
   over Q1's silence). The key is still carried and template-reachable.
5. **TOC title precedence**: user `toc-title` metadata > language table
   (`toc-title-document`) > built-in default.
6. **Callout key duplication**: match Q1 exactly — `callout-note-title` (plain
   callouts) and `crossref-nte-title` (callout-as-crossref) stay independently
   resolvable keys. Deprecating/merging redundant options is a possible future
   pass, made safer by q2's source-mapped diagnostics.
7. **Lua exposure is out of scope for this epic** — no `pandoc.Meta` support
   in q2 Lua filters yet. Follow-up: bd-xzaiqpjq (blocked by bd-2llqjsms,
   bd-a9g50za2). This epic only ensures `quarto.language` is in `meta` before
   filters run.

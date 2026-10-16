# Plan F2: one metadata layer collector, shared by the merge stage and the resolver

Part of the epic [`2026-10-15-format-resolution-epic.md`](2026-10-15-format-resolution-epic.md) (FR14); the execution model and landing procedure are there.

## Execution

- **Lane:** B (`.worktrees/workspace-6`), binding: one agent per plan, in this order: F2, F3c, F4, F6, F5b, F8. This plan is number 1 of 6 in the lane.
- **Autonomy:** there is no orchestrator process. This plan's agent knows its lane, the plan before it and the plan after it from the Lane, Start gate and Next in lane lines of this section, develops it with no sub-agents, and runs at most one plan per session: finish, report, stop (the next plan is a new session with its own gate). The Handoff log is the only channel to later plans. Nothing here waits for Gordon: where a step used to ask, this plan states a default. If a start gate or a precondition fails, the agent reports exactly what is missing in its final message and stops; it does not ask mid-session.
- **Waits on:** nothing.
- **Start gate:** the planning files must be on the integration branch (this plan's own file is the check), and workspace-6 must have a clean tree (on 2026-10-15 it was pre-staged on the throwaway branch `format-resolution/lane-b-staging`, cut from the integration tip with `npm install` already run; anything but untracked build output means the previous work did not finish: stop and report it in the final message). `ready` is printed only if both hold:

  ```bash
  cd "$(git rev-parse --path-format=absolute --git-common-dir)/../.worktrees/workspace-6"
  miss=0
  for f in format-resolution-epic fr-f2-metadata-layers; do
    git cat-file -e "feature/q2-preview-hub-parity:claude-notes/plans/2026-10-15-$f.md" 2>/dev/null || { echo "missing: $f is not on feature/q2-preview-hub-parity yet; stop and report"; miss=1; }
  done
  [ -z "$(git status --short | grep -v '^?? q2-preview-spa/public/')" ] || { echo "missing: workspace-6 tree is not clean; stop and report"; git status --short; miss=1; }
  [ "$miss" = 0 ] && echo ready
  ```

- **Branch:** `format-resolution/f2-metadata-layers`, cut from the tip of `feature/q2-preview-hub-parity`:

  ```bash
  git switch -c format-resolution/f2-metadata-layers feature/q2-preview-hub-parity
  ```

  Rust only: no `npm install`.
- **Merges to:** `feature/q2-preview-hub-parity`: at the end, `git rebase feature/q2-preview-hub-parity`, conflict-marker sweep, `git -C ../workspace-7 merge --ff-only format-resolution/f2-metadata-layers`, then `git tag fr-landed/f2 format-resolution/f2-metadata-layers`.
- **Unlocks:** F3a (with F1).
- **Next in lane:** F3c (`fr-f3c-none-rename`), which waits on nothing but the same plan-file gate.

## Goal

The order, membership and provenance of the metadata layers is computed in one place. `MetadataMergeStage` uses it, with no change in behaviour; the resolver F3a adds reads only the `format` and `book-scope` keys from the same layers, so the early one-key phase and the late full merge cannot disagree. `output_ext` stops being a second, partial resolver. The document layer can be built from a content override (an editor buffer) by one front-matter helper.

## What exists (verified 2026-10-15; review corrections marked)

`MetadataMergeStage::run` (`stage/stages/metadata_merge.rs`) builds the layers inline, lowest to highest: extension `contributes.metadata` (`build_metadata_contribution_layers`, non-`project` keys), the project config (`ctx.project.config.metadata`), the extension format metadata (`build_extension_metadata_layer`, needs the full `target_format` and `is_book`), the `_metadata.yml` chain (`directory_metadata_for_document`, skipped when `ctx.project.is_single_file`), the document's front matter (`doc.ast.meta`), and the runtime metadata (`ctx.runtime.runtime_metadata()`). Each layer is flattened with `resolve_format_config(layer, base_format)` and path-adjusted/marked relative to the document directory before merging; the stage then deletes `format` and writes `ctx.format.target_format`.

The project layer already contains project profiles: `ProjectContext::parse_config` (`project/mod.rs`, with `project_profile.rs`) merges `_quarto-<name>.yml`, `_quarto.yml.local` and `profile.default`, keyed by `--profile`, `QUARTO_PROFILE`, `_environment[.local]`, so any code that reads the discovered `ProjectContext` sees them.

Review findings that shape this plan:

- Extensions are discovered per document in `StageContext::new` (`stage/context.rs:~419`), not at project level. The resolver must not discover them (FR14); the collector therefore takes the extensions as an **optional** input and the resolver passes none. An extension's `contributes.metadata` may carry `format:`, which the stage merges and the resolver ignores: a documented exception, to be tested.
- `directory_metadata_for_document(project, path, runtime)` (`project/mod.rs:135-185`) and `runtime.runtime_metadata()` need no `StageContext`, so the resolver can read them before any pipeline exists. The directory function returns empty for a single-file project and **errors on malformed YAML**; the resolver does project discovery and the `_metadata.yml` chain on every call, so only the document half of a per-keystroke call is cheap (F6 measures the wasm side and caches discovery).
- `json_to_config_value` (`metadata_merge.rs:45`) is private to the stage. It sets `SourceInfo::generated` and the default `merge_op`, loses YAML tags (`!path`, `!prefer`, `!concat`), and the runtime layer skips `mark_format_path_values` (`~:422-428`). It loses nothing the resolver needs (format keys including `null`/`false`, and `book-scope`, survive). The collector's runtime layer needs it, so it moves here.
- No helper turns front-matter text into a `ConfigValue`. The existing peeks disagree on edge cases: `format.rs:688` `extract_yaml_frontmatter` needs `---` at the very start and finds the first `\n---` or `\n...`; the wasm `detect_format_from_content` (`lib.rs:~775`) and `pandoc_request/formats.rs:~181` use `trim_start()` and only `\n---`. `format_key_from_config_value` (`format.rs:~703`) reads with `as_str()`, which the `metadata-as-str` lint rule forbids for metadata (use `as_plain_text()`). The document half of the new helper is a fifth front-matter parser (it uses the yaml-to-config path, not `serde_yaml`).
- Other places that read or merge metadata layers: `output_ext::resolve_output_ext(content, project.config.metadata, target_format)` (front matter and project only: it ignores `_metadata.yml` and the runtime layer; its header admits the gap). Its non-test callers are exactly three, as planned: `render_to_file.rs:~885` (`typst_output_ext`), `pandoc_request/formats.rs:~245` (`resolve_formats`, used by wasm) and `quarto-test/src/runner.rs:~637`; about ten unit tests live in `output_ext.rs`. Precedence today: the document's `format.<k>.output-ext`, then its top-level `output-ext`, then the project's merged value. Also `project/listing/config.rs` and `record.rs` (document-only merges for listings), `extension/read.rs`, `cell_options`. `get-config` prints the merged metadata through the stage. `FormatSource` (document/project/default) is on the wire only as a type in `wasm-quarto-hub-client.d.ts:~303` and Rust tests (`pandoc_request_exports.rs:~220-236`): **no TypeScript code consumes it**, so reporting directory keys as `document` is safe.

## Decisions

- **F2-D1 The collector.** A new module `quarto-core/src/metadata_layers.rs`:

  ```text
  enum LayerKind { ExtensionContribution, Project, ExtensionFormat, Directory, Document, Runtime }
  struct MetadataLayer { kind: LayerKind, source: Option<PathBuf>, config: ConfigValue /* unflattened */, base_dir: PathBuf }
  fn collect_metadata_layers(inputs: &LayerInputs) -> Vec<MetadataLayer>   // lowest to highest
  ```

  `LayerInputs` carries the project, the document path, the document layer (either the parsed front-matter `ConfigValue` or a content override string, F2-D3), `Option<&Extensions>` (`None` for the resolver; the stage passes them), `Option<&str>` target format (`None` skips the extension-format layer, the case for format selection), the runtime, and whether the project is a book. Layers are returned **unflattened**; flattening, path adjustment and css marking stay in the stage, which maps over the returned list. `LayerKind` is the provenance F3a needs for FR5, FR6 and FR10.
- **F2-D2 No behaviour change in the stage.** Same layers, same order, same flattened output; the stage's existing tests and the book and website smoke tests are the guard. The stage's inline comment block (precedence list) is replaced by a pointer to the collector. `json_to_config_value` moves into the collector's module (or a shared helper next to it) and the stage imports it.
- **F2-D3 The document layer from a content override.** One helper: `extract_yaml_frontmatter` (the `format.rs` one) → `quarto_yaml` parse → `pampa::pandoc::yaml_to_config_value`, the same path `directory_metadata_for_document` uses for `_metadata.yml`; it skips the markdown parse, so it is cheap enough for per-keystroke wasm calls. A corpus test asserts the helper agrees with the stage's `doc.ast.meta` on `format` and `book-scope` over a set of front-matter shapes (a leading BOM, **blank lines before the opening `---` (the helper requires `---` at byte 0; the wasm detector trims; the test pins whichever way is chosen and records it)**, `---`/`...` terminators including the `extract_yaml_frontmatter` quirk (`format.rs:688` matches the first `\n---` before it looks for `\n...`, regardless of position, so a `\n...` earlier in the block is ignored whenever any `\n---` follows; the test pins whichever way the helper is chosen to behave and records it), scalar and map `format`, `null`/`false` entries, an unparseable block). Values are read with `as_plain_text()`. This epic does not delete the other three front-matter parsers (F3a, F5a, F5b and F6 delete the ones they replace); the helper is also what F6 uses where a wasm entry needs only the document's own front-matter format keys (`parse_qmd_to_ast_with_attribution`).
- **F2-D4 `output_ext` moves onto the collector.** It reads `output-ext` from the merged layers (directory and runtime included); all callers and the unit tests above are updated to the new signature (project, runtime, path, format). Behaviour change: an `output-ext` set in `_metadata.yml` or the overlay is now honoured. `FormatSource` on the wire keeps its three values; for a key declared in a `_metadata.yml` the source is reported as `document` (a directory key is a document-level key, FR1), and the Handoff log says so.
- **F2-D5 What is *not* collected here:** the project config's own assembly (profiles, `.local`, extension `contributes.project`) stays in `ProjectContext::parse_config`; listing and cell-option merges are different concerns.

## Tasks

Per task: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus `cargo nextest run -p quarto-core`; `quarto` and `quarto-test` as well where a caller or CLI test is touched; `cargo check --lib --target wasm32-unknown-unknown` for the wasm crate once `pandoc_request/formats.rs` is touched (env in `dev-docs/wasm.md`); `cargo xtask lint` at the end.

- [x] 2.1 Baseline: look up the ledger entry for the tip you branched from; do not run the workspace (the global rule: it is recommended, never run unprompted); if there is no ledger entry for the tip, say so in the Handoff log and continue.
- [x] 2.2 Write `metadata_layers.rs` (F2-D1) by lifting the layer-building code out of the stage, keeping the helper functions where they are if they are reused, and moving `json_to_config_value`. Unit tests per layer kind, including: the extension-format layer absent when no target format is given; the extension layers absent when no extensions are passed; the `_metadata.yml` chain absent for a single-file project; deepest directory layer last; runtime layer last.
- [x] 2.3 Switch `MetadataMergeStage` to the collector (F2-D2). Add a test that asserts the stage's merged output is unchanged for a fixture with every layer kind present (compare against the output computed before the refactor, recorded as a snapshot in the test).
- [x] 2.4 The content-override document layer (F2-D3) with the agreement corpus test, and a test that an extension's `contributes.metadata` carrying `format:` is seen by the stage and not by the collector when called without extensions (the documented exception).
- [x] 2.5 `output_ext::resolve_output_ext` onto the collector (F2-D4) with its callers and unit tests; new tests: `output-ext` in front matter, in `_metadata.yml`, in the project, in the overlay (set it by a test runtime that returns runtime metadata), and the precedence between them.
- [x] 2.6 Layer-source tests (a probe key set in each layer, merged value and provenance checked) in `crates/quarto-core/tests/integration/metadata_layers.rs`: project; project profile overlay via `--profile`, via `QUARTO_PROFILE`, via `profile.default` in `_quarto.yml` and in `_quarto.yml.local`; nested `_metadata.yml` (two levels); front matter; runtime. Each asserts the winning layer's `LayerKind`. Reuse the setup in `tests/integration/project_profile_overlays.rs`. Do not call `set_var` for `QUARTO_PROFILE`: supply the profile through `_environment` or `profile.default` in-process (`project_profile.rs:30-53`), and spawn a binary with `.env("QUARTO_PROFILE", ..)` for the environment-variable case.
- [x] 2.7 Verify the two unverified sources and record the result in the Handoff log: (a) the book's `pre_render` computes "book-wide format defaults" (`orchestrator.rs`, near `run_book_single_file_merge`): do they enter the merge as a layer, a mutation of the project config, or another route? Round-4 reading found that the `pre_render` mutations are `number-sections` and `crossref.chapters` (`book/config.rs:463-474`, `project_type.rs:119`) plus `book.date` and the render-list restriction, none touching `format:` or `book-scope`; confirm it. (b) `seed_missing_book_title_metadata` (`project/book/project_type.rs`): same question. If either adds a layer the resolver would need to see, add it to the collector and a test; if not, say so.
- [x] 2.8 `cargo xtask lint`; record the `output-ext` behaviour change in the Handoff log for Gordon's changelog pass; plan end: nextest on quarto-core (and quarto, quarto-test); reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- The stage is the most-read code in the pipeline; the refactor must be byte-identical in effect. 2.3's snapshot is the guard; do not "tidy" ordering.
- The collector must accept a content override for the document layer without reading the disk copy (F3a, F5b and F6 rely on it).
- `pandoc_request/formats.rs` is compiled for wasm; its `output_ext` caller changes there, so run the wasm check.
- Interim state after F2 lands: behaviour-preserving for the merge stage and every caller except one visible change, `output-ext` set in a `_metadata.yml` or in the runtime layer is now honoured (F2-D4); nothing else in the CLI or the browser changes until F3a uses the collector. The Handoff log records the change for Gordon's changelog pass.

## Handoff log

**Headline.** `quarto_core::metadata_layers` is the one place that knows the metadata layers\' order, membership and provenance. `MetadataMergeStage` runs on it with no change in output; `output_ext::resolve_output_ext` reads the same layers. The one visible change: `output-ext` set in a `_metadata.yml` or in the runtime layer is now honoured (for Gordon's changelog pass).

**Baseline (2.1).** The ledger has no entry for this tip (latest is `Ledger: ae2887867: 16134 passed, 202 skipped`, parity epic Plan 13); no workspace run was made.

**The collector's final signature** (`crates/quarto-core/src/metadata_layers.rs`):

```text
enum LayerKind { ExtensionContribution, Project, ExtensionFormat, Directory, Document, Runtime }   // derives Ord: precedence order
struct MetadataLayer { kind, source: Option<PathBuf>, config: ConfigValue /* unflattened */, base_dir: PathBuf }
enum DocumentLayer<'a> { Parsed(&'a ConfigValue), Content(&'a str) }
struct LayerInputs<'a> { project, document_path, document_dir, document: DocumentLayer, extensions: Option<&[Extension]>,
                         target_format: Option<&str>, is_book, runtime: &dyn SystemRuntime }
fn collect_metadata_layers(&LayerInputs) -> Vec<MetadataLayer>          // lowest to highest
fn document_layer_from_content(&str) -> ConfigValue                      // front matter of source text; empty map if none/unparseable
pub(crate) fn json_to_config_value(&Value) -> ConfigValue                // moved from the stage
```

`document_path` feeds the `_metadata.yml` chain, `document_dir` is where document and runtime path values are relative to (the stage passes `doc.path`\'s parent, as before). `source` is the config file (project), each `_metadata.yml`, the extension dir (both extension kinds) or the document; `None` for the runtime layer. The project layer's profile overlays and `_quarto.yml.local` are already merged into the one `Project` layer, so provenance cannot tell them apart (the layer tests assert `Project` for all of them). The stage's per-kind flattening/path-adjusting/css-marking lives in `prepare_layer` (`metadata_merge.rs`), a straight lift of the old inline code.

**What F3a must know.**
- Call with `extensions: None, target_format: None` for selection; `is_book` only matters for the extension-format layer. A malformed `_metadata.yml` gives **no directory layers** (the stage has always swallowed the error); a caller that must report it calls `directory_metadata_for_document` itself.
- `DocumentLayer::Content` never reads the disk copy of the document. The document layer is built in the document-metadata interpretation (strings are `PandocInlines`): read with `as_plain_text()`, never `as_str()`.
- **Deviation from F2-D3:** the helper does not use `format::extract_yaml_frontmatter`. The agreement corpus showed the qmd reader (so the stage) ignores a `...` terminator, skips whitespace-only lines before the opening `---`, and does not take `----` for a terminator, all of which `extract_yaml_frontmatter` gets differently; the helper has its own `frontmatter_block` that follows the reader (BOM allowed; leading whitespace skipped; block ends at the first line that is exactly `---`, trailing whitespace allowed; `...` is not a terminator). Decision on leading whitespace: skip it (the reader does), so the helper and the stage agree. The corpus test covers a BOM, CRLF, blank and space-only lines before `---`, `...`, `----`, `---   `, `--- x`, scalar/map `format`, `null`/`false` entries, `book-scope`, and an unparseable block, all agreeing.
- **One known difference, pinned in a test:** the qmd reader also takes a `---` metadata block that comes *after* other content (Pandoc's rule) as metadata, so the stage sees `format:` from it; the helper reads only the block at the top (where Quarto 1 reads `format:` from). The other three front-matter parsers are untouched (F3a, F5a, F5b and F6 retire the ones they replace).
- The documented exception, tested: an extension's `contributes.metadata` carrying `format:` is a `ExtensionContribution` layer, seen by the stage and absent from a collector call without extensions.

**2.7 answers.** (a) The book's `pre_render` mutates `project.config.metadata` in place (so it is part of the `Project` layer, not a new layer): `book.*` copied to `website.*`, sidebar config, `number-sections` and `crossref.chapters` defaults (`config::apply_format_defaults`), special `book.date`, and the render-list restriction. None touches `format:` or `book-scope`. It runs before the stage, so the resolver sees the project layer with or without it identically for the keys it reads. (b) `seed_missing_book_title_metadata` (called only from `prepare_page_in_book`, typst chapter-alone) also mutates the project metadata in place, and only the book title/author/date keys. No collector change needed for either.

**`FormatSource`.** Unchanged (three values, Rust and `.d.ts` only; no TypeScript consumes it). A `format:` key declared in a `_metadata.yml` is a document-level key (FR1); F2 itself does not compute format sources (that is F3a/F6), so nothing reports directory keys as `document` yet; when F3a/F6 do, they report them as `document`.

**`output_ext` (F2-D4).** New API: `resolve_output_ext(DocumentLocation { project, runtime, path }, content, format_key)` and `resolve_document_output_ext(content, format_key)` (front matter only, for callers with no project). Precedence, highest first: runtime; document (`format.<key>.output-ext`, then top level); `_metadata.yml` chain deepest first; project. A non-string or empty value falls through. Callers updated: `render_to_file.rs` (`typst_output_ext` gained `runtime` and `input_path`), `pandoc_request/formats.rs` (`resolve_formats` now takes `Option<DocumentLocation>` instead of `Option<&ConfigValue>`; its `project_metadata` is derived from the location; `resolve_document_formats` passes one, the unit tests pass `None`), and `quarto-test/src/runner.rs` (discovers the project from the input; falls back to the document-only lookup if discovery fails). Wasm check passed (see below).

**Tests added.** 14 unit tests in `metadata_layers.rs` (every kind in order, absences, `project` key stripped, directory chain/single-file, runtime last, content override vs disk, extension-metadata `format:` exception, parse-stage agreement corpus, pinned differences) using a `MetaRuntime` test runtime (`metadata_layers/test_runtime.rs`, `NativeRuntime` plus fixed `runtime_metadata()`); 14 in `output_ext.rs` (the old ones moved onto real projects, plus `_metadata.yml`, nested precedence, runtime, and the document-only lookup); 9 in `tests/integration/metadata_layers.rs` (the merge-stage no-change guard, written first and **run against the pre-refactor stage and the new one, passing on both**, then a probe key in the project, each profile route (CLI selection, `profile.default` in `_quarto.yml` and in `_quarto.yml.local`, `_environment`), `_quarto.yml.local`, nested `_metadata.yml` and front matter, each asserting the winning `LayerKind`, source file and merged value). The `QUARTO_PROFILE` *environment variable* route is not duplicated: `get_config_env_var_selects_overlay` in the `quarto` crate's `project_profile_cli.rs` spawns the binary with it and goes through the stage, hence the collector. The runtime layer's provenance is covered by the unit tests (the integration binary has no runtime that returns metadata until F1's overlay).

**Gates.** `cargo clippy -p quarto-core -p quarto-test -p quarto --all-targets -- -D warnings` clean; `cargo xtask lint` clean; wasm check of `wasm-quarto-hub-client` passes (its 6 quarto-core warnings are pre-existing dead-code ones in files this plan did not touch); `cargo nextest run -p quarto-core -p quarto-test -p quarto --no-fail-fast`: 6821 run, 6821 passed, 34 skipped (a crate-scoped run, not a workspace ledger entry: no `Ledger:` line is written).

**Recommended before push** (Gordon's call, not run): `cargo nextest run --workspace` (about three minutes), or `nextest -p` on the direct dependents of `quarto-core` (`cargo tree -i quarto-core`); `quarto-lsp-core` and the wasm crates are the dependents this plan did not run.

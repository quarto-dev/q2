# bd-ohvl879u: cell-options facility + jupyter error policy

**Strand:** bd-ohvl879u (bug, P2, discovered-from bd-gthycd33, blocked-by bd-gthycd33)
**Branch:** `braid/bd-ohvl879u-jupyter-engine-ignores-error` (based on
`braid/bd-gthycd33-jupyter-engine-output-not`, i.e. PR #360 — merge that first)
**Status:** decisions 1–7 locked with Carlos (2026-07-02) — awaiting
explicit go-ahead before execution. Note decision 3 upgraded the design:
cell options resolve through a ConfigValue merge against the document's
already-merged metadata (scoped resolution), not a direct YAML read.

## Overview

Two deliverables, one mechanism:

1. **A shared cell-options facility** in `quarto-core`: identify and extract
   the `#|`-style YAML options block at the head of an executable code cell,
   language-aware (each language writes options in its own comment syntax:
   `#|` for python/R, `--|` for lua/sql, `//|` for js/rust, `%|` for matlab,
   block-comment forms like `/*| … */` for C), parsed with `quarto-yaml`,
   **with source locations that map back through `quarto-source-map` to the
   enclosing text**.
2. **The jupyter engine consumes it** to fix the error-policy divergence:
   today q2's jupyter unconditionally embeds a failing cell's error as
   `.cell-output-error` output and reports success, while knitr fails the
   whole render (Q1's default `execute.error: false` policy). After this
   strand: an un-annotated cell error fails the render for jupyter too;
   `#| error: true` opts a cell into embedded error output.

## What the source study found

### Quarto 1 (external-sources/quarto-cli) — the reference

- **Canonical implementation:** `src/core/lib/partition-cell-options.ts`.
  - `kLangCommentChars: Record<string, string | [string, string]>` — the
    registry (~50 languages). Value is a line-comment prefix (`"#"`, `"//"`,
    `"--"`, `"%"`, `"!"`, `"⍝"`, …) or a `[prefix, suffix]` pair for
    block-comment-only languages (`c`/`css`: `["/*","*/"]`, `ocaml`:
    `["(*","*)"]`, `sas`: `["*",";"]`). Unknown language ⇒ `"#"`.
  - Option-line pattern: `^<escaped prefix>\s*\| ?` — comment chars,
    optional whitespace, a literal `|`, one optional space. With a suffix,
    the line must also end with the suffix (stripped along with trailing
    whitespace).
  - Partition: scan **only the leading run** of matching lines; the first
    non-matching line ends the options block. Everything below is the code.
  - `partitionCellOptionsText` keeps per-line ranges so the reassembled
    YAML is a `MappedString` into the original source — parse errors point
    at real file positions. (This MappedString machinery is exactly what we
    replicate with `quarto-source-map`.)
  - `addLanguageComment` — registration hook used by language handlers
    (relevant someday for user-extensible engines; not v1).
  - `guessChunkOptionsFormat` (guess-chunk-options-format.ts): for `r`
    cells only, sniffs knitr's `key=value, key2=value` comma syntax vs YAML
    and skips YAML parsing for the former.
- Q1 has **several other copies** of this logic (jupyter.ts, percent.ts,
  the Python-side `nb_cell_yaml_options` in resources/jupyter/notebook.py,
  filters/modules/constants.lua) — precisely the duplication we should not
  reproduce. q2 gets **one** implementation.
- **Error semantics** (resources/jupyter/notebook.py `cell_execute`,
  ~L550): per-cell options are read before execution; `error: true` adds
  the `raises-exception` tag (execution continues, error is embedded);
  otherwise a raising cell aborts the run. Global default comes from
  document `execute.error` (default false). Q1 also **strips the option
  lines from the source sent to the kernel** (`nb_strip_yaml_options`) so
  cell magics work, and the echoed source excludes them (`sourceStartLine`).

### q2 today — four ad-hoc `#|` sites, no shared facility

(Full survey in session transcript, 2026-07-02.)

- `quarto-core/src/crossref/codeblock_shorthand.rs` — the only site that
  *extracts* values: hardcoded `"#|"`, naive `split_once(':')` (no real
  YAML), `HashMap<String,String>`, no per-option source spans. Consumed by
  the `pre-engine-sugaring` stage.
- `quarto-lsp-core/src/tokens.rs` (`leading_directive_byte_len`,
  `directive_tokens`) — the **best technical precedent**: detects the
  leading `#|` run, strips markers, reassembles one YAML document, and
  keeps a `DirectiveLine { virt_start, doc_start, len }` table mapping
  reassembled-YAML offsets back to document offsets. But it's
  highlight-only (tree-sitter YAML highlighter, no structural parse) and
  hardcodes `"#|"`.
- `engine/jupyter/text_execute.rs` — sends and echoes `#|` lines verbatim;
  no parsing (this strand's consumer).
- `engine/knitr` — delegates `#|` handling to knitr itself (stays that
  way).
- The tree-sitter qmd grammar has **no** node type for option lines; all
  `#|` structure is imposed post-parse. (No grammar changes in this plan.)
- `quarto-lsp-core` already depends on `quarto-core`, so a quarto-core
  facility is reusable by the LSP later.

### quarto-yaml + quarto-source-map — the APIs we compose

- `quarto_yaml::parse_with_parent(content, parent: SourceInfo)` parses a
  fragment; every node gets `SourceInfo::substring(parent, start, end)`
  with fragment-relative offsets — positions compose through the parent
  chain. Values are `YamlWithSourceInfo` (`yaml: yaml_rust2::Yaml` +
  per-node `source_info`, `YamlHashEntry` with key/value/entry spans;
  `get_hash_value("error")`, `yaml.as_bool()` for reads).
- `SourceInfo::concat(pieces)` expresses "this string is a concatenation
  of regions of another source" — the `SourceInfo`-native form of the
  LSP's `DirectiveLine` table, and the right representation for the
  reassembled YAML (option lines are non-contiguous: the `#| ` markers and
  the joining structure are elided). `map_offset` walks Concat → Substring
  → Original automatically. Precedent for the substring pattern:
  `pampa/src/pandoc/meta.rs:334-390` (frontmatter extraction).
- Caveat to design around: synthetic join positions (offsets falling on a
  seam between Concat pieces) don't map; real YAML node spans land inside
  pieces, so this is fine in practice — the seams are the newlines we
  insert. (One per-line subtlety: include each source line's own `\n` as
  part of its piece when possible, so seams are minimized.)
- **Engine-input caveat (important):** the jupyter engine is text-in/
  text-out; its input is the *serialized post-include* QMD (`input_qmd`),
  not the user's original file, and no `SourceInfo` accompanies it through
  `ExecutionEngine::execute`. See decision D4 for what error locations
  mean in v1.
- (Recorded for the future, not this strand: `CodeBlock.source_info` spans
  the whole fenced block *including fences*, and there is no body-only
  `SourceInfo`, so AST-side consumers like the crossref shorthand can't
  yet map `cb.text` offsets faithfully. Follow-up strand, see Phase 6.)

## Design

### New module: `crates/quarto-core/src/cell_options/`

```rust
/// Comment syntax for a language's cell-option lines.
pub struct CommentSyntax {
    pub prefix: &'static str,          // "#", "//", "--", "%", "!", "⍝", "/*", …
    pub suffix: Option<&'static str>,  // Some("*/") for c/css, Some("*)") ocaml, Some(";") sas
}

/// Q1's kLangCommentChars, ported verbatim. Unknown language ⇒ "#".
pub fn comment_syntax_for(language: &str) -> CommentSyntax;

/// One extracted option line (marker stripped), with its provenance.
struct OptionLine { /* content byte-range within the cell body, … */ }

/// Result of partitioning a cell body.
pub struct PartitionedCell {
    /// Parsed options; None when the cell has no option lines.
    pub options: Option<YamlWithSourceInfo>,
    /// The cell body with the option lines removed (what runs / echoes).
    pub code: String,
    /// SourceInfo for `code` (Concat of the retained regions of `body_source`).
    pub code_source: SourceInfo,
    /// Byte length of the option block within the original body (0 if none).
    pub options_len: usize,
}

/// Identify the leading option-line run of `body` (per `language`'s
/// comment syntax), reassemble the YAML with a `SourceInfo::concat` of
/// per-line substrings of `body_source`, parse via
/// `quarto_yaml::parse_with_parent`, and partition body into
/// options/code. Returns Err (with the quarto-yaml diagnostic, which
/// carries a mapped location) when the option lines are not valid YAML.
pub fn partition_cell_options(
    language: &str,
    body: &str,
    body_source: SourceInfo,
) -> Result<PartitionedCell, CellOptionsError>;
```

Notes:
- The option-line matcher mirrors Q1: `^<prefix>\s*\| ?`, leading-run
  only, suffix-checked for block-comment languages. Indentation before the
  prefix is *not* allowed (Q1 anchors at `^`) — pinned by a test.
- The caller supplies `body_source`; the facility never guesses
  provenance. Callers with no better anchor pass
  `SourceInfo::original(ephemeral_file_id, 0, body.len())` over a
  registered in-memory file (what text_execute will do, D4).
- `YamlWithSourceInfo` is the return type — consumers read
  `options.get_hash_value("error").and_then(|v| v.yaml.as_bool())`, and
  every node span maps back through the Concat automatically.
- **Not in v1:** `guessChunkOptionsFormat` (knitr comma-syntax sniffing) —
  the knitr engine keeps handling its own options, and q2 already rejects
  old-style options at parse time (Q-2-36); if/when a shared consumer
  needs r-cell tolerance we port the guesser (Phase 6 note). Also not in
  v1: an `addLanguageComment`-style runtime registration hook (no
  user-extensible engines yet); the registry is a static table with a
  documented extension point.

### Jupyter engine changes (`engine/jupyter/text_execute.rs`)

In `execute_blocks_inner`, per cell:

1. Register the engine input string once as an ephemeral file in a local
   `SourceContext` (name like `<doc-stem>.engine-input.qmd`); build
   `body_source = SourceInfo::original(file_id, block.code_start, block.code_end)`
   (the regex already yields the byte offsets; we extend `CodeBlock` —
   the module-private struct — with the *body* range, which the existing
   capture groups give us).
2. `partition_cell_options(&block.language, &block.code, body_source)`.
   - Malformed options YAML ⇒ `ExecutionError` with the located
     diagnostic (D6).
3. Read `error: true` (absent/false ⇒ errors disallowed — Q1 default).
4. Send **only `partitioned.code`** to the kernel (Q1 strips option lines
   so cell magics work — D5).
5. Echo **only `partitioned.code`** in the `.cell-code` fence (knitr
   parity — verify knitr's echoed fence for a `#| error: true` cell
   during Phase 1 and pin it in the parity suite if content-checkable).
6. On `CellOutput::Error` / `ExecuteStatus::Error`:
   - `error: true` ⇒ current behavior (embed `.cell-output-error` div,
     continue to next cell).
   - otherwise ⇒ **abort**: return `ExecutionError::execution_failed`
     with a diagnostic naming the cell (language, index, first code line)
     + the kernel's `ename: evalue`, and the D4-scoped location. No
     further cells run (matches knitr/Q1: the render fails).

Downstream effects (already in place from bd-gthycd33's work): a failing
`record_capture` marks the hub sidecar `CaptureState::Error` via the
provider, and `q2 render`/`q2 preview` surface pipeline errors — so no
provider/hub-client changes are expected in this strand.

## Decisions (locked 2026-07-02 with Carlos)

1. **Facility home:** `crates/quarto-core/src/cell_options/` (module,
   sibling of `crossref/`); engine consumes now, crossref-shorthand and
   quarto-lsp-core in follow-ups. No new crate for v1.
2. **Scope: `error` only.** `eval` is a follow-up (the facility makes it
   small).
3. **Document-level default via ConfigValue merge — yes, and it's the
   point.** Rather than reading `error` straight off the
   `YamlWithSourceInfo`, convert the cell options to `ConfigValue` and
   interpret **the result of merging against the document's metadata** —
   which at engine time already includes `_quarto.yml` / `_metadata.yml` /
   front-matter merging from Q2's ConfigValue management. Cell-level
   options then naturally respect document-level options
   (`execute: error: true` becomes the default; per-cell `error:` wins).
   Worth the setup cost even so: this starts the infrastructure for
   **scoped resolution of document metadata in cell options**, a
   longstanding Q1 limitation. See "Scoped option resolution" below.
4. **Error-location fidelity: accepted for v1**, with the framing
   corrected — see "Note: how source locations work here" below. Not a
   problem to be fixed, just a mechanism to document.
5. **Strip option lines from kernel input and echo:** yes to both.
6. **Malformed option YAML:** hard error with mapped location.
7. **Crossref-shorthand migration:** follow-up strand.

## Scoped option resolution via ConfigValue (decision 3 design)

The pieces all exist:

- `pampa::pandoc::meta::yaml_to_config_value(YamlWithSourceInfo,
  InterpretationContext, &mut DiagnosticCollector) -> ConfigValue`
  (meta.rs:139) — the unified, source-info-preserving YAML→ConfigValue
  converter (quarto-core already depends on pampa).
- `quarto_config::materialize::merge_with_diagnostics` — the merge
  machinery the rest of Q2's config management uses.
- The engine input's front matter **is the merged document metadata**:
  `MetadataMergeStage` runs before `EngineExecutionStage`, and the
  serialized `input_qmd` front matter carries the result (observed
  empirically during bd-gthycd33: the captured input contained merged
  `title`/`format`/`listing-item`/… keys).

Resolution flow in the engine (text path, self-contained — no engine-API
change):

1. Extract the input's `---` front-matter block (same
   `extract_between_delimiters` pattern as `pampa/src/pandoc/meta.rs`'s
   `rawblock_to_config_value`), parse with
   `quarto_yaml::parse_with_parent`, convert via `yaml_to_config_value`.
   Read the `execute` map from it.
2. Partition each cell; convert its options `YamlWithSourceInfo` to
   `ConfigValue` the same way.
3. **Merge**: cell options over the document's `execute` scope
   (`merge_with_diagnostics`), then read `error` from the merged value.
   Per-cell `error:` overrides document `execute: error:`; absent both ⇒
   `false` (Q1 default).

Implementation questions to settle in Phase 1 (small, not blocking the
plan): which `InterpretationContext` for cell options (recommend
`DocumentMetadata`, matching front matter — cell options include
markdown-bearing values like `fig-cap`); the exact boolean read on the
merged `ConfigValue` (mind the `metadata-as-str` lint — use the proper
bool accessor); and whether the doc-level `execute` extraction should be
memoized once per document rather than per cell (yes, trivially).

The facility itself stays config-agnostic (returns `YamlWithSourceInfo`);
a thin `cell_options_config()` helper (or engine-local code, whichever
reads better) does the ConfigValue conversion + merge. That keeps the
partition mechanism reusable by consumers that don't want config
semantics (LSP highlighting).

## Note: how source locations work here (decision 4)

The engine input is the serialized post-include QMD. Its cell-body spans
are registered as an ephemeral source file so diagnostics render
row/col against exactly the text the engine executed. This is consistent
with how the rest of the pipeline already treats that serialized text:
downstream, the same serialized QMD (as `capture.input_qmd`) is re-parsed
and **reconciled at the AST level against the live pipeline AST**
(`quarto-ast-reconcile`; the capture splice from bd-gthycd33 is one such
consumer), and portions of the AST that don't change retain their
original source locations through that reconciliation. So locating
engine-side errors in the serialized text is not a divergence from the
architecture — it's the same coordinate system the pipeline already
manages, and the existing multi-file error infrastructure
(`SourceContext` with ephemeral files) renders it correctly. v1 proceeds
on this basis; if we later want engine diagnostics re-expressed in
original-file coordinates, that's a reconciliation/mapping pass over an
unchanged mechanism, not a redesign.

## Work items

### Phase 1 — investigation + tests first (TDD)

- [x] Decision-3 implementation questions settled (2026-07-02, probes in
      the bd-gthycd33 worktree, deleted after):
      * `_quarto.yml` `execute: error: true` **does** appear in the
        capture's `input_qmd` front matter (observed directly) — the
        engine sees fully merged metadata.
      * `InterpretationContext::DocumentMetadata` for cell options
        (booleans stay `Yaml::Boolean` in any context; markdown-bearing
        string values match front-matter semantics).
      * Bool read: `merged.get("error").and_then(|v| v.as_bool())` —
        `ConfigValue::as_bool` (config_value.rs:652) matches
        `Scalar(Yaml::Boolean)` only, which is what quarto-yaml produces
        for `true`/`false`; not metadata-as-str-lint territory.
      * Merge: follow the in-tree precedent
        (`build_extension_metadata_layer`, metadata_merge.rs:109) —
        `MergedConfig::new(vec![&lower, &higher])` + `.materialize()`;
        **later layer wins**. (`merge_with_diagnostics` is a validating
        wrapper around the same; use it for the diagnostics.)
- [x] knitr's echoed `.cell-code` for a `#| error: true` cell contains
      **only** `stop("boom")` — the directive line is stripped, and the
      output is an embedded `::: {.cell-output .cell-output-error}` div
      (observed directly). Jupyter must strip too; no discrepancy-log
      entry needed.
- [x] Unit tests, registry (4 tests in `cell_options/mod.rs`): line-comment
      languages incl. `⍝`, block-comment suffix languages, unknown → `#`,
      case-insensitive lookup.
- [x] Unit tests, partition (12 tests): leading-run detection; no-options;
      options-only; blank `#|` run ⇒ no options but still consumed;
      marker spacing variants; indented marker rejected; block-scalar
      reassembly; lua `--|` / js `//|` / c `/*|…*/` markers; wrong-language
      marker rejected; suffix-less block-comment line rejected; malformed
      YAML ⇒ `CellOptionsError::InvalidYaml`.
- [x] Unit tests, source mapping (3 tests): `true` node maps to the byte of
      `t` in the body (via `SourceContext` + `map_offset`, following
      attribution_chain_resolution.rs); second-line option value maps;
      `code_source` maps to the first code byte.
- [x] Engine decision logic is covered by the scoped-resolution unit tests
      (the allow/deny ladder IS the merge) + the kernel-gated integration
      assertions (kernel input / echo stripping observable only through a
      real run — see below).
- [x] Unit tests, scoped resolution (6 tests): the 4-case matrix + both
      override directions + non-`error` keys don't grant + scope keys
      survive the merge. **All 6 pass immediately** — `options_to_config`
      (pampa `yaml_to_config_value`) and `merge_cell_over_scope`
      (`MergedConfig::new` + `materialize`, later-layer-wins) are thin
      compositions of existing infrastructure, which validates the
      decision-3 design end to end before any engine code exists.
- [x] Kernel-gated integration tests: `engine_error_policy.rs` (7 tests:
      plain-error-fails, error-true-embeds-and-strips, doc-level allow,
      cell-false-overrides-doc-true, failing-cell-stops-subsequent-cells,
      malformed-options-fail, healthy-cells-unaffected) +
      `parity_error_policy_behavior` in engine_output_parity.rs (both
      engines must fail for un-annotated error). Also updated the stale
      `parity_error_output` doc comment.
- [x] Run everything; reds confirmed 2026-07-02: 15 registry/partition
      unit tests fail on `todo!()` stubs; 6 scoped tests pass (see above);
      7 integration tests fail for the expected reasons; 2 pass as
      expected (`parity_error_output` shape — unchanged behavior — and
      `document_execute_error_true_allows…`, vacuously green today since
      jupyter never aborts; it becomes the over-aborting guard once the
      policy lands).

### Phase 2 — implement the facility

- [x] `crates/quarto-core/src/cell_options/mod.rs`: registry (Q1 table
      ported, provenance comment, case-insensitive, unknown → `#`),
      `option_content_ranges` matcher (prefix + ws + `|` + one optional
      space, column-0 anchored; suffix languages require + elide the
      terminator, their newline carried as its own piece),
      `partition_cell_options` with `SourceInfo::concat` of per-line
      substrings feeding `quarto_yaml::parse_with_parent`, plus
      `options_to_config` / `merge_cell_over_scope` (decision-3 helpers)
      and `CellOptionsError::location()`. Design win over the plan: for
      prefix-only languages the content ranges run *through each line's
      newline*, so every byte of the reassembled YAML is a real source
      byte — no synthetic seams at all.
- [x] Module docs: contract + provenance + scoped-resolution rationale.
- [x] All 28 unit tests green after implementation (registry 4,
      partition 12, mapping 3, scoped 6, + error-path 3). ✅ 2026-07-02.

### Phase 3 — wire the jupyter engine

- [x] text_execute.rs rewired: per-cell `partition_cell_options`; kernel
      input and echoed `.cell-code` fence use the **partitioned** code
      (`render_cell`/`echoed_source_fence` now take `(language, code)`);
      `document_execute_scope` extracts the front matter's `execute` map
      once per document (parsed with a `Substring` parent over
      `ctx.source_info`); `resolve_allow_errors` = decision-3 merge;
      abort-on-disallowed-error via new `JupyterError::CellExecutionFailed`
      (halts before any later cell runs); malformed options via new
      `JupyterError::InvalidCellOptions`. **Location fidelity came out
      better than planned**: `ExecutionContext` already carries
      `source_info` (a `Concat` from `write_with_source_info` mapping
      engine-input offsets back to the ORIGINAL files, through includes)
      plus a shared `SourceContext` — so diagnostics render
      `path:line:col` in original-file coordinates via `describe_location`,
      and the plan's ephemeral-file fallback was never needed. (This
      confirms the decision-4 note's framing: the infrastructure was
      already there.)
- [x] All Phase 1 tests green with real kernels ✅ 2026-07-02: 48 lib
      tests (cell_options + text_execute), 15/15 engine integration tests
      (7 error-policy + behavior-parity + all pre-existing fences —
      splice pair, 5 shape-parity cases — unchanged).

### Phase 4 — regression sweep

- [x] `cargo nextest run --workspace` ✅ **10217/10217 passed** (run twice:
      before and after the clippy shape fixes; identical results,
      2026-07-02). `cargo clippy -p quarto-core --all-targets` 0 warnings;
      `cargo xtask lint` clean; `cargo fmt --check` clean.
- [x] `cargo xtask verify` (full, WASM leg) ✅ "All verification steps
      passed!" (2026-07-02, fresh-worktree npm install + cold WASM build).
- [x] Existing parity suite + splice pair still green (all 8 bd-gthycd33
      fences pass unchanged; non-error-cell emission byte-identical).

### Phase 5 — end-to-end (per CLAUDE.md)

- [x] CLI e2e through the real binary ✅ 2026-07-02 (worktree-built
      `./target/debug/q2`, scratch fixtures; all outputs inspected):
      * `q2 render fail.qmd` (plain `raise Exception("boom-e2e")`) ⇒
        **exit 1** with:
        `Error: Execution failed in jupyter: code cell at
        …/fail.qmd:9:2 raised Exception: boom-e2e` + the
        `Use `#| error: true` …` hint. Line 9 is exactly the `raise`
        line **in the user's original file** — the `ExecutionContext`
        source-map chain resolves through the serialized engine input
        as designed (decision-4 note vindicated).
      * `q2 render allowed.qmd` (`#| error: true`) ⇒ renders;
        `allowed.html` contains `class="cell-output cell-output-error"`
        and **zero** `#|` occurrences (directive stripped from echo).
      * `q2 render doc-allowed.qmd` (front-matter `execute: error: true`,
        un-annotated failing cell) ⇒ renders;
        `<div class="cell-output cell-output-error">…<code>Exception:
        boom-doc-allowed` present (decision-3 path end to end).
- [x] `q2 preview` spot-check ✅ (2026-07-02, after
      `cargo xtask build-q2-preview-spa` + `cargo build --bin q2`):
      healthy jupyter doc with a `#| echo: true` directive, inspected the
      preview iframe DOM in a real Chrome tab — 1 `div.cell`, echoed
      `.cell-code` text is exactly `2 + 3` (directive stripped through
      the capture path too), output `5` spliced, no `#|` anywhere in the
      rendered body.

### Phase 6 — close out

- [ ] Update this plan with evidence; `braid close bd-ohvl879u`.
- [x] Follow-up strands filed (all `discovered-from: bd-ohvl879u`):
      **bd-eizgnxlx** crossref-shorthand migration (D7);
      **bd-1gty7f7o** LSP `directive_tokens` reuse;
      **bd-2xkpy5ra** body-only `SourceInfo` on `CodeBlock`;
      **bd-moef1ec4** `eval` option (D2);
      **bd-2lc8qu6e** port `guessChunkOptionsFormat` when a shared
      consumer handles r cells.
      The planned "re-express engine diagnostics in original-file
      coordinates" follow-up was NOT filed — it turned out to already
      work: `ExecutionContext.source_info` maps engine-input offsets to
      original files, and the e2e diagnostic pointed at `fail.qmd:9:2`
      out of the box.

## References

- Strand: bd-ohvl879u; parent context bd-gthycd33 (+ its plan,
  `claude-notes/plans/2026-07-01-bd-gthycd33-jupyter-cell-wrapper.md`).
- Q1 canonical partition: `external-sources/quarto-cli/src/core/lib/partition-cell-options.ts`
  (registry at L310; `optionCommentPattern` L294); error semantics:
  `external-sources/quarto-cli/src/resources/jupyter/notebook.py`
  (`cell_execute`, ~L550); knitr-format sniffing:
  `src/core/lib/guess-chunk-options-format.ts`.
- q2 precedents: `crates/quarto-lsp-core/src/tokens.rs` (run detection +
  offset table), `crates/pampa/src/pandoc/meta.rs:334-390`
  (`parse_with_parent` + `SourceInfo::substring` pattern),
  `crates/quarto-core/tests/integration/attribution_chain_resolution.rs`
  (mapping assertions).
- API surveys (quarto-yaml entry points, SourceInfo variants/builders,
  survey of all `#|` sites): session transcript 2026-07-02.
